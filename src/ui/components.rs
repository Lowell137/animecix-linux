use gtk::prelude::*;
use crate::api::{Client, Title};

pub fn bookmark_button(client: &Client, t: &Title) -> gtk::Button {
    let saved = client.is_saved(t.id);
    let b = gtk::Button::from_icon_name(if saved {
        "starred-symbolic"
    } else {
        "non-starred-symbolic"
    });
    b.add_css_class("flat");
    b.add_css_class("circular");
    b.add_css_class("lg-icon");
    b.add_css_class("bookmark-btn");
    // Satır yüksekliğine göre dikey esneyip ovalleşmesin: hep yuvarlak kalsın.
    b.set_valign(gtk::Align::Center);
    b.set_tooltip_text(Some(if saved { "Favorilerden Çıkar" } else { "Favorilere Ekle" }));
    b
}

pub fn marathon_button(client: &Client, t: &Title) -> gtk::Button {
    let in_marathon = client.is_in_marathon(t.id);
    let b = gtk::Button::from_icon_name(if in_marathon {
        "media-playlist-repeat-symbolic"
    } else {
        "media-playlist-consecutive-symbolic"
    });
    b.add_css_class("flat");
    b.add_css_class("circular");
    b.add_css_class("lg-icon");
    b.add_css_class("bookmark-btn");
    // Satır yüksekliğine göre dikey esneyip ovalleşmesin: hep yuvarlak kalsın.
    b.set_valign(gtk::Align::Center);
    b.set_tooltip_text(Some(if in_marathon { "Maratondan Çıkar" } else { "İzleme Maratonuna Ekle" }));
    b
}

/// İkon + yazılı buton. Emoji yerine symbolic ikon kullanmak için.
pub fn icon_button(icon_name: &str, label: &str) -> gtk::Button {
    let b = gtk::Button::new();
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    row.append(&gtk::Image::from_icon_name(icon_name));
    row.append(&gtk::Label::new(Some(label)));
    b.set_child(Some(&row));
    b
}

/// İkon + yazılı durum etiketi (ikisi de sonradan değişebilir).
pub fn status_label(icon_name: &str, text: &str) -> (gtk::Box, gtk::Image, gtk::Label) {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let img = gtk::Image::from_icon_name(icon_name);
    let lbl = gtk::Label::new(Some(text));
    row.append(&img);
    row.append(&lbl);
    (row, img, lbl)
}

/// Seçili olduğunu ikonla gösteren, sola hizalı düz buton.
pub fn check_button(label: &str, checked: bool) -> gtk::Button {
    let b = gtk::Button::new();
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let img = if checked {
        gtk::Image::from_icon_name("object-select-symbolic")
    } else {
        gtk::Image::new()
    };
    img.set_size_request(16, -1);
    let lbl = gtk::Label::new(Some(label));
    lbl.set_xalign(0.0);
    lbl.set_hexpand(true);
    row.append(&img);
    row.append(&lbl);
    b.set_child(Some(&row));
    b.add_css_class("flat");
    b
}

pub fn create_status_page(title: &str, description: &str, icon_name: &str) -> adw::StatusPage {
    let sp = adw::StatusPage::new();
    sp.set_title(title);
    sp.set_description(Some(description));
    sp.set_icon_name(Some(icon_name));
    sp.set_vexpand(true);
    sp
}
