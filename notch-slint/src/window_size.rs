//! Ukuran jendela notch (menciut/melebar) dan aturan drop file ke Shelf.

use crate::*;

// Ruang saat menciut mencakup bubble 280 x 90 px dan pil setengah lingkaran dock bawah.
pub(crate) const COLLAPSED: (u32, u32) = (300, 140);
// Side dock: window 300 x 262 px memuat pil vertikal 60 x 120 dan bubble 230 px.
pub(crate) const COLLAPSED_SIDE: (u32, u32) = (300, 262);
// Greeting inline memerlukan ruang horizontal tambahan di sekitar badan Notch.
pub(crate) const GREETING_INLINE: (u32, u32) = (420, 112);
// 580 x 202 untuk pil + jalur di bawahnya (celah 8 px + tombol Pusatkan 22 px).
pub(crate) const EXPANDED: (u32, u32) = (612, 240);
/// Dashboard Currency membutuhkan ruang vertikal untuk konverter dan ringkasan pasar.
pub(crate) const EXPANDED_CURRENCY: (u32, u32) = (812, 558);
/// Hub Reminder menyediakan ruang untuk header, banner aktif, form dan daftar tanpa terpotong.
pub(crate) const EXPANDED_REMINDER: (u32, u32) = (812, 540);
/// Hub catatan yang diperluas (300 px) + jalur Pusatkan di bawahnya.
pub(crate) const EXPANDED_TALL: (u32, u32) = (612, 338);

/// Hub Ramadan (780 x 540) + jalur Pusatkan dan telinga.
pub(crate) const EXPANDED_RAMADAN: (u32, u32) = (812, 578);

pub(crate) fn expanded_size(w: &NotchWindow) -> (u32, u32) {
    if w.get_page_id() == "prayer" && w.get_ramadan_enabled() {
        EXPANDED_RAMADAN
    } else if w.get_page_id() == "currency" {
        EXPANDED_CURRENCY
    } else if w.get_page_id() == "reminder" {
        EXPANDED_REMINDER
    } else if w.get_page_id() == "notes" && w.get_notes_expanded() {
        EXPANDED_TALL
    } else {
        EXPANDED
    }
}

/// File yang dilepas ke notch masuk ke Shelf kecuali halaman Convert sedang terbuka (halaman itu punya antrean sendiri)
/// atau halaman Shelf dimatikan di pengaturan. Dibaca langsung dari UI agar mengikuti perubahan pengaturan.
pub(crate) fn shelf_accepts_drop(w: &NotchWindow) -> bool {
    !(w.get_expanded() && w.get_page_id() == "convert") && w.get_active_pages().iter().any(|p| p == "shelf")
}
