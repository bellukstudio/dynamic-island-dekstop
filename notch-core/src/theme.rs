//! Palet warna notch dari `[appearance]` di config.toml. Tidak bergantung pada UI.
//! Nilai tema siap pakai mengikuti token di `src/css/notch-themes.css` (versi Tauri).

use crate::config::Appearance;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }
    pub const TRANSPARENT: Rgba = Rgba::new(0, 0, 0, 0);
    pub fn with_alpha(self, alpha: f32) -> Self {
        Self { a: (alpha.clamp(0.0, 1.0) * 255.0).round() as u8, ..self }
    }
    /// Kecerahan 0..1 (rumus yang sama dengan `luma` di frontend: bobot Rec.709).
    pub fn luma(self) -> f32 {
        (0.2126 * f32::from(self.r) + 0.7152 * f32::from(self.g) + 0.0722 * f32::from(self.b)) / 255.0
    }
}

/// "#rrggbb" atau "#rgb" (huruf besar/kecil). Selain itu `None`.
pub fn hex_rgb(s: &str) -> Option<Rgba> {
    let h = s.trim().strip_prefix('#')?;
    let v = |i: usize, n: usize| u8::from_str_radix(h.get(i..i + n)?, 16).ok();
    match h.len() {
        6 => Some(Rgba::rgb(v(0, 2)?, v(2, 2)?, v(4, 2)?)),
        3 => Some(Rgba::rgb(v(0, 1)? * 17, v(1, 1)? * 17, v(2, 1)? * 17)),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    pub body: Rgba,
    pub text: Rgba,
    pub accent: Rgba,
    pub on_accent: Rgba, // teks di atas warna aksen
    pub ok: Rgba,
    pub text_dim: Rgba,
    pub text_faint: Rgba,
    pub chip: Rgba,
    pub chip_hover: Rgba,
    pub track: Rgba,
    pub glow_a: Rgba, // cahaya bawah-kiri
    pub glow_b: Rgba, // cahaya bawah-kanan
    pub rim: Rgba,    // garis tepi tipis
}

struct Base {
    body: Rgba,
    text: Rgba,
    accent: Rgba,
    glow_a: Rgba,
    glow_b: Rgba,
    rim: Rgba,
}

const DEFAULT_ACCENT: Rgba = Rgba::rgb(120, 140, 255); // --accent di base.css

impl Palette {
    /// `os_light`: apakah OS memakai tema terang (dipakai untuk `theme = "auto"`).
    pub fn from_appearance(a: &Appearance, os_light: bool) -> Self {
        let base = match a.notch_theme.as_str() {
            "aurora" => Base {
                body: Rgba::rgb(0x07, 0x0b, 0x1a),
                text: Rgba::rgb(232, 244, 255),
                accent: Rgba::rgb(82, 226, 200),
                glow_a: Rgba::rgb(82, 226, 200).with_alpha(0.50),
                glow_b: Rgba::rgb(139, 92, 246).with_alpha(0.56),
                rim: Rgba::rgb(130, 235, 255).with_alpha(0.16),
            },
            "obsidian" => Base {
                body: Rgba::rgb(0x0c, 0x0c, 0x0e),
                text: Rgba::rgb(246, 239, 224),
                accent: Rgba::rgb(240, 190, 90),
                glow_a: Rgba::rgb(240, 190, 90).with_alpha(0.30),
                glow_b: Rgba::rgb(240, 190, 90).with_alpha(0.30),
                rim: Rgba::rgb(240, 190, 90).with_alpha(0.28),
            },
            "neon" => Base {
                body: Rgba::rgb(0x0a, 0x07, 0x14),
                text: Rgba::rgb(244, 234, 255),
                accent: Rgba::rgb(0, 229, 255),
                glow_a: Rgba::rgb(255, 43, 214).with_alpha(0.46),
                glow_b: Rgba::rgb(0, 229, 255).with_alpha(0.36),
                rim: Rgba::rgb(0, 229, 255).with_alpha(0.45),
            },
            "sakura" => Base {
                body: Rgba::rgb(0xff, 0xe4, 0xef),
                text: Rgba::rgb(90, 35, 64),
                accent: Rgba::rgb(255, 92, 150),
                glow_a: Rgba::rgb(255, 92, 150).with_alpha(0.22),
                glow_b: Rgba::rgb(255, 92, 150).with_alpha(0.22),
                rim: Rgba::rgb(255, 120, 170).with_alpha(0.55),
            },
            "matcha" => Base {
                body: Rgba::rgb(0xd9, 0xf5, 0xdf),
                text: Rgba::rgb(23, 74, 44),
                accent: Rgba::rgb(38, 166, 91),
                glow_a: Rgba::rgb(38, 166, 91).with_alpha(0.22),
                glow_b: Rgba::rgb(38, 166, 91).with_alpha(0.22),
                rim: Rgba::rgb(38, 166, 91).with_alpha(0.50),
            },
            _ => {
                // "default": warna manual dari theme / dark_bg / light_bg / background
                let light = a.theme == "light" || (a.theme == "auto" && os_light);
                let bg_hex = if a.theme == "custom" {
                    &a.background
                } else if light {
                    &a.light_bg
                } else {
                    &a.dark_bg
                };
                let body = hex_rgb(bg_hex).unwrap_or(Rgba::rgb(0, 0, 0));
                let text = hex_rgb(&a.text).unwrap_or(if body.luma() > 0.55 {
                    Rgba::rgb(17, 17, 17)
                } else {
                    Rgba::rgb(255, 255, 255)
                });
                Base {
                    body,
                    text,
                    accent: DEFAULT_ACCENT,
                    glow_a: Rgba::TRANSPARENT,
                    glow_b: Rgba::TRANSPARENT,
                    rim: Rgba::TRANSPARENT,
                }
            }
        };

        // Aksen manual dari Pengaturan selalu menang atas aksen tema.
        let accent = hex_rgb(&a.accent).unwrap_or(base.accent);
        let light_body = base.body.luma() > 0.55;
        Palette {
            body: base.body,
            text: base.text,
            accent,
            on_accent: if accent.luma() > 0.55 { Rgba::rgb(0, 0, 0) } else { Rgba::rgb(255, 255, 255) },
            ok: if light_body { Rgba::rgb(38, 150, 91) } else { Rgba::rgb(123, 220, 154) },
            text_dim: base.text.with_alpha(0.62),
            text_faint: base.text.with_alpha(0.42),
            chip: base.text.with_alpha(0.10),
            chip_hover: base.text.with_alpha(0.18),
            track: base.text.with_alpha(0.12),
            glow_a: base.glow_a,
            glow_b: base.glow_b,
            rim: base.rim,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_parsing() {
        assert_eq!(hex_rgb("#1e1e2e"), Some(Rgba::rgb(0x1e, 0x1e, 0x2e)));
        assert_eq!(hex_rgb(" #FFF "), Some(Rgba::rgb(255, 255, 255)));
        assert_eq!(hex_rgb("#abc"), Some(Rgba::rgb(0xaa, 0xbb, 0xcc)));
        assert_eq!(hex_rgb(""), None);
        assert_eq!(hex_rgb("1e1e2e"), None);
        assert_eq!(hex_rgb("#12345"), None);
        assert_eq!(hex_rgb("#gggggg"), None);
    }

    #[test]
    fn default_is_plain_black_with_white_text_and_no_glow() {
        let p = Palette::from_appearance(&Appearance::default(), false);
        assert_eq!(p.body, Rgba::rgb(0, 0, 0));
        assert_eq!(p.text, Rgba::rgb(255, 255, 255));
        assert_eq!(p.accent, Rgba::rgb(120, 140, 255));
        assert_eq!((p.glow_a, p.glow_b, p.rim), (Rgba::TRANSPARENT, Rgba::TRANSPARENT, Rgba::TRANSPARENT));
    }

    #[test]
    fn auto_follows_os_but_dark_ignores_it() {
        let mut a = Appearance::default();
        a.theme = "auto".into();
        assert_eq!(Palette::from_appearance(&a, true).body, Rgba::rgb(0xf5, 0xf5, 0xf7));
        assert_eq!(Palette::from_appearance(&a, true).text, Rgba::rgb(17, 17, 17));
        a.theme = "dark".into();
        assert_eq!(Palette::from_appearance(&a, true).body, Rgba::rgb(0, 0, 0));
    }

    #[test]
    fn custom_background_and_manual_accent_win() {
        let mut a = Appearance::default();
        a.theme = "custom".into();
        a.background = "#1e1e2e".into();
        a.accent = "#ff6b6b".into();
        let p = Palette::from_appearance(&a, false);
        assert_eq!(p.body, Rgba::rgb(0x1e, 0x1e, 0x2e));
        assert_eq!(p.accent, Rgba::rgb(0xff, 0x6b, 0x6b));
        // aksen manual juga menang atas aksen tema siap pakai
        a.notch_theme = "neon".into();
        assert_eq!(Palette::from_appearance(&a, false).accent, Rgba::rgb(0xff, 0x6b, 0x6b));
    }

    #[test]
    fn ready_made_themes_use_their_tokens() {
        let mut a = Appearance::default();
        a.notch_theme = "neon".into();
        let p = Palette::from_appearance(&a, false);
        assert_eq!(p.body, Rgba::rgb(0x0a, 0x07, 0x14));
        assert_eq!(p.accent, Rgba::rgb(0, 229, 255));
        assert_ne!(p.glow_a, Rgba::TRANSPARENT);
        a.notch_theme = "sakura".into();
        let s = Palette::from_appearance(&a, false);
        assert_eq!(s.body, Rgba::rgb(0xff, 0xe4, 0xef));
        assert!(s.body.luma() > 0.55);
        assert_eq!(s.on_accent, Rgba::rgb(255, 255, 255)); // luma aksen pink ~0,51 (< 0,55) -> teks putih
    }
}
