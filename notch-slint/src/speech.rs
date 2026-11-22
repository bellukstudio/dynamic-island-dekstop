//! Greeting untuk Notch native.
//! Sapaan standar memakai klip MP3 neural dari public/greeting + manifest.json.
//! Teks kustom / preset sistem memakai Windows System.Speech sebagai fallback.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GreetingContent {
    pub message: String,
    pub detail: String,
    pub spoken: String,
}

type GreetingManifest = HashMap<String, Vec<String>>;

static AUDIO_LOCK: Mutex<()> = Mutex::new(());
static LAST_RANDOM_PHRASE: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(usize::MAX);

const PHRASES: &[(&str, &str, Option<u32>, Option<u32>)] = &[
    ("{waktu}, NPC kesayangan!", "Dunia sudah loading, masa kamu belum?", None, None),
    ("Welcome back, legend.", "Akhirnya pemeran utamanya muncul.", None, None),
    ("Wih, masih hidup!", "Server kehidupan masih online rupanya.", None, None),
    ("{bonjour}, manusia sibuk.", "Sibuk beneran atau cuma buka tab baru?", None, None),
    ("Selamat datang kembali.", "Kami pura-pura nggak nungguin.", None, None),
    ("Aura +1000.", "Tinggal produktifnya yang ditingkatkan.", None, None),
    ("Look who's here.", "Notch kecil, ego besar.", None, None),
    ("Hari baru, kamu lagi.", "Semoga hari ini bukan copy-paste kemarin.", Some(4), Some(11)),
    ("Hello, world!", "Error juga ikut menyambutmu.", None, None),
    ("Compile dulu, panik kemudian.", "Kita profesional, katanya.", None, None),
    ("Bug hunter detected.", "Semoga hari ini bukan bug yang berburu balik.", None, None),
    ("Git push, then pray.", "Ritual leluhur software engineer.", None, None),
    ("Code mode: ON.", "Produktivitas sedang dinegosiasikan.", None, None),
    ("404: Motivasi not found.", "Coba restart manusia.", None, None),
    ("Works on my machine.", "Kalimat terakhir sebelum meeting panjang.", None, None),
    ("Commit your feelings.", "Jangan semuanya disimpan di local.", None, None),
    ("Semangat, full-stack warrior.", "Frontend menangis, backend menyusul.", None, None),
    ("Satu bug lagi.", "Kalimat yang mengakhiri tiga jam hidupmu.", None, None),
    ("Psst... manusia.", "Iya, kamu. Jangan pura-pura nggak lihat.", None, None),
    ("Aku melihatmu.", "Bukan CCTV, cuma penasaran.", None, None),
    ("Produktif dikit, boleh?", "Biar hari ini punya plot.", None, None),
    ("Bro is back.", "Timeline kembali masuk akal.", None, None),
    ("Main character energy.", "Background music belum tersedia.", None, None),
    ("Kamu terlihat sibuk.", "Alt-tab bukan olahraga kardio.", None, None),
    ("Tenang, Bos.", "Masalahnya belum tentu tenang.", None, None),
    ("Plot twist hari ini?", "Kamu menyelesaikan pekerjaan tepat waktu.", None, None),
    ("Aura terdeteksi.", "Sumbernya belum diketahui.", None, None),
    ("Jangan overthinking.", "CPU otakmu sudah 98%.", None, None),
    ("Santai dulu, komandan.", "Tidak semua masalah perlu di-fix sekarang.", None, None),
    ("Achievement unlocked.", "Kamu berhasil membuka aplikasi lagi.", None, None),
    ("Masih melek, chief?", "Windows saja tahu kapan harus sleep.", Some(22), Some(4)),
    ("Night shift activated.", "Besok jangan salahkan alarm.", Some(19), Some(4)),
    ("Tidur adalah fitur premium.", "Dan kamu belum berlangganan.", Some(22), Some(4)),
    ("Last task, katanya.", "Kita tahu ending cerita ini.", Some(19), Some(4)),
    ("Sudah malam, bro.", "Bug boleh lembur, kamu jangan.", Some(19), Some(4)),
    ("Save your progress.", "Termasuk progres kesehatanmu.", Some(20), Some(4)),
    ("Closing time.", "Laptop boleh ditutup, pikiran juga.", Some(17), Some(4)),
    ("Good night, legend.", "Semoga mimpi tanpa stack trace.", Some(21), Some(4)),
];

