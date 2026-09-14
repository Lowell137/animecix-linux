//! Siteden gelen bilgi sayfaları: gündem şeridi, haberler, haber detayı,
//! yayın takvimi. Veri `/secure/news` ve `/secure/calendar` uçlarından gelir.
use gtk::prelude::*;
use std::rc::Rc;

use crate::api::{
    CalendarDay, Credit, NewsItem, Review, Title, fmt_tr_datetime, fmt_tr_day,
    fmt_tr_time,
};

/// Bölüm başlığı (örn. "🆕 SON EKLENEN BÖLÜMLER") + sağda opsiyonel "Tümü".
fn rail_head(title_text: &str) -> (gtk::Box, gtk::Label) {
    let head = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    head.set_hexpand(true);
    head.set_halign(gtk::Align::Fill);
    head.set_margin_start(4);
    head.set_margin_end(4);
    let title = gtk::Label::new(Some(title_text));
    title.add_css_class("shelf-title");
    title.set_xalign(0.5);
    title.set_halign(gtk::Align::Fill);
    title.set_justify(gtk::Justification::Center);
    title.set_hexpand(true);
    head.append(&title);
    (head, title)
}

/// Ana sayfadaki son eklenen bölümler şeridi (sitedeki gibi).
/// Sitedeki standart poster kartı: kapak + 1 satır başlık + 1 satır alt yazı.
/// Tüm kartlar aynı boyuttadır (140x270). `marathon_toggle` verilirse
/// hover'da sol üstte maraton ekle/çıkar butonu belirir (tıklama
/// `bool` döner: true=eklendi). Kart tıklaması `on_open` çağırır.
#[allow(clippy::too_many_arguments)]
pub fn poster_card(
    t: &Title,
    sub: Option<&str>,
    cover_loader: impl Fn(Option<&str>, &gtk::Picture, i32, i32) + 'static,
    hover_binder: impl Fn(&gtk::Box, Option<&str>) + 'static,
    marathon_toggle: Option<(bool, Rc<dyn Fn() -> bool>)>,
    on_open: impl Fn(Title) + 'static,
) -> gtk::Box {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 4);
    card.add_css_class("title-btn");
    card.set_size_request(140, 270);
    hover_binder(&card, t.poster.as_deref());

    let overlay = gtk::Overlay::new();
    overlay.add_css_class("poster-lift");
    overlay.set_size_request(140, 210);
    let pic = crate::covers::new_sized_picture(140, 210);
    pic.set_halign(gtk::Align::Center);
    pic.set_can_shrink(false);
    cover_loader(t.poster.as_deref(), &pic, 140, 210);
    overlay.set_child(Some(&pic));

    // Maraton hızlı ekleme (hover'da sol üstte belirir).
    let mara_btn: Option<gtk::Button> = marathon_toggle.map(|(member, toggle)| {
        let b = gtk::Button::from_icon_name(if member {
            "object-select-symbolic"
        } else {
            "list-add-symbolic"
        });
        b.add_css_class("circular");
        b.set_size_request(34, 34);
        b.set_halign(gtk::Align::Start);
        b.set_valign(gtk::Align::Start);
        b.set_margin_top(6);
        b.set_margin_start(6);
        b.set_visible(false);
        b.set_tooltip_text(Some(if member {
            "Maratonda ✓ (çıkarmak için tıkla)"
        } else {
            "Maratona ekle"
        }));
        let b_c = b.clone();
        b.connect_clicked(move |_| {
            let added = toggle();
            b_c.set_icon_name(if added {
                "object-select-symbolic"
            } else {
                "list-add-symbolic"
            });
            b_c.set_tooltip_text(Some(if added {
                "Maratonda ✓ (çıkarmak için tıkla)"
            } else {
                "Maratona ekle"
            }));
        });
        overlay.add_overlay(&b);
        b
    });
    // Hover'da sadece posteri kaldır (yazılar sabit): overlay'e class basılır.
    {
        let lift = gtk::EventControllerMotion::new();
        let ov_e = overlay.clone();
        let ov_l = overlay.clone();
        lift.connect_enter(move |_, _, _| ov_e.add_css_class("lifted"));
        lift.connect_leave(move |_| ov_l.remove_css_class("lifted"));
        card.add_controller(lift);
    }
    card.append(&overlay);

    let name = gtk::Label::new(Some(&t.name));
    name.add_css_class("card-title");
    name.set_wrap(false);
    name.set_single_line_mode(true);
    name.set_max_width_chars(16);
    name.set_lines(1);
    name.set_ellipsize(gtk::pango::EllipsizeMode::End);
    card.append(&name);

    // Alt yazı her kartta 1 satır yer kaplar (boşsa bile) ki kart
    // boyları eşit kalsın.
    let sub_lbl = gtk::Label::new(Some(sub.filter(|s| !s.is_empty()).unwrap_or(" ")));
    sub_lbl.add_css_class("dim-label");
    sub_lbl.set_xalign(0.5);
    sub_lbl.set_halign(gtk::Align::Center);
    sub_lbl.set_wrap(false);
    sub_lbl.set_single_line_mode(true);
    sub_lbl.set_ellipsize(gtk::pango::EllipsizeMode::End);
    sub_lbl.set_max_width_chars(18);
    card.append(&sub_lbl);

    // Hover'da maraton butonunu göster/gizle.
    if mara_btn.is_some() {
        let motion = gtk::EventControllerMotion::new();
        let mb = mara_btn.clone().unwrap();
        let mb_c = mb.clone();
        motion.connect_enter(move |_, _, _| mb.set_visible(true));
        motion.connect_leave(move |_| mb_c.set_visible(false));
        card.add_controller(motion);
    }

    let t_clone = t.clone();
    let on_o = Rc::new(on_open);
    let mara_hit = mara_btn.clone();
    let card_c = card.clone();
    let gesture = gtk::GestureClick::new();
    gesture.connect_pressed(move |_, _, x, y| {
        // Maraton butonuna basıldıysa buton halleder (çift işlem olmasın).
        if let Some(ref mb) = mara_hit {
            if let Some((bx, by)) = mb.translate_coordinates(&card_c, 0.0, 0.0) {
                let w = mb.width() as f64;
                let h = mb.height() as f64;
                if x >= bx && x <= bx + w && y >= by && y <= by + h {
                    return;
                }
            }
        }
        on_o(t_clone.clone());
    });
    card.add_controller(gesture);
    card
}

