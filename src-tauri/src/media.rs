use serde::Serialize;
use tauri::{AppHandle, Emitter};

#[derive(Serialize, Clone, Default, PartialEq)]
pub struct MediaState {
    pub active: bool,
    pub title: String,
    pub artist: String,
    pub playing: bool,
    pub position: f64,
    pub length: f64,
}

// bytes gambar -> data URL (webview tidak boleh membuka file lokal)
// Dibuat langsung ke satu String berkapasitas pas (dulu: encode -> String sementara -> format! -> salinan lagi).
pub(crate) fn data_url(mime: &str, bytes: &[u8]) -> String {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let prefix = format!("data:{mime};base64,");
    let mut s = String::with_capacity(prefix.len() + bytes.len().div_ceil(3) * 4);
    s.push_str(&prefix);
    STANDARD.encode_string(bytes, &mut s);
    s
}

// Artwork lebih besar dari ini tidak dikirim: data URL-nya ikut tinggal di memori webview.
pub(crate) const MAX_ART_BYTES: u64 = 3_000_000;

// Kirim hanya saat keadaan berubah (posisi lagu yang berjalan tetap terkirim tiap detik).
// Diulang tiap 5 detik sebagai pengingat untuk frontend yang baru selesai dimuat.
pub(crate) struct Dedup {
    last: Option<MediaState>,
    at: std::time::Instant,
}
impl Dedup {
    pub(crate) fn new() -> Self {
        Self { last: None, at: std::time::Instant::now() }
    }
    pub(crate) fn emit(&mut self, app: &AppHandle, s: MediaState) {
        if self.last.as_ref() == Some(&s) && self.at.elapsed() < std::time::Duration::from_secs(5) {
            return;
        }
        self.at = std::time::Instant::now();
        let _ = app.emit("media", &s); // referensi: tidak perlu salinan
        self.last = Some(s);
    }
}

pub fn start(app: AppHandle) {
    imp::start(app);
}

#[tauri::command]
pub fn media_control(action: String) {
    imp::control(&action);
}

// Adzan: jeda SEMUA pemutar yang sedang berjalan (kecuali suara milik aplikasi ini sendiri),
// ingat mana yang dijeda, lalu lanjutkan hanya yang tadi dijeda lewat media_resume.
// Dijalankan di thread terpisah supaya tidak memblokir UI. Mengembalikan jumlah pemutar yang dijeda.
#[tauri::command]
pub async fn media_pause_all() -> usize {
    tauri::async_runtime::spawn_blocking(imp::pause_all).await.unwrap_or(0)
}

#[tauri::command]
pub async fn media_resume() {
    let _ = tauri::async_runtime::spawn_blocking(imp::resume_paused).await;
}

// pindah ke menit tertentu (detik dari awal lagu)
#[tauri::command]
pub fn media_seek(position: f64) {
    imp::seek(position);
}

// ======================= Linux (MPRIS) =======================
#[cfg(target_os = "linux")]
mod imp {
    use super::{data_url, MediaState};
    use mpris::{PlaybackStatus, PlayerFinder};
    use std::sync::Mutex;
    use std::time::Duration;
    use tauri::{AppHandle, Emitter};

    // pemutar yang dijeda saat adzan (nama bus D-Bus), dilanjutkan lagi sesudahnya
    static PAUSED: Mutex<Vec<String>> = Mutex::new(Vec::new());

    // suara milik aplikasi ini sendiri (mesin webview) jangan ikut dijeda
    fn is_self(p: &mpris::Player) -> bool {
        let id = format!("{} {}", p.identity(), p.bus_name()).to_lowercase();
        if id.contains("webkit") || id.contains("webview") {
            return true;
        }
        std::env::current_exe()
            .ok()
            .and_then(|e| e.file_stem().map(|s| s.to_string_lossy().to_lowercase()))
            .map(|n| n.len() >= 3 && id.contains(&n))
            .unwrap_or(false)
    }

