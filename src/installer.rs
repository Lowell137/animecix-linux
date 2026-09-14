//! Bağımlılık yükleyici: dağıtımı algılar, paketleri kontrol eder, kurulumu yönetir.
use std::process::Command;
use std::rc::Rc;
use std::cell::RefCell;
use gtk::prelude::*;
use adw::prelude::*;

/// Desteklenen dağıtımlar
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Distro {
    Arch,
    Fedora,
    Debian,
    Ubuntu,
    OpenSUSE,
    Alpine,
    Gentoo,
    NixOS,
    Unknown,
}

impl Distro {
    pub fn detect() -> Self {
        let os_release = std::fs::read_to_string("/etc/os-release").unwrap_or_default();
        let id = os_release
            .lines()
            .find(|l| l.starts_with("ID="))
            .and_then(|l| l.split('=').nth(1))
            .map(|s| s.trim_matches('"').to_lowercase())
            .unwrap_or_default();

        let id_like = os_release
            .lines()
            .find(|l| l.starts_with("ID_LIKE="))
            .and_then(|l| l.split('=').nth(1))
            .map(|s| s.trim_matches('"').to_lowercase())
            .unwrap_or_default();

        // ID_LIKE de kontrol et (örn: manjaro -> arch)
        let all_ids = format!("{} {}", id, id_like);

        if all_ids.contains("arch") || all_ids.contains("manjaro") || all_ids.contains("endeavour") || all_ids.contains("garuda") {
            Distro::Arch
        } else if all_ids.contains("fedora") || all_ids.contains("rhel") || all_ids.contains("centos") || all_ids.contains("rocky") || all_ids.contains("alma") {
            Distro::Fedora
        } else if all_ids.contains("debian") || all_ids.contains("ubuntu") || all_ids.contains("mint") || all_ids.contains("pop") || all_ids.contains("kali") || all_ids.contains("elementary") || all_ids.contains("zorin") {
            Distro::Debian
        } else if all_ids.contains("opensuse") || all_ids.contains("suse") {
            Distro::OpenSUSE
        } else if all_ids.contains("alpine") {
            Distro::Alpine
        } else if all_ids.contains("gentoo") {
            Distro::Gentoo
        } else if all_ids.contains("nixos") || all_ids.contains("nix") {
            Distro::NixOS
        } else {
            Distro::Unknown
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Distro::Arch => "Arch Linux / Manjaro / EndeavourOS",
            Distro::Fedora => "Fedora / RHEL / CentOS",
            Distro::Debian => "Debian / Ubuntu / Mint / Pop!_OS",
            Distro::OpenSUSE => "openSUSE",
            Distro::Alpine => "Alpine Linux",
            Distro::Gentoo => "Gentoo",
            Distro::NixOS => "NixOS",
            Distro::Unknown => "Bilinmeyen",
        }
    }

    pub fn package_manager(&self) -> (&'static str, &'static [&'static str]) {
        match self {
            Distro::Arch => ("pacman", &["-S", "--needed", "--noconfirm"]),
            Distro::Fedora => ("dnf", &["install", "-y"]),
            Distro::Debian => ("apt", &["update", "&&", "apt", "install", "-y"]),
            Distro::OpenSUSE => ("zypper", &["install", "-y"]),
            Distro::Alpine => ("apk", &["add"]),
            Distro::Gentoo => ("emerge", &["--ask", "--verbose"]),
            Distro::NixOS => ("nix-env", &["-iA"]),
            Distro::Unknown => ("", &[]),
        }
    }

    pub fn packages(&self) -> &'static [&'static str] {
        match self {
            // mpv, gtk4, libadwaita, gstreamer (video için), ffmpeg
            Distro::Arch => &["mpv", "gtk4", "libadwaita", "gst-plugins-base", "gst-plugins-good", "gst-plugins-bad", "gst-plugins-ugly", "gst-libav", "ffmpeg"],
            Distro::Fedora => &["mpv", "gtk4", "libadwaita", "gstreamer1-plugins-base", "gstreamer1-plugins-good", "gstreamer1-plugins-bad-free", "gstreamer1-plugins-ugly-free", "gstreamer1-libav", "ffmpeg"],
            Distro::Debian => &["mpv", "libgtk-4-1", "libadwaita-1-0", "gstreamer1.0-plugins-base", "gstreamer1.0-plugins-good", "gstreamer1.0-plugins-bad", "gstreamer1.0-plugins-ugly", "gstreamer1.0-libav", "ffmpeg"],
            Distro::OpenSUSE => &["mpv", "gtk4", "libadwaita-1-0", "gstreamer-plugins-base", "gstreamer-plugins-good", "gstreamer-plugins-bad", "gstreamer-plugins-ugly", "gstreamer-plugins-libav", "ffmpeg"],
            Distro::Alpine => &["mpv", "gtk4", "libadwaita", "gstreamer", "gst-plugins-base", "gst-plugins-good", "gst-plugins-bad", "gst-plugins-ugly", "gst-libav", "ffmpeg"],
            Distro::Gentoo => &["media-video/mpv", "x11-libs/gtk+:4", "gnome-extra/libadwaita", "media-libs/gstreamer", "media-plugins/gst-plugins-base", "media-plugins/gst-plugins-good", "media-plugins/gst-plugins-bad", "media-plugins/gst-plugins-ugly", "media-plugins/gst-plugins-libav", "media-video/ffmpeg"],
            Distro::NixOS => &["mpv", "gtk4", "libadwaita", "gstreamer", "gst-plugins-base", "gst-plugins-good", "gst-plugins-bad", "gst-plugins-ugly", "gst-libav", "ffmpeg"],
            Distro::Unknown => &[],
        }
    }
}

