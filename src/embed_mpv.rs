//! Gömülü libmpv oynatıcı — GTK4 GtkGLArea + OpenGL render API.
//!
//! Wayland (EGL) + X11 (GLX) uyumlu: GL sembolleri `dlsym(RTLD_DEFAULT)`
//! ile çözülür, `vo=libmpv` kullanılır, harici `mpv` penceresine gerek yok.
//!
//! Not: `libmpv` high-level crate sistemdeki mpv 0.41 (API 2.x) ile
//! uyumsuz olduğu için bilerek SADECE `libmpv-sys` (raw FFI) kullanılıyor.
//! Referans pattern: Celluloid / Aurora Media Player.

use anyhow::{bail, Result};
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_double, c_int, c_void};

/// Zaman formatı (saniye -> "HH:MM:SS" veya "MM:SS").
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

// ---------------------------------------------------------------------------
// RenderContext — libmpv OpenGL render API safe wrapper
// ---------------------------------------------------------------------------

type Callback = Box<dyn Fn() + Send + 'static>;

/// `mpv_render_context`. Sadece GL thread'inde (GTK main) kullanılmalı.
pub struct RenderContext {
    ctx: *mut libmpv_sys::mpv_render_context,
    _cb: Option<Box<Callback>>,
}

// SAFETY: tüm kullanım GTK main thread'de; RefCell içinde taşınabilmesi için.
unsafe impl Send for RenderContext {}

/// GL fonksiyon adresi çözümleyici.
/// GTK4 epoxy/GL driver'ı önceden yüklediği için RTLD_DEFAULT yeterlidir.
/// Bu sayede hem GLX (X11/XWayland) hem EGL (Wayland) çalışır.
unsafe extern "C" fn get_proc_addr(_ctx: *mut c_void, name: *const c_char) -> *mut c_void {
    libc::dlsym(libc::RTLD_DEFAULT, name)
}

impl RenderContext {
    /// OpenGL render context oluştur. **GL context current iken çağrılmalı**
    /// (GtkGLArea `realize` + `make_current()` sonrası).
    pub fn new(mpv: *mut libmpv_sys::mpv_handle) -> Result<Self> {
        let mut init_params = libmpv_sys::mpv_opengl_init_params {
            get_proc_address: Some(get_proc_addr),
            get_proc_address_ctx: std::ptr::null_mut(),
            extra_exts: std::ptr::null(),
        };

        let api_type_ptr = libmpv_sys::MPV_RENDER_API_TYPE_OPENGL.as_ptr() as *mut c_void;

        let mut params = [
            libmpv_sys::mpv_render_param {
                type_: libmpv_sys::mpv_render_param_type_MPV_RENDER_PARAM_API_TYPE,
                data: api_type_ptr,
            },
            libmpv_sys::mpv_render_param {
                type_: libmpv_sys::mpv_render_param_type_MPV_RENDER_PARAM_OPENGL_INIT_PARAMS,
                data: &mut init_params as *mut _ as *mut c_void,
            },
            libmpv_sys::mpv_render_param {
                type_: 0,
                data: std::ptr::null_mut(),
            },
        ];

        let mut ctx: *mut libmpv_sys::mpv_render_context = std::ptr::null_mut();
        let ret = unsafe { libmpv_sys::mpv_render_context_create(&mut ctx, mpv, params.as_mut_ptr()) };
        if ret != 0 {
            bail!("mpv_render_context_create başarısız: {}", err_str(ret));
        }
        Ok(Self { ctx, _cb: None })
    }

    /// mpv yeni frame hazırlayınca çağrılacak callback.
    /// İçinde mpv API çağrılmamalı — sadece flag set + queue_render.
    pub fn set_update_callback<F: Fn() + Send + 'static>(&mut self, cb: F) {
        let boxed: Box<Callback> = Box::new(Box::new(cb));
        let ptr = Box::into_raw(boxed);

