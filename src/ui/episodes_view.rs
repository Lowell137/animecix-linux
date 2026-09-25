//! Bölüm listesi görünümü: kart, butonlar, sağ-tık menü, toplu indirme.
use gtk::prelude::*;
use crate::api::{Episode, Title};
use crate::ui::row_menu::RowMenu;

fn create_fact_badges(facts: &[String]) -> gtk::Box {
    let box_ = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    box_.set_halign(gtk::Align::Start);
    for f in facts {
        let b = gtk::Label::new(Some(f));
        b.add_css_class("detail-badge");
        box_.append(&b);
    }
    box_
}

pub fn append_title_submeta(info_box: &gtk::Box, t: &Title) {
    if let Some(g) = t.genre_line() {
        let gl = gtk::Label::new(Some(&g));
        gl.add_css_class("dim-label");
        gl.set_xalign(0.0);
        gl.set_wrap(false);
        gl.set_single_line_mode(true);
        gl.set_ellipsize(gtk::pango::EllipsizeMode::End);
        info_box.append(&gl);
    }
    let meta = gtk::Label::new(Some(&t.meta_line()));
    meta.add_css_class("dim-label");
    meta.set_xalign(0.0);
    meta.set_wrap(false);
    meta.set_single_line_mode(true);
    meta.set_ellipsize(gtk::pango::EllipsizeMode::End);
    info_box.append(&meta);
}

/// Başlık detay başlığı (dizi üst kısmı): kapak + bilgi. Çerçevesiz, sayfa
/// arka planı üzerine doğrudan oturur (Kitsune gibi). Eylem butonları artık
/// başlık çubuğunun sağ üstünde (App::detail_actions).
pub fn create_title_detail_header(
    title: &Title,
    poster_widget: &gtk::Picture,
) -> gtk::Box {
    let card = gtk::Box::new(gtk::Orientation::Horizontal, 24);
    card.add_css_class("title-detail-card");
    card.set_margin_top(8);
    card.set_margin_bottom(12);
    card.set_margin_start(16);
    card.set_margin_end(16);
    poster_widget.add_css_class("detail-poster");
    poster_widget.set_valign(gtk::Align::Start);
    card.append(poster_widget);

    // Kitsune gibi: bilgi sütunu sağda, posterin altına doğru yaslanır.
    let info_box = gtk::Box::new(gtk::Orientation::Vertical, 8);
    info_box.set_hexpand(true);
    info_box.set_valign(gtk::Align::End);

    // Kitsune gibi: başlıkta yıl YOK (yıl aşağıda "Yıl" satırında zaten var).
    let name_lbl = gtk::Label::new(Some(&title.name));
    name_lbl.add_css_class("title-1");
    name_lbl.set_xalign(0.0);
    name_lbl.set_wrap(true);
    name_lbl.set_hexpand(true);
    info_box.append(&name_lbl);

    // Tür çipleri.
    let chips = title.genre_chips();
    if !chips.is_empty() {
        let chip_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        chip_row.set_halign(gtk::Align::Start);
        for g in &chips {
            let c = gtk::Label::new(Some(g));
            c.add_css_class("detail-badge");
            chip_row.append(&c);
        }
        info_box.append(&chip_row);
    }

    // Puan rozeti.
    if let Some(r) = title.rating {
        if r > 0.0 {
            let pill = gtk::Label::new(Some(&format!("★ {:.1}", r)));
            pill.add_css_class("rating-pill");
            pill.set_halign(gtk::Align::Start);
            info_box.append(&pill);
        }
    }

    // Dikey künye bloğu (Tür / Yıl / Bölüm / Süre / Yayın) — etiket soluk, değer beyaz.
    let rows = title.detail_rows();
    if !rows.is_empty() {
        let grid = gtk::Grid::new();
        grid.set_row_spacing(2);
        grid.set_column_spacing(10);
        grid.set_halign(gtk::Align::Start);
        for (i, (label, value)) in rows.iter().enumerate() {
            let l = gtk::Label::new(Some(label));
            l.add_css_class("dim-label");
            l.set_xalign(0.0);
            let v = gtk::Label::new(Some(value));
            v.set_xalign(0.0);
            grid.attach(&l, 0, i as i32, 1, 1);
            grid.attach(&v, 1, i as i32, 1, 1);
        }
        info_box.append(&grid);
    }

    card.append(&info_box);
    card
}

