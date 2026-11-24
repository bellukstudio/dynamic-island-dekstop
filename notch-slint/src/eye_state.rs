//! Konfigurasi kanvas mata dan state animasinya (RNG kecil + `EyeCtx`).

use crate::*;

// Canvas mata mengikuti ukuran notch sesuai orientasi dock.
pub(crate) const EYES_W: f32 = 130.0;
pub(crate) const EYES_H: f32 = 34.0;
pub(crate) const EYES_TOP_H: f32 = 26.0;

pub(crate) fn eye_renderer_for_dock(skin: &str, dock: i32) -> (EyeRenderer, (f32, f32)) {
    let size = match dock {
        0 => (EYES_W, EYES_TOP_H),
        3 | 4 => (EYES_H, EYES_W),
        _ => (EYES_W, EYES_H),
    };
    let renderer = if matches!(dock, 3 | 4) {
        EyeRenderer::new_vertical(skin, size.0, size.1)
    } else {
        EyeRenderer::new(skin, size.0, size.1)
    };
    (renderer, size)
}

/// Pengacak ringan untuk kedip dan tatapan acak (tanpa dependensi `rand`).
pub(crate) struct Rng(pub(crate) Cell<u64>);
impl Rng {
    pub(crate) fn new() -> Rng {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E37_79B9_7F4A_7C15);
        Rng(Cell::new(seed | 1))
    }
    /// 0.0 ..< 1.0
    pub(crate) fn next(&self) -> f32 {
        let mut x = self.0.get();
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0.set(x);
        (x >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// Seluruh keadaan mata: mood, transisi, tatapan, kedip, dan frame terakhir yang digambar.
pub(crate) struct EyeCtx {
    pub(crate) renderer: EyeRenderer,
    pub(crate) canvas_size: (f32, f32),
    pub(crate) skin: usize,
    pub(crate) mood: MoodState,
    pub(crate) cur: Mood,
    pub(crate) cur_since: f32,
    pub(crate) morph: Morph,
    pub(crate) look: (f32, f32),
    pub(crate) look_target: (f32, f32),
    pub(crate) last_frame_ts: f32,
    pub(crate) last_cursor: (i32, i32),
    pub(crate) last_move: f32,
    pub(crate) next_random_look: f32,
    pub(crate) blink_until: f32,
    pub(crate) next_blink: f32,
    pub(crate) last_key: Option<(i32, i32, bool, usize, i64, u32)>,
    pub(crate) rng: Rng,
}
