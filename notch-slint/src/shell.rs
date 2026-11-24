//! Cangkang jendela notch: penempatan, bentuk tepi, hiasan tema, dan transisi melebar/menciut.

use crate::*;

// Ukuran pil saat melebar (px logis); harus sama dengan `pill` di ui/notch.slint.
pub(crate) const PILL_W: f32 = 580.0;
pub(crate) const PILL_H: f32 = 202.0;

/// Penempatan jendela + bentuk tepi + gambar hiasan tema. Hiasan dirender ulang hanya saat mode tepi
/// (atas/melayang/bawah) atau faktor DPI berubah.
pub(crate) struct Shell {
    pub(crate) pos: Cell<Option<(i32, i32)>>, // posisi kustom (pusat x, atas y) atau None = tengah atas
    // Anchor monitor untuk memulihkan dock kiri/kanan; sentinel posisi tidak menyimpan koordinat monitor.
    pub(crate) monitor_hint: Cell<Option<(i32, i32)>>,
    pub(crate) theme: RefCell<String>,
    pub(crate) body: RefCell<Rgba>,
    pub(crate) scale: RefCell<f32>,
    pub(crate) memo: Cell<(Option<Dock>, u32)>, // (dock, bit faktor DPI) yang sedang terpasang
}

impl Shell {
    /// Letakkan jendela sesuai posisi config dan atur bentuk tepinya (menempel atas / melayang / bawah).
    pub(crate) fn place(&self, win: &NotchWindow, size: (u32, u32)) {
        let (origin, dock, size) = self.geometry(win, size);
        if self.memo.get() != (Some(dock), win.window().scale_factor().to_bits()) {
            self.memo.set((Some(dock), win.window().scale_factor().to_bits()));
            win.set_dock(match dock {
                Dock::Top => 0,
                Dock::Floating => 1,
                Dock::Bottom => 2,
                Dock::Left => 3,
                Dock::Right => 4,
            });
            self.rebuild_decor(win, dock, win.window().scale_factor());
        }
        let w = win.window();
        if platform::set_bounds(w, origin.0, origin.1, size.0, size.1) {
            // Selaraskan layout Slint dengan ukuran native saat itu juga, bukan menunggu event Resized.
            w.set_size(PhysicalSize::new(size.0, size.1));
        } else {
            w.set_size(PhysicalSize::new(size.0, size.1));
            w.set_position(PhysicalPosition::new(origin.0, origin.1));
        }
        // Region selalu dipasang (persegi penuh seukuran jendela, sejak penempatan pertama) dan TIDAK PERNAH NULL: DWM
        // menggambar bingkai non-client yang tembus terlihat lewat klien transparan (bingkai biru muda di tepi jendela)
        // pada jendela tanpa region. Pemotongan notch menciut (clip_to_collapsed) hanya mempersempit region ini.
        platform::set_region(w, Some((0, 0, size.0 as i32, size.1 as i32)));
    }

    /// Posisi layar, dock, dan ukuran fisik jendela untuk `size` (piksel logis), tanpa menerapkannya.
    pub(crate) fn geometry(&self, win: &NotchWindow, size: (u32, u32)) -> ((i32, i32), Dock, (u32, u32)) {
        let pos = self.pos.get();
        // Sentinel sisi bukan koordinat monitor. Saat drag ke sisi, pertahankan monitor
        // tempat jendela sedang berada (posisi fisik sebelum ukuran/docking diterapkan).
        // Jika belum ada posisi jendela yang valid, fallback tetap ke monitor utama.
        let side_docked = pos.is_some_and(|(x, _)| x == layout::LEFT_TX || x == layout::RIGHT_TX);
        let monitor_hint = if let Some(anchor) = self.monitor_hint.get() {
            Some(anchor)
        } else if side_docked {
            // Config lama belum menyimpan anchor: gunakan monitor tempat window berada.
            let current = win.window().position();
            let current_size = win.window().size();
            (current_size.width > 0 && current_size.height > 0).then_some((
                current.x + current_size.width as i32 / 2,
                current.y + current_size.height as i32 / 2,
            ))
        } else {
            // Sentinel BOTTOM_TY bukan koordinat desktop. Tanpa anchor lama, fallback ke monitor utama.
            pos.filter(|(_, y)| *y < layout::BOTTOM_TY / 2)
        };
        let work = platform::work_area(monitor_hint);
        // COLLAPSED/EXPANDED dalam piksel logis; jendela diukur dalam piksel fisik
        let sf = win.window().scale_factor();
        let side = pos.is_some_and(|(x, _)| x == layout::LEFT_TX || x == layout::RIGHT_TX);
        let logical_size = if side && size == COLLAPSED { COLLAPSED_SIDE } else { size };
        // Skala notch berlaku pada semua geometri menciut, termasuk mode mini-player,
        // greeting inline, dan dock samping. Ukuran native harus sama dengan ukuran isi Slint.
        let logical_size = if size == COLLAPSED || size == GREETING_INLINE {
            scaled_window_size(win, logical_size)
        } else {
            logical_size
        };
        let size = ((logical_size.0 as f32 * sf).round() as u32, (logical_size.1 as f32 * sf).round() as u32);
        let (origin, dock) = layout::place(&work, size.0 as i32, size.1 as i32, pos);
        (origin, dock, size)
    }

