//! Gömülü oynatıcı — ana pencerenin Stack'inde açılan oynatıcı sayfası.
//!
//! Bölüm tıklanınca harici `mpv` process'i yerine uygulamanın içindeki
//! bu sayfa açılır (Ayarlar → Gömülü Oynatıcı).
//!
//! Davranış birebir harici oynatıcıdaki gibidir:
//! kaynak fallback (ölü kaynağı geç), AniSkip `s`/`e`, kaldığın yerden
//! devam, progress + izlendi kaydı, tercih edilen host, ses kontrolü.
//! Video `GtkGLArea`'da çizilir, IPC socket yok — her şey main thread'de,
//! `MpvEmbed` API'si üzerinden yürür.
//!
//! Sayfadan ayrılınca (geri/başka sekme) oynatıcı `shutdown()` ile
//! durdurulur ve son konum kaydedilir.

use adw::prelude::*;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, Instant};

use crate::api::{self, AniSkipTimes, Client, Episode, Title};
use crate::app::resolve_upscale_shader;
use crate::embed_mpv::{current_fbo, MpvEmbed, RenderContext};

/// Harici `play_candidates` akışından gelen çözümlenmiş oynatma isteği.
pub struct EmbedRequest {
    pub title: Title,
    pub ep: Episode,
    /// Resolve edilmiş direkt medya URL'leri (öncelik sırasıyla).
    pub candidates: Vec<String>,
    /// Hızlı embed URL'leri (sadece host-hint bilgisi için).
    pub fast_embeds: Vec<String>,
    /// JIT çözülecek yedek embed URL'leri.
    pub fallback_embeds: Vec<String>,
    pub saved_pos: Option<f64>,
    pub upscale: String,
    pub aniskip_enabled: bool,
    pub patience_secs: u64,
    /// Ayar açık ise player sayfası tam ekranda açılır.
    pub auto_fullscreen: bool,
    /// Komşu bölümler (listeden bulunur, yoksa None).
    pub prev_ep: Option<Episode>,
    pub next_ep: Option<Episode>,
    /// Tüm bölümler (sağ panel listesi + sezon seçici için).
    pub episodes: Vec<Episode>,
    /// Bölüm değiştirme (App.play'e bağlanır).
    pub play_episode: Option<Rc<dyn Fn(Episode)>>,
    /// Geri gitme (App.go_back'e bağlanır).
    pub on_back: Option<Rc<dyn Fn()>>,
}

#[derive(Clone)]
enum Source {
    Direct(String),
    Embed(String),
}

impl Source {
    fn hint_url(&self) -> &str {
        match self {
            Source::Direct(u) | Source::Embed(u) => u,
        }
    }
}

struct PlayerState {
    // Drop sırası kritik: render_ctx HER ZAMAN player handle'dan önce
    // düşmeli (Rust field'ları bildirim sırasıyla droplar). shutdown()
    // zaten bunu garantiler, bu da sigortasıdır.
    render_ctx: Option<RenderContext>,
    player: MpvEmbed,
    sources: Vec<Source>,
    /// Şu an yüklenen/yüklenmekte olan kaynak index'i.
    index: usize,
    /// Kaynak yükleme başlangıcı (dead-source timeout için).
    load_started: Instant,
    /// Bu kaynakta süre görüldü mü (medya yüklendi sayılır).
    media_loaded: bool,
    /// "Oynatılıyor" durumu gösterildi mi (status satırı için).
    playing_shown: bool,
    /// Arka planda embed çözümü sürüyor mu.
    resolving: bool,
    resolve_result: Arc<Mutex<Option<Result<String, String>>>>,
    /// AniSkip zamanları (background thread doldurur).
    aniskip: Arc<Mutex<AniSkipTimes>>,
    op_prompted: bool,
    ed_prompted: bool,
    op_seek_done: bool,
    ed_seek_done: bool,
    focus_mode: bool,
    auto_next_episode: bool,
    next_ep: Option<Episode>,
    play_episode: Option<Rc<dyn Fn(Episode)>>,
    next_triggered: bool,
    watched_marked: bool,
    host_saved: bool,
    alive: Arc<AtomicBool>,
    seeking: Rc<Cell<bool>>,
    vol_changing: Rc<Cell<bool>>,
    prog_key: String,
    season: u64,
    episode: u64,
    client: Arc<Client>,
    progress: Rc<RefCell<HashMap<String, (f64, f64)>>>,
    // --- sinema modu ---
    main_window: adw::ApplicationWindow,
    header: adw::HeaderBar,
    sidebar: gtk::Revealer,
    gl_area: gtk::GLArea,
    /// Son fare hareketi (oto-gizleme için).
    last_motion: Instant,
    /// Son fare konumu (sahte/yerinde motion olaylarını elemek için).
    last_mx: f64,
    last_my: f64,
    /// Kromun gizlendiği an (sonrası grace süresi).
    hidden_at: Option<Instant>,
    /// Gizlenme anındaki fare konumu (geri dönüş eşiği için).
    hide_mx: f64,
    hide_my: f64,
    /// İmleç gizli mi (kromdan bağımsız, 1sn kuralı).
    cursor_hidden: bool,
    /// Üst yüzen bar (geri + başlık + pencere düğmeleri, kromla gizlenir).
    topbar: gtk::Revealer,
    /// Tam ekranda üst+alt bar gizli mi.
    chrome_hidden: bool,
    /// Tam ekranı biz açtıysak shutdown'da geri al.
    we_fullscreened: bool,
    /// Tek tık bekliyor (çift tık gelirse iptal).
    click_pending: Rc<Cell<bool>>,
    /// Header'daki hareket dinleyici (shutdown'da kaldırılır).
    header_motion: Option<gtk::EventControllerMotion>,
    /// Ortada parlayan ikon (oynat/duraklat geri bildirimi).
    flash_img: gtk::Image,
    flash_rev: gtk::Revealer,
    /// Kaynak boyutları (candidates sırasıyla; yedekler None).
    sizes: Arc<Mutex<Vec<Option<u64>>>>,
    /// Sunucuya son konum raporu (60sn'de bir).
    last_report: Instant,
}

/// Sayfaya gömülen oynatıcı. `App.player` içinde tutulur;
/// sayfadan ayrılınca `shutdown()` çağrılmalıdır.
pub struct EmbeddedPlayer {
    state: Rc<RefCell<PlayerState>>,
    widget: gtk::Box,
}

impl EmbeddedPlayer {
    pub fn widget(&self) -> &gtk::Box {
        &self.widget
    }

    /// Oynatmayı durdurur, son konumu kaydeder, timer'ları durdurur.
    /// Sinema modunu geri alır (header göster, imleç normal, tam ekrandan çık).
    /// GLArea stack'ten düşünce render ctx zaten düşer; burada da garantiye alınır.
    pub fn shutdown(&self) {
        let mut s = self.state.borrow_mut();
        if !s.alive.swap(false, Ordering::Relaxed) {
            return;
        }
        // Önce oynatmayı durdur: mpv kare üretmeyi bıraksın, update callback
        // artık queue_render tetiklemesin. GL ctx bundan sonra düşer.
        s.player.set_pause(true);
        s.player.stop();
        // Render callback'in çizecek bir şeyi kalmasın; stack çocuğu
        // kaldırılırken ölü FBO'ya çizim olmasın.
        s.render_ctx = None;
        // sinema modunu geri al
        s.we_fullscreened = false;
        if s.main_window.is_fullscreen() {
            s.main_window.unfullscreen();
        }
        s.chrome_hidden = false;
        s.cursor_hidden = false;
        s.header.set_visible(true);
        s.sidebar.set_reveal_child(true);
        s.gl_area.set_cursor_from_name(None);
        // kapanış konumunu sunucuya bildir
        {
            let pos = s.player.pos();
            if pos >= 10.0 {
                let rc = s.client.clone();
                let (rt, rs, re_) = (s.tid(), s.season, s.episode);
                std::thread::spawn(move || rc.report_pos(rt, rs, re_, pos));
            }
        }
        let pos = s.player.pos();
        let dur = s.player.dur();
        if pos >= 1.0 {
            s.progress.borrow_mut().insert(s.prog_key.clone(), (pos, dur));
            s.client.save_progress(s.tid(), s.season, s.episode, pos, dur);
        }
        // App'e ait dinleyicileri kaldır (birikmesin).
        if let Some(mc) = s.header_motion.take() {
            s.header.remove_controller(&mc);
        }
        eprintln!("[EMBED] oynatıcı kapatıldı (sayfadan çıkıldı)");
    }

    pub fn is_alive(&self) -> bool {
        self.state.borrow().alive.load(Ordering::Relaxed)
    }
}

impl PlayerState {
    fn tid(&self) -> u64 {
        self.prog_key
            .split(':')
            .next()
            .and_then(|p| p.parse().ok())
            .unwrap_or(0)
    }
}

