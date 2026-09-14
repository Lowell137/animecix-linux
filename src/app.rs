use gtk::prelude::*;
use adw::prelude::*;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use gtk::gio;

use crate::api::{self, CalendarDay, Client, Credit, Episode, LastEpisode, NewsItem, Review, Title};
use crate::covers::CoverManager;

use crate::ui::components;
use crate::ui::episodes_view;
use crate::ui::info_views;
use crate::ui::views;
use crate::{check_all_dependencies, check_desktop_entry_installed, install_desktop_entry};

#[derive(Clone, Debug, PartialEq)]
pub enum Page {
    Welcome,
    Home,
    Kesfet,
    Favs,
    Marathon,
    History,
    Calendar,
    News,
    NewsDetail(NewsItem),
    Reviews { title_id: u64, title_name: String },
    Settings,
    Episodes { title: Title, eps: Vec<Episode> },
    Movie { title: Title, eps: Vec<Episode> },
    Player { title: Title, ep: Episode },
    Downloads,
    Search,
    Account,
}

#[derive(Clone)]
    enum Spot {
        Title { title: Title, resume_ep: Option<Episode> },
        News(NewsItem),
    }

/// Spot yüksekliği: pencere yüksekliğinin %52'si (20px kuantumlu),
/// sütundan gelen 320-420 tabanının altına inmez, 360-560 aralığında.
/// Pencere küçülünce alt içerik kaydırılabilir kalır.
fn hero_h_for_window(win_h: i32, cols: u32) -> i32 {
    let from_h = ((win_h as f32) * 0.52) as i32 / 20 * 20;
    let from_c = 260 + cols.max(3).min(8) as i32 * 20;
    from_h.max(from_c).clamp(360, 560)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SidebarId {
    Home,
    Kesfet,
    Favs,
    Marathon,
    History,
    Calendar,
    News,
    Login,
    Collapse,
}

#[derive(Clone)]
pub(crate) struct SideItem {
    pub(crate) id: SidebarId,
    pub(crate) btn: gtk::Button,
    pub(crate) label: gtk::Label,
    pub(crate) icon: gtk::Image,
    pub(crate) tip: &'static str,
    /// Geniş moddaki tam etiket (dock kısaltması için saklanır).
    pub(crate) full: String,
}

/// Dock (daraltılmış) alt yazıları: sığan kısa adlar.
fn dock_caption(id: SidebarId) -> Option<&'static str> {
    match id {
        SidebarId::Home => Some("Ana Sayfa"),
        SidebarId::Kesfet => Some("Keşfet"),
        SidebarId::Favs => Some("Favori"),
        SidebarId::Marathon => Some("Maraton"),
        SidebarId::History => Some("Geçmiş"),
        SidebarId::Calendar => Some("Takvim"),
        SidebarId::News => Some("Haber"),
        _ => None,
    }
}
pub enum Msg {
    Cats(Result<Vec<api::Category>, String>),
    Search(Result<Vec<Title>, String>),
    Eps(Title, Result<Vec<Episode>, String>, Vec<(u64, u64, f64)>),
    Play(Title, Episode, Result<(Vec<String>, Vec<String>, Vec<String>, Option<f64>), String>),
    Login(Result<(crate::auth::User, usize), String>),
    ServerHistory(Result<(Vec<api::ServerEntry>, usize), String>, bool),
    HistPage(u32, Result<(Vec<api::ServerEntry>, usize), String>),
    ServerMore(u32, Result<(Vec<api::ServerEntry>, usize), String>),
    NewsPage(u32, Result<(Vec<NewsItem>, usize, u32), String>),
    DisPage(u32, Result<(Vec<Title>, usize, u32), String>),
    NewsRail(Result<(Vec<NewsItem>, usize, u32), String>),
    Calendar(Result<Vec<CalendarDay>, String>),
    /// Detay zenginleştirme: benzerler + künye + incelemeler (sayfa 1).
    DetData(u64, Vec<Title>, Vec<Credit>, Vec<Review>, usize),
    RevPage(u64, u32, Result<(Vec<Review>, usize, u32), String>),
    LastRail(Result<(Vec<LastEpisode>, usize, u32), String>),
    FansubsLoaded {
        title: Title,
        ep: Episode,
        fansubs: Result<Vec<api::FansubInfo>, String>,
        default_template: Option<i64>,
        /// Oynatılan başlığın bölüm listesi (player sağ paneli için).
        eps: Vec<Episode>,
    },
    FansubChosen {
        title: Title,
        ep: Episode,
        chosen: Option<api::FansubInfo>,
    },
    EpsFetch {
        title: Title,
        ep: Episode,
    },
    /// Çevirmen listeleri geldi, flashcard/indirme başlat.
    DlLists {
        title: Title,
        quality: String,
        items: Vec<(Episode, Vec<api::FansubInfo>)>,
        is_single: bool,
    },
    /// Çözülen indirme kayıtları hazır, kuyruğa ekle.
    DlBatchResolved(Vec<crate::download::DownloadRecord>, Vec<String>, bool),
}

pub struct App {
    pub window: adw::ApplicationWindow,
    pub stack: gtk::Stack,
    pub back_btn: gtk::Button,
    pub refresh_btn: gtk::Button,
    pub title_label: gtk::Label,
    pub loading: gtk::Box,
    pub toast: adw::ToastOverlay,
    pub sidebar: gtk::Box,
    pub sidebar_revealer: gtk::Revealer,
    pub side_items: Rc<RefCell<Vec<SideItem>>>,
    pub side_collapse_btn: gtk::Button,
    /// Sidebar üst barı (arama + menü düğmeleri).
    pub side_search_btn: gtk::Button,
    pub side_menu_btn: gtk::Button,
    pub side_head: gtk::Box,
    /// Kapak baskın renk önbelleği (kart hover parıltısı için).
    pub pal_cache: Rc<RefCell<HashMap<String, (u8, u8, u8)>>>,
    pub client: Arc<Client>,
    pub covers: CoverManager,
    pub page_history: Rc<RefCell<Vec<Page>>>,
    pub cats: Rc<RefCell<Vec<api::Category>>>,
    pub search_results: Rc<RefCell<Vec<Title>>>,
    pub settings: Rc<RefCell<api::Settings>>,
    pub progress: Rc<RefCell<HashMap<String, (f64, f64)>>>,
    pub progress_bars: Rc<RefCell<HashMap<String, (gtk::ProgressBar, gtk::Label)>>>,
    /// Siteden önden çekilen konumlar (kalıcı değil; satır görünümü için).
    pub remote_progress: Rc<RefCell<HashMap<String, (f64, f64)>>>,
    pub loading_toast: Rc<RefCell<Option<adw::Toast>>>,
    pub opening_toast: Rc<RefCell<Option<adw::Toast>>>,
    pub opening_toast_shown_at: Rc<RefCell<Option<std::time::Instant>>>,
    pub loading_gen: Rc<Cell<u32>>,
    pub home_acts: Rc<RefCell<Vec<Option<usize>>>>,
    pub dl_manager: crate::download::DownloadManager,
    pub dl_rows: Rc<RefCell<HashMap<String, (gtk::ProgressBar, gtk::Label)>>>,
    pub player: Rc<RefCell<Option<crate::player_window::EmbeddedPlayer>>>,
    pub cur_eps_title: Rc<Cell<u64>>,
    pub cur_eps: Rc<RefCell<Vec<Episode>>>,
    pub server_history: Rc<RefCell<Vec<api::ServerEntry>>>,
    pub server_history_loaded: Rc<Cell<bool>>,
    pub header_bar: adw::HeaderBar,
    pub server_history_at: Rc<Cell<u64>>,
    pub hist_fetched: Rc<Cell<u32>>,
    pub hist_items: Rc<RefCell<Vec<api::ServerEntry>>>,
    pub hist_total: Rc<Cell<usize>>,
    pub hist_loading: Rc<Cell<bool>>,
    pub hist_error: Rc<RefCell<Option<String>>>,
    /// Keşfet kataloğu: filtreler + birikmiş kayıtlar + sayfalama.
    pub dis_genres: Rc<RefCell<Vec<String>>>,
    pub dis_keywords: Rc<RefCell<Vec<String>>>,
    pub dis_type: Rc<Cell<u32>>,
    pub dis_order: Rc<Cell<u32>>,
    pub dis_stream: Rc<Cell<bool>>,
    pub dis_items: Rc<RefCell<Vec<Title>>>,
    pub dis_total: Rc<Cell<usize>>,
    pub dis_fetched: Rc<Cell<u32>>,
    pub dis_loading: Rc<Cell<bool>>,
    pub dis_error: Rc<RefCell<Option<String>>>,
    /// Ana sayfa devam bölümünün o anki sayfası (0-indexli, sayfada 10).
    pub cont_page: Rc<Cell<u32>>,
    /// Sunucu havuzu: kaç sayfa yüklü + toplam kayıt + "daha fazla" kilidi.
    pub server_pool_pages: Rc<Cell<u32>>,
    pub server_total: Rc<Cell<usize>>,
    pub server_more_loading: Rc<Cell<bool>>,
    /// Haberler sayfası (1-indexli) + içeriği.
    pub news_items: Rc<RefCell<Vec<NewsItem>>>,
    pub news_page: Rc<Cell<u32>>,
    pub news_total: Rc<Cell<usize>>,
    pub news_last: Rc<Cell<u32>>,
    pub news_loading: Rc<Cell<bool>>,
    pub news_error: Rc<RefCell<Option<String>>>,
    /// Ana sayfadaki gündem şeridi (haberler sayfa 1 önbelleği).
    pub news_rail: Rc<RefCell<Vec<NewsItem>>>,
    /// Yayın takvimi + seçili gün + yükleme durumu.
    pub cal_days: Rc<RefCell<Vec<CalendarDay>>>,
    pub cal_sel: Rc<Cell<usize>>,
    pub cal_loading: Rc<Cell<bool>>,
    pub cal_error: Rc<RefCell<Option<String>>>,
    /// Detay sayfası zenginleştirme (başlık id + benzerler + künye +
    /// incelemeler önizleme + toplam inceleme).
    pub det_title: Rc<Cell<u64>>,
    pub det_related: Rc<RefCell<Vec<Title>>>,
    pub det_credits: Rc<RefCell<Vec<Credit>>>,
    pub det_reviews: Rc<RefCell<Vec<Review>>>,
    pub det_rev_total: Rc<Cell<usize>>,
    /// İncelemeler sayfası (1-indexli) + içeriği.
    pub rev_title: Rc<Cell<u64>>,
    pub rev_name: Rc<RefCell<String>>,
    pub rev_items: Rc<RefCell<Vec<Review>>>,
    pub rev_page: Rc<Cell<u32>>,
    pub rev_total: Rc<Cell<usize>>,
    pub rev_last: Rc<Cell<u32>>,
    pub rev_loading: Rc<Cell<bool>>,
    pub rev_error: Rc<RefCell<Option<String>>>,
    /// Ana sayfadaki son eklenen bölümler şeridi (sayfa 1 önbelleği).
    pub last_rail: Rc<RefCell<Vec<LastEpisode>>>,
    /// Son eklenenler ızgarasının o anki sayfası (0-indexli, sayfada 10).
    pub last_page: Rc<Cell<u32>>,
    /// Izgara sütun sayısı (pencere genişliğine göre 3-8).
    pub grid_cols: Rc<Cell<u32>>,
    /// Spot yüksekliği (pencere yüksekliğinden, 20px kuantumlu).
    pub spot_h: Rc<Cell<i32>>,
    /// Bekleyen kaydırma konumu (asenkron sayfa büyütmede korunur, -1 = yok).
    pub saved_scroll: Rc<Cell<f64>>,
    /// Kategori ızgaralarının sayfaları (kategori indexi → 0-indexli sayfa).
    pub cat_pages: Rc<RefCell<HashMap<usize, u32>>>,
}

pub(crate) fn resolve_upscale_shader(name: &str) -> Option<String> {
    use std::sync::OnceLock;
    use std::sync::Mutex;

    // Embedded shader içeriği (binary'ye gömülü, AppImage extract'ten bağımsız).
    fn embedded(name: &str) -> Option<&'static str> {
        match name {
            "Anime4K_Upscale_CNN_x2_M.glsl" => Some(include_str!("../assets/upscale/Anime4K_Upscale_CNN_x2_M.glsl")),
            "Anime4K_Upscale_CNN_x2_UL.glsl" => Some(include_str!("../assets/upscale/Anime4K_Upscale_CNN_x2_UL.glsl")),
            "Anime4K_Upscale_DTD_x2.glsl" => Some(include_str!("../assets/upscale/Anime4K_Upscale_DTD_x2.glsl")),
            "Anime4K_Upscale_Original_x2.glsl" => Some(include_str!("../assets/upscale/Anime4K_Upscale_Original_x2.glsl")),
            _ => None,
        }
    }

    // Her isim için sadece bir kez temp'e yaz.
    static CACHE: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(p) = cache.lock().unwrap().get(name).cloned() {
        if std::path::Path::new(&p).exists() {
            return Some(p);
        }
    }

    // Gömülü içeriği temp'e yaz.
    if let Some(src) = embedded(name) {
        let dir = std::env::temp_dir().join("animecix-upscale");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(name);
        if std::fs::write(&path, src).is_ok() {
            let p = path.to_string_lossy().into_owned();
            cache.lock().unwrap().insert(name.to_string(), p.clone());
            return Some(p);
        }
    }

    // Fallback: disk üzerinde ara (dev/Flatpak/sistem kurulumları için).
    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(ad) = std::env::var("APPDIR") {
        if !ad.is_empty() {
            candidates.push(std::path::Path::new(&ad).join("usr/share/animecix/assets/upscale").join(name));
        }
    }
    if std::path::Path::new("/app").exists() {
        candidates.push(std::path::PathBuf::from("/app/share/animecix/assets/upscale").join(name));
    }
    candidates.push(std::path::PathBuf::from("/usr/share/animecix/assets/upscale").join(name));
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            candidates.push(parent.join("usr/share/animecix/assets/upscale").join(name));
            candidates.push(parent.join("assets/upscale").join(name));
            candidates.push(parent.join("../../assets/upscale").join(name));
        }
    }
    candidates.into_iter().find(|p| p.exists()).map(|p| p.to_string_lossy().into_owned())
}

fn aniskip_input_conf(t: &api::AniSkipTimes) -> String {
    let fmt_sec = |sec: f64| -> String {
        let s = sec as u64;
        format!("{:02}:{:02}", s / 60, s % 60)
    };
    let skip_cmd = if let (Some(st), Some(et)) = (t.op_start, t.op_end) {
        format!("s seek {et:.1} absolute; show-text \"⏩ İntro Atlandı (AniSkip: {} → {})\" 3000\n", fmt_sec(st), fmt_sec(et))
    } else {
        "s show-text \"⚠️ İntro zamanı bulunamadı (AniSkip)\" 2500\n".to_string()
    };
    let outro_cmd = if let (Some(st), Some(et)) = (t.ed_start, t.ed_end) {
        format!("e seek {et:.1} absolute; show-text \"⏩ Outro Atlandı (AniSkip: {} → {})\" 3000\n", fmt_sec(st), fmt_sec(et))
    } else {
        "e show-text \"⚠️ Outro zamanı bulunamadı (AniSkip)\" 2500\n".to_string()
    };
    format!("{skip_cmd}{outro_cmd}S seek -30; show-text \"⏪ 30s Geri\" 2000\nEnd ignore\n")
}

impl App {
    pub fn new(app: &adw::Application) -> Rc<Self> {
        let client = Arc::new(Client::new());

        {
            let cl = client.clone();
            std::thread::spawn(move || {
                cl.warmup();
                let _ = cl.home_lists();
            });
        }
        let welcome_seen = client.is_welcome_seen();

        let header = adw::HeaderBar::new();
        let title_label = gtk::Label::new(Some("AnimeciX"));
        title_label.add_css_class("title-2");
        title_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        title_label.set_max_width_chars(28);
        header.set_title_widget(Some(&title_label));

        let back_btn = gtk::Button::from_icon_name("go-previous-symbolic");
        back_btn.add_css_class("flat");
        back_btn.add_css_class("circular");
        back_btn.set_tooltip_text(Some("Geri"));
        header.pack_start(&back_btn);

        let refresh_btn = gtk::Button::from_icon_name("view-refresh-symbolic");
        refresh_btn.add_css_class("flat");
        refresh_btn.add_css_class("circular");
        refresh_btn.set_tooltip_text(Some("Yenile"));
        header.pack_start(&refresh_btn);

        // ---- sol yan menü (navigasyon) ----
        let sidebar = gtk::Box::new(gtk::Orientation::Vertical, 4);
        sidebar.set_halign(gtk::Align::Start);
        sidebar.set_hexpand(false);
        sidebar.set_margin_top(8);
        sidebar.set_margin_bottom(8);
        sidebar.set_margin_start(8);
        sidebar.set_margin_end(4);
        sidebar.set_valign(gtk::Align::Fill);
        sidebar.set_vexpand(true);

        fn side_row(
            sidebar: &gtk::Box,
            items: &Rc<RefCell<Vec<SideItem>>>,
            id: SidebarId,
            label_text: &'static str,
            icon: &'static str,
            tip: &'static str,
        ) -> gtk::Button {
            let btn = gtk::Button::new();
            btn.add_css_class("flat");
            btn.add_css_class("side-row");
            btn.set_tooltip_text(Some(tip));
            btn.set_halign(gtk::Align::Fill);
            let inner = gtk::Box::new(gtk::Orientation::Horizontal, 10);
            inner.set_margin_top(6);
            inner.set_margin_bottom(6);
            inner.set_margin_start(10);
            inner.set_margin_end(10);
            let img = gtk::Image::from_icon_name(icon);
            img.set_valign(gtk::Align::Center);
            let lbl = gtk::Label::new(Some(label_text));
            lbl.set_xalign(0.0);
            lbl.set_hexpand(true);
            inner.append(&img);
            inner.append(&lbl);
            btn.set_child(Some(&inner));
            sidebar.append(&btn);
            items.borrow_mut().push(SideItem { id, btn: btn.clone(), label: lbl, icon: img, tip, full: label_text.to_string() });
            btn
        }

        let side_items: Rc<RefCell<Vec<SideItem>>> = Rc::new(RefCell::new(Vec::new()));

        // ---- üst bar: arama (solda) + menü (sağda) ----
        let side_head = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        side_head.set_margin_bottom(4);
        let side_search_btn = gtk::Button::from_icon_name("animecix-search-symbolic");
        side_search_btn.add_css_class("flat");
        side_search_btn.add_css_class("circular");
        side_search_btn.add_css_class("side-head-btn");
        side_search_btn.set_tooltip_text(Some("Ara"));
        let side_menu_btn = gtk::Button::from_icon_name("open-menu-symbolic");
        side_menu_btn.add_css_class("flat");
        side_menu_btn.add_css_class("circular");
        side_menu_btn.add_css_class("side-head-btn");
        side_menu_btn.set_tooltip_text(Some("Menü"));
        let side_head_spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        side_head_spacer.set_hexpand(true);
        side_head.append(&side_search_btn);
        side_head.append(&side_head_spacer);
        side_head.append(&side_menu_btn);
        sidebar.append(&side_head);

        let home_btn = side_row(&sidebar, &side_items, SidebarId::Home, "Ana Sayfa", "go-home-symbolic", "Ana Sayfa");
        let kesfet_btn = side_row(&sidebar, &side_items, SidebarId::Kesfet, "Keşfet", "view-grid-symbolic", "Keşfet");
        let fav_btn = side_row(&sidebar, &side_items, SidebarId::Favs, "Favoriler", "starred-symbolic", "Favoriler");
        let marathon_btn = side_row(&sidebar, &side_items, SidebarId::Marathon, "Maraton", "media-playlist-consecutive-symbolic", "İzleme Maratonu");
        let hist_btn = side_row(&sidebar, &side_items, SidebarId::History, "Geçmiş", "document-open-recent-symbolic", "İzleme Geçmişi");
        let cal_btn = side_row(&sidebar, &side_items, SidebarId::Calendar, "Takvim", "x-office-calendar-symbolic", "Yayın Takvimi");
        let news_btn = side_row(&sidebar, &side_items, SidebarId::News, "Haberler", "animecix-news-symbolic", "Anime Haberleri");

        let side_spacer = gtk::Box::new(gtk::Orientation::Vertical, 0);
        side_spacer.set_vexpand(true);
        side_spacer.set_hexpand(false);
        sidebar.append(&side_spacer);

        // Daraltma satırı: diğer satırlarla aynı yapıda (hizalama otomatik),
        // Ayarlar'ın hemen üstünde. İkon + yazı duruma göre güncellenir.
        let side_collapse_btn = side_row(&sidebar, &side_items, SidebarId::Collapse, "Daralt", "animecix-sidebar-symbolic", "Yan menüyü daralt/genişlet");

        // Ayarlar hamburger menüde (üst bar); sidebar'da satırı yok.

        // En alt: giriş satırı (avatar + kullanıcı adı / "Giriş Yap").
        let login_btn = side_row(&sidebar, &side_items, SidebarId::Login, "Giriş Yap", "avatar-default-symbolic", "Hesap");

        let _ = (home_btn, kesfet_btn, fav_btn, marathon_btn, hist_btn, cal_btn, news_btn, login_btn);

        let main_stack = gtk::Stack::new();
        main_stack.set_transition_type(gtk::StackTransitionType::SlideLeftRight);
        main_stack.set_transition_duration(220);
        main_stack.set_vexpand(true);
        main_stack.set_hexpand(true);

        let loading = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        loading.add_css_class("card");
        loading.set_halign(gtk::Align::Center);
        loading.set_valign(gtk::Align::Start);
        loading.set_margin_top(12);
        loading.set_margin_bottom(12);
        loading.set_margin_start(16);
        loading.set_margin_end(16);

        let spin = gtk::Spinner::new();
        spin.start();
        let l_lbl = gtk::Label::new(Some("Yükleniyor…"));
        l_lbl.add_css_class("title-4");
        loading.append(&spin);
        loading.append(&l_lbl);
        loading.set_visible(false);

        let overlay = gtk::Overlay::new();
        overlay.set_child(Some(&main_stack));
        overlay.add_overlay(&loading);
        overlay.set_vexpand(true);
        overlay.set_hexpand(true);

        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.append(&header);
        content.append(&overlay);
        content.set_hexpand(true);

        let body = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        // Sidebar kayarak açılıp kapanır (animasyonlu).
        let sidebar_revealer = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::SlideRight)
            .transition_duration(220)
            .child(&sidebar)
            .build();
        sidebar_revealer.set_reveal_child(true);
        body.append(&sidebar_revealer);
        let side_sep = gtk::Separator::new(gtk::Orientation::Vertical);
        body.append(&side_sep);
        body.append(&content);

        let toast = adw::ToastOverlay::new();
        toast.set_child(Some(&body));

        // Pencere boyutu sabit: ölçülen 1479x845 (açılış + minimum).
        // Maksimum sınırlanmaz (büyütme/tam ekran serbest).
        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title("AnimeciX")
            .default_width(1479)
            .default_height(845)
            .content(&toast)
            .build();
        window.set_size_request(1479, 845);

        let initial_page = if welcome_seen { Page::Home } else { Page::Welcome };

        let covers = CoverManager::new(client.clone());

        let app_inst = Rc::new(Self {
            window,
            stack: main_stack,
            back_btn,
            refresh_btn,
            title_label,
            loading,
            toast,
            sidebar,
            sidebar_revealer,
            side_items: side_items.clone(),
            side_collapse_btn,
            side_search_btn,
            side_menu_btn,
            side_head,
            pal_cache: Rc::new(RefCell::new(HashMap::new())),
            client: client.clone(),
            covers,
            page_history: Rc::new(RefCell::new(vec![initial_page.clone()])),
            cats: Rc::new(RefCell::new(Vec::new())),
            search_results: Rc::new(RefCell::new(Vec::new())),
            settings: Rc::new(RefCell::new(client.load_settings())),
            progress: Rc::new(RefCell::new(client.load_state().progress)),
            progress_bars: Rc::new(RefCell::new(HashMap::new())),
            remote_progress: Rc::new(RefCell::new(HashMap::new())),
            loading_toast: Rc::new(RefCell::new(None)),
            opening_toast: Rc::new(RefCell::new(None)),
            opening_toast_shown_at: Rc::new(RefCell::new(None)),
            loading_gen: Rc::new(Cell::new(0)),
            home_acts: Rc::new(RefCell::new(Vec::new())),
            dl_manager: {
                let queue_path = crate::download::queue_file_path();
                let (dl_tx, dl_rx) = std::sync::mpsc::channel::<crate::download::UiEvent>();
                let mgr = crate::download::DownloadManager::new(queue_path, dl_tx);
                let dl_rx = std::sync::Arc::new(std::sync::Mutex::new(dl_rx));
                let mgr_clone = mgr.clone();
                glib::timeout_add_local(std::time::Duration::from_millis(500), move || {
                    let mut dirty = false;
                    if let Ok(rx) = dl_rx.lock() {
                        while let Ok(ev) = rx.try_recv() {
                            match ev {
                                crate::download::UiEvent::Tick => dirty = true,
                                crate::download::UiEvent::Changed => {
                                    // Yapısal değişiklik - sayfa yenilenecek
                                }
                                crate::download::UiEvent::Toast(m) => {
                                    eprintln!("[DL] {}", m);
                                }
                            }
                            dirty = true;
                        }
                    }
                    glib::ControlFlow::Continue
                });
                mgr
            },
            dl_rows: Rc::new(RefCell::new(HashMap::new())),
            player: Rc::new(RefCell::new(None)),
            cur_eps_title: Rc::new(Cell::new(0)),
            cur_eps: Rc::new(RefCell::new(Vec::new())),
            server_history: Rc::new(RefCell::new(Vec::new())),
            server_history_loaded: Rc::new(Cell::new(false)),
            header_bar: header,
            server_history_at: Rc::new(Cell::new(0)),
            hist_fetched: Rc::new(Cell::new(0)),
            hist_items: Rc::new(RefCell::new(Vec::new())),
            hist_total: Rc::new(Cell::new(0)),
            hist_loading: Rc::new(Cell::new(false)),
            hist_error: Rc::new(RefCell::new(None)),
            dis_genres: Rc::new(RefCell::new(Vec::new())),
            dis_keywords: Rc::new(RefCell::new(Vec::new())),
            dis_type: Rc::new(Cell::new(0)),
            dis_order: Rc::new(Cell::new(0)),
            dis_stream: Rc::new(Cell::new(true)),
            dis_items: Rc::new(RefCell::new(Vec::new())),
            dis_total: Rc::new(Cell::new(0)),
            dis_fetched: Rc::new(Cell::new(0)),
            dis_loading: Rc::new(Cell::new(false)),
            dis_error: Rc::new(RefCell::new(None)),
            cont_page: Rc::new(Cell::new(0)),
            server_pool_pages: Rc::new(Cell::new(0)),
            server_total: Rc::new(Cell::new(0)),
            server_more_loading: Rc::new(Cell::new(false)),
            news_items: Rc::new(RefCell::new(Vec::new())),
            news_page: Rc::new(Cell::new(0)),
            news_total: Rc::new(Cell::new(0)),
            news_last: Rc::new(Cell::new(1)),
            news_loading: Rc::new(Cell::new(false)),
            news_error: Rc::new(RefCell::new(None)),
            news_rail: Rc::new(RefCell::new(Vec::new())),
            cal_days: Rc::new(RefCell::new(Vec::new())),
            cal_sel: Rc::new(Cell::new(0)),
            cal_loading: Rc::new(Cell::new(false)),
            cal_error: Rc::new(RefCell::new(None)),
            det_title: Rc::new(Cell::new(0)),
            det_related: Rc::new(RefCell::new(Vec::new())),
            det_credits: Rc::new(RefCell::new(Vec::new())),
            det_reviews: Rc::new(RefCell::new(Vec::new())),
            det_rev_total: Rc::new(Cell::new(0)),
            rev_title: Rc::new(Cell::new(0)),
            rev_name: Rc::new(RefCell::new(String::new())),
            rev_items: Rc::new(RefCell::new(Vec::new())),
            rev_page: Rc::new(Cell::new(0)),
            rev_total: Rc::new(Cell::new(0)),
            rev_last: Rc::new(Cell::new(1)),
            rev_loading: Rc::new(Cell::new(false)),
            rev_error: Rc::new(RefCell::new(None)),
            last_rail: Rc::new(RefCell::new(Vec::new())),
            last_page: Rc::new(Cell::new(0)),
            grid_cols: Rc::new(Cell::new(5)),
            spot_h: Rc::new(Cell::new(hero_h_for_window(845, 5))),
            saved_scroll: Rc::new(Cell::new(-1.0)),
            cat_pages: Rc::new(RefCell::new(HashMap::new())),
        });
        {
            // Aicix init deferred to Aşama 2
        }

