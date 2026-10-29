//! İndirilenler sayfası: kuyruk listesi + durum + aksiyonlar.

use gtk::prelude::*;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use crate::download::{DownloadManager, DownloadRecord, DownloadStatus};

pub struct DownloadsView;

impl DownloadsView {
    /// Satır metinleri: (oran, bar yazısı, durum ikonu, durum yazısı).
    pub fn row_state(rec: &DownloadRecord) -> (f64, String, &'static str, String) {
        match &rec.status {
            DownloadStatus::Done => (
                1.0,
                "Tamamlandı".to_string(),
                "object-select-symbolic",
                "Tamamlandı".to_string(),
            ),
            DownloadStatus::Error(e) => (
                0.0,
                "Hata".to_string(),
                "dialog-error-symbolic",
                e.clone(),
            ),
            DownloadStatus::Paused => (
                if rec.total > 0 { rec.have as f64 / rec.total as f64 } else { 0.0 },
                "Duraklatıldı".to_string(),
                "media-playback-pause-symbolic",
                "Duraklatıldı".to_string(),
            ),
            DownloadStatus::Queued if rec.have == 0 => (
                0.0,
                "Bekleniyor".to_string(),
                "alarm-symbolic",
                "Bekleniyor (sırayla indiriliyor)".to_string(),
            ),
            DownloadStatus::Queued => {
                let f = if rec.total > 0 { rec.have as f64 / rec.total as f64 } else { 0.0 };
                let pct = (f * 100.0) as u64;
                (
                    f.clamp(0.0, 1.0),
                    format!("%{pct}"),
                    "alarm-symbolic",
                    format!(
                        "Bekleniyor (sırayla indiriliyor) — %{pct} ({} / {})",
                        crate::download::fmt_bytes(rec.have),
                        crate::download::fmt_bytes(rec.total)
                    ),
                )
            }
            DownloadStatus::Downloading if rec.total == 0 => (
                0.0,
                "%0".to_string(),
                "folder-download-symbolic",
                "Bağlanıyor…".to_string(),
            ),
            _ => {
                let f = if rec.total > 0 { rec.have as f64 / rec.total as f64 } else { 0.0 };
                let pct = (f * 100.0) as u64;
                (
                    f.clamp(0.0, 1.0),
                    format!("%{pct}"),
                    "folder-download-symbolic",
                    format!(
                        "%{pct} ({} / {})",
                        crate::download::fmt_bytes(rec.have),
                        crate::download::fmt_bytes(rec.total)
                    ),
                )
            }
        }
    }

