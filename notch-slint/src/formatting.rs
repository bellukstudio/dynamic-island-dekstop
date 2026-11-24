//! Pemformat teks umum (tanggal/bulan Indonesia, uptime, laju data, angka) dan utilitas kecil.

use crate::*;

pub(crate) const ID_DAYS: [&str; 7] = ["Minggu", "Senin", "Selasa", "Rabu", "Kamis", "Jumat", "Sabtu"];
pub(crate) const ID_MONTHS: [&str; 12] = [
    "Januari", "Februari", "Maret", "April", "Mei", "Juni",
    "Juli", "Agustus", "September", "Oktober", "November", "Desember",
];

pub(crate) const SHORT_MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "Mei", "Jun", "Jul", "Agu", "Sep", "Okt", "Nov", "Des"];

/// "3h 4j" / "5j 12m" / "9m" (sama dengan fmtUptime di Tauri).
pub(crate) fn fmt_uptime(secs: u64) -> String {
    let (d, h, m) = (secs / 86_400, (secs / 3600) % 24, (secs / 60) % 60);
    if d > 0 { format!("{d}h {h}j") } else if h > 0 { format!("{h}j {m}m") } else { format!("{m}m") }
}

pub(crate) fn gib(bytes: f64) -> f64 {
    bytes / (1024.0 * 1024.0 * 1024.0)
}

/// Kecepatan jaringan dalam B/s -> teks pendek (KB/s atau MB/s).
pub(crate) fn fmt_rate(bps: f64) -> String {
    if bps >= 1024.0 * 1024.0 {
        format!("{:.1} MB/s", bps / (1024.0 * 1024.0))
    } else {
        format!("{:.0} KB/s", bps / 1024.0)
    }
}

/// Isi daftar catatan di UI (maksimal 3 terbaru; halaman ini belum bisa menggulir).
/// "Baru saja" / "5 mnt lalu" / "3 jam lalu" / "2 hari lalu" / "17 Nov" (sama dengan fmtNoteDate di Tauri).
pub(crate) fn fmt_note_date(updated_ms: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(updated_ms);
    let min = ((now - updated_ms) / 60_000).max(0);
    if min < 1 {
        return "Baru saja".into();
    }
    if min < 60 {
        return format!("{min} mnt lalu");
    }
    let h = min / 60;
    if h < 24 {
        return format!("{h} jam lalu");
    }
    let d = h / 24;
    if d < 7 {
        return format!("{d} hari lalu");
    }
    let (_, mo, day) = platform::civil_from_days(updated_ms.div_euclid(86_400_000));
    const SHORT: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "Mei", "Jun", "Jul", "Agu", "Sep", "Okt", "Nov", "Des"];
    format!("{day} {}", SHORT[(mo.clamp(1, 12) - 1) as usize])
}

/// "1234567" -> "1.234.567".
pub(crate) fn group_digits(digits: &str) -> String {
    let n = digits.chars().count();
    let mut out = String::with_capacity(n + n / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (n - i) % 3 == 0 {
            out.push('.');
        }
        out.push(c);
    }
    out
}

/// Format angka gaya Indonesia: titik ribuan, koma desimal.
pub(crate) fn fmt_id_number(value: f64, decimals: usize) -> String {
    let text = format!("{:.*}", decimals, value.abs());
    let (int, frac) = match text.split_once('.') {
        Some((a, b)) => (a.to_string(), Some(b.to_string())),
        None => (text.clone(), None),
    };
    let mut out = group_digits(&int);
    if let Some(f) = frac {
        out.push(',');
        out.push_str(&f);
    }
    if value < 0.0 {
        out.insert(0, '-');
    }
    out
}

pub(crate) fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 3);
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

pub(crate) fn local_timezone_offset_minutes() -> i32 {
    #[cfg(windows)]
    let output = {
        use std::os::windows::process::CommandExt;
        std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", "(Get-Date).ToString('zzz')"])
            .creation_flags(0x0800_0000)
            .output()
    };
    #[cfg(not(windows))]
    let output = std::process::Command::new("date").arg("+%z").output();

    let Ok(output) = output else { return 0 };
    let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let compact = raw.replace(':', "");
    if compact.len() != 5 { return 0; }
    let sign = if compact.starts_with('-') { -1 } else if compact.starts_with('+') { 1 } else { return 0 };
    let hours = compact[1..3].parse::<i32>().unwrap_or(0);
    let minutes = compact[3..5].parse::<i32>().unwrap_or(0);
    -(sign * (hours * 60 + minutes))
}