/// Paket durumu
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageStatus {
    Installed,
    Missing,
    Unknown,
}

/// Bağımlılık kontrol sonucu
#[derive(Debug, Clone)]
pub struct DepCheckResult {
    pub distro: Distro,
    pub packages: Vec<(String, PackageStatus)>,
    pub all_installed: bool,
}

impl DepCheckResult {
    pub fn check() -> Self {
        let distro = Distro::detect();
        let pkgs = distro.packages();
        let mut results = Vec::new();
        let mut all_installed = true;

        for pkg in pkgs {
            let status = if Self::is_installed(distro, pkg) {
                PackageStatus::Installed
            } else {
                all_installed = false;
                PackageStatus::Missing
            };
            results.push((pkg.to_string(), status));
        }

        Self { distro, packages: results, all_installed }
    }

    fn is_installed(distro: Distro, pkg: &str) -> bool {
        // Önce which ile dene (binary için)
        if Command::new("which").arg(pkg).output().map(|o| o.status.success()).unwrap_or(false) {
            return true;
        }

        // Paket yöneticisi ile sorgula
        match distro {
            Distro::Arch => Command::new("pacman").args(["-Q", pkg]).output().map(|o| o.status.success()).unwrap_or(false),
            Distro::Fedora => Command::new("rpm").args(["-q", pkg]).output().map(|o| o.status.success()).unwrap_or(false),
            Distro::Debian => Command::new("dpkg").args(["-l", pkg]).output().map(|o| o.status.success()).unwrap_or(false),
            Distro::OpenSUSE => Command::new("rpm").args(["-q", pkg]).output().map(|o| o.status.success()).unwrap_or(false),
            Distro::Alpine => Command::new("apk").args(["info", "-e", pkg]).output().map(|o| o.status.success()).unwrap_or(false),
            Distro::Gentoo => Command::new("equery").args(["list", pkg]).output().map(|o| o.status.success()).unwrap_or(false),
            Distro::NixOS => Command::new("nix-env").args(["-q", pkg]).output().map(|o| o.status.success()).unwrap_or(false),
            Distro::Unknown => false,
        }
    }
}

/// Kurulum komutunu oluştur
pub fn build_install_command(distro: Distro) -> Option<String> {
    let (pm, args) = distro.package_manager();
    if pm.is_empty() {
        return None;
    }
    let pkgs = distro.packages().join(" ");
    match distro {
        Distro::Debian => Some(format!("{} {} {}", pm, args.join(" "), pkgs)),
        _ => Some(format!("{} {} {}", pm, args.join(" "), pkgs)),
    }
}

/// Terminalde kurulum çalıştır (pkexec/kdesudo/gnome-terminal)
pub fn run_install_in_terminal(distro: Distro, parent: &impl IsA<gtk::Window>) {
    let Some(cmd) = build_install_command(distro) else {
        show_error(parent, "Desteklenmeyen dağıtım", "Bu dağıtım için otomatik kurulum desteklenmiyor.");
        return;
    }

    // Terminal emulator bul
    let terminals = [
        ("gnome-terminal", &["--", "bash", "-c"]),
        ("konsole", &["-e", "bash", "-c"]),
        ("xterm", &["-e", "bash", "-c"]),
        ("kitty", &["-e", "bash", "-c"]),
        ("alacritty", &["-e", "bash", "-c"]),
        ("tilix", &["-e", "bash", "-c"]),
        ("mate-terminal", &["-e", "bash", "-c"]),
        ("xfce4-terminal", &["-e", "bash", "-c"]),
        ("terminator", &["-e", "bash", "-c"]),
        ("qterminal", &["-e", "bash", "-c"]),
    ];

    let term = terminals.iter().find(|(name, _)| {
        Command::new("which").arg(name).output().map(|o| o.status.success()).unwrap_or(false)
    });

    let (term_name, term_args) = match term {
        Some(t) => t,
        None => {
            show_error(parent, "Terminal bulunamadı", "Sisteminizde destekli bir terminal emulator bulunamadı.");
            return;
        }
    };

    // pkexec ile root yetkisi al
    let full_cmd = format!("pkexec bash -c '{}'", cmd);
    let mut args_vec: Vec<String> = term_args.iter().map(|s| s.to_string()).collect();
    args_vec.push(full_cmd);

    // Terminal'i başlat
    let child = Command::new(term_name).args(&args_vec).spawn();

    match child {
        Ok(_) => {
            show_info(parent, "Kurulum Başlatıldı", &format!("Terminal açıldı ({}). Şifre girip kurulumu onaylayın.", term_name));
        }
        Err(e) => {
            show_error(parent, "Terminal Başlatılamadı", &format!("Hata: {}", e));
        }
    }
}