pub fn fmt_time(s: f64) -> String {
    if !s.is_finite() || s < 0.0 {
        return "--:--".into();
    }
    let s = s as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

fn sibnet_referer(url: &str) -> Option<String> {
    if !url.contains("video.sibnet.ru/v/") {
        return None;
    }
    let vid = url
        .split("/v/")
        .nth(1)
        .and_then(|s| s.split('/').nth(1))
        .map(|s| s.trim_end_matches(".mp4"))
        .unwrap_or("");
    Some(if vid.is_empty() {
        "https://video.sibnet.ru/".to_string()
    } else {
        format!("https://video.sibnet.ru/shell.php?videoid={vid}")
    })
}

fn proxy_url() -> Option<String> {
    let ok = std::net::TcpStream::connect_timeout(
        &"127.0.0.1:10808".parse().expect("statik adres"),
        Duration::from_millis(300),
    )
    .is_ok();
    if ok {
        eprintln!("[EMBED] yerel proxy aktif, gömülü oynatıcı oradan çıkacak");
        Some("http://127.0.0.1:10808".to_string())
    } else {
        None
    }
}

/// `--opt=val` CLI arg'larını mpv option çiftlerine çevirir.
fn cli_args_to_opts(args: &[String]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for a in args {
        let t = a.strip_prefix("--").unwrap_or(a);
        if let Some((k, v)) = t.split_once('=') {
            out.push((k.to_string(), v.to_string()));
        }
    }
    out
}

fn upscale_opts(upscale: &str) -> Vec<(String, String)> {
    let shader = match upscale {
        "hafif" => resolve_upscale_shader("Anime4K_Upscale_DTD_x2.glsl"),
        "ultra" => resolve_upscale_shader("Anime4K_Upscale_CNN_x2_UL.glsl"),
        "hafif_keskin" => resolve_upscale_shader("Anime4K_Upscale_DTD_x2.glsl"),
        _ => None,
    };
    let args = api::upscale_mpv_args(upscale, shader.as_deref(), None);
    cli_args_to_opts(&args)
}

/// Kromu gizle: alt bar + üst yüzen bar + imleç.
/// Çağıran `PlayerState` borrow'unu elinde tutuyor olmalı.
fn hide_chrome_locked(s: &mut PlayerState, controls: &gtk::Revealer) {
    if s.chrome_hidden {
        return;
    }
    s.chrome_hidden = true;
    s.hidden_at = Some(Instant::now());
    s.hide_mx = s.last_mx;
    s.hide_my = s.last_my;
    controls.set_reveal_child(false);
    s.topbar.set_reveal_child(false);
    if !s.cursor_hidden {
        s.cursor_hidden = true;
        s.gl_area.set_cursor_from_name(Some("none"));
    }
    eprintln!("[EMBED] krom gizleniyor");
}

/// Kromu göster: alt bar + üst yüzen bar + imleç.
/// İzlerken sidebar ASLA açılmaz (bölümler sağ panelden seçilir).
/// (Native header'a dokunmaz — o sayfa açıkken hep gizlidir.)
fn show_chrome_locked(s: &mut PlayerState, controls: &gtk::Revealer) {
    let mut changed = false;
    if s.chrome_hidden {
        s.chrome_hidden = false;
        changed = true;
    }
    controls.set_reveal_child(true);
    s.topbar.set_reveal_child(true);
    if s.cursor_hidden {
        s.cursor_hidden = false;
        s.gl_area.set_cursor_from_name(None);
        changed = true;
    }
    s.last_motion = Instant::now();
    if changed {
        eprintln!("[EMBED] krom gösteriliyor");
    }
}

/// Tam ekran aç/kapat. Üst bar yönetimi krom mantığında
/// (tam ekranda da pencerelide de aynı yüzen bar).
fn set_fullscreen(state: &Rc<RefCell<PlayerState>>, controls: &gtk::Revealer, on: bool) {
    if on {
        eprintln!("[EMBED] tam ekran açılıyor");
        {
            let mut s = state.borrow_mut();
            s.main_window.fullscreen();
            s.we_fullscreened = true;
            // yan menü de tam ekranda gizlenir
            s.sidebar.set_reveal_child(false);
            s.chrome_hidden = false;
            s.cursor_hidden = false;
            s.last_motion = Instant::now();
            s.hidden_at = Some(Instant::now());
            s.hide_mx = s.last_mx;
            s.hide_my = s.last_my;
            s.gl_area.set_cursor_from_name(None);
        }
        controls.set_reveal_child(true);
        state.borrow().topbar.set_reveal_child(true);
    } else {
        eprintln!("[EMBED] tam ekrandan çıkılıyor");
        {
            let mut s = state.borrow_mut();
            s.main_window.unfullscreen();
            s.we_fullscreened = false;
            // Gömülü player'da sidebar kapalı kalır.
            s.sidebar.set_reveal_child(false);
        }
        show_chrome_locked(&mut state.borrow_mut(), controls);
    }
}

/// Fare hareketi: kromu göster, boşta-sayar saati sıfırla.
/// Aynı koordinata gelen sahte olaylar (gizleme sonrası layout kayması gibi)
/// elenir; gizlemeden sonraki ilk 800ms'deki tüm hareketler yoksayılır —
/// yoksa gizle/göster sonsuz kavgaya girer.
fn poke_activity(state: &Rc<RefCell<PlayerState>>, controls: &gtk::Revealer, x: f64, y: f64) {
    let mut s = state.borrow_mut();
    // yerinde sayan sahte olay
    if (x - s.last_mx).abs() < 0.5 && (y - s.last_my).abs() < 0.5 {
        return;
    }
    s.last_mx = x;
    s.last_my = y;
    // grace: gizleme sonrası layout kaymasından gelen sentetik hareketler
    if let Some(h) = s.hidden_at {
        if h.elapsed() < Duration::from_millis(800) {
            return;
        }
    }
    // gizliyken küçük kıpırtılar uyandırmasın (eşik 12px)
    if s.chrome_hidden || s.cursor_hidden {
        let dx = (x - s.hide_mx).abs();
        let dy = (y - s.hide_my).abs();
        if dx.max(dy) < 12.0 {
            return;
        }
    }
    let need_show = s.cursor_hidden || s.chrome_hidden;
    drop(s);
    if need_show {
        show_chrome_locked(&mut state.borrow_mut(), controls);
    } else {
        state.borrow_mut().last_motion = Instant::now();
    }
}

fn toast_in(overlay: &adw::ToastOverlay, msg: &str, secs: u32) {
    let t = adw::Toast::new(msg);
    t.set_timeout(secs);
    overlay.add_toast(t);
}

/// Kapak karesini arka planda indirip boyuta uydurur, hazır olunca
/// resme koyar. UI thread'ini tutmaz; hedef öldüyse sessizce vazgeçer.
/// Kapak karesini arka planda indirip boyuta uydurur, hazır olunca
/// resme koyar. UI thread'ini tutmaz; pencere kapandıysa sonuç düşer.
fn load_thumb_async(client: &Arc<Client>, url: Option<String>, pic: &gtk::Picture, w: i32, h: i32) {
    let Some(url) = url else {
        return;
    };
    let client = client.clone();
    let (tx, rx) = std::sync::mpsc::channel::<Option<Vec<u8>>>();
    std::thread::spawn(move || {
        let _ = tx.send(client.get_bytes(&url));
    });
    let pic_c = pic.clone();
    glib::timeout_add_local(Duration::from_millis(50), move || {
        match rx.try_recv() {
            Ok(bytes) => {
                if let Some(b) = bytes {
                    if let Some(t) = crate::covers::CoverManager::scale_texture(&b, w, h) {
                        pic_c.set_paintable(Some(&t));
                    }
                }
                glib::ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(_) => glib::ControlFlow::Break,
        }
    });
}

fn ctx_row(box_: &gtk::Box, pop: &gtk::Popover, label: &str, icon: &str, sensitive: bool, hint: Option<&str>, f: impl Fn() + 'static) {
    // Butonun kendi ikon+etiket dizilimine güvenmiyoruz (temaya göre
    // etiketi yutabiliyor); satırı elle kuruyoruz: ikon + yazı + kısayol.
    let b = gtk::Button::new();
    b.add_css_class("flat");
    b.set_sensitive(sensitive);
    b.set_hexpand(true);
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row.set_hexpand(true);
    row.set_margin_top(5);
    row.set_margin_bottom(5);
    let img = gtk::Image::from_icon_name(icon);
    img.set_valign(gtk::Align::Center);
    let lbl = gtk::Label::new(Some(label));
    lbl.set_xalign(0.0);
    lbl.set_hexpand(true);
    row.append(&img);
    row.append(&lbl);
    if let Some(h) = hint {
        if !h.is_empty() {
            let hl = gtk::Label::new(Some(h));
            hl.add_css_class("dim-label");
            hl.add_css_class("caption");
            row.append(&hl);
        }
    }
    b.set_child(Some(&row));
    let pop_c = pop.clone();
    b.connect_clicked(move |_| {
        pop_c.popdown();
        f();
    });
    box_.append(&b);
}

fn ctx_sep(box_: &gtk::Box) {
    box_.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
}

/// Dosya adında yasak karakterleri temizler.
fn safe_fname(s: &str) -> String {
    let mut o: String = s.chars().filter(|c| !"/\\:*?\"<>|".contains(*c)).collect();
    o = o.trim().to_string();
    if o.chars().count() > 80 {
        o = o.chars().take(80).collect();
    }
    if o.is_empty() {
        o = "animecix".into();
    }
    o
}
fn flash(state: &Rc<RefCell<PlayerState>>, icon: &str) {
    let s = state.borrow();
    s.flash_img.set_icon_name(Some(icon));
    s.flash_rev.set_reveal_child(true);
    let rev_c = s.flash_rev.clone();
    let alive_c = s.alive.clone();
    drop(s);
    glib::timeout_add_local_once(Duration::from_millis(550), move || {
        if alive_c.load(Ordering::Relaxed) {
            rev_c.set_reveal_child(false);
        }
    });
}

/// Oynat/duraklat + ortada ikon. paused=true dönerse video durdu.
fn toggle_with_flash(state: &Rc<RefCell<PlayerState>>) -> bool {
    let paused = {
        let s = state.borrow();
        s.player.toggle_pause();
        s.player.paused()
    };
    flash(
        state,
        if paused {
            "media-playback-pause-symbolic"
        } else {
            "media-playback-start-symbolic"
        },
    );
    paused
}

/// URL'yi dosyaya indirir. İlerleme callback'i + iptal bayrağı ile.
pub(crate) fn download_to_file(
    url: &str,
    path: &std::path::Path,
    mut on_prog: impl FnMut(u64, Option<u64>),
    cancel: &AtomicBool,
) -> Result<u64, String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::USER_AGENT,
        "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36"
            .parse()
            .unwrap(),
    );
    if let Some(r) = sibnet_referer(url) {
        headers.insert(
            reqwest::header::REFERER,
            r.parse().map_err(|e| format!("referer: {e:?}"))?,
        );
    }
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;
    let mut resp = client
        .get(url)
        .headers(headers)
        .send()
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let total = resp.content_length();
    let mut file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut done: u64 = 0;
    let mut buf = [0u8; 64 * 1024];
    use std::io::Read;
    loop {
        if cancel.load(Ordering::Relaxed) {
            drop(file);
            let _ = std::fs::remove_file(path);
            return Err("iptal edildi".into());
        }
        match resp.read(&mut buf).map_err(|e| e.to_string())? {
            0 => break,
            n => {
                use std::io::Write;
                file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
                done += n as u64;
                on_prog(done, total);
            }
        }
    }
    Ok(done)
}