        unsafe extern "C" fn trampoline(ctx: *mut c_void) {
            let cb = &*(ctx as *const Callback);
            cb();
        }

        unsafe {
            libmpv_sys::mpv_render_context_set_update_callback(
                self.ctx,
                Some(trampoline),
                ptr as *mut c_void,
            );
        }
        self._cb = Some(unsafe { Box::from_raw(ptr) });
    }

    /// Mevcut frame'i verilen FBO'ya çiz. **GL context current iken.**
    pub fn render(&self, fbo: c_int, w: c_int, h: c_int, flip_y: bool) -> Result<()> {
        let mut fbo_params = libmpv_sys::mpv_opengl_fbo {
            fbo,
            w,
            h,
            internal_format: 0,
        };
        let mut flip: c_int = flip_y as c_int;
        let mut params = [
            libmpv_sys::mpv_render_param {
                type_: libmpv_sys::mpv_render_param_type_MPV_RENDER_PARAM_OPENGL_FBO,
                data: &mut fbo_params as *mut _ as *mut c_void,
            },
            libmpv_sys::mpv_render_param {
                type_: libmpv_sys::mpv_render_param_type_MPV_RENDER_PARAM_FLIP_Y,
                data: &mut flip as *mut _ as *mut c_void,
            },
            libmpv_sys::mpv_render_param {
                type_: 0,
                data: std::ptr::null_mut(),
            },
        ];
        let ret = unsafe { libmpv_sys::mpv_render_context_render(self.ctx, params.as_mut_ptr()) };
        if ret != 0 {
            bail!("mpv_render_context_render başarısız: {}", err_str(ret));
        }
        Ok(())
    }

    pub fn report_swap(&self) {
        unsafe { libmpv_sys::mpv_render_context_report_swap(self.ctx) };
    }
}

impl Drop for RenderContext {
    fn drop(&mut self) {
        unsafe {
            libmpv_sys::mpv_render_context_set_update_callback(
                self.ctx,
                None,
                std::ptr::null_mut(),
            );
            libmpv_sys::mpv_render_context_free(self.ctx);
        }
    }
}

/// Şu an bağlı olan GL DRAW_FRAMEBUFFER id'si.
pub fn current_fbo() -> i32 {
    const GL_DRAW_FRAMEBUFFER_BINDING: u32 = 0x8CA6;
    type GlGetIntegervFn = unsafe extern "C" fn(pname: u32, params: *mut i32);
    let sym =
        unsafe { libc::dlsym(libc::RTLD_DEFAULT, b"glGetIntegerv\0".as_ptr() as *const c_char) };
    if sym.is_null() {
        return 0;
    }
    let f: GlGetIntegervFn = unsafe { std::mem::transmute(sym) };
    let mut fbo: i32 = 0;
    unsafe { f(GL_DRAW_FRAMEBUFFER_BINDING, &mut fbo) };
    fbo
}

// ---------------------------------------------------------------------------
// Ham mpv handle yardımcıları (sadece libmpv-sys)
// ---------------------------------------------------------------------------

fn err_str(code: c_int) -> String {
    let p = unsafe { libmpv_sys::mpv_error_string(code) };
    if p.is_null() {
        return format!("kod {code}");
    }
    unsafe { CStr::from_ptr(p).to_string_lossy().into_owned() }
}

fn mpv_cmd(ctx: *mut libmpv_sys::mpv_handle, args: &[&str]) -> Result<()> {
    let cs: Vec<CString> = args
        .iter()
        .map(|s| CString::new(*s).map_err(|e| anyhow::anyhow!("{e}")))
        .collect::<Result<_>>()?;
    let mut ptrs: Vec<*const c_char> = cs.iter().map(|s| s.as_ptr()).collect();
    ptrs.push(std::ptr::null());
    let ret = unsafe { libmpv_sys::mpv_command(ctx, ptrs.as_mut_ptr()) };
    if ret != 0 {
        bail!("mpv_command başarısız: {} ({args:?})", err_str(ret));
    }
    Ok(())
}