    pub fn pause_all() -> usize {
        let mut done = Vec::new();
        if let Ok(finder) = PlayerFinder::new() {
            if let Ok(players) = finder.find_all() {
                for p in players {
                    if is_self(&p) {
                        continue;
                    }
                    if matches!(p.get_playback_status(), Ok(PlaybackStatus::Playing)) && p.pause().is_ok() {
                        done.push(p.bus_name().to_string());
                    }
                }
            }
        }
        let n = done.len();
        *PAUSED.lock().unwrap_or_else(|e| e.into_inner()) = done;
        n
    }

    pub fn resume_paused() {
        let names = std::mem::take(&mut *PAUSED.lock().unwrap_or_else(|e| e.into_inner()));
        if names.is_empty() {
            return;
        }
        if let Ok(finder) = PlayerFinder::new() {
            if let Ok(players) = finder.find_all() {
                for p in players {
                    if names.iter().any(|n| n == p.bus_name()) {
                        let _ = p.play();
                    }
                }
            }
        }
    }

    fn load_art(url: &str) -> Option<String> {
        if let Some(path) = url.strip_prefix("file://") {
            if std::fs::metadata(path).ok()?.len() > super::MAX_ART_BYTES {
                return None;
            }
            let bytes = std::fs::read(path).ok()?;
            let mime = if path.ends_with(".png") { "image/png" } else { "image/jpeg" };
            Some(data_url(mime, &bytes))
        } else {
            Some(url.to_string())
        }
    }

    pub fn start(app: AppHandle) {
        std::thread::spawn(move || {
            let finder = match PlayerFinder::new() {
                Ok(f) => f,
                Err(e) => {
                    eprintln!("MPRIS tidak tersedia: {e}");
                    return;
                }
            };
            let mut last_art = String::new();
            let mut dedup = super::Dedup::new();

            loop {
                let mut state = MediaState::default();

                if let Ok(player) = finder.find_active() {
                    if let Ok(meta) = player.get_metadata() {
                        state.active = true;
                        state.title = meta.title().unwrap_or("").to_string();
                        state.artist = meta.artists().map(|a| a.join(", ")).unwrap_or_default();
                        state.length = meta.length().map(|d| d.as_secs_f64()).unwrap_or(0.0);
                        state.playing =
                            matches!(player.get_playback_status(), Ok(PlaybackStatus::Playing));
                        state.position = player
                            .get_position()
                            .map(|d| d.as_secs_f64())
                            .unwrap_or(0.0);

                        // artwork hanya dikirim saat berganti
                        let art_url = meta.art_url().unwrap_or("").to_string();
                        if art_url != last_art {
                            last_art = art_url.clone();
                            let art = if art_url.is_empty() { None } else { load_art(&art_url) };
                            let _ = app.emit("media-art", art);
                        }
                    }
                }

                dedup.emit(&app, state);
                std::thread::sleep(Duration::from_millis(1000));
            }
        });
    }

    pub fn control(action: &str) {
        if let Ok(finder) = PlayerFinder::new() {
            if let Ok(p) = finder.find_active() {
                let _ = match action {
                    "toggle" => p.play_pause(),
                    "pause" => p.pause(),
                    "play" => p.play(),
                    "next" => p.next(),
                    "prev" => p.previous(),
                    _ => Ok(()),
                };
            }
        }
    }

    // MPRIS: seek relatif terhadap posisi sekarang
    pub fn seek(secs: f64) {
        if let Ok(finder) = PlayerFinder::new() {
            if let Ok(p) = finder.find_active() {
                if let Ok(cur) = p.get_position() {
                    let target = Duration::from_secs_f64(secs.max(0.0));
                    let _ = if target >= cur {
                        p.seek_forwards(&(target - cur))
                    } else {
                        p.seek_backwards(&(cur - target))
                    };
                }
            }
        }
    }
}