/// Kitsune gibi: açıklama (synopsis) başlık bloğunun ALTINDA, tam genişlik.
/// `create_title_detail_header` info sütunundan çıkarıldı; sayfa köküne eklenir.
pub fn create_detail_description(title: &Title) -> Option<gtk::Label> {
    let desc = title.description.as_deref()?.trim();
    if desc.is_empty() {
        return None;
    }
    let lbl = gtk::Label::new(Some(desc));
    lbl.add_css_class("detail-desc");
    lbl.set_xalign(0.0);
    lbl.set_wrap(true);
    lbl.set_margin_start(16);
    lbl.set_margin_end(16);
    lbl.set_margin_top(4);
    lbl.set_margin_bottom(8);
    Some(lbl)
}

pub fn create_quick_search_tip_banner(shortcut_str: &str, on_dismiss: impl Fn() + 'static) -> gtk::Box {
    let banner = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    banner.add_css_class("tip-banner");

    let icon = gtk::Image::from_icon_name("dialog-information-symbolic");
    icon.set_icon_size(gtk::IconSize::Normal);
    icon.set_valign(gtk::Align::Center);

    let text = gtk::Label::new(Some(&format!(
        "Klavyeden '{}' kısayoluna basarak bölüm listesinde hızlıca arama yapabilirsiniz.",
        shortcut_str
    )));
    text.add_css_class("tip-banner-text");
    text.set_xalign(0.0);
    text.set_wrap(true);
    text.set_hexpand(true);
    text.set_valign(gtk::Align::Center);

    let dismiss_btn = gtk::Button::with_label("Anladım");
    dismiss_btn.add_css_class("flat");
    dismiss_btn.add_css_class("pill");
    dismiss_btn.set_valign(gtk::Align::Center);
    let banner_clone = banner.clone();
    dismiss_btn.connect_clicked(move |_| {
        banner_clone.set_visible(false);
        on_dismiss();
    });

    banner.append(&icon);
    banner.append(&text);
    banner.append(&dismiss_btn);
    banner
}

pub fn create_right_click_tip_banner(on_dismiss: impl Fn() + 'static) -> gtk::Box {
    let banner = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    banner.add_css_class("tip-banner");

    let icon = gtk::Image::from_icon_name("input-mouse-symbolic");
    icon.set_icon_size(gtk::IconSize::Normal);
    icon.set_valign(gtk::Align::Center);

    let text = gtk::Label::new(Some(
        "Bir bölüme sağ tıklayarak o bölümü izlendi / izlenmedi olarak manuel işaretleyebilirsiniz.",
    ));
    text.add_css_class("tip-banner-text");
    text.set_xalign(0.0);
    text.set_wrap(true);
    text.set_hexpand(true);
    text.set_valign(gtk::Align::Center);

    let dismiss_btn = gtk::Button::with_label("Anladım");
    dismiss_btn.add_css_class("flat");
    dismiss_btn.add_css_class("pill");
    dismiss_btn.set_valign(gtk::Align::Center);
    let banner_clone = banner.clone();
    dismiss_btn.connect_clicked(move |_| {
        banner_clone.set_visible(false);
        on_dismiss();
    });

    banner.append(&icon);
    banner.append(&text);
    banner.append(&dismiss_btn);
    banner
}

