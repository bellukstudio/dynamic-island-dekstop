//! Pengambilan lirik dari LRCLIB + parser LRC.
//! Dipanggil frontend lewat: invoke("lyrics_fetch", { title, artist, length })

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

const API: &str = "https://lrclib.net/api";
const USER_AGENT: &str = "WellNotch/0.1 (https://github.com/your-name/WellNotch)";
/// Selisih durasi maksimum (detik) agar hasil pencarian dianggap lagu yang sama.
const MAX_DURATION_DIFF: f64 = 6.0;

#[derive(Serialize, Clone, Debug)]
pub struct LyLine {
    /// Waktu mulai baris, dalam detik.
    pub t: f64,
    pub text: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct Lyrics {
    pub synced: bool,
    pub lines: Vec<LyLine>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct LrcRecord {
    #[serde(default)]
    synced_lyrics: Option<String>,
    #[serde(default)]
    plain_lyrics: Option<String>,
    #[serde(default)]
    instrumental: bool,
    #[serde(default)]
    duration: Option<f64>,
}

// ---------- klien & cache ----------

fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(8))
            .build()
            .expect("gagal membuat HTTP client")
    })
}

fn cache() -> &'static Mutex<HashMap<String, Option<Lyrics>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<Lyrics>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

// ---------- command Tauri ----------

#[tauri::command]
pub async fn lyrics_fetch(
    title: String,
    artist: String,
    length: f64,
) -> Result<Option<Lyrics>, String> {
    let title = clean_title(&title);
    let artist = clean_artist(&artist);
    if title.is_empty() {
        return Ok(None);
    }

    let key = format!("{}|{}|{}", artist.to_lowercase(), title.to_lowercase(), length.round() as i64);
    if let Some(hit) = cache().lock().ok().and_then(|c| c.get(&key).cloned()) {
        return Ok(hit);
    }

    let result = fetch_lrclib(&title, &artist, length).await?;

    // Hanya hasil sukses (ada/tidak ada) yang disimpan; error jaringan dilempar tanpa cache.
    if let Ok(mut c) = cache().lock() {
        if c.len() > 200 {
            c.clear();
        }
        c.insert(key, result.clone());
    }
    Ok(result)
}

// ---------- LRCLIB ----------

async fn fetch_lrclib(title: &str, artist: &str, length: f64) -> Result<Option<Lyrics>, String> {
    // 1) Pencocokan tepat (dengan durasi bila ada).
    let mut params: Vec<(&str, String)> = vec![
        ("track_name", title.to_string()),
        ("artist_name", artist.to_string()),
    ];
    if length > 0.0 {
        params.push(("duration", (length.round() as i64).to_string()));
    }
    if let Some(rec) = get_exact(&params).await? {
        if let Some(l) = record_to_lyrics(&rec) {
            return Ok(Some(l));
        }
    }

    // 2) Pencarian lebih longgar, pilih kandidat terbaik.
    let resp = client()
        .get(format!("{API}/search"))
        .query(&[("track_name", title), ("artist_name", artist)])
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        return Ok(None);
    }
    let records: Vec<LrcRecord> = resp.json().await.map_err(|e| e.to_string())?;

    Ok(pick_best(&records, length))
}

async fn get_exact(params: &[(&str, String)]) -> Result<Option<LrcRecord>, String> {
    let resp = client()
        .get(format!("{API}/get"))
        .query(params)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    match resp.status().as_u16() {
        200 => Ok(Some(resp.json::<LrcRecord>().await.map_err(|e| e.to_string())?)),
        404 => Ok(None),
        s => Err(format!("LRCLIB mengembalikan status {s}")),
    }
}

fn pick_best(records: &[LrcRecord], length: f64) -> Option<Lyrics> {
    let mut best: Option<(f64, Lyrics)> = None;

    for rec in records {
        let Some(lyrics) = record_to_lyrics(rec) else { continue };

        let diff = match (length > 0.0, rec.duration) {
            (true, Some(d)) => (d - length).abs(),
            _ => 0.0,
        };
        if length > 0.0 && rec.duration.is_some() && diff > MAX_DURATION_DIFF {
            continue;
        }

        // Skor: lirik bersinkron lebih disukai, lalu durasi paling dekat.
        let score = diff + if lyrics.synced { 0.0 } else { 100.0 };
        if best.as_ref().map_or(true, |(s, _)| score < *s) {
            best = Some((score, lyrics));
        }
    }
    best.map(|(_, l)| l)
}

