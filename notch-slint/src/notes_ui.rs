//! Halaman Catatan: daftar catatan dan jendela detail catatan.

use crate::*;

pub(crate) fn show_notes(model: &VecModel<NoteItem>, notes: Vec<Note>) {
    model.set_vec(
        notes
            .into_iter()
            .map(|n| {
                let first_lines = n.text.split('\n').take(2).collect::<Vec<_>>().join(" ");
                let mut preview: String = first_lines.chars().take(120).collect();
                if n.text.chars().count() > 120 {
                    preview.push('\u{2026}');
                }
                let meta = format!("{} \u{00B7} {} kar", fmt_note_date(n.updated_at), n.text.chars().count());
                NoteItem { id: n.id.into(), text: n.text.into(), preview: preview.into(), meta: meta.into() }
            })
            .collect::<Vec<_>>(),
    );
}

pub(crate) fn note_meta(ms: i64) -> String {
    let (y, m, d, h, mi) = reminder::local_parts(ms, local_timezone_offset_minutes());
    format!("{d} {} {y}, {h:02}.{mi:02}", SHORT_MONTHS[(m.clamp(1, 12) - 1) as usize])
}

pub(crate) fn note_counts(text: &str) -> (String, String) {
    (
        format!("{} karakter", group_digits(&text.chars().count().to_string())),
        format!("{} baris", if text.is_empty() { 0 } else { text.split('\n').count() }),
    )
}

/// Buka (atau fokuskan) jendela detail untuk satu catatan: edit, simpan (Ctrl+S), hapus dua langkah, Esc.
pub(crate) fn open_note_detail(
    id: &str,
    store: &Rc<RefCell<Option<NotesStore>>>,
    path: &std::path::Path,
    model: &Rc<VecModel<NoteItem>>,
    details: &Rc<RefCell<std::collections::HashMap<String, NoteDetailWindow>>>,
    accent: slint::Color,
) {
    if let Some(existing) = details.borrow().get(id) {
        let _ = existing.show();
        return;
    }
    let note = {
        let mut guard = store.borrow_mut();
        let s = guard.get_or_insert_with(|| NotesStore::open(path));
        s.list().into_iter().find(|n| n.id == id)
    };
    let Some(note) = note else { return };
    let Ok(win) = NoteDetailWindow::new() else { return };
    win.set_accent(accent);
    win.set_text(note.text.clone().into());
    win.set_meta(note_meta(note.updated_at).into());
    let (chars, lines) = note_counts(&note.text);
    win.set_chars(chars.into());
    win.set_lines(lines.into());
    let original = Rc::new(RefCell::new(note.text.clone()));
    let warned = Rc::new(Cell::new(false));
    let note_id = id.to_string();

    let toast = {
        let weak = win.as_weak();
        Rc::new(move |msg: &str| {
            if let Some(w) = weak.upgrade() {
                w.set_toast(msg.into());
                let weak = w.as_weak();
                Timer::single_shot(Duration::from_secs(2), move || {
                    if let Some(w) = weak.upgrade() {
                        w.set_toast("".into());
                    }
                });
            }
        })
    };
    let close: Rc<dyn Fn()> = {
        let (weak, details, note_id) = (win.as_weak(), details.clone(), note_id.clone());
        Rc::new(move || {
            if let Some(w) = weak.upgrade() {
                let _ = w.hide();
            }
            // Lepas jendela setelah callback selesai agar tidak membebaskan dirinya sendiri.
            let (details, note_id) = (details.clone(), note_id.clone());
            Timer::single_shot(Duration::from_millis(50), move || {
                details.borrow_mut().remove(&note_id);
            });
        })
    };
    {
        let (weak, original, warned) = (win.as_weak(), original.clone(), warned.clone());
        win.on_edited(move |text| {
            let Some(w) = weak.upgrade() else { return };
            let dirty = text.trim() != original.borrow().trim();
            w.set_dirty(dirty);
            w.set_can_save(dirty && !text.trim().is_empty());
            let (chars, lines) = note_counts(text.as_str());
            w.set_chars(chars.into());
            w.set_lines(lines.into());
            if !dirty {
                warned.set(false);
            }
        });
    }
    {
        let (weak, store, path, model, original, toast, note_id) =
            (win.as_weak(), store.clone(), path.to_path_buf(), model.clone(), original.clone(), toast.clone(), note_id.clone());
        win.on_save(move || {
            let Some(w) = weak.upgrade() else { return };
            let text = w.get_text().trim().to_string();
            if text.is_empty() || !w.get_can_save() {
                return;
            }
            let list = {
                let mut guard = store.borrow_mut();
                let s = guard.get_or_insert_with(|| NotesStore::open(&path));
                s.update(&note_id, &text)
            };
            if let Some(n) = list.iter().find(|n| n.id == note_id) {
                w.set_meta(note_meta(n.updated_at).into());
            }
            show_notes(&model, list);
            *original.borrow_mut() = text.clone();
            w.set_text(text.clone().into());
            let (chars, lines) = note_counts(&text);
            w.set_chars(chars.into());
            w.set_lines(lines.into());
            w.set_dirty(false);
            w.set_can_save(false);
            toast("\u{2713} Disimpan");
        });
    }
    {
        let (weak, store, path, model, close, note_id) =
            (win.as_weak(), store.clone(), path.to_path_buf(), model.clone(), close.clone(), note_id.clone());
        win.on_delete(move || {
            let Some(w) = weak.upgrade() else { return };
            if !w.get_confirm_delete() {
                w.set_confirm_delete(true);
                let weak = w.as_weak();
                Timer::single_shot(Duration::from_secs(3), move || {
                    if let Some(w) = weak.upgrade() {
                        w.set_confirm_delete(false);
                    }
                });
                return;
            }
            let list = {
                let mut guard = store.borrow_mut();
                let s = guard.get_or_insert_with(|| NotesStore::open(&path));
                s.remove(&note_id)
            };
            show_notes(&model, list);
            close();
        });
    }
    {
        let (weak, close, warned, toast) = (win.as_weak(), close.clone(), warned.clone(), toast.clone());
        win.on_esc(move || {
            let Some(w) = weak.upgrade() else { return };
            if w.get_dirty() && !warned.get() {
                warned.set(true);
                toast("Belum disimpan \u{2014} tekan Esc lagi untuk tutup");
            } else {
                close();
            }
        });
    }
    {
        let close = close.clone();
        win.window().on_close_requested(move || {
            close();
            slint::CloseRequestResponse::HideWindow
        });
    }
    let _ = win.show();
    details.borrow_mut().insert(id.to_string(), win);
}