fn tr_known_for(s: &str) -> &str {
    match s.trim().to_lowercase().as_str() {
        "acting" => "Oyunculuk",
        "directing" => "Yönetmenlik",
        "writing" => "Senaryo",
        "production" => "Yapım",
        "sound" => "Ses · Müzik",
        "music" => "Müzik",
        "camera" => "Kamera",
        "art" => "Sanat",
        "editing" => "Kurgu",
        "costume & make-up" | "costume" => "Kostüm",
        "visual effects" => "Görsel Efekt",
        "crew" => "Ekip",
        _ => return "Ekip",
    }
}

/// Detay sayfasındaki oyuncu & ekip şeridi (tıklamasız; kişi sayfası sonraki pakette).
pub fn credits_strip(
    credits: &[Credit],
    cover_loader: impl Fn(Option<&str>, &gtk::Picture, i32, i32) + 'static,
) -> gtk::Box {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 4);
    root.set_hexpand(true);
    root.set_halign(gtk::Align::Fill);
    root.set_margin_top(12);
    let (head, _) = rail_head("🎭 OYUNCULAR & EKİP");
    root.append(&head);

    let scroll = gtk::ScrolledWindow::new();
    scroll.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Never);
    scroll.set_hexpand(true);
    scroll.set_halign(gtk::Align::Fill);

    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row.set_margin_top(6);
    row.set_margin_bottom(6);
    row.set_margin_start(4);
    row.set_margin_end(4);

    let cover_rc = Rc::new(cover_loader);
    for c in credits {
        let card = gtk::Box::new(gtk::Orientation::Vertical, 4);
        card.add_css_class("title-btn");
        // 120 foto + 2 satır ad + rol; sabit yükseklik kesilmeyi önler.
        card.set_size_request(96, 220);
        let pic = crate::covers::new_sized_picture(80, 120);
        pic.set_halign(gtk::Align::Center);
        pic.set_can_shrink(false);
        cover_rc(c.poster.as_deref(), &pic, 80, 120);
        card.append(&pic);
        let name = gtk::Label::new(Some(&c.name));
        name.add_css_class("card-title");
        name.set_wrap(true);
        name.set_max_width_chars(12);
        name.set_lines(2);
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        card.append(&name);
        if !c.known_for.is_empty() {
            let job = gtk::Label::new(Some(tr_known_for(&c.known_for)));
            job.add_css_class("dim-label");
            job.set_xalign(0.5);
            job.set_halign(gtk::Align::Center);
            card.append(&job);
        }
        row.append(&card);
    }
    scroll.set_child(Some(&row));
    root.append(&scroll);
    root
}

