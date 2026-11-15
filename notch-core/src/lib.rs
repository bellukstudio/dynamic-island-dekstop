//! Logika inti Notch tanpa ketergantungan pada Tauri atau Slint.
//! Setiap modul hanya mengurus data dan sampling; pengiriman ke UI dilakukan pemanggil.

pub mod battery;
pub mod calendar;
pub mod config;
pub mod decor;
pub mod eyes;
mod eyes_data;
mod holidays_data;
#[cfg(feature = "net")]
pub mod http;
pub mod layout;
pub mod notes;
pub mod paths;
pub mod pomodoro;
pub mod quick;
pub mod stats;
pub mod theme;
