use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use crate::api::Client;

fn ensure_size_provider(w: i32, h: i32) {
    static CACHE: Mutex<Option<HashMap<(i32, i32), ()>>> = Mutex::new(None);
    let mut guard = CACHE.lock().unwrap();
    let map = guard.get_or_insert_with(HashMap::new);
    if map.contains_key(&(w, h)) {
        return;
    }

    let css = gtk::CssProvider::new();
    // Not: GTK CSS'te width/height/max-* geçersizdir (parser uyarısı verir);
    // yalnızca min-* yazılır, gerçek ölçü set_size_request ile tutulur.
    css.load_from_string(&format!(
        ".cover-fixed-{w}-{h} {{ min-width:{w}px; min-height:{h}px; }}"
    ));

    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }

    map.insert((w, h), ());
}

pub fn new_sized_picture(w: i32, h: i32) -> gtk::Picture {
    let pic = gtk::Picture::new();
    pic.set_width_request(w);
    pic.set_height_request(h);
    pic.set_hexpand(false);
    pic.set_vexpand(false);
    pic.set_can_shrink(true);
    pic.set_content_fit(gtk::ContentFit::Cover);

    ensure_size_provider(w, h);
    let fixed_class = format!("cover-fixed-{w}-{h}");
    if h >= w {
        let size_class = match w {
            0..=60 => "cover-thumb",
            61..=130 => "cover-header",
            131..=150 => "cover-shelf",
            _ => "cover-movie-header",
        };
        pic.set_css_classes(&["cover", size_class, &fixed_class]);
    } else {
        pic.set_css_classes(&["cover", &fixed_class]);
    }
    pic
}

    /// Ana iş parçacığına taşınabilecek sıkı piksel tamponu.
///
/// `gdk_pixbuf::Pixbuf` Rust tarafında `Send` değil; bu yüzden iş
/// parçacığında dokuyu değil düz baytları gönderiyoruz. Ana döngüde
/// yeniden Pixbuf kurulup dokuya yükleniyor.
struct ScaledCover {
    w: i32,
    h: i32,
    nch: i32,
    stride: i32,
    data: Vec<u8>,
}

pub struct CoverManager {
    client: Arc<Client>,
    cache: Rc<RefCell<HashMap<String, Option<gtk::gdk::Texture>>>>,
    cache_order: Rc<RefCell<VecDeque<String>>>,
    waiters: Rc<RefCell<HashMap<String, Vec<gtk::Picture>>>>,
    queue: Arc<Mutex<VecDeque<String>>>,
    active: Rc<Cell<usize>>,
    /// Okuma dilimi dokuları (büyük; ayrı ve sınırlı önbellek).
    slice_cache: Rc<RefCell<HashMap<String, Option<gtk::gdk::Texture>>>>,
}

impl CoverManager {
    pub fn new(client: Arc<Client>) -> Self {
        Self {
            client,
            cache: Rc::new(RefCell::new(HashMap::new())),
            cache_order: Rc::new(RefCell::new(VecDeque::new())),
            waiters: Rc::new(RefCell::new(HashMap::new())),
            queue: Arc::new(Mutex::new(VecDeque::new())),
            active: Rc::new(Cell::new(0)),
            slice_cache: Rc::new(RefCell::new(HashMap::new())),
        }
    }

    pub fn clone_ref(&self) -> Self {
        Self {
            client: self.client.clone(),
            cache: self.cache.clone(),
            cache_order: self.cache_order.clone(),
            waiters: self.waiters.clone(),
            queue: self.queue.clone(),
            active: self.active.clone(),
            slice_cache: self.slice_cache.clone(),
        }
    }

    pub fn cover_picture(&self, url: Option<&str>, w: i32, h: i32) -> gtk::Picture {
        let pic = new_sized_picture(w, h);
        self.load_cover(url, &pic, w, h);
        pic
    }