fn review_row(
    r: &Review,
    cover_loader: &Rc<dyn Fn(Option<&str>, &gtk::Picture, i32, i32)>,
) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row.add_css_class("history-item-card");
    row.set_margin_top(4);
    row.set_margin_bottom(4);
    row.set_margin_start(6);
    row.set_margin_end(6);

    let avatar = crate::covers::new_sized_picture(48, 48);
    avatar.set_valign(gtk::Align::Start);
    // Avatar yoksa/yüklenemezse baş harf rozeti görünür (arkada durur).
    let overlay = gtk::Overlay::new();
    overlay.set_size_request(48, 48);
    let initial = r.username.chars().next().unwrap_or('?').to_string().to_uppercase();
    let fallback = gtk::Label::new(Some(&initial));
    fallback.add_css_class("title-2");
    fallback.set_halign(gtk::Align::Center);
    fallback.set_valign(gtk::Align::Center);
    overlay.set_child(Some(&fallback));
    overlay.add_overlay(&avatar);
    cover_loader(r.avatar.as_deref(), &avatar, 48, 48);
    row.append(&overlay);

    let text_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
    text_box.set_hexpand(true);

    let head = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let user = gtk::Label::new(Some(if r.username.is_empty() {
        "Anonim"
    } else {
        &r.username
    }));
    user.add_css_class("title-4");
    user.set_xalign(0.0);
    user.set_hexpand(true);
    head.append(&user);
    if r.score > 0.0 {
        let score = gtk::Label::new(Some(&format!("★ {}", trim_score(r.score))));
        score.add_css_class("status-badge-progress");
        head.append(&score);
    }
    text_box.append(&head);

    let date = gtk::Label::new(Some(&fmt_tr_datetime(&r.created_at)));
    date.add_css_class("dim-label");
    date.set_xalign(0.0);
    text_box.append(&date);

    let body = gtk::Label::new(Some(r.body.trim()));
    body.set_xalign(0.0);
    body.set_wrap(true);
    body.set_selectable(true);
    text_box.append(&body);

    row.append(&text_box);
    row
}

fn trim_score(s: f64) -> String {
    if (s - s.round()).abs() < 0.01 {
        format!("{}", s.round() as i64)
    } else {
        format!("{s:.1}")
    }
}

/// Detay sayfasındaki inceleme önizlemesi (ilk 3 + tümü butonu).
pub fn reviews_preview(
    reviews: &[Review],
    total: usize,
    cover_loader: impl Fn(Option<&str>, &gtk::Picture, i32, i32) + 'static,
    on_all: impl Fn() + 'static,
) -> gtk::Box {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 4);
    root.set_hexpand(true);
    root.set_halign(gtk::Align::Fill);
    root.set_margin_top(12);
    let (head, _) = rail_head(&if total > 0 {
        format!("⭐ İNCELEMELER ({total})")
    } else {
        "⭐ İNCELEMELER".to_string()
    });
    let all_btn = gtk::Button::with_label("Tümü ›");
    all_btn.add_css_class("flat");
    all_btn.add_css_class("pill");
    all_btn.set_tooltip_text(Some("Tüm incelemeler"));
    let on_all_rc = Rc::new(on_all);
    all_btn.connect_clicked(move |_| on_all_rc());
    head.append(&all_btn);
    root.append(&head);

    let cover_rc: Rc<dyn Fn(Option<&str>, &gtk::Picture, i32, i32)> = Rc::new(cover_loader);
    for r in reviews.iter().take(3) {
        root.append(&review_row(r, &cover_rc));
    }
    root
}