        app_inst.chain_signals();
        app_inst.apply_ui_scale();
        app_inst.apply_sidebar();
        // Açılışta girişliyse sunucu favorilerini sessizce birleştir.
        {
            let c = app_inst.client.clone();
            std::thread::spawn(move || {
                let n = c.merge_server_favs();
                if n > 0 {
                    eprintln!("[AUTH] açılışta {n} favori birleştirildi");
                }
            });
        }
        app_inst.show_page(&initial_page);
        if welcome_seen {
            app_inst.fetch_home();
        }
        app_inst.apply_goto_arg();
        app_inst
    }

    pub fn clone_ref(&self) -> Rc<Self> {
        Rc::new(Self {
            window: self.window.clone(),
            stack: self.stack.clone(),
            back_btn: self.back_btn.clone(),
            refresh_btn: self.refresh_btn.clone(),
            title_label: self.title_label.clone(),
            loading: self.loading.clone(),
            toast: self.toast.clone(),
            sidebar: self.sidebar.clone(),
            sidebar_revealer: self.sidebar_revealer.clone(),
            side_items: self.side_items.clone(),
            side_collapse_btn: self.side_collapse_btn.clone(),
            side_search_btn: self.side_search_btn.clone(),
            side_menu_btn: self.side_menu_btn.clone(),
            side_head: self.side_head.clone(),
            pal_cache: self.pal_cache.clone(),
            client: self.client.clone(),
            covers: self.covers.clone_ref(),
            page_history: self.page_history.clone(),
            cats: self.cats.clone(),
            search_results: self.search_results.clone(),
            settings: self.settings.clone(),
            progress: self.progress.clone(),
            progress_bars: self.progress_bars.clone(),
            remote_progress: self.remote_progress.clone(),
            loading_toast: self.loading_toast.clone(),
            opening_toast: self.opening_toast.clone(),
            opening_toast_shown_at: self.opening_toast_shown_at.clone(),
            loading_gen: self.loading_gen.clone(),
            home_acts: self.home_acts.clone(),
            dl_manager: self.dl_manager.clone(),
            dl_rows: self.dl_rows.clone(),
            player: self.player.clone(),
            header_bar: self.header_bar.clone(),
            cur_eps_title: self.cur_eps_title.clone(),
            cur_eps: self.cur_eps.clone(),
            server_history: self.server_history.clone(),
            server_history_loaded: self.server_history_loaded.clone(),
            server_history_at: self.server_history_at.clone(),
            hist_fetched: self.hist_fetched.clone(),
            hist_items: self.hist_items.clone(),
            hist_total: self.hist_total.clone(),
            hist_loading: self.hist_loading.clone(),
            dis_genres: self.dis_genres.clone(),
            dis_keywords: self.dis_keywords.clone(),
            dis_type: self.dis_type.clone(),
            dis_order: self.dis_order.clone(),
            dis_stream: self.dis_stream.clone(),
            dis_items: self.dis_items.clone(),
            dis_total: self.dis_total.clone(),
            dis_fetched: self.dis_fetched.clone(),
            dis_loading: self.dis_loading.clone(),
            dis_error: self.dis_error.clone(),
            hist_error: self.hist_error.clone(),
            cont_page: self.cont_page.clone(),
            server_pool_pages: self.server_pool_pages.clone(),
            server_total: self.server_total.clone(),
            server_more_loading: self.server_more_loading.clone(),
            news_items: self.news_items.clone(),
            news_page: self.news_page.clone(),
            news_total: self.news_total.clone(),
            news_last: self.news_last.clone(),
            news_loading: self.news_loading.clone(),
            news_error: self.news_error.clone(),
            news_rail: self.news_rail.clone(),
            cal_days: self.cal_days.clone(),
            cal_sel: self.cal_sel.clone(),
            cal_loading: self.cal_loading.clone(),
            cal_error: self.cal_error.clone(),
            det_title: self.det_title.clone(),
            det_related: self.det_related.clone(),
            det_credits: self.det_credits.clone(),
            det_reviews: self.det_reviews.clone(),
            det_rev_total: self.det_rev_total.clone(),
            rev_title: self.rev_title.clone(),
            rev_name: self.rev_name.clone(),
            rev_items: self.rev_items.clone(),
            rev_page: self.rev_page.clone(),
            rev_total: self.rev_total.clone(),
            rev_last: self.rev_last.clone(),
            rev_loading: self.rev_loading.clone(),
            rev_error: self.rev_error.clone(),
            last_rail: self.last_rail.clone(),
            last_page: self.last_page.clone(),
            grid_cols: self.grid_cols.clone(),
            spot_h: self.spot_h.clone(),
            saved_scroll: self.saved_scroll.clone(),
            cat_pages: self.cat_pages.clone(),
        })
    }

    fn chain_signals(&self) {
        // Yan menü buton bulucu.
        fn find_btn(items: &Rc<RefCell<Vec<SideItem>>>, id: SidebarId) -> Option<gtk::Button> {
            items.borrow().iter().find(|it| it.id == id).map(|it| it.btn.clone())
        }

        // Arama satırı: arama modunu açar.
        // Üst bar arama butonu: popup arama açar.
        {
            let this = self.clone_ref();
            self.side_search_btn.connect_clicked(move |_| {
                this.open_search_popup();
            });
        }

        // Hamburger menü: Ayarlar + Hakkında (doğrudan butonlu popover;
        // action'lı PopoverMenu bazı temalarda tıklamayı yutuyordu).
        {
            let this = self.clone_ref();
            let pop = gtk::Popover::new();
            let pbox = gtk::Box::new(gtk::Orientation::Vertical, 2);
            pbox.set_margin_top(6);
            pbox.set_margin_bottom(6);
            pbox.set_margin_start(6);
            pbox.set_margin_end(6);
            let mk_item = |text: &str| {
                let b = gtk::Button::with_label(text);
                b.add_css_class("flat");
                b.set_halign(gtk::Align::Fill);
                if let Some(lbl) = b.child().and_downcast::<gtk::Label>() {
                    lbl.set_xalign(0.0);
                }
                b
            };
            let b_set = mk_item("Ayarlar");
            let b_about = mk_item("AnimeciX Hakkında");
            pbox.append(&b_set);
            pbox.append(&b_about);
            pop.set_child(Some(&pbox));
            pop.set_parent(&this.side_menu_btn);
            let pop_c = pop.clone();
            let t_set = this.clone_ref();
            b_set.connect_clicked(move |_| {
                pop_c.popdown();
                let mut st = t_set.page_history.borrow_mut();
                if st.last() != Some(&Page::Settings) {
                    st.push(Page::Settings);
                }
                drop(st);
                t_set.show_page(&Page::Settings);
            });
            let pop_c2 = pop.clone();
            let t_about = this.clone_ref();
            b_about.connect_clicked(move |_| {
                pop_c2.popdown();
                t_about.show_about();
            });
            this.side_menu_btn.connect_clicked(move |_| {
                pop.popup();
            });
        }

        // Sayfa satırları: Ana Sayfa / Favoriler / Maraton / Geçmiş / Takvim / Haberler / Hesap.
        // (Ayarlar hamburger menüden açılır; vurgusu yok.)
        for (id, page) in [
            (SidebarId::Home, Page::Home),
            (SidebarId::Kesfet, Page::Kesfet),
            (SidebarId::Favs, Page::Favs),
            (SidebarId::Marathon, Page::Marathon),
            (SidebarId::History, Page::History),
            (SidebarId::Calendar, Page::Calendar),
            (SidebarId::News, Page::News),
            (SidebarId::Login, Page::Account),
        ] {
            if let Some(b) = find_btn(&self.side_items, id) {
                let this = self.clone_ref();
                b.connect_clicked(move |_| {
                    let mut st = this.page_history.borrow_mut();
                    if st.last() != Some(&page) {
                        st.push(page.clone());
                    }
                    drop(st);
                    this.show_page(&page);
                    if id == SidebarId::Home {
                        this.fetch_home();
                    }
                    this.auto_refresh(&page);
                });
            }
        }

        // Daralt/genişlet.
        {
            let this = self.clone_ref();
            self.side_collapse_btn.connect_clicked(move |_| {
                let v = !this.settings.borrow().sidebar_collapsed;
                {
                    let mut s = this.settings.borrow_mut();
                    s.sidebar_collapsed = v;
                    this.client.save_settings(&s);
                }
                this.apply_sidebar();
            });
        }

        // Profil (hesap sayfası) artık sidebar giriş satırından açılır.

        let this = self.clone_ref();
        self.back_btn.connect_clicked(move |_| {
            this.go_back();
        });

        let this = self.clone_ref();
        self.refresh_btn.connect_clicked(move |_| {
            this.refresh_current();
        });

        // Pencere boyuna göre spot boyu (debounced; ızgaralı
        // sayfalar yeniden kurulur, diğerleri etkilenmez).
        // Izgara sütunu sabit 5'tir (5 üst + 5 alt sayfalı düzen).
        // Yüzeyin `layout` sinyali gerçek boyutu verir (`default-width`
        // bildirimi kullanıcı yeniden boyutlandırmasında ateşlenmez).
        // Not: SourceId saklayıp remove() çağırmak yarışta patlıyor
        // (ateşlenmiş kaynağı silmek abort eder); nesil sayacı kullanılır.
        {
            let this = self.clone_ref();
            let gen = Rc::new(Cell::new(0u32));
            let request = Rc::new(move |_win_w: i32, win_h: i32| {
                let spot = hero_h_for_window(win_h, 5);
                if spot == this.spot_h.get() {
                    return;
                }
                let my = gen.get() + 1;
                gen.set(my);
                let this2 = this.clone_ref();
                let gen2 = gen.clone();
                glib::timeout_add_local_once(
                    std::time::Duration::from_millis(300),
                    move || {
                        if my != gen2.get() {
                            return;
                        }
                        if spot == this2.spot_h.get() {
                            return;
                        }
                        let old_h = this2.spot_h.get();
                        this2.spot_h.set(spot);
                        eprintln!("[LAYOUT] spot {old_h} -> {spot}");
                        let top = this2
                            .page_history
                            .borrow()
                            .last()
                            .cloned()
                            .unwrap_or(Page::Home);
                        if matches!(
                            top,
                            Page::Home | Page::Search | Page::History
                        ) {
                            this2.show_page(&top);
                        }
                    },
                );
            });
            // Yüzey her yerleştiğinde gerçek boyutu kullan.
            let req_lay = request.clone();
            self.window.connect_map(move |win| {
                if let Some(surface) = win.native().and_then(|n| n.surface()) {
                    let req = req_lay.clone();
                    let _ = surface.connect_layout(move |_, w, h| {
                        req(w, h);
                    });
                }
                // İlk açılışta gerçek boyuta göre senkronize et.
                let a = win.allocation();
                if a.width() > 0 && a.height() > 0 {
                    req_lay(a.width(), a.height());
                }
            });
        }

        // Fare yan tuşu (geri): oynatıcı dışında sayfalar arası geri gider.
        // (Oynatıcı sayfasında yan tuşlar ±10sn aramaya aittir.)
        {
            let this = self.clone_ref();
            let nav_click = gtk::GestureClick::new();
            nav_click.set_button(8);
            nav_click.connect_pressed(move |_, _, _, _| {
                let is_player = matches!(
                    this.page_history.borrow().last(),
                    Some(Page::Player { .. })
                );
                if !is_player {
                    this.go_back();
                }
            });
            self.window.add_controller(nav_click);
        }

        // Genel arama kısayolu (Ctrl+S / / …): popup aramayı açar.
        {
            let this = self.clone_ref();
            let settings = self.settings.clone();
            let key_ctrl = gtk::EventControllerKey::new();
            key_ctrl.connect_key_pressed(move |_, keyval, _, state| {
                let sc = settings.borrow().search_shortcut.clone();
                let key_name = keyval.name().map(|s| s.to_string()).unwrap_or_default();
                let is_ctrl = state.contains(gtk::gdk::ModifierType::CONTROL_MASK);
                let triggered = match sc.as_str() {
                    "Ctrl+K" => is_ctrl && (key_name == "k" || key_name == "K"),
                    "F2" => key_name == "F2",
                    "/" => key_name == "slash" || key_name == "kp_divide",
                    _ => is_ctrl && (key_name == "s" || key_name == "S"), // Ctrl+S
                };
                if triggered {
                    this.open_search_popup();
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            });
            self.window.add_controller(key_ctrl);
        }
    }

    /// Yan menü genişliği + etiket görünürlüğü (daraltma ayarına göre).
    /// Daraltılmış mod ikon dock'tur: 60px genişlik, 17px ortalı ikon,
    /// altında kısa alt yazı. Üst bar hep yatay: arama solda, menü sağda.
    fn apply_sidebar(&self) {
        let collapsed = self.settings.borrow().sidebar_collapsed;
        self.sidebar.set_size_request(if collapsed { 60 } else { 164 }, -1);
        self.sidebar.set_margin_start(if collapsed { 4 } else { 8 });
        self.sidebar.set_margin_end(if collapsed { 4 } else { 4 });
        // Üst bar daima yatay (dock'ta mini butonlar).
        self.side_head.set_orientation(gtk::Orientation::Horizontal);
        self.side_head.set_halign(gtk::Align::Fill);
        for btn in [&self.side_search_btn, &self.side_menu_btn] {
            if collapsed {
                if !btn.has_css_class("dock-mini") {
                    btn.add_css_class("dock-mini");
                }
            } else {
                btn.remove_css_class("dock-mini");
            }
            if let Some(img) = btn.child().and_downcast::<gtk::Image>() {
                img.set_pixel_size(if collapsed { 14 } else { -1 });
            }
        }
        let uname = self
            .client
            .session_user()
            .map(|u| u.name)
            .filter(|n| !n.is_empty());
        for it in self.side_items.borrow().iter() {
            let inner = it.btn.child().and_downcast::<gtk::Box>();
            if collapsed {
                it.icon.set_pixel_size(17);
                it.icon.set_halign(gtk::Align::Center);
                it.label.set_visible(true);
                if !it.label.has_css_class("side-caption") {
                    it.label.add_css_class("side-caption");
                }
                it.label.set_xalign(0.5);
                it.label.set_lines(1);
                it.label.set_ellipsize(gtk::pango::EllipsizeMode::End);
                it.label.set_text(&match it.id {
                    SidebarId::Login => {
                        uname.clone().unwrap_or_else(|| "Giriş Yap".to_string())
                    }
                    SidebarId::Collapse => "Genişlet".to_string(),
                    _ => dock_caption(it.id)
                        .map(str::to_string)
                        .unwrap_or_else(|| it.full.clone()),
                });
                if let Some(inner) = inner {
                    inner.set_orientation(gtk::Orientation::Vertical);
                    // Yazı ikondan biraz aşağıda dursun.
                    inner.set_spacing(6);
                    inner.set_halign(gtk::Align::Center);
                    inner.set_margin_start(2);
                    inner.set_margin_end(2);
                }
            } else {
                it.icon.set_pixel_size(-1);
                it.icon.set_halign(gtk::Align::Fill);
                it.label.set_visible(true);
                it.label.remove_css_class("side-caption");
                it.label.set_xalign(0.0);
                it.label.set_lines(-1);
                it.label.set_ellipsize(gtk::pango::EllipsizeMode::None);
                it.label.set_text(&match it.id {
                    SidebarId::Login => {
                        uname.clone().unwrap_or_else(|| "Giriş Yap".to_string())
                    }
                    SidebarId::Collapse => {
                        if collapsed { "Genişlet" } else { "Daralt" }.to_string()
                    }
                    _ => it.full.clone(),
                });
                if let Some(inner) = inner {
                    inner.set_orientation(gtk::Orientation::Horizontal);
                    inner.set_spacing(10);
                    inner.set_halign(gtk::Align::Fill);
                    inner.set_margin_start(10);
                    inner.set_margin_end(10);
                }
            }
            if it.id == SidebarId::Collapse {
                it.icon.set_icon_name(Some(if collapsed {
                    "animecix-sidebar-symbolic"
                } else {
                    "animecix-sidebar-collapse-symbolic"
                }));
            }
        }
    }

    /// O anki sayfaya göre yan menü seçim vurgusu.
    fn update_sidebar_selection(&self, page: &Page) {
        let sel = match page {
            Page::Home | Page::Episodes { .. } | Page::Movie { .. } => Some(SidebarId::Home),
            Page::Favs => Some(SidebarId::Favs),
            Page::Marathon => Some(SidebarId::Marathon),
            Page::History => Some(SidebarId::History),
            Page::Kesfet => Some(SidebarId::Kesfet),
            Page::Calendar => Some(SidebarId::Calendar),
            Page::News | Page::NewsDetail(_) => Some(SidebarId::News),
            Page::Reviews { .. } => Some(SidebarId::Home),
            Page::Account => Some(SidebarId::Login),
            _ => None,
        };
        // Giriş satırı etiketi her navigasyonda tazelenir.
        let uname = self
            .client
            .session_user()
            .map(|u| u.name)
            .filter(|n| !n.is_empty());
        for it in self.side_items.borrow().iter() {
            if it.id == SidebarId::Login {
                it.label.set_text(uname.as_deref().unwrap_or("Giriş Yap"));
            }
            if Some(it.id) == sel {
                it.btn.add_css_class("side-selected");
            } else {
                it.btn.remove_css_class("side-selected");
            }
        }
    }

    /// Kart hover parıltısı: kapağın baskın renginde glow + CSS lift
    /// (.title-btn:hover .poster-lift zaten posteri yukarı taşır). Palet ilk hover'da arka
    /// planda çıkarılıp önbelleğe alınır; sonrakiler anında uygulanır.
    /// Not: GTK CSS'te backdrop-blur yok; geniş-yumuşak gölge ışıma
    /// etkisi verir.
    /// Kart hover: sadece posteri kaldır (lift), glow kaldırıldı.
    fn bind_card_hover(&self, _card: &gtk::Box, _poster_url: Option<&str>) {
        // Glow efekti tamamen kaldırıldı; lift .poster-lift.lifted ile koddan yönetiliyor.
    }

    fn glow_provider((r, g, b): (u8, u8, u8)) -> gtk::CssProvider {
        let css = gtk::CssProvider::new();
        css.load_from_string(&format!(
            ".card-glow {{ box-shadow: 0 10px 24px 2px rgba({r},{g},{b},0.38); }}"
        ));
        css
    }

    /// Palet yoksa varsayılan vurgu renginde parıltı (hover her zaman çalışır).
    fn glow_fallback() -> (u8, u8, u8) {
        (122, 162, 247)
    }

    pub fn go_back(&self) {
        let mut st = self.page_history.borrow_mut();
        if st.len() > 1 {
            st.pop();
            while st.len() > 1 && st.last() == st.get(st.len() - 2) {
                st.pop();
            }
        }
        let top = st.last().cloned().unwrap_or(Page::Home);
        drop(st);
        self.show_page(&top);
        self.auto_refresh(&top);
    }

    /// O anki sayfanın verisini tazele (yenile butonu).
    /// Kapatıp açmaya gerek kalmadan güncel geçmiş/liste gelir.
    pub fn refresh_current(&self) {
        let top = self
            .page_history
            .borrow()
            .last()
            .cloned()
            .unwrap_or(Page::Home);
        match &top {
            Page::History => {
                // Tam sıfırla + ilk sayfadan başla (silinenler de düşer).
                if !self.hist_loading.get() {
                    *self.hist_items.borrow_mut() = Vec::new();
                    self.hist_total.set(0);
                    self.hist_fetched.set(0);
                    *self.hist_error.borrow_mut() = None;
                    self.fetch_hist_page(0);
                }
            }
            Page::Home => {
                self.fetch_home();
                self.server_history_at.set(0);
                self.fetch_server_history(true);
            }
            Page::News => {
                if !self.news_loading.get() {
                    *self.news_items.borrow_mut() = Vec::new();
                    self.news_page.set(0);
                    self.news_total.set(0);
                    *self.news_error.borrow_mut() = None;
                    self.fetch_news_page(1);
                }
            }
            Page::Calendar => {
                if !self.cal_loading.get() {
                    self.fetch_calendar();
                }
            }
            Page::Reviews { title_id, .. } => {
                if !self.rev_loading.get() {
                    self.fetch_reviews_page(*title_id, 1);
                }
            }
            Page::Episodes { .. } | Page::Movie { .. } | Page::Favs | Page::Marathon => {
                self.show_page(&top);
            }
            _ => {}
        }
    }

    /// Sayfa geçişleri sonrası anlık tazeleme: Ge geçmişi + ana sayfa
    /// her ziyarette güncellenir (yeniden başlatmaya gerek kalmaz).
    fn auto_refresh(&self, page: &Page) {
        match page {
            Page::History => {
                // Üst sayfayı birleştir (yeni izlenenler üste düşer,
                // birikmiş sayfalar korunur).
                if self.client.is_logged_in() && !self.hist_loading.get() {
                    self.fetch_hist_page(0);
                }
            }
            Page::Home => {
                self.server_history_at.set(0);
                self.fetch_server_history(false);
            }
            Page::Kesfet => {
                if self.dis_items.borrow().is_empty() && !self.dis_loading.get() {
                    self.dis_refetch();
                }
            }
            _ => {}
        }
    }

    pub fn busy(&self, on: bool) {
        let gen = self.loading_gen.get() + 1;
        self.loading_gen.set(gen);
        self.loading.set_visible(false);

        if on {
            if let Some(t) = self.loading_toast.borrow_mut().take() {
                t.dismiss();
            }
            let t = adw::Toast::new("Yükleniyor…");
            t.set_timeout(0);
            self.toast.add_toast(t.clone());
            *self.loading_toast.borrow_mut() = Some(t);
        } else if let Some(t) = self.loading_toast.borrow_mut().take() {
            t.dismiss();
        }
    }

    pub fn refresh_internet_status(&self) {
        // Ana sayfayı yeniden inşa eder; build_home_view yeniden kontrol eder
        let stack = self.stack.clone();
        let this = self.clone_ref();
        glib::timeout_add_local_once(
            std::time::Duration::from_millis(50),
            move || {
                let widget = this.build_home_view();
                if let Some(prev) = stack.child_by_name("home") {
                    stack.remove(&prev);
                }
                stack.add_named(&widget, Some("home"));
                stack.set_visible_child_name("home");
            },
        );
    }

    fn apply_ui_scale(&self) {
        let s = self.settings.borrow().ui_scale;
        self.window.remove_css_class("ui-scale-125");
        self.window.remove_css_class("ui-scale-150");
        if (s - 1.25).abs() < 0.01 {
            self.window.add_css_class("ui-scale-125");
        } else if s >= 1.4 {
            self.window.add_css_class("ui-scale-150");
        }
    }

    fn apply_movie_tint(&self, target: &gtk::Box, poster: Option<&str>) {
        let Some(url) = poster.map(|s| s.to_string()) else { return };
        let client = self.client.clone();
        let (tx, rx) = std::sync::mpsc::channel::<Option<[(u8, u8, u8); 3]>>();
        std::thread::spawn(move || {
            let _ = tx.send(client.cover_palette(&url));
        });
        let weak = target.downgrade();
        glib::idle_add_local(move || match rx.try_recv() {
            Ok(pal) => {
                let Some(root) = weak.upgrade() else { return glib::ControlFlow::Break };
                let [c1, c2, c3] =
                    pal.unwrap_or([(122, 162, 247), (55, 70, 110), (140, 110, 190)]);
                let (r1, g1, b1) = c1;
                let (r2, g2, b2) = c2;
                let (r3, g3, b3) = c3;
                let css_a = format!(
                    "#movie-tint-root {{ background-color: rgba({r2},{g2},{b2},0.35); \
                     background: radial-gradient(ellipse at 50% 0%, \
                     rgba({r1},{g1},{b1},0.32), rgba(0,0,0,0) 70%), \
                     linear-gradient(135deg, rgba({r1},{g1},{b1},0.30), \
                     rgba({r2},{g2},{b2},0.20) 55%, rgba({r3},{g3},{b3},0.30)); }}"
                );
                let css_b = format!(
                    "#movie-tint-root {{ background-color: rgba({r2},{g2},{b2},0.35); \
                     background: radial-gradient(ellipse at 50% 100%, \
                     rgba({r3},{g3},{b3},0.30), rgba(0,0,0,0) 70%), \
                     linear-gradient(315deg, rgba({r3},{g3},{b3},0.30), \
                     rgba({r1},{g1},{b1},0.20) 55%, rgba({r2},{g2},{b2},0.30)); }}"
                );
                let prov_a = gtk::CssProvider::new();
                prov_a.load_from_string(&css_a);
                let prov_b = gtk::CssProvider::new();
                prov_b.load_from_string(&css_b);
                root.set_widget_name("movie-tint-root");
                let display = root.display();
                gtk::style_context_add_provider_for_display(
                    &display,
                    &prov_a,
                    gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
                );
                let weak2 = root.downgrade();
                let disp2 = display.clone();
                let (pa_c, pb_c) = (prov_a.clone(), prov_b.clone());
                let showing_a = Rc::new(Cell::new(true));
                glib::timeout_add_local(std::time::Duration::from_secs(5), move || {
                    if weak2.upgrade().is_none() {
                        gtk::style_context_remove_provider_for_display(&disp2, &pa_c);
                        gtk::style_context_remove_provider_for_display(&disp2, &pb_c);
                        return glib::ControlFlow::Break;
                    }
                    if showing_a.get() {
                        gtk::style_context_remove_provider_for_display(&display, &prov_a);
                        gtk::style_context_add_provider_for_display(
                            &display,
                            &prov_b,
                            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
                        );
                    } else {
                        gtk::style_context_remove_provider_for_display(&display, &prov_b);
                        gtk::style_context_add_provider_for_display(
                            &display,
                            &prov_a,
                            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
                        );
                    }
                    showing_a.set(!showing_a.get());
                    glib::ControlFlow::Continue
                });
                glib::ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(_) => glib::ControlFlow::Break,
        });
    }

    pub fn show_page(&self, page: &Page) {
        use gtk::prelude::IsA;
        // Oynatıcı sayfası dışındaki HER sayfaya geçişte gömülü oynatıcıyı
        // durdur (konum kaydedilir). Not: geri tuşu history'den düşürüp sonra
        // buraya geldiği için history'ye bakmak yetmez — player açıksa kapat.
        if !matches!(page, Page::Player { .. }) {
            if let Some(p) = self.player.borrow().as_ref() {
                p.shutdown();
            }
            *self.player.borrow_mut() = None;
        }
        // Native header sadece oynatıcı DIŞINDA görünür; izlerken üstte
        // videonun üstünde yüzen bar vardır (geri + başlık + pencere düğmeleri).
        // Yan menü de izlerken gizlidir (hover şeridiyle açılır).
        let on_player = matches!(page, Page::Player { .. });
        self.header_bar.set_visible(!on_player);
        if on_player {
            self.sidebar_revealer.set_reveal_child(false);
        } else {
            self.sidebar_revealer.set_reveal_child(!matches!(page, Page::Welcome));
        }
        // Yan menü seçim vurgusu.
        self.update_sidebar_selection(page);
        self.progress_bars.borrow_mut().clear();
        self.back_btn.set_sensitive(self.page_history.borrow().len() > 1);

        fn switch<T: IsA<gtk::Widget>>(
            stack: &gtk::Stack,
            name: &str,
            transition: gtk::StackTransitionType,
            widget: T,
        ) {
            if let Some(old) = stack.child_by_name(name) {
                stack.remove(&old);
            }
            stack.set_transition_type(transition);
            stack.add_named(&widget, Some(name));
            stack.set_visible_child_name(name);

            let stack_c = stack.clone();
            glib::timeout_add_local_once(
                std::time::Duration::from_millis(400),
                move || {
                    let Some(visible) = stack_c.visible_child() else { return; };
                    let mut to_rm = vec![];
                    let mut cur = stack_c.first_child();
                    while let Some(child) = cur {
                        let next = child.next_sibling();
                        if child != visible {
                            to_rm.push(child);
                        }
                        cur = next;
                    }
                    for c in to_rm {
                        stack_c.remove(&c);
                    }
                },
            );
        }

        match page {
            Page::Welcome => {
                self.title_label.set_text("Hoş Geldiniz");
                switch(&self.stack, "welcome", gtk::StackTransitionType::Crossfade, self.build_welcome_view());
            }
            Page::Home => {
                self.title_label.set_text("AnimeciX");
                switch(&self.stack, "home", gtk::StackTransitionType::Crossfade, self.build_home_view());
            }
            Page::Favs => {
                self.title_label.set_text("Favorilerim");
                switch(&self.stack, "favs", gtk::StackTransitionType::Crossfade, self.build_favs_view());
            }
            Page::Marathon => {
                self.title_label.set_text("İzleme Maratonum 🏃‍♂️");
                switch(&self.stack, "marathon", gtk::StackTransitionType::Crossfade, self.build_marathon_view());
            }
            Page::History => {
                self.title_label.set_text("İzleme Geçmişi");
                switch(&self.stack, "history", gtk::StackTransitionType::Crossfade, self.build_history_view());
            }
            Page::Calendar => {
                self.title_label.set_text("Yayın Takvimi");
                if self.cal_days.borrow().is_empty() && !self.cal_loading.get() {
                    self.fetch_calendar();
                }
                switch(&self.stack, "calendar", gtk::StackTransitionType::Crossfade, self.build_calendar_view());
            }
            Page::News => {
                self.title_label.set_text("Haberler");
                if self.news_page.get() == 0 && !self.news_loading.get() {
                    self.fetch_news_page(1);
                }
                switch(&self.stack, "news", gtk::StackTransitionType::Crossfade, self.build_news_view());
            }
            Page::Kesfet => {
                self.title_label.set_text("Keşfet");
                if self.dis_items.borrow().is_empty()
                    && !self.dis_loading.get()
                    && self.dis_error.borrow().is_none()
                {
                    self.fetch_dis_page(1);
                }
                switch(&self.stack, "kesfet", gtk::StackTransitionType::Crossfade, self.build_kesfet_view());
            }
            Page::NewsDetail(item) => {
                self.title_label.set_text(&item.title);
                switch(&self.stack, "news_detail", gtk::StackTransitionType::SlideLeft, self.build_news_detail_view(item));
            }
            Page::Reviews { title_id, title_name } => {
                self.title_label.set_text(&format!("İncelemeler: {title_name}"));
                if self.rev_title.get() != *title_id || (self.rev_page.get() == 0 && !self.rev_loading.get()) {
                    self.rev_title.set(*title_id);
                    *self.rev_name.borrow_mut() = title_name.clone();
                    *self.rev_items.borrow_mut() = Vec::new();
                    self.rev_page.set(0);
                    self.rev_total.set(0);
                    self.fetch_reviews_page(*title_id, 1);
                }
                switch(&self.stack, "reviews", gtk::StackTransitionType::SlideLeft, self.build_reviews_view());
            }
            Page::Settings => {
                self.title_label.set_text("Ayarlar");
                switch(&self.stack, "settings", gtk::StackTransitionType::Crossfade, self.build_settings_view());
            }
            Page::Search => {
                self.title_label.set_text("Arama Sonuçları");
                switch(&self.stack, "search", gtk::StackTransitionType::SlideLeft, self.build_search_view());
            }
            Page::Episodes { title, eps } | Page::Movie { title, eps } => {
                self.title_label.set_text(&title.name);
                let page_name = format!("eps_{}", title.id);
                switch(&self.stack, &page_name, gtk::StackTransitionType::SlideLeft, self.build_episodes_view(title, eps));
            }
            Page::Player { title, ep } => {
                self.title_label.set_text(&format!("{} | S{:02}E{:02}", title.name, ep.season, ep.episode));
                switch(&self.stack, "player", gtk::StackTransitionType::SlideLeft, self.build_player_view());
            }
            Page::Account => {
                self.title_label.set_text("Hesap");
                // Takip sayısı güncel gelsin (5dk korumalı, döngü yapmaz).
                self.fetch_server_history(false);
                switch(&self.stack, "account", gtk::StackTransitionType::Crossfade, self.build_account_view());
            }
            Page::Downloads => {
                self.title_label.set_text("İndirilenler");
                switch(&self.stack, "downloads", gtk::StackTransitionType::Crossfade, self.build_downloads_view());
            }
        }
    }

    fn apply_goto_arg(&self) {
        let args: Vec<String> = std::env::args().collect();
        let mut it = args.iter();
        while let Some(a) = it.next() {
            if a == "--goto" {
                if let Some(val) = it.next() {
                    self.goto_page(val);
                }
            }
        }
    }

    fn goto_page(&self, val: &str) {
        self.page_history.borrow_mut().clear();
        match val {
            "welcome" => {
                self.page_history.borrow_mut().push(Page::Welcome);
                self.show_page(&Page::Welcome);
            }
            "home" => {
                self.page_history.borrow_mut().push(Page::Home);
                self.show_page(&Page::Home);
                self.fetch_home();
            }
            "favorites" => {
                self.page_history.borrow_mut().push(Page::Favs);
                self.show_page(&Page::Favs);
            }
            "marathon" => {
                self.page_history.borrow_mut().push(Page::Marathon);
                self.show_page(&Page::Marathon);
            }
            "history" => {
                self.page_history.borrow_mut().push(Page::History);
                self.show_page(&Page::History);
            }
            "calendar" => {
                self.page_history.borrow_mut().push(Page::Calendar);
                self.show_page(&Page::Calendar);
            }
            "news" => {
                self.page_history.borrow_mut().push(Page::News);
                self.show_page(&Page::News);
            }
            "settings" => {
                self.page_history.borrow_mut().push(Page::Settings);
                self.show_page(&Page::Settings);
            }
            "account" => {
                self.page_history.borrow_mut().push(Page::Account);
                self.show_page(&Page::Account);
            }
            "kesfet" => {
                self.page_history.borrow_mut().push(Page::Kesfet);
                self.show_page(&Page::Kesfet);
            }
            "search" => {
                self.page_history.borrow_mut().push(Page::Home);
                self.show_page(&Page::Home);
                self.fetch_home();
                self.open_search_popup();
            }
            "episodes" => {
                self.page_history.borrow_mut().push(Page::Home);
                self.show_page(&Page::Home);
                self.fetch_home();
                let this = self.clone_ref();
                glib::timeout_add_local_once(std::time::Duration::from_millis(1800), move || {
                    let cats = this.cats.borrow();
                    let first = cats.iter().flat_map(|c| c.items.iter()).next().cloned();
                    drop(cats);
                    if let Some(t) = first {
                        this.open_episodes(t);
                    }
                });
            }
            _ => {}
        }
    }

    fn build_welcome_view(&self) -> gtk::ScrolledWindow {
        let scroll = gtk::ScrolledWindow::new();
        let root = gtk::Box::new(gtk::Orientation::Vertical, 16);
        root.set_margin_top(20);
        root.set_margin_bottom(24);
        root.set_margin_start(24);
        root.set_margin_end(24);

        let header_box = gtk::Box::new(gtk::Orientation::Vertical, 8);
        header_box.set_halign(gtk::Align::Center);

        let icon = gtk::Image::from_icon_name("tr.com.animecix");
        icon.set_pixel_size(84);
        header_box.append(&icon);

        let title = gtk::Label::new(Some("AnimeciX Masaüstü Kurulum & Kontrol Sihirbazı"));
        title.add_css_class("title-1");
        header_box.append(&title);

        let desc = gtk::Label::new(Some(
            "Uygulamayı kullanmaya başlamadan önce sistem bağımlılıklarını ve masaüstü entegrasyonunu kontrol edin."
        ));
        desc.add_css_class("dim-label");
        desc.set_wrap(true);
        desc.set_justify(gtk::Justification::Center);
        header_box.append(&desc);

        root.append(&header_box);

        let dep_group = adw::PreferencesGroup::new();
        dep_group.set_title("1. Sistem Bağımlılık Kontrolü");
        dep_group.set_description(Some("Uygulamanın sorunsuz çalışabilmesi için gerekli sistem araçları:"));

        let deps = check_all_dependencies();
        for dep in &deps {
            let row = adw::ActionRow::new();
            row.set_title(dep.name);
            row.set_subtitle(dep.desc);

            let status_badge = gtk::Label::new(None);
            status_badge.set_valign(gtk::Align::Center);
            if dep.installed {
                status_badge.set_markup("<span foreground='#2ec27e' weight='bold'>🟢 Yüklü</span>");
            } else {
                status_badge.set_markup("<span foreground='#e01b24' weight='bold'>🔴 Eksik</span>");
                if let Some(cmd) = &dep.install_cmd {
                    row.set_subtitle(&format!("{} • Kurulum: {}", dep.desc, cmd));
                }
            }
            row.add_suffix(&status_badge);
            dep_group.add(&row);
        }

        root.append(&dep_group);

        let desktop_group = adw::PreferencesGroup::new();
        desktop_group.set_title("2. Masaüstü Uygulama Menüsü Entegrasyonu");
        desktop_group.set_description(Some("AnimeciX'i işletim sisteminizin uygulama başlatıcı menüsüne ekleyin (AppImage ~/.local/bin konumuna kopyalanır):"));

        let desktop_row = adw::ActionRow::new();
        desktop_row.set_title("Masaüstü Menü Başlatıcısı (tr.com.animecix.desktop)");

        let is_installed = check_desktop_entry_installed();
        let desktop_status_lbl = gtk::Label::new(None);
        desktop_status_lbl.set_valign(gtk::Align::Center);

        let desktop_btn = gtk::Button::new();
        desktop_btn.set_valign(gtk::Align::Center);
        desktop_btn.add_css_class("pill");

        if is_installed {
            desktop_status_lbl.set_markup("<span foreground='#2ec27e' weight='bold'>🟢 Menüde Ekli</span>");
            desktop_row.set_subtitle("AnimeciX uygulama menünüzde hazır.");
            desktop_btn.set_label("Yeniden Entegre Et 📌");
            desktop_btn.add_css_class("flat");
        } else {
            desktop_status_lbl.set_markup("<span foreground='#f5c211' weight='bold'>🟡 Menüde Yok</span>");
            desktop_row.set_subtitle("Uygulama menüsüne eklemek için butona tıklayın.");
            desktop_btn.set_label("Uygulamalar Listesine Ekle 📌");
            desktop_btn.add_css_class("suggested-action");
        }

        let this_desk = self.clone_ref();
        let lbl_clone = desktop_status_lbl.clone();
        let row_clone = desktop_row.clone();
        let btn_clone = desktop_btn.clone();

        desktop_btn.connect_clicked(move |_| {
            match install_desktop_entry() {
                Ok(_) => {
                    lbl_clone.set_markup("<span foreground='#2ec27e' weight='bold'>🟢 Başarıyla Eklendi</span>");
                    row_clone.set_subtitle("AnimeciX masaüstü uygulama menüsüne eklendi!");
                    btn_clone.set_label("Yeniden Entegre Et 📌");
                    btn_clone.remove_css_class("suggested-action");
                    btn_clone.add_css_class("flat");

                    let toast = adw::Toast::new("📌 AnimeciX masaüstü uygulama menüsüne eklendi!");
                    toast.set_timeout(4);
                    this_desk.toast.add_toast(toast);
                }
                Err(e) => {
                    let toast = adw::Toast::new(&format!("⚠️ Masaüstü menüsüne eklenemedi: {e}"));
                    this_desk.toast.add_toast(toast);
                }
            }
        });

        desktop_row.add_suffix(&desktop_status_lbl);
        desktop_row.add_suffix(&desktop_btn);
        desktop_group.add(&desktop_row);
        root.append(&desktop_group);

        let player_group = adw::PreferencesGroup::new();
        player_group.set_title("3. Hızlı Başlangıç Tercihleri");

        let fs_row = adw::SwitchRow::new();
        fs_row.set_title("Otomatik Tam Ekran");
        fs_row.set_subtitle("Video başladığında oynatıcıyı otomatik tam ekran modunda açar");
        fs_row.set_active(self.settings.borrow().auto_fullscreen);

        let aniskip_row = adw::SwitchRow::new();
        aniskip_row.set_title("AniSkip Otomatik İntro Atlama Entegrasyonu");
        aniskip_row.set_subtitle("AniSkip API üzerinden 's' kısayol tuşu ile intro bitişine otomatik atlar");
        aniskip_row.set_active(self.settings.borrow().aniskip_enabled);

        player_group.add(&fs_row);
        player_group.add(&aniskip_row);
        root.append(&player_group);

        let start_btn = gtk::Button::with_label("Kurulumu Tamamla ve Başlat 🚀");
        start_btn.add_css_class("suggested-action");
        start_btn.add_css_class("pill");
        start_btn.add_css_class("title-3");
        start_btn.set_halign(gtk::Align::Center);
        start_btn.set_margin_top(12);

        let this = self.clone_ref();
        let fs_r = fs_row.clone();
        let ani_r = aniskip_row.clone();

        start_btn.connect_clicked(move |_| {
            let mut s = this.settings.borrow().clone();
            s.auto_fullscreen = fs_r.is_active();
            s.aniskip_enabled = ani_r.is_active();
            this.client.save_settings(&s);
            *this.settings.borrow_mut() = s;

            this.client.set_welcome_seen(true);
            this.page_history.borrow_mut().clear();
            this.page_history.borrow_mut().push(Page::Home);
            this.show_page(&Page::Home);
            this.fetch_home();
        });
        root.append(&start_btn);

        scroll.set_child(Some(&root));
        scroll
    }

    /// Devam listesi: yerel + sunucu kayıtları gerçek zamana göre
    /// birleştirilir (en yeni önce). Aynı yapımda yenisi kazanır.
    /// Dönen: (başlık, kaldığı bölüm [yoksa None]).
    fn continue_items(&self) -> Vec<(Title, Option<Episode>)> {
        // (zaman_ms, id, başlık, bölüm)
        let mut scored: Vec<(u64, u64, Title, Option<Episode>)> = Vec::new();
        if self.settings.borrow().local_history_enabled {
            for h in &self.client.load_state().history {
                scored.push((h.ts.saturating_mul(1000), h.title.id, h.title.clone(), Some(h.episode.clone())));
            }
        }
        for s in self.server_history.borrow().iter() {
            let ep = if s.season > 0 && s.episode > 0 {
                Some(Episode {
                    season: s.season,
                    episode: s.episode,
                    name: format!("Bölüm {}", s.episode),
                    thumbnail: None,
                })
            } else {
                None
            };
            scored.push((s.date, s.title.id, s.title.clone(), ep));
        }
        scored.sort_by(|a, b| b.0.cmp(&a.0));
        let mut seen = std::collections::HashSet::new();
        let mut out: Vec<(Title, Option<Episode>)> = Vec::new();
        for (_, id, t, e) in scored {
            if seen.insert(id) {
                out.push((t, e));
            }
        }
        out
    }

    /// Başlığın kaldığı sezonu: önce yerel geçmiş, sonra sunucu kaydı.
    fn resume_season(&self, title_id: u64) -> Option<u64> {
        if self.settings.borrow().local_history_enabled {
            if let Some(h) = self
                .client
                .load_state()
                .history
                .iter()
                .find(|h| h.title.id == title_id)
            {
                if h.episode.season > 0 {
                    return Some(h.episode.season);
                }
            }
        }
        self.server_history
            .borrow()
            .iter()
            .find(|s| s.title.id == title_id)
            .filter(|s| s.season > 0)
            .map(|s| s.season)
    }

    /// Son eklenen bölümler ızgarası: 5 sütun, sayfada 10 kart, ‹ ›.
    fn build_last_section(&self, items: &[LastEpisode]) -> gtk::Box {
        const PER: usize = 10;
        let pages = ((items.len() + PER - 1) / PER).max(1);
        let mut page = self.last_page.get();
        page = page.min(pages as u32 - 1);
        let root = gtk::Box::new(gtk::Orientation::Vertical, 4);
        root.set_hexpand(true);
        root.set_halign(gtk::Align::Fill);
        let this = self.clone_ref();
        let cols = self.grid_cols.get();
        root.append(&Self::pager_head(
            "🆕 SON EKLENEN BÖLÜMLER",
            page,
            pages as u32,
            Self::grid_width(cols),
            move |p| {
                this.last_page.set(p);
                this.show_page_keep_scroll(&Page::Home);
            },
        ));
        let mut cards = Vec::new();
        for item in items.iter().skip(page as usize * PER).take(PER) {
            let mut sub = String::new();
            if item.season > 0 && item.episode > 0 {
                sub.push_str(&format!("S{:02}E{:02}", item.season, item.episode));
            }
            if let Some((_, m, d, _, _)) = api::split_iso(&item.release_date) {
                if !sub.is_empty() {
                    sub.push_str(" · ");
                }
                sub.push_str(&format!("{d:02}.{m:02}"));
            }
            let t = item.ref_title();
            let this_open = self.clone_ref();
            cards.push(self.std_poster_card(&t, Some(&sub), true, move |tt| {
                this_open.open_episodes(tt);
            }));
        }
        root.append(&Self::poster_grid(cards, self.grid_cols.get(), true));
        root
    }

    /// Benzer başlıklar ızgarası (detay sayfası + film).
    fn build_related_section(&self, titles: &[Title]) -> gtk::Box {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 4);
        root.set_hexpand(true);
        root.set_halign(gtk::Align::Fill);
        root.set_margin_top(12);
        let head = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        head.set_hexpand(true);
        head.set_halign(gtk::Align::Fill);
        head.set_margin_start(4);
        head.set_margin_end(4);
        let title = gtk::Label::new(Some("✨ BENZERLERİ"));
        title.add_css_class("shelf-title");
        title.set_xalign(0.5);
        title.set_halign(gtk::Align::Fill);
        title.set_justify(gtk::Justification::Center);
        title.set_hexpand(true);
        head.append(&title);
        root.append(&head);
        let mut cards = Vec::new();
        for t in titles {
            let this_open = self.clone_ref();
            cards.push(self.std_poster_card(t, None, true, move |tt| {
                this_open.open_episodes(tt);
            }));
        }
        root.append(&Self::poster_grid(cards, self.grid_cols.get(), true));
        root
    }

    /// Site tarzı devam bölümü: başlık + sağda ‹ ›, altında 5x2 ızgara.
    /// Kartta hover-play (son bölüm direkt), kartın kendisi sayfayı açar.
    fn build_continue_section(&self, items: &[(Title, Option<Episode>)]) -> gtk::Box {
        const PER: usize = 10;
        // COLS artık sabit değil: FlowBox genişliğe göre sarar (dar = 5x2,
        // maximize = tek satırda 10'a kadar). Sayfalama (PER=10) korunur.
        // Sunucuda daha varsa › ilerledikçe çeker (havuz büyür).
        let pool_len = items.len();
        let more_avail = self.client.is_logged_in()
            && self.server_history.borrow().len() < self.server_total.get();
        let pages = ((pool_len + PER - 1) / PER).max(1);
        let page = self.cont_page.get() as usize;
        let root = gtk::Box::new(gtk::Orientation::Vertical, 4);
        root.set_hexpand(true);
        root.set_halign(gtk::Align::Fill);
        let head = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        head.set_hexpand(false);
        head.set_halign(gtk::Align::Center);
        head.set_size_request(Self::grid_width(self.grid_cols.get()) + 64, -1);
        head.set_margin_start(4);
        head.set_margin_end(4);
        let title = gtk::Label::new(Some("Kaldığın Yerden Devam Et"));
        title.add_css_class("shelf-title");
        title.set_xalign(0.5);
        title.set_halign(gtk::Align::Fill);
        title.set_justify(gtk::Justification::Center);
        title.set_hexpand(true);
        head.append(&title);
        // Oklar: sayfa varsa her zaman görünür (sitedeki gibi).
        let prev = gtk::Button::from_icon_name("go-previous-symbolic");
        prev.add_css_class("flat");
        prev.add_css_class("circular");
        prev.set_tooltip_text(Some("Önceki sayfa"));
        prev.set_margin_start(0);
        prev.set_margin_end(0);
        let next = gtk::Button::from_icon_name("go-next-symbolic");
        next.add_css_class("flat");
        next.add_css_class("circular");
        next.set_tooltip_text(Some("Sonraki sayfa"));
        next.set_margin_start(0);
        next.set_margin_end(0);
        prev.set_sensitive(page > 0);
        next.set_sensitive(page + 1 < pages || more_avail);
        let this_p = self.clone_ref();
        prev.connect_clicked(move |_| {
            let p = this_p.cont_page.get();
            if p > 0 {
                this_p.cont_page.set(p - 1);
                this_p.show_page_keep_scroll(&Page::Home);
            }
        });
        let this_n = self.clone_ref();
        let pool_n = pool_len;
        let more_n = more_avail;
        next.connect_clicked(move |_| {
            let p = this_n.cont_page.get() as usize;
            if (p + 1) * PER < pool_n {
                // Havuzda var: anında geç.
                this_n.cont_page.set((p + 1) as u32);
                this_n.show_page_keep_scroll(&Page::Home);
            } else if more_n {
                // Havuz bitti: sonraki sayfaları çek, yeni sayfaya geç.
                this_n.cont_page.set((p + 1) as u32);
                this_n.saved_scroll.set(this_n.current_scroll_value());
                this_n.fetch_server_more();
            }
        });
        head.append(&prev);
        head.append(&next);
        root.append(&head);
        if page * PER >= pool_len {
            // Henüz gelmemiş sayfa (çekiliyor): yükleniyor görünümü.
            let spin_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            spin_row.set_halign(gtk::Align::Center);
            spin_row.set_margin_top(24);
            spin_row.set_margin_bottom(24);
            let sp = gtk::Spinner::new();
            sp.start();
            let lbl = gtk::Label::new(Some("Sonraki sayfa yükleniyor…"));
            lbl.add_css_class("dim-label");
            spin_row.append(&sp);
            spin_row.append(&lbl);
            root.append(&spin_row);
            return root;
        }
        // Sabit 5 sütun ızgara (5x2): FlowBox'un yatay ölçümündeki
        // yazı kesilmesi bu düzende olmaz. Sayfa başına PER=10 kart.
        let mut cards = Vec::new();
        for (t, ep) in items.iter().skip(page * PER).take(PER) {
            cards.push(self.continue_card(t, ep.as_ref()));
        }
        root.append(&Self::poster_grid(cards, self.grid_cols.get(), true));
        root
    }

    /// Site tarzı devam kartı: kapak + hover-play + ad + "S01E02 · Tür".
    /// Izgarada sabit boydur (140x290); maraton butonu her zaman görünür, play butonu hover'da.
    fn continue_card(&self, t: &Title, ep: Option<&Episode>) -> gtk::Box {
        let card = gtk::Box::new(gtk::Orientation::Vertical, 4);
        card.add_css_class("title-btn");
        card.set_size_request(140, 290);
        // Fill: sütun genişliğine (140) sabitlenir. Center natural boyutta
        // çizerdi; uzun başlıklı kartlar daha geniş basılıyordu.
        card.set_halign(gtk::Align::Fill);
        card.set_valign(gtk::Align::Start);
        let pic = self.covers.cover_picture(t.poster.as_deref(), 140, 210);
        pic.set_size_request(140, 210);
        pic.set_can_shrink(false);
        let overlay = gtk::Overlay::new();
        overlay.add_css_class("poster-lift");
        overlay.set_size_request(140, 210);
        overlay.set_child(Some(&pic));
        let play_btn: Option<gtk::Button> = ep.map(|e| {
            let b = gtk::Button::from_icon_name("media-playback-start-symbolic");
            b.add_css_class("circular");
            b.add_css_class("suggested-action");
            b.set_size_request(46, 46);
            b.set_halign(gtk::Align::Center);
            b.set_valign(gtk::Align::Center);
            b.set_visible(false);
            b.set_tooltip_text(Some("Kaldığın yerden oynat"));
            let tc = t.clone();
            let ec = e.clone();
            let this = self.clone_ref();
            b.connect_clicked(move |_| {
                this.play(&tc, &ec);
            });
            overlay.add_overlay(&b);
            b
        });
        // Maraton hızlı ekleme (sol üstte her zaman görünür).
        let member = self.client.is_in_marathon(t.id);
        let mara_btn = {
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
            b.set_visible(true); // her zaman görünür
            b.set_tooltip_text(Some(if member {
                "Maratonda ✓ (çıkarmak için tıkla)"
            } else {
                "Maratona ekle"
            }));
            let b_c = b.clone();
            let t_m = t.clone();
            let this_m = self.clone_ref();
            b.connect_clicked(move |_| {
                let added = this_m.toggle_marathon_quick(&t_m);
                b_c.set_icon_name(if added {
                    "object-select-symbolic"
                } else {
                    "list-add-symbolic"
                });
            });
            overlay.add_overlay(&b);
            b
        };
        card.append(&overlay);
        let name = gtk::Label::new(Some(&t.name));
        name.add_css_class("card-title");
        // Tek satır: 2 satıra saran başlıklar kart boylarını bozuyordu.
        name.set_wrap(false);
        name.set_single_line_mode(true);
        name.set_justify(gtk::Justification::Center);
        name.set_xalign(0.5);
        name.set_max_width_chars(16);
        name.set_lines(1);
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        card.append(&name);
        let mut sub = String::new();
        if let Some(e) = ep {
            if e.season > 0 && e.episode > 0 {
                sub.push_str(&format!("S{:02}E{:02}", e.season, e.episode));
            }
        }
        if let Some(g) = t.genre_line() {
            let first = g.split("  •  ").next().unwrap_or(&g);
            if !sub.is_empty() {
                sub.push_str(" · ");
            }
            sub.push_str(first);
        }
        // Alt yazı her kartta 1 satır yer kaplar (boşsa bile) ki kart
        // boyları eşit kalsın.
        let meta = gtk::Label::new(Some(if sub.is_empty() { " " } else { &sub }));
        meta.add_css_class("dim-label");
        meta.set_xalign(0.5);
        meta.set_wrap(false);
        meta.set_single_line_mode(true);
        meta.set_ellipsize(gtk::pango::EllipsizeMode::End);
        meta.set_max_width_chars(18);
        card.append(&meta);
        // Hover'da sadece play butonunu göster/gizle, posteri hafif kaldır.
        {
            let motion = gtk::EventControllerMotion::new();
            let pb_c = play_btn.clone();
            let pb_c2 = play_btn.clone();
            let ov_e = overlay.clone();
            let ov_l = overlay.clone();
            motion.connect_enter(move |_, _, _| {
                if let Some(ref pb) = pb_c {
                    pb.set_visible(true);
                }
                ov_e.add_css_class("lifted");
            });
            motion.connect_leave(move |_| {
                if let Some(ref pb) = pb_c2 {
                    pb.set_visible(false);
                }
                ov_l.remove_css_class("lifted");
            });
            card.add_controller(motion);
        }
        let title_clone = t.clone();
        let this = self.clone_ref();
        let pb_hit = play_btn.clone();
        let mb_hit = mara_btn.clone();
        let card_c = card.clone();
        let gesture = gtk::GestureClick::new();
        gesture.connect_pressed(move |_, _, x, y| {
            // Butonlara basıldıysa buton halleder (çift işlem olmasın).
            let targets: [Option<&gtk::Button>; 2] = [pb_hit.as_ref(), Some(&mb_hit)];
            for target in targets.into_iter().flatten() {
                if let Some((bx, by)) = target.translate_coordinates(&card_c, 0.0, 0.0) {
                    let w = target.width() as f64;
                    let h = target.height() as f64;
                    if x >= bx && x <= bx + w && y >= by && y <= by + h {
                        return;
                    }
                }
            }
            this.open_episodes(title_clone.clone());
        });
        card.add_controller(gesture);
        self.bind_card_hover(&card, t.poster.as_deref());
        card
    }

    /// Spot ışığı öğeleri: birleşik geçmişin en yenisi (direkt devam),
    /// sonra ilk kategoriden öne çıkanlar (en fazla 8 slayt),
    /// en sonda son 3 haber.
    fn spotlight_items(&self) -> Vec<Spot> {
        let mut out: Vec<Spot> = Vec::new();
        // (zaman_ms, başlık, kaldığı bölüm)
        let mut first: Option<(u64, Title, Option<Episode>)> = None;
        if self.settings.borrow().local_history_enabled {
            if let Some(h) = self.client.load_state().history.first() {
                first = Some((h.ts.saturating_mul(1000), h.title.clone(), Some(h.episode.clone())));
            }
        }
        for s in self.server_history.borrow().iter() {
            let ep = if s.season > 0 && s.episode > 0 {
                Some(Episode {
                    episode: s.episode,
                    season: s.season,
                    name: format!("Bölüm {}", s.episode),
                    thumbnail: None,
                })
            } else {
                None
            };
            match &first {
                Some((ts, _, _)) if *ts >= s.date => {}
                _ => first = Some((s.date, s.title.clone(), ep)),
            }
        }
        if let Some((_, title, resume_ep)) = first {
            out.push(Spot::Title { title, resume_ep });
        }
        let cats = self.cats.borrow();
        if let Some(first) = cats.first() {
            for t in first.items.iter().take(12) {
                if out.len() >= 8 {
                    break;
                }
                if out.iter().any(|s| matches!(s, Spot::Title { title, .. } if title.id == t.id)) {
                    continue;
                }
                out.push(Spot::Title {
                    title: t.clone(),
                    resume_ep: None,
                });
            }
        }
        drop(cats);
        // En son 3 haber spotun kuyruğuna eklenir.
        for n in self.news_rail.borrow().iter().take(3) {
            out.push(Spot::News(n.clone()));
        }
        out
    }

    fn build_spot_slide(&self, sp: &Spot) -> gtk::Box {
        match sp {
            Spot::Title { title, resume_ep } => self.build_spot_title_slide(title, resume_ep.as_ref()),
            Spot::News(item) => self.build_spot_news_slide(item),
        }
    }

    /// Haber slaytı: banner + başlık + tarih + "Haberi Oku".
    fn build_spot_news_slide(&self, item: &NewsItem) -> gtk::Box {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.set_hexpand(true);
        root.set_halign(gtk::Align::Fill);
        root.add_css_class("hero-card");
        let overlay = gtk::Overlay::new();
        overlay.set_size_request(-1, self.spot_h.get());
        overlay.add_css_class("hero-clip");
        let pic = self.covers.cover_picture_hero(
            item.backdrop.as_deref().or(item.image.as_deref()),
            1280,
            self.spot_h.get(),
        );
        pic.set_hexpand(true);
        pic.set_width_request(-1);
        pic.set_content_fit(gtk::ContentFit::Cover);
        overlay.set_child(Some(&pic));

        let shade = gtk::Box::new(gtk::Orientation::Vertical, 6);
        shade.add_css_class("hero-shade");
        shade.set_halign(gtk::Align::Fill);
        shade.set_valign(gtk::Align::End);
        shade.set_hexpand(true);
        let kick = gtk::Label::new(Some("📰 GÜNDEM"));
        kick.add_css_class("dim-label");
        kick.set_xalign(0.0);
        shade.append(&kick);
        let name = gtk::Label::new(Some(item.title.trim()));
        name.add_css_class("title-1");
        name.set_xalign(0.0);
        name.set_wrap(true);
        name.set_lines(2);
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        name.set_max_width_chars(90);
        shade.append(&name);
        let date = gtk::Label::new(Some(&api::fmt_tr_datetime(&item.created_at)));
        date.add_css_class("dim-label");
        date.set_xalign(0.0);
        shade.append(&date);
        let btn = gtk::Button::new();
        btn.add_css_class("suggested-action");
        btn.add_css_class("pill");
        let bbox = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        bbox.append(&gtk::Image::from_icon_name("internet-news-reader-symbolic"));
        bbox.append(&gtk::Label::new(Some("Haberi Oku")));
        btn.set_child(Some(&bbox));
        btn.set_halign(gtk::Align::Start);
        {
            let this = self.clone_ref();
            let it = item.clone();
            btn.connect_clicked(move |_| {
                this.open_news(&it);
            });
        }
        shade.append(&btn);
        overlay.add_overlay(&shade);
        root.append(&overlay);
        root
    }

    fn build_spot_title_slide(&self, t: &Title, resume_ep: Option<&Episode>) -> gtk::Box {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.set_hexpand(true);
        root.set_halign(gtk::Align::Fill);
        root.add_css_class("hero-card");
        let overlay = gtk::Overlay::new();
        overlay.set_size_request(-1, self.spot_h.get());
        overlay.add_css_class("hero-clip");
        let pic = self.covers.cover_picture_hero(
            t.backdrop.as_deref().or(t.poster.as_deref()),
            1280,
            self.spot_h.get(),
        );
        pic.set_hexpand(true);
        // Genişlik minimumu dayatma (pencereye sığmaz, sidebar'ı ezer);
        // texture 1280px üretilir, widget esner.
        pic.set_width_request(-1);
        pic.set_content_fit(gtk::ContentFit::Cover);
        overlay.set_child(Some(&pic));

        let shade = gtk::Box::new(gtk::Orientation::Vertical, 6);
        shade.add_css_class("hero-shade");
        shade.set_halign(gtk::Align::Fill);
        shade.set_valign(gtk::Align::End);
        shade.set_hexpand(true);
        // Kaynak rozeti (sitedeki "Sezonun İncileri" hapı gibi).
        if let Some(g) = t.genre_line() {
            let first = g.split("  •  ").next().unwrap_or(&g).to_string();
            if !first.is_empty() {
                let badge = gtk::Label::new(Some(&first));
                badge.add_css_class("detail-badge");
                badge.set_xalign(0.0);
                badge.set_halign(gtk::Align::Start);
                shade.append(&badge);
            }
        }
        let name = gtk::Label::new(Some(&t.display_name()));
        name.add_css_class("title-1");
        name.set_xalign(0.0);
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        shade.append(&name);
        if let Some(g) = t.genre_line() {
            let sub = gtk::Label::new(Some(&format!(
                "{g} · {}",
                t.year.map(|y| y.to_string()).unwrap_or_default()
            )));
            sub.add_css_class("hero-genre");
            sub.set_xalign(0.0);
            sub.set_wrap(false);
            sub.set_single_line_mode(true);
            sub.set_ellipsize(gtk::pango::EllipsizeMode::End);
            shade.append(&sub);
        }
        if let Some(d) = t.description.as_deref() {
            let ds = gtk::Label::new(Some(d));
            ds.set_wrap(true);
            ds.set_lines(2);
            ds.set_ellipsize(gtk::pango::EllipsizeMode::End);
            ds.set_xalign(0.0);
            ds.set_max_width_chars(90);
            ds.add_css_class("dim-label");
            shade.append(&ds);
        }
        let btn = gtk::Button::new();
        btn.add_css_class("suggested-action");
        btn.add_css_class("pill");
        // Butonun kendi ikon+etiket dizilimi etiketi yutabiliyor; elle kur.
        let bbox = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        bbox.append(&gtk::Image::from_icon_name("media-playback-start-symbolic"));
        bbox.append(&gtk::Label::new(Some(if resume_ep.is_some() {
            "Devam Et"
        } else {
            "İzle"
        })));
        btn.set_child(Some(&bbox));
        btn.set_halign(gtk::Align::Start);
        {
            let this = self.clone_ref();
            let tc = t.clone();
            // Devam Et: direkt kaldığın bölümü oynat.
            // Diğerleri: bölüm listesini aç.
            match resume_ep.cloned() {
                Some(ep) => btn.connect_clicked(move |_| {
                    this.play(&tc, &ep);
                }),
                None => btn.connect_clicked(move |_| {
                    this.open_episodes(tc.clone());
                }),
            };
        }
        shade.append(&btn);
        overlay.add_overlay(&shade);
        root.append(&overlay);
        root
    }

    /// Otomatik dönen spot ışığı (yerel Carousel + nokta göstergesi).
    /// Kaydırma/sürükleme yereldir; 6sn'de bir sonraki slayta kayar.
    /// Tekerlek carousel'i kaydırmaz (sayfa kayar); noktalar üstünde
    /// tekerlek slayt değiştirir.
    fn build_spotlight(&self, spots: &[Spot]) -> gtk::Box {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 6);
        let carousel = adw::Carousel::new();
        carousel.set_allow_mouse_drag(true);
        carousel.set_allow_scroll_wheel(false);
        carousel.set_size_request(-1, self.spot_h.get());
        carousel.set_hexpand(true);
        let mut pages: Vec<gtk::Box> = Vec::with_capacity(spots.len());
        for sp in spots.iter() {
            let page = self.build_spot_slide(sp);
            carousel.append(&page);
            pages.push(page);
        }
        let pages = Rc::new(pages);
        let dots = adw::CarouselIndicatorDots::new();
        dots.set_carousel(Some(&carousel));
        dots.set_halign(gtk::Align::Center);
        {
            // Noktalar üstünde tekerlek: önceki/sonraki slayt.
            let pages_w = pages.clone();
            let car_w = carousel.downgrade();
            let wheel = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
            wheel.connect_scroll(move |_, _, dy| {
                let Some(car) = car_w.upgrade() else {
                    return glib::Propagation::Proceed;
                };
                let n = car.n_pages();
                if n < 2 {
                    return glib::Propagation::Proceed;
                }
                let cur = car.position().round() as i32;
                let ni = (cur + if dy > 0.0 { 1 } else { -1 }).rem_euclid(n as i32) as u32;
                if let Some(page) = pages_w.get(ni as usize) {
                    car.scroll_to(page, true);
                    return glib::Propagation::Stop;
                }
                glib::Propagation::Proceed
            });
            dots.add_controller(wheel);
        }
        if pages.len() > 1 {
            let pages_t = pages.clone();
            let car_w = carousel.downgrade();
            glib::timeout_add_local(std::time::Duration::from_secs(6), move || {
                let Some(car) = car_w.upgrade() else {
                    return glib::ControlFlow::Break;
                };
                let n = car.n_pages();
                if n < 2 {
                    return glib::ControlFlow::Continue;
                }
                let ni = (car.position().round() as u32 + 1) % n;
                if let Some(page) = pages_t.get(ni as usize) {
                    car.scroll_to(page, true);
                }
                glib::ControlFlow::Continue
            });
        }
        root.append(&carousel);
        root.append(&dots);
        root
    }

    /// Site tarzı poster kartı (maraton hızlı eklemeli).
    fn std_poster_card(
        &self,
        t: &Title,
        sub: Option<&str>,
        with_marathon: bool,
        on_open: impl Fn(Title) + 'static,
    ) -> gtk::Box {
        let this_cov = self.clone_ref();
        let this_hov = self.clone_ref();
        let this_mar = self.clone_ref();
        let t_m = t.clone();
        let mara: Option<(bool, Rc<dyn Fn() -> bool>)> = if with_marathon {
            let member = self.client.is_in_marathon(t.id);
            Some((
                member,
                Rc::new(move || this_mar.toggle_marathon_quick(&t_m)) as Rc<dyn Fn() -> bool>,
            ))
        } else {
            None
        };
        info_views::poster_card(
            t,
            sub,
            move |poster, pic, w, h| {
                this_cov.covers.load_cover(poster, &pic, w, h);
            },
            move |card, poster| {
                this_hov.bind_card_hover(card, poster);
            },
            mara,
            on_open,
        )
    }
    fn poster_grid(cards: Vec<gtk::Box>, cols: u32, center: bool) -> gtk::Grid {
        let cols = cols.max(1);
        let grid = gtk::Grid::new();
        grid.set_column_spacing(12);
        // Tüm sütunlar en geniş kadar: uzun başlıklı kart komşusunu itemez.
        grid.set_column_homogeneous(true);
        grid.set_row_spacing(18);
        grid.set_halign(if center { gtk::Align::Center } else { gtk::Align::Start });
        grid.set_hexpand(true);
        for (i, card) in cards.into_iter().enumerate() {
            grid.attach(&card, (i as u32 % cols) as i32, (i as u32 / cols) as i32, 1, 1);
        }
        grid
    }

    /// Sütun sayısından ızgara piksel genişliği (başlık hizası için).
    fn grid_width(cols: u32) -> i32 {
        let cols = cols.max(1) as i32;
        cols * 140 + (cols - 1) * 12
    }


    /// Mevcut sayfanın kaydırma konumu (yoksa 0).
    fn current_scroll_value(&self) -> f64 {
        self.stack
            .visible_child()
            .and_downcast::<gtk::ScrolledWindow>()
            .map(|sw| sw.vadjustment().value())
            .unwrap_or(0.0)
    }

    /// Kaydedilmiş konumu idle'da geri yaz (yeniden kurulum sonrası).
    fn restore_scroll_value(&self, saved: f64) {
        if saved <= 0.0 {
            return;
        }
        let stack = self.stack.clone();
        glib::idle_add_local_once(move || {
            if let Some(sw) = stack.visible_child().and_downcast::<gtk::ScrolledWindow>() {
                let adj = sw.vadjustment();
                let max = (adj.upper() - adj.page_size()).max(adj.lower());
                adj.set_value(saved.clamp(adj.lower(), max));
            }
        });
    }

    /// show_page + kaydırma koruma (sayfa ‹ › okları için).
    fn show_page_keep_scroll(&self, page: &Page) {
        let saved = self.current_scroll_value();
        self.show_page(page);
        self.restore_scroll_value(saved);
    }

    /// Sayfalı bölüm başlığı: ortalı başlık + ızgara kenarında ‹ ›.
    /// Başlık kutusu ızgaradan biraz geniştir (ok payı); oklar ızgaranın
    /// sağına oturur ama içeriden taşmaz.
    fn pager_head(
        title_text: &str,
        page: u32,
        pages: u32,
        width: i32,
        set_page: impl Fn(u32) + 'static,
    ) -> gtk::Box {
        let head = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        head.set_hexpand(false);
        head.set_halign(gtk::Align::Center);
        head.set_size_request(width + 64, -1);
        head.set_margin_start(0);
        head.set_margin_end(0);
        let title = gtk::Label::new(Some(title_text));
        title.add_css_class("shelf-title");
        title.set_xalign(0.5);
        title.set_halign(gtk::Align::Fill);
        title.set_justify(gtk::Justification::Center);
        title.set_hexpand(true);
        let prev = gtk::Button::from_icon_name("go-previous-symbolic");
        prev.add_css_class("flat");
        prev.add_css_class("circular");
        prev.add_css_class("grid-pager-btn");
        prev.set_tooltip_text(Some("Önceki sayfa"));
        prev.set_margin_start(0);
        prev.set_margin_end(0);
        let next = gtk::Button::from_icon_name("go-next-symbolic");
        next.add_css_class("flat");
        next.add_css_class("circular");
        next.add_css_class("grid-pager-btn");
        next.set_tooltip_text(Some("Sonraki sayfa"));
        next.set_margin_start(0);
        next.set_margin_end(0);
        prev.set_sensitive(page > 0);
        next.set_sensitive(page + 1 < pages);
        let set_page = Rc::new(set_page);
        let set_p = set_page.clone();
        prev.connect_clicked(move |_| {
            if page > 0 {
                set_p(page - 1);
            }
        });
        let set_n = set_page.clone();
        next.connect_clicked(move |_| {
            set_n(page + 1);
        });
        head.append(&title);
        head.append(&prev);
        head.append(&next);
        head
    }

    /// Kategori ızgarası: 5 sütun, sayfada 10 kart, başlıkta ‹ ›.
    fn build_carousel(&self, cat: &api::Category, idx: usize) -> gtk::Box {
        const PER: usize = 10;
        let pages = ((cat.items.len() + PER - 1) / PER).max(1);
        let mut page = self.cat_pages.borrow().get(&idx).copied().unwrap_or(0);
        page = page.min(pages as u32 - 1);
        let root = gtk::Box::new(gtk::Orientation::Vertical, 4);
        root.set_hexpand(true);
        root.set_halign(gtk::Align::Fill);
        let this = self.clone_ref();
        let cols = self.grid_cols.get();
        root.append(&Self::pager_head(
            &cat.name,
            page,
            pages as u32,
            Self::grid_width(cols),
            move |p| {
                this.cat_pages.borrow_mut().insert(idx, p);
                this.show_page_keep_scroll(&Page::Home);
            },
        ));
        let mut cards = Vec::new();
        for t in cat.items.iter().skip(page as usize * PER).take(PER) {
            let sub = t.genre_line().map(|g| g.replace("  •  ", " / "));
            let this_open = self.clone_ref();
            cards.push(self.std_poster_card(t, sub.as_deref(), true, move |tt| {
                this_open.open_episodes(tt);
            }));
        }
        root.append(&Self::poster_grid(cards, self.grid_cols.get(), true));
        root
    }

    fn build_home_view(&self) -> gtk::ScrolledWindow {
        let scroll = gtk::ScrolledWindow::new();
        scroll.set_hexpand(true);
        scroll.set_vexpand(true);

        let outer = gtk::Box::new(gtk::Orientation::Vertical, 0);
        scroll.set_child(Some(&outer));

        // İnternet bağlantı uyarısı (offline ise)
        match crate::api::check_internet() {
            crate::api::InternetStatus::Online => {}
            crate::api::InternetStatus::Offline { reason } => {
                let banner = adw::Banner::new("İnternet bağlantısı yok");
                banner.set_button_label(Some("Yeniden Kontrol Et"));
                let this = self.clone_ref();
                banner.connect_button_clicked(move |_| {
                    this.refresh_internet_status();
                });
                outer.append(&banner);
            }
        }

        let cats = self.cats.borrow();

        if cats.is_empty() {
            let spinner_box = gtk::Box::new(gtk::Orientation::Vertical, 16);
            spinner_box.set_valign(gtk::Align::Center);
            spinner_box.set_halign(gtk::Align::Center);
            spinner_box.set_vexpand(true);
            let spinner = gtk::Spinner::new();
            spinner.set_size_request(48, 48);
            spinner.start();
            let lbl = gtk::Label::new(Some("İçerikler yükleniyor…"));
            lbl.add_css_class("dim-label");
            spinner_box.append(&spinner);
            spinner_box.append(&lbl);
            scroll.set_child(Some(&spinner_box));
            return scroll;
        }

        let main_box = gtk::Box::new(gtk::Orientation::Vertical, 18);
        main_box.set_hexpand(true);
        main_box.set_halign(gtk::Align::Fill);
        // Üstten bitişik: hızlı arama kendi payını taşır, spot yukarı oturur.
        main_box.set_margin_top(0);
        main_box.set_margin_bottom(18);
        main_box.set_margin_start(12);
        main_box.set_margin_end(12);

        self.fetch_server_history(true);
        // Spot en üstte, bitişik (üzerinde yüzen hızlı arama hapı var).
        let spots = self.spotlight_items();
        if !spots.is_empty() {
            main_box.append(&self.build_spotlight(&spots));
        }
        // Spotun altında: kaldığın yerler — site tarzı 5x2 ızgara,
        // başlıkta ‹ › (sayfa başına 10).
        {
            let items = self.continue_items();
            if !items.is_empty() {
                main_box.append(&self.build_continue_section(&items));
            }
        }
        // Devamın altında: son eklenen bölümler (5x2 ızgara, ‹ › sayfalı).
        {
            let rail = self.last_rail.borrow().clone();
            if !rail.is_empty() {
                main_box.append(&self.build_last_section(&rail));
            }
        }

        for (idx, cat) in cats.iter().enumerate() {
            main_box.append(&self.build_carousel(cat, idx));
        }

        // Spotun üzerinde yüzen hızlı arama hapı. İçerik GNOME HIG'e göre
        // kelepçeli (geniş pencerede okunabilirlik); hero kelepçeye yayılır.
        let overlay = gtk::Overlay::new();
        let clamp = adw::Clamp::new();
        clamp.set_maximum_size(1400);
        clamp.set_child(Some(&main_box));
        overlay.set_child(Some(&clamp));
        {
            let quick = gtk::SearchEntry::new();
            quick.set_placeholder_text(Some("Hızlı ara… (Enter)"));
            quick.add_css_class("quick-search");
            quick.set_size_request(560, -1);
            quick.set_halign(gtk::Align::Center);
            quick.set_valign(gtk::Align::Start);
            quick.set_margin_top(10);
            let this = self.clone_ref();
            quick.connect_activate(move |e| {
                let q = e.text().to_string();
                if !q.trim().is_empty() {
                    this.do_search(q);
                }
            });
            overlay.add_overlay(&quick);
        }
        scroll.set_child(Some(&overlay));
        scroll
    }

    fn build_calendar_view(&self) -> gtk::ScrolledWindow {
        let days = self.cal_days.borrow().clone();
        let sel = self.cal_sel.get() as usize;
        let loading = self.cal_loading.get();
        let error = self.cal_error.borrow().clone();
        let this_day = self.clone_ref();
        let this_open = self.clone_ref();
        let this_cov = self.clone_ref();
        let this_retry = self.clone_ref();
        let view = info_views::calendar_view(
            &days,
            sel,
            loading,
            error,
            move |idx| {
                this_day.cal_sel.set(idx);
                this_day.show_page(&Page::Calendar);
            },
            move |title| {
                this_open.open_episodes(title);
            },
            move |poster, pic, w, h| {
                this_cov.covers.load_cover(poster, &pic, w, h);
            },
            move || {
                this_retry.fetch_calendar();
            },
        );
        let scroll = gtk::ScrolledWindow::new();
        scroll.set_hexpand(true);
        scroll.set_vexpand(true);
        scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        scroll.set_child(Some(&view));
        scroll
    }

    fn build_news_view(&self) -> gtk::ScrolledWindow {
        let items = self.news_items.borrow().clone();
        let total = self.news_total.get();
        let page = self.news_page.get();
        let last = self.news_last.get();
        let loading = self.news_loading.get();
        let error = self.news_error.borrow().clone();
        let this_open = self.clone_ref();
        let this_cov = self.clone_ref();
        let this_more = self.clone_ref();
        let this_retry = self.clone_ref();
        let view = info_views::news_list(
            &items,
            total,
            page,
            last,
            loading,
            error,
            move |item| {
                this_open.open_news(&item);
            },
            move |poster, pic, w, h| {
                this_cov.covers.load_cover(poster, &pic, w, h);
            },
            move || {
                this_more.fetch_news_page(this_more.news_page.get() + 1);
            },
            move || {
                *this_retry.news_items.borrow_mut() = Vec::new();
                this_retry.news_page.set(0);
                this_retry.fetch_news_page(1);
            },
        );
        let scroll = gtk::ScrolledWindow::new();
        scroll.set_hexpand(true);
        scroll.set_vexpand(true);
        scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        scroll.set_child(Some(&view));
        scroll
    }

    fn build_news_detail_view(&self, item: &NewsItem) -> gtk::ScrolledWindow {
        let this_cov = self.clone_ref();
        let view = info_views::news_detail(
            item,
            move |poster, pic, w, h| {
                this_cov.covers.load_cover(poster, &pic, w, h);
            },
        );
        let scroll = gtk::ScrolledWindow::new();
        scroll.set_hexpand(true);
        scroll.set_vexpand(true);
        scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        scroll.set_child(Some(&view));
        scroll
    }

    fn build_reviews_view(&self) -> gtk::ScrolledWindow {
        let items = self.rev_items.borrow().clone();
        let total = self.rev_total.get();
        let page = self.rev_page.get();
        let last = self.rev_last.get();
        let loading = self.rev_loading.get();
        let error = self.rev_error.borrow().clone();
        let this_more = self.clone_ref();
        let this_retry = self.clone_ref();
        let this_cov = self.clone_ref();
        let tid = self.rev_title.get();
        let view = info_views::reviews_list(
            &items,
            total,
            page,
            last,
            loading,
            error,
            move |poster, pic, w, h| {
                this_cov.covers.load_cover(poster, &pic, w, h);
            },
            move || {
                this_more.fetch_reviews_page(this_more.rev_title.get(), this_more.rev_page.get() + 1);
            },
            move || {
                *this_retry.rev_items.borrow_mut() = Vec::new();
                this_retry.rev_page.set(0);
                this_retry.fetch_reviews_page(tid, 1);
            },
        );
        let scroll = gtk::ScrolledWindow::new();
        scroll.set_hexpand(true);
        scroll.set_vexpand(true);
        scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        scroll.set_child(Some(&view));
        scroll
    }

    fn build_marathon_view(&self) -> gtk::ScrolledWindow {
        let scroll = gtk::ScrolledWindow::new();
        let this_click = self.clone_ref();
        let this_toggle = self.clone_ref();
        let this_remove = self.clone_ref();
        let this_clear = self.clone_ref();
        let this_cover = self.clone_ref();
        let this_reorder = self.clone_ref();

        let view = views::MarathonView::build(
            self.client.clone(),
            move |title| {
                this_click.open_episodes(title);
            },
            move |id| {
                let item = this_toggle.client.get_marathon().into_iter().find(|m| m.title.id == id);
                let Some(item) = item else { return; };
                if item.completed {
                    this_toggle.client.mark_title_unwatched(id);
                    this_toggle.client.set_marathon_completed(id, false);
                    let toast = adw::Toast::new("⏳ Tüm bölümler izlenmedi olarak işaretlendi");
                    toast.set_timeout(2);
                    this_toggle.toast.add_toast(toast);
                    this_toggle.show_page(&Page::Marathon);
                    return;
                }
                let title = item.title.clone();
                let client = this_toggle.client.clone();
                let (tx, rx) = std::sync::mpsc::channel::<Result<usize, String>>();
                std::thread::spawn(move || {
                    let _ = tx.send(client.mark_title_watched(&title));
                });
                let this_async = this_toggle.clone();
                glib::idle_add_local(move || match rx.try_recv() {
                    Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                    msg => {
                        match msg {
                            Ok(Ok(n)) => {
                                this_async.client.set_marathon_completed(id, true);
                                let toast = adw::Toast::new(&format!("🏁 {n} bölüm izlendi olarak işaretlendi!"));
                                toast.set_timeout(2);
                                this_async.toast.add_toast(toast);
                            }
                            _ => {
                                let toast = adw::Toast::new("❌ Bölüm listesi alınamadı (internete bağlı mısın?)");
                                toast.set_timeout(3);
                                this_async.toast.add_toast(toast);
                            }
                        }
                        this_async.show_page(&Page::Marathon);
                        glib::ControlFlow::Break
                    }
                });
            },
            move |id| {
                this_remove.client.remove_from_marathon(id);
                let toast = adw::Toast::new("Maratondan kaldırıldı");
                toast.set_timeout(2);
                this_remove.toast.add_toast(toast);
                this_remove.show_page(&Page::Marathon);
            },
            move || {
                this_clear.client.clear_marathon();
                let toast = adw::Toast::new("İzleme maratonu temizlendi");
                toast.set_timeout(2);
                this_clear.toast.add_toast(toast);
                this_clear.show_page(&Page::Marathon);
            },
            move |id, new_index| {
                this_reorder.client.reorder_marathon(id, new_index);
                this_reorder.show_page(&Page::Marathon);
            },
            move |poster, pic, w, h| {
                this_cover.covers.load_cover(poster, &pic, w, h);
            },
        );
        scroll.set_child(Some(&view));

        let motion = gtk::DropControllerMotion::new();
        let drag_pos: Rc<RefCell<Option<(f64, f64)>>> = Rc::new(RefCell::new(None));
        let motion_state = drag_pos.clone();
        let scroll_m = scroll.clone();
        motion.connect_motion(move |_, _x, y| {
            let h = scroll_m.height() as f64;
            *motion_state.borrow_mut() = Some((y, h));
        });
        let leave_state = drag_pos.clone();
        motion.connect_leave(move |_| {
            *leave_state.borrow_mut() = None;
        });
        scroll.add_controller(motion);

        let scroll_t = scroll.clone();
        let timer_state = drag_pos.clone();
        gtk::glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
            if let Some((y, h)) = *timer_state.borrow() {
                let margin = 50.0;
                let adj = scroll_t.vadjustment();
                let max = (adj.upper() - adj.page_size()).max(0.0);
                let cur = adj.value();
                let new = if y < margin {
                    (cur - ((margin - y) * 0.6 + 6.0)).clamp(0.0, max)
                } else if y > h - margin {
                    (cur + ((y - (h - margin)) * 0.6 + 6.0)).clamp(0.0, max)
                } else {
                    cur
                };
                adj.set_value(new);
            }
            gtk::glib::ControlFlow::Continue
        });

        scroll
    }

    fn build_favs_view(&self) -> gtk::ScrolledWindow {
        let scroll = gtk::ScrolledWindow::new();
        scroll.set_hexpand(true);
        scroll.set_vexpand(true);
        scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        let saved = self.client.load_state().saved;
        if saved.is_empty() {
            let sp = components::create_status_page(
                "Henüz Favori Eklenmedi",
                "Beğendiğiniz anime, dizileri ve filmleri yıldız ikonuna tıklayarak favorilerinize ekleyin.",
                "starred-symbolic",
            );
            scroll.set_child(Some(&sp));
            return scroll;
        }

        let list_box = gtk::Box::new(gtk::Orientation::Vertical, 5);
        list_box.set_margin_top(6);
        list_box.set_margin_bottom(6);
        list_box.set_margin_start(10);
        list_box.set_margin_end(10);
        list_box.set_vexpand(false);

        for t in saved {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
            row.add_css_class("fav-item-card");

            let pic = gtk::Picture::new();
            pic.set_width_request(48);
            pic.set_height_request(72);
            pic.set_can_shrink(true);
            pic.set_content_fit(gtk::ContentFit::Cover);
            pic.set_css_classes(&["cover", "cover-thumb"]);
            pic.set_valign(gtk::Align::Center);
            self.covers.load_cover(t.poster.as_deref(), &pic, 48, 72);

            let info_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
            info_box.set_valign(gtk::Align::Center);
            info_box.set_hexpand(true);

            let name = gtk::Label::new(Some(&t.name));
            name.add_css_class("title-3");
            name.set_xalign(0.0);
            name.set_wrap(false);
            name.set_single_line_mode(true);
            name.set_ellipsize(gtk::pango::EllipsizeMode::End);

            info_box.append(&name);
            episodes_view::append_title_submeta(&info_box, &t);

            let actions = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            actions.set_valign(gtk::Align::Center);

            let play_btn = gtk::Button::with_label("▶ İzle");
            play_btn.add_css_class("suggested-action");
            play_btn.add_css_class("pill");
            let this_play = self.clone_ref();
            let t_play = t.clone();
            play_btn.connect_clicked(move |_| this_play.open_episodes(t_play.clone()));

            let del_btn = gtk::Button::from_icon_name("user-trash-symbolic");
            del_btn.add_css_class("flat");
            del_btn.add_css_class("circular");
            del_btn.add_css_class("destructive-action");
            del_btn.set_tooltip_text(Some("Favorilerden Çıkar"));
            let this_del = self.clone_ref();
            let t_del = t.clone();
            del_btn.connect_clicked(move |_| {
                this_del.client.toggle_saved(&t_del);
                this_del.show_page(&Page::Favs);
            });

            actions.append(&play_btn);
            actions.append(&del_btn);

            row.append(&pic);
            row.append(&info_box);
            row.append(&actions);

            list_box.append(&row);
        }

        scroll.set_child(Some(&list_box));
        scroll
    }

    /// Sunucu üst geçmişini çek (girişliyse): devam listesi + spot.
    /// reset=true: havuzu ilk 3 sayfayla değiştirir (açılış/giriş/yenile).
    /// reset=false: ilk 3 sayfayı birleştirir (gezinti tazeliği).
    /// 5dk önbellekli. Bitince sayfa tazelenir.
    fn fetch_server_history(&self, reset: bool) {
        if !self.client.is_logged_in() {
            return;
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let stale = now.saturating_sub(self.server_history_at.get()) >= 300;
        if self.server_history_loaded.get() && !stale {
            return;
        }
        // Bayrakları hemen koy (çift tetiklenmesin).
        self.server_history_loaded.set(true);
        self.server_history_at.set(now);
        self.spawn(move |c| {
            let res = c.server_top(3);
            move || Msg::ServerHistory(res, reset)
        });
    }

    /// Devam havuzunu büyüt: sonraki 3 sayfayı çekip birleştirir
    /// (ana sayfadaki "Daha Fazla Yükle" butonu).
    fn fetch_server_more(&self) {
        if !self.client.is_logged_in() || self.server_more_loading.get() {
            return;
        }
        let from = self.server_pool_pages.get();
        let new_pool = from + 3;
        self.server_more_loading.set(true);
        // Buton durumu hemen güncellensin.
        if self.page_history.borrow().last() == Some(&Page::Home) {
            self.show_page(&Page::Home);
        }
        self.spawn(move |c| {
            let res = c.server_pages(from, 3);
            move || Msg::ServerMore(new_pool, res)
        });
    }

    /// Geçmişin istenen sayfasını API'den çekip listeye ekler
    /// ("Daha Fazla Göster" / ilk açılış / yenileme).
    /// Bitince Geçmiş sayfası tazelenir.
    fn fetch_hist_page(&self, page: u32) {
        if !self.client.is_logged_in() {
            return;
        }
        self.hist_loading.set(true);
        *self.hist_error.borrow_mut() = None;
        // Spinner hemen görünsün.
        if self.page_history.borrow().last() == Some(&Page::History) {
            self.show_page(&Page::History);
        }
        self.spawn(move |c| {
            let res = c.server_history_page(page);
            move || Msg::HistPage(page, res)
        });
    }

    /// Haber sayfası (1-indexli). İlk sayfada liste sıfırlanır,
    /// sonrakiler alta eklenir.
    fn fetch_news_page(&self, page: u32) {
        let page = page.max(1);
        if self.news_loading.get() {
            return;
        }
        self.news_loading.set(true);
        *self.news_error.borrow_mut() = None;
        if self.page_history.borrow().last() == Some(&Page::News) {
            self.show_page(&Page::News);
        }
        self.spawn(move |c| {
            let res = c.news(page);
            move || Msg::NewsPage(page, res)
        });
    }

    /// Yayın takvimi (tek istek, 7 gün).
    fn fetch_calendar(&self) {
        if self.cal_loading.get() {
            return;
        }
        self.cal_loading.set(true);
        *self.cal_error.borrow_mut() = None;
        if self.page_history.borrow().last() == Some(&Page::Calendar) {
            self.show_page(&Page::Calendar);
        }
        self.spawn(move |c| {
            let res = c.calendar();
            move || Msg::Calendar(res)
        });
    }

    /// Ana sayfadaki spot haber slaytları (haberler sayfa 1, sessiz).
    fn fetch_news_rail(&self) {
        if !self.news_rail.borrow().is_empty() {
            return;
        }
        self.spawn(move |c| {
            let res = c.news(1);
            move || Msg::NewsRail(res)
        });
    }

    /// İnceleme sayfası (1-indexli). İlk sayfada liste sıfırlanır,
    /// sonrakiler alta eklenir.
    fn fetch_reviews_page(&self, title_id: u64, page: u32) {
        let page = page.max(1);
        if self.rev_loading.get() {
            return;
        }
        self.rev_loading.set(true);
        *self.rev_error.borrow_mut() = None;
        if matches!(self.page_history.borrow().last(), Some(Page::Reviews { .. })) {
            self.show_page(&Page::Reviews {
                title_id,
                title_name: self.rev_name.borrow().clone(),
            });
        }
        self.spawn(move |c| {
            let res = c.reviews(title_id, page);
            move || Msg::RevPage(title_id, page, res)
        });
    }

    /// Ana sayfadaki son eklenen bölümler şeridi (sayfa 1, sessiz).
    fn fetch_last_rail(&self) {
        if !self.last_rail.borrow().is_empty() {
            return;
        }
        self.spawn(move |c| {
            let res = c.last_episodes(1);
            move || Msg::LastRail(res)
        });
    }

    /// İncelemeler sayfasını aç (geri tuşu detaya döner).
    pub fn open_reviews(&self, title_id: u64, title_name: &str) {
        let page = Page::Reviews {
            title_id,
            title_name: title_name.to_string(),
        };
        let mut st = self.page_history.borrow_mut();
        if st.last() != Some(&page) {
            st.push(page.clone());
        }
        drop(st);
        self.show_page(&page);
    }

    /// Hakkında penceresi (hamburger menüden).
    pub fn show_about(&self) {
        let w = adw::AboutWindow::builder()
            .transient_for(&self.window)
            .application_name("AnimeciX")
            .version(env!("CARGO_PKG_VERSION"))
            .comments("animecix.tv masaüstü istemcisi")
            .website("https://animecix.tv")
            .build();
        w.present();
    }

    /// Ortalanmış popup arama (canlı sonuçlu).
    pub fn open_search_popup(&self) {
        let dlg = gtk::Window::builder()
            .transient_for(&self.window)
            .modal(true)
            .title("Ara")
            .default_width(580)
            .build();

        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);

        let bar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        bar.set_margin_top(12);
        bar.set_margin_bottom(8);
        bar.set_margin_start(12);
        bar.set_margin_end(12);
        let entry = gtk::SearchEntry::new();
        entry.set_placeholder_text(Some("Anime ara…"));
        entry.set_hexpand(true);
        bar.append(&entry);
        root.append(&bar);

        let scroll = gtk::ScrolledWindow::new();
        scroll.set_hexpand(true);
        scroll.set_vexpand(true);
        scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        scroll.set_min_content_height(0);
        scroll.set_visible(false);
        let results = gtk::Box::new(gtk::Orientation::Vertical, 6);
        results.set_margin_start(12);
        results.set_margin_end(12);
        results.set_margin_bottom(12);
        scroll.set_child(Some(&results));
        root.append(&scroll);
        dlg.set_child(Some(&root));

        fn clear(box_: &gtk::Box) {
            let mut cur = box_.first_child();
            while let Some(child) = cur {
                let next = child.next_sibling();
                box_.remove(&child);
                cur = next;
            }
        }


        fn spinner(box_: &gtk::Box) {
            clear(box_);
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            row.set_halign(gtk::Align::Center);
            row.set_margin_top(24);
            let sp = gtk::Spinner::new();
            sp.start();
            let lbl = gtk::Label::new(Some("Aranıyor…"));
            lbl.add_css_class("dim-label");
            row.append(&sp);
            row.append(&lbl);
            box_.append(&row);
        }

        clear(&results);

        let dlg_w = dlg.downgrade();
        // Esc kapatır.
        {
            let d = dlg_w.clone();
            let kc = gtk::EventControllerKey::new();
            kc.connect_key_pressed(move |_, keyval, _, _| {
                if keyval.name().map(|s| s.to_string()).unwrap_or_default() == "Escape" {
                    if let Some(w) = d.upgrade() {
                        w.destroy();
                    }
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            });
            dlg.add_controller(kc);
        }

        let gen = Rc::new(Cell::new(0u32));
        let first_hit: Rc<RefCell<Option<Title>>> = Rc::new(RefCell::new(None));
        {
            let results_w = results.downgrade();
            let dlg_w = dlg_w.clone();
            let gen = gen.clone();
            let first_hit = first_hit.clone();
            let client = self.client.clone();
            let this = self.clone_ref();
            let scroll_w = scroll.downgrade();
            let dlg_q = dlg_w.clone();
            entry.connect_search_changed(move |e| {
                let q = e.text().to_string();
                let my = gen.get() + 1;
                gen.set(my);
                if q.trim().is_empty() {
                    if let (Some(b), Some(sc)) = (results_w.upgrade(), scroll_w.upgrade()) {
                        clear(&b);
                        sc.set_visible(false);
                    }
                    if let Some(w) = dlg_q.upgrade() {
                        w.set_default_size(580, -1);
                    }
                    *first_hit.borrow_mut() = None;
                    return;
                }
                if let (Some(b), Some(sc)) = (results_w.upgrade(), scroll_w.upgrade()) {
                    spinner(&b);
                    sc.set_min_content_height(0);
                    sc.set_visible(true);
                }
                let results_w2 = results_w.clone();
                let dlg_w2 = dlg_w.clone();
                let gen2 = gen.clone();
                let first_hit2 = first_hit.clone();
                let client2 = client.clone();
                let this2 = this.clone();
                let scroll_w2 = scroll_w.clone();
                glib::timeout_add_local_once(
                    std::time::Duration::from_millis(350),
                    move || {
                        if gen2.get() != my {
                            return;
                        }
                        let (tx, rx) = std::sync::mpsc::channel();
                        std::thread::spawn(move || {
                            let _ = tx.send(client2.search(&q));
                        });
                        let scroll_w3 = scroll_w2.clone();
                        glib::idle_add_local(move || match rx.try_recv() {
                            Ok(res) => {
                                if gen2.get() != my {
                                    return glib::ControlFlow::Break;
                                }
                                let (Some(box_), Some(_dlg)) =
                                    (results_w2.upgrade(), dlg_w2.upgrade())
                                else {
                                    return glib::ControlFlow::Break;
                                };
                                clear(&box_);
                                match res {
                                    Ok(list) => {
                                        let n = list.iter().take(12).count();
                                        if let Some(sc) = scroll_w3.upgrade() {
                                            sc.set_visible(true);
                                            sc.set_min_content_height((n as i32 * 88).clamp(90, 440));
                                        }
                                        let mut first: Option<Title> = None;
                                        for t in list.iter().take(12) {
                                            if first.is_none() {
                                                first = Some(t.clone());
                                            }
                                            let row = gtk::Box::new(
                                                gtk::Orientation::Horizontal,
                                                12,
                                            );
                                            row.add_css_class("history-item-card");
                                            row.set_margin_top(4);
                                            row.set_margin_bottom(4);
                                            row.set_margin_start(4);
                                            row.set_margin_end(4);
                                            let pic = this2.covers.cover_picture(
                                                t.poster.as_deref(),
                                                48,
                                                72,
                                            );
                                            pic.set_valign(gtk::Align::Center);
                                            row.append(&pic);
                                            let vb = gtk::Box::new(
                                                gtk::Orientation::Vertical,
                                                4,
                                            );
                                            vb.set_valign(gtk::Align::Center);
                                            vb.set_hexpand(true);
                                            let name = gtk::Label::new(Some(&t.name));
                                            name.add_css_class("title-4");
                                            name.set_xalign(0.0);
                                            name.set_wrap(false);
                                            name.set_single_line_mode(true);
                                            name.set_ellipsize(
                                                gtk::pango::EllipsizeMode::End,
                                            );
                                            vb.append(&name);
                                            let meta = gtk::Label::new(Some(
                                                &t.meta_line(),
                                            ));
                                            meta.add_css_class("dim-label");
                                            meta.set_xalign(0.0);
                                            vb.append(&meta);
                                            row.append(&vb);
                                            let tc = t.clone();
                                            let this3 = this2.clone();
                                            let dlg_w3 = dlg_w2.clone();
                                            let gesture = gtk::GestureClick::new();
                                            gesture.connect_pressed(move |_, _, _, _| {
                                                if let Some(w) = dlg_w3.upgrade() {
                                                    w.destroy();
                                                }
                                                this3.open_episodes(tc.clone());
                                            });
                                            row.add_controller(gesture);
                                            box_.append(&row);
                                        }
                                        *first_hit2.borrow_mut() = first;
                                        if box_.first_child().is_none() {
                                            let lbl = gtk::Label::new(Some(
                                                "Sonuç bulunamadı",
                                            ));
                                            lbl.add_css_class("dim-label");
                                            lbl.set_halign(gtk::Align::Center);
                                            lbl.set_margin_top(24);
                                            box_.append(&lbl);
                                        }
                                    }
                                    Err(err) => {
                                        if let Some(sc) = scroll_w3.upgrade() {
                                            sc.set_visible(true);
                                            sc.set_min_content_height(90);
                                        }
                                        let lbl = gtk::Label::new(Some(&format!(
                                            "Arama başarısız: {err}"
                                        )));
                                        lbl.add_css_class("dim-label");
                                        lbl.set_wrap(true);
                                        lbl.set_halign(gtk::Align::Center);
                                        lbl.set_margin_top(24);
                                        box_.append(&lbl);
                                    }
                                }
                                glib::ControlFlow::Break
                            }
                            Err(_) => glib::ControlFlow::Continue,
                        });
                    },
                );
            });
        }
        // Enter: ilk sonuca git.
        {
            let dlg_w = dlg_w.clone();
            let first_hit = first_hit.clone();
            let this = self.clone_ref();
            entry.connect_activate(move |_| {
                if let Some(t) = first_hit.borrow().clone() {
                    if let Some(w) = dlg_w.upgrade() {
                        w.destroy();
                    }
                    this.open_episodes(t);
                }
            });
        }

        dlg.present();
        entry.grab_focus();
    }

    /// Maraton hızlı ekle/çıkar (kart hover butonu). Dönen: true=eklendi.
    pub fn toggle_marathon_quick(&self, t: &Title) -> bool {
        let added = self.client.toggle_marathon(t);
        let toast = adw::Toast::new(if added {
            "🏆 Maratona eklendi"
        } else {
            "Maratondan çıkarıldı"
        });
        toast.set_timeout(2);
        self.toast.add_toast(toast);
        added
    }

    /// Haber detayını aç (geri tuşu listeye döner).
    pub fn open_news(&self, item: &NewsItem) {
        let page = Page::NewsDetail(item.clone());
        let mut st = self.page_history.borrow_mut();
        if st.last() != Some(&page) {
            st.push(page.clone());
        }
        drop(st);
        self.show_page(&page);
    }

    /// Keşfet filtresi (mevcut UI durumundan).
    fn dis_filter(&self, page: u32) -> api::DiscoverFilter {
        let t = self.dis_type.get();
        let o = self.dis_order.get();
        api::DiscoverFilter {
            genres: self.dis_genres.borrow().clone(),
            keywords: self.dis_keywords.borrow().clone(),
            title_type: match t {
                1 => Some("series".to_string()),
                2 => Some("movie".to_string()),
                _ => None,
            },
            order: api::DISCOVER_ORDERS
                .get(o as usize)
                .and_then(|(_, s)| *s)
                .map(str::to_string),
            only_streamable: self.dis_stream.get(),
            page,
        }
    }

    /// Keşfet sayfası (1-indexli). İlk sayfada liste sıfırlanır,
    /// sonrakiler alta eklenir. Bitince sayfa tazelenir.
    fn fetch_dis_page(&self, page: u32) {
        let page = page.max(1);
        if self.dis_loading.get() {
            return;
        }
        self.dis_loading.set(true);
        *self.dis_error.borrow_mut() = None;
        if page <= 1 {
            *self.dis_items.borrow_mut() = Vec::new();
            self.dis_total.set(0);
        }
        // Spinner hemen görünsün.
        if self.page_history.borrow().last() == Some(&Page::Kesfet) {
            self.show_page(&Page::Kesfet);
        }
        let filter = self.dis_filter(page);
        self.spawn(move |c| {
            let res = c.discover(&filter);
            move || Msg::DisPage(page, res)
        });
    }

    /// Filtre değişti: sıfırla + ilk sayfadan çek.
    fn dis_refetch(&self) {
        *self.dis_items.borrow_mut() = Vec::new();
        self.dis_total.set(0);
        self.dis_fetched.set(0);
        self.fetch_dis_page(1);
    }

    fn build_kesfet_view(&self) -> gtk::ScrolledWindow {
        let scroll = gtk::ScrolledWindow::new();
        scroll.set_hexpand(true);
        scroll.set_vexpand(true);
        let outer = gtk::Box::new(gtk::Orientation::Vertical, 12);
        outer.set_margin_top(12);
        outer.set_margin_bottom(18);
        outer.set_margin_start(12);
        outer.set_margin_end(12);

        // ---- filtre çubuğu ----
        let filt = gtk::Box::new(gtk::Orientation::Vertical, 8);
        let row1 = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let type_drop = gtk::DropDown::from_strings(&["Tümü", "Anime", "Film"]);
        type_drop.set_selected(self.dis_type.get());
        type_drop.set_tooltip_text(Some("Seri tipi"));
        {
            let this = self.clone_ref();
            type_drop.connect_selected_notify(move |dd| {
                this.dis_type.set(dd.selected());
                this.dis_refetch();
            });
        }
        row1.append(&type_drop);
        let order_labels: Vec<&str> = api::DISCOVER_ORDERS.iter().map(|(l, _)| *l).collect();
        let order_drop = gtk::DropDown::from_strings(&order_labels);
        order_drop.set_selected(self.dis_order.get());
        order_drop.set_tooltip_text(Some("Sıralama"));
        {
            let this = self.clone_ref();
            order_drop.connect_selected_notify(move |dd| {
                this.dis_order.set(dd.selected());
                this.dis_refetch();
            });
        }
        row1.append(&order_drop);
        let stream_chk = gtk::CheckButton::with_label("Sadece izlenebilenler");
        stream_chk.set_active(self.dis_stream.get());
        {
            let this = self.clone_ref();
            stream_chk.connect_toggled(move |c| {
                this.dis_stream.set(c.is_active());
                this.dis_refetch();
            });
        }
        row1.append(&stream_chk);
        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        row1.append(&spacer);
        let count_lbl = gtk::Label::new(Some(&format!("{} başlık", self.dis_total.get())));
        count_lbl.add_css_class("dim-label");
        count_lbl.set_valign(gtk::Align::Center);
        row1.append(&count_lbl);
        filt.append(&row1);
        // Filtrele düğmesi + popover (türler + anahtar sözcükler).
        // Seçimler anında uygulanır; sonuç gelince sayfa yenilenir.
        let nsel = self.dis_genres.borrow().len() + self.dis_keywords.borrow().len();
        let flabel = if nsel > 0 {
            format!("Filtrele ({nsel})")
        } else {
            "Filtrele".to_string()
        };
        let filter_btn = gtk::Button::builder()
            .icon_name("funnel-symbolic")
            .label(&flabel)
            .build();
        row1.prepend(&filter_btn);
        let pop = gtk::Popover::new();
        pop.set_parent(&filter_btn);
        let pop_scroll = gtk::ScrolledWindow::new();
        pop_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        pop_scroll.set_min_content_width(460);
        pop_scroll.set_min_content_height(440);
        pop_scroll.set_max_content_height(600);
        pop_scroll.set_propagate_natural_height(true);
        let pop_box = gtk::Box::new(gtk::Orientation::Vertical, 8);
        pop_box.set_margin_top(12);
        pop_box.set_margin_bottom(12);
        pop_box.set_margin_start(12);
        pop_box.set_margin_end(12);
        let ghead = gtk::Label::new(Some("Türler"));
        ghead.add_css_class("dim-label");
        ghead.set_xalign(0.0);
        pop_box.append(&ghead);
        let flow = gtk::Grid::new();
        flow.set_column_spacing(6);
        flow.set_row_spacing(6);
        {
            let sel = self.dis_genres.borrow().clone();
            for (i, (label, slug)) in api::DISCOVER_GENRES.iter().enumerate() {
                let b = gtk::CheckButton::with_label(label);
                b.add_css_class("filter-chip");
                b.set_hexpand(true);
                b.set_active(sel.contains(&slug.to_string()));
                let this = self.clone_ref();
                let slug = slug.to_string();
                b.connect_toggled(move |btn| {
                    let mut cur = this.dis_genres.borrow_mut();
                    if btn.is_active() {
                        if !cur.contains(&slug) {
                            cur.push(slug.clone());
                        }
                    } else if let Some(i) = cur.iter().position(|s| *s == slug) {
                        cur.remove(i);
                    }
                    drop(cur);
                    this.dis_refetch();
                });
                flow.attach(&b, (i % 4) as i32, (i / 4) as i32, 1, 1);
            }
        }
        pop_box.append(&flow);
        let khead = gtk::Label::new(Some("Anahtar sözcükler"));
        khead.add_css_class("dim-label");
        khead.set_xalign(0.0);
        pop_box.append(&khead);
        let kflow = gtk::Grid::new();
        kflow.set_column_spacing(6);
        kflow.set_row_spacing(6);
        {
            let sel = self.dis_keywords.borrow().clone();
            for (i, (label, slug)) in api::DISCOVER_KEYWORDS.iter().enumerate() {
                let b = gtk::CheckButton::with_label(label);
                b.add_css_class("filter-chip");
                b.set_hexpand(true);
                b.set_active(sel.contains(&slug.to_string()));
                let this = self.clone_ref();
                let slug = slug.to_string();
                b.connect_toggled(move |btn| {
                    let mut cur = this.dis_keywords.borrow_mut();
                    if btn.is_active() {
                        if !cur.contains(&slug) {
                            cur.push(slug.clone());
                        }
                    } else if let Some(i) = cur.iter().position(|s| *s == slug) {
                        cur.remove(i);
                    }
                    drop(cur);
                    this.dis_refetch();
                });
                kflow.attach(&b, (i % 4) as i32, (i / 4) as i32, 1, 1);
            }
        }
        pop_box.append(&kflow);
        let clear_btn = gtk::Button::with_label("Temizle");
        clear_btn.set_halign(gtk::Align::Start);
        {
            let this = self.clone_ref();
            clear_btn.connect_clicked(move |_| {
                this.dis_genres.borrow_mut().clear();
                this.dis_keywords.borrow_mut().clear();
                this.dis_refetch();
            });
        }
        pop_box.append(&clear_btn);
        pop_scroll.set_child(Some(&pop_box));
        pop.set_child(Some(&pop_scroll));
        {
            let pop_c = pop.clone();
            filter_btn.connect_clicked(move |_| {
                pop_c.popup();
            });
        }
        // Test kancası: ANIMECIX_POP_FILTER=1 ile açılışta otomatik açılır.
        if std::env::var_os("ANIMECIX_POP_FILTER").is_some() {
            let pop_a = pop.clone();
            glib::timeout_add_local_once(std::time::Duration::from_millis(1200), move || {
                pop_a.popup();
            });
        }
        // Seçili haplar (tek tıkla kaldır).
        let pill_flow = gtk::FlowBox::new();
        pill_flow.set_selection_mode(gtk::SelectionMode::None);
        pill_flow.set_column_spacing(6);
        pill_flow.set_row_spacing(6);
        for (label, slug) in api::DISCOVER_GENRES.iter().chain(api::DISCOVER_KEYWORDS.iter()) {
            let in_g = self.dis_genres.borrow().iter().any(|s| s.as_str() == *slug);
            let in_k = self.dis_keywords.borrow().iter().any(|s| s.as_str() == *slug);
            if in_g || in_k {
                let b = gtk::Button::with_label(&format!("{label} ×"));
                b.add_css_class("pill");
                let this = self.clone_ref();
                let slug = slug.to_string();
                b.connect_clicked(move |_| {
                    let mut g = this.dis_genres.borrow_mut();
                    if let Some(i) = g.iter().position(|s| s == &slug) {
                        g.remove(i);
                    }
                    drop(g);
                    let mut k = this.dis_keywords.borrow_mut();
                    if let Some(i) = k.iter().position(|s| s == &slug) {
                        k.remove(i);
                    }
                    drop(k);
                    this.dis_refetch();
                });
                pill_flow.append(&b);
            }
        }
        outer.append(&filt);
        outer.append(&pill_flow);

        // ---- sonuç bilgisi + ızgara ----
        let total = self.dis_total.get();
        let items = self.dis_items.borrow().clone();
        let loading = self.dis_loading.get();
        let error = self.dis_error.borrow().clone();
        if items.is_empty() && !loading && error.is_none() {
            let sp = components::create_status_page("Keşfet", "Tür seçerek kataloğa göz at.", "view-grid-symbolic");
            outer.append(&sp);
        } else {
            let mut cards = Vec::new();
            for t in &items {
                let this_o = self.clone_ref();
                cards.push(self.std_poster_card(
                    t,
                    t.genre_line().as_deref(),
                    true,
                    move |tt| this_o.open_episodes(tt),
                ));
            }
            outer.append(&Self::poster_grid(cards, self.grid_cols.get(), true));
        }
        if loading {
            let spin_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            spin_row.set_halign(gtk::Align::Center);
            spin_row.set_margin_top(12);
            let sp = gtk::Spinner::new();
            sp.start();
            let lbl = gtk::Label::new(Some("Yükleniyor…"));
            lbl.add_css_class("dim-label");
            spin_row.append(&sp);
            spin_row.append(&lbl);
            outer.append(&spin_row);
        }
        match error {
            Some(e) => {
                let err = gtk::Label::new(Some(&format!("Keşfet alınamadı: {e}")));
                err.add_css_class("error");
                err.set_wrap(true);
                err.set_halign(gtk::Align::Center);
                outer.append(&err);
                let retry = gtk::Button::with_label("Tekrar Dene");
                retry.set_halign(gtk::Align::Center);
                let this_r = self.clone_ref();
                retry.connect_clicked(move |_| {
                    this_r.dis_refetch();
                });
                outer.append(&retry);
            }
            None => {
                if !loading && !items.is_empty() && items.len() < total {
                    let more = gtk::Button::with_label("Daha Fazla");
                    more.set_halign(gtk::Align::Center);
                    let this_m = self.clone_ref();
                    more.connect_clicked(move |_| {
                        let p = this_m.dis_fetched.get() + 1;
                        this_m.fetch_dis_page(p.max(1));
                    });
                    outer.append(&more);
                }
            }
        }
        scroll.set_child(Some(&outer));
        scroll
    }

    fn build_history_view(&self) -> gtk::ScrolledWindow {
        let scroll = gtk::ScrolledWindow::new();
        scroll.set_hexpand(true);
        scroll.set_vexpand(true);
        scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        self.fetch_server_history(true);
        // Geçmiş sayfası ilk sayfayı ister (sonrakiler "Daha Fazla" ile).
        // Hata varken otomatik deneme (sonsuz döngü olmasın).
        if self.client.is_logged_in()
            && !self.hist_loading.get()
            && self.hist_error.borrow().is_none()
            && self.hist_items.borrow().is_empty()
        {
            self.fetch_hist_page(self.hist_fetched.get());
        }
        let history = self.client.load_state().history;
        let history = if self.settings.borrow().local_history_enabled {
            history
        } else {
            Vec::new()
        };
        let server = self.hist_items.borrow().clone();
        let server_loading = self.hist_loading.get();
        let total = self.hist_total.get();
        let server_error = self.hist_error.borrow().clone();
        if history.is_empty() && server.is_empty() && !server_loading && server_error.is_none() {
            let sp = components::create_status_page(
                "İzleme Geçmişi Boş",
                "İzlediğiniz bölümler burada görünecek.",
                "avatar-default-symbolic",
            );
            scroll.set_child(Some(&sp));
            return scroll;
        }

        let this_del = self.clone_ref();
        let this_clr = self.clone_ref();
        let this_open = self.clone_ref();
        let this_cov = self.clone_ref();
        let this_more = self.clone_ref();
        let this_retry = self.clone_ref();
        let view = views::HistoryView::build(
            &self.client,
            &history,
            &server,
            server_loading,
            total,
            self.grid_cols.get(),
            move |ids| {
                this_del.client.remove_history_items(&ids);
                this_del.show_page(&Page::History);
            },
            move || {
                this_clr.client.clear_history();
                this_clr.show_page(&Page::History);
            },
            move |h| {
                this_open.open_episodes(h.title.clone());
            },
            {
                let this_card = self.clone_ref();
                move |t: &Title, sub: &str| {
                    let this_o = this_card.clone();
                    this_card.std_poster_card(t, Some(sub), true, move |tt| {
                        this_o.open_episodes(tt);
                    })
                }
            },
            move || {
                this_more.fetch_hist_page(this_more.hist_fetched.get());
            },
            server_error,
            move || {
                let p = this_retry.hist_fetched.get();
                this_retry.fetch_hist_page(p);
            },
            move |url, pic, w, h| {
                this_cov.covers.load_cover(url, pic, w, h);
            },
        );

        scroll.set_child(Some(&view));
        scroll
    }
    fn effective_download_dir(&self) -> std::path::PathBuf {
        let d = self
            .settings
            .borrow()
            .download_dir
            .clone()
            .map(std::path::PathBuf::from)
            .unwrap_or_else(crate::download::default_download_dir);
        let _ = std::fs::create_dir_all(&d);
        d
    }
    fn build_downloads_view(&self) -> gtk::ScrolledWindow {
        let (scroll, rows) = crate::ui::downloads_view::DownloadsView::build(
            &self.dl_manager,
            self.effective_download_dir(),
        );
        *self.dl_rows.borrow_mut() = rows;
        scroll
    }
    /// Kalite sorusu (tekli: her indirmede; toplu: grup başı bir kez).
    fn ask_download_quality(&self, cb: impl Fn(Option<String>) + 'static) {
        let dialog = adw::MessageDialog::builder()
            .heading("İndirme Kalitesi")
            .body("Bu indirme için hangi kalite kullanılsın?")
            .close_response("cancel")
            .default_response("best")
            .build();
        dialog.set_transient_for(Some(&self.window));
        dialog.add_response("cancel", "İptal");
        dialog.add_response("best", "En iyi");
        dialog.add_response("1080p", "1080p");
        dialog.add_response("720p", "720p");
        dialog.add_response("480p", "480p");
        dialog.set_response_appearance("best", adw::ResponseAppearance::Suggested);
        dialog.connect_response(None, move |_, resp| match resp {
            "best" | "1080p" | "720p" | "480p" => cb(Some(resp.to_string())),
            _ => cb(None),
        });
        dialog.present();
    }
    /// Bölüm listesinin çevirmenlerini worker'da önden çeker.
    fn start_download_prefetch(&self, title: Title, eps: Vec<Episode>, quality: String, is_single: bool) {
        self.busy(true);
        let title_c = title.clone();
        self.spawn(move |c| {
            let mut items = Vec::new();
            for ep in &eps {
                let fansubs = c.list_fansubs(title_c.id, ep.episode, ep.season).unwrap_or_default();
                items.push((ep.clone(), fansubs));
            }
            move || Msg::DlLists { title: title_c, quality, items, is_single }
        });
    }
    fn build_settings_view(&self) -> gtk::ScrolledWindow {
        let scroll = gtk::ScrolledWindow::new();
        let settings = self.settings.borrow();
        let this_save = self.clone_ref();
        let this_wipe = self.clone_ref();

        let last_save: Rc<RefCell<std::time::Instant>> = Rc::new(RefCell::new(std::time::Instant::now()));
        let last_save_c = last_save.clone();
        let view = views::SettingsView::build(
            &settings,
            move |new_s| {
                *this_save.settings.borrow_mut() = new_s.clone();
                this_save.client.save_settings(&new_s);
                this_save.apply_ui_scale();
                let now = std::time::Instant::now();
                let elapsed = now.duration_since(*last_save_c.borrow()).as_millis();
                *last_save_c.borrow_mut() = now;
                if elapsed >= 400 {
                    let toast = adw::Toast::new("Ayarlar kaydedildi");
                    toast.set_timeout(2);
                    this_save.toast.add_toast(toast);
                }
            },
            move |remove_app| {
                this_wipe.client.wipe_all_data();
                if remove_app {
                    crate::uninstall_application();
                    std::process::exit(0);
                } else {
                    let toast = adw::Toast::new("Tüm veriler temizlendi ve sıfırlandı!");
                    toast.set_timeout(3);
                    this_wipe.toast.add_toast(toast);
                    this_wipe.page_history.borrow_mut().clear();
                    this_wipe.page_history.borrow_mut().push(Page::Welcome);
                    this_wipe.show_page(&Page::Welcome);
                }
            },
        );

        scroll.set_child(Some(&view));
        scroll
    }

    fn build_account_view(&self) -> gtk::ScrolledWindow {
        let scroll = gtk::ScrolledWindow::new();
        scroll.set_hexpand(true);
        scroll.set_vexpand(true);
        scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        let root = gtk::Box::new(gtk::Orientation::Vertical, 12);
        root.set_margin_top(24);
        root.set_margin_bottom(12);
        root.set_margin_start(24);
        root.set_margin_end(24);
        root.set_halign(gtk::Align::Center);
        root.set_valign(gtk::Align::Start);
        scroll.set_child(Some(&root));
        if let Some(u) = self.client.session_user() {
            // ---- girişli ----
            let avatar = gtk::Image::from_icon_name("avatar-default-symbolic");
            avatar.set_pixel_size(72);
            avatar.set_halign(gtk::Align::Center);
            let name = gtk::Label::new(Some(&u.name));
            name.add_css_class("title-2");
            name.set_xalign(0.5);
            let mail = gtk::Label::new(Some(&u.email));
            mail.add_css_class("dim-label");
            mail.set_xalign(0.5);
            let out = gtk::Button::with_label("Çıkış Yap");
            out.set_halign(gtk::Align::Center);
            let this = self.clone_ref();
            out.connect_clicked(move |_| {
                this.client.logout();
                *this.server_history.borrow_mut() = Vec::new();
                this.server_history_loaded.set(false);
                *this.hist_items.borrow_mut() = Vec::new();
                this.hist_total.set(0);
                this.hist_fetched.set(0);
                this.hist_loading.set(false);
                let t = adw::Toast::new("Çıkış yapıldı (misafir modu)");
                t.set_timeout(3);
                this.toast.add_toast(t);
                this.show_page(&Page::Account);
            });
            root.append(&avatar);
            root.append(&name);
            root.append(&mail);
            // İstatistikler (yerel veri + sunucu takip listesi).
            let st = self.client.load_state();
            let group = adw::PreferencesGroup::new();
            group.set_title("İstatistikler");
            let stat_row = |title: &str, sub: &str| {
                let r = adw::ActionRow::new();
                r.set_title(title);
                r.set_subtitle(sub);
                r
            };
            let r_hist = stat_row("İzlenen bölüm", &format!("{} kayıt", st.history.len()));
            r_hist.set_activatable(true);
            {
                let this = self.clone_ref();
                r_hist.connect_activated(move |_| {
                    let mut h = this.page_history.borrow_mut();
                    if h.last() != Some(&Page::History) {
                        h.push(Page::History);
                    }
                    drop(h);
                    this.show_page(&Page::History);
                });
            }
            group.add(&r_hist);
            let r_fav = stat_row("Favori", &format!("{} başlık", st.saved.len()));
            r_fav.set_activatable(true);
            {
                let this = self.clone_ref();
                r_fav.connect_activated(move |_| {
                    let mut h = this.page_history.borrow_mut();
                    if h.last() != Some(&Page::Favs) {
                        h.push(Page::Favs);
                    }
                    drop(h);
                    this.show_page(&Page::Favs);
                });
            }
            group.add(&r_fav);
            let r_mar = stat_row("Maraton", &format!("{} başlık", st.marathon.len()));
            r_mar.set_activatable(true);
            {
                let this = self.clone_ref();
                r_mar.connect_activated(move |_| {
                    let mut h = this.page_history.borrow_mut();
                    if h.last() != Some(&Page::Marathon) {
                        h.push(Page::Marathon);
                    }
                    drop(h);
                    this.show_page(&Page::Marathon);
                });
            }
            group.add(&r_mar);
            let r_watch = stat_row(
                "Takip listesi",
                &format!("{} başlık (sitedeki izleme listen)", self.server_history.borrow().len()),
            );
            group.add(&r_watch);
            root.append(&group);
            // Favoriler ızgarası (en fazla 10).
            if st.saved.is_empty() {
                let dim = gtk::Label::new(Some("Henüz favori eklemedin."));
                dim.add_css_class("dim-label");
                dim.set_xalign(0.5);
                root.append(&dim);
            } else {
                let mut cards = Vec::new();
                for t in st.saved.iter().take(10) {
                    let this_o = self.clone_ref();
                    cards.push(self.std_poster_card(
                        t,
                        t.genre_line().as_deref(),
                        false,
                        move |tt| this_o.open_episodes(tt),
                    ));
                }
                let grid = Self::poster_grid(cards, 5, true);
                grid.set_halign(gtk::Align::Center);
                root.append(&grid);
            }
            root.append(&out);
        } else {
            // ---- girişsiz ----
            let title = gtk::Label::new(Some("Hesaba Giriş"));
            title.add_css_class("title-2");
            title.set_xalign(0.5);
            let sub = gtk::Label::new(Some("animecix.tv hesabınla giriş yap"));
            sub.add_css_class("dim-label");
            sub.set_xalign(0.5);
            let mail = gtk::Entry::new();
            mail.set_placeholder_text(Some("E-posta"));
            mail.set_input_purpose(gtk::InputPurpose::Email);
            mail.set_width_request(320);
            let pass = gtk::PasswordEntry::new();
            pass.set_placeholder_text(Some("Şifre"));
            pass.set_show_peek_icon(true);
            let login_btn = gtk::Button::with_label("Giriş Yap");
            login_btn.add_css_class("suggested-action");
            login_btn.add_css_class("pill");
            login_btn.set_halign(gtk::Align::Center);
            let err_lbl = gtk::Label::new(None);
            err_lbl.add_css_class("error");
            err_lbl.set_xalign(0.5);
            err_lbl.set_wrap(true);
            err_lbl.set_visible(false);
            let hint = gtk::Label::new(Some("Hesabın yoksa animecix.tv'de oluştur."));
            hint.add_css_class("dim-label");
            hint.add_css_class("caption");
            hint.set_xalign(0.5);
            {
                let this = self.clone_ref();
                let mail_c = mail.clone();
                let pass_c = pass.clone();
                let err_c = err_lbl.clone();
                let do_login = Rc::new(move || {
                    let email = mail_c.text().trim().to_string();
                    let password = pass_c.text().to_string();
                    if email.is_empty() || password.is_empty() {
                        err_c.set_text("E-posta ve şifre gerekli");
                        err_c.set_visible(true);
                        return;
                    }
                    err_c.set_visible(false);
                    this.busy(true);
                    this.spawn(move |c| {
                        // Giriş + keşif + favori birleştirme.
                        let res = c.login(&email, &password).map(|u| {
                            c.probe_authed();
                            let n = c.merge_server_favs();
                            (u, n)
                        });
                        move || Msg::Login(res)
                    });
                });
                let dl_c = do_login.clone();
                login_btn.connect_clicked(move |_| dl_c());
                let dl_c2 = do_login;
                pass.connect_activate(move |_| dl_c2());
            }
            root.append(&title);
            root.append(&sub);
            root.append(&mail);
            root.append(&pass);
            root.append(&login_btn);
            root.append(&err_lbl);
            root.append(&hint);
        }
        scroll
    }

    fn build_search_view(&self) -> gtk::ScrolledWindow {
        let scroll = gtk::ScrolledWindow::new();
        let results = self.search_results.borrow();

        if results.is_empty() {
            let sp = components::create_status_page(
                "Sonuç Bulunamadı",
                "Arama sorgunuza uygun anime, dizi veya film bulunamadı.",
                "system-search-symbolic",
            );
            scroll.set_child(Some(&sp));
            return scroll;
        }

        let outer = gtk::Box::new(gtk::Orientation::Vertical, 4);
        outer.set_margin_top(12);
        outer.set_margin_bottom(18);
        outer.set_margin_start(12);
        outer.set_margin_end(12);
        outer.set_hexpand(true);
        outer.set_halign(gtk::Align::Fill);

        let mut cards = Vec::new();
        for t in results.iter() {
            let sub = t.meta_line();
            let this_open = self.clone_ref();
            cards.push(self.std_poster_card(t, Some(&sub), true, move |tt| {
                this_open.open_episodes(tt);
            }));
        }
        outer.append(&Self::poster_grid(cards, self.grid_cols.get(), false));

        scroll.set_child(Some(&outer));
        scroll
    }

    fn build_episodes_view(&self, title: &Title, eps: &[Episode]) -> gtk::ScrolledWindow {
        let scroll = gtk::ScrolledWindow::new();

        // Komşu bölüm butonları için listeyi sakla.
        self.cur_eps_title.set(title.id);
        *self.cur_eps.borrow_mut() = eps.to_vec();

        let is_movie = title.title_type.as_deref() == Some("movie")
            || (eps.len() <= 1 && eps.first().map(|e| e.name.contains("Filmi")).unwrap_or(false));

        if is_movie {
            let header_poster = self.covers.cover_picture(title.poster.as_deref(), 220, 330);
            let bookmark_btn = components::bookmark_button(&self.client, title);
            let this_bm = self.clone_ref();
            let t_clone = title.clone();
            bookmark_btn.connect_clicked(move |b| {
                let saved = this_bm.client.toggle_saved(&t_clone);
                b.set_icon_name(if saved { "starred-symbolic" } else { "non-starred-symbolic" });
                b.set_tooltip_text(Some(if saved { "Favorilerden Çıkar" } else { "Favorilere Ekle" }));
                this_bm.sync_fav_remote(t_clone.id, saved);
            });

            let marathon_btn = components::marathon_button(&self.client, title);
            let this_mar = self.clone_ref();
            let t_clone_mar = title.clone();
            marathon_btn.connect_clicked(move |b| {
                let added = this_mar.client.toggle_marathon(&t_clone_mar);
                b.set_icon_name(if added { "media-playlist-repeat-symbolic" } else { "media-playlist-consecutive-symbolic" });
                b.set_tooltip_text(Some(if added { "Maratondan Çıkar" } else { "İzleme Maratonuna Ekle" }));
                let msg = if added { "🏆 İzleme Maratonuna eklendi!" } else { "İzleme Maratonundan çıkarıldı" };
                let toast = adw::Toast::new(msg);
                toast.set_timeout(2);
                this_mar.toast.add_toast(toast);
            });

            let this_play = self.clone_ref();
            let title_c = title.clone();
            let ep_c = eps.first().cloned().unwrap_or(Episode {
                episode: 1,
                season: 1,
                name: title.name.clone(),
                thumbnail: None,
            });
            let movie_progress = self.client.get_progress(title.id, 1, 1);
            let (movie_view, movie_pb, movie_lbl) = episodes_view::create_movie_detail_view(
                title,
                &header_poster,
                &bookmark_btn,
                &marathon_btn,
                movie_progress,
                move || {
                    this_play.play(&title_c, &ep_c);
                },
            );
            let prog_key = format!("{}:1:1", title.id);
            self.progress_bars.borrow_mut().insert(prog_key, (movie_pb, movie_lbl));
            movie_view.add_css_class("movie-tint");
            self.apply_movie_tint(&movie_view, title.poster.as_deref());
            if self.det_title.get() == title.id {
                let related = self.det_related.borrow().clone();
                if !related.is_empty() {
                    movie_view.append(&self.build_related_section(&related));
                }
            }
            scroll.set_child(Some(&movie_view));
            return scroll;
        }

        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);

        let header_poster = self.covers.cover_picture(title.poster.as_deref(), 120, 180);
        let bookmark_btn = components::bookmark_button(&self.client, title);
        let this_bm = self.clone_ref();
        let t_clone = title.clone();
        bookmark_btn.connect_clicked(move |b| {
            let saved = this_bm.client.toggle_saved(&t_clone);
            b.set_icon_name(if saved { "starred-symbolic" } else { "non-starred-symbolic" });
            b.set_tooltip_text(Some(if saved { "Favorilerden Çıkar" } else { "Favorilere Ekle" }));
        });

        let marathon_btn = components::marathon_button(&self.client, title);
        let this_mar = self.clone_ref();
        let t_clone_mar = title.clone();
        marathon_btn.connect_clicked(move |b| {
            let added = this_mar.client.toggle_marathon(&t_clone_mar);
            b.set_icon_name(if added { "media-playlist-repeat-symbolic" } else { "flag-symbolic" });
            b.set_tooltip_text(Some(if added { "Maratondan Çıkar" } else { "İzleme Maratonuna Ekle" }));
            let msg = if added { "🏆 İzleme Maratonuna eklendi!" } else { "İzleme Maratonundan çıkarıldı" };
            let toast = adw::Toast::new(msg);
            toast.set_timeout(2);
            this_mar.toast.add_toast(toast);
        });

        // Toplu indirme modu butonu (başlıkta kullanılır).
        let dl_mode_btn = gtk::Button::from_icon_name("folder-download-symbolic");
        dl_mode_btn.add_css_class("flat");
        dl_mode_btn.add_css_class("circular");
        dl_mode_btn.add_css_class("lg-icon");
        dl_mode_btn.set_valign(gtk::Align::Center);
        dl_mode_btn.set_tooltip_text(Some("Toplu İndirme Modu"));

        let detail_header = episodes_view::create_title_detail_header(title, &header_poster, &bookmark_btn, &marathon_btn, &dl_mode_btn);
        root.append(&detail_header);

        // Diskteki güncel ilerlemeyi al (oynatıcı yazmış olabilir).
        *self.progress.borrow_mut() = self.client.load_state().progress;

        let settings = self.settings.borrow();

        if settings.quick_search_enabled && !self.client.is_quick_search_tip_seen() {
            let this_tip = self.clone_ref();
            let tip_banner = episodes_view::create_quick_search_tip_banner(
                &settings.quick_search_shortcut,
                move || {
                    this_tip.client.set_quick_search_tip_seen(true);
                },
            );
            root.append(&tip_banner);
        }

        if !self.client.is_right_click_tip_seen() {
            let this_tip2 = self.clone_ref();
            let right_click_tip = episodes_view::create_right_click_tip_banner(move || {
                this_tip2.client.set_right_click_tip_seen(true);
            });
            root.append(&right_click_tip);
        }

        let ep_search_entry = gtk::SearchEntry::new();
        if settings.quick_search_enabled {
            let search_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            search_box.set_margin_start(12);
            search_box.set_margin_end(12);
            search_box.set_margin_bottom(8);

            ep_search_entry.set_placeholder_text(Some(&format!(
                "Bölüm numarası veya adı ara… ({})",
                settings.quick_search_shortcut
            )));
            ep_search_entry.set_hexpand(true);
            search_box.append(&ep_search_entry);
            root.append(&search_box);

            let shortcut_key = settings.quick_search_shortcut.clone();
            let ep_entry_clone = ep_search_entry.clone();
            let key_controller = gtk::EventControllerKey::new();
            key_controller.connect_key_pressed(move |_, keyval, _, state| {
                let key_name = keyval.name().map(|s| s.to_string()).unwrap_or_default();
                let is_ctrl = state.contains(gtk::gdk::ModifierType::CONTROL_MASK);

                let triggered = match shortcut_key.as_str() {
                    "Ctrl+F" => is_ctrl && (key_name == "f" || key_name == "F"),
                    "Ctrl+K" => is_ctrl && (key_name == "k" || key_name == "K"),
                    "F3" => key_name == "F3",
                    _ => key_name == "slash" || key_name == "kp_divide",
                };

                if triggered {
                    ep_entry_clone.grab_focus();
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            });
            self.window.add_controller(key_controller);
        }
        drop(settings);

        let list_box = gtk::ListBox::new();
        list_box.add_css_class("content-list");
        list_box.set_margin_start(12);
        list_box.set_margin_end(12);
        list_box.set_margin_bottom(16);

        if eps.is_empty() {
            let sp = components::create_status_page(
                "Bölüm Bulunamadı",
                "Bu yapım için henüz bölüm listesi bulunmuyor.",
                "media-tape-symbolic",
            );
            root.append(&sp);
        } else {
            let rows: Vec<(Episode, gtk::Box)> = eps.iter().map(|e| {
                let key = format!("{}:{}:{}", title.id, e.season, e.episode);

                let name = gtk::Label::new(Some(&format!(
                    "S{:02} E{:02}   {}",
                    e.season, e.episode, e.name
                )));
                name.set_xalign(0.0);
                name.add_css_class("title-4");
                name.set_hexpand(true);

                let time_lbl = gtk::Label::new(None);
                time_lbl.set_xalign(1.0);
                time_lbl.add_css_class("dim-label");

                let header_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
                header_box.append(&name);
                header_box.append(&time_lbl);

                let pic = if let Some(thumb) = e.thumbnail.as_deref() {
                    // Bölüm kapağı (stili, 16:9): API thumbnail'i.
                    self.covers.cover_picture(Some(thumb), 96, 54)
                } else {
                    self.covers.cover_picture(title.poster.as_deref(), 48, 72)
                };
                pic.set_valign(gtk::Align::Center);

                let right_col = gtk::Box::new(gtk::Orientation::Vertical, 4);
                right_col.set_valign(gtk::Align::Center);
                right_col.append(&header_box);

                let (lpos, ldur) = self.progress.borrow()
                    .get(&key).copied()
                    .unwrap_or((0.0, 0.0));
                // Sitedeki konum da varsa büyüğünü göster (süre yerelden,
                // yoksa bölüm süresinden tahmin edilir).
                let rpos = self.remote_progress.borrow()
                    .get(&key).map(|(p, _)| *p)
                    .unwrap_or(0.0);
                let saved_pos = lpos.max(rpos);
                let saved_dur = if ldur > 0.0 {
                    ldur
                } else {
                    title.runtime.unwrap_or(0).max(0) as f64 * 60.0
                };

                let progress_bar = gtk::ProgressBar::new();
                progress_bar.add_css_class("episode-progress");

                let fmt_time = |s: f64| -> String {
                    let s = s as u64;
                    if s >= 3600 { format!("{}:{:02}:{:02}", s/3600, (s%3600)/60, s%60) }
                    else { format!("{}:{:02}", s/60, s%60) }
                };

                if saved_dur > 0.0 && saved_pos > 1.0 {
                    progress_bar.set_fraction((saved_pos / saved_dur).clamp(0.0, 1.0));
                    progress_bar.set_visible(true);
                    time_lbl.set_text(&format!("{} / {}", fmt_time(saved_pos), fmt_time(saved_dur)));
                    time_lbl.set_visible(true);
                } else {
                    progress_bar.set_visible(false);
                    time_lbl.set_visible(false);
                }
                right_col.append(&progress_bar);

                self.progress_bars.borrow_mut().insert(key.clone(), (progress_bar, time_lbl));

                let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
                row.set_margin_top(6);
                row.set_margin_bottom(6);
                row.set_margin_start(14);
                row.set_margin_end(14);
                row.set_valign(gtk::Align::Center);
                row.append(&pic);
                row.append(&right_col);

                let is_watched = Rc::new(RefCell::new(
                    self.client.is_watched(title.id, e.season, e.episode)
                    || (saved_dur > 0.0 && saved_pos / saved_dur > 0.9)
                ));

                let done_icon = gtk::Image::from_icon_name("object-select-symbolic");
                done_icon.add_css_class("dim-label");
                done_icon.set_tooltip_text(Some("İzlendi"));
                done_icon.set_valign(gtk::Align::Center);
                done_icon.set_visible(*is_watched.borrow());
                row.append(&done_icon);

                let this_play = self.clone_ref();
                let title_play = title.clone();
                let ep_play = e.clone();
                let click = gtk::GestureClick::new();
                click.set_button(1); // sadece sol tık
                click.connect_pressed(move |_, _, _, _| {
                    this_play.play(&title_play, &ep_play);
                });
                row.add_controller(click);

                let this_ctx = self.clone_ref();
                let title_ctx = title.clone();
                let ep_ctx = e.clone();
                let is_watched_ctx = is_watched.clone();
                let done_icon_ctx = done_icon.clone();
                let row_ctx = row.clone();

                let right_click = gtk::GestureClick::new();
                right_click.set_button(3);
                right_click.connect_pressed(move |gesture, _, x, y| {
                    gesture.set_state(gtk::EventSequenceState::Claimed);

                    let currently_watched = *is_watched_ctx.borrow();
                    let label = if currently_watched {
                        "✖ İzlenmedi Olarak İşaretle"
                    } else {
                        "✅ İzlendi Olarak İşaretle"
                    };

                    let menu_model = gio::Menu::new();
                    menu_model.append(Some(label), Some("row.toggle-watched"));

                    let client_c = this_ctx.client.clone();
                    let title_c = title_ctx.clone();
                    let ep_c = ep_ctx.clone();
                    let is_watched_c = is_watched_ctx.clone();
                    let done_icon_c = done_icon_ctx.clone();
                    let this_refresh = this_ctx.clone_ref();

                    let action_group = gio::SimpleActionGroup::new();
                    let action = gio::SimpleAction::new("toggle-watched", None);
                    action.connect_activate(move |_, _| {
                        let was_watched = *is_watched_c.borrow();
                        if was_watched {
                            client_c.remove_watched(title_c.id, ep_c.season, ep_c.episode);
                            *is_watched_c.borrow_mut() = false;
                            done_icon_c.set_visible(false);
                        } else {
                            let w = api::Watched {
                                title_id: title_c.id,
                                episode: ep_c.episode,
                                season: ep_c.season,
                            };
                            client_c.save_watched(&w, &title_c.name);
                            client_c.add_history(&title_c, &ep_c);
                            *is_watched_c.borrow_mut() = true;
                            done_icon_c.set_visible(true);
                        }
                        let msg = if was_watched {
                            "✖ İzlenmedi olarak işaretlendi"
                        } else {
                            "✅ İzlendi olarak işaretlendi"
                        };
                        let toast = adw::Toast::new(msg);
                        toast.set_timeout(2);
                        this_refresh.toast.add_toast(toast);
                    });
                    action_group.add_action(&action);
                    row_ctx.insert_action_group("row", Some(&action_group));

                    let popover = gtk::PopoverMenu::from_model(Some(&menu_model));
                    popover.set_parent(&row_ctx);
                    let rect = gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1);
                    popover.set_pointing_to(Some(&rect));
                    popover.set_has_arrow(true);
                    popover.popup();
                });
                row.add_controller(right_click);

                (e.clone(), row)
            }).collect();

            for (_, row_widget) in &rows {
                list_box.append(row_widget);
            }

            let rows_rc = Rc::new(rows);
            // Sezon sekmeleri (1'den fazlaysa): kaldığın sezona açıl.
            // Arama + sekme tek filtreden geçer.
            let mut seasons: Vec<u64> = eps.iter().map(|e| e.season).collect();
            seasons.sort_unstable();
            seasons.dedup();
            let default_season = self
                .resume_season(title.id)
                .filter(|s| seasons.contains(s))
                .or_else(|| seasons.first().copied())
                .unwrap_or(1);
            let sel_season = Rc::new(Cell::new(default_season));
            let apply_filter: Rc<dyn Fn()> = {
                let rows_c = rows_rc.clone();
                let sel_c = sel_season.clone();
                let entry_c = ep_search_entry.clone();
                Rc::new(move || {
                    let query = entry_c.text().trim().to_lowercase();
                    let sel = sel_c.get();
                    for (ep_data, row_widget) in rows_c.iter() {
                        let visible = if ep_data.season != sel {
                            false
                        } else if query.is_empty() {                            true
                        } else {
                            let name_match = ep_data.name.to_lowercase().contains(&query);
                            let ep_num_match = ep_data.episode.to_string() == query
                                || format!("e{}", ep_data.episode) == query
                                || format!("s{:02}e{:02}", ep_data.season, ep_data.episode)
                                    == query;
                            name_match || ep_num_match
                        };
                        row_widget.set_visible(visible);
                        // ListBox satır sarmalayıcısını da gizle (boş satır
                        // yüksekliği kalmasın).
                        if let Some(par) = row_widget.parent() {
                            par.set_visible(visible);
                        }
                    }
                })
            };
            {
                let a = apply_filter.clone();
                ep_search_entry.connect_search_changed(move |_| a());
            }
            if seasons.len() > 1 {
                let tab_bar = gtk::Box::new(gtk::Orientation::Horizontal, 6);
                tab_bar.set_margin_start(12);
                tab_bar.set_margin_end(12);
                tab_bar.set_margin_bottom(8);
                let season_lbl = gtk::Label::new(Some("Sezon:"));
                season_lbl.add_css_class("dim-label");
                season_lbl.set_valign(gtk::Align::Center);
                tab_bar.append(&season_lbl);
                let tab_btns: Rc<RefCell<Vec<(u64, gtk::Button)>>> =
                    Rc::new(RefCell::new(Vec::new()));
                for s in &seasons {
                    let b = gtk::Button::with_label(&format!("{s}"));
                    b.add_css_class("pill");
                    b.add_css_class("season-tab");
                    if *s == default_season {
                        b.add_css_class("suggested-action");
                    }
                    let sel_c = sel_season.clone();
                    let a = apply_filter.clone();
                    let btns_c = tab_btns.clone();
                    let ss = *s;
                    b.connect_clicked(move |_| {
                        sel_c.set(ss);
                        for (bs, bb) in btns_c.borrow().iter() {
                            if *bs == ss {
                                bb.add_css_class("suggested-action");
                            } else {
                                bb.remove_css_class("suggested-action");
                            }
                        }
                        a();
                    });
                    tab_btns.borrow_mut().push((*s, b.clone()));
                    tab_bar.append(&b);
                }
                root.append(&tab_bar);
            }
            apply_filter();

            root.append(&list_box);
        }

        // Site detay bölümleri: benzerler + künye + incelemeler önizleme.
        // Veri geldikçe (DetData) sayfa yeniden kurulur.
        if self.det_title.get() == title.id {
            let related = self.det_related.borrow().clone();
            if !related.is_empty() {
                root.append(&self.build_related_section(&related));
            }
            let credits = self.det_credits.borrow().clone();
            if !credits.is_empty() {
                let this_cov = self.clone_ref();
                root.append(&info_views::credits_strip(
                    &credits,
                    move |poster, pic, w, h| {
                        this_cov.covers.load_cover(poster, &pic, w, h);
                    },
                ));
            }
            let reviews = self.det_reviews.borrow().clone();
            let rev_total = self.det_rev_total.get();
            if !reviews.is_empty() || rev_total > 0 {
                let this_all = self.clone_ref();
                let this_cov = self.clone_ref();
                let tid = title.id;
                let tname = title.name.clone();
                root.append(&info_views::reviews_preview(
                    &reviews,
                    rev_total,
                    move |poster, pic, w, h| {
                        this_cov.covers.load_cover(poster, &pic, w, h);
                    },
                    move || {
                        this_all.open_reviews(tid, &tname);
                    },
                ));
            }
        }

        scroll.set_child(Some(&root));
        scroll
    }

    fn spawn<F, R>(&self, f: F)
    where
        F: FnOnce(Arc<Client>) -> R + Send + 'static,
        R: FnOnce() -> Msg + Send + 'static,
    {
        let c = self.client.clone();
        let (tx, rx) = std::sync::mpsc::channel::<Msg>();
        std::thread::spawn(move || {
            let res_fn = f(c);
            let _ = tx.send(res_fn());
        });
        let this = self.clone_ref();
        glib::idle_add_local(move || match rx.try_recv() {
            Ok(msg) => {
                this.busy(false);
                this.handle_msg(msg);
                glib::ControlFlow::Break
            }
            Err(_) => glib::ControlFlow::Continue,
        });
    }

    fn handle_msg(&self, msg: Msg) {
        match msg {
            Msg::Cats(res) => match res {
                Ok(cats) => {
                    *self.cats.borrow_mut() = cats;
                    if self.page_history.borrow().last() == Some(&Page::Home) {
                        self.show_page(&Page::Home);
                    }
                }
                Err(e) => self.show_error(&e),
            },
            Msg::Search(res) => match res {
                Ok(results) => {
                    *self.search_results.borrow_mut() = results;
                    let mut st = self.page_history.borrow_mut();
                    if st.last() != Some(&Page::Search) {
                        st.push(Page::Search);
                    }
                    drop(st);
                    self.show_page(&Page::Search);
                }
                Err(e) => self.show_error(&e),
            },
            Msg::Eps(title, res, remotes) => {
                // Diskteki güncel ilerlemeyi al (oynatıcı yazmış olabilir).
                *self.progress.borrow_mut() = self.client.load_state().progress;
                // Uzak konumları ayrı haritada tut (görünüm için; diske yazılmaz).
                if !remotes.is_empty() {
                    let mut rp = self.remote_progress.borrow_mut();
                    for (s, e, rpos) in remotes {
                        rp.insert(format!("{}:{s}:{e}", title.id), (rpos, 0.0));
                    }
                }
                match res {
                    Ok(eps) => {
                        let page = if eps.is_empty() {
                            Page::Movie { title, eps }
                        } else {
                            match title.title_type.as_deref() {
                                Some("movie") => Page::Movie { title, eps },
                                _ => Page::Episodes { title, eps },
                            }
                        };
                        self.page_history.borrow_mut().push(page.clone());
                        self.show_page(&page);
                    }
                    Err(e) => self.show_error(&e),
                }
            }
            Msg::Play(title, ep, res) => match res {
                Ok((fast, fast_embeds, fallback, remote)) => {
                    self.play_candidates(&title, &ep, &fast, &fast_embeds, &fallback, remote)
                }
                Err(e) => self.show_error(&e),
            },
            Msg::Login(res) => match res {
                Ok((u, n_fav)) => {
                    let msg = if n_fav > 0 {
                        format!("Hoş geldin, {}! (siteden {n_fav} favori alındı)", u.name)
                    } else {
                        format!("Hoş geldin, {}!", u.name)
                    };
                    let t = adw::Toast::new(&msg);
                    t.set_timeout(4);
                    self.toast.add_toast(t);
                    // Girişle birlikte sunucu geçmişini de çek.
                    self.fetch_server_history(true);
                    *self.hist_items.borrow_mut() = Vec::new();
                    self.hist_total.set(0);
                    self.hist_fetched.set(0);
                    self.hist_loading.set(false);
                    let top = self.page_history.borrow().last().cloned();
                    if top == Some(Page::Account) || top == Some(Page::Favs) {
                        self.show_page(&top.unwrap_or(Page::Account));
                    }
                }
                Err(e) => self.show_error(&e),
            },
            Msg::ServerHistory(res, reset) => {
                match res {
                    Ok((titles, total)) => {
                        if reset {
                            *self.server_history.borrow_mut() = titles;
                            self.server_pool_pages.set(3);
                            self.cont_page.set(0);
                        } else {
                            // Birleştir (gezinti tazeliği; havuz korunur).
                            let mut cur = self.server_history.borrow_mut();
                            for e in titles {
                                if let Some(i) = cur.iter().position(|x| x.title.id == e.title.id) {
                                    cur[i] = e;
                                } else {
                                    cur.push(e);
                                }
                            }
                            cur.sort_by(|a, b| b.date.cmp(&a.date));
                            drop(cur);
                            self.server_pool_pages.set(self.server_pool_pages.get().max(3));
                        }
                        self.server_total.set(total);
                    }
                    Err(e) => {
                        eprintln!("[AUTH] sunucu geçmişi alınamadı: {e}");
                        // Bayrağı geri al, sonra tekrar denensin.
                        self.server_history_loaded.set(false);
                    }
                }
                let top = self.page_history.borrow().last().cloned();
                if top == Some(Page::History) || top == Some(Page::Home) {
                    self.show_page(&top.unwrap_or(Page::Home));
                }
            }
            Msg::ServerMore(new_pool, res) => {
                self.server_more_loading.set(false);
                match res {
                    Ok((items, total)) => {
                        let mut cur = self.server_history.borrow_mut();
                        for e in items {
                            if let Some(i) = cur.iter().position(|x| x.title.id == e.title.id) {
                                cur[i] = e;
                            } else {
                                cur.push(e);
                            }
                        }
                        cur.sort_by(|a, b| b.date.cmp(&a.date));
                        drop(cur);
                        self.server_total.set(total);
                        self.server_pool_pages.set(new_pool);
                    }
                    Err(e) => {
                        eprintln!("[AUTH] devam havuzu büyütülemedi: {e}");
                        // Bekleyen sayfa geçersiz kaldıysa son geçerliye dön.
                        let pool = self.continue_items().len();
                        let maxp = if pool == 0 { 0 } else { (pool - 1) / 10 };
                        if self.cont_page.get() as usize > maxp {
                            self.cont_page.set(maxp as u32);
                        }
                        self.show_error(&e);
                    }
                }
                let top = self.page_history.borrow().last().cloned();
                if top == Some(Page::Home) {
                    self.show_page(&Page::Home);
                    let saved = self.saved_scroll.get();
                    self.saved_scroll.set(-1.0);
                    self.restore_scroll_value(saved);
                }
            }
            Msg::HistPage(page, res) => {
                self.hist_loading.set(false);
                match res {
                    Ok((items, total)) => {
                        // Birleştir: id'ye göre upsert (yenisi kazanır),
                        // tarih-azalan sırala. Eski yanıtlar zararsızdır.
                        let mut cur = self.hist_items.borrow_mut();
                        for e in items {
                            if let Some(i) = cur.iter().position(|x| x.title.id == e.title.id) {
                                cur[i] = e;
                            } else {
                                cur.push(e);
                            }
                        }
                        cur.sort_by(|a, b| b.date.cmp(&a.date));
                        drop(cur);
                        self.hist_total.set(total);
                        self.hist_fetched.set(self.hist_fetched.get().max(page + 1));
                        *self.hist_error.borrow_mut() = None;
                    }
                    Err(e) => {
                        eprintln!("[AUTH] geçmiş sayfa {page} alınamadı: {e}");
                        // Hata varken otomatik tekrar deneme (sonsuz döngü
                        // olmasın); kullanıcı "Tekrar Dene"ye basar.
                        *self.hist_error.borrow_mut() = Some(e.clone());
                        self.show_error(&e);
                    }
                }
                let top = self.page_history.borrow().last().cloned();
                if top == Some(Page::History) {
                    self.show_page(&Page::History);
                }
            }
            Msg::NewsPage(page, res) => {
                self.news_loading.set(false);
                match res {
                    Ok((items, total, last)) => {
                        if page <= 1 {
                            *self.news_items.borrow_mut() = items;
                        } else {
                            self.news_items.borrow_mut().extend(items);
                        }
                        self.news_page.set(page);
                        self.news_total.set(total);
                        self.news_last.set(last);
                        *self.news_error.borrow_mut() = None;
                    }
                    Err(e) => {
                        eprintln!("[NEWS] sayfa {page} alınamadı: {e}");
                        *self.news_error.borrow_mut() = Some(e.clone());
                        self.show_error(&e);
                    }
                }
                let top = self.page_history.borrow().last().cloned();
                if top == Some(Page::News) {
                    self.show_page(&Page::News);
                }
            }
            Msg::NewsRail(res) => {
                if let Ok((items, _, _)) = res {
                    *self.news_rail.borrow_mut() = items;
                    let top = self.page_history.borrow().last().cloned();
                    if top == Some(Page::Home) {
                        self.show_page(&Page::Home);
                    }
                }
                // Sessiz: hata olursa şerit gizli kalır, toast yok.
            }
            Msg::DisPage(page, res) => {
                self.dis_loading.set(false);
                match res {
                    Ok((items, total, _last)) => {
                        if page <= 1 {
                            *self.dis_items.borrow_mut() = items;
                        } else {
                            self.dis_items.borrow_mut().extend(items);
                        }
                        self.dis_total.set(total);
                        self.dis_fetched.set(self.dis_fetched.get().max(page));
                        *self.dis_error.borrow_mut() = None;
                    }
                    Err(e) => {
                        eprintln!("[DIS] keşfet sayfa {page} alınamadı: {e}");
                        *self.dis_error.borrow_mut() = Some(e);
                    }
                }
                let top = self.page_history.borrow().last().cloned();
                if top == Some(Page::Kesfet) {
                    self.show_page(&Page::Kesfet);
                }
            }
            Msg::Calendar(res) => {
                self.cal_loading.set(false);
                match res {
                    Ok(days) => {
                        *self.cal_days.borrow_mut() = days;
                        *self.cal_error.borrow_mut() = None;
                        // Seçili gün aralığın dışındaysa bugüne en yakın güne dön.
                        let n = self.cal_days.borrow().len();
                        if n > 0 && self.cal_sel.get() as usize >= n {
                            self.cal_sel.set(0);
                        }
                    }
                    Err(e) => {
                        eprintln!("[CAL] takvim alınamadı: {e}");
                        *self.cal_error.borrow_mut() = Some(e.clone());
                        self.show_error(&e);
                    }
                }
                let top = self.page_history.borrow().last().cloned();
                if top == Some(Page::Calendar) {
                    self.show_page(&Page::Calendar);
                }
            }
            Msg::DetData(tid, related, credits, reviews, rev_total) => {
                // Geç kalmış yanıtı ele (kullanıcı başka başlığa geçtiyse).
                if self.det_title.get() != tid {
                    return;
                }
                *self.det_related.borrow_mut() = related;
                *self.det_credits.borrow_mut() = credits;
                *self.det_reviews.borrow_mut() = reviews;
                self.det_rev_total.set(rev_total);
                let top = self.page_history.borrow().last().cloned();
                if matches!(top, Some(Page::Episodes { .. }) | Some(Page::Movie { .. })) {
                    self.show_page(&top.unwrap_or(Page::Home));
                }
            }
            Msg::RevPage(tid, page, res) => {
                self.rev_loading.set(false);
                if self.rev_title.get() != tid {
                    return;
                }
                match res {
                    Ok((items, total, last)) => {
                        if page <= 1 {
                            *self.rev_items.borrow_mut() = items;
                        } else {
                            self.rev_items.borrow_mut().extend(items);
                        }
                        self.rev_page.set(page);
                        self.rev_total.set(total);
                        self.rev_last.set(last);
                        *self.rev_error.borrow_mut() = None;
                    }
                    Err(e) => {
                        eprintln!("[REV] sayfa {page} alınamadı: {e}");
                        *self.rev_error.borrow_mut() = Some(e.clone());
                        self.show_error(&e);
                    }
                }
                let top = self.page_history.borrow().last().cloned();
                if matches!(top, Some(Page::Reviews { .. })) {
                    self.show_page(&top.unwrap_or(Page::Home));
                }
            }
            Msg::LastRail(res) => {
                if let Ok((items, _, _)) = res {
                    *self.last_rail.borrow_mut() = items;
                    let top = self.page_history.borrow().last().cloned();
                    if top == Some(Page::Home) {
                        self.show_page(&Page::Home);
                    }
                }
                // Sessiz: hata olursa şerit gizli kalır, toast yok.
            }
            Msg::FansubsLoaded { title, ep, fansubs, default_template, eps } => {
                // Bölüm listesini tazele (boşsa ve başka başlıksa yine de
                // kimliği güncelle ki panel yanlış liste göstermesin).
                if !eps.is_empty() || self.cur_eps_title.get() != title.id {
                    *self.cur_eps.borrow_mut() = eps;
                    self.cur_eps_title.set(title.id);
                }
                match fansubs {
                    Ok(list) => self.after_fansubs_loaded(title, ep, list, default_template),
                    Err(e) => self.show_error(&e),
                }
            }
            Msg::FansubChosen { title, ep, chosen } => {
                if let Some(fs) = chosen {
                    self.play_with_fansub(&title, &ep, &fs);
                } else {
                    self.play_resolved(&title, &ep, None);
                }
            },
            Msg::EpsFetch { title, ep } => {
                let title_c = title.clone();
                let ep_c = ep.clone();
                self.spawn(move |c| {
                    let res = match c.episode_candidates(title_c.id, ep_c.episode, ep_c.season) {
                        Ok(list) => Ok((Vec::new(), Vec::new(), list, None)),
                        Err(e) => Err(e),
                    };
                    move || Msg::Play(title_c, ep_c, res)
                });
            }
            Msg::DlLists { title, quality, items, is_single } => {
                let with_subs: Vec<(Episode, Vec<api::FansubInfo>)> = items
                    .into_iter()
                    .filter(|(_, l)| !l.is_empty())
                    .collect();
                if with_subs.is_empty() {
                    let t = adw::Toast::new("⚠️ Seçili bölümlerde çeviri bulunamadı");
                    t.set_timeout(3);
                    self.toast.add_toast(t);
                    return;
                }
                if !self.settings.borrow().fansub_ask_each_time {
                    let auto: Vec<(Episode, api::FansubInfo)> = with_subs
                        .into_iter()
                        .map(|(ep, mut l)| (ep, l.remove(0)))
                        .collect();
                    let title_c = title.clone();
                    let quality_c = quality.clone();
                    let this_c = self.clone_ref();
                    let dir = this_c.effective_download_dir();
                    let series = crate::download::sanitize_filename(&title_c.name);
                    self.spawn(move |c| {
                        let mut recs = Vec::new();
                        let mut skipped = Vec::new();
                        for (ep, fs) in &auto {
                            match crate::download::resolve_for_download(
                                &c, &dir, &series, ep, fs, &quality_c,
                            ) {
                                Ok(rec) => recs.push(rec),
                                Err(e) => {
                                    eprintln!("[DL] çözümleme atlandı: {e}");
                                    skipped.push(format!(
                                        "S{:02}E{:02}: {e}",
                                        ep.season, ep.episode
                                    ));
                                }
                            }
                        }
                        move || Msg::DlBatchResolved(recs, skipped, is_single)
                    });
                    return;
                }
                let quality_c = quality.clone();
                let this_c = self.clone_ref();
                let dir = this_c.effective_download_dir();
                let series = crate::download::sanitize_filename(&title.name);
                crate::ui::flashcard::show_flashcard_wizard(
                    &self.window,
                    &title,
                    with_subs,
                    move |done| {
                        if done.is_empty() {
                            return;
                        }
                        let dir_c = dir.clone();
                        let series_c = series.clone();
                        let quality_cc = quality_c.clone();
                        this_c.spawn(move |c| {
                            let mut recs = Vec::new();
                            let mut skipped = Vec::new();
                            for (ep, fs) in &done {
                                match crate::download::resolve_for_download(
                                    &c, &dir_c, &series_c, ep, fs, &quality_cc,
                                ) {
                                    Ok(rec) => recs.push(rec),
                                    Err(e) => {
                                        eprintln!("[DL] çözümleme atlandı: {e}");
                                        skipped.push(format!(
                                            "S{:02}E{:02}: {e}",
                                            ep.season, ep.episode
                                        ));
                                    }
                                }
                            }
                            move || Msg::DlBatchResolved(recs, skipped, is_single)
                        });
                    },
                );
            }
            Msg::DlBatchResolved(recs, skipped, _is_single) => {
                for rec in recs {
                    self.dl_manager.enqueue(rec, true);
                }
                if !skipped.is_empty() {
                    let t = adw::Toast::new(&format!("⚠️ {} indirme atlandı", skipped.len()));
                    t.set_timeout(3);
                    self.toast.add_toast(t);
                }
                self.show_page(&Page::Downloads);
            }
        }
    }

    pub fn open_episodes(&self, title: Title) {
        self.busy(true);
        // Detay zenginleştirme (benzerler + künye + incelemeler) sessizce.
        self.det_title.set(title.id);
        *self.det_related.borrow_mut() = Vec::new();
        *self.det_credits.borrow_mut() = Vec::new();
        *self.det_reviews.borrow_mut() = Vec::new();
        self.det_rev_total.set(0);
        let det_id = title.id;
        self.spawn(move |c| {
            // Üç bağımsız uç paralel çekilir (sıralı olsaydı 3x RTT).
            let (related, credits, rev) = std::thread::scope(|s| {
                let r = s.spawn(|| c.related_titles(det_id));
                let cr = s.spawn(|| c.title_credits(det_id));
                let rv = s.spawn(|| c.reviews(det_id, 1));
                (
                    r.join().unwrap_or_default(),
                    cr.join().unwrap_or_default(),
                    rv.join().unwrap_or_else(|_| Err("incelemeler alınamadı".to_string())),
                )
            });
            let (reviews, total) = rev
                .map(|(r, t, _)| (r, t))
                .unwrap_or((Vec::new(), 0));
            move || Msg::DetData(det_id, related, credits, reviews, total)
        });
        let logged = self.client.is_logged_in();
        self.spawn(move |c| {
            let enriched = c.enrich_title(&title);
            let res = c.episodes(&enriched);
            // Uzak konumlar önden çekilir (ilerleme çubuğu + izlendi tiki);
            // yerelde izlenenler atlanır — istek fırtınası küçülür.
            let mut remotes: Vec<(u64, u64, f64)> = Vec::new();
            if logged {
                if let Ok(eps) = &res {
                    let local = c.load_state().progress;
                    let tid0 = enriched.id;
                    let mut targets: Vec<(u64, u64)> = eps
                        .iter()
                        .map(|e| (e.season, e.episode))
                        .filter(|(s, e)| !local.contains_key(&format!("{tid0}:{s}:{e}")))
                        .collect();
                    targets.truncate(120);
                    let tid = enriched.id;
                    std::thread::scope(|scope| {
                        let mut hs = Vec::new();
                        for (s, e) in targets {
                            let cc = c.clone();
                            hs.push(scope.spawn(move || {
                                let r = cc.get_remote_pos(tid, s, e).unwrap_or(0.0);
                                ((s, e), r)
                            }));
                        }
                        for h in hs {
                            if let Ok(((s, e), r)) = h.join() {
                                if r > 5.0 {
                                    remotes.push((s, e, r));
                                }
                            }
                        }
                    });
                }
            }
            move || Msg::Eps(enriched.clone(), res, remotes)
        });
    }

    fn play(&self, title: &Title, ep: &Episode) {
        let title = title.clone();
        let ep = ep.clone();
        eprintln!("[PLAY] çağrıldı: {} S{:02}E{:02}", title.name, ep.season, ep.episode);
        let is_movie = title.title_type.as_deref() == Some("movie");
        if is_movie {
            // Filmde bölüm paneli anlamsız: eski listeyi temizle.
            *self.cur_eps.borrow_mut() = Vec::new();
            self.cur_eps_title.set(title.id);
            self.play_resolved(&title, &ep, None);
            return;
        }
        let default_template = self.settings.borrow().default_fansub_template;
        self.busy(true);
        let title_s = title.clone();
        let ep_s = ep.clone();
        self.spawn(move |c| {
            let res = c.list_fansubs(title_s.id, ep_s.episode, ep_s.season);
            // Bölüm listesi de burada alınır (player sağ paneli + prev/next
            // hover-play yolunda liste sayfası açılmadığı için boş kalırdı).
            let eps = c.episodes(&title_s).unwrap_or_default();
            move || Msg::FansubsLoaded {
                title: title_s,
                ep: ep_s,
                fansubs: res,
                default_template,
                eps,
            }
        });
    }

    fn after_fansubs_loaded(&self, title: Title, ep: Episode, fansubs: Vec<api::FansubInfo>, default_template: Option<i64>) {
        self.busy(false);

        if fansubs.is_empty() {
            self.play_resolved(&title, &ep, None);
            return;
        }

        if let Some(tpl) = default_template {
            if let Some(fs) = fansubs.iter().find(|f| f.template_id == tpl) {
                self.play_with_fansub(&title, &ep, fs);
                return;
            }
        }

        if fansubs.len() == 1 {
            self.play_with_fansub(&title, &ep, &fansubs[0]);
            return;
        }

        let ask = self.settings.borrow().fansub_ask_each_time;
        if !ask {
            if let Some(best) = fansubs.first() {
                eprintln!("[FS] otomatik seçim: {} ({:.2}★)", best.name, best.rating);
                self.play_with_fansub(&title, &ep, best);
                return;
            }
        }

        let title_s = title.clone();
        let ep_s = ep.clone();
        let app_rc = self.clone_ref();
        crate::ui::fansub_dialog::show_fansub_dialog(
            &self.window,
            &format!("{} — S{:02}E{:02}", title.name, ep.season, ep.episode),
            fansubs,
            move |chosen: api::FansubInfo| {
                app_rc.play_with_fansub(&title_s, &ep_s, &chosen);
            },
        );
    }

    fn play_with_fansub(&self, title: &Title, ep: &Episode, fs: &api::FansubInfo) {
        let toast = adw::Toast::new(&format!(
            "🎬 {} hazırlanıyor ({} · {:.1}★)…",
            title.name, fs.name, fs.rating
        ));
        toast.set_timeout(2);
        self.toast.add_toast(toast);
        eprintln!(
            "[PLAY-FS] {} S{:02}E{:02} → {} ({:.2}★, {} mirror)",
            title.name, ep.season, ep.episode, fs.name, fs.rating, fs.mirror_count
        );
        let mirror_urls: Vec<String> = fs.mirrors.iter().map(|m| m.url.clone()).collect();
        let title_c = title.clone();
        let ep_c = ep.clone();
        let fansub_name = fs.name.clone();
        let client = self.client.clone();
        self.busy(true);
        self.spawn(move |_| {
            // Sunucuya izleme kaydı arka planda işlenir (çözümü bekletmez).
            let pt_c = client.clone();
            let (pt_t, pt_e) = (title_c.clone(), ep_c.clone());
            std::thread::spawn(move || pt_c.put_title(&pt_t, &pt_e));
            let res = client
                .resolve_urls(&mirror_urls, mirror_urls.len().max(1))
                .and_then(|fast_pairs| {
                    let mut fb = client
                        .episode_candidates(title_c.id, ep_c.episode, ep_c.season)
                        .unwrap_or_default();
                    let tried = 3.min(fb.len());
                    fb.drain(..tried);
                    let fast: Vec<String> = fast_pairs.iter().map(|(m, _)| m.clone()).collect();
                    let fast_emb: Vec<String> = fast_pairs.iter().map(|(_, e)| e.clone()).collect();
                    let remote = client.get_remote_pos(title_c.id, ep_c.season, ep_c.episode);
                    Ok((fast, fast_emb, fb, remote))
                })
                .or_else(|e| {
                    eprintln!(
                        "[PLAY-FS] {} mirror'ları çözülemedi: {}",
                        fansub_name, e
                    );
                    Err(format!(
                        "{} çevirisi oynatılamadı ({}). Başka bir çeviri seçin.",
                        fansub_name, e
                    ))
                });
            move || Msg::Play(title_c, ep_c, res)
        });
    }

    fn play_resolved(&self, title: &Title, ep: &Episode, _fansub_template: Option<i64>) {
        let toast = adw::Toast::new(&format!(
            "🎬 {} hazırlanıyor…",
            title.name
        ));
        toast.set_timeout(2);
        self.toast.add_toast(toast);
        let title = title.clone();
        let ep = ep.clone();
        self.busy(true);
        self.spawn(move |c| {
            // Sunucuya izleme kaydı arka planda işlenir (çözümü bekletmez).
            let pt_c = c.clone();
            let (pt_t, pt_e) = (title.clone(), ep.clone());
            std::thread::spawn(move || pt_c.put_title(&pt_t, &pt_e));
            let pref = c.get_preferred_host(title.id);
            let res = if title.title_type.as_deref() == Some("movie") {
                c.resolve_movie(title.id).map(|u| {
                    let remote = c.get_remote_pos(title.id, ep.season, ep.episode);
                    (vec![u], Vec::new(), Vec::new(), remote)
                })
            } else {
                c.resolve_top(title.id, ep.episode, ep.season, 3, pref.as_deref())
                    .and_then(|fast_pairs| {
                        let mut fb = c.episode_candidates(title.id, ep.episode, ep.season)?;
                        let tried = 3.min(fb.len());
                        fb.drain(..tried);
                        if let Some(p) = &pref {
                            fb.sort_by_key(|u| {
                                if api::Client::source_host_hint(u) == p.as_str() { 0 } else { 1 }
                            });
                        }
                        let fast: Vec<String> = fast_pairs.iter().map(|(m, _)| m.clone()).collect();
                        let fast_emb: Vec<String> = fast_pairs.iter().map(|(_, e)| e.clone()).collect();
                        let remote = c.get_remote_pos(title.id, ep.season, ep.episode);
                        Ok((fast, fast_emb, fb, remote))
                    })
            };
            move || Msg::Play(title, ep, res)
        });
    }

    fn decide_retry(
        exited: bool,
        success: bool,
        playing: bool,
    ) -> (bool, bool) {
        if !exited {
            return (false, playing);
        }
        if playing && !success {
            return (true, false);
        }
        (false, playing)
    }

    const SOCKET_TIMEOUT_SECS: u64 = 25;

    fn source_is_dead(elapsed_secs: u64, core_idle: bool, duration: f64, media_loaded: bool, threshold_secs: u64) -> bool {
        !media_loaded && core_idle && duration <= 0.0 && elapsed_secs >= threshold_secs
    }

    fn socket_timeout_hit(elapsed_secs: u64, socket_seen: bool) -> bool {
        !socket_seen && elapsed_secs >= Self::SOCKET_TIMEOUT_SECS
    }

    /// Gömülü oynatıcı dalı: çözümlenmiş kaynakları uygulamanın içindeki
    /// libmpv penceresinde açar (harici mpv process'i başlatılmaz).
    /// Yıldız değişimini sunucuya yaz (girişliyse, arka planda).
    fn sync_fav_remote(&self, title_id: u64, saved: bool) {
        if !self.client.is_logged_in() {
            return;
        }
        let c = self.client.clone();
        std::thread::spawn(move || {
            let r = if saved {
                c.fav_add(title_id)
            } else {
                c.fav_remove(title_id)
            };
            match r {
                Ok(()) => eprintln!("[AUTH] favori senkron tamam ({title_id})"),
                Err(e) => eprintln!("[AUTH] favori senkron hatası: {e}"),
            }
        });
    }

    /// Başlangıç konumu: yerel + sunucu (girişliyse), büyük olanı al.
    fn start_pos(&self, title_id: u64, season: u64, episode: u64, remote: Option<f64>) -> Option<f64> {
        let local = self
            .client
            .get_progress(title_id, season, episode)
            .filter(|(pos, dur)| *pos > 5.0 && *dur > 0.0 && *pos / *dur < 0.95)
            .map(|(pos, _)| pos);
        let remote = remote.filter(|r| *r > 5.0);
        match (local, remote) {
            (Some(l), Some(r)) => {
                if r > l + 5.0 {
                    eprintln!("[SYNC] sunucu konumu kullanılıyor: {r:.0}sn (yerel {l:.0}sn)");
                    Some(r)
                } else {
                    Some(l)
                }
            }
            (Some(l), None) => Some(l),
            (None, Some(r)) => {
                eprintln!("[SYNC] sunucu konumu kullanılıyor: {r:.0}sn");
                Some(r)
            }
            (None, None) => None,
        }
    }

    fn play_embedded(
        &self,
        title: &Title,
        ep: &Episode,
        candidates: &[String],
        fast_embeds: &[String],
        fallback_embeds: &[String],
        remote_pos: Option<f64>,
    ) {
        let saved_pos = self.start_pos(title.id, ep.season, ep.episode, remote_pos);
        let s = self.settings.borrow();
        // Komşu bölümler (aynı liste açıksa): önceki/sonraki butonları için.
        let (prev_ep, next_ep, all_eps) = if self.cur_eps_title.get() == title.id {
            let eps = self.cur_eps.borrow();
            let all = eps.clone();
            let pair = match eps.iter().position(|e| e.season == ep.season && e.episode == ep.episode) {
                Some(i) => (
                    i.checked_sub(1).and_then(|p| eps.get(p).cloned()),
                    eps.get(i + 1).cloned(),
                ),
                None => (None, None),
            };
            (pair.0, pair.1, all)
        } else {
            (None, None, Vec::new())
        };
        let title_c = title.clone();
        let app_c = self.clone_ref();
        let play_episode: Rc<dyn Fn(Episode)> =
            Rc::new(move |next: Episode| app_c.play(&title_c, &next));
        let app_back = self.clone_ref();
        let on_back: Rc<dyn Fn()> = Rc::new(move || app_back.go_back());
        let req = crate::player_window::EmbedRequest {
            title: title.clone(),
            ep: ep.clone(),
            candidates: candidates.to_vec(),
            fast_embeds: fast_embeds.to_vec(),
            fallback_embeds: fallback_embeds.to_vec(),
            saved_pos,
            upscale: s.upscale.clone(),
            aniskip_enabled: s.aniskip_enabled,
            patience_secs: self.client.load_settings().source_patience_secs.max(10),
            auto_fullscreen: s.auto_fullscreen,
            prev_ep,
            next_ep,
            episodes: all_eps,
            play_episode: Some(play_episode),
            on_back: Some(on_back),
        };
        drop(s);
        eprintln!(
            "[PLAY-EMBED] gömülü oynatıcı: {} S{:02}E{:02} (kaynak sayısı={})",
            title.name,
            ep.season,
            ep.episode,
            candidates.len()
        );
        // Önceki oynatıcı varsa kapat (üst üste tıklama).
        if let Some(p) = self.player.borrow().as_ref() {
            p.shutdown();
        }
        *self.player.borrow_mut() = None;
        match crate::player_window::build_embedded_player(
            &self.window,
            &self.header_bar,
            &self.sidebar_revealer,
            &self.toast,
            &self.client,
            &self.progress,
            &self.progress_bars,
            req,
        ) {
            Some(handle) => {
                *self.player.borrow_mut() = Some(handle);
                let page = Page::Player { title: title.clone(), ep: ep.clone() };
                self.page_history.borrow_mut().push(page.clone());
                self.show_page(&page);
            }
            None => self.show_error("Gömülü oynatıcı başlatılamadı (libmpv kurulu mu?)"),
        }
    }

    fn build_player_view(&self) -> gtk::Box {
        if let Some(p) = self.player.borrow().as_ref() {
            p.widget().clone()
        } else {
            // Güvenlik: handle yoksa boş sayfa (normalde buraya düşülmez).
            let b = gtk::Box::new(gtk::Orientation::Vertical, 0);
            b.append(&crate::ui::components::create_status_page(
                "Oynatıcı Kapalı",
                "Geri dönüp bölümü yeniden seçin.",
                "media-playback-stop-symbolic",
            ));
            b
        }
    }

    fn play_candidates(&self, title: &Title, ep: &Episode, candidates: &[String], fast_embeds: &[String], fallback_embeds: &[String], remote_pos: Option<f64>) {
        let w = api::Watched {
            title_id: title.id,
            episode: ep.episode,
            season: ep.season,
        };
        self.client.set_current(&w);
        self.client.add_history(&title, &ep);

        // GÖMÜLÜ DAL: video uygulamanın içindeki oynatıcı pencerede açılır.
        if self.settings.borrow().embedded_player {
            self.play_embedded(title, ep, candidates, fast_embeds, fallback_embeds, remote_pos);
            return;
        }
        eprintln!(
            "[PLAY-CAND] yeni mpv başlatılıyor: {} S{:02}E{:02} (kaynak sayısı={})",
            title.name, ep.season, ep.episode, candidates.len()
        );

        let media_title = format!("{} | S{:02}E{:02}", title.name, ep.season, ep.episode);
        let tid = title.id;
        let season = ep.season;
        let episode = ep.episode;
        let prog_key = format!("{tid}:{season}:{episode}");

        let saved_pos = self.start_pos(tid, season, episode, remote_pos);

        let sock_path = format!("/tmp/animecix-mpv-{tid}-{season}-{episode}.sock");
        let _ = std::fs::remove_file(&sock_path);

        let auto_fullscreen = self.settings.borrow().auto_fullscreen;
        let upscale = self.settings.borrow().upscale.clone();
        let aniskip_enabled = self.settings.borrow().aniskip_enabled;
        let aniskip_shared = std::sync::Arc::new(std::sync::Mutex::new(api::AniSkipTimes::default()));

        let skip_cmd = if aniskip_enabled {
            "s show-text \"⏳ AniSkip çözümleniyor…\" 2000\n".to_string()
        } else {
            "s show-text \"AniSkip kapalı (Ayarlar)\" 2000\n".to_string()
        };
        let outro_cmd = if aniskip_enabled {
            "e show-text \"⏳ AniSkip çözümleniyor…\" 2000\n".to_string()
        } else {
            "e show-text \"AniSkip kapalı (Ayarlar)\" 2000\n".to_string()
        };

        let input_conf_path = format!("/tmp/animecix-input-{tid}.conf");
        let input_conf_content = format!(
            "{skip_cmd}\
             {outro_cmd}\
             S seek -30; show-text \"⏪ 30s Geri\" 2000\n\
             End ignore\n"
        );
        let _ = std::fs::write(&input_conf_path, input_conf_content);

        let progress = self.progress.clone();
        let progress_bars = self.progress_bars.clone();
        let client = self.client.clone();
        let toast = self.toast.clone();

        if let Some(old) = self.opening_toast.borrow_mut().take() {
            old.dismiss();
        }
        let t = adw::Toast::new(&format!(
            "▶ {media_title} açılıyor…{}",
            saved_pos.map(|p| {
                let s = p as u64;
                if s >= 3600 { format!(" ({}:{:02}:{:02}'den)", s/3600, (s%3600)/60, s%60) }
                else { format!(" ({}:{:02}'den)", s/60, s%60) }
            }).unwrap_or_default()
        ));
        t.set_timeout(0);
        self.opening_toast.borrow_mut().replace(t.clone());
        self.opening_toast_shown_at.borrow_mut().replace(std::time::Instant::now());
        self.toast.add_toast(t);

        let alive = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        let current_shared = std::sync::Arc::new(std::sync::Mutex::new((episode, season)));

        let mpv_child = std::sync::Arc::new(std::sync::Mutex::new(None::<std::process::Child>));

        let (toast_tx, toast_rx) = std::sync::mpsc::channel::<String>();
        const DISMISS_OPENING: &str = "__animecix_dismiss_opening__";
        {
            let toast_rx = std::sync::Arc::new(std::sync::Mutex::new(toast_rx));
            let alive_toast = alive.clone();
            let toast_h = toast.clone();
            let opening_toast_h = self.opening_toast.clone();
            let opening_toast_shown_at_h = self.opening_toast_shown_at.clone();
            glib::timeout_add_local(std::time::Duration::from_millis(200), move || {
                let rx = toast_rx.lock().unwrap();
                let mut msg: Option<String> = None;
                loop {
                    match rx.try_recv() {
                        Ok(m) => msg = Some(m),
                        Err(std::sync::mpsc::TryRecvError::Empty) => break,
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
                    }
                }
                drop(rx);
                if let Some(m) = msg {
                    if m == DISMISS_OPENING {
                        if let Some(t) = opening_toast_h.borrow_mut().take() {
                            let elapsed = opening_toast_shown_at_h
                                .borrow()
                                .map(|i| i.elapsed())
                                .unwrap_or_default();
                            if elapsed < std::time::Duration::from_secs(10) {
                                let remain = std::time::Duration::from_secs(10) - elapsed;
                                let t2 = t.clone();
                                glib::timeout_add_local(remain, move || {
                                    t2.dismiss();
                                    glib::ControlFlow::Break
                                });
                            } else {
                                t.dismiss();
                            }
                        }
                    } else {
                        let tt = adw::Toast::new(&m);
                        tt.set_timeout(3);
                        toast_h.add_toast(tt);
                    }
                }
                if !alive_toast.load(std::sync::atomic::Ordering::Relaxed) {
                    glib::ControlFlow::Break
                } else {
                    glib::ControlFlow::Continue
                }
            });
        }

        if aniskip_enabled {
            let alive_r = alive.clone();
            let client_r = client.clone();
            let name_r = title.name.clone();
            let shared_r = aniskip_shared.clone();
            let sock_r = sock_path.clone();
            let conf_r = input_conf_path.clone();
            let toast_tx_r = toast_tx.clone();
            std::thread::spawn(move || {
                const MAX_TRIES: u32 = 5;
                for attempt in 0..MAX_TRIES {
                    if !alive_r.load(std::sync::atomic::Ordering::Relaxed) { return; }
                    let t = client_r.fetch_aniskip_timestamps(&name_r, episode);
                    if t.op_end.is_some() || t.ed_end.is_some() {
                        *shared_r.lock().unwrap() = t;
                        let t2 = shared_r.lock().unwrap().clone();
                        let _ = std::fs::write(&conf_r, aniskip_input_conf(&t2));
                        if std::path::Path::new(&sock_r).exists() {
                            if let Some(et) = t2.op_end {
                                crate::player::send_mpv_cmd(&sock_r, &format!("{{\"command\":[\"keybind\",\"s\",\"seek {et:.1} absolute\"]}}\n"));
                            }
                            if let Some(et) = t2.ed_end {
                                crate::player::send_mpv_cmd(&sock_r, &format!("{{\"command\":[\"keybind\",\"e\",\"seek {et:.1} absolute\"]}}\n"));
                            }
                        }
                        let _ = toast_tx_r.send("⏩ AniSkip hazır ('s' ile intro atlarsın)".to_string());
                        return;
                    }
                    if attempt + 1 < MAX_TRIES {
                        std::thread::sleep(std::time::Duration::from_secs(45));
                    }
                }
                if std::path::Path::new(&sock_r).exists() {
                    crate::player::send_mpv_cmd(&sock_r, "{\"command\":[\"keybind\",\"s\",\"show-text \\\"⚠️ İntro zamanı bulunamadı (AniSkip)\\\" 2500\"]}\n");
                    crate::player::send_mpv_cmd(&sock_r, "{\"command\":[\"keybind\",\"e\",\"show-text \\\"⚠️ Outro zamanı bulunamadı (AniSkip)\\\" 2500\"]}\n");
                }
                let _ = toast_tx_r.send("⚠️ AniSkip: intro/outro zamanları bulunamadı".to_string());
            });
        }

        {
            let alive = alive.clone();
            let sock_poll = sock_path.clone();
            let sock_c = sock_path.clone();
            let aniskip_c = aniskip_shared.clone();
            let client_c = client.clone();
            let current_shared_c = current_shared.clone();
            let (sender, receiver) = std::sync::mpsc::channel::<(f64, f64)>();
            std::thread::spawn(move || {
                let mut op_prompted = false;
                let mut ed_prompted = false;

                while alive.load(std::sync::atomic::Ordering::Relaxed)
                    && !std::path::Path::new(&sock_poll).exists()
                {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                }
                if !alive.load(std::sync::atomic::Ordering::Relaxed) { return; }

                loop {
                    if std::path::Path::new(&sock_c).exists() {
                        let snap = aniskip_c.lock().unwrap().clone();
                        let (pos, dur) = crate::player::query_mpv_position(&sock_c).unwrap_or((0.0, 0.0));
                        if let Some(st) = snap.op_start {
                            if !op_prompted && pos >= (st - 1.5) && pos <= (st + 25.0) {
                                op_prompted = true;
                                crate::player::send_mpv_cmd(&sock_c, "{\"command\":[\"show-text\", \"⏩ İntro Başladı ('s' ile atlayabilirsiniz)\", 7000]}\n");
                            }
                        }
                        if let Some(st) = snap.ed_start {
                            if !ed_prompted && pos >= (st - 1.5) && pos <= (st + 25.0) {
                                ed_prompted = true;
                                crate::player::send_mpv_cmd(&sock_c, "{\"command\":[\"show-text\", \"🏁 Outro Başladı\", 7000]}\n");
                            }
                        }
                        if sender.send((pos, dur)).is_err() { break; }
                    } else {
                        if !alive.load(std::sync::atomic::Ordering::Relaxed) { break; }
                        std::thread::sleep(std::time::Duration::from_millis(250));
                        continue;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(500));
                }
            });

            let progress2 = progress.clone();
            let progress_bars2 = progress_bars.clone();
            let client_prog = client.clone();
            let receiver = std::sync::Arc::new(std::sync::Mutex::new(receiver));
            let current_shared_t = current_shared.clone();
            let mut marked_ep: Option<(u64, u64)> = None;
            let mut last_report = std::time::Instant::now();
            glib::timeout_add_local(std::time::Duration::from_millis(500), move || {
                let rx = receiver.lock().unwrap();
                let mut latest: Option<(f64, f64)> = None;
                let mut disconnected = false;
                loop {
                    match rx.try_recv() {
                        Ok(v) => { latest = Some(v); }
                        Err(std::sync::mpsc::TryRecvError::Empty) => break,
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => { disconnected = true; break; }
                    }
                }
                drop(rx);
                let cur = *current_shared_t.lock().unwrap();
                let (cur_ep, cur_season) = cur;
                let pk_cur = format!("{tid}:{cur_season}:{cur_ep}");

                if disconnected {
                    let last = progress2.borrow().get(&pk_cur).copied();
                    if let Some((pos, dur)) = last {
                        client_prog.save_progress(tid, cur.1, cur.0, pos, dur);
                    }
                    return glib::ControlFlow::Break;
                }

                if let Some((pos, dur)) = latest {
                    if pos < 1.0 { return glib::ControlFlow::Continue; }
                    progress2.borrow_mut().insert(pk_cur.clone(), (pos, dur));
                    if let Some((pb, lbl)) = progress_bars2.borrow().get(&pk_cur) {
                        if dur > 0.0 {
                            pb.set_fraction((pos / dur).clamp(0.0, 1.0));
                            let fmt = |s: f64| -> String {
                                let s = s as u64;
                                if s >= 3600 { format!("{}:{:02}:{:02}", s/3600, (s%3600)/60, s%60) }
                                else { format!("{}:{:02}", s/60, s%60) }
                            };
                            lbl.set_text(&format!("{} / {}", fmt(pos), fmt(dur)));
                            lbl.set_visible(true);
                            pb.set_visible(true);
                        }
                    }
                    client_prog.save_progress(tid, cur.1, cur.0, pos, dur);
                    // Sunucuya konum raporu (60sn'de bir, arka planda).
                    if pos >= 10.0 && last_report.elapsed().as_secs() >= 60 {
                        last_report = std::time::Instant::now();
                        let rc = client_prog.clone();
                        std::thread::spawn(move || rc.report_pos(tid, cur.1, cur.0, pos));
                    }
                    if api::Client::played_enough(pos, dur) && marked_ep != Some(cur) {
                        client_prog.save_watched(&api::Watched { title_id: tid, episode: cur.0, season: cur.1 }, "");
                        marked_ep = Some(cur);
                    }
                }
                glib::ControlFlow::Continue
            });
        }

        {
            let alive = alive.clone();
            let sock_path_c = sock_path.clone();
            let input_conf_path_c = input_conf_path.clone();
            let media_title_c = media_title.clone();
            let toast_tx_c = toast_tx.clone();
            let saved_pos_c = saved_pos;
            let auto_fullscreen_c = auto_fullscreen;
            let upscale_c = upscale;
            let mpv_child_c = mpv_child.clone();
            let mut candidates: Vec<String> = candidates.to_vec();
            let fallback_embeds_c = fallback_embeds.to_vec();
            let fast_embeds_c = fast_embeds.to_vec();
            let candidates_len_c = candidates.len();
            let client_fb = self.client.clone();
            let tid_c = title.id;
            let patience_c = self.client.load_settings().source_patience_secs.max(10);
            let total = candidates.len() + fallback_embeds_c.len();
            let use_proxy = std::net::TcpStream::connect_timeout(
                &"127.0.0.1:10808".parse().expect("statik adres"),
                std::time::Duration::from_millis(300),
            ).is_ok();
            if use_proxy {
                eprintln!("[SUP] yerel proxy aktif (127.0.0.1:10808), mpv oradan çıkacak");
            }
            std::thread::spawn(move || {
                'supervisor: for i in 0..total {
                    let url: String = if i < candidates.len() {
                        candidates[i].clone()
                    } else {
                        let emb = &fallback_embeds_c[i - candidates.len()];
                        eprintln!("[SUP] JIT yedek çözümleniyor");
                        match client_fb.resolve_single(emb) {
                            Ok(u) => u,
                            Err(e) => {
                                eprintln!("[SUP] JIT yedek çözülemedi, geçiliyor: {e}");
                                continue;
                            }
                        }
                    };
                    let _ = std::fs::remove_file(&sock_path_c);
                    let mut cmd = std::process::Command::new("mpv");
                    cmd.arg("--user-agent=Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
                        .arg(format!("--force-media-title={media_title_c}"))
                        .arg("--keep-open=yes")
                        .arg(format!("--input-ipc-server={sock_path_c}"));
                    if auto_fullscreen_c { cmd.arg("--fullscreen"); }
                    cmd.arg(format!("--input-conf={input_conf_path_c}"));
                    cmd.args(saved_pos_c.map(|p| format!("--start={p:.1}")).as_slice())
                        .arg("--cache=yes")
                        .arg("--demuxer-max-bytes=128MiB")
                        .arg("--demuxer-max-back-bytes=32MiB")
                        .arg("--demuxer-readahead-secs=120")
                        .arg("--cache-pause=yes")
                        .arg("--cache-pause-wait=3")
                        .arg("--cache-secs=120")
                        .arg("--stream-lavf-o=reconnect=1,reconnect_streamed=1,reconnect_delay_max=5")
                        .arg("--network-timeout=10")
                        .arg("--hwdec=auto-safe")
                        .arg("--ytdl-format=bestvideo[height<=1080]+bestaudio/best")
                        .args(crate::api::upscale_mpv_args(&upscale_c, match upscale_c.as_str() {
                            "hafif" => resolve_upscale_shader("Anime4K_Upscale_DTD_x2.glsl"),
                            "ultra" => resolve_upscale_shader("Anime4K_Upscale_CNN_x2_UL.glsl"),
                            "hafif_keskin" => resolve_upscale_shader("Anime4K_Upscale_DTD_x2.glsl"),
                            _ => None,
                        }.as_deref(), None))
                        .arg(url.as_str());
                    if url.contains("video.sibnet.ru/v/") {
                        let vid = url
                            .split("/v/")
                            .nth(1)
                            .and_then(|s| s.split('/').nth(1))
                            .map(|s| s.trim_end_matches(".mp4"))
                            .unwrap_or("");
                        let referer = if vid.is_empty() {
                            "https://video.sibnet.ru/".to_string()
                        } else {
                            format!("https://video.sibnet.ru/shell.php?videoid={}", vid)
                        };
                        cmd.arg(format!(
                            "--http-header-fields=Referer: {}\nAccept: */*",
                            referer
                        ));
                    }
                    if use_proxy {
                        cmd.arg("--http-proxy=http://127.0.0.1:10808");
                    }
                    eprintln!("[SUP] mpv spawn deneniyor (ep={}, kaynak={}, url={:.80})", episode, i, url);
                    let child = match cmd.spawn() {
                        Ok(c) => c,
                        Err(e) => { eprintln!("[SUP] HATA mpv başlatılamadı (ep={}, kaynak={}): {}", episode, i, e); continue; }
                    };
                    eprintln!("[SUP] mpv spawn edildi (ep={}, kaynak={})", episode, i);
                    *mpv_child_c.lock().unwrap() = Some(child);

                    let start = std::time::Instant::now();
                    let mut playing = false;
                    let mut media_loaded = false;
                    loop {
                        let (exited, success) = {
                            let mut g = mpv_child_c.lock().unwrap();
                            match g.as_mut().unwrap().try_wait() {
                                Ok(Some(status)) => (true, status.success()),
                                Ok(None) => (false, false),
                                Err(_) => (true, false),
                            }
                        };
                        if exited {
                            let retry = Self::decide_retry(
                                true,
                                success,
                                playing,
                            ).0;
                            if retry {
                                eprintln!("[SUP] kaynak hatalı çıktı, sonraki kaynağa geçiliyor (ep={}, kaynak={})", episode, i);
                                playing = false;
                            }
                            break;
                        }
                        if std::path::Path::new(&sock_path_c).exists() {
                            if !playing {
                                eprintln!("[SUP] socket belirdi, oynatma başladı (ep={}, kaynak={})", episode, i);
                                let _ = toast_tx_c.send(DISMISS_OPENING.to_string());
                            }
                            playing = true;
                        }
                        if playing {
                            let idle = crate::player::query_mpv_prop(&sock_path_c, "core-idle").unwrap_or(0.0);
                            let dur = crate::player::query_mpv_prop(&sock_path_c, "duration").unwrap_or(0.0);
                            if dur > 0.0 {
                                media_loaded = true;
                            }
                            let elapsed_secs = start.elapsed().as_secs();
                            if Self::source_is_dead(elapsed_secs, idle >= 1.0, dur, media_loaded, patience_c) {
                                eprintln!("[SUP] kaynak hiç yüklemedi (idle+duration=0, {}sn), sonraki kaynağa geçiliyor (ep={}, kaynak={})", elapsed_secs, episode, i);
                                let _ = toast_tx_c.send("Kaynak açıldı ama oynatamadı, diğer kaynağa geçiliyor…".to_string());
                                if let Some(c) = mpv_child_c.lock().unwrap().as_mut() {
                                    let _ = c.kill();
                                }
                                playing = false;
                                break;
                            }
                        }
                        if Self::socket_timeout_hit(start.elapsed().as_secs(), playing) {
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(250));
                    }

                    eprintln!("[SUP] döngü bitti (ep={}, kaynak={}, playing={})", episode, i, playing);

                    if playing {
                        let src_hint = if i < fast_embeds_c.len() {
                            api::Client::source_host_hint(&fast_embeds_c[i])
                        } else if i >= candidates_len_c {
                            api::Client::source_host_hint(&fallback_embeds_c[i - candidates_len_c])
                        } else {
                            ""
                        };
                        if !src_hint.is_empty() {
                            client_fb.set_preferred_host(tid_c, src_hint);
                        }
                        if let Some(c) = mpv_child_c.lock().unwrap().as_mut() { let _ = c.wait(); }
                        break 'supervisor;
                    } else {
                        if let Some(c) = mpv_child_c.lock().unwrap().as_mut() {
                            let _ = c.kill();
                            let _ = c.wait();
                        }
                        if i + 1 < total {
                            let _ = toast_tx_c.send("Kaynak açılamadı, diğer kaynağa geçiliyor…".to_string());
                            continue;
                        } else {
                            let _ = toast_tx_c.send("Bölüm hiçbir kaynakta açılamadı.".to_string());
                            break;
                        }
                    }
                }
                alive.store(false, std::sync::atomic::Ordering::SeqCst);
                let _ = std::fs::remove_file(&sock_path_c);
            });
        }
    }

    fn do_search(&self, q: String) {
        let q = q.trim().to_string();
        if q.is_empty() { return; }
        self.busy(true);
        self.spawn(move |c| {
            let res = c.search(&q);
            move || Msg::Search(res)
        });
    }

    fn fetch_home(&self) {
        self.busy(true);
        self.spawn(move |c| {
            let res = c.home_lists();
            move || Msg::Cats(res)
        });
        // Gündem + son eklenenler şeritleri sessizce (önbellekli, boşsa bir kez).
        self.fetch_news_rail();
        self.fetch_last_rail();
    }

    fn show_error(&self, msg: &str) {
        eprintln!("animecix hatası: {msg}");
        let t = adw::Toast::new(&format!("Hata: {msg}"));
        t.set_timeout(4);
        self.toast.add_toast(t);
    }
}