pub fn create_movie_detail_view(
    title: &Title,
    poster_widget: &gtk::Picture,
    bookmark_btn: &gtk::Button,
    marathon_btn: &gtk::Button,
    progress: Option<(f64, f64)>,
    on_play: impl Fn() + 'static,
) -> (gtk::Box, gtk::ProgressBar, gtk::Label) {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 14);
    root.add_css_class("movie-big");
    root.set_margin_top(44);
    root.set_margin_bottom(40);
    root.set_margin_start(28);
    root.set_margin_end(28);
    root.set_size_request(760, -1);
    root.set_halign(gtk::Align::Center);
    root.set_vexpand(true);
    root.set_valign(gtk::Align::Center);

    poster_widget.set_halign(gtk::Align::Center);
    root.append(poster_widget);

    let name_lbl = gtk::Label::new(Some(&title.display_name()));
    name_lbl.add_css_class("title-1");
    name_lbl.set_xalign(0.5);
    name_lbl.set_halign(gtk::Align::Center);
    name_lbl.set_justify(gtk::Justification::Center);
    name_lbl.set_wrap(true);
    name_lbl.set_margin_start(24);
    name_lbl.set_margin_end(24);
    root.append(&name_lbl);

    if let Some(genre) = title.genre_line() {
        let genre_lbl = gtk::Label::new(Some(&genre));
        genre_lbl.add_css_class("dim-label");
        genre_lbl.add_css_class("title-4");
        genre_lbl.set_xalign(0.5);
        genre_lbl.set_halign(gtk::Align::Center);
        genre_lbl.set_justify(gtk::Justification::Center);
        genre_lbl.set_wrap(true);
        genre_lbl.set_margin_start(24);
        genre_lbl.set_margin_end(24);
        genre_lbl.set_margin_top(4);
        root.append(&genre_lbl);
    }

    let facts = title.detail_facts();
    if !facts.is_empty() {
        let badges = create_fact_badges(&facts);
        badges.set_halign(gtk::Align::Center);
        badges.set_margin_start(24);
        badges.set_margin_end(24);
        root.append(&badges);
    }

    if let Some(desc) = &title.description {
        let clean_desc = desc.trim();
        if !clean_desc.is_empty() {
            let desc_lbl = gtk::Label::new(Some(clean_desc));
            desc_lbl.add_css_class("dim-label");
            desc_lbl.set_xalign(0.5);
            desc_lbl.set_halign(gtk::Align::Center);
            desc_lbl.set_justify(gtk::Justification::Center);
            desc_lbl.set_wrap(true);
            desc_lbl.set_max_width_chars(80);
            desc_lbl.set_margin_start(24);
            desc_lbl.set_margin_end(24);
            desc_lbl.set_margin_top(12);
            root.append(&desc_lbl);
        }
    }

    let btn_row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    btn_row.set_halign(gtk::Align::Center);
    btn_row.set_valign(gtk::Align::Center);
    btn_row.append(bookmark_btn);
    btn_row.append(marathon_btn);

    let play_btn = crate::ui::components::icon_button("media-playback-start-symbolic", "Filmi İzle");
    play_btn.add_css_class("suggested-action");
    play_btn.add_css_class("pill");
    play_btn.set_margin_top(8);
    play_btn.connect_clicked(move |_| on_play());
    btn_row.append(&play_btn);
    root.append(&btn_row);

    let prog_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
    prog_box.set_halign(gtk::Align::Center);
    prog_box.set_size_request(520, -1);
    let pb = gtk::ProgressBar::new();
    pb.add_css_class("episode-progress");
    let lbl = gtk::Label::new(None);
    lbl.add_css_class("dim-label");
    lbl.set_xalign(0.5);
    if let Some((pos, dur)) = progress {
        if dur > 0.0 {
            pb.set_fraction((pos / dur).clamp(0.0, 1.0));
            lbl.set_text(&format!("{} / {}", fmt_time(pos), fmt_time(dur)));
        }
    }
    prog_box.append(&pb);
    prog_box.append(&lbl);
    root.append(&prog_box);

    (root, pb, lbl)
}

fn fmt_time(s: f64) -> String {
    let s = s as u64;
    if s >= 3600 { format!("{}:{:02}:{:02}", s/3600, (s%3600)/60, s%60) }
    else { format!("{}:{:02}", s/60, s%60) }
}