    /// Geniş kartlar (bölüm ızgarası) için yüksek çözünürlüklü kapak. TMDB
    /// görselini w500'den çekip dokuyu düzen boyutunun 2 katı (w*2, h*2)
    /// olarak pişiririz: akış kutusu kartı hücreye gerdiğinde (~240→~300px)
    /// bile kaynak yukarı ölçeklenmez, keskin kalır. Widget'ın doğal boyu
    /// yine set_size_request ile (w,h)'te sabit — sütun sayısı şişmez.
    pub fn cover_picture_hd(&self, url: Option<&str>, w: i32, h: i32) -> gtk::Picture {
        let pic = new_sized_picture(w, h);
        if let Some(url) = url {
            let big = url
                .replace("image.tmdb.org/t/p/original", "image.tmdb.org/t/p/w500")
                .replace("image.tmdb.org/t/p/w342", "image.tmdb.org/t/p/w500")
                .replace("image.tmdb.org/t/p/w185", "image.tmdb.org/t/p/w500");
            self.load_cover_impl(&big, &pic, w * 2, h * 2);
        }
        pic
    }

    pub fn thumb_url(url: &str, hd: bool) -> String {
        let t = if hd { "w342" } else { "w185" };
        url.replace("image.tmdb.org/t/p/original", &format!("image.tmdb.org/t/p/{t}"))
            .replace("image.tmdb.org/t/p/w500", &format!("image.tmdb.org/t/p/{t}"))
            .replace("image.tmdb.org/t/p/w342", &format!("image.tmdb.org/t/p/{t}"))
            .replace("image.tmdb.org/t/p/w185", &format!("image.tmdb.org/t/p/{t}"))
    }

    pub fn load_cover(&self, url: Option<&str>, pic: &gtk::Picture, w: i32, h: i32) {
        let Some(url) = url else { return };
        self.load_cover_impl(&Self::thumb_url(&url, false), pic, w, h);
    }

    /// Okuma dilimi önbelleği. Kapak önbelleğinden ayrı tutuluyor: bir
    /// dilim 900x1500 (~5 MB), kapaklar 140x270 (~150 KB). Karıştırılırsa
    /// onlarca sayfa kapak önbelleğini doldurup belleği şişirir.
    pub fn slice_cache_get(&self, key: &str) -> Option<Option<gtk::gdk::Texture>> {
        self.slice_cache.borrow().get(key).cloned()
    }

    pub fn slice_cache_put(&self, key: &str, t: gtk::gdk::Texture) {
        const SLICE_CACHE_MAX: usize = 5;
        let mut c = self.slice_cache.borrow_mut();
        // Basit FIFO: en eskiyi düşür.
        while c.len() >= SLICE_CACHE_MAX {
            if let Some(k) = c.keys().next().cloned() {
                c.remove(&k);
            } else {
                break;
            }
        }
        c.insert(key.to_string(), Some(t));
    }

    /// Manga okuyucusu: bir webtoon şeridinin TEK dilimini dokuya çevirir.
    ///
    /// Manga sayfaları çoğu zaman tek parça uzun şerit (700x13230 gibi).
    /// Tamamını dokuya pişirmek 37 MB yer; kullanıcı da pencereye sığdırılınca
    /// okuyamıyor. Bu yüzden kaynak önce okuma genişliğine ölçekleniyor,
    /// sonra `[y_off .. y_off+len]` satırları KIRPILIYOR. Çözdüğümüz baytları
    /// çağıran taraf saklıyor; her dilim aynı baytlardan üretilir, yani ağdan
    /// bir kez iniyor.
    pub fn manga_slice_texture(
        bytes: &[u8],
        out_w: i32,
        y_off: i32,
        y_len: i32,
    ) -> Option<gtk::gdk::Texture> {
        let loader = gdk_pixbuf::PixbufLoader::new();
        loader.write(bytes).ok()?;
        loader.close().ok()?;
        let src = loader.pixbuf()?;
        let (sw, sh) = (src.width(), src.height());
        if sw == 0 || sh == 0 || y_len <= 0 {
            return None;
        }
        // Okuma genişliğine ölçekle (büyütme yok).
        let dw = out_w.clamp(1, sw);
        let scale = dw as f64 / sw as f64;
        let dh = ((sh as f64 * scale).round() as i32).max(1);
        let scaled = src.scale_simple(dw, dh, gdk_pixbuf::InterpType::Bilinear)?;
        // Dilim koordinatları ölçeklenmiş uzayda.
        let y0 = y_off.clamp(0, dh.saturating_sub(1));
        let hh = y_len.clamp(1, dh - y0);
        let slice = scaled.new_subpixbuf(0, y0, dw, hh);
        Some(gtk::gdk::Texture::for_pixbuf(&slice))
    }

