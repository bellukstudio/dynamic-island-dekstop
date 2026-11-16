//! Text-to-speech untuk sapaan Notch. Menggunakan Windows System.Speech tanpa WebView/Tauri.

#[cfg(windows)]
pub fn speak(text: String, voice: String) -> Result<(), String> {
    let text = text.chars().take(240).collect::<String>();
    if text.trim().is_empty() { return Ok(()); }
    std::thread::Builder::new()
        .name("notch-greeting-tts".into())
        .spawn(move || {
            let quote = |value: &str| value.replace('\'', "''");
            let safe_text = quote(&text);
            let safe_voice = quote(&voice);
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
            use std::io::Write;
            use std::process::{Command, Stdio};
            let encoded = {
                let mut bytes = Vec::with_capacity(script.len() * 2);
                for unit in script.encode_utf16() { bytes.extend_from_slice(&unit.to_le_bytes()); }
                base64_encode(&bytes)
            };
            let mut child = match Command::new("powershell.exe")
                .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden", "-EncodedCommand", &encoded])
                .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
                Ok(child) => child,
                Err(_) => return,
            };
            let _ = child.wait();
        })
        .map(|_| ())
        .map_err(|error| format!("Tidak dapat memulai audio greeting: {error}"))
}

#[cfg(not(windows))]
pub fn speak(_text: String, _voice: String) -> Result<(), String> {
    Err("Audio greeting tersedia pada Windows.".into())
}

pub fn greeting_text(mode: &str, custom_text: &str, hour: u32) -> Option<String> {
    let part = if (4..11).contains(&hour) { "pagi" }
        else if (11..15).contains(&hour) { "siang" }
        else if (15..18).contains(&hour) { "sore" }
        else { "malam" };
    match mode {
        "halo" => Some("Halo!".into()),
        "morning" => Some("Selamat pagi!".into()),
        "noon" => Some("Selamat siang!".into()),
        "afternoon" => Some("Selamat sore!".into()),
        "evening" => Some("Selamat malam!".into()),
        "custom" if !custom_text.trim().is_empty() => Some(custom_text.trim().chars().take(120).collect()),
        "auto" => Some(format!("Halo! Selamat {part}!")),
        "random" => Some(format!("Halo! Selamat {part}, NPC kesayangan!")),
        _ => None,
    }
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
    use super::greeting_text;

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
    fn unknown_greeting_mode_is_rejected() {
        assert_eq!(greeting_text("invalid", "", 10), None);
    }
}