/// İncelemeler sayfası: liste + "Daha Fazla" sayfalama.
#[allow(clippy::too_many_arguments)]
pub fn reviews_list(
    items: &[Review],
    total: usize,
    page: u32,
    last_page: u32,
    loading: bool,
    error: Option<String>,
    cover_loader: impl Fn(Option<&str>, &gtk::Picture, i32, i32) + 'static,
    on_more: impl Fn() + 'static,
    on_retry: impl Fn() + 'static,
) -> gtk::Box {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
    root.set_hexpand(true);
    root.set_halign(gtk::Align::Fill);
    root.set_margin_top(8);
    root.set_margin_bottom(12);
    root.set_margin_start(12);
    root.set_margin_end(12);

    if let Some(err) = error {
        if items.is_empty() && !loading {
            let sp = crate::ui::components::create_status_page(
                "İncelemeler Yüklenemedi",
                &err,
                "network-error-symbolic",
            );
            root.append(&sp);
            let retry_btn = gtk::Button::with_label("Tekrar Dene");
            retry_btn.add_css_class("suggested-action");
            retry_btn.add_css_class("pill");
            retry_btn.set_halign(gtk::Align::Center);
            retry_btn.set_margin_top(8);
            let on_r = Rc::new(on_retry);
            retry_btn.connect_clicked(move |_| {
                on_r();
            });
            root.append(&retry_btn);
            return root;
        }
    }

    if loading && items.is_empty() {
        let spin_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        spin_row.set_halign(gtk::Align::Center);
        spin_row.set_margin_top(24);
        let sp = gtk::Spinner::new();
        sp.start();
        let lbl = gtk::Label::new(Some("İncelemeler yükleniyor…"));
        lbl.add_css_class("dim-label");
        spin_row.append(&sp);
        spin_row.append(&lbl);
        root.append(&spin_row);
        return root;
    }

    if items.is_empty() {
        let sp = crate::ui::components::create_status_page(
            "Henüz İnceleme Yok",
            "Bu başlık için yazılmış inceleme bulunmuyor.",
            "dialog-information-symbolic",
        );
        root.append(&sp);
        return root;
    }

    let cover_rc_list: Rc<dyn Fn(Option<&str>, &gtk::Picture, i32, i32)> = Rc::new(cover_loader);
    for r in items {
        root.append(&review_row(r, &cover_rc_list));
    }

    if page < last_page {
        let more_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        more_box.set_halign(gtk::Align::Center);
        more_box.set_margin_top(10);
        more_box.set_margin_bottom(4);
        let shown = items.len();
        let more_btn = gtk::Button::with_label(&if total > 0 {
            format!("Daha Fazla Göster ({shown} / {total})")
        } else {
            "Daha Fazla Göster".to_string()
        });
        more_btn.add_css_class("pill");
        more_btn.set_sensitive(!loading);
        let on_m = Rc::new(on_more);
        more_btn.connect_clicked(move |_| {
            on_m();
        });
        more_box.append(&more_btn);
        if loading {
            let sp = gtk::Spinner::new();
            sp.start();
            sp.set_valign(gtk::Align::Center);
            more_box.append(&sp);
        }
        root.append(&more_box);
    }
    root
}
/// Haberler sayfası: kart listesi + "Daha Fazla" sayfalama (1-indexli).
#[allow(clippy::too_many_arguments)]
pub fn news_list(
    items: &[NewsItem],
    total: usize,
    page: u32,
    last_page: u32,
    loading: bool,
    error: Option<String>,
    on_open: impl Fn(NewsItem) + 'static,
    cover_loader: impl Fn(Option<&str>, &gtk::Picture, i32, i32) + 'static,
    on_more: impl Fn() + 'static,
    on_retry: impl Fn() + 'static,
) -> gtk::Box {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
    root.set_hexpand(true);
    root.set_halign(gtk::Align::Fill);
    root.set_margin_top(8);
    root.set_margin_bottom(12);
    root.set_margin_start(12);
    root.set_margin_end(12);

    if let Some(err) = error {
        if items.is_empty() && !loading {
            let sp = crate::ui::components::create_status_page(
                "Haberler Yüklenemedi",
                &err,
                "network-error-symbolic",
            );
            root.append(&sp);
            let retry_btn = gtk::Button::with_label("Tekrar Dene");
            retry_btn.add_css_class("suggested-action");
            retry_btn.add_css_class("pill");
            retry_btn.set_halign(gtk::Align::Center);
            retry_btn.set_margin_top(8);
            let on_r = Rc::new(on_retry);
            retry_btn.connect_clicked(move |_| {
                on_r();
            });
            root.append(&retry_btn);
            return root;
        }
    }

    if loading && items.is_empty() {
        let spin_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        spin_row.set_halign(gtk::Align::Center);
        spin_row.set_margin_top(24);
        let sp = gtk::Spinner::new();
        sp.start();
        let lbl = gtk::Label::new(Some("Haberler yükleniyor…"));
        lbl.add_css_class("dim-label");
        spin_row.append(&sp);
        spin_row.append(&lbl);
        root.append(&spin_row);
        return root;
    }

    if items.is_empty() {
        let sp = crate::ui::components::create_status_page(
            "Henüz Haber Yok",
            "Site haberleri burada görünecek.",
            "internet-news-reader-symbolic",
        );
        root.append(&sp);
        return root;
    }

    let list_box = gtk::Box::new(gtk::Orientation::Vertical, 8);
    list_box.set_hexpand(true);
    list_box.set_halign(gtk::Align::Fill);

    let cover_rc = Rc::new(cover_loader);
    let open_rc = Rc::new(on_open);
    for item in items {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        row.add_css_class("history-item-card");
        row.set_margin_top(4);
        row.set_margin_bottom(4);
        row.set_margin_start(6);
        row.set_margin_end(6);

        let pic = crate::covers::new_sized_picture(160, 90);
        pic.set_valign(gtk::Align::Center);
        cover_rc(item.image.as_deref(), &pic, 160, 90);
        row.append(&pic);

        let text_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
        text_box.set_valign(gtk::Align::Center);
        text_box.set_hexpand(true);

        let name = gtk::Label::new(Some(item.title.trim()));
        name.add_css_class("title-3");
        name.set_xalign(0.0);
        name.set_wrap(true);
        name.set_max_width_chars(60);
        name.set_lines(2);
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        text_box.append(&name);

        let date = gtk::Label::new(Some(&fmt_tr_datetime(&item.created_at)));
        date.add_css_class("dim-label");
        date.set_xalign(0.0);
        text_box.append(&date);

        let first_line = item.body.lines().next().unwrap_or("").trim().to_string();
        if !first_line.is_empty() {
            let sum = gtk::Label::new(Some(&first_line));
            sum.add_css_class("dim-label");
            sum.set_xalign(0.0);
            sum.set_wrap(false);
            sum.set_single_line_mode(true);
            sum.set_ellipsize(gtk::pango::EllipsizeMode::End);
            text_box.append(&sum);
        }
        row.append(&text_box);

        let it = item.clone();
        let on_o = open_rc.clone();
        let gesture = gtk::GestureClick::new();
        gesture.connect_pressed(move |_, _, _, _| {
            on_o(it.clone());
        });
        row.add_controller(gesture);

        list_box.append(&row);
    }
    root.append(&list_box);

    if page < last_page {
        let more_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        more_box.set_halign(gtk::Align::Center);
        more_box.set_margin_top(10);
        more_box.set_margin_bottom(4);
        let shown = items.len();
        let more_btn = gtk::Button::with_label(&if total > 0 {
            format!("Daha Fazla Göster ({shown} / {total})")
        } else {
            "Daha Fazla Göster".to_string()
        });
        more_btn.add_css_class("pill");
        more_btn.set_sensitive(!loading);
        let on_m = Rc::new(on_more);
        more_btn.connect_clicked(move |_| {
            on_m();
        });
        more_box.append(&more_btn);
        if loading {
            let sp = gtk::Spinner::new();
            sp.start();
            sp.set_valign(gtk::Align::Center);
            more_box.append(&sp);
        }
        root.append(&more_box);
    }
    root
}