#[cfg(test)]
mod decide_retry_tests {
    use super::App;


    #[test]
    fn hero_h_scales_with_window() {
        assert_eq!(super::hero_h_for_window(885, 8), 460);
        assert_eq!(super::hero_h_for_window(800, 5), 400);
        assert_eq!(super::hero_h_for_window(680, 3), 360);
        assert_eq!(super::hero_h_for_window(2000, 8), 560);
    }

    // Kart boyları her başlıkta eşit kalmalı (ekran yoksa atlanır).
    #[test]
    fn continue_cards_stay_uniform() {
        use gtk::prelude::WidgetExt as _;
        if gtk::init().is_err() {
            eprintln!("SKIP: ekran yok");
            return;
        }
        let app = adw::Application::builder()
            .application_id("com.test.cardprobe2")
            .build();
        let inst = super::App::new(&app);
        let cases = [
            ("Super no Ura de Yani Suu Futari", 1, 3),
            ("Mushoku Tensei: Jobless Reincarnation", 1, 22),
            ("MAO", 1, 2),
            ("Kimetsu no Yaiba: Mugen Jou-hen", 1, 1),
            ("D-Frag!", 1, 1),
        ];
        for (name, season, episode) in cases {
            let t = super::Title {
                id: 1,
                name: name.to_string(),
                ..Default::default()
            };
            let e = super::Episode {
                episode,
                season,
                name: "Bölüm".to_string(),
                thumbnail: None,
            };
            let card = inst.continue_card(&t, Some(&e));
            let (mn, nat, _, _) = card.measure(gtk::Orientation::Vertical, 140);
            eprintln!("[CARD140] {name:?} min_h={mn} nat_h={nat}");
        }
    }