fn show_info(parent: &impl IsA<gtk::Window>, heading: &str, body: &str) {
    let dialog = adw::MessageDialog::builder()
        .heading(heading)
        .body(body)
        .close_response("ok")
        .default_response("ok")
        .build();
    dialog.set_transient_for(Some(parent));
    dialog.add_response("ok", "Tamam");
    dialog.present();
}

fn show_error(parent: &impl IsA<gtk::Window>, heading: &str, body: &str) {
    let dialog = adw::MessageDialog::builder()
        .heading(heading)
        .body(body)
        .close_response("ok")
        .default_response("ok")
        .build();
    dialog.set_transient_for(Some(parent));
    dialog.add_response("ok", "Tamam");
    dialog.present();
}

/// Ayarlar sayfasına eklenecek installer UI'sını oluştur
pub fn build_installer_ui(parent_window: &impl IsA<gtk::Window>) -> gtk::Box {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 12);
    root.set_margin_top(12);
    root.set_margin_bottom(12);
    root.set_margin_start(16);
    root.set_margin_end(16);

    let group = adw::PreferencesGroup::new();
    group.set_title("Sistem Bağımlılıkları");

    let result = Rc::new(RefCell::new(DepCheckResult::check()));
    let result_clone = result.clone();

    // Dağıtım bilgi satırı
    let distro_row = adw::ActionRow::new();
    distro_row.set_title("Algılanan Dağıtım");
    distro_row.set_subtitle(result.borrow().distro.name());
    group.add(&distro_row);

    // Paket listesi
    let list_box = gtk::ListBox::new();
    list_box.set_selection_mode(gtk::SelectionMode::None);
    list_box.add_css_class("boxed-list");

    let refresh_list = {
        let list_box = list_box.clone();
        let result = result.clone();
        move || {
            // Listeyi temizle
            while let Some(child) = list_box.first_child() {
                list_box.remove(&child);
            }
            let res = result.borrow();
            for (pkg, status) in &res.packages {
                let row = adw::ActionRow::new();
                row.set_title(pkg);
                match status {
                    PackageStatus::Installed => {
                        row.set_subtitle("✓ Kurulu");
                        row.add_css_class("dim-label");
                    }
                    PackageStatus::Missing => {
                        row.set_subtitle("✗ Eksik");
                        row.add_css_class("error");
                    }
                    PackageStatus::Unknown => {
                        row.set_subtitle("? Bilinmiyor");
                        row.add_css_class("dim-label");
                    }
                }
                list_box.append(&row);
            }
        }
    };

    refresh_list();
    group.add(&list_box);

    // Butonlar
    let btn_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    btn_box.set_halign(gtk::Align::End);

    let check_btn = gtk::Button::with_label("Yeniden Kontrol Et");
    check_btn.add_css_class("pill");
    {
        let result = result.clone();
        let refresh_list = refresh_list.clone();
        check_btn.connect_clicked(move |_| {
            *result.borrow_mut() = DepCheckResult::check();
            distro_row.set_subtitle(result.borrow().distro.name());
            refresh_list();
        });
    }
    btn_box.append(&check_btn);

    let install_btn = gtk::Button::with_label("Eksikleri Kur");
    install_btn.add_css_class("suggested-action");
    install_btn.add_css_class("pill");
    install_btn.set_sensitive(!result.borrow().all_installed);
    {
        let result = result.clone();
        let install_btn = install_btn.clone();
        let parent_win = parent_window.clone();
        let refresh_list = refresh_list.clone();
        install_btn.connect_clicked(move |_| {
            let res = result.borrow();
            if res.all_installed {
                return;
            }
            let distro = res.distro;
            drop(res);
            run_install_in_terminal(distro, &parent_win);
            // Kurulum sonrası yeniden kontrol için biraz bekle
            let result2 = result.clone();
            let install_btn2 = install_btn.clone();
            let refresh_list2 = refresh_list.clone();
            glib::timeout_add_seconds_local(10, move || {
                *result2.borrow_mut() = DepCheckResult::check();
                let all = result2.borrow().all_installed;
                install_btn2.set_sensitive(!all);
                refresh_list2();
                glib::ControlFlow::Break
            });
        });
    }
    btn_box.append(&install_btn);

    group.add(&btn_box);
    root.append(&group);

    root
}