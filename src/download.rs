//! Bölüm indirme çekirdeği: streaming indirme, resume, kuyruk kalıcılığı.
//! API çözümleme Http katmanını kullanır; indirme ayrı streaming istemcidir
//! (büyük dosyayı RAM'e almaz). Duraklatma = iptal + `.part` üzerinden devam.

use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, AtomicU32, Ordering},
    mpsc, Arc,
};

pub(crate) const UA: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";
pub(crate) const CHUNK: usize = 128 * 1024;

#[derive(Clone, Debug, PartialEq)]
pub enum DownloadStatus {
    Queued,
    Downloading,
    Paused,
    Done,
    Error(String),
}

#[derive(Clone, Debug)]
pub struct DownloadRecord {
    pub id: String,
    pub title: String,
    /// Site başlık kimliği; yerelden oynatmada izleme geçmişi/ilerleme için.
    /// Eski kuyruk dosyalarında yok → 0 (o zaman yerelden oynatma sunulmaz).
    pub title_id: u64,
    pub season: u64,
    pub episode: u64,
    pub fansub: String,
    pub quality: String,
    pub url: String,
    pub referer: Option<String>,
    pub dest: PathBuf,
    pub total: u64,
    pub have: u64,
    pub status: DownloadStatus,
}

#[derive(Clone, Debug)]
pub enum DownloadEvent {
    Progress(u64, u64),
    Done,
    Cancelled,
    Error(String),
}

pub struct DownloadHandle {
    cancel: Arc<AtomicBool>,
}

impl DownloadHandle {
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub(crate) fn for_cancel(cancel: Arc<AtomicBool>) -> Self {
        Self { cancel }
    }
}

/// Dosya adı temizler: Türkçe korunur, ayırıcılar atılır, 100 karakter kap.
pub fn sanitize_filename(s: &str) -> String {
    let mut out: String = s
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | ' ' | '(' | ')' | '[' | ']') {
                c
            } else {
                '_'
            }
        })
        .collect();
    out = out.split_whitespace().collect::<Vec<_>>().join(" ");
    out = out.trim_matches(|c| c == '.' || c == ' ').to_string();
    const RESERVED: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7",
        "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    if RESERVED.contains(&out.to_uppercase().as_str()) {
        out.push('_');
    }
    if out.is_empty() {
        out.push_str("video");
    }
    // Karakter sınırında UTF-8 bütünlüğünü koru.
    let mut end = out.len().min(100);
    while !out.is_char_boundary(end) {
        end -= 1;
    }
    out.truncate(end);
    out.trim_end_matches(|c| c == '.' || c == ' ').to_string()
}

/// Bölüm dosya adı: `S01E02 - Ad [Fansub][1080p].mp4`.
pub fn episode_filename(
    season: u64,
    episode: u64,
    ep_name: &str,
    fansub: &str,
    quality: &str,
) -> String {
    let ep = sanitize_filename(ep_name);
    let fs = sanitize_filename(fansub);
    let q = sanitize_filename(quality);
    format!("S{season:02}E{episode:02} - {ep} [{fs}][{q}].mp4")
}

/// Resume başlığı (kalan bayt varsa).
pub fn range_header(have: u64) -> Option<String> {
    if have > 0 {
        Some(format!("bytes={have}-"))
    } else {
        None
    }
}

/// Hedef dizinde boş alan (bayt). Öğrenilemezse None.
pub fn free_space(path: &Path) -> Option<u64> {
    let anchor = if path.exists() { path.to_path_buf() } else { path.parent()?.to_path_buf() };
    let c = std::ffi::CString::new(anchor.to_string_lossy().as_bytes()).ok()?;
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut stat) } != 0 {
        return None;
    }
    (stat.f_bavail as u64).checked_mul(stat.f_frsize as u64)
}

/// Kalite etiketine göre bölüm başına kaba indirme tahmini (bayt).
/// Sunucu boyutu indirme başlayana kadar bilinmiyor; bu yüzden peşin uyarı
/// ancak bu mertebede olabilir.
pub fn est_episode_bytes(quality: &str) -> u64 {
    let q = quality.to_lowercase();
    if q.contains("2160") || q.contains("4k") {
        3_200_000_000
    } else if q.contains("720") {
        700_000_000
    } else if q.contains("480") {
        350_000_000
    } else {
        // 1080p ve tanımsız etiketler ("en iyi" vb.) üst banda varsayılır.
        1_300_000_000
    }
}

/// `dir` diskinde `need` bayt için yer var mı? Yoksa (boş alan, gereken) döner.
pub fn space_shortfall_bytes(dir: &Path, need: u64) -> Option<(u64, u64)> {
    if need == 0 {
        return None;
    }
    let free = free_space(dir)?;
    if free < need {
        Some((free, need))
    } else {
        None
    }
}

fn build_client_for(_url: &str) -> Result<reqwest::blocking::Client, String> {
    let b = reqwest::blocking::Client::builder().timeout(std::time::Duration::from_secs(30));
    b.build().map_err(|e| e.to_string())
}