/// Bölüm indirme penceresi (ilerleme + iptal).
fn start_download(
    parent: &adw::ApplicationWindow,
    toast: &adw::ToastOverlay,
    url: &str,
    title_name: &str,
    season: u64,
    episode: u64,
    ep_name: &str,
) {
    let ext = url
        .split('?')
        .next()
        .unwrap_or("")
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_string();
    let ext = if !ext.is_empty() && ext.len() <= 4 && ext.chars().all(|c| c.is_ascii_alphanumeric()) {
        ext
    } else {
        "mp4".into()
    };
    let fname = format!(
        "{} S{:02}E{:02} - {}.{}",
        safe_fname(title_name),
        season,
        episode,
        safe_fname(ep_name),
        ext
    );
    let dir = glib::user_special_dir(glib::UserDirectory::Videos)
        .map(|p| p.join("Animecix"))
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp/Animecix"));
    let path = dir.join(&fname);

    let win = gtk::Window::builder()
        .title("Bölüm indiriliyor")
        .modal(true)
        .transient_for(parent)
        .default_width(400)
        .resizable(false)
        .build();
    let v = gtk::Box::new(gtk::Orientation::Vertical, 8);
    v.set_margin_top(16);
    v.set_margin_bottom(16);
    v.set_margin_start(16);
    v.set_margin_end(16);
    let name_lbl = gtk::Label::new(Some(&fname));
    name_lbl.set_wrap(true);
    name_lbl.set_max_width_chars(48);
    let bar = gtk::ProgressBar::new();
    bar.set_show_text(true);
    let status = gtk::Label::new(Some("bağlanıyor…"));
    status.add_css_class("dim-label");
    let cancel_btn = gtk::Button::with_label("İptal");
    cancel_btn.add_css_class("destructive-action");
    v.append(&name_lbl);
    v.append(&bar);
    v.append(&status);
    v.append(&cancel_btn);
    win.set_child(Some(&v));

    let (prog_tx, prog_rx) = std::sync::mpsc::channel::<(u64, Option<u64>)>();
    let (done_tx, done_rx) = std::sync::mpsc::channel::<Result<u64, String>>();
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_c = cancel.clone();
    let url_c = url.to_string();
    let path_c = path.clone();
    std::thread::spawn(move || {
        let res = download_to_file(&url_c, &path_c, |d, t| {
            let _ = prog_tx.send((d, t));
        }, &cancel_c);
        let _ = done_tx.send(res);
    });
    {
        let cancel_c = cancel.clone();
        cancel_btn.connect_clicked(move |_| {
            cancel_c.store(true, Ordering::Relaxed);
        });
    }
    {
        let cancel_c = cancel.clone();
        win.connect_close_request(move |_| {
            cancel_c.store(true, Ordering::Relaxed);
            glib::Propagation::Proceed
        });
    }
    let toast_c = toast.clone();
    let path_str = path.to_string_lossy().into_owned();
    let win_c = win.clone();
    glib::timeout_add_local(Duration::from_millis(200), move || {
        while let Ok((d, t)) = prog_rx.try_recv() {
            if let Some(total) = t {
                if total > 0 {
                    bar.set_fraction((d as f64 / total as f64).clamp(0.0, 1.0));
                    bar.set_text(Some(&format!(
                        "{:.1} / {:.1} MB",
                        d as f64 / 1048576.0,
                        total as f64 / 1048576.0
                    )));
                }
            } else {
                bar.pulse();
                bar.set_text(Some(&format!("{:.1} MB", d as f64 / 1048576.0)));
            }
            status.set_text(&format!("indiriliyor… ({path_str})"));
        }
        match done_rx.try_recv() {
            Ok(Ok(bytes)) => {
                toast_in(
                    &toast_c,
                    &format!("✓ İndirildi ({:.1} MB): {fname}", bytes as f64 / 1048576.0),
                    5,
                );
                win_c.close();
                glib::ControlFlow::Break
            }
            Ok(Err(e)) => {
                if e != "iptal edildi" {
                    toast_in(&toast_c, &format!("İndirme hatası: {e}"), 4);
                } else {
                    toast_in(&toast_c, "İndirme iptal edildi", 3);
                }
                win_c.close();
                glib::ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(_) => {
                win_c.close();
                glib::ControlFlow::Break
            }
        }
    });
    win.present();
}

/// Video bilgi penceresi.
fn show_video_info(parent: &adw::ApplicationWindow, heading: &str, info: &[(String, String)]) {
    let body = if info.is_empty() {
        "Bilgi yok (video henüz yüklenmedi mi?)".to_string()
    } else {
        info.iter()
            .map(|(k, v)| format!("{k}: {v}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let d = adw::MessageDialog::builder()
        .heading(heading)
        .body(&body)
        .close_response("ok")
        .build();
    d.add_response("ok", "Tamam");
    d.set_transient_for(Some(parent));
    d.present();
}

fn section_label(t: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(t));
    l.set_xalign(0.0);
    l.add_css_class("dim-label");
    l.add_css_class("caption");
    l
}

/// Şu an atlanabilir intro/outro penceresi varsa (hedef, intro_mu) döner.
fn skip_target(t: &AniSkipTimes, pos: f64, op_done: bool, ed_done: bool) -> Option<(f64, bool)> {
    if !op_done {
        if let (Some(st), Some(en)) = (t.op_start, t.op_end) {
            if pos >= st - 1.5 && pos <= en {
                return Some((en, true));
            }
        }
    }
    if !ed_done {
        if let (Some(st), Some(en)) = (t.ed_start, t.ed_end) {
            if pos >= st - 1.5 && pos <= en {
                return Some((en, false));
            }
        }
    }
    None
}

/// Sağ tık bağlam menüsünü o anki duruma göre kurar.
#[allow(clippy::too_many_arguments)]
fn rebuild_ctx_menu(
    pop: &gtk::Popover,
    state: &Rc<RefCell<PlayerState>>,
    controls: &gtk::Revealer,
    toast: &adw::ToastOverlay,
    win: &adw::ApplicationWindow,
    play_cb: &Option<Rc<dyn Fn(Episode)>>,
    prev: &Option<Episode>,
    next: &Option<Episode>,
    dl: &(String, u64, u64, String),
) {
    let v = gtk::Box::new(gtk::Orientation::Vertical, 2);
    v.set_margin_top(6);
    v.set_margin_bottom(6);
    v.set_margin_start(6);
    v.set_margin_end(6);

    let (paused, skip, fill, cur_url) = {
        let s = state.borrow();
        let t = s.aniskip.lock().unwrap().clone();
        let sk = skip_target(&t, s.player.pos(), s.op_seek_done, s.ed_seek_done);
        let url = match &s.sources[s.index] {
            Source::Direct(u) => Some(u.clone()),
            Source::Embed(_) => None,
        };
        (s.player.paused(), sk, s.player.panscan() >= 0.5, url)
    };

    // oynat / duraklat
    {
        let st_c = state.clone();
        ctx_row(
            &v, pop,
            if paused { "Oynat" } else { "Duraklat" },
            if paused { "media-playback-start-symbolic" } else { "media-playback-pause-symbolic" },
            true, Some("Space"),
            move || { toggle_with_flash(&st_c); },
        );
    }
    // tam ekran
    {
        let st_c = state.clone();
        let rev_c = controls.clone();
        let is_fs = win.is_fullscreen();
        ctx_row(
            &v, pop,
            if is_fs { "Pencereli moda dön" } else { "Tam ekran" },
            if is_fs { "view-restore-symbolic" } else { "view-fullscreen-symbolic" },
            true, Some("F"),
            move || set_fullscreen(&st_c, &rev_c, !is_fs),
        );
    }
    ctx_sep(&v);
    // intro / outro atla
    {
        let st_c = state.clone();
        let toast_c = toast.clone();
        let (lbl, target) = match skip {
            Some((t, true)) => ("İntroyu atla".to_string(), Some(t)),
            Some((t, false)) => ("Outro'yu atla".to_string(), Some(t)),
            None => ("Atlanacak yer yok".to_string(), None),
        };
        ctx_row(&v, pop, &lbl, "media-seek-forward-symbolic", target.is_some(), Some("S"), move || {
            if let (Some(t), Ok(mut s)) = (target, st_c.try_borrow_mut()) {
                if s.aniskip.lock().unwrap().clone().op_end == Some(t) {
                    s.op_seek_done = true;
                } else {
                    s.ed_seek_done = true;
                }
                s.player.seek_abs(t);
                s.player.show_text("⏩ Atlandı", 2500);
                toast_in(&toast_c, "⏩ Atlandı", 2);
            }
        });
    }
    // önceki / sonraki bölüm
    {
        let cb_c = play_cb.clone();
        let p = prev.clone();
        ctx_row(&v, pop, "Önceki bölüm", "media-skip-backward-symbolic", p.is_some() && cb_c.is_some(), None, move || {
            if let (Some(cb), Some(ep)) = (cb_c.as_ref(), p.as_ref()) {
                cb(ep.clone());
            }
        });
        let cb_c2 = play_cb.clone();
        let n = next.clone();
        ctx_row(&v, pop, "Sonraki bölüm", "media-skip-forward-symbolic", n.is_some() && cb_c2.is_some(), None, move || {
            if let (Some(cb), Some(ep)) = (cb_c2.as_ref(), n.as_ref()) {
                cb(ep.clone());
            }
        });
    }
    ctx_sep(&v);
    // doldur / sığdır
    {
        let st_c = state.clone();
        ctx_row(
            &v, pop,
            if fill { "Orijinal oran" } else { "Ekranı doldur" },
            "view-fullscreen-symbolic",
            true, Some("A"),
            move || {
                if let Ok(s) = st_c.try_borrow() {
                    s.player.set_panscan(if fill { 0.0 } else { 1.0 });
                }
            },
        );
    }
    // ekran görüntüsü
    {
        let st_c = state.clone();
        let toast_c = toast.clone();
        let win_cc = win.clone();
        let (tn, ts, te) = (dl.0.clone(), dl.1, dl.2);
        ctx_row(&v, pop, "Ekran görüntüsü al", "camera-photo-symbolic", true, None, move || {
            let pics = glib::user_special_dir(glib::UserDirectory::Pictures)
                .unwrap_or_else(|| std::path::PathBuf::from("/tmp"));
            let ts_now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let p = pics.join(format!("{}-S{:02}E{:02}-{ts_now}.png", safe_fname(&tn), ts, te));
            let disp_win = win_cc.clone();
            let msg = match st_c.try_borrow() {
                Ok(s) => match s.player.screenshot_to_file(&p.to_string_lossy()) {
                    Ok(()) => {
                        // dosyaya kaydet + panoya kopyala (yapıştırılabilsin)
                        let clip: Result<(), String> = (|| {
                            let f = gtk::gio::File::for_path(&p);
                            let tex = gtk::gdk::Texture::from_file(&f)
                                .map_err(|e| e.to_string())?;
                            gtk::prelude::WidgetExt::display(&disp_win).clipboard().set_texture(&tex);
                            Ok(())
                        })();
                        match clip {
                            Ok(()) => "✓ Kaydedildi + panoya kopyalandı".to_string(),
                            Err(e) => format!("✓ Dosyaya kaydedildi (pano: {e})"),
                        }
                    }
                    Err(e) => format!("Ekran görüntüsü hatası: {e}"),
                },
                Err(_) => "Şu an alınamadı".to_string(),
            };
            toast_in(&toast_c, &msg, 4);
        });
    }
    // video bilgisi
    {
        let st_c = state.clone();
        let win_c = win.clone();
        let heading = format!("{} S{:02}E{:02}", dl.0, dl.1, dl.2);
        ctx_row(&v, pop, "Video bilgisi", "dialog-information-symbolic", true, None, move || {
            let info = st_c.try_borrow().map(|s| s.player.video_info()).unwrap_or_default();
            show_video_info(&win_c, &heading, &info);
        });
    }
    ctx_sep(&v);
    // indir
    {
        let toast_c = toast.clone();
        let win_c = win.clone();
        let (tn, ts, te, ten) = (dl.0.clone(), dl.1, dl.2, dl.3.clone());
        let cur = cur_url.clone();
        ctx_row(&v, pop, "Bölümü indir", "folder-download-symbolic", cur_url.is_some(), None, move || {
            match cur.as_deref() {
                Some(u) => start_download(&win_c, &toast_c, u, &tn, ts, te, &ten),
                None => toast_in(&toast_c, "Önce kaynağın açılmasını bekle", 3),
            }
        });
    }

    pop.set_child(Some(&v));
}

fn fmt_bytes(b: u64) -> String {
    let mb = b as f64 / 1048576.0;
    if mb >= 1024.0 {
        format!("{:.2} GB", mb / 1024.0)
    } else {
        format!("{mb:.0} MB")
    }
}

/// Kaynak/kalite seçici popover içeriği. Boyutlar worker ile dolar.
/// Birleşik Ayarlar menüsü (Kalite & Kaynak, Hız, Ses & Altyazı, Focus Mod, Otomatik Bölüm).
fn rebuild_settings_pop(
    pop: &gtk::Popover,
    state: &Rc<RefCell<PlayerState>>,
    client: &Arc<Client>,
    toast: &adw::ToastOverlay,
    status: &gtk::Label,
    src_lbl: &gtk::Label,
) {
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.set_max_content_height(460);
    scroll.set_min_content_width(300);
    scroll.set_propagate_natural_height(true);

    let v = gtk::Box::new(gtk::Orientation::Vertical, 4);
    v.set_margin_top(8);
    v.set_margin_bottom(8);
    v.set_margin_start(10);
    v.set_margin_end(10);

    // 1. Kalite & Kaynak
    v.append(&section_label("Kalite & Kaynak"));
    let res_text = match state.borrow().player.video_height() {
        Some(h) if h > 0 => format!("📺 Oynatılan Çözünürlük: {h}p"),
        _ => "📺 Çözünürlük: Otomatik".to_string(),
    };
    let res_lbl = gtk::Label::new(Some(&res_text));
    res_lbl.add_css_class("dim-label");
    res_lbl.add_css_class("caption");
    res_lbl.set_xalign(0.0);
    v.append(&res_lbl);

    let (sources, cur) = {
        let s = state.borrow();
        (s.sources.clone(), s.index)
    };
    let sizes = state.borrow().sizes.clone().lock().unwrap().clone();
    for (i, src) in sources.iter().enumerate() {
        let host = api::Client::source_host_hint(src.hint_url());
        let host = if host.is_empty() { "Kaynak" } else { host };
        let size_txt = match sizes.get(i).copied().flatten() {
            Some(b) if b > 0 => format!(" · {}", fmt_bytes(b)),
            _ => match src {
                Source::Direct(_) => String::new(),
                Source::Embed(_) => " · yedek".to_string(),
            },
        };
        let mark = if i == cur { "✓ " } else { "   " };
        let rec = if i == 0 { " (Önerilen)" } else { "" };
        let b = gtk::Button::with_label(&format!("{mark}Kaynak {} · {host}{rec}{size_txt}", i + 1));
        b.add_css_class("flat");
        if let Some(lbl) = b.child().and_downcast::<gtk::Label>() {
            lbl.set_xalign(0.0);
        }
        let st2 = state.clone();
        let client2 = client.clone();
        let toast2 = toast.clone();
        let status2 = status.clone();
        let src2 = src_lbl.clone();
        let pop2 = pop.clone();
        b.connect_clicked(move |_| {
            pop2.popdown();
            load_source_at(&st2, &client2, &toast2, &status2, &src2, i);
        });
        v.append(&b);
    }

    // 2. Oynatma Hızı
    v.append(&section_label("Oynatma Hızı"));
    let speed_box = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    let cur_speed = state.borrow().player.speed();
    for (lbl, val) in [
        ("0.5×", 0.5),
        ("0.75×", 0.75),
        ("1×", 1.0),
        ("1.25×", 1.25),
        ("1.5×", 1.5),
        ("2×", 2.0),
    ] {
        let b = gtk::Button::with_label(lbl);
        b.add_css_class("flat");
        b.set_hexpand(true);
        if (cur_speed - val).abs() < 0.05 {
            b.add_css_class("suggested-action");
        }
        let st2 = state.clone();
        let pop2 = pop.clone();
        let cl2 = client.clone();
        let to2 = toast.clone();
        let sta2 = status.clone();
        let sr2 = src_lbl.clone();
        b.connect_clicked(move |_| {
            if let Ok(s) = st2.try_borrow() {
                s.player.set_speed(val);
            }
            rebuild_settings_pop(&pop2, &st2, &cl2, &to2, &sta2, &sr2);
        });
        speed_box.append(&b);
    }
    v.append(&speed_box);

    // 3. Ses & Altyazı
    let tracks = state.borrow().player.tracks();
    let audios: Vec<_> = tracks.iter().filter(|t| t.kind == "audio").collect();
    let subs: Vec<_> = tracks.iter().filter(|t| t.kind == "sub").collect();

    if !audios.is_empty() {
        v.append(&section_label("Ses Parçası"));
        for t in audios {
            let id = t.id;
            let mark = if t.selected { "✓ " } else { "   " };
            let b = gtk::Button::with_label(&format!("{mark}{}", t.label()));
            b.add_css_class("flat");
            if let Some(lbl) = b.child().and_downcast::<gtk::Label>() {
                lbl.set_xalign(0.0);
            }
            let st2 = state.clone();
            let pop2 = pop.clone();
            let cl2 = client.clone();
            let to2 = toast.clone();
            let sta2 = status.clone();
            let sr2 = src_lbl.clone();
            b.connect_clicked(move |_| {
                if let Ok(s) = st2.try_borrow() {
                    s.player.set_audio(id);
                }
                rebuild_settings_pop(&pop2, &st2, &cl2, &to2, &sta2, &sr2);
            });
            v.append(&b);
        }
    }

    if !subs.is_empty() {
        v.append(&section_label("Altyazı"));
        let any_sub = subs.iter().any(|t| t.selected);
        let off_mark = if !any_sub { "✓ " } else { "   " };
        let off_b = gtk::Button::with_label(&format!("{off_mark}Kapalı"));
        off_b.add_css_class("flat");
        if let Some(lbl) = off_b.child().and_downcast::<gtk::Label>() {
            lbl.set_xalign(0.0);
        }
        let st2 = state.clone();
        let pop2 = pop.clone();
        let cl2 = client.clone();
        let to2 = toast.clone();
        let sta2 = status.clone();
        let sr2 = src_lbl.clone();
        off_b.connect_clicked(move |_| {
            if let Ok(s) = st2.try_borrow() {
                s.player.set_sub(None);
            }
            rebuild_settings_pop(&pop2, &st2, &cl2, &to2, &sta2, &sr2);
        });
        v.append(&off_b);

        for t in subs {
            let id = t.id;
            let mark = if t.selected { "✓ " } else { "   " };
            let b = gtk::Button::with_label(&format!("{mark}{}", t.label()));
            b.add_css_class("flat");
            if let Some(lbl) = b.child().and_downcast::<gtk::Label>() {
                lbl.set_xalign(0.0);
            }
            let st2 = state.clone();
            let pop2 = pop.clone();
            let cl2 = client.clone();
            let to2 = toast.clone();
            let sta2 = status.clone();
            let sr2 = src_lbl.clone();
            b.connect_clicked(move |_| {
                if let Ok(s) = st2.try_borrow() {
                    s.player.set_sub(Some(id));
                }
                rebuild_settings_pop(&pop2, &st2, &cl2, &to2, &sta2, &sr2);
            });
            v.append(&b);
        }
    }

    // 4. Focus Mod & Otomatik Bölüm
    v.append(&section_label("Oynatma Seçenekleri"));
    let (cur_focus, cur_autonext) = {
        let s = state.borrow();
        (s.focus_mode, s.auto_next_episode)
    };

    let focus_mark = if cur_focus { "✓ " } else { "   " };
    let focus_btn = gtk::Button::with_label(&format!("{focus_mark}Focus Mod (İntro/Outro Atla & Oto Bölüm)"));
    focus_btn.add_css_class("flat");
    if cur_focus {
        focus_btn.add_css_class("suggested-action");
    }
    if let Some(lbl) = focus_btn.child().and_downcast::<gtk::Label>() {
        lbl.set_xalign(0.0);
    }
    focus_btn.set_tooltip_text(Some("İntro ve outro'ları otomatik atlar, bölüm bitince sonrakine geçer"));
    let st_f = state.clone();
    let pop_f = pop.clone();
    let cl_f = client.clone();
    let to_f = toast.clone();
    let sta_f = status.clone();
    let sr_f = src_lbl.clone();
    focus_btn.connect_clicked(move |_| {
        let new_val = if let Ok(mut s) = st_f.try_borrow_mut() {
            s.focus_mode = !s.focus_mode;
            let mut set = cl_f.load_settings();
            set.focus_mode = s.focus_mode;
            cl_f.save_settings(&set);
            s.focus_mode
        } else {
            false
        };
        let t = adw::Toast::new(if new_val { "⚡ Focus Mod Açık: İntro/outro otomatik atlanacak" } else { "Focus Mod Kapalı" });
        t.set_timeout(2);
        to_f.add_toast(t);
        rebuild_settings_pop(&pop_f, &st_f, &cl_f, &to_f, &sta_f, &sr_f);
    });
    v.append(&focus_btn);

    let auto_mark = if cur_autonext { "✓ " } else { "   " };
    let auto_btn = gtk::Button::with_label(&format!("{auto_mark}Otomatik Bölüm Atlama"));
    auto_btn.add_css_class("flat");
    if cur_autonext {
        auto_btn.add_css_class("suggested-action");
    }
    if let Some(lbl) = auto_btn.child().and_downcast::<gtk::Label>() {
        lbl.set_xalign(0.0);
    }
    auto_btn.set_tooltip_text(Some("Bölüm bitince bir sonraki bölüme otomatik geçer"));
    let st_a = state.clone();
    let pop_a = pop.clone();
    let cl_a = client.clone();
    let to_a = toast.clone();
    let sta_a = status.clone();
    let sr_a = src_lbl.clone();
    auto_btn.connect_clicked(move |_| {
        let new_val = if let Ok(mut s) = st_a.try_borrow_mut() {
            s.auto_next_episode = !s.auto_next_episode;
            let mut set = cl_a.load_settings();
            set.auto_next_episode = s.auto_next_episode;
            cl_a.save_settings(&set);
            s.auto_next_episode
        } else {
            false
        };
        let t = adw::Toast::new(if new_val { "Otomatik Bölüm Atlama Açık" } else { "Otomatik Bölüm Atlama Kapalı" });
        t.set_timeout(2);
        to_a.add_toast(t);
        rebuild_settings_pop(&pop_a, &st_a, &cl_a, &to_a, &sta_a, &sr_a);
    });
    v.append(&auto_btn);

    scroll.set_child(Some(&v));
    pop.set_child(Some(&scroll));
}

/// İkon temasında var olan ilk adı seçer (eksik temalarda kırık ikon çıkmaz).
fn pick_icon(display: &gtk::gdk::Display, candidates: &[&str]) -> String {
    let theme = gtk::IconTheme::for_display(display);
    for c in candidates {
        if theme.has_icon(c) {
            return c.to_string();
        }
    }
    candidates.last().unwrap_or(&"emblem-system-symbolic").to_string()
}

fn vol_icon(volume: f64, muted: bool) -> &'static str {    if muted || volume <= 0.0 {
        "audio-volume-muted-symbolic"
    } else if volume < 35.0 {
        "audio-volume-low-symbolic"
    } else if volume < 70.0 {
        "audio-volume-medium-symbolic"
    } else {
        "audio-volume-high-symbolic"
    }
}

/// Gömülü oynatıcı sayfa içeriğini kurar.
/// `None` dönerse mpv başlatılamamıştır (çağıran toast bassın).
pub fn build_embedded_player(
    main_window: &adw::ApplicationWindow,
    header: &adw::HeaderBar,
    sidebar: &gtk::Revealer,
    toast: &adw::ToastOverlay,
    client: &Arc<Client>,
    progress: &Rc<RefCell<HashMap<String, (f64, f64)>>>,
    progress_bars: &Rc<RefCell<HashMap<String, (gtk::ProgressBar, gtk::Label)>>>,
    req: EmbedRequest,
) -> Option<EmbeddedPlayer> {
    let media_title = format!(
        "{} | S{:02}E{:02}",
        req.title.name, req.ep.season, req.ep.episode
    );
    eprintln!("[EMBED] gömülü oynatıcı açılıyor: {media_title}");

    let mut sources: Vec<Source> = req.candidates.iter().cloned().map(Source::Direct).collect();
    sources.extend(req.fallback_embeds.iter().cloned().map(Source::Embed));
    if sources.is_empty() {
        eprintln!("[EMBED] oynatılacak kaynak yok");
        return None;
    }

    let opts = upscale_opts(&req.upscale);
    let opts_ref: Vec<(&str, &str)> = opts.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let player = match MpvEmbed::new(proxy_url().as_deref(), req.saved_pos, &opts_ref) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[EMBED] mpv init başarısız: {e}");
            return None;
        }
    };
    let start_volume = player.volume();
    let start_muted = player.muted();

    // ---- sayfa içeriği: video + üzerine yüzen ince kontrol barı ----
    let vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);
    vbox.set_hexpand(true);
    vbox.set_vexpand(true);

    // şeffaf alt gölge (scrim) stili — video üzerinde yüzen ince bar
    {
        let css = gtk::CssProvider::new();
        css.load_from_string(
            ".embed-scrim { background: linear-gradient(to top, rgba(0,0,0,0.85), rgba(0,0,0,0.45) 60%, rgba(0,0,0,0.0)); } \
             .embed-controls button { min-width: 30px; min-height: 30px; } \
             .embed-controls scale { padding: 0 2px; } \
             .embed-controls scale trough { min-height: 6px; border-radius: 3px; } \
             .embed-controls scale highlight { min-height: 6px; border-radius: 3px; } \
             .embed-controls scale slider { opacity: 0; } \
             .embed-controls scale:hover slider { opacity: 1; } \
             .embed-controls scale.seek-hot trough { min-height: 8px; border-radius: 4px; } \
             .embed-controls scale.seek-hot highlight { min-height: 8px; border-radius: 4px; } \
             .embed-controls scale.seek-hot slider { opacity: 1; min-width: 18px; min-height: 18px; } \
             scale.vol-scale slider, \
             scale.vol-scale > trough > slider, \
             .vol-scale slider, \
             .vol-scale > trough > slider, \
             .vol-pop scale slider, \
             .vol-pop scale > trough > slider { \
                 opacity: 1; \
                 min-width: 16px; \
                 min-height: 16px; \
                 border-radius: 9999px; \
                 background-color: #ffffff; \
                 background: #ffffff; \
                 border: 1px solid rgba(0, 0, 0, 0.3); \
                 box-shadow: 0 1px 4px rgba(0, 0, 0, 0.6); \
                 margin: -5px 0; \
             } \
             .vol-scale trough, .vol-pop scale trough { min-height: 6px; border-radius: 3px; } \
             .vol-scale highlight, .vol-pop scale highlight { min-height: 6px; border-radius: 3px; } \
             .seek-prev { background: rgba(10,10,14,0.88); border-radius: 10px; padding: 6px; } \
             .ep-current { border: 2px solid @accent_color; border-radius: 10px; } \
             .embed-status { font-size: 0.78em; } \
             .embed-flash { background: rgba(10,10,14,0.62); border-radius: 999px; padding: 18px; } \
             .embed-back { background: transparent; color: white; } \
             .embed-back:hover { background: rgba(255,255,255,0.14); } \
             .embed-title { color: white; background: rgba(0,0,0,0.38); border-radius: 10px; padding: 2px 14px; } \
             .embed-topbar { background: linear-gradient(to bottom, rgba(0,0,0,0.8) 0%, rgba(0,0,0,0.0) 100%); border-radius: 0; } \
              .embed-playerbar { background: transparent; border: none; box-shadow: none; padding: 8px 10px 6px 10px; } \
              .embed-edge { background: rgba(255,255,255,0.38); border-radius: 999px; } \
              .ep-panel { background: rgba(10,10,14,0.90); border-radius: 14px 0 0 14px; padding: 10px; } \
              .ep-panel-title { font-weight: 800; letter-spacing: 0.08em; }",
        );
        gtk::style_context_add_provider_for_display(
            &vbox.display(),
            &css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }

    let stage = gtk::Overlay::new();
    stage.set_hexpand(true);
    stage.set_vexpand(true);
    vbox.append(&stage);

    let gl_area = gtk::GLArea::builder().hexpand(true).vexpand(true).build();
    gl_area.set_required_version(3, 0);
    gl_area.set_focusable(true);
    // Not: kalıcı tooltip yok — video üstünde beliren bilgi baloncuğu
    // oto-gizlemeyi bozuyordu. Kısayollar açılışta bir kez toast ile verilir.
    stage.set_child(Some(&gl_area));

    // alt kontrol barı (revealer içinde, fare boşta kalınca gizlenir)
    let controls = gtk::Box::new(gtk::Orientation::Vertical, 0);
    controls.add_css_class("embed-scrim");
    controls.add_css_class("embed-controls");
    controls.set_valign(gtk::Align::End);

    let bar = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    bar.set_margin_top(20);
    bar.set_margin_bottom(2);
    bar.set_margin_start(8);
    bar.set_margin_end(8);
    let play_btn = gtk::Button::from_icon_name("media-playback-pause-symbolic");
    play_btn.set_tooltip_text(Some("Oynat / Duraklat (Boşluk / tıkla)"));
    for b in [&play_btn] {
        b.add_css_class("flat");
        b.add_css_class("circular");
    }
    let seek = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 1000.0, 1.0);
    seek.set_hexpand(true);
    seek.set_draw_value(false);
    seek.set_tooltip_text(Some("İlerleme çubuğu — sürükle"));
    let time_lbl = gtk::Label::new(Some("--:-- / --:--"));

    // --- ses: MenuButton + Popover (tıklayınca YUKARI doğru açılır) ---
    let vol_btn = gtk::MenuButton::builder()
        .icon_name(vol_icon(start_volume, start_muted))
        .tooltip_text("Ses seviyesi (M)")
        .build();
    vol_btn.add_css_class("flat");
    vol_btn.add_css_class("circular");
    let vol_pop = gtk::Popover::builder()
        .position(gtk::PositionType::Top)
        .build();
    vol_pop.add_css_class("vol-pop");
    let vol_scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 100.0, 1.0);
    vol_scale.add_css_class("vol-scale");
    vol_scale.set_value(start_volume);
    vol_scale.set_draw_value(false);
    vol_scale.set_size_request(130, 36);
    vol_scale.set_margin_top(8);
    vol_scale.set_margin_bottom(8);
    vol_scale.set_margin_start(12);
    vol_scale.set_margin_end(12);
    vol_pop.set_child(Some(&vol_scale));
    vol_btn.set_popover(Some(&vol_pop));

    let fs_btn = gtk::Button::from_icon_name("view-fullscreen-symbolic");
    fs_btn.set_tooltip_text(Some("Tam ekran (F / çift tık)"));
    fs_btn.add_css_class("flat");
    fs_btn.add_css_class("circular");

    let settings_btn = gtk::MenuButton::builder()
        .icon_name("emblem-system-symbolic")
        .tooltip_text("Ayarlar (Kalite, Hız, Altyazı, Focus)")
        .build();
    settings_btn.add_css_class("flat");
    settings_btn.add_css_class("circular");
    let settings_pop = gtk::Popover::new();
    settings_btn.set_popover(Some(&settings_pop));
    let src_lbl = gtk::Label::new(None);

    bar.append(&play_btn);
    bar.append(&vol_btn);
    bar.append(&seek);
    let rightbox = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    rightbox.append(&time_lbl);
    rightbox.append(&settings_btn);
    rightbox.append(&fs_btn);
    bar.append(&rightbox);
    controls.append(&bar);

    let status = gtk::Label::new(Some("yükleniyor…"));
    // Sol alt durum metni kaldırıldı (bilgiler toast ile veriliyor).
    status.set_visible(false);

    // alt bar revealer ile sahneye gömülür (oto-gizleme için)
    let controls_revealer = gtk::Revealer::builder()
        .transition_type(gtk::RevealerTransitionType::SlideUp)
        .transition_duration(220)
        .child(&controls)
        .build();
    controls_revealer.set_reveal_child(true);
    controls_revealer.set_valign(gtk::Align::End);
    controls_revealer.set_hexpand(true);
    stage.add_overlay(&controls_revealer);

    // ortada belirip kaybolan büyük oynat/duraklat ikonu
    let flash_img = gtk::Image::builder()
        .icon_name("media-playback-start-symbolic")
        .pixel_size(64)
        .build();
    let flash_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    flash_box.add_css_class("embed-flash");
    flash_box.append(&flash_img);
    let flash_rev = gtk::Revealer::builder()
        .transition_type(gtk::RevealerTransitionType::Crossfade)
        .transition_duration(180)
        .child(&flash_box)
        .build();
    flash_rev.set_reveal_child(false);
    flash_rev.set_halign(gtk::Align::Center);
    flash_rev.set_valign(gtk::Align::Center);
    stage.add_overlay(&flash_rev);

    // sağ altta beliren "İntroyu Atla" butonu
    let skip_btn = gtk::Button::with_label("⏩ İntroyu Atla");
    skip_btn.add_css_class("suggested-action");
    skip_btn.add_css_class("pill");
    let skip_rev = gtk::Revealer::builder()
        .transition_type(gtk::RevealerTransitionType::Crossfade)
        .transition_duration(220)
        .child(&skip_btn)
        .build();
    skip_rev.set_reveal_child(false);
    skip_rev.set_halign(gtk::Align::End);
    skip_rev.set_valign(gtk::Align::End);
    skip_rev.set_margin_end(16);
    skip_rev.set_margin_bottom(110);
    stage.add_overlay(&skip_rev);

    // üst yüzen bar: GERÇEK HeaderBar, şeffaf + çizgisiz, videonun üstünde.
    // Native header player sayfasında gizlidir; bu bar onun yerini tutar
    // ve kromla birlikte gizlenip açılır. Yerli pencere düğmeleriyle gelir.
    let top_title = gtk::Label::new(Some(&media_title));
    top_title.add_css_class("title-4");
    top_title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    let top_back = gtk::Button::from_icon_name("go-previous-symbolic");
    top_back.add_css_class("flat");
    top_back.add_css_class("circular");
    top_back.set_tooltip_text(Some("Geri"));
    {
        let cb = req.on_back.clone();
        top_back.connect_clicked(move |_| {
            if let Some(f) = cb.as_ref() {
                f();
            }
        });
    }
    let player_bar = adw::HeaderBar::builder()
        .title_widget(&top_title)
        .show_start_title_buttons(false)
        .show_end_title_buttons(true)
        .build();
    player_bar.pack_start(&top_back);
    player_bar.add_css_class("embed-playerbar");
    // Üst bar pencere tutamacı: düğmeler çalışır, boş alandan sürükleyince
    // pencere taşınır (native header gizli olduğu için şart).
    player_bar.set_hexpand(true);
    let drag_handle = gtk::WindowHandle::builder()
        .child(&player_bar)
        .hexpand(true)
        .build();
    drag_handle.add_css_class("embed-topbar");
    // Gradyan tam kanala otursun: dış boşluk yok, iç dolgu headerbar'da.
    drag_handle.set_margin_top(0);
    drag_handle.set_margin_start(0);
    drag_handle.set_margin_end(0);
    // Başlığa çift tık: büyüt / eski boyut.
    {
        let w = main_window.clone();
        let title_click = gtk::GestureClick::new();
        title_click.connect_pressed(move |_, n_press, _, _| {
            if n_press == 2 {
                if w.is_maximized() {
                    w.unmaximize();
                } else {
                    w.maximize();
                }
            }
        });
        top_title.add_controller(title_click);
    }
    let topbar = gtk::Revealer::builder()
        .transition_type(gtk::RevealerTransitionType::SlideDown)
        .transition_duration(220)
        .child(&drag_handle)
        .build();
    topbar.set_reveal_child(true);
    topbar.set_valign(gtk::Align::Start);
    topbar.set_hexpand(true);
    stage.add_overlay(&topbar);


    // ---- bölüm paneli: sağda yüzen overlay (kendi alanı yok, sayfayı itmez).
    // Arka plan yarı saydam koyu (GTK CSS'te blur yok).
    let ep_panel = gtk::Box::new(gtk::Orientation::Vertical, 8);
    ep_panel.add_css_class("ep-panel");
    ep_panel.set_size_request(300, -1);
    ep_panel.set_margin_top(12);
    ep_panel.set_margin_bottom(12);
    let ep_head = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let ep_title = gtk::Label::new(Some("BÖLÜMLER"));
    ep_title.add_css_class("ep-panel-title");
    ep_title.set_xalign(0.0);
    ep_title.set_hexpand(true);
    ep_head.append(&ep_title);
    ep_panel.append(&ep_head);
    // Sezon seçici (ekrandaki gibi: "SEZON: 2. Sezon").
    let mut seasons: Vec<u64> = req.episodes.iter().map(|e| e.season).collect();
    seasons.sort_unstable();
    seasons.dedup();
    if seasons.is_empty() {
        seasons.push(req.ep.season.max(1));
    }
    // Sezon sekmeleri (kompakt): dropdown yerine panel-içi sekmeler
    // (odak kaybında anında kapanmayla uyumlu — popover yok).
    let season_bar = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    season_bar.set_halign(gtk::Align::Start);
    ep_panel.append(&season_bar);
    let ep_scroll = gtk::ScrolledWindow::new();
    ep_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    ep_scroll.set_vexpand(true);
    ep_scroll.set_hexpand(true);
    let ep_list = gtk::Box::new(gtk::Orientation::Vertical, 4);
    ep_list.set_hexpand(true);
    ep_scroll.set_child(Some(&ep_list));
    ep_panel.append(&ep_scroll);
    let ep_rev = gtk::Revealer::builder()
        .transition_type(gtk::RevealerTransitionType::SlideLeft)
        .transition_duration(220)
        .child(&ep_panel)
        .build();
    ep_rev.set_reveal_child(false);
    ep_rev.set_halign(gtk::Align::End);
    ep_rev.set_valign(gtk::Align::Fill);
    ep_rev.set_vexpand(true);
    stage.add_overlay(&ep_rev);

    // Liste kurucu: seçili sezonun bölümleri (o anki vurgulu).
    let ep_all = req.episodes.clone();
    let ep_cur_s = req.ep.season;
    let ep_cur_e = req.ep.episode;
    let ep_cb = req.play_episode.clone();
    let rebuild_eps: Rc<dyn Fn(u64)> = {
        let ep_list = ep_list.clone();
        let ep_rev_c = ep_rev.clone();
        let ep_all = ep_all.clone();
        let ep_cb = ep_cb.clone();
        let client_c = client.clone();
        let poster_fb = req.title.poster.clone();
        let runtime_min = req.title.runtime;
        Rc::new(move |season: u64| {
            let mut cur = ep_list.first_child();
            while let Some(child) = cur {
                let next = child.next_sibling();
                ep_list.remove(&child);
                cur = next;
            }
            for e in ep_all.iter().filter(|e| e.season == season) {
                let is_cur = e.season == ep_cur_s && e.episode == ep_cur_e;
                let b = gtk::Button::new();
                b.add_css_class("flat");
                b.set_halign(gtk::Align::Fill);
                let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
                row.set_hexpand(true);
                // Sol önizleme karesi (bölüm kapağı, yoksa başlık posteri).
                let thumb = gtk::Picture::new();
                thumb.set_size_request(96, 54);
                thumb.set_can_shrink(false);
                thumb.set_content_fit(gtk::ContentFit::Cover);
                thumb.set_valign(gtk::Align::Center);
                thumb.set_halign(gtk::Align::Start);
                load_thumb_async(&client_c, e.thumbnail.clone().or(poster_fb.clone()), &thumb, 96, 54);
                row.append(&thumb);
                let vb = gtk::Box::new(gtk::Orientation::Vertical, 2);
                vb.set_hexpand(true);
                vb.set_valign(gtk::Align::Center);
                let nm = gtk::Label::new(Some(&e.name));
                nm.set_xalign(0.0);
                nm.set_wrap(true);
                nm.set_lines(2);
                nm.set_ellipsize(gtk::pango::EllipsizeMode::End);
                nm.set_hexpand(true);
                vb.append(&nm);
                let mut sub = format!("S{:02}E{:02}", e.season, e.episode);
                if let Some(rt) = runtime_min {
                    sub.push_str(&format!(" · {rt} dk"));
                }
                let meta = gtk::Label::new(Some(&sub));
                meta.add_css_class("dim-label");
                meta.set_xalign(0.0);
                vb.append(&meta);
                row.append(&vb);
                b.set_child(Some(&row));
                if is_cur {
                    b.add_css_class("ep-current");
                    b.set_sensitive(false);
                    b.set_tooltip_text(Some("Şu an izlenen bölüm"));
                } else {
                    b.set_tooltip_text(Some("Bu bölümü oynat"));
                }
                let ep_c = e.clone();
                let cb_c = ep_cb.clone();
                let rev_c = ep_rev_c.clone();
                b.connect_clicked(move |_| {
                    rev_c.set_reveal_child(false);
                    if let Some(cb) = cb_c.as_ref() {
                        cb(ep_c.clone());
                    }
                });
                ep_list.append(&b);
            }
            })
    };
    rebuild_eps(req.ep.season);
    {
        let rebuild_c = rebuild_eps.clone();
        for s in seasons.clone() {
            let b = gtk::Button::with_label(&format!("S{s}"));
            b.add_css_class("season-tab");
            if s == req.ep.season {
                b.add_css_class("suggested-action");
            }
            let rebuild_b = rebuild_c.clone();
            let bar_c = season_bar.clone();
            b.connect_clicked(move |btn| {
                rebuild_b(s);
                // Sekme vurgusu: seçiliyi işaretle.
                let mut cur = bar_c.first_child();
                while let Some(ch) = cur {
                    cur = ch.next_sibling();
                    if let Some(bb) = ch.downcast_ref::<gtk::Button>() {
                        bb.remove_css_class("suggested-action");
                    }
                }
                btn.add_css_class("suggested-action");
            });
            season_bar.append(&b);
        }
    }
    // Sağ kenar hover şeridi: bölümler panelini açar.
    let ep_pill = gtk::Box::new(gtk::Orientation::Vertical, 0);
    ep_pill.add_css_class("embed-edge");
    ep_pill.set_size_request(4, 110);
    ep_pill.set_halign(gtk::Align::Center);
    ep_pill.set_valign(gtk::Align::Center);
    let ep_zone = gtk::Box::new(gtk::Orientation::Vertical, 0);
    ep_zone.set_size_request(16, 220);
    ep_zone.set_halign(gtk::Align::End);
    ep_zone.set_valign(gtk::Align::Center);
    ep_zone.set_hexpand(false);
    ep_zone.append(&ep_pill);
    let ep_strip = gtk::Revealer::builder()
        .transition_type(gtk::RevealerTransitionType::SlideLeft)
        .transition_duration(180)
        .child(&ep_zone)
        .build();
    ep_strip.set_reveal_child(true);
    ep_strip.set_halign(gtk::Align::End);
    ep_strip.set_valign(gtk::Align::Center);
    ep_strip.set_hexpand(false);
    // Tek bölümlük içerikte (film) panel/şerit gereksiz.
    ep_strip.set_visible(req.episodes.len() > 1);
    stage.add_overlay(&ep_strip);
    {
        let rev_c = ep_rev.clone();
        let enter = gtk::EventControllerMotion::new();
        enter.connect_enter(move |_, _, _| {
            rev_c.set_reveal_child(true);
        });
        ep_zone.add_controller(enter);
    }
    // Odak kaybında anında kapanır (sezon sekmeleri panel içinde,
    // popover olmadığı için seçim yarıda kesilmez).
    {
        let rev_c = ep_rev.clone();
        let leave_motion = gtk::EventControllerMotion::new();
        leave_motion.connect_leave(move |_| {
            rev_c.set_reveal_child(false);
        });
        ep_panel.add_controller(leave_motion);
    }

    // ---- paylaşılan durum (main thread) ----
    let alive = Arc::new(AtomicBool::new(true));
    let prog_key = format!("{}:{}:{}", req.title.id, req.ep.season, req.ep.episode);
    let sources_len_for_sizes = sources.len();
    let state = Rc::new(RefCell::new(PlayerState {
        player,
        render_ctx: None,
        sources,
        index: 0,
        load_started: Instant::now(),
        media_loaded: false,
        playing_shown: false,
        resolving: false,
        resolve_result: Arc::new(Mutex::new(None)),
        aniskip: Arc::new(Mutex::new(AniSkipTimes::default())),
        op_prompted: false,
        ed_prompted: false,
        op_seek_done: false,
        ed_seek_done: false,
        focus_mode: client.load_settings().focus_mode,
        auto_next_episode: client.load_settings().auto_next_episode,
        next_ep: req.next_ep.clone(),
        play_episode: req.play_episode.clone(),
        next_triggered: false,
        watched_marked: false,
        host_saved: false,
        alive: alive.clone(),
        seeking: Rc::new(Cell::new(false)),
        vol_changing: Rc::new(Cell::new(false)),
        prog_key,
        season: req.ep.season,
        episode: req.ep.episode,
        client: client.clone(),
        progress: progress.clone(),
        main_window: main_window.clone(),
        header: header.clone(),
        sidebar: sidebar.clone(),
        gl_area: gl_area.clone(),
        last_motion: Instant::now(),
        last_mx: -1.0,
        last_my: -1.0,
        hidden_at: None,
        hide_mx: -1.0,
        hide_my: -1.0,
        cursor_hidden: false,
        topbar: topbar.clone(),
        chrome_hidden: false,
        we_fullscreened: false,
        click_pending: Rc::new(Cell::new(false)),
        header_motion: None,
        flash_img: flash_img.clone(),
        flash_rev: flash_rev.clone(),
        sizes: Arc::new(Mutex::new(vec![None; sources_len_for_sizes])),
        last_report: Instant::now(),
    }));
    let _ = &req.fast_embeds;

    // ---- AniSkip arka plan fetch ----
    if req.aniskip_enabled {
        let client_c = client.clone();
        let name_c = req.title.name.clone();
        let ep_c = req.ep.episode;
        let shared_c = state.borrow().aniskip.clone();
        let alive_c = alive.clone();
        std::thread::spawn(move || {
            const MAX_TRIES: u32 = 5;
            for attempt in 0..MAX_TRIES {
                if !alive_c.load(Ordering::Relaxed) {
                    return;
                }
                let t = client_c.fetch_aniskip_timestamps(&name_c, ep_c);
                if t.op_end.is_some() || t.ed_end.is_some() {
                    *shared_c.lock().unwrap() = t;
                    eprintln!("[EMBED] AniSkip hazır");
                    return;
                }
                if attempt + 1 < MAX_TRIES {
                    std::thread::sleep(Duration::from_secs(45));
                }
            }
            eprintln!("[EMBED] AniSkip bulunamadı");
        });
    }

    // ---- GLArea realize/render/unrealize ----
    let needs_render = Arc::new(AtomicBool::new(false));
    {
        let st_c = state.clone();
        let flag_c = needs_render.clone();
        let status_c = status.clone();
        gl_area.connect_realize(move |area| {
            area.make_current();
            if let Some(err) = area.error() {
                eprintln!("[EMBED] GLArea OpenGL hatası: {err}");
            }
            let mut s = st_c.borrow_mut();
            // shutdown edilmiş bir player'a ctx kurma (geri dönüşte
            // yeni handle zaten yeni state ile gelir).
            if !s.alive.load(Ordering::Relaxed) {
                return;
            }
            // sayfaya dönüşte ctx yeniden kurulur
            if s.render_ctx.is_none() {
                match s.player.create_render_context() {
                    Ok(mut ctx) => {
                        let flag = flag_c.clone();
                        let alive_cb = s.alive.clone();
                        ctx.set_update_callback(move || {
                            if alive_cb.load(Ordering::Relaxed) {
                                flag.store(true, Ordering::Relaxed);
                            }
                        });
                        s.render_ctx = Some(ctx);
                        eprintln!("[EMBED] render ctx hazır");
                    }
                    Err(e) => {
                        status_c.set_text(&format!("Render hatası: {e}"));
                        eprintln!("[EMBED] render ctx hatası: {e}");
                    }
                }
            }
        });
    }
    {
        let flag_c = needs_render.clone();
        let weak = gl_area.downgrade();
        let alive_c = alive.clone();
        glib::timeout_add_local(Duration::from_millis(8), move || {
            if !alive_c.load(Ordering::Relaxed) {
                return glib::ControlFlow::Break;
            }
            if flag_c.swap(false, Ordering::Relaxed) {
                if let Some(a) = weak.upgrade() {
                    a.queue_render();
                }
            }
            glib::ControlFlow::Continue
        });
    }
    {
        let st_c = state.clone();
        let alive_c = alive.clone();
        gl_area.connect_render(move |area, _ctx| {
            // shutdown sonrası GLArea stack'ten düşerken gelen son kareler
            // ölü ctx'e çizilmesin; yoksa sürücü çöker (NVIDIA'da sarı ekran).
            if !alive_c.load(Ordering::Relaxed) {
                return glib::Propagation::Stop;
            }
            let s = st_c.borrow();
            if let Some(ctx) = s.render_ctx.as_ref() {
                let fbo = current_fbo();
                if let Err(e) = ctx.render(fbo, area.width(), area.height(), true) {
                    eprintln!("[EMBED] render hatası: {e}");
                }
                ctx.report_swap();
            }
            glib::Propagation::Stop
        });
    }
    {
        let st_c = state.clone();
        gl_area.connect_unrealize(move |_| {
            st_c.borrow_mut().render_ctx = None;
        });
    }

    // ---- kaynak yükleme ----
    // Native header player sayfasında gizlidir; üst yüzen bar onun yerindedir.
    // (Sayfadan çıkınca shutdown + show_page garantisiyle geri gelir.)
    header.set_visible(false);
    let total_sources = state.borrow().sources.len();
    update_src_label(&src_lbl, 0, total_sources, state.borrow().sources[0].hint_url());
    load_source_at(&state, client, toast, &status, &src_lbl, 0);
    toast_in(
        toast,
        "Tıkla: oynat/duraklat • Çift tık: tam ekran • Boşluk ←/→ S/E M F",
        5,
    );
    // klavye odağı videoda başlasın
    {
        let area_c = gl_area.clone();
        glib::timeout_add_local_once(Duration::from_millis(400), move || {
            area_c.grab_focus();
        });
    }
    // ayar açıksa tam ekranda başla (harici mpv'deki davranışla aynı)
    if req.auto_fullscreen {
        set_fullscreen(&state, &controls_revealer, true);
    }

    // ---- 500ms supervisor tick ----
    {
        let st_c = state.clone();
        let client_c = client.clone();
        let progress_c = progress.clone();
        let progress_bars_c = progress_bars.clone();
        let toast_c = toast.clone();
        let status_c = status.clone();
        let src_lbl_c = src_lbl.clone();
        let seek_w = seek.downgrade();
        let lbl_w = time_lbl.downgrade();
        let play_w = play_btn.downgrade();
        let vol_w = vol_scale.downgrade();
        let vol_btn_w = vol_btn.downgrade();
        let rev_w = controls_revealer.downgrade();
        let skip_rev_w = skip_rev.downgrade();
        let skip_btn_c = skip_btn.clone();
        let settings_pop_w = settings_pop.downgrade();
        let vol_pop_w = vol_pop.downgrade();
        let fs_w = fs_btn.downgrade();
        let patience = req.patience_secs.max(10);
        let tid = req.title.id;
        let aniskip_on = req.aniskip_enabled;

        glib::timeout_add_local(Duration::from_millis(500), move || {
            if !alive.load(Ordering::Relaxed) {
                return glib::ControlFlow::Break;
            }
            let mut s = st_c.borrow_mut();
            let Some(seek) = seek_w.upgrade() else { return glib::ControlFlow::Continue };

            // JIT resolve sonucu geldi mi?
            if s.resolving {
                let done = s.resolve_result.lock().unwrap().take();
                if let Some(res) = done {
                    s.resolving = false;
                    match res {
                        Ok(url) => {
                            let idx = s.index;
                            s.sources[idx] = Source::Direct(url.clone());
                            s.load_started = Instant::now();
                            s.media_loaded = false;
                            s.playing_shown = false;
                            if let Some(r) = sibnet_referer(&url) {
                                s.player.set_http_headers(&r);
                            }
                            if let Err(e) = s.player.load_url(&url) {
                                status_c.set_text(&format!("Yükleme hatası: {e}"));
                                drop(s);
                                next_source(&st_c, &client_c, &toast_c, &status_c, &src_lbl_c, None);
                                return glib::ControlFlow::Continue;
                            }
                            update_src_label(&src_lbl_c, idx, s.sources.len(), &url);
                            status_c.set_text("yükleniyor…");
                        }
                        Err(e) => {
                            eprintln!("[EMBED] yedek çözülemedi: {e}");
                            drop(s);
                            next_source(
                                &st_c, &client_c, &toast_c, &status_c, &src_lbl_c,
                                Some("Yedek çözülemedi, diğerine geçiliyor…"),
                            );
                            return glib::ControlFlow::Continue;
                        }
                    }
                } else {
                    return glib::ControlFlow::Continue;
                }
            }

            let pos = s.player.pos();
            let dur = s.player.dur();
            let idle = s.player.idle();
            let eof = s.player.eof();
            if dur > 0.0 {
                if !s.media_loaded {
                    s.media_loaded = true;
                    // Video hazır — oynatmaya başla
                    s.player.set_pause(false);
                    eprintln!("[PLAYER] video hazır, oynatılıyor");
                }
            }
            if s.media_loaded && !eof && !s.playing_shown {
                s.playing_shown = true;
                let host = api::Client::source_host_hint(s.sources[s.index].hint_url());
                let msg = if host.is_empty() {
                    "oynatılıyor ▶".to_string()
                } else {
                    format!("oynatılıyor ▶ · {host}")
                };
                status_c.set_text(&msg);
            }

            // --- AniSkip prompt ---
            if aniskip_on {
                let snap = s.aniskip.lock().unwrap().clone();
                if let Some(st) = snap.op_start {
                    if !s.op_prompted && pos >= (st - 1.5) && pos <= (st + 25.0) {
                        s.op_prompted = true;
                        s.player.show_text("⏩ İntro Başladı ('s' ile atlayabilirsiniz)", 7000);
                        toast_in(&toast_c, "⏩ İntro başladı — 's' ile atla", 4);
                    }
                }
                if let Some(st) = snap.ed_start {
                    if !s.ed_prompted && pos >= (st - 1.5) && pos <= (st + 25.0) {
                        s.ed_prompted = true;
                        s.player.show_text("🏁 Outro Başladı ('e' ile atlayabilirsiniz)", 7000);
                        toast_in(&toast_c, "🏁 Outro başladı — 'e' ile atla", 4);
                    }
                }
            }

            // --- EOF: erken bittiyse bozuk kaynak, yoksa normal bitiş ---
            if eof {
                let broken = dur > 0.0 && pos < dur * 0.9 && s.load_started.elapsed().as_secs() < 60;
                if broken && s.index + 1 < s.sources.len() {
                    eprintln!("[EMBED] kaynak erken bitti, sonrakine geçiliyor");
                    drop(s);
                    next_source(
                        &st_c, &client_c, &toast_c, &status_c, &src_lbl_c,
                        Some("Kaynak bozuk çıktı, diğerine geçiliyor…"),
                    );
                    return glib::ControlFlow::Continue;
                }
                if !s.watched_marked && dur > 0.0 {
                    s.watched_marked = true;
                    client_c.save_watched(
                        &api::Watched { title_id: tid, episode: s.episode, season: s.season },
                        "",
                    );
                }
                status_c.set_text("bitti ✓");
                return glib::ControlFlow::Continue;
            }

            // --- dead source: medya gelmiyor ---
            if s.resolving {
                // çözüm bekleniyor, sabret
            } else if !s.media_loaded && idle {
                if s.load_started.elapsed().as_secs() >= 25 {
                    eprintln!("[EMBED] kaynak 25sn'de başlamadı, sonrakine");
                    drop(s);
                    next_source(
                        &st_c, &client_c, &toast_c, &status_c, &src_lbl_c,
                        Some("Kaynak açılamadı, diğerine geçiliyor…"),
                    );
                    return glib::ControlFlow::Continue;
                }
            } else if !s.media_loaded {
                let elapsed = s.load_started.elapsed().as_secs();
                if s.player.core_idle() && elapsed >= patience {
                    eprintln!("[EMBED] kaynak ölü (idle, {elapsed}sn), sonrakine");
                    drop(s);
                    next_source(
                        &st_c, &client_c, &toast_c, &status_c, &src_lbl_c,
                        Some("Kaynak oynatamadı, diğerine geçiliyor…"),
                    );
                    return glib::ControlFlow::Continue;
                }
            }

            // --- sinema modu: imleç 1sn, krom 2sn boşta gizlenir ---
            // (Pencerelide de tam ekranda da aynı: üst yüzen bar + alt bar.)
            {
                let pop_open = settings_pop_w.upgrade().map(|p| p.is_visible()).unwrap_or(false)
                    || vol_pop_w.upgrade().map(|p| p.is_visible()).unwrap_or(false);
                if pop_open {
                    s.last_motion = Instant::now();
                }
                let idle = s.last_motion.elapsed();
                if !s.cursor_hidden && idle.as_secs() >= 1 && !pop_open {
                    s.cursor_hidden = true;
                    s.hide_mx = s.last_mx;
                    s.hide_my = s.last_my;
                    s.gl_area.set_cursor_from_name(Some("none"));
                }
                if !s.chrome_hidden && idle.as_secs() >= 2 && !pop_open {
                    if let Some(rv) = rev_w.upgrade() {
                        hide_chrome_locked(&mut s, &rv);
                    }
                }
            }
            // --- introyu-atla butonu ---
            if aniskip_on {
                if let Some((_, is_op)) =
                    skip_target(&s.aniskip.lock().unwrap().clone(), pos, s.op_seek_done, s.ed_seek_done)
                {
                    skip_btn_c.set_label(if is_op {
                        "⏩ İntroyu Atla"
                    } else {
                        "⏩ Outro'yu Atla"
                    });
                    if let Some(r) = skip_rev_w.upgrade() {
                        r.set_reveal_child(true);
                    }
                } else if let Some(r) = skip_rev_w.upgrade() {
                    r.set_reveal_child(false);
                }
            } else if let Some(r) = skip_rev_w.upgrade() {
                r.set_reveal_child(false);
            }
            // --- UI + progress ---
            if let Some(l) = lbl_w.upgrade() {
                l.set_text(&format!("{} / {}", fmt_time(pos), fmt_time(dur)));
            }
            if !s.seeking.get() && dur > 0.0 && pos >= 1.0 {
                seek.set_value((pos / dur * 1000.0).clamp(0.0, 1000.0));
            }
            if let Some(b) = play_w.upgrade() {
                b.set_icon_name(if s.player.paused() {
                    "media-playback-start-symbolic"
                } else {
                    "media-playback-pause-symbolic"
                });
            }
            if let Some(f) = fs_w.upgrade() {
                f.set_icon_name(if s.main_window.is_fullscreen() {
                    "view-restore-symbolic"
                } else {
                    "view-fullscreen-symbolic"
                });
            }
            // ses senkronu (kullanıcı sürüklemiyorsa)
            if !s.vol_changing.get() {
                let v = s.player.volume();
                let m = s.player.muted();
                if let Some(vs) = vol_w.upgrade() {
                    if (vs.value() - v).abs() > 0.5 {
                        vs.set_value(v);
                    }
                }
                if let Some(vb) = vol_btn_w.upgrade() {
                    vb.set_icon_name(vol_icon(v, m));
                }
            }
            if pos >= 1.0 {
                progress_c.borrow_mut().insert(s.prog_key.clone(), (pos, dur));
                if let Some((pb, lbl)) = progress_bars_c.borrow().get(&s.prog_key) {
                    if dur > 0.0 {
                        pb.set_fraction((pos / dur).clamp(0.0, 1.0));
                        lbl.set_text(&format!("{} / {}", fmt_time(pos), fmt_time(dur)));
                        lbl.set_visible(true);
                        pb.set_visible(true);
                    }
                }
                client_c.save_progress(tid, s.season, s.episode, pos, dur);
                // Sunucuya konum raporu (60sn'de bir, arka planda).
                if pos >= 10.0 && s.last_report.elapsed().as_secs() >= 60 {
                    s.last_report = Instant::now();
                    let rc = client_c.clone();
                    let (rt, rs, re_, rp) = (tid, s.season, s.episode, pos);
                    std::thread::spawn(move || rc.report_pos(rt, rs, re_, rp));
                }
                if api::Client::played_enough(pos, dur) {
                    if !s.watched_marked {
                        s.watched_marked = true;
                        client_c.save_watched(
                            &api::Watched { title_id: tid, episode: s.episode, season: s.season },
                            "",
                        );
                    }
                    if !s.host_saved {
                        s.host_saved = true;
                        let hint = api::Client::source_host_hint(s.sources[s.index].hint_url());
                        if !hint.is_empty() {
                            client_c.set_preferred_host(tid, hint);
                        }
                    }
                }
            }
            glib::ControlFlow::Continue
        });
    }

    // ---- kontroller ----
    {
        let st_c = state.clone();
        play_btn.connect_clicked(move |_| {
            toggle_with_flash(&st_c);
        });
    }
    // Ayarlar menüsü (Kalite + Hız + Altyazı + Focus Mod + Oto Bölüm)
    {
        let st_c = state.clone();
        let pop_c = settings_pop.clone();
        let client_c = client.clone();
        let toast_c = toast.clone();
        let status_c = status.clone();
        let src_c = src_lbl.clone();
        rebuild_settings_pop(&pop_c, &st_c, &client_c, &toast_c, &status_c, &src_c);
        let pop_c2 = pop_c.clone();
        pop_c.connect_show(move |_| {
            rebuild_settings_pop(&pop_c2, &st_c, &client_c, &toast_c, &status_c, &src_c);
        });
    }
    // kaynak boyutları (arka planda ölç, menüde göster)
    {
        let slot = state.borrow().sizes.clone();
        let urls: Vec<(usize, String)> = state
            .borrow()
            .sources
            .iter()
            .enumerate()
            .filter_map(|(i, s)| match s {
                Source::Direct(u) => Some((i, u.clone())),
                Source::Embed(_) => None,
            })
            .collect();
        std::thread::spawn(move || {
            let Ok(client) = reqwest::blocking::Client::builder()
                .connect_timeout(Duration::from_secs(4))
                .timeout(Duration::from_secs(8))
                .build()
            else {
                return;
            };
            for (i, u) in urls {
                if u.contains("video.sibnet.ru") {
                    continue;
                }
                let sz = client
                    .head(&u)
                    .header(
                        reqwest::header::USER_AGENT,
                        "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
                    )
                    .send()
                    .ok()
                    .and_then(|r| r.content_length());
                if let Ok(mut sl) = slot.lock() {
                    if i < sl.len() {
                        sl[i] = sz;
                    }
                }
            }
        });
    }
    // seek üstü önizleme: bölüm kapağı + zaman (kare önizleme kutusu).
    let prev_pic = gtk::Picture::new();
    prev_pic.set_size_request(128, 72);
    prev_pic.set_can_shrink(false);
    prev_pic.set_content_fit(gtk::ContentFit::Cover);
    let prev_lbl = gtk::Label::new(Some("--:--"));
    prev_lbl.add_css_class("dim-label");
    prev_lbl.set_xalign(0.5);
    let prev_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
    prev_box.add_css_class("seek-prev");
    prev_box.set_halign(gtk::Align::Start);
    prev_box.set_valign(gtk::Align::End);
    prev_box.set_margin_bottom(110);
    prev_box.set_visible(false);
    prev_box.append(&prev_pic);
    prev_box.append(&prev_lbl);
    stage.add_overlay(&prev_box);
    load_thumb_async(
        &client,
        req.ep.thumbnail.clone().or(req.title.poster.clone()),
        &prev_pic,
        128,
        72,
    );
    // seek üzerine gelince zaman ipucu
    // seek üstü önizleme: zaman ipucu + kapaklı kare kutu.
    {
        let st_c = state.clone();
        let seek_c = seek.clone();
        let prev_c = prev_box.clone();
        let lbl_c = prev_lbl.clone();
        let sm = gtk::EventControllerMotion::new();
        sm.connect_motion(move |_, x, _| {
            if let Ok(s) = st_c.try_borrow() {
                let d = s.player.dur();
                if d > 0.0 {
                    let w = seek_c.width() as f64;
                    if w > 0.0 {
                        let t = d * (x / w).clamp(0.0, 1.0);
                        seek_c.set_tooltip_text(Some(&fmt_time(t)));
                        lbl_c.set_text(&fmt_time(t));
                        prev_c.set_margin_start(((x - 64.0).clamp(8.0, (w - 136.0).max(8.0))) as i32);
                        prev_c.set_visible(true);
                        return;
                    }
                }
                seek_c.set_tooltip_text(None);
                prev_c.set_visible(false);
            }
        });
        seek.add_controller(sm);
        let prev_h = prev_box.clone();
        let leave = gtk::EventControllerMotion::new();
        leave.connect_leave(move |_| {
            prev_h.set_visible(false);
        });
        seek.add_controller(leave);
    }
    // seek hover: çubuk kalınlaşır + imleç belirir.
    {
        let seek_c = seek.clone();
        let enter = gtk::EventControllerMotion::new();
        enter.connect_enter(move |_, _, _| seek_c.add_css_class("seek-hot"));
        seek.add_controller(enter);
        let seek_c2 = seek.clone();
        let leave = gtk::EventControllerMotion::new();
        leave.connect_leave(move |_| seek_c2.remove_css_class("seek-hot"));
        seek.add_controller(leave);
    }
    // introyu-atla butonu
    {
        let st_c = state.clone();
        let rev_c = skip_rev.clone();
        let toast_c = toast.clone();
        skip_btn.connect_clicked(move |_| {
            if let Ok(mut s) = st_c.try_borrow_mut() {
                let t = s.aniskip.lock().unwrap().clone();
                let pos = s.player.pos();
                if let Some((target, is_op)) = skip_target(&t, pos, s.op_seek_done, s.ed_seek_done) {
                    if is_op {
                        s.op_seek_done = true;
                    } else {
                        s.ed_seek_done = true;
                    }
                    s.player.seek_abs(target);
                    s.player.show_text("⏩ Atlandı", 2500);
                    toast_in(&toast_c, "⏩ Atlandı", 2);
                }
                rev_c.set_reveal_child(false);
            }
        });
    }
    {
        let st_c = state.clone();
        let seeking_c = state.borrow().seeking.clone();
        seek.connect_change_value(move |_, _, v| {
            seeking_c.set(true);
            if let Ok(s) = st_c.try_borrow() {
                let d = s.player.dur();
                if d > 0.0 {
                    s.player.seek_abs(d * v / 1000.0);
                }
            }
            seeking_c.set(false);
            glib::Propagation::Proceed
        });
    }
    // --- ses kontrolleri ---
    {
        let st_c = state.clone();
        let vol_changing_c = state.borrow().vol_changing.clone();
        vol_scale.connect_change_value(move |scale, _, v| {
            vol_changing_c.set(true);
            if let Ok(s) = st_c.try_borrow() {
                s.player.set_volume(v);
                if v > 0.0 && s.player.muted() {
                    s.player.set_mute(false);
                }
            }
            scale.set_tooltip_text(Some(&format!("Ses: %{:.0}", v)));
            vol_changing_c.set(false);
            glib::Propagation::Proceed
        });
    }
    // alt bar üzerinde fare hareketi algılanınca oto-gizleme zamanlayıcısını sıfırla
    {
        let st_c = state.clone();
        let motion = gtk::EventControllerMotion::new();
        motion.connect_motion(move |_, _, _| {
            if let Ok(mut s) = st_c.try_borrow_mut() {
                s.last_motion = Instant::now();
            }
        });
        controls.add_controller(motion);
    }
    // ---- videoya tıklama: tek tık oynat/duraklat, çift tık tam ekran ----
    {
        let st_c = state.clone();
        let rev_c = controls_revealer.clone();
        let gesture = gtk::GestureClick::new();
        gesture.set_button(1); // sadece sol tık (sağ tık menüye ait)
        gesture.connect_pressed(move |_, n_press, _, _| {
            if n_press == 2 {
                // çift tık → bekleyen tek tıkı iptal et + tam ekran değiştir
                if let Ok(s) = st_c.try_borrow() {
                    s.click_pending.set(false);
                }
                let on = !st_c.borrow().main_window.is_fullscreen();
                set_fullscreen(&st_c, &rev_c, on);
            } else if n_press == 1 {
                if let Ok(s) = st_c.try_borrow() {
                    s.click_pending.set(true);
                    s.gl_area.grab_focus();
                }
                // çift tık gelmezse 260ms sonra oynat/duraklat (+ ortada ikon)
                let st2 = st_c.clone();
                glib::timeout_add_local_once(Duration::from_millis(260), move || {
                    let go = st2
                        .try_borrow()
                        .map(|s| {
                            let g = s.click_pending.get();
                            if g {
                                s.click_pending.set(false);
                            }
                            g
                        })
                        .unwrap_or(false);
                    if go {
                        toggle_with_flash(&st2);
                    }
                });
            }
        });
        gl_area.add_controller(gesture);
    }
    // ---- fare yan tuşları: geri = -10sn, ileri = +10sn ----
    {
        let st_c = state.clone();
        for (btn, secs) in [(8u32, -10.0), (9u32, 10.0)] {
            let g = gtk::GestureClick::new();
            g.set_button(btn);
            let st2 = st_c.clone();
            g.connect_pressed(move |_, _, _, _| {
                if let Ok(s) = st2.try_borrow() {
                    s.player.seek_rel(secs);
                    s.player.show_text(
                        if secs < 0.0 { "⏪ 10 saniye" } else { "10 saniye ⏩" },
                        1200,
                    );
                }
            });
            gl_area.add_controller(g);
        }
    }
    // ---- sağ tık bağlam menüsü ----
    let ctx_pop = gtk::Popover::new();
    ctx_pop.set_parent(&gl_area);
    ctx_pop.add_css_class("menu");
    {
        let st_c = state.clone();
        let rev_c = controls_revealer.clone();
        let toast_c = toast.clone();
        let win_c = main_window.clone();
        let pop_c = ctx_pop.clone();
        let play_cb_c = req.play_episode.clone();
        let prev_c = req.prev_ep.clone();
        let next_c = req.next_ep.clone();
        let dl_c = (
            req.title.name.clone(),
            req.ep.season,
            req.ep.episode,
            req.ep.name.clone(),
        );
        let rc = gtk::GestureClick::new();
        rc.set_button(3);
        rc.connect_pressed(move |_, _, x, y| {
            rebuild_ctx_menu(
                &pop_c, &st_c, &rev_c, &toast_c, &win_c,
                &play_cb_c, &prev_c, &next_c, &dl_c,
            );
            pop_c.set_pointing_to(Some(&gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
            pop_c.popup();
        });
        gl_area.add_controller(rc);
    }
    // klavye: sayfa kökünde (odak hangi düğmede olursa olsun çalışır:
    // space, oklar, s/e, m, f, a)
    {
        let st_c = state.clone();
        let rev_c = controls_revealer.clone();
        let motion = gtk::EventControllerMotion::new();
        motion.connect_motion(move |_, x, y| {
            poke_activity(&st_c, &rev_c, x, y);
        });
        vbox.add_controller(motion);
        // header üzerindeki hareket de aktivite saysın
        let st_h = state.clone();
        let rev_h = controls_revealer.clone();
        let hm = gtk::EventControllerMotion::new();
        hm.connect_motion(move |_, x, y| {
            poke_activity(&st_h, &rev_h, x, y);
        });
        header.add_controller(hm.clone());
        state.borrow_mut().header_motion = Some(hm);
    }
    // pencere yöneticisinden tam ekran değişirse logla
    // (krom mantığı fs-bağımsız, senkron gerekmez)
    {
        main_window.connect_fullscreened_notify(move |win| {
            eprintln!("[EMBED] fullscreen bildirimi: is_fullscreen={}", win.is_fullscreen());
        });
    }
    {
        let st_c = state.clone();
        let rev_c = controls_revealer.clone();
        fs_btn.connect_clicked(move |_| {
            let on = !st_c.borrow().main_window.is_fullscreen();
            set_fullscreen(&st_c, &rev_c, on);
        });
    }
    // klavye: space, oklar, s/e (aniskip), m + ses, f
    {
        let st_c = state.clone();
        let rev_c = controls_revealer.clone();
        let toast_c = toast.clone();
        let vol_changing_k = state.borrow().vol_changing.clone();
        let key = gtk::EventControllerKey::new();
        key.connect_key_pressed(move |_, keyval, _, _| {
            let Ok(s) = st_c.try_borrow() else { return glib::Propagation::Proceed };
            match keyval {
                gtk::gdk::Key::space => {
                    drop(s);
                    toggle_with_flash(&st_c);
                    glib::Propagation::Stop
                }
                gtk::gdk::Key::Right => {
                    s.player.seek_rel(10.0);
                    glib::Propagation::Stop
                }
                gtk::gdk::Key::Left => {
                    s.player.seek_rel(-10.0);
                    glib::Propagation::Stop
                }
                gtk::gdk::Key::Up => {
                    vol_changing_k.set(true);
                    s.player.set_volume(s.player.volume() + 5.0);
                    if s.player.muted() {
                        s.player.set_mute(false);
                    }
                    vol_changing_k.set(false);
                    glib::Propagation::Stop
                }
                gtk::gdk::Key::Down => {
                    vol_changing_k.set(true);
                    s.player.set_volume(s.player.volume() - 5.0);
                    vol_changing_k.set(false);
                    glib::Propagation::Stop
                }
                gtk::gdk::Key::m | gtk::gdk::Key::M => {
                    s.player.set_mute(!s.player.muted());
                    glib::Propagation::Stop
                }
                gtk::gdk::Key::f | gtk::gdk::Key::F => {
                    // s (read guard) yaşarken borrow_mut yasak → önce düşür!
                    drop(s);
                    let on = !st_c.borrow().main_window.is_fullscreen();
                    set_fullscreen(&st_c, &rev_c, on);
                    glib::Propagation::Stop
                }
                gtk::gdk::Key::a | gtk::gdk::Key::A => {
                    // Sığdır <-> doldur (yan bantları kırparak)
                    let fill = s.player.panscan() < 0.5;
                    s.player.set_panscan(if fill { 1.0 } else { 0.0 });
                    s.player.show_text(
                        if fill { "Ekran dolduruluyor (kırp)" } else { "Orijinal oran" },
                        1800,
                    );
                    glib::Propagation::Stop
                }
                gtk::gdk::Key::s | gtk::gdk::Key::S => {
                    let t = s.aniskip.lock().unwrap().clone();
                    if let Some(et) = t.op_end {
                        if !s.op_seek_done {
                            drop(s);
                            if let Ok(mut s2) = st_c.try_borrow_mut() {
                                s2.op_seek_done = true;
                                s2.player.seek_abs(et);
                                s2.player.show_text("⏩ İntro atlandı", 2500);
                            }
                            toast_in(&toast_c, "⏩ İntro atlandı", 2);
                        } else {
                            s.player.seek_abs(et);
                        }
                    } else {
                        s.player.show_text("⚠️ İntro zamanı yok (AniSkip)", 2500);
                    }
                    glib::Propagation::Stop
                }
                gtk::gdk::Key::e | gtk::gdk::Key::E => {
                    let t = s.aniskip.lock().unwrap().clone();
                    if let Some(et) = t.ed_end {
                        if !s.ed_seek_done {
                            drop(s);
                            if let Ok(mut s2) = st_c.try_borrow_mut() {
                                s2.ed_seek_done = true;
                                s2.player.seek_abs(et);
                                s2.player.show_text("⏩ Outro atlandı", 2500);
                            }
                            toast_in(&toast_c, "⏩ Outro atlandı", 2);
                        } else {
                            s.player.seek_abs(et);
                        }
                    } else {
                        s.player.show_text("⚠️ Outro zamanı yok (AniSkip)", 2500);
                    }
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            }
        });
        // Odak sayfa içinde herhangi bir düğmede bile olsa tuşlar çalışsın
        // diye kontrolcü sayfa köküne (vbox) takılır.
        vbox.add_controller(key);
    }

    Some(EmbeddedPlayer { state, widget: vbox })
}

fn update_src_label(lbl: &gtk::Label, idx: usize, total: usize, url: &str) {
    let hint = api::Client::source_host_hint(url);
    let host = if hint.is_empty() { "kaynak" } else { hint };
    lbl.set_text(&format!("{}/{} · {}", idx + 1, total, host));
}

/// Belirtilen index'teki kaynağı yükler (direct → hemen, embed → worker'da çöz).
fn load_source_at(
    state: &Rc<RefCell<PlayerState>>,
    client: &Arc<Client>,
    overlay: &adw::ToastOverlay,
    status: &gtk::Label,
    src_lbl: &gtk::Label,
    idx: usize,
) {
    let mut s = state.borrow_mut();
    if idx >= s.sources.len() {
        status.set_text("Tüm kaynaklar denendi, açılamadı ⚠️");
        toast_in(overlay, "Tüm kaynaklar denendi, video açılamadı", 4);
        return;
    }
    s.index = idx;
    s.load_started = Instant::now();
    s.media_loaded = false;
    s.playing_shown = false;
    let src = s.sources[idx].clone();
    drop(s);

    match src {
        Source::Direct(url) => {
            let s = state.borrow();
            if let Some(r) = sibnet_referer(&url) {
                s.player.set_http_headers(&r);
            }
            match s.player.load_url(&url) {
                Ok(()) => {
                    update_src_label(src_lbl, idx, s.sources.len(), &url);
                    status.set_text("yükleniyor…");
                }
                Err(e) => {
                    drop(s);
                    status.set_text(&format!("Yükleme hatası: {e}"));
                    next_source(state, client, overlay, status, src_lbl, None);
                }
            }
        }
        Source::Embed(embed_url) => {
            status.set_text("yedek kaynak çözümleniyor…");
            let mut s = state.borrow_mut();
            if s.resolving {
                return;
            }
            s.resolving = true;
            s.resolve_result = Arc::new(Mutex::new(None));
            let slot = s.resolve_result.clone();
            let client_c = client.clone();
            drop(s);
            std::thread::spawn(move || {
                let res = client_c.resolve_single(&embed_url);
                *slot.lock().unwrap() = Some(res);
            });
        }
    }
}

/// Sonraki kaynağa geçer; liste bittiyse bilgi verir.
fn next_source(
    state: &Rc<RefCell<PlayerState>>,
    client: &Arc<Client>,
    overlay: &adw::ToastOverlay,
    status: &gtk::Label,
    src_lbl: &gtk::Label,
    msg: Option<&str>,
) {
    if let Some(m) = msg {
        toast_in(overlay, m, 3);
    }
    let next = state.borrow().index + 1;
    let total = state.borrow().sources.len();
    if next >= total {
        status.set_text("Tüm kaynaklar denendi, açılamadı ⚠️");
        toast_in(overlay, "Tüm kaynaklar denendi, video açılamadı", 4);
        return;
    }
    state.borrow().player.stop();
    load_source_at(state, client, overlay, status, src_lbl, next);
}
