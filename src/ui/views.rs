use gtk::prelude::*;
use adw::prelude::*;
use std::rc::Rc;
use std::cell::RefCell;
use crate::api::{Client, HistoryEntry, Settings, Title, marathon_summary};
use crate::ui::episodes_view;

/// Milisaniye epoch → "GG.AA" (sunucu geçmiş tarihleri için, UTC).
fn fmt_day_month(ms: u64) -> String {
    let days = ms / 86_400_000;
    let mut y: i64 = 1970;
    let mut d = days as i64;
    loop {
        let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
        let yd = if leap { 366 } else { 365 };
        if d < yd {
            break;
        }
        d -= yd;
        y += 1;
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let months = [
        31,
        if leap { 29 } else { 28 },
        31, 30, 31, 30, 31, 31, 30, 31, 30, 31,
    ];
    let mut m = 0;
    while m < 12 && d >= months[m] {
        d -= months[m];
        m += 1;
    }
    format!("{:02}.{:02}", d + 1, m + 1)
}

pub(crate) fn show_info_dialog(parent: Option<&gtk::Window>, heading: &str, body: &str) {
    let dialog = adw::MessageDialog::builder()
        .heading(heading)
        .body(body)
        .close_response("ok")
        .default_response("ok")
        .build();
    if let Some(win) = parent {
        dialog.set_transient_for(Some(win));
    }
    dialog.add_response("ok", "Tamam");
    dialog.present();
}

pub struct MarathonView;

impl MarathonView {
    pub fn build(
        client: std::sync::Arc<Client>,
        on_item_click: impl Fn(Title) + 'static,
        on_toggle_completed: impl Fn(u64) + 'static,
        on_remove_item: impl Fn(u64) + 'static,
        on_clear_all: impl Fn() + 'static,
        on_reorder: impl Fn(u64, usize) + 'static,
        cover_loader: impl Fn(Option<&str>, &gtk::Picture, i32, i32) + 'static,
    ) -> gtk::Box {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 12);
        root.set_margin_top(12);
        root.set_margin_bottom(12);
        root.set_margin_start(16);
        root.set_margin_end(16);

        let marathon_items = client.get_marathon();
        if marathon_items.is_empty() {
            let sp = crate::ui::components::create_status_page(
                "İzleme Maratonunuz Boş",
                "Gelecekte izleyeceğiniz anime, dizi ve filmleri detay sayfasındaki maraton butonuna () tıklayarak ekleyebilirsiniz.",
                "media-playlist-repeat-symbolic",
            );
            root.append(&sp);
            return root;
        }

        let total_count = marathon_items.len();

        let summary_card = gtk::Box::new(gtk::Orientation::Vertical, 10);
        summary_card.add_css_class("marathon-summary-card");

        let top_row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        let header_lbl = gtk::Label::new(Some("İzleme Maratonu İlerlemesi"));
        header_lbl.add_css_class("title-2");
        header_lbl.set_xalign(0.0);
        header_lbl.set_hexpand(true);

        let clear_btn = gtk::Button::with_label("Tümünü Temizle");
        clear_btn.add_css_class("destructive-action");
        clear_btn.add_css_class("pill");
        let on_clear_all_rc = Rc::new(on_clear_all);
        let on_clear_c = on_clear_all_rc.clone();
        clear_btn.connect_clicked(move |_| on_clear_c());

        top_row.append(&header_lbl);
        top_row.append(&clear_btn);

        let stats_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let stats_lbl = gtk::Label::new(Some(&format!(
            "… / {} Anime Tamamlandı", total_count
        )));
        stats_lbl.add_css_class("title-4");
        stats_lbl.add_css_class("dim-label");
        stats_lbl.set_xalign(0.0);
        stats_lbl.set_hexpand(true);

        let percent_pill = gtk::Label::new(Some("%…"));
        percent_pill.add_css_class("marathon-percent-pill");

        stats_row.append(&stats_lbl);
        stats_row.append(&percent_pill);

        let pbar = gtk::ProgressBar::new();
        pbar.add_css_class("episode-progress");
        pbar.set_fraction(0.0);

        summary_card.append(&top_row);
        summary_card.append(&stats_row);
        summary_card.append(&pbar);

        root.append(&summary_card);

        let summary_titles: Vec<Title> = marathon_items.iter().map(|m| m.title.clone()).collect();
        let client_s = client.clone();
        let (stx, srx) = std::sync::mpsc::channel::<(usize, u32)>();
        std::thread::spawn(move || {
            let fracs: Vec<f64> = summary_titles.iter().map(|t| client_s.title_progress_frac(t)).collect();
            let _ = stx.send(marathon_summary(&fracs));
        });
        glib::idle_add_local(move || match srx.try_recv() {
            Ok((done, percent)) => {
                stats_lbl.set_text(&format!("{} / {} Anime Tamamlandı", done, total_count));
                percent_pill.set_text(&format!("%{}", percent));
                pbar.set_fraction((percent as f64 / 100.0).clamp(0.0, 1.0));
                glib::ControlFlow::Break
            }
            Err(_) => glib::ControlFlow::Continue,
        });

        let list_box = gtk::Box::new(gtk::Orientation::Vertical, 8);

        let on_item_click_rc = Rc::new(on_item_click);
        let on_toggle_rc = Rc::new(on_toggle_completed);
        let on_remove_rc = Rc::new(on_remove_item);
        let on_reorder_rc = Rc::new(on_reorder);
        let cover_loader_rc = Rc::new(cover_loader);

        for (idx, item) in marathon_items.iter().enumerate() {
            let card_box = gtk::Box::new(gtk::Orientation::Horizontal, 12);
            card_box.add_css_class("marathon-item-card");

            let num = gtk::Label::new(Some(&format!("{}", idx + 1)));
            num.add_css_class("marathon-index");
            num.set_xalign(0.5);
            num.set_yalign(0.5);
            num.set_valign(gtk::Align::Center);
            card_box.append(&num);

            let drag = gtk::DragSource::new();
            drag.set_actions(gtk::gdk::DragAction::MOVE);
            let card_for_icon = card_box.clone();
            let src_id = item.title.id;
            drag.connect_prepare(move |drag, _, _| {
                let alloc = card_for_icon.allocation();
                let paintable = gtk::WidgetPaintable::new(Some(&card_for_icon));
                drag.set_icon(Some(&paintable), alloc.width() / 2, alloc.height() / 2);
                Some(gtk::gdk::ContentProvider::for_value(
                    &glib::Value::from(src_id.to_string()),
                ))
            });
            let card_dim = card_box.clone();
            drag.connect_drag_begin(move |_, _| {
                card_dim.set_opacity(0.35);
            });
            let card_restore = card_box.clone();
            drag.connect_drag_end(move |_, _, _| {
                card_restore.set_opacity(1.0);
            });
            card_box.add_controller(drag);

            let drop = gtk::DropTarget::new(glib::Type::STRING, gtk::gdk::DragAction::MOVE);
            let self_id = item.title.id;
            let on_r = on_reorder_rc.clone();
            drop.connect_drop(move |_, value, _, _| {
                if let Ok(s) = value.get::<String>() {
                    if let Ok(src_id) = s.parse::<u64>() {
                        if src_id != self_id {
                            on_r(src_id, idx);
                        }
                    }
                }
                true
            });
            card_box.add_controller(drop);

            let chk = gtk::CheckButton::new();
            chk.set_active(item.completed);
            chk.set_valign(gtk::Align::Center);
            chk.set_tooltip_text(Some(if item.completed { "Tamamlandı olarak işaretli" } else { "Tamamlandı olarak işaretle" }));
            let tid = item.title.id;
            let on_t_c = on_toggle_rc.clone();
            let chk_guard = Rc::new(std::cell::Cell::new(false));
            let chk_guard_c = chk_guard.clone();
            chk.connect_toggled(move |_| { if chk_guard_c.get() { return; } on_t_c(tid); });

            let pic = gtk::Picture::new();
            pic.set_width_request(48);
            pic.set_height_request(72);
            pic.set_hexpand(false);
            pic.set_vexpand(false);
            pic.set_can_shrink(true);
            pic.set_content_fit(gtk::ContentFit::Cover);
            pic.set_css_classes(&["cover", "cover-thumb"]);
            pic.set_valign(gtk::Align::Center);
            cover_loader_rc(item.title.poster.as_deref(), &pic, 48, 72);

            let info_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
            info_box.set_valign(gtk::Align::Center);
            info_box.set_hexpand(true);
            let title_row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
            let name_lbl = gtk::Label::new(Some(&item.title.name));
            name_lbl.add_css_class("title-3");
            name_lbl.set_xalign(0.0);
            name_lbl.set_wrap(false);
            name_lbl.set_single_line_mode(true);
            name_lbl.set_ellipsize(gtk::pango::EllipsizeMode::End);
            if item.completed { name_lbl.add_css_class("dim-label"); }
            let (badge, badge_icon, badge_lbl) = crate::ui::components::status_label(
                if item.completed { "object-select-symbolic" } else { "alarm-symbolic" },
                if item.completed { "Tamamlandı" } else { "Devam Ediyor" },
            );
            badge.add_css_class(if item.completed { "status-badge-completed" } else { "status-badge-progress" });
            title_row.append(&name_lbl);
            title_row.append(&badge);
            info_box.append(&title_row);
            episodes_view::append_title_submeta(&info_box, &item.title);

            let prog = gtk::ProgressBar::new();
            prog.add_css_class("episode-progress");
            prog.set_margin_top(4);
            prog.set_valign(gtk::Align::Center);
            info_box.append(&prog);
            let client_c = client.clone();
            let t_c = item.title.clone();
            let (tx, rx) = std::sync::mpsc::channel::<f64>();
            std::thread::spawn(move || { let _ = tx.send(client_c.title_progress_frac(&t_c)); });
            let chk_u = chk.clone();
            let badge_u = badge.clone();
            let badge_icon_u = badge_icon.clone();
            let badge_lbl_u = badge_lbl.clone();
            let name_u = name_lbl.clone();
            let guard_u = chk_guard.clone();
            glib::idle_add_local(move || match rx.try_recv() {
                Ok(frac) => {
                    prog.set_fraction(frac);
                    let done = frac >= 0.999;
                    guard_u.set(true);
                    chk_u.set_active(done);
                    guard_u.set(false);
                    chk_u.set_tooltip_text(Some(if done { "Tamamlandı olarak işaretli" } else { "Tamamlandı olarak işaretle" }));
                    badge_lbl_u.set_text(if done { "Tamamlandı" } else { "Devam Ediyor" });
                    badge_icon_u.set_from_icon_name(Some(if done {
                        "object-select-symbolic"
                    } else {
                        "alarm-symbolic"
                    }));
                    badge_u.remove_css_class("status-badge-completed");
                    badge_u.remove_css_class("status-badge-progress");
                    badge_u.add_css_class(if done { "status-badge-completed" } else { "status-badge-progress" });
                    if done { name_u.add_css_class("dim-label"); } else { name_u.remove_css_class("dim-label"); }
                    glib::ControlFlow::Break
                }
                Err(_)   => glib::ControlFlow::Continue,
            });

            let actions = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            actions.set_valign(gtk::Align::Center);
            let play_btn = crate::ui::components::icon_button("media-playback-start-symbolic", "İzle");
            play_btn.add_css_class("suggested-action");
            play_btn.add_css_class("pill");
            let on_ic = on_item_click_rc.clone();
            let t_clone = item.title.clone();
            play_btn.connect_clicked(move |_| on_ic(t_clone.clone()));
            let del_btn = gtk::Button::from_icon_name("user-trash-symbolic");
            del_btn.add_css_class("flat");
            del_btn.add_css_class("circular");
            del_btn.add_css_class("destructive-action");
            del_btn.set_tooltip_text(Some("Maratondan Kaldır"));
            let on_rem = on_remove_rc.clone();
            del_btn.connect_clicked(move |_| on_rem(tid));
            actions.append(&play_btn);
            actions.append(&del_btn);

            card_box.append(&chk);
            card_box.append(&pic);
            card_box.append(&info_box);
            card_box.append(&actions);
            list_box.append(&card_box);
        }

        root.append(&list_box);
        root
    }
}