/// İndirmeyi başlatır (arka plan thread). `.part` varsa kaldığı yerden devam.
/// `conns` = iç motorda eşzamanlı parça (segment) sayısı.
/// Dönüş: iptal kolu. Olaylar `tx` üzerinden akar.
pub fn start_download(
    url: &str,
    referer: Option<&str>,
    dest_final: &Path,
    tx: mpsc::Sender<DownloadEvent>,
    conns: u32,
) -> DownloadHandle {
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_r = cancel.clone();
    let url = url.to_string();
    let referer = referer.map(str::to_string);
    let dest_final = dest_final.to_path_buf();
    std::thread::spawn(move || {
        if let Some(parent) = dest_final.parent() {
            if std::fs::create_dir_all(parent).is_err() {
                let _ = tx.send(DownloadEvent::Error("klasör oluşturulamadı".into()));
                return;
            }
        }
        let part = dest_final.with_extension("mp4.part");
        let have = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
        let client = match build_client_for(&url) {
            Ok(c) => c,
            Err(e) => {
                let _ = tx.send(DownloadEvent::Error(format!("istemci: {e}")));
                return;
            }
        };
        // Temiz başlangıç + yeterince büyük + aralıklı sunucu → eşzamanlı bağlantılar.
        // Başarısızlıkta kısmi .part silinir, tek-bağlantıya düşülür (bozulma yok).
        if have == 0 {
            if let Some(total) = crate::segmented::probe_len(&client, &url, referer.as_deref()) {
                if total >= crate::segmented::MIN_SEGMENTED_BYTES {
                    let conns = conns.max(1) as usize;
                    eprintln!("[DL] {conns} bağlantı: {total} bayt");
                    let seg_tx = tx.clone();
                    let seg_progress = move |done: u64, tot: u64| {
                        let _ = seg_tx.send(DownloadEvent::Progress(done, tot));
                    };
                    match crate::segmented::download_segmented(
                        &client,
                        &url,
                        referer.as_deref(),
                        &part,
                        total,
                        conns,
                        &seg_progress,
                        &cancel_r,
                    ) {
                        Ok(()) => {
                            if std::fs::rename(&part, &dest_final).is_err() {
                                let _ = tx.send(DownloadEvent::Error(
                                    "tamamlama (rename) hatası".into(),
                                ));
                                return;
                            }
                            let _ = tx.send(DownloadEvent::Progress(total, total));
                            let _ = tx.send(DownloadEvent::Done);
                            return;
                        }
                        Err(e) => {
                            if cancel_r.load(Ordering::Relaxed) {
                                let _ = tx.send(DownloadEvent::Cancelled);
                                return;
                            }
                            eprintln!("[DL] parça başarısız, tekliye düşülüyor: {e}");
                            let _ = std::fs::remove_file(&part);
                        }
                    }
                }
            }
        }
        let mut req = client.get(&url).header("User-Agent", UA);
        if let Some(r) = &referer {
            req = req.header("Referer", r);
        }
        if let Some(range) = range_header(have) {
            req = req.header("Range", range);
        }
        let mut resp = match req.send() {
            Ok(r) => r,
            Err(e) => {
                let _ = tx.send(DownloadEvent::Error(format!("bağlantı: {e}")));
                return;
            }
        };
        // Sunucu aralığı yok saydıysa (200) baştan başla.
        let resume = resp.status() == reqwest::StatusCode::PARTIAL_CONTENT;
        if !(resp.status().is_success() || resume) {
            let _ = tx.send(DownloadEvent::Error(format!("HTTP {}", resp.status())));
            return;
        }
        let total = resp.content_length().unwrap_or(0);
        let total = if resume { total + have } else { have.max(0) + total };
        let mut file = match std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(!resume)
            .append(resume)
            .open(&part)
        {
            Ok(f) => f,
            Err(e) => {
                let _ = tx.send(DownloadEvent::Error(format!("dosya: {e}")));
                return;
            }
        };
        let mut done = if resume { have } else { 0 };
        if !resume && have > 0 {
            done = 0;
        }
        let _ = tx.send(DownloadEvent::Progress(done, total));
        use std::io::{Read, Write};
        let mut buf = vec![0u8; CHUNK];
        let mut since_report: u64 = 0;
        loop {
            if cancel_r.load(Ordering::Relaxed) {
                let _ = tx.send(DownloadEvent::Cancelled);
                return;
            }
            match resp.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if file.write_all(&buf[..n]).is_err() {
                        let _ = tx.send(DownloadEvent::Error("yazma hatası (disk dolu?)".into()));
                        return;
                    }
                    done += n as u64;
                    since_report += n as u64;
                    if since_report >= CHUNK as u64 * 4 {
                        since_report = 0;
                        let _ = tx.send(DownloadEvent::Progress(done, total));
                    }
                }
                Err(e) => {
                    let _ = tx.send(DownloadEvent::Error(format!("ağ: {e}")));
                    return;
                }
            }
        }
        let _ = file.flush();
        drop(file);
        if std::fs::rename(&part, &dest_final).is_err() {
            let _ = tx.send(DownloadEvent::Error("tamamlama (rename) hatası".into()));
            return;
        }
        let _ = tx.send(DownloadEvent::Progress(done, total.max(done)));
        let _ = tx.send(DownloadEvent::Done);
    });
    DownloadHandle { cancel }
}

/// sibnet mp4 için oynatmayla aynı Referer kuralı.
pub fn sibnet_referer(mp4_url: &str) -> Option<String> {
    if !mp4_url.contains("video.sibnet.ru/v/") {
        return None;
    }
    let vid = mp4_url
        .split("/v/")
        .nth(1)
        .and_then(|s| s.split('/').nth(1))
        .map(|s| s.trim_end_matches(".mp4"))
        .unwrap_or("");
    Some(if vid.is_empty() {
        "https://video.sibnet.ru/".to_string()
    } else {
        format!("https://video.sibnet.ru/shell.php?videoid={vid}")
    })
}

/// Kayıt kurar (saf: alan yerleşimi testle kilitli).
/// `label` görünen kalite, `url` indirilen adres olmalıdır.
pub fn build_record(
    base_dir: &Path,
    series: &str,
    title_id: u64,
    ep: &crate::api::Episode,
    fs: &crate::api::FansubInfo,
    label: &str,
    url: &str,
) -> DownloadRecord {
    let label = if label.is_empty() { "en iyi".to_string() } else { label.to_string() };
    let dest = base_dir.join(series).join(episode_filename(
        ep.season,
        ep.episode,
        &ep.name,
        &fs.name,
        &label,
    ));
    DownloadRecord {
        id: format!("{series}:S{:02}E{:02}:{}", ep.season, ep.episode, fs.template_id),
        title: series.to_string(),
        title_id,
        season: ep.season,
        episode: ep.episode,
        fansub: fs.name.clone(),
        quality: label,
        url: url.to_string(),
        referer: sibnet_referer(url),
        dest,
        total: 0,
        have: 0,
        status: DownloadStatus::Queued,
    }
}
/// İlk çözülebilen ayna kazanır; hiçbiri olmazsa Err.
/// NOT: `resolve_mirror_quality` (etiket, url) sırasıyla döner.
/// Katı kip: istenen kalite yoksa üst kaliteye sessizce çıkılmaz, Err döner.
pub fn resolve_for_download(
    client: &crate::api::Client,
    base_dir: &Path,
    series: &str,
    title_id: u64,
    ep: &crate::api::Episode,
    fs: &crate::api::FansubInfo,
    quality: &str,
) -> Result<DownloadRecord, String> {
    let mut last_err = "ayna yok".to_string();
    for m in &fs.mirrors {
        match client.resolve_mirror_quality_strict(&m.url, quality) {
            Ok((label, url)) => {
                let label = if label.is_empty() { quality.to_string() } else { label };
                return Ok(build_record(base_dir, series, title_id, ep, fs, &label, &url));
            }
            Err(e) => last_err = e,
        }
    }
    Err(last_err)
}