    /// Bir kaynak görselin kaç okuma dilimine bölüneceğini söyler.
    /// `out_w`/`slice_h` okuma hedefi; sonuç (dilim sayısı, toplam yükseklik).
    pub fn manga_slice_count(src_w: u32, src_h: u32, out_w: i32, slice_h: i32) -> (usize, i32) {
        if src_w == 0 || src_h == 0 || out_w <= 0 || slice_h <= 0 {
            return (0, 0);
        }
        let scale = (out_w as f64 / src_w as f64).min(1.0);
        let dh = ((src_h as f64 * scale).round() as i32).max(1);
        if dh <= slice_h {
            return (1, dh);
        }
        let n = (dh + slice_h - 1) / slice_h;
        (n as usize, dh)
    }

    /// Manga okuyucusu sayfası.
    ///
    /// NOT: doku hattı HEDEF boyuta ölçekleyip kırpar; 0x0 verirsen 1x1
    /// doku üretip sayfa boş görünür. Bu yüzden kaynağı en fazla `max_w`
    /// genişliğe indiriyoruz (okuyucuda 1400px üstü kazanım yok, bellek
    /// 11 MB/sayfa yerine ~2 MB).
    pub fn manga_page_picture(&self, url: &str, src_w: u32, src_h: u32, max_w: u32) -> gtk::Picture {
        let pic = gtk::Picture::new();
        pic.set_content_fit(gtk::ContentFit::Contain);
        pic.set_halign(gtk::Align::Center);
        pic.set_valign(gtk::Align::Center);
        let (w, h) = if src_w > 0 && src_h > 0 {
            let w = max_w.min(src_w);
            let h = ((w as f64) * (src_h as f64) / (src_w as f64)).round() as u32;
            (w as i32, h as i32)
        } else {
            (max_w as i32, 0)
        };
        self.load_cover_impl(url, &pic, w, h);
        pic
    }

    /// Spot ışığı için tam boy (w1280) yükleme. TMDB dışı URL'ler
    /// olduğu gibi geçer (haber görselleri).
    pub fn cover_picture_hero(&self, url: Option<&str>, w: i32, h: i32) -> gtk::Picture {
        let pic = new_sized_picture(w, h);
        if let Some(url) = url {
            let big = url
                .replace("image.tmdb.org/t/p/original", "image.tmdb.org/t/p/w1280")
                .replace("image.tmdb.org/t/p/w500", "image.tmdb.org/t/p/w1280")
                .replace("image.tmdb.org/t/p/w342", "image.tmdb.org/t/p/w780")
                .replace("image.tmdb.org/t/p/w185", "image.tmdb.org/t/p/w780");
            self.load_cover_impl(&big, &pic, w, h);
        }
        pic
    }

    fn load_cover_impl(&self, url: &str, pic: &gtk::Picture, w: i32, h: i32) {
        let url = url.to_string();
        let key = format!("{url}@{w}x{h}");

        if let Some(Some(t)) = self.cache.borrow().get(&key) {
            pic.set_paintable(Some(t));
            return;
        }
        if let Some(None) = self.cache.borrow().get(&key) {
            return;
        }

        self.waiters.borrow_mut().entry(key).or_default().push(pic.clone());
        let mut q = self.queue.lock().unwrap();
        if !q.iter().any(|u| u == &url) {
            q.push_back(url);
        }
        drop(q);
        self.pump_covers();
    }