pub struct HistoryView;

impl HistoryView {
    pub fn build(
        _client: &Client,
        history: &[HistoryEntry],
        server: &[crate::api::ServerEntry],
        server_loading: bool,
        total: usize,
        cols: u32,
        on_delete_selected: impl Fn(Vec<u64>) + 'static,
        on_clear_all: impl Fn() + 'static,
        on_item_click: impl Fn(HistoryEntry) + 'static,
        on_card: impl Fn(&crate::api::Title, &str) -> gtk::Box + 'static,
        on_more: impl Fn() + 'static,
        server_error: Option<String>,
        on_retry: impl Fn() + 'static,
        cover_loader: impl Fn(Option<&str>, &gtk::Picture, i32, i32) + 'static,
    ) -> gtk::Box {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
        root.set_hexpand(true);
        root.set_halign(gtk::Align::Fill);
        root.set_margin_top(8);
        root.set_margin_bottom(8);
        root.set_margin_start(12);
        root.set_margin_end(12);

        if history.is_empty() && server.is_empty() && !server_loading {
            let sp = crate::ui::components::create_status_page(
                "İzleme Geçmişi Boş",
                "Henüz bir bölüm veya film izlemediniz.",
                "document-open-recent-symbolic",
            );
            root.append(&sp);
            return root;
        }

        let section = |text: &str| {
            let l = gtk::Label::new(Some(text));
            l.add_css_class("shelf-title");
            l.set_xalign(0.0);
            l.set_margin_top(6);
            l
        };

        if !history.is_empty() {
            root.append(&section("Bu cihazda izlenenler"));
        }

        let action_bar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        action_bar.set_margin_bottom(6);

        let select_all_chk = gtk::CheckButton::with_label("Tümünü Seç");
        select_all_chk.set_valign(gtk::Align::Center);

        let delete_sel_btn = gtk::Button::with_label("Seçilenleri Sil (0)");
        delete_sel_btn.add_css_class("destructive-action");
        delete_sel_btn.add_css_class("pill");
        delete_sel_btn.set_sensitive(false);
        delete_sel_btn.set_valign(gtk::Align::Center);

        let clear_all_btn = gtk::Button::with_label("Tümünü Temizle");
        clear_all_btn.add_css_class("flat");
        clear_all_btn.add_css_class("pill");
        clear_all_btn.set_valign(gtk::Align::Center);

        action_bar.append(&select_all_chk);
        action_bar.append(&delete_sel_btn);

        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        action_bar.append(&spacer);
        action_bar.append(&clear_all_btn);

        root.append(&action_bar);

        let list_box = gtk::Box::new(gtk::Orientation::Vertical, 5);
        list_box.set_hexpand(true);
        list_box.set_halign(gtk::Align::Fill);
        list_box.set_vexpand(false);

        let selected_ids = Rc::new(RefCell::new(Vec::<u64>::new()));
        let check_buttons = Rc::new(RefCell::new(Vec::<(u64, gtk::CheckButton)>::new()));
        let on_item_click_rc = Rc::new(on_item_click);

        let update_delete_btn = {
            let selected_ids = selected_ids.clone();
            let delete_sel_btn = delete_sel_btn.clone();
            move || {
                let count = selected_ids.borrow().len();
                delete_sel_btn.set_label(&format!("Seçilenleri Sil ({count})"));
                delete_sel_btn.set_sensitive(count > 0);
            }
        };

        for h in history {
            let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 10);
            row_box.add_css_class("history-item-card");
            row_box.set_vexpand(false);
            row_box.set_height_request(84);

            let chk = gtk::CheckButton::new();
            chk.set_valign(gtk::Align::Center);

            let tid = h.title.id;
            let sel_clone = selected_ids.clone();
            let upd_clone = update_delete_btn.clone();
            chk.connect_toggled(move |b| {
                let mut ids = sel_clone.borrow_mut();
                if b.is_active() {
                    if !ids.contains(&tid) { ids.push(tid); }
                } else {
                    ids.retain(|&id| id != tid);
                }
                drop(ids);
                upd_clone();
            });
            check_buttons.borrow_mut().push((tid, chk.clone()));

            let pic = crate::covers::new_sized_picture(48, 72);
            pic.set_valign(gtk::Align::Center);
            cover_loader(h.title.poster.as_deref(), &pic, 48, 72);

            let text_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
            text_box.set_vexpand(true);
            text_box.set_valign(gtk::Align::Center);
            text_box.set_hexpand(true);

            let name = gtk::Label::new(Some(&h.title.name));
            name.set_xalign(0.0);
            name.set_wrap(false);
            name.set_single_line_mode(true);
            name.set_ellipsize(gtk::pango::EllipsizeMode::End);
            name.add_css_class("title-3");

            let sub = gtk::Label::new(Some(&format!(
                "S{:02} E{:02} · {}",
                h.episode.season, h.episode.episode, h.episode.name
            )));
            sub.set_xalign(0.0);
            sub.set_wrap(false);
            sub.set_single_line_mode(true);
            sub.set_ellipsize(gtk::pango::EllipsizeMode::End);
            sub.add_css_class("dim-label");

            text_box.append(&name);
            text_box.append(&sub);
            episodes_view::append_title_submeta(&text_box, &h.title);

            let click_btn = crate::ui::components::icon_button("media-playback-start-symbolic", "İzle");
            click_btn.add_css_class("suggested-action");
            click_btn.add_css_class("pill");
            click_btn.set_valign(gtk::Align::Center);

            let h_clone = h.clone();
            let on_ic = on_item_click_rc.clone();
            click_btn.connect_clicked(move |_| {
                on_ic(h_clone.clone());
            });

            row_box.append(&chk);
            row_box.append(&pic);
            row_box.append(&text_box);
            row_box.append(&click_btn);

            list_box.append(&row_box);
        }