/// Kuyruk kalıcılığı (JSON). Yol uygulamadan verilir (test edilebilirlik).
pub fn save_queue(path: &Path, items: &[DownloadRecord]) -> Result<(), String> {
    let v: Vec<serde_json::Value> = items
        .iter()
        .map(|r| {
            let status = match &r.status {
                DownloadStatus::Queued => "queued".to_string(),
                DownloadStatus::Downloading => "downloading".to_string(),
                DownloadStatus::Paused => "paused".to_string(),
                DownloadStatus::Done => "done".to_string(),
                DownloadStatus::Error(e) => format!("error:{e}"),
            };
            serde_json::json!({
                "id": r.id, "title": r.title, "title_id": r.title_id,
                "season": r.season, "episode": r.episode,
                "fansub": r.fansub, "quality": r.quality, "url": r.url,
                "referer": r.referer, "dest": r.dest.to_string_lossy(),
                "total": r.total, "have": r.have, "status": status,
            })
        })
        .collect();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}

pub fn load_queue(path: &Path) -> Vec<DownloadRecord> {
    let Ok(bytes) = std::fs::read(path) else { return Vec::new() };
    let Ok(v) = serde_json::from_slice::<Vec<serde_json::Value>>(&bytes) else { return Vec::new() };
    v.iter()
        .filter_map(|r| {
            let status = match r["status"].as_str().unwrap_or("queued") {
                "done" => DownloadStatus::Done,
                "paused" => DownloadStatus::Paused,
                s if s.starts_with("error:") => {
                    DownloadStatus::Error(s.trim_start_matches("error:").to_string())
                }
                _ => DownloadStatus::Queued,
            };
            // Yarım kalan indirme kuyruğa döner (devam eder).
            let status = match status {
                DownloadStatus::Downloading => DownloadStatus::Queued,
                s => s,
            };
            // Eski bozuk kayıt: kalite alanına URL kaçmışsa temizle.
            let mut quality = r["quality"].as_str().unwrap_or("").to_string();
            if quality.contains("://") {
                quality = "en iyi".to_string();
            }
            Some(DownloadRecord {
                id: r["id"].as_str().unwrap_or("").to_string(),
                title: r["title"].as_str().unwrap_or("").to_string(),
                title_id: r["title_id"].as_u64().unwrap_or(0),
                season: r["season"].as_u64().unwrap_or(0),
                episode: r["episode"].as_u64().unwrap_or(0),
                fansub: r["fansub"].as_str().unwrap_or("").to_string(),
                quality,
                url: r["url"].as_str().unwrap_or("").to_string(),
                referer: r["referer"].as_str().map(str::to_string),
                dest: PathBuf::from(r["dest"].as_str().unwrap_or("")),
                total: r["total"].as_u64().unwrap_or(0),
                have: r["have"].as_u64().unwrap_or(0),
                status,
            })
        })
        .collect()
}

/// Bayt gösterimi (B/KB/MB/GB).
pub fn fmt_bytes(n: u64) -> String {
    const KB: f64 = 1024.0;
    let f = n as f64;
    if f < KB {
        format!("{n} B")
    } else if f < KB * KB {
        format!("{:.1} KB", f / KB)
    } else if f < KB * KB * KB {
        format!("{:.1} MB", f / (KB * KB))
    } else {
        format!("{:.2} GB", f / (KB * KB * KB))
    }
}

/// Uygulama veri dizini (`~/.local/share/animecix`).
pub fn app_data_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join(".local/share/animecix")
}

pub fn queue_file_path() -> PathBuf {
    app_data_dir().join("downloads.json")
}

/// Varsayılan indirme klasörü (Videolar/Animecix, yoksa ~/Videos/Animecix).
pub fn default_download_dir() -> PathBuf {
    let base = glib::user_special_dir(glib::UserDirectory::Videos)
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string())).join("Videos")
        });
    base.join("Animecix")
}

/// Arayüz olayları (pompa arayüze taşır).
#[derive(Clone, Debug)]
pub enum UiEvent {
    Tick,
    /// Yapısal kir (ekle/duraklat/devam/kaldır/başla/bit/hata): sayfa yeniden kurulur.
    Changed,
    Toast(String),
}

/// Sıralı (tek tek) indirme yöneticisi. Klonlanabilir (paylaşımlı iç durum).
#[derive(Clone)]
pub struct DownloadManager {
    inner: Arc<ManagerInner>,
}

struct ManagerInner {
    queue: std::sync::Mutex<Vec<DownloadRecord>>,
    handles: std::sync::Mutex<std::collections::HashMap<String, DownloadHandle>>,
    worker_on: AtomicBool,
    /// Dosya başına eşzamanlı bağlantı/segment sayısı (kullanıcı ayarı).
    conns: AtomicU32,
    queue_path: PathBuf,
    ui_tx: mpsc::Sender<UiEvent>,
}

impl DownloadManager {
    /// Kuyruğu diskten yükler. `ui_tx` pompa kanalına bağlanır.
    pub fn new(queue_path: PathBuf, ui_tx: mpsc::Sender<UiEvent>) -> Self {
        let mut items = load_queue(&queue_path);
        // Yarım kalanların konumunu .part'tan tazele.
        for r in &mut items {
            if !matches!(r.status, DownloadStatus::Done) {
                let part = r.dest.with_extension("mp4.part");
                r.have = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
            }
        }
        let m = Self {
            inner: Arc::new(ManagerInner {
                queue: std::sync::Mutex::new(items),
                handles: std::sync::Mutex::new(std::collections::HashMap::new()),
                worker_on: AtomicBool::new(false),
                conns: AtomicU32::new(crate::aria::SEGMENTS),
                queue_path,
                ui_tx,
            }),
        };
        m.save();
        m
    }