/// Haber detayı: banner + başlık + tarih + tam metin.
pub fn news_detail(
    item: &NewsItem,
    cover_loader: impl Fn(Option<&str>, &gtk::Picture, i32, i32) + 'static,
) -> gtk::Box {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 10);
    root.set_hexpand(true);
    root.set_halign(gtk::Align::Fill);
    root.set_margin_top(12);
    root.set_margin_bottom(18);
    root.set_margin_start(14);
    root.set_margin_end(14);

    let banner_url = item.backdrop.as_deref().or(item.image.as_deref());
    if banner_url.is_some() {
        let pic = crate::covers::new_sized_picture(900, 280);
        pic.set_hexpand(true);
        pic.set_halign(gtk::Align::Fill);
        pic.set_content_fit(gtk::ContentFit::Cover);
        cover_loader(banner_url, &pic, 900, 280);
        root.append(&pic);
    }

    let name = gtk::Label::new(Some(item.title.trim()));
    name.add_css_class("title-1");
    name.set_xalign(0.0);
    name.set_wrap(true);
    root.append(&name);

    let date = gtk::Label::new(Some(&fmt_tr_datetime(&item.created_at)));
    date.add_css_class("dim-label");
    date.set_xalign(0.0);
    root.append(&date);

    let body = gtk::Label::new(Some(item.body.trim()));
    body.set_xalign(0.0);
    body.set_wrap(true);
    body.set_selectable(true);
    root.append(&body);
    root
}