        let check_buttons_clone = check_buttons.clone();
        select_all_chk.connect_toggled(move |b| {
            let active = b.is_active();
            for (_, chk) in check_buttons_clone.borrow().iter() {
                chk.set_active(active);
            }
        });

        let selected_ids_clone = selected_ids.clone();
        let on_del = Rc::new(on_delete_selected);
        delete_sel_btn.connect_clicked(move |_| {
            let ids = selected_ids_clone.borrow().clone();
            if !ids.is_empty() {
                on_del(ids);
            }
        });

        let on_ca = Rc::new(on_clear_all);
        clear_all_btn.connect_clicked(move |_| {
            on_ca();
        });

        root.append(&list_box);

        // ---- sunucu geçmişi ----
        if let Some(err) = server_error {
            if server.is_empty() && !server_loading {
                let sp = crate::ui::components::create_status_page(
                    "Geçmiş Yüklenemedi",
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
        }        if server_loading && server.is_empty() {
            let spin_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            spin_row.set_margin_top(8);
            let sp = gtk::Spinner::new();
            sp.start();
            let lbl = gtk::Label::new(Some("Sitedeki geçmişin yükleniyor…"));
            lbl.add_css_class("dim-label");
            spin_row.append(&sp);
            spin_row.append(&lbl);
            root.append(&spin_row);
        }
        if !server.is_empty() {
            root.append(&section("Sitede izlenenler (son izlenen önce)"));
            // Sabit 5 sütun ızgara (diğer bölümlerle aynı kart boyu).
            let grid = gtk::Grid::new();
            grid.set_column_spacing(12);
            grid.set_row_spacing(18);
            grid.set_halign(gtk::Align::Center);
            grid.set_hexpand(true);
            grid.set_margin_top(6);
            grid.set_margin_bottom(6);
            let on_card_rc = Rc::new(on_card);
            let cols = cols.max(1);
            for (i, s) in server.iter().enumerate() {
                let t = &s.title;
                // Sunucunun bildiği son bölüm + tarih (örn. "S01E02 · 11.09").
                let mut sub = String::new();
                if s.season > 0 && s.episode > 0 {
                    sub.push_str(&format!("S{:02}E{:02}", s.season, s.episode));
                }
                if s.date > 0 {
                    if !sub.is_empty() {
                        sub.push_str(" · ");
                    }
                    sub.push_str(&fmt_day_month(s.date));
                }
                grid.attach(
                    &on_card_rc(t, &sub),
                    (i as u32 % cols) as i32,
                    (i as u32 / cols) as i32,
                    1,
                    1,
                );
            }
            root.append(&grid);
            // Progressive yükleme: "Daha Fazla Göster" sonraki sayfayı
            // API'den çekip listeye ekler (hepsi tek seferde inmez).
            if total > server.len() {
                let more_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
                more_box.set_halign(gtk::Align::Center);
                more_box.set_margin_top(10);
                more_box.set_margin_bottom(4);
                let more_btn = gtk::Button::with_label(&format!(
                    "Daha Fazla Göster ({} / {})",
                    server.len(),
                    total
                ));
                more_btn.add_css_class("pill");
                more_btn.set_sensitive(!server_loading);
                let on_m = Rc::new(on_more);
                more_btn.connect_clicked(move |_| {
                    on_m();
                });
                more_box.append(&more_btn);
                if server_loading {
                    let sp = gtk::Spinner::new();
                    sp.start();
                    sp.set_valign(gtk::Align::Center);
                    more_box.append(&sp);
                }
                root.append(&more_box);
            }
        }
        root
    }
}

pub struct SettingsView;

impl SettingsView {
    pub fn build(
        window: &adw::ApplicationWindow,
        settings: &Settings,
        on_save: impl Fn(Settings) + 'static,
        on_wipe: impl Fn(bool) + 'static,
    ) -> adw::PreferencesDialog {
        let dialog = adw::PreferencesDialog::new();
        dialog.set_title("Ayarlar");
        dialog.set_search_enabled(true);

        let page_view = adw::PreferencesPage::new();
        page_view.set_title("Görünüm");
        page_view.set_icon_name(Some("applications-graphics-symbolic"));

        let page_keys = adw::PreferencesPage::new();
        page_keys.set_title("Gezinme");
        page_keys.set_icon_name(Some("input-keyboard-symbolic"));

        let page_player = adw::PreferencesPage::new();
        page_player.set_title("Oynatıcı");
        page_player.set_icon_name(Some("media-playback-start-symbolic"));

        let page_data = adw::PreferencesPage::new();
        page_data.set_title("Depolama");
        page_data.set_icon_name(Some("drive-harddisk-symbolic"));

        let ep_group = adw::PreferencesGroup::new();
        ep_group.set_title("Hızlı Bölüm Arama");

        let search_toggle_row = adw::SwitchRow::new();
        search_toggle_row.set_title("Aktif");
        search_toggle_row.set_subtitle("Bölüm ekranında klavye kısayolu ile hızlı bölüm arama çubuğunu aktif et");
        search_toggle_row.set_active(settings.quick_search_enabled);

        let shortcut_row = adw::ComboRow::new();
        shortcut_row.set_title("Kısayol Tuşu");
        shortcut_row.set_subtitle("Bölüm sayfasında aramayı başlatacak klavye kısayolu");
        let ep_shortcuts = &["/", "Ctrl+F", "F3", "Ctrl+K"];
        let ep_shortcut_model = gtk::StringList::new(ep_shortcuts);
        shortcut_row.set_model(Some(&ep_shortcut_model));
        let current_ep_sc = ep_shortcuts.iter().position(|&s| s == settings.quick_search_shortcut).unwrap_or(0);
        shortcut_row.set_selected(current_ep_sc as u32);
        shortcut_row.set_sensitive(settings.quick_search_enabled);

        ep_group.add(&search_toggle_row);
        ep_group.add(&shortcut_row);
        page_keys.add(&ep_group);

        let search_group = adw::PreferencesGroup::new();
        search_group.set_title("Anime / Dizi Arama Kısayolu");

        let search_sc_row = adw::ComboRow::new();
        search_sc_row.set_title("Kısayol Tuşu");
        search_sc_row.set_subtitle("Ana ekranda arama penceresini açacak klavye kısayolu");
        let search_shortcuts = &["Ctrl+S", "Ctrl+K", "F2", "/"];
        let search_sc_model = gtk::StringList::new(search_shortcuts);
        search_sc_row.set_model(Some(&search_sc_model));
        let current_sc = search_shortcuts.iter().position(|&s| s == settings.search_shortcut).unwrap_or(0);
        search_sc_row.set_selected(current_sc as u32);
        search_group.add(&search_sc_row);
        page_keys.add(&search_group);
        let sidebar_group = adw::PreferencesGroup::new();
        sidebar_group.set_title("Sidebar Sekmeleri");
        let sidebar_rows: Vec<(&'static str, adw::SwitchRow)> = [
            ("kesfet", "Keşfet"),
            ("favs", "Favoriler"),
            ("marathon", "Maraton"),
            ("history", "Geçmiş"),
            ("calendar", "Takvim"),
            ("news", "Haberler"),
            ("downloads", "İndirilenler"),
        ]
        .into_iter()
        .map(|(key, title)| {
            let row = adw::SwitchRow::new();
            row.set_title(title);
            row.set_subtitle("Sidebar'da göster");
            row.set_active(settings.sidebar_visible.iter().any(|v| v == key));
            sidebar_group.add(&row);
            (key, row)
        })
        .collect();
        page_keys.add(&sidebar_group);

        let view_group = adw::PreferencesGroup::new();
        view_group.set_title("Görünüm");

        let scale_row = adw::ComboRow::new();
        scale_row.set_title("Arayüz Ölçeği");
        scale_row.set_subtitle("Büyük monitörlerde arayüzü büyütür, anında uygulanır");
        let scales = &["%100 (Normal)", "%125", "%150"];
        let scale_model = gtk::StringList::new(scales);
        scale_row.set_model(Some(&scale_model));
        let current_scale = if (settings.ui_scale - 1.25).abs() < 0.01 {
            1
        } else if settings.ui_scale >= 1.4 {
            2
        } else {
            0
        };
        scale_row.set_selected(current_scale);
        view_group.add(&scale_row);

        let blur_row = adw::SwitchRow::new();
        blur_row.set_title("İzlenmeyen bölümleri bulanıklaştır");
        blur_row.set_subtitle("Bölüm ızgarasında henüz izlenmemiş bölümlerin kapak görsellerini spoiler olmasın diye bulanık gösterir");
        blur_row.set_active(settings.blur_unwatched);
        view_group.add(&blur_row);

        let gradient_row = adw::SwitchRow::new();
        gradient_row.set_title("Vurgu rengi gradyan arka plan");
        gradient_row.set_subtitle("Dizi detay sayfasının arka planını afişin baskın renklerinden oluşan yumuşak bir gradyanla boyar");
        gradient_row.set_active(settings.gradient_bg);
        view_group.add(&gradient_row);

        let frosted_row = adw::SwitchRow::new();
        frosted_row.set_title("Buzlu cam efekti");
        frosted_row.set_subtitle("Dizi detay sayfasındaki başlık kartını gradyanın üstünde yarı saydam cam gibi gösterir");
        frosted_row.set_active(settings.frosted_glass);
        view_group.add(&frosted_row);
        page_view.add(&view_group);

        let player_group = adw::PreferencesGroup::new();
        player_group.set_title("Oynatıcı Ayarları");

        let fs_row = adw::SwitchRow::new();
        fs_row.set_title("Otomatik Tam Ekran");
        fs_row.set_subtitle("Video başladığında oynatıcıyı otomatik tam ekran modunda açar");
        fs_row.set_active(settings.auto_fullscreen);
        player_group.add(&fs_row);

        let embed_row = adw::SwitchRow::new();
        embed_row.set_title("Gömülü Oynatıcı (GTK içinde)");
        embed_row.set_subtitle("Bölümler harici MPV penceresi yerine uygulamanın içindeki oynatıcıda açılır");
        embed_row.set_active(settings.embedded_player);
        player_group.add(&embed_row);

        let aniskip_row = adw::SwitchRow::new();
        aniskip_row.set_title("AniSkip Otomatik İntro Atlama Entegrasyonu");
        aniskip_row.set_subtitle("AniSkip API üzerinden 's' kısayol tuşu ile intro bitişine otomatik atlar");
        aniskip_row.set_active(settings.aniskip_enabled);
        player_group.add(&aniskip_row);

        let focus_row = adw::SwitchRow::new();
        focus_row.set_title("Focus Mod");
        focus_row.set_subtitle("İntro ve outro'ları otomatik atlar, bölüm bitince sonrakine geçer");
        focus_row.set_active(settings.focus_mode);
        player_group.add(&focus_row);

        let auto_next_row = adw::SwitchRow::new();
        auto_next_row.set_title("Otomatik Bölüm Atlama");
        auto_next_row.set_subtitle("Bölüm bitince bir sonraki bölüme otomatik geçer");
        auto_next_row.set_active(settings.auto_next_episode);
        player_group.add(&auto_next_row);

        let local_hist_row = adw::SwitchRow::new();
        local_hist_row.set_title("Yerel geçmişi kaydet");
        local_hist_row.set_subtitle("Kapalıysa bu cihazda izlenenler kaydedilmez; sadece sitedeki geçmiş gösterilir");
        local_hist_row.set_active(settings.local_history_enabled);
        player_group.add(&local_hist_row);
        page_player.add(&player_group);

        let perf_group = adw::PreferencesGroup::new();
        perf_group.set_title("Performans");

        let light_row = adw::SwitchRow::new();
        light_row.set_title("Hafif Mod (Düşük RAM)");
        light_row.set_subtitle("Arayüzü CPU ile çizer, bellek kullanımını ~%35 azaltır. Uygulamayı yeniden başlatınca geçerli olur.");
        light_row.set_active(settings.light_mode);
        perf_group.add(&light_row);

        let patience_row = adw::ActionRow::new();
        patience_row.set_title("Kaynak Açılış Sabrı");
        patience_row.set_subtitle("Yavaş internet için artırın. Medya hiç açılmazsa ölü kaynakta bu kadar saniye (20-120) beklenir, sonra sıradakine geçilir.");
        let patience_adj = gtk::Adjustment::new(settings.source_patience_secs as f64, 20.0, 120.0, 5.0, 10.0, 0.0);
        let patience_spin = gtk::SpinButton::new(Some(&patience_adj), 1.0, 0);
        patience_spin.set_numeric(true);
        patience_spin.set_value(settings.source_patience_secs as f64);
        patience_row.add_suffix(&patience_spin);
        perf_group.add(&patience_row);
        page_view.add(&perf_group);


        let img_group = adw::PreferencesGroup::new();
        img_group.set_title("Görüntü İyileştirme");
        let upscale_row = adw::ComboRow::new();
        upscale_row.set_title("Görüntü İyileştirme");
        upscale_row.set_subtitle("Düşük çözünürlüklü kaynağı yukarı ölçekler (1080p+ kaynaklarda sadece 'Keskinleştir' etkilidir)");
        let upscale_model = gtk::StringList::new(&[
            "Kapalı",
            "Keskinleştir",
            "Hafif",
            "Ultra",
            "Hafif + Keskinleştirme",
        ]);
        upscale_row.set_model(Some(&upscale_model));
        upscale_row.set_selected(match settings.upscale.as_str() {
            "hafif_keskin" => 4,
            "ultra" => 3,
            "hafif" => 2,
            "sharp" => 1,
            _ => 0,
        });
        img_group.add(&upscale_row);

        let upscale_desc = gtk::Label::new(Some(
            "Yalnızca kaynak çözünürlüğü ekrandan küçükse etki eder.\nHafif: DTD (iGPU dostu, hafif). Ultra: CNN (en kaliteli). Hafif + Keskinleştirme: DTD + keskinleştirme filtresi.",
        ));
        upscale_desc.set_wrap(true);
        upscale_desc.set_xalign(0.0);
        upscale_desc.set_margin_top(2);
        upscale_desc.set_margin_bottom(8);
        upscale_desc.set_margin_start(14);
        upscale_desc.set_selectable(false);
        upscale_desc.add_css_class("dim-label");
        img_group.add(&upscale_desc);
        page_player.add(&img_group);

        let fansub_group = adw::PreferencesGroup::new();
        fansub_group.set_title("Çeviri (Fansub) Seçimi");
        let ask_row = adw::SwitchRow::new();
        ask_row.set_title("Her bölümde sor");
        ask_row.set_subtitle("Kapalıysa otomatik olarak en yüksek puanlı çeviri seçilir");
        ask_row.set_active(settings.fansub_ask_each_time);
        fansub_group.add(&ask_row);
        let fansub_desc = gtk::Label::new(Some(
            "Bir bölüme tıkladığınızda mevcut çeviriler listelenir (örn. Kirigana, Wolwead). Puan yıldızı topluluk oylarına dayanır.",
        ));
        fansub_desc.set_wrap(true);
        fansub_desc.set_xalign(0.0);
        fansub_desc.set_margin_top(2);
        fansub_desc.set_margin_bottom(8);
        fansub_desc.set_margin_start(14);
        fansub_desc.set_selectable(false);
        fansub_desc.add_css_class("dim-label");
        fansub_group.add(&fansub_desc);
        page_player.add(&fansub_group);
        let on_save = Rc::new(on_save);
        let dl_group = adw::PreferencesGroup::new();
        dl_group.set_title("İndirme");
        let dl_dir_row = adw::ActionRow::new();
        dl_dir_row.set_title("İndirme Klasörü");
        let initial_dl = settings.download_dir.clone().unwrap_or_else(|| {
            crate::download::default_download_dir().to_string_lossy().into_owned()
        });
        dl_dir_row.set_subtitle(&initial_dl);
        let dl_pick = gtk::Button::with_label("Değiştir");
        dl_pick.add_css_class("flat");
        dl_pick.add_css_class("pill");
        dl_pick.set_valign(gtk::Align::Center);
        dl_dir_row.add_suffix(&dl_pick);
        dl_group.add(&dl_dir_row);
        {
            let s_o = settings.clone();
            let on_o = on_save.clone();
            let row_o = dl_dir_row.clone();
            dl_pick.connect_clicked(move |_| {
                let s_base_c = s_o.clone();
                let on_save_c = on_o.clone();
                let row_c = row_o.clone();
                let dialog = gtk::FileDialog::builder().title("İndirme Klasörü Seç").build();
                dialog.select_folder(
                    None::<&gtk::Window>,
                    None::<&gio::Cancellable>,
                    move |res| match res {
                        Ok(f) => {
                            if let Some(path) = f.path() {
                                let dir = path.to_string_lossy().into_owned();
                                let mut s = s_base_c.clone();
                                s.download_dir = Some(dir.clone());
                                row_c.set_subtitle(&dir);
                                on_save_c(s);
                            }
                        }
                        Err(e) => eprintln!("[DL] klasör seçilemedi: {e}"),
                    },
                );
            });
        }
        const CONN_VALUES: &[u32] = &[1, 2, 4, 6, 8, 12, 16];
        let conn_labels: Vec<String> =
            CONN_VALUES.iter().map(|n| format!("{n} bağlantı")).collect();
        let conn_refs: Vec<&str> = conn_labels.iter().map(String::as_str).collect();
        let conn_row = adw::ComboRow::new();
        conn_row.set_title("Bağlantı Sayısı");
        conn_row.set_subtitle("Her dosyayı kaç paralel bağlantıyla indirir. Sunucu tek bağlantıya izin veriyorsa düşürün");
        conn_row.set_model(Some(&gtk::StringList::new(&conn_refs)));
        let cur_conn = CONN_VALUES
            .iter()
            .position(|&v| v == settings.download_connections)
            .unwrap_or(2);
        conn_row.set_selected(cur_conn as u32);
        dl_group.add(&conn_row);
        page_data.add(&dl_group);
        let install_pref_group = adw::PreferencesGroup::new();
        install_pref_group.set_title("Sistem Kurulumu");
        let install_ui = crate::installer::build_installer_ui(window);
        install_pref_group.add(&install_ui);
        page_data.add(&install_pref_group);
        let update_group = adw::PreferencesGroup::new();
        let auto_update_row = adw::SwitchRow::new();
        auto_update_row.set_title("Otomatik Güncelleme");
        auto_update_row.set_subtitle("Başlatmada yeni sürümü kontrol eder ve AppImage'i kendisi günceller");
        auto_update_row.set_active(settings.auto_update);
        auto_update_row.set_sensitive(crate::update::is_appimage());
        update_group.add(&auto_update_row);

        let notify_row = adw::SwitchRow::new();
        notify_row.set_title("Güncel Sürüm Bildirimi");
        notify_row.set_subtitle("Başlatmada güncel sürümdeyken bilgilendirme göster");
        notify_row.set_active(settings.notify_uptodate);
        notify_row.set_sensitive(crate::update::is_appimage());
        update_group.add(&notify_row);

        let check_btn = gtk::Button::with_label("Şimdi Güncelle");
        check_btn.add_css_class("flat");
        check_btn.add_css_class("pill");
        check_btn.set_margin_top(4);
        check_btn.set_sensitive(crate::update::is_appimage());
        let check_btn_c = check_btn.clone();
        let settings_for_suppress = settings.clone();
        let on_save_suppress = on_save.clone();
        check_btn.connect_clicked(move |_| {
            if let Some(win) = check_btn_c.root().and_downcast::<gtk::Window>() {
                let cur = settings_for_suppress.clone();
                let suppress = on_save_suppress.clone();
                crate::update::check_and_prompt(&win, true, move || {
                    let mut sup = cur.clone();
                    sup.notify_uptodate = false;
                    suppress(sup);
                });
            }
        });
        update_group.add(&check_btn);
        page_data.add(&update_group);

        let shortcut_row_c = shortcut_row.clone();
        search_toggle_row.connect_active_notify(move |r| {
            shortcut_row_c.set_sensitive(r.is_active());
        });

        let s_base = settings.clone();

        let save_all = {
            let st_r = search_toggle_row.clone();
            let sc_r = shortcut_row.clone();
            let ssc_r = search_sc_row.clone();
            let scale_r = scale_row.clone();
            let bl_r = blur_row.clone();
            let grad_r = gradient_row.clone();
            let frost_r = frosted_row.clone();
            let fs_r = fs_row.clone();
            let emb_r = embed_row.clone();
            let ani_r = aniskip_row.clone();
            let foc_r = focus_row.clone();
            let anx_r = auto_next_row.clone();
            let au_r = auto_update_row.clone();
            let notify_r = notify_row.clone();
            let up_r = upscale_row.clone();
            let light_r = light_row.clone();
            let patience_spin_c = patience_spin.clone();
            let ask_r = ask_row.clone();
            let hist_r = local_hist_row.clone();
            let conn_r = conn_row.clone();
            let sidebar_rows_c = sidebar_rows.clone();
            let s = s_base.clone();
            let on_save = on_save.clone();
            Rc::new(move || {
                let mut updated = s.clone();
                updated.quick_search_enabled = st_r.is_active();
                updated.quick_search_shortcut = match sc_r.selected() {
                    1 => "Ctrl+F".into(),
                    2 => "F3".into(),
                    3 => "Ctrl+K".into(),
                    _ => "/".into(),
                };
                updated.search_shortcut = match ssc_r.selected() {
                    1 => "Ctrl+K".into(),
                    2 => "F2".into(),
                    3 => "/".into(),
                    _ => "Ctrl+S".into(),
                };
                updated.ui_scale = match scale_r.selected() {
                    1 => 1.25,
                    2 => 1.5,
                    _ => 1.0,
                };
                updated.blur_unwatched = bl_r.is_active();
                updated.gradient_bg = grad_r.is_active();
                updated.frosted_glass = frost_r.is_active();
                updated.auto_fullscreen = fs_r.is_active();
                updated.embedded_player = emb_r.is_active();
                updated.aniskip_enabled = ani_r.is_active();
                updated.focus_mode = foc_r.is_active();
                updated.auto_next_episode = anx_r.is_active();
                updated.auto_update = au_r.is_active();
                updated.notify_uptodate = notify_r.is_active();
                updated.upscale = match up_r.selected() {
                    1 => "sharp".into(),
                    2 => "hafif".into(),
                    3 => "ultra".into(),
                    4 => "hafif_keskin".into(),
                    _ => "off".into(),
                };
                updated.light_mode = light_r.is_active();
                updated.source_patience_secs = patience_spin_c.value() as u64;
                updated.fansub_ask_each_time = ask_r.is_active();
                updated.local_history_enabled = hist_r.is_active();
                let i = conn_r.selected() as usize;
                updated.download_connections = CONN_VALUES.get(i).copied().unwrap_or(6);
                updated.sidebar_visible = {
                    let mut visible = vec!["home".to_string()];
                    for (key, row) in &sidebar_rows_c {
                        if row.is_active() {
                            visible.push((*key).to_string());
                        }
                    }
                    visible
                };
                on_save(updated);
            })
        };

        let sa_ask = save_all.clone();
        ask_row.connect_active_notify(move |_| sa_ask());
        let sa_hist = save_all.clone();
        local_hist_row.connect_active_notify(move |_| sa_hist());

        let sa1 = save_all.clone();
        search_toggle_row.connect_active_notify(move |_| sa1());
        let sa2 = save_all.clone();
        shortcut_row.connect_selected_notify(move |_| sa2());
        let sa3 = save_all.clone();
        search_sc_row.connect_selected_notify(move |_| sa3());
        let sa_scale = save_all.clone();
        scale_row.connect_selected_notify(move |_| sa_scale());
        let sa4 = save_all.clone();
        fs_row.connect_active_notify(move |_| sa4());
        let sa_emb = save_all.clone();
        embed_row.connect_active_notify(move |_| sa_emb());
        let sa5 = save_all.clone();
        aniskip_row.connect_active_notify(move |_| sa5());
        let sa_foc = save_all.clone();
        focus_row.connect_active_notify(move |_| sa_foc());
        let sa_anx = save_all.clone();
        auto_next_row.connect_active_notify(move |_| sa_anx());
        let sa6 = save_all.clone();
        auto_update_row.connect_active_notify(move |_| sa6());
        let sa7 = save_all.clone();
        notify_row.connect_active_notify(move |_| sa7());
        let sa8 = save_all.clone();
        upscale_row.connect_selected_notify(move |_| sa8());
        let sa9 = save_all.clone();
        light_row.connect_active_notify(move |_| sa9());
        let sa_blur = save_all.clone();
        blur_row.connect_active_notify(move |_| sa_blur());
        let sa_grad = save_all.clone();
        gradient_row.connect_active_notify(move |_| sa_grad());
        let sa_frost = save_all.clone();
        frosted_row.connect_active_notify(move |_| sa_frost());
        let sa10 = save_all.clone();
        patience_spin.connect_value_changed(move |_| sa10());
        let sa_conn = save_all.clone();
        conn_row.connect_selected_notify(move |_| sa_conn());
        for (_, row) in &sidebar_rows {
            let sa = save_all.clone();
            row.connect_active_notify(move |_| sa());
        }

        let data_group = adw::PreferencesGroup::new();
        data_group.set_title("Veri Yönetimi");

        let uninstall_row = adw::SwitchRow::new();
        uninstall_row.set_title("Uygulamayı ve Başlatıcıyı da Sistemden Kaldır");
        uninstall_row.set_subtitle("Sıfırlama ile birlikte uygulama binary dosyasını ve masaüstü kısayollarını tamamen siler");
        uninstall_row.set_active(true);
        data_group.add(&uninstall_row);

        let wipe_btn = gtk::Button::with_label("Tüm Verileri Sıfırla ve Temizle");
        wipe_btn.add_css_class("destructive-action");
        wipe_btn.set_margin_top(8);
        let on_wipe = Rc::new(on_wipe);
        let un_c = uninstall_row.clone();

        wipe_btn.connect_clicked(move |btn| {
            let remove_app = un_c.is_active();
            let parent_win = btn.root().and_downcast::<gtk::Window>();

            let dialog = adw::MessageDialog::builder()
                .heading("Kalıcı Sıfırlama Onayı")
                .body(if remove_app {
                    "Tüm izleme geçmişiniz, ayarlarınız, kapak önbelleği ve UYGULAMA DOSYALARI sisteminizden kalıcı olarak silinecek. Emin misiniz?"
                } else {
                    "Tüm izleme geçmişiniz, ayarlarınız ve kapak önbelleği sıfırlanacak. Emin misiniz?"
                })
                .close_response("cancel")
                .default_response("cancel")
                .build();

            if let Some(win) = parent_win.as_ref() {
                dialog.set_transient_for(Some(win));
            }

            dialog.add_response("cancel", "İptal");
            dialog.add_response("wipe", "Evet, Kalıcı Olarak Sil");
            dialog.set_response_appearance("wipe", adw::ResponseAppearance::Destructive);

            let on_wipe_c = on_wipe.clone();
            dialog.connect_response(None, move |_, resp| {
                if resp == "wipe" {
                    on_wipe_c(remove_app);
                }
            });

            dialog.present();
        });
        data_group.add(&wipe_btn);
        page_data.add(&data_group);

        let info_group = adw::PreferencesGroup::new();
        info_group.set_title("Uygulama Bilgisi");

        let ver_row = adw::ActionRow::new();
        ver_row.set_title("Sürüm Numarası");
        ver_row.set_subtitle(&format!("AnimeciX Masaüstü İstemcisi  •  v{}", env!("CARGO_PKG_VERSION")));

        let ver_badge = gtk::Label::new(Some("Güncel"));
        ver_badge.add_css_class("status-badge-completed");
        ver_badge.set_valign(gtk::Align::Center);
        ver_row.add_suffix(&ver_badge);
        info_group.add(&ver_row);

        let reinstall_btn = gtk::Button::with_label("Masaüstü Başlatıcısını Sistemime Kur / Güncelle");
        reinstall_btn.add_css_class("flat");
        reinstall_btn.add_css_class("pill");
        reinstall_btn.set_margin_top(4);
        reinstall_btn.connect_clicked(|_| {
            let _ = crate::install_desktop_entry();
        });
        info_group.add(&reinstall_btn);

        page_data.add(&info_group);

        dialog.add(&page_view);
        dialog.add(&page_keys);
        dialog.add(&page_player);
        dialog.add(&page_data);

        dialog
    }
}