    /// Dosya başına bağlantı sayısı (1..=MAX_CONNECTIONS); sonraki indirmelerde etkili.
    pub fn set_connections(&self, n: u32) {
        self.inner.conns.store(n.clamp(1, crate::aria::MAX_CONNECTIONS), Ordering::Relaxed);
    }

    pub fn connections(&self) -> u32 {
        self.inner.conns.load(Ordering::Relaxed).clamp(1, crate::aria::MAX_CONNECTIONS)
    }

    fn save(&self) {
        if let Ok(q) = self.inner.queue.lock() {
            let _ = save_queue(&self.inner.queue_path, &q);
        }
    }

    fn emit(&self, ev: UiEvent) {
        let _ = self.inner.ui_tx.send(ev);
    }

    pub fn snapshot(&self) -> Vec<DownloadRecord> {
        self.inner.queue.lock().map(|q| q.clone()).unwrap_or_default()
    }

    /// Kuyruğa ekler (aynı hedef zaten sıradaysa tekrar eklemez).
    /// `quiet`: toplu eklemelerde pompa bildirimini susturur.
    pub fn enqueue(&self, mut rec: DownloadRecord, quiet: bool) {
        let mut q = match self.inner.queue.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        let dup = q.iter().any(|r| {
            r.dest == rec.dest
                && !matches!(r.status, DownloadStatus::Done | DownloadStatus::Error(_))
        });
        if dup {
            return;
        }
        rec.status = DownloadStatus::Queued;
        rec.have = std::fs::metadata(rec.dest.with_extension("mp4.part"))
            .map(|m| m.len())
            .unwrap_or(0);
        q.push(rec);
        drop(q);
        self.save();
        self.emit(UiEvent::Changed);
        if !quiet {
            self.emit(UiEvent::Toast("Kuyruğa eklendi".to_string()));
        }
        self.ensure_worker();
    }

    /// Duraklatır (`.part` kalır, devam eder).
    pub fn pause(&self, id: &str) {
        if let Ok(h) = self.inner.handles.lock() {
            if let Some(handle) = h.get(id) {
                handle.cancel();
            }
        }
        if let Ok(mut q) = self.inner.queue.lock() {
            if let Some(r) = q.iter_mut().find(|r| r.id == id) {
                if matches!(r.status, DownloadStatus::Downloading | DownloadStatus::Queued) {
                    r.status = DownloadStatus::Paused;
                }
            }
        }
        self.save();
        self.emit(UiEvent::Changed);
    }

    /// Devam ettirir.
    pub fn resume(&self, id: &str) {
        if let Ok(mut q) = self.inner.queue.lock() {
            if let Some(r) = q.iter_mut().find(|r| r.id == id) {
                if matches!(r.status, DownloadStatus::Paused | DownloadStatus::Error(_)) {
                    r.status = DownloadStatus::Queued;
                }
            }
        }
        self.save();
        self.emit(UiEvent::Changed);
        self.ensure_worker();
    }

    /// Durabilmek olan tüm kayıtları (indiriliyor + sırada) duraklatır; sayı döner.
    pub fn pause_all(&self) -> usize {
        let Ok(mut q) = self.inner.queue.lock() else {
            return 0;
        };
        let running: Vec<String> = q
            .iter()
            .filter(|r| matches!(r.status, DownloadStatus::Downloading))
            .map(|r| r.id.clone())
            .collect();
        let mut n = 0;
        for r in q.iter_mut() {
            if matches!(r.status, DownloadStatus::Downloading | DownloadStatus::Queued) {
                r.status = DownloadStatus::Paused;
                n += 1;
            }
        }
        drop(q);
        if let Ok(h) = self.inner.handles.lock() {
            for id in &running {
                if let Some(handle) = h.get(id) {
                    handle.cancel();
                }
            }
        }
        if n > 0 {
            self.save();
            self.emit(UiEvent::Changed);
        }
        n
    }

    /// Duraklatılmış ve hatalı kayıtları sıraya geri koyar; sayı döner.
    pub fn resume_all(&self) -> usize {
        let mut n = 0;
        if let Ok(mut q) = self.inner.queue.lock() {
            for r in q.iter_mut() {
                if matches!(r.status, DownloadStatus::Paused | DownloadStatus::Error(_)) {
                    r.status = DownloadStatus::Queued;
                    n += 1;
                }
            }
        }
        if n > 0 {
            self.save();
            self.emit(UiEvent::Changed);
            self.ensure_worker();
        }
        n
    }

    /// Kayıttan kaldırır (bitmemişse `.part`/aria artıkları silinir, bitmiş dosya korunur).
    pub fn remove(&self, id: &str) {
        if let Ok(h) = self.inner.handles.lock() {
            if let Some(handle) = h.get(id) {
                handle.cancel();
            }
        }
        if let Ok(mut q) = self.inner.queue.lock() {
            if let Some(pos) = q.iter().position(|r| r.id == id) {
                let r = q.remove(pos);
                if !matches!(r.status, DownloadStatus::Done) {
                    let _ = std::fs::remove_file(r.dest.with_extension("mp4.part"));
                    // aria artığı: .aria2 kontrolü varsa bitmemiş dest de onundur.
                    let ctl = PathBuf::from(format!("{}.aria2", r.dest.display()));
                    if ctl.exists() {
                        let _ = std::fs::remove_file(&ctl);
                        let _ = std::fs::remove_file(&r.dest);
                    }
                }
            }
        }
        if let Ok(mut h) = self.inner.handles.lock() {
            h.remove(id);
        }
        self.save();
        self.emit(UiEvent::Changed);
    }

    /// Kaydı KALDIRIR ve diskteki dosyayı (ve .part/.aria2 artıklarını) siler.
    /// Herhangi bir dosya silinemezse Err döner ve kayıt olduğu yerde kalır.
    pub fn remove_with_file(&self, id: &str) -> Result<(), String> {
        let dest = {
            let Ok(q) = self.inner.queue.lock() else {
                return Err("kuyruk kilitlenemedi".to_string());
            };
            q.iter().find(|r| r.id == id).map(|r| r.dest.clone())
        };
        let Some(dest) = dest else {
            return Err("kayıt bulunamadı".to_string());
        };
        let ctl = PathBuf::from(format!("{}.aria2", dest.display()));
        let mut kalan = Vec::new();
        for p in [dest.clone(), dest.with_extension("mp4.part"), ctl] {
            if p.is_file() {
                if let Err(e) = std::fs::remove_file(&p) {
                    kalan.push(format!("{}: {e}", p.display()));
                }
            }
        }
        if !kalan.is_empty() {
            return Err(kalan.join(" · "));
        }
        self.remove(id);
        Ok(())
    }