    /// Sayfayı kurar; ilerleme Tick'lerinde yerinde güncelleme için satır tutamaçlarını döner.
    pub fn build(
        manager: &DownloadManager,
        open_dir: PathBuf,
        on_play: Rc<dyn Fn(DownloadRecord)>,
        on_delete: Rc<dyn Fn(DownloadRecord)>,
    ) -> (
        gtk::ScrolledWindow,
        HashMap<String, (gtk::ProgressBar, gtk::Image, gtk::Label)>,
    ) {
        let mut rows: HashMap<String, (gtk::ProgressBar, gtk::Image, gtk::Label)> = HashMap::new();
        let scroll = gtk::ScrolledWindow::new();
        scroll.add_css_class("clear-scroll");
        scroll.set_hexpand(true);
        scroll.set_vexpand(true);
        scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        let items = manager.snapshot();
        if items.is_empty() {
            let sp = crate::ui::components::create_status_page(
                "İndirme Yok",
                "Bölüm sayfasındaki indirme düğmesiyle bölüm indirin; kuyruk burada görünür.",
                "folder-download-symbolic",
            );
            scroll.set_child(Some(&sp));
            return (scroll, rows);
        }

        let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
        root.set_margin_top(12);
        root.set_margin_bottom(12);
        root.set_margin_start(12);
        root.set_margin_end(12);

        let head = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let count = gtk::Label::new(Some(&format!("{} kayıt", items.len())));
        count.add_css_class("title-4");
        count.set_xalign(0.0);
        count.set_hexpand(true);
        head.append(&count);
        // Tümü aktifse temizlenecek kayıt yok: düğme gereksiz.
        let cleanable = items
            .iter()
            .filter(|r| matches!(r.status, DownloadStatus::Done | DownloadStatus::Error(_)))
            .count();
        // Kuyrukta/indirmede olan varsa toplu duraklatma anlamlı.
        let pausable = items.iter().any(|r| {
            matches!(r.status, DownloadStatus::Queued | DownloadStatus::Downloading)
        });
        // Duraklatılmış/hatalı kayıt varsa toplu devam anlamlı.
        let resumable = items
            .iter()
            .any(|r| matches!(r.status, DownloadStatus::Paused | DownloadStatus::Error(_)));
        if pausable {
            let pause =
                crate::ui::components::icon_button("media-playback-pause-symbolic", "Tümünü Duraklat");
            pause.add_css_class("flat");
            pause.add_css_class("pill");
            pause.set_tooltip_text(Some(
                "Bekleyen ve süren tüm indirmeleri duraklatır.",
            ));
            let m = manager.clone();
            pause.connect_clicked(move |_| {
                m.pause_all();
            });
            head.append(&pause);
        }
        if resumable {
            let resume =
                crate::ui::components::icon_button("media-playback-start-symbolic", "Tümüne Devam");
            resume.add_css_class("flat");
            resume.add_css_class("pill");
            resume.set_tooltip_text(Some("Duraklatılan ve hatalı tüm indirmeleri kuyruğa geri alır."));
            let m = manager.clone();
            resume.connect_clicked(move |_| {
                m.resume_all();
            });
            head.append(&resume);
        }
        if cleanable > 0 {
            let clear = crate::ui::components::icon_button("user-trash-symbolic", "Geçmişi Temizle");
            clear.add_css_class("flat");
            clear.add_css_class("pill");
            clear.set_tooltip_text(Some(
                "Tamamlanan ve hatalı kayıtları listeden çıkarır; indirilen dosyalar silinmez.",
            ));
            let m = manager.clone();
            clear.connect_clicked(move |_| {
                m.clear_finished();
            });
            head.append(&clear);
        }
        let open_btn = crate::ui::components::icon_button("folder-symbolic", "Klasörü Aç");
        open_btn.add_css_class("flat");
        open_btn.add_css_class("pill");
        open_btn.connect_clicked(move |_| {
            let _ = std::process::Command::new("xdg-open").arg(&open_dir).spawn();
        });
        head.append(&open_btn);
        root.append(&head);

        for rec in items {
            let card = gtk::Box::new(gtk::Orientation::Vertical, 2);
            card.add_css_class("card");
            card.set_margin_top(4);
            card.set_margin_bottom(4);
            card.set_margin_start(8);
            card.set_margin_end(8);

            let title = gtk::Label::new(Some(&format!(
                "{} | S{:02}E{:02} [{}][{}]",
                rec.title, rec.season, rec.episode, rec.fansub, rec.quality
            )));
            title.add_css_class("title-4");
            title.set_xalign(0.0);
            title.set_max_width_chars(48);
            title.set_lines(1);
            title.set_ellipsize(gtk::pango::EllipsizeMode::End);
            title.set_margin_start(8);
            title.set_margin_end(8);
            title.set_margin_top(6);
            card.append(&title);

            let bar = gtk::ProgressBar::new();
            bar.set_show_text(true);
            let (frac, txt, sicon, stxt) = Self::row_state(&rec);
            bar.set_fraction(frac);
            bar.set_text(Some(&txt));
            bar.set_margin_start(8);
            bar.set_margin_end(8);
            card.append(&bar);

            let foot = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            foot.set_margin_start(8);
            foot.set_margin_end(8);
            foot.set_margin_bottom(6);
            let (status_row, status_icon, status) =
                crate::ui::components::status_label(sicon, &stxt);
            status_row.add_css_class("dim-label");
            status.set_xalign(0.0);
            status_row.set_hexpand(true);
            status.set_ellipsize(gtk::pango::EllipsizeMode::End);
            foot.append(&status_row);

            let id = rec.id.clone();
            rows.insert(id.clone(), (bar.clone(), status_icon.clone(), status.clone()));
            match &rec.status {
                DownloadStatus::Downloading | DownloadStatus::Queued => {
                    let b = crate::ui::components::icon_button("media-playback-pause-symbolic", "Duraklat");
                    b.add_css_class("flat");
                    b.add_css_class("pill");
                    let m = manager.clone();
                    let idc = id.clone();
                    b.connect_clicked(move |_| m.pause(&idc));
                    foot.append(&b);
                }
                DownloadStatus::Paused | DownloadStatus::Error(_) => {
                    let b = crate::ui::components::icon_button("media-playback-start-symbolic", "Devam");
                    b.add_css_class("flat");
                    b.add_css_class("pill");
                    let m = manager.clone();
                    let idc = id.clone();
                    b.connect_clicked(move |_| m.resume(&idc));
                    foot.append(&b);
                }
                DownloadStatus::Done => {}
            }
            if !matches!(rec.status, DownloadStatus::Done) {
                let rm = crate::ui::components::icon_button("user-trash-symbolic", "Kaldır");
                rm.add_css_class("flat");
                rm.add_css_class("pill");
                let m = manager.clone();
                let idc = id.clone();
                rm.connect_clicked(move |_| m.remove(&idc));
                foot.append(&rm);
            } else {
                if rec.dest.is_file() {
                    let play =
                        crate::ui::components::icon_button("media-playback-start-symbolic", "İzle");
                    play.add_css_class("flat");
                    play.add_css_class("pill");
                    play.set_tooltip_text(Some("Diskteki dosyadan oynat"));
                    let on_play = on_play.clone();
                    let rec_c = rec.clone();
                    play.connect_clicked(move |_| on_play(rec_c.clone()));
                    foot.append(&play);
                }
                let open = crate::ui::components::icon_button("folder-symbolic", "Göster");
                open.add_css_class("flat");
                open.add_css_class("pill");
                let dest = rec.dest.clone();
                open.connect_clicked(move |_| {
                    if let Some(parent) = dest.parent() {
                        let _ = std::process::Command::new("xdg-open").arg(parent).spawn();
                    }
                });
                foot.append(&open);
                if rec.dest.is_file() {
                    let del = crate::ui::components::icon_button("edit-delete-symbolic", "Dosyayı Sil");
                    del.add_css_class("flat");
                    del.add_css_class("pill");
                    del.set_tooltip_text(Some("İndirilen dosyayı diskten siler"));
                    let on_delete = on_delete.clone();
                    let rec_c = rec.clone();
                    del.connect_clicked(move |_| on_delete(rec_c.clone()));
                    foot.append(&del);
                }
            }
            card.append(&foot);
            root.append(&card);
        }

        scroll.set_child(Some(&root));
        (scroll, rows)
    }
}
