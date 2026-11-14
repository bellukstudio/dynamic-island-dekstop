//! Timer Pomodoro sebagai state machine murni (tanpa UI). Aturannya sama dengan
//! `src/main-modules/pomodoro.ts`: fokus -> istirahat -> ... -> istirahat panjang setelah ronde
//! terakhir, lalu ronde kembali ke 1. Pergantian fase tidak otomatis menjalankan timer berikutnya.

use crate::config::Pomodoro as PomodoroCfg;
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Focus,
    Break,
    Long,
}

pub struct Pomodoro {
    cfg: PomodoroCfg,
    phase: Phase,
    round: u32,
    remaining: u32, // detik
    end_at: Option<Instant>,
}

impl Pomodoro {
    pub fn new(cfg: &PomodoroCfg) -> Self {
        let mut cfg = cfg.clone();
        // nilai 0 membuat pembagian nol di progress(): paksa minimal 1
        cfg.focus_min = cfg.focus_min.max(1);
        cfg.break_min = cfg.break_min.max(1);
        cfg.long_break_min = cfg.long_break_min.max(1);
        cfg.rounds = cfg.rounds.max(1);
        let remaining = cfg.focus_min * 60;
        Self { cfg, phase: Phase::Focus, round: 1, remaining, end_at: None }
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn round(&self) -> u32 {
        self.round
    }
    pub fn remaining(&self) -> u32 {
        self.remaining
    }
    pub fn running(&self) -> bool {
        self.end_at.is_some()
    }

    pub fn phase_len(&self, p: Phase) -> u32 {
        60 * match p {
            Phase::Focus => self.cfg.focus_min,
            Phase::Break => self.cfg.break_min,
            Phase::Long => self.cfg.long_break_min,
        }
    }

    pub fn time_text(&self) -> String {
        format!("{:02}:{:02}", self.remaining / 60, self.remaining % 60)
    }

    pub fn label(&self) -> String {
        match self.phase {
            Phase::Focus => format!("Fokus · ronde {}/{}", self.round, self.cfg.rounds),
            Phase::Break => "Istirahat".to_string(),
            Phase::Long => "Istirahat panjang".to_string(),
        }
    }

    /// 0.0..=1.0, bagian fase yang sudah berlalu.
    pub fn progress(&self) -> f32 {
        let total = self.phase_len(self.phase) as f32;
        ((total - self.remaining as f32) / total).clamp(0.0, 1.0)
    }

    pub fn toggle(&mut self, now: Instant) {
        if self.running() {
            self.end_at = None;
        } else {
            self.end_at = Some(now + std::time::Duration::from_secs(u64::from(self.remaining)));
        }
    }

    /// Hentikan dan ulangi fase ini dari awal.
    pub fn reset(&mut self) {
        self.end_at = None;
        self.remaining = self.phase_len(self.phase);
    }

    /// Lompat ke fase berikutnya (tombol skip).
    pub fn skip(&mut self) {
        self.next_phase();
    }

    /// Panggil tiap ~1 detik. Mengembalikan `Some(fase_yang_selesai)` saat waktu habis
    /// dan fase otomatis pindah (timer berhenti).
    pub fn tick(&mut self, now: Instant) -> Option<Phase> {
        let end = self.end_at?;
        let left = end.saturating_duration_since(now).as_secs_f64().round() as u32;
        self.remaining = left;
        if left == 0 {
            let finished = self.phase;
            self.next_phase();
            return Some(finished);
        }
        None
    }

    fn next_phase(&mut self) {
        self.end_at = None;
        match self.phase {
            Phase::Focus => {
                self.phase = if self.round >= self.cfg.rounds { Phase::Long } else { Phase::Break };
            }
            Phase::Break => {
                self.round += 1;
                self.phase = Phase::Focus;
            }
            Phase::Long => {
                self.round = 1;
                self.phase = Phase::Focus;
            }
        }
        self.remaining = self.phase_len(self.phase);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn cfg(rounds: u32) -> PomodoroCfg {
        PomodoroCfg { focus_min: 25, break_min: 5, long_break_min: 15, rounds }
    }

    #[test]
    fn starts_in_focus_round_one() {
        let p = Pomodoro::new(&cfg(4));
        assert_eq!(p.phase(), Phase::Focus);
        assert_eq!(p.time_text(), "25:00");
        assert_eq!(p.label(), "Fokus · ronde 1/4");
        assert_eq!(p.progress(), 0.0);
        assert!(!p.running());
    }

    #[test]
    fn full_cycle_with_two_rounds() {
        let mut p = Pomodoro::new(&cfg(2));
        p.skip();
        assert_eq!(p.phase(), Phase::Break);
        assert_eq!(p.label(), "Istirahat");
        assert_eq!(p.time_text(), "05:00");
        p.skip();
        assert_eq!((p.phase(), p.round()), (Phase::Focus, 2));
        assert_eq!(p.label(), "Fokus · ronde 2/2");
        p.skip(); // ronde terakhir -> istirahat panjang
        assert_eq!(p.phase(), Phase::Long);
        assert_eq!(p.label(), "Istirahat panjang");
        assert_eq!(p.time_text(), "15:00");
        p.skip(); // selesai satu siklus
        assert_eq!((p.phase(), p.round()), (Phase::Focus, 1));
    }

    #[test]
    fn pause_keeps_remaining_and_resume_continues() {
        let mut p = Pomodoro::new(&cfg(4));
        let t0 = Instant::now();
        p.toggle(t0);
        assert!(p.running());
        assert_eq!(p.tick(t0 + Duration::from_secs(60)), None);
        assert_eq!(p.time_text(), "24:00");
        p.toggle(t0 + Duration::from_secs(60)); // jeda
        assert!(!p.running());
        // tick saat jeda tidak mengubah apa pun
        assert_eq!(p.tick(t0 + Duration::from_secs(600)), None);
        assert_eq!(p.time_text(), "24:00");
        p.toggle(t0 + Duration::from_secs(600)); // lanjut
        assert_eq!(p.tick(t0 + Duration::from_secs(630)), None);
        assert_eq!(p.time_text(), "23:30");
    }

    #[test]
    fn finishing_advances_phase_and_stops() {
        let mut p = Pomodoro::new(&cfg(4));
        let t0 = Instant::now();
        p.toggle(t0);
        assert_eq!(p.tick(t0 + Duration::from_secs(25 * 60)), Some(Phase::Focus));
        assert_eq!(p.phase(), Phase::Break);
        assert!(!p.running(), "fase baru tidak otomatis berjalan");
        assert_eq!(p.time_text(), "05:00");
    }

    #[test]
    fn reset_restarts_current_phase() {
        let mut p = Pomodoro::new(&cfg(4));
        let t0 = Instant::now();
        p.toggle(t0);
        p.tick(t0 + Duration::from_secs(100));
        p.reset();
        assert!(!p.running());
        assert_eq!(p.time_text(), "25:00");
        assert_eq!(p.progress(), 0.0);
    }

    #[test]
    fn progress_and_zero_config_are_safe() {
        let mut p = Pomodoro::new(&PomodoroCfg { focus_min: 0, break_min: 0, long_break_min: 0, rounds: 0 });
        assert_eq!(p.time_text(), "01:00");
        assert_eq!(p.label(), "Fokus · ronde 1/1");
        let t0 = Instant::now();
        p.toggle(t0);
        p.tick(t0 + Duration::from_secs(30));
        assert!((p.progress() - 0.5).abs() < 0.02);
    }
}