    /// Bölümün diskteki tamamlanmış dosyası; yoksa None. `series` = sanitize_filename(başlık adı)
    /// (kayıttaki `title` alanıyla birebir aynı biçimde).
    pub fn local_file(&self, series: &str, season: u64, episode: u64) -> Option<PathBuf> {
        let q = self.inner.queue.lock().ok()?;
        q.iter()
            .find(|r| {
                r.title == series
                    && r.season == season
                    && r.episode == episode
                    && matches!(r.status, DownloadStatus::Done)
            })
            .map(|r| r.dest.clone())
            .filter(|p| p.is_file())
    }

    /// Bitmiş (Done) ve hatalı kayıtları listeden çıkarır; diskteki dosyalara dokunmaz.
    /// Kaç kayıt gittiğini döner.
    pub fn clear_finished(&self) -> usize {
        let mut n = 0;
        if let Ok(mut q) = self.inner.queue.lock() {
            let before = q.len();
            q.retain(|r| {
                !matches!(r.status, DownloadStatus::Done | DownloadStatus::Error(_))
            });
            n = before - q.len();
        }
        if n > 0 {
            self.save();
            self.emit(UiEvent::Changed);
        }
        n
    }

    fn set_status(&self, id: &str, status: DownloadStatus) {
        if let Ok(mut q) = self.inner.queue.lock() {
            if let Some(r) = q.iter_mut().find(|r| r.id == id) {
                r.status = status;
            }
        }
    }

    fn set_progress(&self, id: &str, have: u64, total: u64) {
        if let Ok(mut q) = self.inner.queue.lock() {
            if let Some(r) = q.iter_mut().find(|r| r.id == id) {
                r.have = have;
                if total > 0 {
                    r.total = total;
                }
            }
        }
    }

    fn ensure_worker(&self) {
        if self.inner.worker_on.swap(true, Ordering::SeqCst) {
            return;
        }
        let this = self.clone();
        std::thread::spawn(move || {
            let mut last_tick = std::time::Instant::now();
            let mut tick = |force: bool| {
                if force || last_tick.elapsed() >= std::time::Duration::from_secs(1) {
                    last_tick = std::time::Instant::now();
                    this.emit(UiEvent::Tick);
                }
            };
            loop {
                let job = this.inner.queue.lock().map(|mut q| {
                    q.iter_mut()
                        .find(|r| matches!(r.status, DownloadStatus::Queued))
                        .map(|r| {
                            r.status = DownloadStatus::Downloading;
                            r.clone()
                        })
                });
                let rec = match job {
                    Ok(Some(r)) => r,
                    _ => break,
                };
                this.save();
                this.emit(UiEvent::Changed);
                this.run_job(rec, &mut tick);
            }
            this.inner.worker_on.store(false, Ordering::SeqCst);
            // Yarış: kuyruk kapanıştan sonra dolduysa yeniden başlat.
            let pending = this
                .inner
                .queue
                .lock()
                .map(|q| q.iter().any(|r| matches!(r.status, DownloadStatus::Queued)))
                .unwrap_or(false);
            if pending {
                this.ensure_worker();
            }
        });
    }

