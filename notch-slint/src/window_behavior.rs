//! Perilaku jendela notch yang berjalan lewat timer: sembunyi saat aplikasi fullscreen dan drag native.

use crate::*;

/// Sembunyikan notch saat aplikasi foreground benar-benar menutupi seluruh monitor.
/// Timer tetap aktif ketika window disembunyikan agar notch muncul kembali setelah fullscreen berakhir.
/// Timer yang dikembalikan harus disimpan pemanggil selama aplikasi hidup (drop = berhenti).
pub(crate) fn start_fullscreen_watch(
    win: &NotchWindow,
    shell: &Rc<Shell>,
    enabled: &Rc<Cell<bool>>,
    hidden: &Rc<Cell<bool>>,
) -> Timer {
    let timer = Timer::default();
    let weak = win.as_weak();
    let shell = shell.clone();
    let enabled = enabled.clone();
    let hidden = hidden.clone();
    timer.start(TimerMode::Repeated, Duration::from_millis(500), move || {
        let Some(w) = weak.upgrade() else { return };
        let should_hide = enabled.get() && platform::foreground_is_fullscreen();
        if should_hide && !hidden.get() {
            if w.hide().is_ok() {
                hidden.set(true);
            }
        } else if !should_hide && hidden.get() {
            if w.show().is_ok() {
                platform::hide_from_taskbar(w.window());
                // Jendela transparan yang baru di-show bisa tampil putih (tanpa alpha) sampai ada gambar ulang:
                // pasang ulang ukuran, posisi, dan region lalu paksa redraw.
                let size = if w.get_expanded() { expanded_size(&w) } else { COLLAPSED };
                shell.place(&w, size);
                w.window().request_redraw();
                hidden.set(false);
            }
        }
    });
    timer
}

