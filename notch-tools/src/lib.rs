//! UI-independent tools shared by the Slint and Tauri frontends.
//!
//! Keep filesystem dialogs, UI state, progress reporting, and platform integration
//! in the frontend. Processing functions in this crate operate on explicit paths.

pub mod compress;
pub mod convert;