    #[test]
    fn decide_retry_source_error_retries() {
        let (retry, playing) = App::decide_retry(true, false, true);
        assert!(retry, "kaynak hatası yeniden denenmeli");
        assert!(!playing);
    }

    #[test]
    fn decide_retry_user_close_no_retry() {
        let (retry, playing) = App::decide_retry(true, true, true);
        assert!(!retry);
        assert!(playing);
    }

    #[test]
    fn decide_retry_not_exited_no_retry() {
        let (retry, playing) = App::decide_retry(false, false, true);
        assert!(!retry);
        assert!(playing);
    }

    #[test]
    fn decide_retry_never_opened_no_retry_flag() {
        let (retry, playing) = App::decide_retry(true, false, false);
        assert!(!retry);
        assert!(!playing);
    }


    #[test]
    fn slow_source_within_window_is_not_killed() {
        assert!(!App::source_is_dead(5, true, 0.0, false, 20), "5sn'de öldürülmemeli");
        assert!(!App::source_is_dead(19, true, 0.0, false, 20), "19sn'de hâlâ sabırlı olunmalı");
    }

    #[test]
    fn never_loaded_idle_source_is_dead_after_threshold() {
        assert!(App::source_is_dead(20, true, 0.0, false, 20), "20sn+idle+dur=0 -> ölü");
        assert!(App::source_is_dead(120, true, 0.0, false, 20));
    }