    pub fn scale_pixbuf(bytes: &[u8], w: i32, h: i32) -> Option<gdk_pixbuf::Pixbuf> {
        let loader = gdk_pixbuf::PixbufLoader::new();
        loader.write(bytes).ok()?;
        loader.close().ok()?;
        let src = loader.pixbuf()?;
        // Oranı koru: hedefi dolduracak şekilde büyüt, ortadan kırp.
        // (Yoksa banner gibi farklı oranlı hedeflerde resim gerilir.)
        let (sw, sh) = (src.width() as f64, src.height() as f64);
        if sw <= 0.0 || sh <= 0.0 {
            return None;
        }
        let scale = (w as f64 / sw).max(h as f64 / sh);
        let dw = (sw * scale).ceil() as i32;
        let dh = (sh * scale).ceil() as i32;
        let big = src.scale_simple(dw.max(1), dh.max(1), gdk_pixbuf::InterpType::Bilinear)?;
        let x = ((dw - w) / 2).max(0);
        let y = ((dh - h) / 2).max(0);
        let cw = w.min(dw).max(1);
        let ch = h.min(dh).max(1);
        if x + cw > dw || y + ch > dh {
            return None;
        }
        Some(big.new_subpixbuf(x, y, cw, ch))
    }

    pub fn scale_texture(bytes: &[u8], w: i32, h: i32) -> Option<gtk::gdk::Texture> {
        let pb = Self::scale_pixbuf(bytes, w, h)?;
        Some(gtk::gdk::Texture::for_pixbuf(&pb))
    }

    /// Görseli tam daire şeklinde kırparak (köşeleri şeffaf yaparak) Texture döner.
    pub fn scale_texture_circular(bytes: &[u8], size: i32) -> Option<gtk::gdk::Texture> {
        let loader = gdk_pixbuf::PixbufLoader::new();
        loader.write(bytes).ok()?;
        loader.close().ok()?;
        let src = loader.pixbuf()?;
        let size = size.max(1);
        let (sw, sh) = (src.width() as f64, src.height() as f64);
        if sw <= 0.0 || sh <= 0.0 {
            return None;
        }
        let scale = (size as f64 / sw).max(size as f64 / sh);
        let dw = (sw * scale).ceil() as i32;
        let dh = (sh * scale).ceil() as i32;
        let big = src.scale_simple(dw.max(1), dh.max(1), gdk_pixbuf::InterpType::Bilinear)?;
        let x = ((dw - size) / 2).max(0);
        let y = ((dh - size) / 2).max(0);
        let cropped = big.new_subpixbuf(x, y, size, size);
        let with_alpha = if cropped.has_alpha() {
            cropped
        } else {
            cropped.add_alpha(false, 0, 0, 0).ok()?
        };

        let pixels = with_alpha.read_pixel_bytes();
        let slice = pixels.as_ref();
        let stride = with_alpha.rowstride() as usize;
        let n_channels = with_alpha.n_channels() as usize;
        let mut out = vec![0u8; (size * size * 4) as usize];
        let center = size as f64 / 2.0;
        let radius = size as f64 / 2.0;

        for py in 0..size {
            for px in 0..size {
                let out_idx = ((py * size + px) * 4) as usize;
                let dx = px as f64 - center + 0.5;
                let dy = py as f64 - center + 0.5;
                let dist = (dx * dx + dy * dy).sqrt();

                let src_idx = py as usize * stride + px as usize * n_channels;
                let r = slice[src_idx];
                let g = slice[src_idx + 1];
                let b = slice[src_idx + 2];
                let a = if n_channels == 4 { slice[src_idx + 3] } else { 255 };

                let final_a = if dist > radius {
                    0
                } else if dist > radius - 1.0 {
                    ((a as f64) * (radius - dist)) as u8
                } else {
                    a
                };
                if final_a == 0 {
                    out[out_idx] = 0;
                    out[out_idx + 1] = 0;
                    out[out_idx + 2] = 0;
                    out[out_idx + 3] = 0;
                } else {
                    out[out_idx] = r;
                    out[out_idx + 1] = g;
                    out[out_idx + 2] = b;
                    out[out_idx + 3] = final_a;
                }
            }
        }

        let glib_bytes = glib::Bytes::from_owned(out);
        let circular_pb = gdk_pixbuf::Pixbuf::from_bytes(
            &glib_bytes,
            gdk_pixbuf::Colorspace::Rgb,
            true,
            8,
            size,
            size,
            size * 4,
        );
        Some(gtk::gdk::Texture::for_pixbuf(&circular_pb))
    }

