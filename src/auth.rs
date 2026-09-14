//! Hesap girişi (animecix.tv cookie oturumu).
//!
//! Akış (sitedeki Angular istemcisiyle birebir):
//! 1. `GET {BASE}/` → `XSRF-TOKEN` cookie'si alınır.
//! 2. `POST {BASE}/secure/auth/login` + `Cookie` + `X-XSRF-TOKEN`
//!    header'ları, JSON `{email, password}`.
//! 3. Başarılıysa session cookie'leri saklanır (`session.json`).
//!
//! Bearer token YOKTUR; Angular interceptor da cookie + XSRF kullanır
//! (`withCredentials`, `X-XSRF-TOKEN`). Göreli tüm API path'leri
//! `secure/` prefix'i alır.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::api::{Client, BASE};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct User {
    pub id: u64,
    pub name: String,
    pub email: String,
    pub avatar: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Session {
    /// name -> value (XSRF-TOKEN dahil).
    pub cookies: HashMap<String, String>,
    pub user: Option<User>,
}

impl Session {
    fn path() -> PathBuf {
        let mut p = crate::api::Client::settings_path();
        p.set_file_name("session.json");
        p
    }

    pub fn load() -> Self {
        std::fs::read_to_string(Self::path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    fn save(&self) {
        if let Some(parent) = Self::path().parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(s) = serde_json::to_string(self) {
            let _ = std::fs::write(Self::path(), s);
        }
    }

    pub fn clear() {
        let _ = std::fs::remove_file(Self::path());
    }

    pub(crate) fn cookie_header(&self) -> String {
        self.cookies
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("; ")
    }

    pub(crate) fn xsrf(&self) -> Option<String> {
        self.cookies.get("XSRF-TOKEN").cloned()
    }

    fn store_from(&mut self, resp: &crate::http::Resp) {
        for raw in resp.header_all("set-cookie") {
            if let Some(pair) = raw.split(';').next() {
                if let Some((k, v)) = pair.split_once('=') {
                    let k = k.trim();
                    let v = v.trim();
                    if k.is_empty() {
                        continue;
                    }
                    // Silinen cookie: değeri boş + Expires geçmişte.
                    if v.is_empty() && raw.to_lowercase().contains("expires=thu, 01 jan 1970") {
                        self.cookies.remove(k);
                    } else if !v.is_empty() {
                        self.cookies.insert(k.to_string(), v.to_string());
                    }
                }
            }
        }
    }
}

fn parse_user(v: &serde_json::Value) -> Option<User> {
    // Başarılı yanıtta kullanıcı `user` altındadır (login yanıtı),
    // bazen `data` altında gelir (bootstrapper).
    let d = if v["user"].is_object() {
        &v["user"]
    } else if v["data"].is_object() {
        &v["data"]
    } else {
        v
    };
    let name = d["username"]
        .as_str()
        .or_else(|| d["name"].as_str())
        .or_else(|| d["display_name"].as_str())
        .unwrap_or("")
        .to_string();
    if name.is_empty() && d["id"].as_u64().is_none() {
        return None;
    }
    Some(User {
        id: d["id"].as_u64().unwrap_or(0),
        name,
        email: d["email"].as_str().unwrap_or("").to_string(),
        avatar: d["avatar"]
            .as_str()
            .or_else(|| d["avatar_url"].as_str())
            .map(|s| s.to_string()),
    })
}

fn err_msg(v: &serde_json::Value) -> String {
    v["message"]
        .as_str()
        .or_else(|| v["error"].as_str())
        .or_else(|| v["errors"]["email"].as_str())
        .unwrap_or("Giriş başarısız")
        .to_string()
}

impl Client {
    /// Kayıtlı oturum varsa döner.
    pub fn session_user(&self) -> Option<User> {
        self.session.lock().ok()?.user.clone()
    }

    pub fn is_logged_in(&self) -> bool {
        self.session_user().is_some()
    }

    /// E-posta + şifre ile giriş. Başarılıysa oturumu saklar.
    ///
    /// Site XSRF cookie'sini her yanıtta vermiyor; ilk POST genelde
    /// 403 (CSRF) dönüp cookie set ediyor — o cookie ile tekrar denenince
    /// giriş tamamlanıyor. Bu döngü o akışı otomatik yürütür.
    pub fn login(&self, email: &str, password: &str) -> Result<User, String> {
        // 0) Isınma: ana sayfadan cookie topla (bazen verir).
        if let Ok(r) = self.http.get(format!("{BASE}/")).send() {
            if let Ok(mut s) = self.session.lock() {
                s.store_from(&r);
            }
        }
        let body = serde_json::json!({ "email": email, "password": password });
        let mut last_err = "Giriş başarısız".to_string();
        for round in 0..4 {
            let (cookie, xsrf) = self
                .session
                .lock()
                .map(|s| (s.cookie_header(), s.xsrf().unwrap_or_default()))
                .unwrap_or_default();
            eprintln!("[AUTH] login denemesi {} ({} cookie)", round + 1, cookie.len());
            let resp = self
                .http
                .post(format!("{BASE}/secure/auth/login"))
                .header("Cookie", &cookie)
                .header("X-XSRF-TOKEN", &xsrf)
                .header("X-Requested-With", "XMLHttpRequest")
                .header("Accept", "application/json")
                .json(&body)
                .timeout(20)
                .send()?;
            // Yanıttaki cookie'leri her turda topla.
            {
                let mut s = self.session.lock().map_err(|e| e.to_string())?;
                s.store_from(&resp);
            }
            let v: serde_json::Value = resp.json()?;
            if resp_ok(&v) {
                let u = parse_user(&v).ok_or("Kullanıcı bilgisi okunamadı")?;
                let mut s = self.session.lock().map_err(|e| e.to_string())?;
                s.user = Some(u.clone());
                s.save();
                eprintln!("[AUTH] giriş başarılı: {}", u.name);
                return Ok(u);
            }
            let msg = err_msg(&v);
            eprintln!("[AUTH] login yanıtı: {msg}");
            // CSRF hatası = cookie tazelendi, tekrar dene.
            // Yanlış şifre = dur.
            let m = msg.to_lowercase();
            if m.contains("csrf") || m.contains("token") || m.contains("419") {
                last_err = msg;
                std::thread::sleep(std::time::Duration::from_secs(2));
                continue;
            }
            return Err(msg);
        }
        Err(last_err)
    }

    /// Çıkış: sunucuya bildir + yerel oturumu sil.
    pub fn logout(&self) {
        let (cookie, xsrf) = self
            .session
            .lock()
            .map(|s| (s.cookie_header(), s.xsrf().unwrap_or_default()))
            .unwrap_or_default();
        if !cookie.is_empty() {
            let _ = self
                .http
                .post(format!("{BASE}/secure/auth/logout"))
                .header("Cookie", &cookie)
                .header("X-XSRF-TOKEN", &xsrf)
                .header("X-Requested-With", "XMLHttpRequest")
                .header("Accept", "application/json")
                .timeout(15)
                .send();
        }
        if let Ok(mut s) = self.session.lock() {
            *s = Session::default();
        }
        Session::clear();
        eprintln!("[AUTH] çıkış yapıldı");
    }

    /// Girişli istek: cookie + XSRF header'lı GET, JSON parse eder.
    /// Giriş yoksa Err döner.
    pub fn authed_get_json<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T, String> {
        let (cookie, xsrf) = self
            .session
            .lock()
            .map(|s| (s.cookie_header(), s.xsrf().unwrap_or_default()))
            .map_err(|e| e.to_string())?;
        if cookie.is_empty() {
            return Err("Giriş yapılmamış".into());
        }
        let resp = self
            .http
            .get(format!("{BASE}/secure/{path}"))
            .header("Cookie", &cookie)
            .header("X-XSRF-TOKEN", &xsrf)
            .header("X-Requested-With", "XMLHttpRequest")
            .header("Accept", "application/json")
            .timeout(20)
            .send()?;
        if resp.status() == 401 {
            return Err("Oturum geçersiz (tekrar giriş yap)".into());
        }
        resp.json()
    }

    /// Sunucudaki izleme konumu (saniye). Yoksa/hata varsa None.
    /// Şema: POST history/get-current-time {titleId, seasonNumber, episodeNumber}
    /// Yanıt: {time: milisaniye}.
    pub fn get_remote_pos(&self, title_id: u64, season: u64, episode: u64) -> Option<f64> {
        if !self.is_logged_in() {
            return None;
        }
        let key = (title_id, season, episode);
        // 5dk bellek önbelleği: anime açışında 120 istek fırtınası diner.
        if let Ok(map) = self.pos_cache.lock() {
            if let Some((at, v)) = map.get(&key) {
                if at.elapsed().as_secs() < 300 {
                    return Some(*v);
                }
            }
        }
        let (cookie, xsrf) = self
            .session
            .lock()
            .map(|s| (s.cookie_header(), s.xsrf().unwrap_or_default()))
            .ok()?;
        let body = serde_json::json!({
            "titleId": title_id,
            "seasonNumber": season,
            "episodeNumber": episode,
        });
        let resp = self
            .http
            .post(format!("{BASE}/secure/history/get-current-time"))
            .header("Cookie", &cookie)
            .header("X-XSRF-TOKEN", &xsrf)
            .header("X-Requested-With", "XMLHttpRequest")
            .header("Accept", "application/json")
            .json(&body)
            .timeout(15)
            .send()
            .ok()?;
        let v: serde_json::Value = resp.json().ok()?;
        let ms = v["time"].as_f64().or_else(|| v["data"]["time"].as_f64())?;
        if ms > 5000.0 {
            let v = ms / 1000.0;
            if let Ok(mut map) = self.pos_cache.lock() {
                if map.len() > 5000 {
                    map.clear();
                }
                map.insert(key, (std::time::Instant::now(), v));
            }
            Some(v)
        } else {
            None
        }
    }

    /// İzleme konumunu sunucuya bildir (sessiz; hata yutulur).
    /// Şema: POST history/report-current-time {titleId, seasonNumber,
    /// episodeNumber, time: milisaniye}.
    pub fn report_pos(&self, title_id: u64, season: u64, episode: u64, pos_secs: f64) {
        if !self.is_logged_in() || pos_secs < 10.0 {
            return;
        }
        let (cookie, xsrf) = match self.session.lock() {
            Ok(s) => (s.cookie_header(), s.xsrf().unwrap_or_default()),
            Err(_) => return,
        };
        let body = serde_json::json!({
            "titleId": title_id,
            "seasonNumber": season,
            "episodeNumber": episode,
            "time": (pos_secs * 1000.0) as u64,
        });
        match self
            .http
            .post(format!("{BASE}/secure/history/report-current-time"))
            .header("Cookie", &cookie)
            .header("X-XSRF-TOKEN", &xsrf)
            .header("X-Requested-With", "XMLHttpRequest")
            .header("Accept", "application/json")
            .json(&body)
            .timeout(15)
            .send()
        {
            Ok(r) => eprintln!("[AUTH] konum bildirildi (HTTP {})", r.status()),
            Err(e) => eprintln!("[AUTH] konum bildirilemedi: {e}"),
        }
    }

    /// Sunucuya izleme kaydı işle (oynatma başında, sessiz; hata yutulur).
    /// Şema: POST history/put-title {id, name, date: ms,
    /// videos: [açılan bölümün ilk videosu], episodes: []}.
    /// Sunucu kaydı upsert eder: `date` yenilenir, `videos[0]` son
    /// bölüm olur (site de bu alandan "S1B2" rozetini üretir).
    pub fn put_title(&self, title: &crate::api::Title, ep: &crate::api::Episode) {
        if !self.is_logged_in() {
            return;
        }
        let (cookie, xsrf) = match self.session.lock() {
            Ok(s) => (s.cookie_header(), s.xsrf().unwrap_or_default()),
            Err(_) => return,
        };
        let first_video = self
            .episode_points(title.id, ep.episode, ep.season)
            .get("videos")
            .and_then(|v| v.as_array())
            .and_then(|a| a.first().cloned());
        let body = serde_json::json!({
            "id": title.id,
            "name": title.name,
            "date": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
            "videos": first_video.map(|v| vec![v]).unwrap_or_default(),
            "episodes": [],
        });
        match self
            .http
            .post(format!("{BASE}/secure/history/put-title"))
            .header("Cookie", &cookie)
            .header("X-XSRF-TOKEN", &xsrf)
            .header("X-Requested-With", "XMLHttpRequest")
            .header("Accept", "application/json")
            .json(&body)
            .timeout(15)
            .send()
        {
            Ok(r) => eprintln!("[AUTH] geçmiş kaydı işlendi (HTTP {})", r.status()),
            Err(e) => eprintln!("[AUTH] geçmiş kaydı işlenemedi: {e}"),
        }
    }

    /// Sunucu geçmişi TEK sayfa (0-indexli, tarih-azalan).
    /// Şema: GET secure/history/get-titles?page=N → {data: {totalData, totalCount}}.
    /// Her kayıt: tam title objesi + `date` (ms) + son bölümün
    /// videoları (`videos[0]` → season_num/episode_num).
    /// Dönen: (bu sayfadaki kayıtlar, toplam kayıt sayısı).
    pub fn server_history_page(&self, page: u32) -> Result<(Vec<crate::api::ServerEntry>, usize), String> {
        if !self.is_logged_in() {
            return Ok((Vec::new(), 0));
        }
        let (cookie, xsrf, http) = {
            let s = self.session.lock().map_err(|e| e.to_string())?;
            (
                s.cookie_header(),
                s.xsrf().unwrap_or_default(),
                self.http.clone(),
            )
        };
        let (entries, total) = Self::fetch_history_page(&http, &cookie, &xsrf, page)?;
        eprintln!("[AUTH] geçmiş sayfa {page}: {} kayıt (toplam {total})", entries.len());
        Ok((entries, total))
    }

    fn fetch_history_page(
        http: &crate::http::Http,
        cookie: &str,
        xsrf: &str,
        page: u32,
    ) -> Result<(Vec<crate::api::ServerEntry>, usize), String> {
        let resp = http
            .get(format!("{BASE}/secure/history/get-titles?page={page}"))
            .header("Cookie", cookie)
            .header("X-XSRF-TOKEN", xsrf)
            .header("X-Requested-With", "XMLHttpRequest")
            .header("Accept", "application/json")
            .timeout(20)
            .send()?;
        let v: serde_json::Value = resp.json()?;
        if v["success"] == serde_json::Value::Bool(false) {
            return Err(v["message"]
                .as_str()
                .or_else(|| v["error"].as_str())
                .unwrap_or("Sunucu geçmişi vermedi")
                .to_string());
        }
        let total = v["data"]["totalCount"][0]["count"].as_u64().unwrap_or(0) as usize;
        let items = match v["data"]["totalData"].as_array() {
            Some(a) => a.clone(),
            None => {
                return Err(v["message"]
                    .as_str()
                    .unwrap_or("Geçmiş yanıtı anlaşılmadı")
                    .to_string())
            }
        };
        let mut entries: Vec<crate::api::ServerEntry> = items
            .iter()
            .filter_map(|r| {
                let title = crate::api::Title::from_value(r)?;
                let date = r["date"].as_u64().unwrap_or(0);
                let v0 = r["videos"].as_array().and_then(|a| a.first());
                let season = v0.and_then(|v| v["season_num"].as_u64()).unwrap_or(0);
                let episode = v0.and_then(|v| v["episode_num"].as_u64()).unwrap_or(0);
                Some(crate::api::ServerEntry { title, date, season, episode })
            })
            .collect();
        // API zaten tarih-azalan dönüyor; garantiye al.
        entries.sort_by(|a, b| b.date.cmp(&a.date));
        Ok((entries, total))
    }

    /// Devam listesi için en yeni sayfalar (tarih-azalan birleşik).
    /// Geçmiş sayfası tek sayfa ister; ama devam havuzu için
    /// ilk `max_pages` sayfa paralel çekilir.
    pub fn server_top(&self, max_pages: u32) -> Result<(Vec<crate::api::ServerEntry>, usize), String> {
        self.server_pages(0, max_pages)
    }

    /// `[from, from+count)` aralığındaki geçmiş sayfaları (paralel).
    /// Dönen: (kayıtlar tarih-azalan, toplam kayıt sayısı).
    pub fn server_pages(&self, from: u32, count: u32) -> Result<(Vec<crate::api::ServerEntry>, usize), String> {
        if !self.is_logged_in() {
            return Ok((Vec::new(), 0));
        }
        let (cookie, xsrf, http) = {
            let s = self.session.lock().map_err(|e| e.to_string())?;
            (
                s.cookie_header(),
                s.xsrf().unwrap_or_default(),
                self.http.clone(),
            )
        };
        let count = count.max(1);
        let (mut all, total) = Self::fetch_history_page(&http, &cookie, &xsrf, from)?;
        if count > 1 {
            std::thread::scope(|scope| {
                let mut handles = Vec::new();
                for p in (from + 1)..(from + count) {
                    let (h, c, x) = (http.clone(), cookie.clone(), xsrf.clone());
                    handles.push(scope.spawn(move || (p, Self::fetch_history_page(&h, &c, &x, p))));
                }
                let mut rest: Vec<(u32, Vec<crate::api::ServerEntry>)> = Vec::new();
                for h in handles {
                    match h.join() {
                        Ok((p, Ok((items, _)))) => rest.push((p, items)),
                        Ok((p, Err(e))) => eprintln!("[AUTH] üst geçmiş sayfa {p} hatası: {e}"),
                        Err(_) => eprintln!("[AUTH] üst geçmiş sayfası thread hatası"),
                    }
                }
                rest.sort_by_key(|(p, _)| *p);
                for (_, items) in rest {
                    all.extend(items);
                }
            });
        }
        all.sort_by(|a, b| b.date.cmp(&a.date));
        eprintln!("[AUTH] üst geçmiş: {} başlık (toplam {total})", all.len());
        Ok((all, total))
    }

    /// Kullanıcının watchlist id'si (sistem listesi).
    /// Şema: GET secure/user-profile/{userId}/lists → data[] içinde
    /// name=="watchlist" && system.
    pub fn watchlist_id(&self) -> Result<u64, String> {
        let uid = self
            .session_user()
            .map(|u| u.id)
            .filter(|id| *id > 0)
            .ok_or("Giriş yapılmamış")?;
        let v: serde_json::Value = self.authed_get_json(&format!("user-profile/{uid}/lists?per_page=30"))?;
        let items = v["pagination"]["data"]
            .as_array()
            .or_else(|| v["data"].as_array())
            .cloned()
            .unwrap_or_default();
        items
            .iter()
            .find(|x| x["name"] == "watchlist")
            .and_then(|x| x["id"].as_u64())
            .ok_or("Watchlist bulunamadı".into())
    }

    /// Watchlist'teki başlıklar (tam Title objeleri).
    pub fn watchlist_titles(&self) -> Result<Vec<crate::api::Title>, String> {
        let uid = self
            .session_user()
            .map(|u| u.id)
            .filter(|id| *id > 0)
            .ok_or("Giriş yapılmamış")?;
        let v: serde_json::Value = self.authed_get_json(&format!("user-profile/{uid}/lists?per_page=30"))?;
        let items = v["pagination"]["data"]
            .as_array()
            .or_else(|| v["data"].as_array())
            .cloned()
            .unwrap_or_default();
        let wl = items
            .iter()
            .find(|x| x["name"] == "watchlist")
            .ok_or("Watchlist bulunamadı")?;
        let titles: Vec<crate::api::Title> = wl["items"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(crate::api::Title::from_value)
            .collect();
        eprintln!("[AUTH] watchlist: {} başlık", titles.len());
        Ok(titles)
    }

    fn list_item(&self, list_id: u64, action: &str, title_id: u64) -> Result<(), String> {
        let (cookie, xsrf) = self
            .session
            .lock()
            .map(|s| (s.cookie_header(), s.xsrf().unwrap_or_default()))
            .map_err(|e| e.to_string())?;
        let body = serde_json::json!({ "itemId": title_id, "itemType": "title" });
        let resp = self
            .http
            .post(format!("{BASE}/secure/lists/{list_id}/{action}"))
            .header("Cookie", &cookie)
            .header("X-XSRF-TOKEN", &xsrf)
            .header("X-Requested-With", "XMLHttpRequest")
            .header("Accept", "application/json")
            .json(&body)
            .timeout(15)
            .send()?;
        let v: serde_json::Value = resp.json()?;
        if resp_ok(&v) {
            Ok(())
        } else {
            Err(err_msg(&v))
        }
    }

    /// Watchlist'e ekle / çıkar.
    pub fn fav_add(&self, title_id: u64) -> Result<(), String> {
        let id = self.watchlist_id()?;
        self.list_item(id, "add", title_id)
    }

    pub fn fav_remove(&self, title_id: u64) -> Result<(), String> {
        let id = self.watchlist_id()?;
        self.list_item(id, "remove", title_id)
    }

    /// Sunucu watchlist'ini yerel favorilerle birleştir (site → cihaz).
    /// Yeni eklenen sayısını döner.
    pub fn merge_server_favs(&self) -> usize {
        if !self.is_logged_in() {
            return 0;
        }
        let Ok(titles) = self.watchlist_titles() else {
            return 0;
        };
        let mut st = self.load_state();
        let mut added = 0;
        for t in titles {
            if !st.saved.iter().any(|x| x.id == t.id) {
                st.saved.insert(0, t);
                added += 1;
            }
        }
        if added > 0 {
            self.save_state(&st);
        }
        eprintln!("[AUTH] favori birleştirme: {added} yeni");
        added
    }

    /// Giriş sonrası senkron adaylarını yoklar (teşhis logu).
    /// Dönen: (path, status, boyut) listesi.
    pub fn probe_authed(&self) -> Vec<(String, u16, usize)> {
        let paths = [
            "auth/social-profiles",
            "lists?per_page=1",
            "lists/liked",
            "last-episodes?page=1",
            "list-includes?itemId=1",
        ];
        let mut out = Vec::new();
        for p in paths {
            let (cookie, xsrf) = match self.session.lock() {
                Ok(s) => (s.cookie_header(), s.xsrf().unwrap_or_default()),
                Err(_) => continue,
            };
            if cookie.is_empty() {
                continue;
            }
            let r = self
                .http
                .get(format!("{BASE}/secure/{p}"))
                .header("Cookie", &cookie)
                .header("X-XSRF-TOKEN", &xsrf)
                .header("X-Requested-With", "XMLHttpRequest")
                .header("Accept", "application/json")
                .timeout(15)
                .send();
            match r {
                Ok(resp) => {
                    let status = resp.status();
                    let body_len = resp.bytes().map(|b| b.len()).unwrap_or(0);
                    eprintln!("[AUTH-PROBE] {p} -> {status} ({body_len} byte)");
                    out.push((p.to_string(), status, body_len));
                }
                Err(e) => {
                    eprintln!("[AUTH-PROBE] {p} -> HATA {e}");
                    out.push((p.to_string(), 0, 0));
                }
            }
        }
        out
    }
}

fn resp_ok(v: &serde_json::Value) -> bool {
    if v["success"] == serde_json::Value::Bool(true) {
        return true;
    }
    // success alanı yoksa: user objesi varsa + hata yoksa başarılı say.
    (v["user"].is_object() || v["data"].is_object() || v["id"].as_u64().is_some())
        && v["error"].is_null()
        && v["message"].as_str().is_none_or(|m| {
            let m = m.to_lowercase();
            !m.contains("başarısız") && !m.contains("fail") && !m.contains("hata")
        })
}
