//! Gömülü libmpv test penceresi (Wayland uyumlu).
//!
//! Çalıştır:
//!   cargo run --bin embed_test -- "https://.../video.mp4"
//! URL verilmezse BigBuckBunny sample açılır.

#[path = "../embed_mpv.rs"]
mod embed_mpv;

use adw::prelude::*;
use embed_mpv::{current_fbo, MpvEmbed, RenderContext};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

const FALLBACK_URL: &str =
    "https://test-videos.co.uk/vids/bigbuckbunny/mp4/h264/720/Big_Buck_Bunny_720_10s_1MB.mp4";

struct Shared {
    player: Option<MpvEmbed>,
    render_ctx: Option<RenderContext>,
}

fn fmt(s: f64) -> String {
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

fn main() {
    let url = std::env::args().nth(1).unwrap_or_else(|| FALLBACK_URL.to_string());
    eprintln!("[EMBED-TEST] url={url}");

    let app = adw::Application::builder()
        .application_id("tr.com.animecix.embedtest")
        .build();

    app.connect_activate(move |app| {
        let shared: Rc<RefCell<Shared>> = Rc::new(RefCell::new(Shared {
            player: None,
            render_ctx: None,
        }));

        // Proxy varsa gömülü de oradan çıksın (mevcut davranışla aynı).
        let use_proxy = std::net::TcpStream::connect_timeout(
            &"127.0.0.1:10808".parse().expect("statik adres"),
            std::time::Duration::from_millis(300),
        )
        .is_ok();

        let player = MpvEmbed::new(
            if use_proxy { Some("http://127.0.0.1:10808") } else { None },
            None,
            &[],
        )
        .expect("mpv init başarısız (libmpv kurulu mu?)");
        shared.borrow_mut().player = Some(player);

        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title("Animecix — Gömülü MPV Test")
            .default_width(960)
            .default_height(640)
            .build();

        let vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);

        // --- video alanı ---
        let gl_area = gtk::GLArea::builder().hexpand(true).vexpand(true).build();
        // Wayland'da alpha + depth istemiyoruz; video için sade tut.
        gl_area.set_required_version(3, 2);
        vbox.append(&gl_area);

        // --- kontroller ---
        let bar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        bar.set_margin_top(8);
        bar.set_margin_bottom(8);
        bar.set_margin_start(12);
        bar.set_margin_end(12);

        let play_btn = gtk::Button::with_label("⏸ Duraklat");
        let time_lbl = gtk::Label::new(Some("--:-- / --:--"));
        time_lbl.set_xalign(0.0);
        let seek = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 1000.0, 1.0);
        seek.set_hexpand(true);
        seek.set_draw_value(false);

        let url_entry = gtk::Entry::builder()
            .text(url.as_str())
            .hexpand(true)
            .build();
        let load_btn = gtk::Button::with_label("Yükle");

        bar.append(&play_btn);
        bar.append(&seek);
        bar.append(&time_lbl);
        vbox.append(&bar);

        let urlbar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        urlbar.set_margin_bottom(8);
        urlbar.set_margin_start(12);
        urlbar.set_margin_end(12);
        urlbar.append(&url_entry);
        urlbar.append(&load_btn);
        vbox.append(&urlbar);

        let status = gtk::Label::new(Some("durum: başlıyor…"));
        status.set_xalign(0.0);
        status.set_margin_start(12);
        status.set_margin_bottom(8);
        vbox.append(&status);

        window.set_content(Some(&vbox));

        // --- GLArea realize: render context kur ---
        let needs_render = Arc::new(AtomicBool::new(false));
        {
            let shared_c = shared.clone();
            let flag_c = needs_render.clone();
            let status_c = status.clone();
            gl_area.connect_realize(move |area| {
                area.make_current();
                if let Some(err) = area.error() {
                    status_c.set_text(&format!("durum: GL hatası: {err}"));
                    eprintln!("[EMBED-TEST] GLArea hatası: {err}");
                    return;
                }
                let mut s = shared_c.borrow_mut();
                let Some(player) = s.player.as_mut() else {
                    status_c.set_text("durum: player yok");
                    return;
                };
                match player.create_render_context() {
                    Ok(mut ctx) => {
                        let flag = flag_c.clone();
                        ctx.set_update_callback(move || {
                            flag.store(true, Ordering::Relaxed);
                        });
                        s.render_ctx = Some(ctx);
                        status_c.set_text("durum: render ctx hazır");
                        eprintln!("[EMBED-TEST] render ctx hazır");
                    }
                    Err(e) => {
                        status_c.set_text(&format!("durum: render ctx hatası: {e}"));
                        eprintln!("[EMBED-TEST] render ctx hatası: {e}");
                    }
                }
            });
        }
        // mpv "yeni frame" dediğinde -> queue_render (8ms poll, thread-safe)
        {
            let flag_c = needs_render.clone();
            let weak = gl_area.downgrade();
            glib::timeout_add_local(std::time::Duration::from_millis(8), move || {
                if flag_c.swap(false, Ordering::Relaxed) {
                    if let Some(a) = weak.upgrade() {
                        a.queue_render();
                    }
                }
                glib::ControlFlow::Continue
            });
        }
        // --- GLArea render: mpv frame çiz ---
        {
            let shared_c = shared.clone();
            gl_area.connect_render(move |area, _ctx| {
                let s = shared_c.borrow();
                if let Some(ctx) = s.render_ctx.as_ref() {
                    let fbo = current_fbo();
                    if let Err(e) = ctx.render(fbo, area.width(), area.height(), true) {
                        eprintln!("[EMBED-TEST] render hatası: {e}");
                    }
                    ctx.report_swap();
                }
                glib::Propagation::Stop
            });
        }
        // --- GLArea unrealize: önce render ctx düşür (mpv kuralı) ---
        {
            let shared_c = shared.clone();
            gl_area.connect_unrealize(move |_| {
                shared_c.borrow_mut().render_ctx = None;
                eprintln!("[EMBED-TEST] render ctx düşürüldü");
            });
        }

        // --- butonlar ---
        {
            let shared_c = shared.clone();
            let btn = play_btn.clone();
            play_btn.connect_clicked(move |_| {
                if let Some(p) = shared_c.borrow().player.as_ref() {
                    p.toggle_pause();
                    btn.set_label(if p.paused() { "▶ Oynat" } else { "⏸ Duraklat" });
                }
            });
        }
        {
            let shared_c = shared.clone();
            let seeking = Rc::new(RefCell::new(false));
            let seeking_c = seeking.clone();
            seek.connect_change_value(move |_, _, v| {
                *seeking_c.borrow_mut() = true;
                if let Some(p) = shared_c.borrow().player.as_ref() {
                    let d = p.dur();
                    if d > 0.0 {
                        p.seek_abs(d * v / 1000.0);
                    }
                }
                *seeking_c.borrow_mut() = false;
                glib::Propagation::Proceed
            });
            // periyodik UI güncelleme
            let shared_t = shared.clone();
            let seek_w = seek.downgrade();
            let lbl_w = time_lbl.downgrade();
            let btn_w = play_btn.downgrade();
            let status_w = status.downgrade();
            let tick = Rc::new(RefCell::new(0u32));
            glib::timeout_add_local(std::time::Duration::from_millis(500), move || {
                let Some(seek) = seek_w.upgrade() else { return glib::ControlFlow::Break };
                let s = shared_t.borrow();
                let Some(p) = s.player.as_ref() else { return glib::ControlFlow::Continue };
                let pos = p.pos();
                let dur = p.dur();
                if let Some(l) = lbl_w.upgrade() {
                    l.set_text(&format!("{} / {}", fmt(pos), fmt(dur)));
                }
                if !*seeking.borrow() && dur > 0.0 {
                    seek.set_value((pos / dur * 1000.0).clamp(0.0, 1000.0));
                }
                if let Some(b) = btn_w.upgrade() {
                    b.set_label(if p.paused() { "▶ Oynat" } else { "⏸ Duraklat" });
                }
                if let Some(st) = status_w.upgrade() {
                    let state = if p.eof() {
                        "bitti (EOF)"
                    } else if p.idle() {
                        "boşta (idle)"
                    } else if p.core_idle() {
                        "yükleniyor… (buffering)"
                    } else if p.paused() {
                        "duraklatıldı"
                    } else {
                        "oynatılıyor"
                    };
                    st.set_text(&format!("durum: {state} | wayland={}", wayland_mi()));
                }
                // headless doğrulama için periyodik log (~2sn'de bir)
                {
                    let mut t = tick.borrow_mut();
                    *t += 1;
                    if *t % 4 == 0 {
                        eprintln!(
                            "[EMBED-TEST] tick pos={:.1} dur={:.1} paused={} idle={} eof={}",
                            pos,
                            dur,
                            p.paused(),
                            p.idle(),
                            p.eof()
                        );
                    }
                }
                glib::ControlFlow::Continue
            });
        }
        {
            let shared_c = shared.clone();
            let entry_c = url_entry.clone();
            let status_c = status.clone();
            load_btn.connect_clicked(move |_| {
                let u = entry_c.text().to_string();
                if u.trim().is_empty() {
                    return;
                }
                // sibnet referer numarası (mevcut harici davranışla aynı)
                if u.contains("video.sibnet.ru/v/") {
                    if let Some(p) = shared_c.borrow().player.as_ref() {
                        let vid = u
                            .split("/v/")
                            .nth(1)
                            .and_then(|s| s.split('/').nth(1))
                            .map(|s| s.trim_end_matches(".mp4"))
                            .unwrap_or("");
                        let referer = if vid.is_empty() {
                            "https://video.sibnet.ru/".to_string()
                        } else {
                            format!("https://video.sibnet.ru/shell.php?videoid={vid}")
                        };
                        p.set_http_headers(&referer);
                    }
                }
                if let Some(p) = shared_c.borrow().player.as_ref() {
                    match p.load_url(u.trim()) {
                        Ok(()) => status_c.set_text("durum: yükleniyor…"),
                        Err(e) => status_c.set_text(&format!("durum: yükleme hatası: {e}")),
                    }
                }
            });
        }
        // klavye: space=pause, sağ/sol=±10sn
        {
            let shared_c = shared.clone();
            let key = gtk::EventControllerKey::new();
            key.connect_key_pressed(move |_, keyval, _, _| {
                let s = shared_c.borrow();
                let Some(p) = s.player.as_ref() else { return glib::Propagation::Proceed };
                match keyval {
                    gtk::gdk::Key::space => {
                        p.toggle_pause();
                        glib::Propagation::Stop
                    }
                    gtk::gdk::Key::Right => {
                        p.seek_rel(10.0);
                        glib::Propagation::Stop
                    }
                    gtk::gdk::Key::Left => {
                        p.seek_rel(-10.0);
                        glib::Propagation::Stop
                    }
                    _ => glib::Propagation::Proceed,
                }
            });
            window.add_controller(key);
        }

        window.present();

        // açılışta otomatik yükle (realize'dan biraz sonra, ctx hazır olsun)
        {
            let shared_c = shared.clone();
            let url_c = url.clone();
            glib::timeout_add_local_once(std::time::Duration::from_millis(600), move || {
                // sibnet referer
                if url_c.contains("video.sibnet.ru/v/") {
                    if let Some(p) = shared_c.borrow().player.as_ref() {
                        let vid = url_c
                            .split("/v/")
                            .nth(1)
                            .and_then(|s| s.split('/').nth(1))
                            .map(|s| s.trim_end_matches(".mp4"))
                            .unwrap_or("");
                        let referer = if vid.is_empty() {
                            "https://video.sibnet.ru/".to_string()
                        } else {
                            format!("https://video.sibnet.ru/shell.php?videoid={vid}")
                        };
                        p.set_http_headers(&referer);
                    }
                }
                if let Some(p) = shared_c.borrow().player.as_ref() {
                    if let Err(e) = p.load_url(&url_c) {
                        eprintln!("[EMBED-TEST] oto-yükleme hatası: {e}");
                    }
                }
            });
        }
    });

    app.run_with_args(&["embed_test"]);
}

fn wayland_mi() -> &'static str {
    if std::env::var("WAYLAND_DISPLAY").is_ok() {
        "evet"
    } else if std::env::var("DISPLAY").is_ok() {
        "hayır (X11)"
    } else {
        "bilinmiyor"
    }
}
