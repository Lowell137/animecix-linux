//! Kısayol yardım penceresi: uygulamanın gerçekten ele aldığı tuşlar.

use gtk::prelude::*;

/// Ayarlardaki "Ctrl+S" biçimini hızlandırıcı söz dizimine çevirir.
fn accel(sc: &str) -> String {
    match sc.strip_prefix("Ctrl+") {
        Some(k) => format!("<ctrl>{}", k.to_lowercase()),
        // "/" düz metin olarak ayrışmıyor; tuş adı verilmeli.
        None if sc == "/" => "slash".into(),
        None => sc.to_string(),
    }
}

/// Ayar değerleri kullanıcı dosyasından gelir; XML'e olduğu gibi girmez.
fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn item(sc: &str, title: &str) -> String {
    format!(
        r##"<child><object class="GtkShortcutsShortcut">
            <property name="accelerator">{}</property>
            <property name="title">{}</property>
          </object></child>"##,
        esc(sc),
        esc(title)
    )
}

fn group(title: &str, items: Vec<String>) -> String {
    format!(
        r##"<child><object class="GtkShortcutsGroup">
            <property name="title">{}</property>
            {}
          </object></child>"##,
        esc(title),
        items.concat()
    )
}

/// Yardım penceresinin arayüz tanımı.
fn ui_xml(search: &str, quick_search: &str, quick_enabled: bool) -> String {
    let mut groups = vec![group(
        "Genel",
        vec![
            item(&accel(search), "Aramayı aç"),
            item("Escape", "Arama penceresini kapat"),
        ],
    )];
    if quick_enabled {
        groups.push(group(
            "Bölüm Sayfası",
            vec![item(&accel(quick_search), "Bölüm listesinde ara")],
        ));
    }
    groups.push(group(
        "Oynatıcı",
        vec![
            item("space", "Oynat / duraklat"),
            item("Left", "10 saniye geri"),
            item("Right", "10 saniye ileri"),
            item("Up", "Sesi 5 artır"),
            item("Down", "Sesi 5 azalt"),
            item("m", "Sesi aç / kapat"),
            item("f", "Tam ekran"),
            item("a", "Sığdır / doldur"),
            item("s", "İntroyu atla (AniSkip)"),
            item("e", "Outroyu atla (AniSkip)"),
        ],
    ));
    format!(
        r#"<interface>
          <object class="GtkShortcutsWindow" id="shortcuts">
            <child><object class="GtkShortcutsSection">
              <property name="section-name">shortcuts</property>
              <property name="max-height">12</property>
              {}
            </object></child>
          </object>
        </interface>"#,
        groups.concat()
    )
}

/// Kısayol penceresini açar; ayarlardan gelen tuşlar listeye işlenir.
pub fn present(
    parent: &impl IsA<gtk::Window>,
    search_shortcut: &str,
    quick_search: &str,
    quick_enabled: bool,
) {
    let builder = gtk::Builder::new();
    if let Err(e) = builder.add_from_string(&ui_xml(search_shortcut, quick_search, quick_enabled)) {
        eprintln!("[KISAYOL] pencere kurulamadı: {e}");
        return;
    }
    let Some(win) = builder.object::<gtk::ShortcutsWindow>("shortcuts") else {
        eprintln!("[KISAYOL] GtkShortcutsWindow bulunamadı");
        return;
    };
    win.set_transient_for(Some(parent));
    win.set_modal(true);
    win.present();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accel_uses_gtk_syntax() {
        assert_eq!(accel("Ctrl+S"), "<ctrl>s");
        assert_eq!(accel("Ctrl+K"), "<ctrl>k");
        assert_eq!(accel("/"), "slash");
        assert_eq!(accel("F2"), "F2");
    }

    /// Ayarlardaki her olası değer ayrışabilmeli ve pencere kurulabilmeli.
    /// `gtk::accelerator_parse` ile pencere kurma GTK başlatmayı ister → tek test.
    /// Görüntü sunucusu ister; `cargo test -- --ignored shortcuts_window` ile denenir.
    #[test]
    #[ignore]
    fn shortcuts_window_builds() {
        gtk::init().expect("gtk başlatılamadı");
        for sc in ["Ctrl+S", "Ctrl+K", "F2", "/", "Ctrl+F", "F3"] {
            let a = accel(sc);
            assert!(
                gtk::accelerator_parse(&a).is_some(),
                "{sc} → {a} ayrışmıyor"
            );
        }
        for a in [
            "space", "Escape", "Left", "Right", "Up", "Down", "m", "f", "a", "s", "e",
        ] {
            assert!(gtk::accelerator_parse(a).is_some(), "{a} ayrışmıyor");
        }
        // accel()'in "/" → "slash" çevirisinin nedeni.
        assert!(gtk::accelerator_parse("/").is_none());

        let builder = gtk::Builder::new();
        builder
            .add_from_string(&ui_xml("Ctrl+S", "/", true))
            .expect("arayüz tanımı çözülemedi");
        let _win = builder
            .object::<gtk::ShortcutsWindow>("shortcuts")
            .expect("GtkShortcutsWindow yok");
    }

    #[test]
    fn xml_includes_settings_and_drops_quick_group() {
        let with = ui_xml("F2", "Ctrl+F", true);
        assert!(with.contains(r#"<property name="accelerator">&lt;ctrl&gt;f</property>"#));
        assert!(with.contains("Aramayı aç"));
        assert!(with.contains("İntroyu atla"));
        assert!(with.contains("Bölüm Sayfası"));
        let without = ui_xml("Ctrl+S", "/", false);
        assert!(!without.contains("Bölüm Sayfası"));
        assert!(without.contains(r#"<property name="accelerator">&lt;ctrl&gt;s</property>"#));
    }

    #[test]
    fn xml_escapes_setting_values() {
        let x = ui_xml("<b>&x", "/", true);
        assert!(!x.contains("<b>"), "ham etiket kaçınılmalı");
        assert!(x.contains("&lt;b&gt;&amp;x"));
        assert!(x.contains(r#"<property name="accelerator">slash</property>"#));
    }
}