fn set_opt(ctx: *mut libmpv_sys::mpv_handle, name: &str, val: &str) {
    let Ok(n) = CString::new(name) else { return };
    let Ok(v) = CString::new(val) else { return };
    unsafe {
        libmpv_sys::mpv_set_option_string(ctx, n.as_ptr(), v.as_ptr());
    }
}

fn set_prop_str(ctx: *mut libmpv_sys::mpv_handle, name: &str, val: &str) {
    let (Ok(n), Ok(v)) = (CString::new(name), CString::new(val)) else { return };
    let mut ptr = v.as_ptr() as *mut c_void;
    // MPV_FORMAT_STRING için char* işaretçisinin adresi verilir.
    unsafe {
        libmpv_sys::mpv_set_property(
            ctx,
            n.as_ptr(),
            libmpv_sys::mpv_format_MPV_FORMAT_STRING,
            &mut ptr as *mut _ as *mut c_void,
        );
    }
}

fn set_prop_flag(ctx: *mut libmpv_sys::mpv_handle, name: &str, val: bool) {
    let Ok(n) = CString::new(name) else { return };
    let mut v: c_int = val as c_int;
    unsafe {
        libmpv_sys::mpv_set_property(
            ctx,
            n.as_ptr(),
            libmpv_sys::mpv_format_MPV_FORMAT_FLAG,
            &mut v as *mut _ as *mut c_void,
        );
    }
}

fn set_prop_double(ctx: *mut libmpv_sys::mpv_handle, name: &str, val: f64) {
    let Ok(n) = CString::new(name) else { return };
    let mut v: c_double = val;
    unsafe {
        libmpv_sys::mpv_set_property(
            ctx,
            n.as_ptr(),
            libmpv_sys::mpv_format_MPV_FORMAT_DOUBLE,
            &mut v as *mut _ as *mut c_void,
        );
    }
}

fn get_double(ctx: *mut libmpv_sys::mpv_handle, name: &str) -> f64 {
    let Ok(n) = CString::new(name) else { return 0.0 };
    let mut v: c_double = 0.0;
    let ret = unsafe {
        libmpv_sys::mpv_get_property(
            ctx,
            n.as_ptr(),
            libmpv_sys::mpv_format_MPV_FORMAT_DOUBLE,
            &mut v as *mut _ as *mut c_void,
        )
    };
    if ret == 0 { v } else { 0.0 }
}

fn get_flag(ctx: *mut libmpv_sys::mpv_handle, name: &str, def: bool) -> bool {
    let Ok(n) = CString::new(name) else { return def };
    let mut v: c_int = 0;
    let ret = unsafe {
        libmpv_sys::mpv_get_property(
            ctx,
            n.as_ptr(),
            libmpv_sys::mpv_format_MPV_FORMAT_FLAG,
            &mut v as *mut _ as *mut c_void,
        )
    };
    if ret == 0 { v != 0 } else { def }
}

fn get_i64(ctx: *mut libmpv_sys::mpv_handle, name: &str) -> Option<i64> {
    let Ok(n) = CString::new(name) else { return None };
    let mut v: i64 = 0;
    let ret = unsafe {
        libmpv_sys::mpv_get_property(
            ctx,
            n.as_ptr(),
            libmpv_sys::mpv_format_MPV_FORMAT_INT64,
            &mut v as *mut _ as *mut c_void,
        )
    };
    if ret == 0 { Some(v) } else { None }
}