    fn pump_covers(&self) {
        let max_workers = 6;
        let mut active = self.active.get();

        while active < max_workers {
            let url = self.queue.lock().unwrap().pop_front();
            let Some(url) = url else { break };

            // Bu URL'i bekleyen anahtarları ve boyutlarını ANA iş parçacığında
            // topla. Piksel işi (çöz + ölçekle + kırp) iş parçacığında
            // yapılacak; ana döngü yalnızca doku yükleyecek.
            let jobs: Vec<(String, i32, i32)> = self
                .waiters
                .borrow()
                .iter()
                .filter_map(|(key, _)| Self::split_key(key, &url).map(|(w, h)| (key.clone(), w, h)))
                .collect();
            if jobs.is_empty() {
                continue;
            }

            self.active.set(active + 1);
            active += 1;

            let client = self.client.clone();
            let url2 = url.clone();
            let (tx, rx) = futures::channel::oneshot::channel::<Vec<(String, Option<ScaledCover>)>>();

            let this = self.clone_ref();
            glib::MainContext::default().spawn_local(async move {
                if let Ok(done) = rx.await {
                    this.finish_cover(done);
                    let curr = this.active.get();
                    if curr > 0 {
                        this.active.set(curr - 1);
                    }
                    this.pump_covers();
                }
            });

            std::thread::spawn(move || {
                let bytes = client.get_bytes(&url);
                let done = match bytes {
                    Some(b) => jobs
                        .into_iter()
                        .map(|(key, w, h)| {
                            let scaled =
                                Self::scale_pixbuf(&b, w, h).and_then(|pb| Self::pack_pixbuf(&pb));
                            (key, scaled)
                        })
                        .collect(),
                    // İndirilemedi: ölçek yok, anahtar "başarısız" olarak önbelleğe girer.
                    None => jobs.into_iter().map(|(key, _, _)| (key, None)).collect(),
                };
                let _ = tx.send(done);
            });
        }
    }

    /// Önbellek anahtarını "`url@WxH`" biçiminden boyutlara ayırır.
    fn split_key(key: &str, url: &str) -> Option<(i32, i32)> {
        let rest = key.strip_prefix(url)?.strip_prefix('@')?;
        let (ws, hs) = rest.split_once('x')?;
        Some((ws.parse().ok()?, hs.parse().ok()?))
    }

    fn pack_pixbuf(pb: &gdk_pixbuf::Pixbuf) -> Option<ScaledCover> {
        let (w, h) = (pb.width(), pb.height());
        let nch = pb.n_channels();
        let stride = pb.rowstride() as usize;
        let want = w as usize * nch as usize;
        if want == 0 || h == 0 {
            return None;
        }
        let px = pb.read_pixel_bytes();
        let src = px.as_ref();
        let mut data = Vec::with_capacity(want * h as usize);
        for y in 0..h as usize {
            let row = src.get(y * stride..y * stride + want)?;
            data.extend_from_slice(row);
        }
        Some(ScaledCover {
            w,
            h,
            nch,
            stride: want as i32,
            data,
        })
    }

    fn finish_cover(&self, done: Vec<(String, Option<ScaledCover>)>) {
        const TEX_CACHE_MAX: usize = 36;
        let mut cache = self.cache.borrow_mut();
        let mut order = self.cache_order.borrow_mut();
        let mut waiters = self.waiters.borrow_mut();

        for (key, scaled) in done {
            let texture = scaled.and_then(|s| {
                let bytes = glib::Bytes::from_owned(s.data);
                let pb = gdk_pixbuf::Pixbuf::from_bytes(
                    &bytes,
                    gdk_pixbuf::Colorspace::Rgb,
                    s.nch == 4,
                    8,
                    s.w,
                    s.h,
                    s.stride,
                );
                Some(gtk::gdk::Texture::for_pixbuf(&pb))
            });
            if let Some(t) = &texture {
                if let Some(pics) = waiters.get(&key) {
                    for p in pics {
                        p.set_paintable(Some(t));
                    }
                }
            }
            if !cache.contains_key(&key) {
                order.push_back(key.clone());
            }
            cache.insert(key.clone(), texture);
            waiters.remove(&key);
        }

        while cache.len() > TEX_CACHE_MAX {
            if let Some(old) = order.pop_front() {
                cache.remove(&old);
            } else {
                break;
            }
        }
    }
}