fn record_to_lyrics(rec: &LrcRecord) -> Option<Lyrics> {
    if rec.instrumental {
        return None;
    }
    if let Some(s) = rec.synced_lyrics.as_deref() {
        let lines = parse_lrc(s);
        if !lines.is_empty() {
            return Some(Lyrics { synced: true, lines });
        }
    }
    if let Some(p) = rec.plain_lyrics.as_deref() {
        let lines = parse_plain(p);
        if !lines.is_empty() {
            return Some(Lyrics { synced: false, lines });
        }
    }
    None
}

// ---------- parser ----------

/// Mengurai LRC: `[mm:ss.xx] teks`, mendukung beberapa timestamp per baris,
/// mengabaikan tag metadata (`[ar:..]`, `[ti:..]`, `[offset:..]`) dan tag kata `<mm:ss.xx>`.
pub fn parse_lrc(src: &str) -> Vec<LyLine> {
    let mut out: Vec<LyLine> = Vec::new();

    for raw in src.lines() {
        let mut rest = raw.trim();
        let mut times: Vec<f64> = Vec::new();

        while rest.starts_with('[') {
            let Some(end) = rest.find(']') else { break };
            match parse_timestamp(&rest[1..end]) {
                Some(t) => {
                    times.push(t);
                    rest = rest[end + 1..].trim_start();
                }
                None => break, // tag metadata
            }
        }

        if times.is_empty() {
            continue;
        }

        let text = strip_word_tags(rest).trim().to_string();
        for t in times {
            out.push(LyLine { t, text: text.clone() });
        }
    }

    out.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// Lirik tanpa waktu: satu baris per entri, waktu 0 (frontend memperkirakan dari progres lagu).
fn parse_plain(src: &str) -> Vec<LyLine> {
    let lines: Vec<LyLine> = src
        .lines()
        .map(|l| LyLine { t: 0.0, text: l.trim().to_string() })
        .collect();
    if lines.iter().all(|l| l.text.is_empty()) {
        return Vec::new();
    }
    lines
}

/// "mm:ss", "mm:ss.xx" atau "mm:ss.xxx" -> detik.
fn parse_timestamp(s: &str) -> Option<f64> {
    let (m, sec) = s.split_once(':')?;
    let m: u32 = m.trim().parse().ok()?;
    let sec = sec.trim().replace(',', ".");
    if sec.is_empty() || !sec.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return None;
    }
    let sec: f64 = sec.parse().ok()?;
    Some(m as f64 * 60.0 + sec)
}

/// Hapus tag waktu per kata ala "enhanced LRC": `<00:12.34>`.
fn strip_word_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '<' {
            let mut tag = String::new();
            let mut closed = false;
            for n in chars.by_ref() {
                if n == '>' {
                    closed = true;
                    break;
                }
                tag.push(n);
            }
            if closed && parse_timestamp(&tag).is_some() {
                continue; // buang tag waktu
            }
            out.push('<');
            out.push_str(&tag);
            if closed {
                out.push('>');
            }
        } else {
            out.push(c);
        }
    }
    out
}

// ---------- pembersihan judul/artis ----------

fn clean_title(s: &str) -> String {
    let mut t = s.trim().to_string();

    // Buang sufiks dalam kurung yang biasanya mengganggu pencarian.
    const NOISE: [&str; 9] = [
        "official", "video", "audio", "lyric", "lyrics", "visualizer", "hd", "4k", "remaster",
    ];
    loop {
        let open = t.rfind(|c| c == '(' || c == '[');
        let Some(i) = open else { break };
        let tail = t[i..].to_lowercase();
        if tail.ends_with(')') || tail.ends_with(']') {
            if NOISE.iter().any(|n| tail.contains(n)) {
                t.truncate(i);
                t = t.trim_end().to_string();
                continue;
            }
        }
        break;
    }
    t.trim().to_string()
}

fn clean_artist(s: &str) -> String {
    let a = s.trim();
    a.strip_suffix(" - Topic").unwrap_or(a).trim().to_string()
}

// ---------- tes ----------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic() {
        let l = parse_lrc("[ar:Foo]\n[00:12.50]Halo\n[01:02.5][00:05.00]Dunia\n[00:20.00]");
        assert_eq!(l.len(), 4);
        assert_eq!(l[0].text, "Dunia");
        assert!((l[0].t - 5.0).abs() < 1e-6);
        assert!((l[2].t - 20.0).abs() < 1e-6);
        assert_eq!(l[2].text, "");
        assert!((l[3].t - 62.5).abs() < 1e-6);
    }

    #[test]
    fn word_tags() {
        assert_eq!(strip_word_tags("<00:01.00>Ha<00:01.50>lo"), "Halo");
    }

    #[test]
    fn title_cleaning() {
        assert_eq!(clean_title("Lagu Ini (Official Video)"), "Lagu Ini");
        assert_eq!(clean_title("Lagu (feat. X)"), "Lagu (feat. X)");
    }
}