fn get_string(ctx: *mut libmpv_sys::mpv_handle, name: &str) -> Option<String> {
    let Ok(n) = CString::new(name) else { return None };
    let mut ptr: *mut c_char = std::ptr::null_mut();
    let ret = unsafe {
        libmpv_sys::mpv_get_property(
            ctx,
            n.as_ptr(),
            libmpv_sys::mpv_format_MPV_FORMAT_STRING,
            &mut ptr as *mut _ as *mut c_void,
        )
    };
    if ret != 0 || ptr.is_null() {
        return None;
    }
    let s = unsafe { CStr::from_ptr(ptr).to_string_lossy().into_owned() };
    unsafe { libmpv_sys::mpv_free(ptr as *mut c_void) };
    Some(s)
}

fn set_prop_i64(ctx: *mut libmpv_sys::mpv_handle, name: &str, val: i64) {
    let Ok(n) = CString::new(name) else { return };
    let mut v: i64 = val;
    unsafe {
        libmpv_sys::mpv_set_property(
            ctx,
            n.as_ptr(),
            libmpv_sys::mpv_format_MPV_FORMAT_INT64,
            &mut v as *mut _ as *mut c_void,
        );
    }
}

/// Medya parçası (ses/altyazı/video).
#[derive(Clone, Debug)]
pub struct Track {
    pub id: i64,
    pub kind: String,
    pub lang: Option<String>,
    pub title: Option<String>,
    pub selected: bool,
}

impl Track {
    pub fn label(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(l) = &self.lang {
            if !l.is_empty() && l != "und" {
                parts.push(l.clone());
            }
        }
        if let Some(t) = &self.title {
            if !t.is_empty() {
                parts.push(t.clone());
            }
        }
        if parts.is_empty() {
            format!("#{}", self.id)
        } else {
            format!("#{} · {}", self.id, parts.join(" · "))
        }
    }
}

// ---------------------------------------------------------------------------
// MpvEmbed — yüksek seviye gömülü oynatıcı (raw handle sarmalayıcı)
// ---------------------------------------------------------------------------

pub struct MpvEmbed {
    ctx: *mut libmpv_sys::mpv_handle,
}

// mpv handle thread-safe: get/set_property belgeli olarak thread-safe.
unsafe impl Send for MpvEmbed {}

impl MpvEmbed {
    /// Yeni gömülü mpv instance. `vo=libmpv` initialize ÖNCESİ şart.
    /// `extra_opts`: initialize öncesi set edilecek ek mpv option'ları
    /// (örn. upscale için `[("scale","ewa_lanczossharp"), ("glsl-shaders", path)]`).
    pub fn new(
        proxy_url: Option<&str>,
        start_pos: Option<f64>,
        extra_opts: &[(&str, &str)],
    ) -> Result<Self> {
        // GTK LC_ALL'i resetler; mpv sayı parse'ı bozulmasın diye.
        unsafe {
            libc::setlocale(libc::LC_NUMERIC, b"C\0".as_ptr() as *const c_char);
        }

        let ctx = unsafe { libmpv_sys::mpv_create() };
        if ctx.is_null() {
            bail!("mpv_create NULL döndü");
        }

        // --- initialize ÖNCESİ zorunlu ---
        set_opt(ctx, "vo", "libmpv");

        // --- mevcut harici mpv argümanlarıyla aynı ruh (option olarak) ---
        set_opt(ctx, "hwdec", "auto-safe");
        set_opt(ctx, "hr-seek", "yes");
        set_opt(ctx, "keep-open", "yes");
        set_opt(ctx, "cache", "yes");
        set_opt(ctx, "demuxer-max-bytes", "134217728");
        set_opt(ctx, "demuxer-max-back-bytes", "33554432");
        set_opt(ctx, "demuxer-readahead-secs", "120");
        set_opt(ctx, "cache-secs", "120");
        set_opt(ctx, "network-timeout", "10");
        set_opt(
            ctx,
            "user-agent",
            "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
        );
        set_opt(ctx, "osc", "no");
        // gömülüde terminal mesajı kirletmesin
        set_opt(ctx, "msg-level", "all=no");

        for (k, v) in extra_opts {
            set_opt(ctx, k, v);
        }

        if let Some(p) = proxy_url {
            set_opt(ctx, "http-proxy", p);
            eprintln!("[EMBED] proxy aktif: {p}");
        }
        if let Some(s) = start_pos {
            if s > 5.0 {
                set_opt(ctx, "start", &format!("{s:.1}"));
            }
        }

        let ret = unsafe { libmpv_sys::mpv_initialize(ctx) };
        if ret != 0 {
            unsafe { libmpv_sys::mpv_terminate_destroy(ctx) };
            bail!("mpv_initialize başarısız: {}", err_str(ret));
        }

        // initialize SONRASI property'ler
        set_prop_double(ctx, "volume", 100.0);

        let ver = unsafe { libmpv_sys::mpv_client_api_version() };
        eprintln!("[EMBED] mpv handle hazır (client API v{ver:#x})");
        Ok(Self { ctx })
    }

