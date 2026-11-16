//! Catatan cepat, disimpan sebagai JSON. Path file diberikan pemanggil (lihat `paths::data_file`).
//! Disimpan sebagai file sendiri di samping config.toml supaya catatan tetap ada walau config di-reset.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_CHARS: usize = 4000;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Note {
    pub id: String,
    pub text: String,
    pub updated_at: i64, // unix ms
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn uid() -> String {
    let n = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    format!("{:x}{:x}", n.as_millis(), n.subsec_nanos())
}

fn clean(text: &str) -> String {
    text.trim().chars().take(MAX_CHARS).collect()
}

fn sorted(mut items: Vec<Note>) -> Vec<Note> {
    items.sort_by(|a, b| b.updated_at.cmp(&a.updated_at)); // terbaru di atas
    items
}

pub struct NotesStore {
    path: PathBuf,
    items: Vec<Note>,
}

impl NotesStore {
    /// Memuat dari `path`. File hilang atau rusak = daftar kosong.
    pub fn open(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let items = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Self { path, items }
    }

    fn save(&self) {
        if let Some(dir) = self.path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = serde_json::to_string_pretty(&self.items) {
            let _ = std::fs::write(&self.path, text);
        }
    }

    /// Hanya baca: tidak menulis ulang file.
    pub fn list(&self) -> Vec<Note> {
        sorted(self.items.clone())
    }

    pub fn add(&mut self, text: &str) -> Vec<Note> {
        let text = clean(text);
        if !text.is_empty() {
            self.items.push(Note { id: uid(), text, updated_at: now_ms() });
        }
        self.save();
        self.list()
    }

    pub fn update(&mut self, id: &str, text: &str) -> Vec<Note> {
        if let Some(n) = self.items.iter_mut().find(|n| n.id == id) {
            n.text = clean(text);
            n.updated_at = now_ms();
        }
        self.save();
        self.list()
    }

    pub fn remove(&mut self, id: &str) -> Vec<Note> {
        self.items.retain(|n| n.id != id);
        self.save();
        self.list()
    }

    pub fn clear(&mut self) -> Vec<Note> {
        self.items.clear();
        self.save();
        self.list()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("notch-core-test-{}-{}", std::process::id(), name));
        let _ = std::fs::remove_dir_all(&d);
        d.join("notes.json")
    }

    #[test]
    fn add_trims_and_ignores_empty() {
        let mut s = NotesStore::open(tmp("add"));
        assert!(s.add("   ").is_empty());
        let l = s.add("  halo  ");
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].text, "halo");
    }

    #[test]
    fn text_is_limited_to_4000_chars() {
        let mut s = NotesStore::open(tmp("limit"));
        let l = s.add(&"a".repeat(5000));
        assert_eq!(l[0].text.chars().count(), 4000);
        let id = l[0].id.clone();
        let l = s.update(&id, &"b".repeat(4500));
        assert_eq!(l[0].text.chars().count(), 4000);
    }

    #[test]
    fn update_remove_clear() {
        let mut s = NotesStore::open(tmp("urc"));
        let id = s.add("satu")[0].id.clone();
        assert_eq!(s.update(&id, "dua")[0].text, "dua");
        assert_eq!(s.update("tidak-ada", "x").len(), 1);
        assert!(s.remove(&id).is_empty());
        s.add("a");
        s.add("b");
        assert!(s.clear().is_empty());
    }

    #[test]
    fn persists_and_sorts_newest_first() {
        let path = tmp("persist");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            r#"[{"id":"a","text":"lama","updated_at":100},
                {"id":"c","text":"baru","updated_at":300},
                {"id":"b","text":"tengah","updated_at":200}]"#,
        )
        .unwrap();
        let s = NotesStore::open(&path);
        let texts: Vec<_> = s.list().into_iter().map(|n| n.text).collect();
        assert_eq!(texts, ["baru", "tengah", "lama"]);

        let mut s = NotesStore::open(&path);
        s.add("paling baru");
        let s2 = NotesStore::open(&path);
        assert_eq!(s2.list()[0].text, "paling baru");
        assert_eq!(s2.list().len(), 4);
    }

    #[test]
    fn missing_or_corrupt_file_gives_empty_list() {
        assert!(NotesStore::open(tmp("missing")).list().is_empty());
        let path = tmp("corrupt");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "bukan json").unwrap();
        assert!(NotesStore::open(&path).list().is_empty());
    }

    #[test]
    fn list_does_not_write_file() {
        let path = tmp("readonly");
        let s = NotesStore::open(&path);
        let _ = s.list();
        assert!(!path.exists());
    }
}