    #[test]
    fn threshold_is_user_configurable() {
        assert!(!App::source_is_dead(45, true, 0.0, false, 90), "90sn sabırda 45sn ölü sayılmamalı");
        assert!(App::source_is_dead(90, true, 0.0, false, 90), "90sn sabırda eşikte ölü");
    }

    #[test]
    fn loaded_source_is_never_killed_by_dead_check() {
        assert!(!App::source_is_dead(600, true, 1435.0, true, 20));
        assert!(!App::source_is_dead(600, true, 0.0, true, 20), "yüklendiyse duration anlık 0 okunsa bile");
    }

    #[test]
    fn playing_source_or_unknown_duration_not_dead() {
        assert!(!App::source_is_dead(600, false, 0.0, false, 20));
        assert!(!App::source_is_dead(600, true, 12.0, false, 20));
    }

    #[test]
    fn socket_timeout_only_when_socket_never_seen() {
        assert!(App::socket_timeout_hit(26, false), "soket hiç gelmedi + 25sn doldu -> vazgeç");
        assert!(!App::socket_timeout_hit(24, false), "henüz süre dolmadı");
        assert!(!App::socket_timeout_hit(600, true), "soket varken zaman aşımı uygulanmaz");
    }

    #[test]
    fn anime4k_normal_maps_to_bundled_cnn_shader() {
        let p = super::resolve_upscale_shader("Anime4K_Upscale_CNN_x2_M.glsl");
        assert!(p.is_some(), "normal modu için CNN_x2_M shader'ı bundle edilmiş olmalı");
    }
}