    /// Tampilkan notch menciut TANPA mengubah ukuran jendela: jendela tetap seukuran hub dan hanya persegi notch
    /// yang terlihat (lihat `clip`). Mengembalikan false bila notch tidak bisa disejajarkan dengan pasti (dock
    /// samping, atau jendela besar terjepit tepi layar); pemanggil lalu mengecilkan jendela seperti biasa.
    pub(crate) fn clip_to_collapsed(&self, win: &NotchWindow, size: (u32, u32)) -> bool {
        // Saklar uji: jalankan dengan variabel lingkungan NOTCH_NO_CLIP=1 untuk memakai cara lama (jendela diperkecil).
        static DISABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        if *DISABLED.get_or_init(|| std::env::var_os("NOTCH_NO_CLIP").is_some()) {
            return false;
        }
        let (origin, dock, csize) = self.geometry(win, size);
        // Dock samping dan bawah menempel di tepi lain (dan tatapan mata memakai tepi atas jendela): tetap diperkecil.
        if !matches!(dock, Dock::Top | Dock::Floating) {
            return false;
        }
        let w = win.window();
        let (bpos, bsize) = (w.position(), w.size());
        let Some(rect) = clip::collapsed_rect(
            origin,
            (csize.0 as i32, csize.1 as i32),
            (bpos.x, bpos.y),
            (bsize.width as i32, bsize.height as i32),
        ) else {
            return false;
        };
        platform::set_region(w, Some(rect))
    }

    pub(crate) fn rebuild_decor(&self, win: &NotchWindow, dock: Dock, sf: f32) {
        if matches!(dock, Dock::Left | Dock::Right) {
            win.set_has_decor(false);
            return;
        }
        let (w, h) = ((PILL_W * sf).round() as u32, (PILL_H * sf).round() as u32);
        let r = (if dock == Dock::Floating { 28.0 } else { 32.0 }) * *self.scale.borrow() * sf;
        match decor::render(&self.theme.borrow(), *self.body.borrow(), dock, w, h, r, sf) {
            Some(buf) => {
                win.set_decor(Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&buf, w, h)));
                win.set_has_decor(true);
            }
            None => win.set_has_decor(false), // tema default: tanpa hiasan
        }
    }
}

/// Pastikan ukuran jendela native mengikuti hub yang terbuka. Pada hover pertama setelah startup, `set_size`
/// bisa diabaikan atau ditimpa (jendela native belum siap, atau hide/show untuk menyembunyikan taskbar), sehingga
/// hub tergambar terpotong di jendela 300 x 140. Cek ulang beberapa kali dan pasang ulang ukuran bila tidak cocok.
pub(crate) fn settle_expanded_geometry(weak: slint::Weak<NotchWindow>, shell: Rc<Shell>, tries_left: u8) {
    if tries_left == 0 {
        return;
    }
    Timer::single_shot(Duration::from_millis(80), move || {
        let Some(w) = weak.upgrade() else { return };
        if !w.get_expanded() {
            return;
        }
        let (lw, lh) = expanded_size(&w);
        let sf = w.window().scale_factor();
        let want = ((lw as f32 * sf).round() as i32, (lh as f32 * sf).round() as i32);
        let got = w.window().size();
        if (got.width as i32 - want.0).abs() > 2 || (got.height as i32 - want.1).abs() > 2 {
            eprintln!("[Geometry] ukuran hub {}x{} != {}x{}, dipasang ulang", got.width, got.height, want.0, want.1);
            shell.memo.set((None, 0));
            shell.place(&w, (lw, lh));
            settle_expanded_geometry(weak.clone(), shell.clone(), tries_left - 1);
        }
    });
}