/// Drag native: polling pointer global agar seluruh permukaan Notch dapat dipindahkan
/// tanpa WebView/Tauri. Ambang 3px membedakan klik biasa dari drag.
pub(crate) fn start_native_drag(win: &NotchWindow, shell: &Rc<Shell>) {
    let weak = win.as_weak();
    let shell = shell.clone();
    let config_path = notch_core::paths::config_path();
    let mut was_down = false;
    let mut drag: Option<((i32, i32), (i32, i32), bool, (i32, i32))> = None;
    // Tekan di dalam jendela yang belum "armed": event press Slint bisa tiba beberapa milidetik
    // setelah GetAsyncKeyState melaporkan tombol turun, jadi pemeriksaan diulang selama tombol ditahan.
    let mut candidate: Option<((i32, i32), (i32, i32))> = None;
    // Skala dan ukuran ORB fisik saat ini; diperbarui bila pointer berpindah monitor DPI berbeda.
    let mut drag_scale = 1.0f32;
    let mut drag_orb_size = 0u32;
    let click_suppress_timer = Timer::default();
    let drag_timer = Timer::default();
    drag_timer.start(TimerMode::Repeated, Duration::from_millis(16), move || {
        let Some(w) = weak.upgrade() else { return };
        let down = platform::left_mouse_down();
        let cursor = platform::cursor_pos();
        if down && !was_down {
            if let Some((cx, cy)) = cursor {
                let p = w.window().position();
                let z = w.window().size();
                if cx >= p.x && cx < p.x + z.width as i32 && cy >= p.y && cy < p.y + z.height as i32 {
                    if w.get_drag_armed() {
                        drag = Some(((cx, cy), (p.x, p.y), false, (0, 0)));
                        w.set_play_interaction_active(true);
                    } else {
                        candidate = Some(((cx, cy), (p.x, p.y)));
                    }
                }
            }
        }
        if down && drag.is_none() {
            if let Some((start_cursor, start_window)) = candidate {
                if w.get_drag_armed() {
                    drag = Some((start_cursor, start_window, false, (0, 0)));
                    candidate = None;
                    w.set_play_interaction_active(true);
                }
            }
        }
        if down {
            if let (Some((cx, cy)), Some((start_cursor, start_window, moved, mut orb_offset))) = (cursor, drag) {
                let dx = cx - start_cursor.0;
                let dy = cy - start_cursor.1;
                // Ambang drag 3 logical px, dikonversi ke koordinat pointer fisik.
                let drag_threshold = (3.0 * w.window().scale_factor()).round() as i32;
                let is_moved = moved || dx.abs() + dy.abs() >= drag_threshold;
                if is_moved {
                    w.set_play_interaction_active(true);
                    if !moved {
                        // Klik baru boleh dipulihkan setelah tombol mouse dilepas.
                        // Timer di sini terlalu dini: drag panjang (>350 ms) bisa memicu klik saat release.
                        w.set_suppress_next_pill_click(true);
                        let sf = w.window().scale_factor();
                        let orb_size = (64.0 * sf).round() as u32;
                        // Pusat ORB langsung mengikuti titik pointer yang memulai drag.
                        // Offset tengah window lama membuat snap sisi gagal bila drag dimulai dari ujung hub.
                        orb_offset = (
                            cx - start_window.0 - orb_size as i32 / 2,
                            cy - start_window.1 - orb_size as i32 / 2,
                        );
                        w.window().set_size(PhysicalSize::new(orb_size, orb_size));
                        drag_scale = sf;
                        drag_orb_size = orb_size;
                        // Saat drag dimulai dari hub yang terbuka, tutup konten sebelum window menjadi ORB.
                        w.set_expanded(false);
                        w.set_dragging(true);
                    }
                    // Saat melintasi monitor dengan DPI berbeda, jaga ORB tetap 64 logical px.
                    let current_scale = w.window().scale_factor();
                    if (current_scale - drag_scale).abs() > f32::EPSILON {
                        let next_orb_size = (64.0 * current_scale).round() as u32;
                        let center_adjust = (drag_orb_size as i32 - next_orb_size as i32) / 2;
                        orb_offset.0 += center_adjust;
                        orb_offset.1 += center_adjust;
                        w.window().set_size(PhysicalSize::new(next_orb_size, next_orb_size));
                        drag_scale = current_scale;
                        drag_orb_size = next_orb_size;
                    }
                    w.window().set_position(PhysicalPosition::new(start_window.0 + dx + orb_offset.0, start_window.1 + dy + orb_offset.1));
                    drag = Some((start_cursor, start_window, true, orb_offset));
                }
            }
        } else if was_down {
            candidate = None;
            if let Some((_, _, moved, _)) = drag.take() {
                if moved {
                    // Jaga proteksi klik selama seluruh drag, lalu beri jeda singkat
                    // agar event release dari sistem window tidak lolos sebagai klik.
                    let clear_weak = weak.clone();
                    click_suppress_timer.start(TimerMode::SingleShot, Duration::from_millis(350), move || {
                        if let Some(w) = clear_weak.upgrade() { w.set_suppress_next_pill_click(false); }
                    });
                    // Jika pembacaan pointer gagal tepat saat release, gunakan pusat ORB
                    // yang terakhir diposisikan. Ini mencegah jendela tertinggal dalam ukuran 64px.
                    let (cx, cy) = cursor.unwrap_or_else(|| {
                        let p = w.window().position();
                        let s = w.window().size();
                        (p.x + s.width as i32 / 2, p.y + s.height as i32 / 2)
                    });
                    {
                        let work = platform::work_area(Some((cx, cy)));
                        let size = w.window().size();
                        let sf = w.window().scale_factor();
                        let previous_dock = w.get_dock();
                        let was_left = previous_dock == 3;
                        let was_right = previous_dock == 4;
                        let was_bottom = previous_dock == 2;
                        let was_top = previous_dock == 0;
                        // Samakan prioritas dan hysteresis Tauri: bawah lebih dulu,
                        // lalu atas, baru sisi. Semua ambang dikonversi ke piksel fisik.
                        let near_bottom = work.y + work.h - cy
                            <= ((if was_bottom { 90.0 } else { 48.0 }) * sf).round() as i32;
                        let near_top = cy - work.y
                            <= ((if was_top { 60.0 } else { 24.0 }) * sf).round() as i32;
                        let near_left = cx - work.x
                            <= ((if was_left { 110.0 } else { 70.0 }) * sf).round() as i32;
                        let near_right = work.x + work.w - cx
                            <= ((if was_right { 110.0 } else { 70.0 }) * sf).round() as i32;
                        let (pos, mode) = if near_bottom {
                            ((cx, layout::BOTTOM_TY), "bottom")
                        } else if near_top {
                            ((cx, work.y), "top")
                        } else if near_left {
                            ((layout::LEFT_TX, cy), "left")
                        } else if near_right {
                            ((layout::RIGHT_TX, cy), "right")
                        } else {
                            ((cx, cy - (size.height as i32 / 2)), "floating")
                        };
                        shell.pos.set(Some(pos));
                        let monitor_center = (work.x + work.w / 2, work.y + work.h / 2);
                        // Simpan anchor monitor untuk semua posisi, termasuk bottom sentinel.
                        shell.monitor_hint.set(Some(monitor_center));
                        shell.memo.set((None, 0));
                        shell.place(&w, COLLAPSED);
                        let mut next = Config::load_readonly(&config_path);
                        next.position.custom = true;
                        next.position.x = pos.0;
                        next.position.y = pos.1;
                        next.position.monitor_center_x = monitor_center.0;
                        next.position.monitor_center_y = monitor_center.1;
                        if next.save(&config_path).is_ok() {

                        }
                        w.set_position_mode(mode.into());
                        w.set_dragging(false);
                        w.set_play_interaction_active(false);
                    }
                } else {
                    w.set_play_interaction_active(false);
                }
            }
        }
        was_down = down;
    });
}