/// Yayın takvimi: 7 gün çipi + seçili günün bölümleri.
#[allow(clippy::too_many_arguments)]
pub fn calendar_view(
    days: &[CalendarDay],
    sel: usize,
    loading: bool,
    error: Option<String>,
    on_day: impl Fn(usize) + 'static,
    on_title: impl Fn(Title) + 'static,
    cover_loader: impl Fn(Option<&str>, &gtk::Picture, i32, i32) + 'static,
    on_retry: impl Fn() + 'static,
) -> gtk::Box {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
    root.set_hexpand(true);
    root.set_halign(gtk::Align::Fill);
    root.set_margin_top(8);
    root.set_margin_bottom(12);
    root.set_margin_start(12);
    root.set_margin_end(12);

    if let Some(err) = error {
        if days.is_empty() && !loading {
            let sp = crate::ui::components::create_status_page(
                "Takvim Yüklenemedi",
                &err,
                "network-error-symbolic",
            );
            root.append(&sp);
            let retry_btn = gtk::Button::with_label("Tekrar Dene");
            retry_btn.add_css_class("suggested-action");
            retry_btn.add_css_class("pill");
            retry_btn.set_halign(gtk::Align::Center);
            retry_btn.set_margin_top(8);
            let on_r = Rc::new(on_retry);
            retry_btn.connect_clicked(move |_| {
                on_r();
            });
            root.append(&retry_btn);
            return root;
        }
    }

    if loading && days.is_empty() {
        let spin_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        spin_row.set_halign(gtk::Align::Center);
        spin_row.set_margin_top(24);
        let sp = gtk::Spinner::new();
        sp.start();
        let lbl = gtk::Label::new(Some("Yayın takvimi yükleniyor…"));
        lbl.add_css_class("dim-label");
        spin_row.append(&sp);
        spin_row.append(&lbl);
        root.append(&spin_row);
        return root;
    }

    if days.is_empty() {
        let sp = crate::ui::components::create_status_page(
            "Takvim Boş",
            "Bu hafta için yayın bilgisi bulunamadı.",
            "x-office-calendar-symbolic",
        );
        root.append(&sp);
        return root;
    }

    // Gün çipleri.
    let chips = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    chips.set_halign(gtk::Align::Center);
    chips.set_margin_bottom(4);
    let on_day_rc = Rc::new(on_day);
    for (idx, day) in days.iter().enumerate() {
        let btn = gtk::Button::with_label(&format!(
            "{} · {}",
            fmt_tr_day(&day.date),
            day.episodes.len()
        ));
        btn.add_css_class("pill");
        if idx == sel {
            btn.add_css_class("suggested-action");
        }
        let on_d = on_day_rc.clone();
        btn.connect_clicked(move |_| {
            on_d(idx);
        });
        chips.append(&btn);
    }
    root.append(&chips);

    let Some(day) = days.get(sel.min(days.len() - 1)) else {
        return root;
    };
    if day.episodes.is_empty() {
        let sp = crate::ui::components::create_status_page(
            "Bu Gün Yayın Yok",
            "Seçili günde yayınlanacak bölüm bulunamadı.",
            "x-office-calendar-symbolic",
        );
        root.append(&sp);
        return root;
    }

    let list_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
    list_box.set_hexpand(true);
    list_box.set_halign(gtk::Align::Fill);

    let cover_rc = Rc::new(cover_loader);
    let open_rc = Rc::new(on_title);
    for ep in &day.episodes {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        row.add_css_class("history-item-card");
        row.set_margin_top(4);
        row.set_margin_bottom(4);
        row.set_margin_start(6);
        row.set_margin_end(6);

        let thumb = ep.poster.as_deref().or(ep.title.poster.as_deref());
        let (tw, th) = if ep.poster.is_some() { (96, 54) } else { (48, 72) };
        let pic = crate::covers::new_sized_picture(tw, th);
        pic.set_valign(gtk::Align::Center);
        cover_rc(thumb, &pic, tw, th);
        row.append(&pic);

        let text_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
        text_box.set_valign(gtk::Align::Center);
        text_box.set_hexpand(true);

        let anime = if ep.title.name.is_empty() {
            format!("Anime #{}", ep.title.id)
        } else {
            ep.title.name.clone()
        };
        let name = gtk::Label::new(Some(&anime));
        name.add_css_class("title-4");
        name.set_xalign(0.0);
        name.set_wrap(false);
        name.set_single_line_mode(true);
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        text_box.append(&name);

        let mut sub = String::new();
        if ep.season > 0 && ep.episode > 0 {
            sub.push_str(&format!("S{:02}E{:02}", ep.season, ep.episode));
        }
        if !ep.name.is_empty() {
            if !sub.is_empty() {
                sub.push_str(" · ");
            }
            sub.push_str(&ep.name);
        }
        if !sub.is_empty() {
            let sub_lbl = gtk::Label::new(Some(&sub));
            sub_lbl.add_css_class("dim-label");
            sub_lbl.set_xalign(0.0);
            sub_lbl.set_wrap(false);
            sub_lbl.set_single_line_mode(true);
            sub_lbl.set_ellipsize(gtk::pango::EllipsizeMode::End);
            text_box.append(&sub_lbl);
        }
        row.append(&text_box);

        let time = fmt_tr_time(&ep.release_date);
        if !time.is_empty() {
            let time_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
            time_box.set_valign(gtk::Align::Center);
            let time_lbl = gtk::Label::new(Some(&time));
            time_lbl.add_css_class("title-4");
            time_lbl.set_xalign(1.0);
            let tsi = gtk::Label::new(Some("TSİ"));
            tsi.add_css_class("dim-label");
            tsi.set_xalign(1.0);
            time_box.append(&time_lbl);
            time_box.append(&tsi);
            row.append(&time_box);
        }

        let t = ep.title.clone();
        let on_o = open_rc.clone();
        let gesture = gtk::GestureClick::new();
        gesture.connect_pressed(move |_, _, _, _| {
            on_o(t.clone());
        });
        row.add_controller(gesture);

        list_box.append(&row);
    }
    root.append(&list_box);
    root
}
