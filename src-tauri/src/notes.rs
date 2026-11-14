//! Pembungkus Tauri untuk catatan. Logika dan penyimpanan ada di crate `notch-core`
//! (dipakai juga oleh prototipe Slint, membaca file notes.json yang sama).

use notch_core::notes::{Note, NotesStore};
use std::sync::{Mutex, OnceLock};

static STORE: OnceLock<Mutex<NotesStore>> = OnceLock::new();

fn store() -> &'static Mutex<NotesStore> {
    // dimuat dari disk saat pertama dipakai, sama seperti sebelumnya
    STORE.get_or_init(|| Mutex::new(NotesStore::open(notch_core::paths::data_file("notes.json"))))
}

#[tauri::command]
pub fn notes_list() -> Vec<Note> {
    store().lock().unwrap().list()
}

#[tauri::command]
pub fn notes_add(text: String) -> Vec<Note> {
    store().lock().unwrap().add(&text)
}

#[tauri::command]
pub fn notes_update(id: String, text: String) -> Vec<Note> {
    store().lock().unwrap().update(&id, &text)
}

#[tauri::command]
pub fn notes_remove(id: String) -> Vec<Note> {
    store().lock().unwrap().remove(&id)
}

#[tauri::command]
pub fn notes_clear() -> Vec<Note> {
    store().lock().unwrap().clear()
}