fn part_of_day(hour: u32) -> &'static str {
    if (4..11).contains(&hour) { "pagi" }
    else if (11..15).contains(&hour) { "siang" }
    else if (15..18).contains(&hour) { "sore" }
    else { "malam" }
}

fn capitalize(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn in_hours(hour: u32, from: u32, to: u32) -> bool {
    if from <= to { hour >= from && hour < to }
    else { hour >= from || hour < to }
}

fn random_phrase(hour: u32) -> GreetingContent {
    let part = part_of_day(hour);
    let previous = LAST_RANDOM_PHRASE.load(std::sync::atomic::Ordering::Relaxed);
    let mut candidates = PHRASES.iter().enumerate().filter_map(|(index, phrase)| {
        if index == previous { return None; }
        let (_, _, from, to) = phrase;
        let weight = match (from, to) {
            (Some(start), Some(end)) if in_hours(hour, *start, *end) => 3_u64,
            (Some(_), Some(_)) => return None,
            _ => 1_u64,
        };
        Some((index, weight))
    }).collect::<Vec<_>>();
    if candidates.is_empty() {
        candidates = PHRASES.iter().enumerate().map(|(index, _)| (index, 1)).collect();
    }
    let total_weight = candidates.iter().map(|(_, weight)| *weight).sum::<u64>().max(1);
    let time_seed = SystemTime::now().duration_since(UNIX_EPOCH)
        .unwrap_or_default().as_nanos() as u64;
    let mut pick = (time_seed ^ (std::process::id() as u64).rotate_left(13)) % total_weight;
    let mut selected = candidates[0].0;
    for (index, weight) in candidates {
        if pick < weight { selected = index; break; }
        pick -= weight;
    }
    LAST_RANDOM_PHRASE.store(selected, std::sync::atomic::Ordering::Relaxed);

    let (raw_title, subtitle, _, _) = PHRASES[selected];
    let title = raw_title
        .replace("{waktu}", &capitalize(part))
        .replace("{bonjour}", if part == "malam" { "Bonsoir" } else { "Bonjour" });
    let spoken = format!("{title} {subtitle}");
    GreetingContent { message: title, detail: subtitle.to_string(), spoken }
}

pub fn greeting_content(mode: &str, custom_text: &str, hour: u32) -> Option<GreetingContent> {
    if mode == "random" { return Some(random_phrase(hour)); }
    let part = part_of_day(hour);
    let message = match mode {
        "halo" => "Halo!".to_string(),
        "morning" => "Selamat pagi!".to_string(),
        "noon" => "Selamat siang!".to_string(),
        "afternoon" => "Selamat sore!".to_string(),
        "evening" => "Selamat malam!".to_string(),
        "custom" if !custom_text.trim().is_empty() => custom_text.trim().chars().take(120).collect(),
        "auto" => format!("Halo! Selamat {part}!"),
        _ => return None,
    };
    let detail = if mode == "custom" { "Sapaan kustom" } else { "Notch siap menemani" };
    Some(GreetingContent { spoken: message.clone(), message, detail: detail.to_string() })
}

pub fn greeting_text(mode: &str, custom_text: &str, hour: u32) -> Option<String> {
    greeting_content(mode, custom_text, hour).map(|greeting| greeting.spoken)
}

pub fn estimated_duration_ms(text: &str) -> u64 {
    let words = text.split_whitespace().count() as u64;
    (1_800 + words * 470).clamp(3_500, 11_000)
}

fn normalized(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn greeting_directory() -> Option<PathBuf> {
    std::env::current_exe().ok()?.parent().map(|parent| parent.join("greeting"))
}

fn manifest() -> Option<&'static GreetingManifest> {
    static MANIFEST: OnceLock<Option<GreetingManifest>> = OnceLock::new();
    MANIFEST.get_or_init(|| {
        let path = greeting_directory()?.join("manifest.json");
        let raw = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&raw).ok()
    }).as_ref()
}