    /// Tek kaydın indirmesini yürütür; olaylar bitene kadar izlenir.
    fn run_job(&self, rec: DownloadRecord, tick: &mut impl FnMut(bool)) {
        let dest_dir = rec
            .dest
            .parent()
            .unwrap_or(std::path::Path::new("/"))
            .to_path_buf();
        // Kalan veri kadar yer kaldığını doğrula; ilk Progress'te çalışır,
        // sonra 5 saniyede bir (disk yavaş yavaş dolabilir).
        let mut last_space_check: Option<std::time::Instant> = None;
        let (tx, rx) = mpsc::channel();
        let conns = self.connections();
        // aria2c varsa çok bağlantı, yoksa iç motor (imza/olay sözleşmesi aynı).
        let handle = if crate::aria::find_aria2c().is_some() {
            crate::aria::start_download_aria(&rec.url, rec.referer.as_deref(), &rec.dest, tx, conns)
        } else {
            start_download(&rec.url, rec.referer.as_deref(), &rec.dest, tx, conns)
        };
        if let Ok(mut h) = self.inner.handles.lock() {
            h.insert(rec.id.clone(), handle);
        }
        for ev in rx.iter() {
            match ev {
                DownloadEvent::Progress(a, b) => {
                    self.set_progress(&rec.id, a, b);
                    let due = last_space_check
                        .map(|t| t.elapsed() >= std::time::Duration::from_secs(5))
                        .unwrap_or(true);
                    if due {
                        last_space_check = Some(std::time::Instant::now());
                        // Kalan + 64 MB pay; boyut bilinmiyorsa (b=0) kontrol edilmez.
                        let need = b.saturating_sub(a).saturating_add(64 * 1024 * 1024);
                        if let Some((free, need)) = space_shortfall_bytes(&dest_dir, need) {
                            let msg = format!(
                                "Diskte yer yok: ~{} gerekiyor, {} boş",
                                fmt_bytes(need),
                                fmt_bytes(free)
                            );
                            eprintln!("[DL] {}: {}", rec.id, msg);
                            if let Ok(h) = self.inner.handles.lock() {
                                if let Some(h) = h.get(&rec.id) {
                                    h.cancel();
                                }
                            }
                            self.set_status(&rec.id, DownloadStatus::Error(msg.clone()));
                            self.save();
                            self.emit(UiEvent::Changed);
                            self.emit(UiEvent::Toast(msg));
                            break;
                        }
                    }
                    tick(false);
                }
                DownloadEvent::Done => {
                    self.set_status(&rec.id, DownloadStatus::Done);
                    self.set_progress(&rec.id, rec.total.max(1), rec.total.max(1));
                    // Gerçek boyutu diskten al.
                    if let Ok(m) = std::fs::metadata(&rec.dest) {
                        let len = m.len();
                        self.set_progress(&rec.id, len, len);
                    }
                    self.save();
                    self.emit(UiEvent::Changed);
                    self.emit(UiEvent::Toast(format!("İndi: {}", rec.dest.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())));
                    break;
                }
                DownloadEvent::Cancelled => {
                    self.set_progress(
                        &rec.id,
                        std::fs::metadata(rec.dest.with_extension("mp4.part")).map(|m| m.len()).unwrap_or(0),
                        rec.total,
                    );
                    self.set_status(&rec.id, DownloadStatus::Paused);
                    self.save();
                    self.emit(UiEvent::Changed);
                    break;
                }
                DownloadEvent::Error(e) => {
                    self.set_status(&rec.id, DownloadStatus::Error(e.clone()));
                    self.save();
                    self.emit(UiEvent::Changed);
                    self.emit(UiEvent::Toast(format!("İndirme hatası: {e}")));
                    break;
                }
            }
        }
        if let Ok(mut h) = self.inner.handles.lock() {
            h.remove(&rec.id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_filename_keeps_turkish_drops_separators() {
        assert_eq!(sanitize_filename("Gün Batımı: Bölüm/1?"), "Gün Batımı_ Bölüm_1_");
        assert_eq!(sanitize_filename("Kaoru Hana wa Rin to Saku"), "Kaoru Hana wa Rin to Saku");
        assert_eq!(sanitize_filename("CON"), "CON_");
        assert_eq!(sanitize_filename("..."), "video");
        assert_eq!(sanitize_filename(""), "video");
        let long = "a".repeat(200);
        assert!(sanitize_filename(&long).len() <= 100);
        assert_eq!(
            episode_filename(1, 7, "Gün Batımı", "RaionSubs", "1080p"),
            "S01E07 - Gün Batımı [RaionSubs][1080p].mp4"
        );
    }

    fn test_ep() -> crate::api::Episode {
        crate::api::Episode { season: 1, episode: 2, name: "Bölüm".into(), thumbnail: None }
    }

    fn test_fs() -> crate::api::FansubInfo {
        crate::api::FansubInfo {
            template_id: 7,
            name: "FS".into(),
            rating: 9.0,
            total_votes: 0,
            language: "tr".into(),
            approved_only: true,
            mirror_count: 1,
            hosts: Vec::new(),
            mirrors: Vec::new(),
        }
    }

    #[test]
    fn build_record_field_placement_regression() {
        // Regresyon: (etiket, url) sırası karışırsa kaliteye URL, url'ye
        // etiket düşer ve indirme "builder error" verir.
        let r = build_record(Path::new("/tmp/x"), "Dizi", 7, &test_ep(), &test_fs(), "1080p", "https://cdn/v.mp4");
        assert_eq!(r.quality, "1080p", "kalite etikettir: {}", r.quality);
        assert_eq!(r.url, "https://cdn/v.mp4", "url adrestir: {}", r.url);
        assert!(r.dest.to_string_lossy().ends_with("[1080p].mp4"), "dosya adı: {}", r.dest.display());
        let r2 = build_record(Path::new("/tmp/x"), "Dizi", 7, &test_ep(), &test_fs(), "", "https://cdn/v.mp4");
        assert_eq!(r2.quality, "en iyi");
        assert_eq!(r2.url, "https://cdn/v.mp4");
    }

    #[test]
    fn load_queue_sanitizes_poisoned_quality() {
        let dir = std::env::temp_dir().join("animecix-dl-poison");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("q.json"),
            r#"[{"id":"a","title":"T","season":1,"episode":1,"fansub":"F","quality":"https://cdn/v.mp4","url":"1080p","referer":null,"dest":"/tmp/x.mp4","total":0,"have":0,"status":"error:x"}]"#,
        )
        .unwrap();
        let back = load_queue(&dir.join("q.json"));
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].quality, "en iyi", "zehirli kalite temizlenmeli");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sibnet_referer_shapes() {        assert_eq!(
            sibnet_referer("https://video.sibnet.ru/v/abc123/1.mp4").as_deref(),
            Some("https://video.sibnet.ru/shell.php?videoid=1")
        );
        assert!(sibnet_referer("https://ornek/v.mp4").is_none());
    }

    #[test]
    fn fmt_bytes_shapes() {        assert_eq!(fmt_bytes(0), "0 B");
        assert_eq!(fmt_bytes(512), "512 B");
        assert_eq!(fmt_bytes(1536), "1.5 KB");
        assert_eq!(fmt_bytes(5 * 1024 * 1024), "5.0 MB");
        assert_eq!(fmt_bytes(3 * 1024 * 1024 * 1024), "3.00 GB");
    }

    #[test]
    fn range_header_shapes() {        assert_eq!(range_header(0), None);
        assert_eq!(range_header(12345).as_deref(), Some("bytes=12345-"));
    }

    #[test]
    fn space_estimates_and_shortfall() {
        // Kalite büyüdükçe tahmin büyür; 1080p ve tanımsız etiket aynı üst bantta.
        assert!(est_episode_bytes("480p") < est_episode_bytes("720p"));
        assert!(est_episode_bytes("720p") < est_episode_bytes("1080p"));
        assert!(est_episode_bytes("1080p") < est_episode_bytes("2160p"));
        assert_eq!(est_episode_bytes("en iyi"), est_episode_bytes("1080p"));
        // İstenmeyen yer yoksa sorun yok; ölçülemeyen yol sessiz geçer.
        assert_eq!(space_shortfall_bytes(Path::new("/tmp"), 0), None);
        assert_eq!(space_shortfall_bytes(Path::new(""), 1 << 40), None);
        assert_eq!(space_shortfall_bytes(Path::new("/tmp"), 1024), None);
        // İmkânsız ihtiyaç → (boş, gereken).
        let s = space_shortfall_bytes(Path::new("/tmp"), u64::MAX / 2).expect("yer yetmemeli");
        assert_eq!(s.1, u64::MAX / 2);
        assert!(s.0 < s.1);
    }

    #[test]
    fn free_space_tmp_is_sane() {
        let f = free_space(Path::new("/tmp")).expect("/tmp ölçülebilmeli");
        assert!(f > 1024 * 1024, "anlamsız boş alan: {f}");
    }

    #[test]
    fn queue_roundtrip() {
        let dir = std::env::temp_dir().join("animecix-dl-test");
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("downloads.json");
        let rec = DownloadRecord {
            id: "t1".into(),
            title: "Frieren".into(),
            title_id: 3,
            season: 1,
            episode: 7,
            fansub: "Raion".into(),
            quality: "1080p".into(),
            url: "https://ornek/v.mp4".into(),
            referer: None,
            dest: dir.join("v.mp4"),
            total: 100,
            have: 40,
            status: DownloadStatus::Downloading,
        };
        save_queue(&path, &[rec]).expect("kayıt");
        let back = load_queue(&path);
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].episode, 7);
        assert_eq!(back[0].title_id, 3);
        // Devam eden, kuyruğa döner.
        assert_eq!(back[0].status, DownloadStatus::Queued);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Yerel mini sunucu: tam indirme + progress + resume (206) kanıtı.    #[test]
    fn download_full_and_resume_against_local_server() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::sync::atomic::{AtomicUsize, Ordering as O};