    pub fn raw_ptr(&self) -> *mut libmpv_sys::mpv_handle {
        self.ctx
    }

    pub fn create_render_context(&mut self) -> Result<RenderContext> {
        RenderContext::new(self.ctx)
    }

    pub fn load_url(&self, url: &str) -> Result<()> {
        eprintln!("[EMBED] loadfile: {url:.100}");
        mpv_cmd(self.ctx, &["loadfile", url, "replace"])?;
        // Duraklatılmış başla — ilk video frame hazır olana kadar ses duyulmasın
        set_prop_flag(self.ctx, "pause", true);
        Ok(())
    }

    pub fn set_http_headers(&self, referer: &str) {
        let fields = format!("Referer: {referer}\nAccept: */*");
        set_prop_str(self.ctx, "http-header-fields", &fields);
    }

    pub fn toggle_pause(&self) {
        let _ = mpv_cmd(self.ctx, &["cycle", "pause"]);
    }
    pub fn set_pause(&self, p: bool) {
        set_prop_flag(self.ctx, "pause", p);
    }
    pub fn seek_abs(&self, secs: f64) {
        let _ = mpv_cmd(self.ctx, &["seek", &format!("{secs:.1}"), "absolute"]);
    }
    pub fn seek_rel(&self, secs: f64) {
        let _ = mpv_cmd(self.ctx, &["seek", &format!("{secs:.0}")]);
    }
    pub fn stop(&self) {
        let _ = mpv_cmd(self.ctx, &["stop"]);
    }