fn resource_clips(text: &str, voice: &str) -> Option<Vec<PathBuf>> {
    if voice == "system" { return None; }
    let key = format!("{voice}|{}", normalized(text));
    let files = manifest()?.get(&key)?;
    let directory = greeting_directory()?;
    let paths = files.iter().map(|name| {
        let path_part = Path::new(name);
        if path_part.components().count() != 1 || path_part.extension().and_then(|ext| ext.to_str()) != Some("mp3") {
            return None;
        }
        let path = directory.join(path_part);
        path.is_file().then_some(path)
    }).collect::<Option<Vec<_>>>()?;
    if paths.is_empty() { None } else { Some(paths) }
}

#[cfg(windows)]
fn mci_send(command: &str) -> Result<(), String> {
    use windows::core::PCWSTR;
    use windows::Win32::Media::Multimedia::mciSendStringW;
    let wide = command.encode_utf16().chain(std::iter::once(0)).collect::<Vec<_>>();
    let status = unsafe { mciSendStringW(PCWSTR(wide.as_ptr()), None, None) };
    if status == 0 { Ok(()) } else { Err(format!("MCI gagal ({status}) untuk perintah audio")) }
}

#[cfg(windows)]
fn play_resource_clips(paths: &[PathBuf]) -> Result<(), String> {
    let _guard = AUDIO_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    for (index, path) in paths.iter().enumerate() {
        let alias = format!("notchgreet{index}");
        let file = path.to_string_lossy();
        // Driver mpegvideo membuat jendela ActiveMovie berbingkai (tombol - [] x) yang tampak putih karena thread ini
        // tidak memompa pesan. `nowindow` mencegah jendela dibuat; bila ditolak driver, buka biasa lalu sembunyikan
        // segera. Hanya audio yang diputar, jadi jendela itu memang tidak diperlukan.
        mci_send(&format!("open \"{file}\" type mpegvideo alias {alias} nowindow"))
            .or_else(|_| mci_send(&format!("open \"{file}\" type mpegvideo alias {alias}")))?;
        let _ = mci_send(&format!("window {alias} state hide"));
        let played = mci_send(&format!("play {alias} wait"));
        let closed = mci_send(&format!("close {alias}"));
        played?;
        closed?;
    }
    Ok(())
}