// ======================= Windows (SMTC) =======================
#[cfg(target_os = "windows")]
mod imp {
    use super::{data_url, MediaState};
    use std::time::Duration;
    use tauri::{AppHandle, Emitter};
    use windows::Media::Control::{
        GlobalSystemMediaTransportControlsSession as Session,
        GlobalSystemMediaTransportControlsSessionManager as Manager,
        GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
    };
    use std::sync::Mutex;
    use windows::Storage::Streams::DataReader;
    use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};

    // pemutar yang dijeda saat adzan (SourceAppUserModelId), dilanjutkan lagi sesudahnya
    static PAUSED: Mutex<Vec<String>> = Mutex::new(Vec::new());

    fn app_id(s: &Session) -> String {
        s.SourceAppUserModelId().map(|h| h.to_string()).unwrap_or_default().to_lowercase()
    }

    // Suara adzan diputar webview aplikasi ini dan ikut terdaftar sebagai sesi SMTC.
    // Sesi itu jangan dianggap "media pengguna": tidak ditampilkan, tidak dijeda, tidak dilanjutkan.
    fn is_self(s: &Session) -> bool {
        let id = app_id(s);
        if id.contains("webview2") {
            return true;
        }
        std::env::current_exe()
            .ok()
            .and_then(|e| e.file_stem().map(|n| n.to_string_lossy().to_lowercase()))
            .map(|n| n.len() >= 3 && id.contains(&n))
            .unwrap_or(false)
    }

    fn session() -> windows::core::Result<Session> {
        let mgr = Manager::RequestAsync()?.get()?;
        let cur = mgr.GetCurrentSession()?;
        if !is_self(&cur) {
            return Ok(cur);
        }
        for s in mgr.GetSessions()? {
            if !is_self(&s) {
                return Ok(s);
            }
        }
        Err(windows::core::Error::from(windows::core::HRESULT(0x8000_4005_u32 as i32)))
    }

    pub fn pause_all() -> usize {
        unsafe {
            let _ = RoInitialize(RO_INIT_MULTITHREADED);
        }
        let mut done = Vec::new();
        if let Ok(mgr) = Manager::RequestAsync().and_then(|o| o.get()) {
            if let Ok(list) = mgr.GetSessions() {
                for s in list {
                    if is_self(&s) {
                        continue;
                    }
                    let playing = s
                        .GetPlaybackInfo()
                        .and_then(|i| i.PlaybackStatus())
                        .map(|st| st == Status::Playing)
                        .unwrap_or(false);
                    if !playing {
                        continue;
                    }
                    // tunggu hasilnya supaya adzan baru diputar setelah media benar-benar dijeda
                    let _ = s.TryPauseAsync().and_then(|o| o.get());
                    done.push(app_id(&s));
                }
            }
        }
        let n = done.len();
        *PAUSED.lock().unwrap_or_else(|e| e.into_inner()) = done;
        n
    }

    pub fn resume_paused() {
        unsafe {
            let _ = RoInitialize(RO_INIT_MULTITHREADED);
        }
        let ids = std::mem::take(&mut *PAUSED.lock().unwrap_or_else(|e| e.into_inner()));
        if ids.is_empty() {
            return;
        }
        if let Ok(mgr) = Manager::RequestAsync().and_then(|o| o.get()) {
            if let Ok(list) = mgr.GetSessions() {
                for s in list {
                    if !is_self(&s) && ids.contains(&app_id(&s)) {
                        let _ = s.TryPlayAsync().and_then(|o| o.get());
                    }
                }
            }
        }
    }

    fn read_state(s: &Session) -> windows::core::Result<MediaState> {
        let props = s.TryGetMediaPropertiesAsync()?.get()?;
        let tl = s.GetTimelineProperties()?;
        let info = s.GetPlaybackInfo()?;

        let playing = info.PlaybackStatus()? == Status::Playing;
        let length = tl.EndTime()?.Duration as f64 / 10_000_000.0;
        let mut position = tl.Position()?.Duration as f64 / 10_000_000.0;

        // SMTC hanya memberi snapshot posisi; saat play, tambahkan waktu yang sudah berlalu
        if playing {
            // LastUpdatedTime: satuan 100 ns sejak 1 Jan 1601 (epoch Windows)
            let updated = tl.LastUpdatedTime()?.UniversalTime as f64 / 10_000_000.0;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs_f64())
                .unwrap_or(0.0)
                + 11_644_473_600.0; // selisih epoch 1601 -> 1970 (detik)
            let elapsed = now - updated;
            if elapsed > 0.0 && elapsed < 6.0 * 3600.0 {
                position += elapsed;
            }
        }
        if length > 0.0 {
            position = position.min(length);
        }

        Ok(MediaState {
            active: true,
            title: props.Title()?.to_string(),
            artist: props.Artist()?.to_string(),
            playing,
            position,
            length,
        })
    }

    // artwork dari thumbnail SMTC
    fn thumb(s: &Session) -> Option<String> {
        let props = s.TryGetMediaPropertiesAsync().ok()?.get().ok()?;
        let stream = props.Thumbnail().ok()?.OpenReadAsync().ok()?.get().ok()?;
        let size = stream.Size().ok()? as u32;
        if size == 0 || size as u64 > super::MAX_ART_BYTES {
            return None;
        }
        let input = stream.GetInputStreamAt(0).ok()?;
        let reader = DataReader::CreateDataReader(&input).ok()?;
        reader.LoadAsync(size).ok()?.get().ok()?;
        let mut buf = vec![0u8; size as usize];
        reader.ReadBytes(&mut buf).ok()?;
        let mime = if buf.starts_with(&[0x89, b'P']) { "image/png" } else { "image/jpeg" };
        Some(data_url(mime, &buf))
    }

    pub fn start(app: AppHandle) {
        std::thread::spawn(move || {
            unsafe {
                let _ = RoInitialize(RO_INIT_MULTITHREADED);
            }
            let mut last_key = String::new();
            let mut dedup = super::Dedup::new();
            let mut art_done = false;
            let mut tries: u8 = 0;

            loop {
                let mut state = MediaState::default();
                let mut idle = true; // tidak ada sesi media -> poll lebih jarang

                if let Ok(s) = session() {
                    if let Ok(st) = read_state(&s) {
                        // lagu baru -> reset status artwork
                        let key = format!("{}|{}", st.title, st.artist);
                        if key != last_key {
                            last_key = key;
                            art_done = false;
                            tries = 0;
                        }

                        // ulangi sampai thumbnail benar-benar siap
                        if !art_done {
                            match thumb(&s) {
                                Some(img) => {
                                    let _ = app.emit("media-art", Some(img));
                                    art_done = true;
                                }
                                None => {
                                    tries += 1;
                                    if tries >= 8 {
                                        // menyerah: sembunyikan gambar
                                        let _ = app.emit("media-art", None::<String>);
                                        art_done = true;
                                    }
                                }
                            }
                        }
                        state = st;
                        idle = false;
                    }
                }

                dedup.emit(&app, state);
                std::thread::sleep(Duration::from_millis(if idle { 2000 } else { 1000 }));
            }
        });
    }

    pub fn control(action: &str) {
        if let Ok(s) = session() {
            let _ = match action {
                "toggle" => s.TryTogglePlayPauseAsync().map(|_| ()),
                "pause" => s.TryPauseAsync().map(|_| ()),
                "play" => s.TryPlayAsync().map(|_| ()),
                "next" => s.TrySkipNextAsync().map(|_| ()),
                "prev" => s.TrySkipPreviousAsync().map(|_| ()),
                _ => Ok(()),
            };
        }
    }

    // SMTC: posisi dalam satuan 100 ns. Hanya berhasil kalau aplikasi pemutarnya mengizinkan seek.
    pub fn seek(secs: f64) {
        if let Ok(s) = session() {
            let ticks = (secs.max(0.0) * 10_000_000.0) as i64;
            let _ = s.TryChangePlaybackPositionAsync(ticks);
        }
    }
}