    pub fn pos(&self) -> f64 {
        get_double(self.ctx, "time-pos")
    }
    pub fn dur(&self) -> f64 {
        get_double(self.ctx, "duration")
    }
    pub fn volume(&self) -> f64 {
        get_double(self.ctx, "volume")
    }
    pub fn set_volume(&self, v: f64) {
        set_prop_double(self.ctx, "volume", v.clamp(0.0, 100.0));
    }
    pub fn muted(&self) -> bool {
        get_flag(self.ctx, "mute", false)
    }
    pub fn set_mute(&self, m: bool) {
        set_prop_flag(self.ctx, "mute", m);
    }
    pub fn speed(&self) -> f64 {
        let v = get_double(self.ctx, "speed");
        if v > 0.0 { v } else { 1.0 }
    }
    pub fn set_speed(&self, v: f64) {
        set_prop_double(self.ctx, "speed", v.clamp(0.25, 4.0));
    }
    /// Gerçek video yüksekliği (örn. 1080) — kalite etiketi için.
    pub fn video_height(&self) -> Option<i64> {
        get_i64(self.ctx, "video-params/h").filter(|h| *h > 0)
    }
    /// Görüntü doldurma: 0.0 = sığdır (orijinal oran), 1.0 = ekranı
    /// doldur (oranı koruyarak kırp).
    pub fn panscan(&self) -> f64 {
        get_double(self.ctx, "panscan")
    }
    pub fn set_panscan(&self, v: f64) {
        set_prop_double(self.ctx, "panscan", v.clamp(0.0, 1.0));
    }
    /// Ses/altyazı/video parçaları.
    pub fn tracks(&self) -> Vec<Track> {
        let count = get_i64(self.ctx, "track-list/count").unwrap_or(0);
        (0..count)
            .filter_map(|i| {
                let kind = get_string(self.ctx, &format!("track-list/{i}/type"))?;
                let id = get_i64(self.ctx, &format!("track-list/{i}/id")).unwrap_or(0);
                let lang = get_string(self.ctx, &format!("track-list/{i}/lang"));
                let title = get_string(self.ctx, &format!("track-list/{i}/title"));
                let selected = get_flag(self.ctx, &format!("track-list/{i}/selected"), false);
                Some(Track { id, kind, lang, title, selected })
            })
            .collect()
    }
    pub fn set_audio(&self, id: i64) {
        set_prop_i64(self.ctx, "aid", id);
    }
    /// None → altyazı kapat.
    pub fn set_sub(&self, id: Option<i64>) {
        match id {
            Some(v) => set_prop_i64(self.ctx, "sid", v),
            None => set_prop_str(self.ctx, "sid", "no"),
        }
    }
    pub fn paused(&self) -> bool {
        get_flag(self.ctx, "pause", true)
    }
    pub fn idle(&self) -> bool {
        get_flag(self.ctx, "idle-active", true)
    }
    pub fn eof(&self) -> bool {
        get_flag(self.ctx, "eof-reached", false)
    }
    pub fn core_idle(&self) -> bool {
        get_i64(self.ctx, "core-idle").map(|v| v != 0).unwrap_or(false)
    }
    pub fn show_text(&self, msg: &str, ms: u64) {
        let _ = mpv_cmd(self.ctx, &["show-text", msg, &format!("{ms}")]);
    }
    /// O anki kareyi dosyaya kaydeder.
    pub fn screenshot_to_file(&self, path: &str) -> Result<()> {
        mpv_cmd(self.ctx, &["screenshot-to-file", path])
    }
    /// Video bilgi satırları (codec, çözünürlük, fps, boyut...).
    pub fn video_info(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        if let (Some(w), Some(h)) = (
            get_i64(self.ctx, "video-params/w"),
            get_i64(self.ctx, "video-params/h"),
        ) {
            if w > 0 && h > 0 {
                out.push(("Çözünürlük".into(), format!("{w}×{h}")));
            }
        }
        let mut fps = get_double(self.ctx, "video-params/fps");
        if fps <= 0.0 {
            fps = get_double(self.ctx, "container-fps");
        }
        if fps > 0.0 {
            out.push(("FPS".into(), format!("{fps:.2}")));
        }
        if let Some(c) = get_string(self.ctx, "video-codec") {
            if !c.is_empty() {
                out.push(("Video".into(), c));
            }
        }
        if let Some(c) = get_string(self.ctx, "audio-codec") {
            if !c.is_empty() {
                out.push(("Ses".into(), c));
            }
        }
        let dur = get_double(self.ctx, "duration");
        if dur > 0.0 {
            out.push(("Süre".into(), fmt_time(dur)));
        }
        if let Some(sz) = get_i64(self.ctx, "file-size") {
            if sz > 0 {
                let mb = sz as f64 / 1048576.0;
                out.push((
                    "Boyut".into(),
                    if mb >= 1024.0 {
                        format!("{:.2} GB", mb / 1024.0)
                    } else {
                        format!("{mb:.1} MB")
                    },
                ));
            }
        }
        out
    }
}

impl Drop for MpvEmbed {
    fn drop(&mut self) {
        // RenderContext HER ZAMAN bundan önce düşürülmeli (sıralama kuralı).
        unsafe { libmpv_sys::mpv_terminate_destroy(self.ctx) };
    }
}