#[cfg(windows)]
fn speak_system(text: &str, voice: &str) -> Result<(), String> {
    let quote = |value: &str| value.replace('\'', "''");
    let safe_text = quote(text);
    let safe_voice = quote(voice);
    let script = format!(r#"
Add-Type -AssemblyName System.Speech
$synth = New-Object System.Speech.Synthesis.SpeechSynthesizer
try {{
  $wanted = '{safe_voice}'
  if ($wanted -ne 'system') {{
    $available = @($synth.GetInstalledVoices() | ForEach-Object {{ $_.VoiceInfo.Name }})
    $pattern = switch ($wanted) {{
      'cewek' {{ 'female|zira|aria|jenny|susan|hazel|eva' }}
      'cowok' {{ 'male|david|guy|mark|james|ryan' }}
      'anime-cewek' {{ 'anime.*female|female.*anime|girl|kawaii' }}
      'anime-cowok' {{ 'anime.*male|male.*anime|boy' }}
      default {{ '' }}
    }}
    if ($pattern) {{
      $match = $available | Where-Object {{ $_ -match $pattern }} | Select-Object -First 1
      if ($match) {{ $synth.SelectVoice($match) }}
    }}
  }}
  $synth.Rate = 0
  $synth.Volume = 85
  $synth.Speak('{safe_text}')
}} finally {{ $synth.Dispose() }}
"#);
    use std::process::{Command, Stdio};
    let encoded = {
        let mut bytes = Vec::with_capacity(script.len() * 2);
        for unit in script.encode_utf16() { bytes.extend_from_slice(&unit.to_le_bytes()); }
        base64_encode(&bytes)
    };
    // `-WindowStyle Hidden` baru menyembunyikan konsol SETELAH PowerShell hidup, jadi jendela konsol (judul putih,
    // tombol - [] x) sempat berkedip saat greeting. CREATE_NO_WINDOW mencegah konsol dibuat sama sekali, seperti
    // semua pemanggilan proses lain di proyek ini.
    use std::os::windows::process::CommandExt;
    let mut child = Command::new("powershell.exe")
        .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden", "-EncodedCommand", &encoded])
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
        .creation_flags(0x0800_0000)
        .spawn()
        .map_err(|error| format!("Tidak dapat memulai audio greeting: {error}"))?;
    child.wait().map_err(|error| format!("Audio greeting gagal: {error}"))?;
    Ok(())
}

#[cfg(windows)]
pub fn speak(text: String, voice: String) -> Result<(), String> {
    let text = text.chars().take(240).collect::<String>();
    if text.trim().is_empty() { return Ok(()); }
    std::thread::Builder::new()
        .name("notch-greeting-audio".into())
        .spawn(move || {
            if let Some(paths) = resource_clips(&text, &voice) {
                match play_resource_clips(&paths) {
                    Ok(()) => return,
                    Err(error) => eprintln!("[Greeting] Klip MP3 gagal, memakai suara sistem: {error}"),
                }
            }
            if let Err(error) = speak_system(&text, &voice) {
                eprintln!("[Greeting] Suara sistem gagal: {error}");
            }
        })
        .map(|_| ())
        .map_err(|error| format!("Tidak dapat memulai audio greeting: {error}"))
}

#[cfg(not(windows))]
pub fn speak(_text: String, _voice: String) -> Result<(), String> {
    Err("Audio greeting tersedia pada Windows.".into())
}

fn base64_encode(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let a = chunk[0] as u32;
        let b = chunk.get(1).copied().unwrap_or(0) as u32;
        let c = chunk.get(2).copied().unwrap_or(0) as u32;
        let n = (a << 16) | (b << 8) | c;
        output.push(TABLE[((n >> 18) & 63) as usize] as char);
        output.push(TABLE[((n >> 12) & 63) as usize] as char);
        output.push(if chunk.len() > 1 { TABLE[((n >> 6) & 63) as usize] as char } else { '=' });
        output.push(if chunk.len() > 2 { TABLE[(n & 63) as usize] as char } else { '=' });
    }
    output
}

#[cfg(test)]
mod tests {
    use super::{greeting_content, greeting_text};

    #[test]
    fn resolves_time_aware_greeting() {
        assert_eq!(greeting_text("auto", "", 7).as_deref(), Some("Halo! Selamat pagi!"));
        assert_eq!(greeting_text("auto", "", 13).as_deref(), Some("Halo! Selamat siang!"));
        assert_eq!(greeting_text("auto", "", 17).as_deref(), Some("Halo! Selamat sore!"));
        assert_eq!(greeting_text("auto", "", 20).as_deref(), Some("Halo! Selamat malam!"));
    }

    #[test]
    fn custom_greeting_is_trimmed_and_bounded() {
        assert_eq!(greeting_text("custom", "  Halo Lukman!  ", 10).as_deref(), Some("Halo Lukman!"));
        assert_eq!(greeting_text("custom", "   ", 10), None);
    }

    #[test]
    fn random_greeting_has_matching_title_subtitle_and_audio_text() {
        let greeting = greeting_content("random", "", 13).unwrap();
        assert!(!greeting.message.is_empty());
        assert!(!greeting.detail.is_empty());
        assert_eq!(greeting.spoken, format!("{} {}", greeting.message, greeting.detail));
    }

    #[test]
    fn unknown_greeting_mode_is_rejected() {
        assert_eq!(greeting_text("invalid", "", 10), None);
    }
}