/// Perkecil jendela ke ukuran menciut setelah animasi menciut selesai (notch tetap terlihat, seperti Tauri).
/// Saat jendela native berubah ukuran/posisi, Windows bisa menampilkan frame terakhir pada koordinat baru sampai
/// Slint menggambar ulang. Perubahan dijatuhkan tepat setelah vsync DWM supaya gambar ulang (jendela kecil, cepat)
/// selesai sebelum komposisi berikutnya dan frame basi hampir tidak sempat tampil.
pub(crate) fn shrink_window_synced(weak: slint::Weak<NotchWindow>, shell: Rc<Shell>) {
    let Some(w) = weak.upgrade() else { return };
    if w.get_expanded() {
        return;
    }
    // Greeting inline (385 px) masih tampil: jangan potong ke 300 px.
    let target = |w: &NotchWindow| if w.get_greeting_inline_visible() { GREETING_INLINE } else { COLLAPSED };
    let size = target(&w);
    if !window_needs_resize(&w, size) {
        // Ukuran sama (hanya posisi/dekorasi yang bisa berubah): tidak ada frame basi yang perlu disembunyikan.
        shell.place(&w, size);
        return;
    }
    // Cara utama: jendela dibiarkan besar dan hanya persegi notch yang tampil. Tanpa perubahan ukuran tidak ada
    // frame basi, jadi notch tidak berkedip. Jendela baru benar-benar diperkecil bila ini tidak mungkin.
    if shell.clip_to_collapsed(&w, size) {
        return;
    }
    // Jendela akan mengecil sementara notch (mata) masih tampak: frame lama jendela besar tampil di koordinat baru
    // sebagai notch/mata ganda ("hantu"), sama seperti sisi expand(). Sembunyikan notch dulu, beri beberapa frame
    // agar frame kosong sempat sampai ke layar, baru ubah ukuran jendela tepat setelah vsync.
    w.set_geometry_hold(true);
    Timer::single_shot(Duration::from_millis(40), move || {
        let Some(w) = weak.upgrade() else { return };
        // Hub dibuka lagi selama jeda: expand() yang mengatur ukuran, cukup lepas penahan.
        if w.get_expanded() {
            w.set_geometry_hold(false);
            return;
        }
        platform::wait_vsync();
        // Jendela sudah berganti ukuran di bawah frame kosong, jadi notch boleh tampil seketika (tanpa fade-in).
        w.set_hold_instant(true);
        w.set_geometry_hold(false);
        shell.place(&w, target(&w));
        // Kembalikan fade-in normal (dipakai saat hub membuka) setelah animasi pill sempat dievaluasi.
        let weak = w.as_weak();
        Timer::single_shot(Duration::from_millis(200), move || {
            if let Some(w) = weak.upgrade() {
                w.set_hold_instant(false);
            }
        });
    });
}

/// Tampilkan kartu greeting inline (dock atas) dengan aman. Jendela harus membesar 300 -> 385 px; bila itu dilakukan
/// saat notch masih tampak, frame lama muncul di koordinat baru sebagai notch ganda ("hantu"), sama seperti pada
/// `expand()`. Karena itu notch disembunyikan dulu, beberapa frame diberi waktu sampai ke layar, jendela dibesarkan
/// tepat setelah vsync, lalu kartu ditampilkan di jendela yang sudah berukuran benar.
pub(crate) fn show_greeting_inline(weak: slint::Weak<NotchWindow>, shell: Rc<Shell>) {
    let Some(w) = weak.upgrade() else { return };
    if w.get_expanded() {
        return;
    }
    if !window_needs_resize(&w, GREETING_INLINE) {
        shell.place(&w, GREETING_INLINE);
        w.set_greeting_inline_visible(true);
        return;
    }
    w.set_geometry_hold(true);
    Timer::single_shot(Duration::from_millis(40), move || {
        let Some(w) = weak.upgrade() else { return };
        // Hub dibuka selama jeda: expand() yang mengatur ukuran, cukup lepas penahan.
        if w.get_expanded() {
            w.set_geometry_hold(false);
            return;
        }
        platform::wait_vsync();
        w.set_hold_instant(true);
        shell.place(&w, GREETING_INLINE);
        w.set_greeting_inline_visible(true);
        w.set_geometry_hold(false);
        let weak = w.as_weak();
        Timer::single_shot(Duration::from_millis(200), move || {
            if let Some(w) = weak.upgrade() {
                w.set_hold_instant(false);
            }
        });
    });
}

/// True bila ukuran jendela native berbeda dari `size` (piksel logis) setelah dikali skala layar.
pub(crate) fn window_needs_resize(w: &NotchWindow, size: (u32, u32)) -> bool {
    let sf = w.window().scale_factor();
    let got = w.window().size();
    let side = w.get_dock() == 3 || w.get_dock() == 4;
    let logical_size = if side && size == COLLAPSED { COLLAPSED_SIDE } else { size };
    let logical_size = if size == COLLAPSED || size == GREETING_INLINE {
        scaled_window_size(w, logical_size)
    } else {
        logical_size
    };
    let want = ((logical_size.0 as f32 * sf).round() as i32, (logical_size.1 as f32 * sf).round() as i32);
    (got.width as i32 - want.0).abs() > 2 || (got.height as i32 - want.1).abs() > 2
}

/// Bagian akhir `expand()`: besarkan jendela, buka hub, dan tampilkan notch lagi (memudar masuk).
pub(crate) fn finish_expand(weak: &slint::Weak<NotchWindow>, shell: &Rc<Shell>, sync: &dyn Fn()) {
    let Some(w) = weak.upgrade() else { return };
    w.set_geometry_hold(false);
    let size = expanded_size(&w);
    shell.place(&w, size);
    w.set_expanded(true);
    sync();
    settle_expanded_geometry(weak.clone(), shell.clone(), 8);
}