        static BODY_LEN: usize = 300_000;
        let listener = TcpListener::bind("127.0.0.1:0").expect("dinle");
        let port = listener.local_addr().unwrap().port();
        let hits = Arc::new(AtomicUsize::new(0));
        let hits_r = hits.clone();
        let server = std::thread::spawn(move || {
            for stream in listener.incoming().take(2) {
                let Ok(mut s) = stream else { continue };
                hits_r.fetch_add(1, O::Relaxed);
                let mut buf = vec![0u8; 4096];
                let n = s.read(&mut buf).unwrap_or(0);
                let req: String = String::from_utf8_lossy(&buf[..n]).into_owned();
                let range = req
                    .lines()
                    .find_map(|l| l.strip_prefix("Range: bytes="))
                    .and_then(|v| v.trim_end_matches(['\r', '-']).parse::<usize>().ok());
                let (status, start) = match range {
                    Some(st) if st < BODY_LEN => ("206 Partial Content", st),
                    _ => ("200 OK", 0),
                };
                let body_len = BODY_LEN - start;
                let head = if status.starts_with("206") {
                    format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Length: {body_len}\r\nContent-Range: bytes {start}-{}/{BODY_LEN}\r\nConnection: close\r\n\r\n",
                        BODY_LEN - 1
                    )
                } else {
                    format!("HTTP/1.1 200 OK\r\nContent-Length: {body_len}\r\nConnection: close\r\n\r\n")
                };
                let _ = s.write_all(head.as_bytes());
                let body = vec![0xABu8; body_len];
                let _ = s.write_all(&body);
            }
        });

        let dir = std::env::temp_dir().join("animecix-dl-live");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("v.mp4");
        let url = format!("http://127.0.0.1:{port}/v.mp4");

        // 1) tam indirme.
        let (tx, rx) = mpsc::channel();
        let _h = start_download(&url, None, &dest, tx, crate::aria::SEGMENTS);
        let mut last = (0u64, 0u64);
        let mut done = false;
        for ev in rx.iter() {
            match ev {
                DownloadEvent::Progress(a, b) => last = (a, b),
                DownloadEvent::Done => { done = true; break; }
                DownloadEvent::Error(e) => panic!("indirme hatası: {e}"),
                DownloadEvent::Cancelled => panic!("beklenmedik iptal"),
            }
        }
        assert!(done);
        assert_eq!(last, (BODY_LEN as u64, BODY_LEN as u64), "progress: {last:?}");
        assert_eq!(std::fs::metadata(&dest).unwrap().len(), BODY_LEN as u64);

        // 2) yarım .part + resume.
        std::fs::remove_file(&dest).unwrap();
        std::fs::write(dest.with_extension("mp4.part"), vec![0xABu8; 100_000]).unwrap();
        let (tx2, rx2) = mpsc::channel();
        let _h2 = start_download(&url, None, &dest, tx2, crate::aria::SEGMENTS);
        let mut done2 = false;
        for ev in rx2.iter() {
            match ev {
                DownloadEvent::Done => { done2 = true; break; }
                DownloadEvent::Error(e) => panic!("resume hatası: {e}"),
                _ => {}
            }
        }
        assert!(done2);
        assert_eq!(std::fs::metadata(&dest).unwrap().len(), BODY_LEN as u64);
        assert!(hits.load(O::Relaxed) >= 2);
        let _ = server.join();
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn test_record(id: &str, dest: PathBuf) -> DownloadRecord {
        DownloadRecord {
            id: id.into(),
            title: "T".into(),
            title_id: 7,
            season: 1,
            episode: 1,
            fansub: "F".into(),
            quality: "1080p".into(),
            url: "http://127.0.0.1:9/v.mp4".into(),
            referer: None,
            dest,
            total: 0,
            have: 0,
            status: DownloadStatus::Queued,
        }
    }

    #[test]
    fn manager_dedup_and_remove() {
        let dir = std::env::temp_dir().join("animecix-dl-mgr");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (tx, _rx) = mpsc::channel();
        let m = DownloadManager::new(dir.join("q.json"), tx);
        let dest = dir.join("v.mp4");
        m.enqueue(test_record("a", dest.clone()), false);
        m.enqueue(test_record("b", dest.clone()), true);
        assert_eq!(m.snapshot().len(), 1, "aynı hedef tekrar eklenmemeli");
        // Sahte .part ile kaldırma temizliği.
        std::fs::write(dest.with_extension("mp4.part"), vec![0u8; 10]).unwrap();
        m.remove("a");
        assert!(m.snapshot().is_empty());
        assert!(!dest.with_extension("mp4.part").exists(), ".part silinmeli");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn manager_clear_finished_keeps_active_and_files() {
        let dir = std::env::temp_dir().join("animecix-dl-clear");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let qpath = dir.join("q.json");

        let done = dir.join("done.mp4");
        std::fs::write(&done, b"x").unwrap();
        let mut r_done = test_record("d", done.clone());
        r_done.status = DownloadStatus::Done;
        let mut r_err = test_record("e", dir.join("err.mp4"));
        r_err.status = DownloadStatus::Error("koptu".into());
        let r_active = test_record("a", dir.join("act.mp4"));
        save_queue(&qpath, &[r_done, r_err, r_active]).unwrap();

        let (tx, _rx) = mpsc::channel();
        let m = DownloadManager::new(qpath, tx);
        assert_eq!(m.snapshot().len(), 3);

        assert_eq!(m.clear_finished(), 2, "Done + Error gitti");
        let left = m.snapshot();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].id, "a");
        assert!(done.exists(), "indirilen dosya silinmemeli");
        assert_eq!(m.clear_finished(), 0, "temizlenecek bir şey kalmadı");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn manager_remove_with_file_deletes_record_and_file() {
        let dir = std::env::temp_dir().join("animecix-dl-delfile");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let qpath = dir.join("q.json");

        let dest = dir.join("bolum.mp4");
        let mut r_done = test_record("d", dest.clone());
        r_done.status = DownloadStatus::Done;
        let part = dir.join("yarim.mp4.part");
        let mut r_paused = test_record("p", dir.join("yarim.mp4"));
        r_paused.status = DownloadStatus::Paused;
        save_queue(&qpath, &[r_done, r_paused]).unwrap();
        std::fs::write(&dest, b"veri").unwrap();
        std::fs::write(&part, b"yarim").unwrap();

        let (tx, _rx) = mpsc::channel();
        let m = DownloadManager::new(qpath, tx);
        assert!(m.remove_with_file("yok").is_err(), "olmayan kayıt");
        assert!(m.remove_with_file("d").is_ok());
        assert!(!dest.exists(), "dosya silinmeli");
        assert!(part.exists(), "diğer kaydın artığına dokunulmamalı");
        let left = m.snapshot();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].id, "p");
        // .part artığı da kayıtla birlikte gider.
        assert!(m.remove_with_file("p").is_ok());
        assert!(!part.exists());
        assert!(m.snapshot().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn manager_connections_setting_clamps_to_bounds() {
        let dir = std::env::temp_dir().join("animecix-dl-conns");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (tx, _rx) = mpsc::channel();
        let m = DownloadManager::new(dir.join("q.json"), tx);
        assert_eq!(m.connections(), crate::aria::SEGMENTS);
        m.set_connections(12);
        assert_eq!(m.connections(), 12);
        m.set_connections(0);
        assert_eq!(m.connections(), 1);
        m.set_connections(u32::MAX);
        assert_eq!(m.connections(), crate::aria::MAX_CONNECTIONS);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn manager_pause_all_and_resume_all_count() {
        let dir = std::env::temp_dir().join("animecix-dl-pauseall");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let qpath = dir.join("q.json");
        let mut paused = test_record("p", dir.join("p.mp4"));
        paused.status = DownloadStatus::Paused;
        let mut broken = test_record("h", dir.join("h.mp4"));
        broken.status = DownloadStatus::Error("boş".into());
        // test_record Queued gelir; biri de sırada beklesin.
        let queued = test_record("k", dir.join("k.mp4"));
        save_queue(&qpath, &[paused, broken, queued]).unwrap();
        let (tx, _rx) = mpsc::channel();
        let m = DownloadManager::new(qpath.clone(), tx);
        // Sadece sıradaki duraklatılır; hatalı/duraklamış olduğu yerde kalır.
        assert_eq!(m.pause_all(), 1);
        let snap = m.snapshot();
        assert_eq!(snap.iter().filter(|r| r.status == DownloadStatus::Paused).count(), 2);
        assert!(matches!(
            snap.iter().find(|r| r.id == "h").unwrap().status,
            DownloadStatus::Error(_)
        ));
        // Devam: duraklamış + hatalı yeniden sıraya girer (3 kayıt).
        // (Çalışan işçi statüsü hemen Downloading'e çevirebilir, sayıdan sonrasını kontrol etmiyoruz.)
        assert_eq!(m.resume_all(), 3);
        // Yeniden duraklatma: sıradaki/şu anki işler yine durur.
        assert!(m.pause_all() >= 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn manager_local_file_only_for_finished_existing() {
        let dir = std::env::temp_dir().join("animecix-dl-local");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let qpath = dir.join("q.json");

        let mut r_done = test_record("d", dir.join("here.mp4"));
        r_done.status = DownloadStatus::Done;
        let mut r_missing = test_record("m", dir.join("gone.mp4"));
        r_missing.status = DownloadStatus::Done;
        r_missing.season = 3;
        let mut r_running = test_record("r", dir.join("part.mp4"));
        r_running.status = DownloadStatus::Downloading;
        r_running.season = 2;
        // Kayıtlardaki hedefler diskte sonra oluşturulacak (save_queue dosya yazmaz).
        save_queue(&qpath, &[r_done, r_missing, r_running]).unwrap();

        let (tx, _rx) = mpsc::channel();
        let m = DownloadManager::new(qpath, tx);
        // Dosya diskte yoksa Done kayıt bile yerel kaynak sayılmaz.
        assert_eq!(m.local_file("T", 1, 1), None);
        std::fs::write(dir.join("here.mp4"), b"x").unwrap();
        assert_eq!(m.local_file("T", 1, 1), Some(dir.join("here.mp4")));
        // Dosya var ama indirme sürüyor → henüz yerelden oynatılmaz.
        std::fs::write(dir.join("part.mp4"), b"x").unwrap();
        assert_eq!(m.local_file("T", 2, 1), None);
        assert_eq!(m.local_file("T", 3, 1), None, "dosya diskte yok");
        assert_eq!(m.local_file("Başka", 1, 1), None, "adda uymayan kayıt");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn manager_load_requeues_downloading() {
        let dir = std::env::temp_dir().join("animecix-dl-mgr2");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut rec = test_record("a", dir.join("v.mp4"));
        rec.status = DownloadStatus::Downloading;
        let mut rec2 = test_record("b", dir.join("w.mp4"));
        rec2.status = DownloadStatus::Done;
        save_queue(&dir.join("q.json"), &[rec, rec2]).unwrap();
        let (tx, _rx) = mpsc::channel();
        let m = DownloadManager::new(dir.join("q.json"), tx);
        let snap = m.snapshot();
        assert_eq!(snap.len(), 2);
        assert_eq!(snap[0].status, DownloadStatus::Queued, "yarım iş kuyruğa dönmeli");
        assert_eq!(snap[1].status, DownloadStatus::Done);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
