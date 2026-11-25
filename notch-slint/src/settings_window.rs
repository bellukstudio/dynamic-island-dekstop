//! Jendela Pengaturan terpisah: sinkronisasi state dari NotchWindow, pengkabelan callback, dan siklus hidup jendela.

use crate::*;

/// Diset oleh tombol "Mulai ulang Notch" di Pengaturan; `main` menjalankan proses baru setelah event loop berhenti.
pub(crate) static RESTART_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Minta Slint menggambar ulang setelah jendela pengaturan di-show atau dipulihkan.
/// Dua frame tertunda membantu mencegah jendela transparan/berlapis tertahan pada frame kosong lama.
fn request_settings_redraw(win: &SettingsWindow) {
    win.window().request_redraw();
    let weak = win.as_weak();
    Timer::single_shot(Duration::from_millis(90), move || {
        if let Some(win) = weak.upgrade() {
            win.window().request_redraw();
        }
    });
}

/// Salin nilai tampilan dari NotchWindow (pemilik state) ke jendela Pengaturan. Property yang nilainya sama tidak memicu render ulang.
pub(crate) fn sync_settings_window(m: &NotchWindow, s: &SettingsWindow) {
    s.set_weather_city(m.get_weather_city());
    s.set_weather_lat(m.get_weather_lat());
    s.set_weather_lon(m.get_weather_lon());
    s.set_weather_save_status(m.get_weather_save_status());
    s.set_weather_hits(m.get_weather_hits());
    s.set_weather_search_status(m.get_weather_search_status());
    s.set_adhan_active(m.get_adhan_active());
    s.set_adhan_status(m.get_prayer_status());
    s.set_shortcut_hub(m.get_shortcut_hub());
    s.set_shortcut_settings(m.get_shortcut_settings());
    s.set_shortcut_pomodoro(m.get_shortcut_pomodoro());
    s.set_shortcut_prayer(m.get_shortcut_prayer());
    s.set_appearance_theme(m.get_appearance_theme());
    s.set_appearance_eye_theme(m.get_appearance_eye_theme());
    s.set_appearance_notch_theme(m.get_appearance_notch_theme());
    s.set_appearance_mood(m.get_appearance_mood());
    s.set_appearance_accent(m.get_appearance_accent());
    s.set_appearance_custom_bg(m.get_appearance_custom_bg());
    s.set_appearance_custom_text(m.get_appearance_custom_text());
    s.set_appearance_dark_bg(m.get_appearance_dark_bg());
    s.set_appearance_light_bg(m.get_appearance_light_bg());
    s.set_appearance_save_status(m.get_appearance_save_status());
    s.set_behavior_scale(m.get_behavior_scale());
    s.set_behavior_scale_value(m.get_behavior_scale().parse::<f32>().unwrap_or(1.0));
    s.set_behavior_hover_open(m.get_behavior_hover_open());
    s.set_behavior_hover_close(m.get_behavior_hover_close());
    s.set_behavior_sleep_after(m.get_behavior_sleep_after());
    s.set_behavior_hide_fullscreen(m.get_behavior_hide_fullscreen());
    s.set_autostart_enabled(m.get_autostart_enabled());
    s.set_autostart_save_status(m.get_autostart_save_status());
    s.set_behavior_save_status(m.get_behavior_save_status());
    s.set_motion_enabled(m.get_motion_enabled());
    s.set_dock_play_enabled(m.get_dock_play_enabled());
    s.set_motion_interval(m.get_motion_interval());
    s.set_motion_max_moves(m.get_motion_max_moves());
    s.set_motion_save_status(m.get_motion_save_status());
    s.set_nudge_enabled(m.get_nudge_enabled());
    s.set_nudge_idle_min(m.get_nudge_idle_min());
    s.set_nudge_message(m.get_nudge_message());
    s.set_nudge_emoji(m.get_nudge_emoji());
    s.set_nudge_mood(m.get_nudge_mood());
    s.set_nudge_show_ms(m.get_nudge_show_ms());
    s.set_nudge_only_silent(m.get_nudge_only_silent());
    s.set_nudge_until_move(m.get_nudge_until_move());
    s.set_nudge_save_status(m.get_nudge_save_status());
    s.set_greeting_enabled(m.get_greeting_enabled());
    s.set_greeting_mode(m.get_greeting_mode());
    s.set_greeting_voice(m.get_greeting_voice());
    s.set_greeting_custom_text(m.get_greeting_custom_text());
    s.set_greeting_save_status(m.get_greeting_save_status());
    s.set_conversation_enabled(m.get_conversation_enabled());
    s.set_conversation_on_play(m.get_conversation_on_play());
    s.set_conversation_on_track_change(m.get_conversation_on_track_change());
    s.set_conversation_on_pause(m.get_conversation_on_pause());
    s.set_conversation_on_return(m.get_conversation_on_return());
    s.set_conversation_on_weather_change(m.get_conversation_on_weather_change());
    s.set_conversation_on_rain_soon(m.get_conversation_on_rain_soon());
    s.set_conversation_on_low_battery(m.get_conversation_on_low_battery());
    s.set_conversation_on_charging(m.get_conversation_on_charging());
    s.set_conversation_return_after_min(m.get_conversation_return_after_min());
    s.set_conversation_cooldown_min(m.get_conversation_cooldown_min());
    s.set_conversation_save_status(m.get_conversation_save_status());
    s.set_focus_companion_enabled(m.get_focus_companion_enabled());
    s.set_focus_companion_on_start(m.get_focus_companion_on_start());
    s.set_focus_companion_on_halfway(m.get_focus_companion_on_halfway());
    s.set_focus_companion_on_focus_complete(m.get_focus_companion_on_focus_complete());
    s.set_focus_companion_on_break_end(m.get_focus_companion_on_break_end());
    s.set_focus_companion_save_status(m.get_focus_companion_save_status());
    s.set_position_mode(m.get_position_mode());
    s.set_active_pages(m.get_active_pages());
    s.set_hub_pages(m.get_hub_pages());
    // Posisi dapat berubah lewat preset, tray, atau aksi pusatkan; cerminkan pilihan terbaru
    // ke SettingsWindow agar indikator pilihan tidak tertinggal dari posisi notch sebenarnya.
    s.set_position_mode(m.get_position_mode());
    s.set_pages_save_status(m.get_pages_save_status());
    s.set_prayer_province(m.get_prayer_province());
    s.set_prayer_city(m.get_prayer_city());
    s.set_prayer_mode(m.get_prayer_mode());
    s.set_prayer_remind_before(m.get_prayer_remind_before());
    s.set_prayer_remind_min(m.get_prayer_remind_min());
    s.set_pomodoro_focus(m.get_pomodoro_focus());
    s.set_pomodoro_break(m.get_pomodoro_break());
    s.set_pomodoro_long_break(m.get_pomodoro_long_break());
    s.set_pomodoro_rounds(m.get_pomodoro_rounds());
    s.set_save_status(m.get_settings_save_status());
}

/// Sinkronkan status yang berubah saat Settings terbuka tanpa menimpa input yang sedang diedit pengguna.
pub(crate) fn sync_settings_live_state(m: &NotchWindow, s: &SettingsWindow) {
    s.set_weather_save_status(m.get_weather_save_status());
    s.set_weather_hits(m.get_weather_hits());
    s.set_weather_search_status(m.get_weather_search_status());
    s.set_adhan_active(m.get_adhan_active());
    s.set_adhan_status(m.get_prayer_status());
    s.set_appearance_save_status(m.get_appearance_save_status());
    s.set_autostart_save_status(m.get_autostart_save_status());
    s.set_behavior_save_status(m.get_behavior_save_status());
    s.set_motion_save_status(m.get_motion_save_status());
    s.set_nudge_save_status(m.get_nudge_save_status());
    s.set_greeting_save_status(m.get_greeting_save_status());
    s.set_conversation_save_status(m.get_conversation_save_status());
    s.set_focus_companion_save_status(m.get_focus_companion_save_status());
    // Pilihan docking harus mengikuti posisi terbaru setelah preset diterapkan dari Settings.
    s.set_position_mode(m.get_position_mode());
    s.set_active_pages(m.get_active_pages());
    s.set_hub_pages(m.get_hub_pages());
    s.set_pages_save_status(m.get_pages_save_status());
    s.set_save_status(m.get_settings_save_status());
}

/// Teruskan tombol Simpan/Tes di jendela Pengaturan ke handler yang sudah ada di NotchWindow.
pub(crate) fn wire_settings_window(m: &NotchWindow, s: &SettingsWindow) {
    {
        // Simpan Weak handle yang owned; jangan menangkap referensi Window dari parameter `s` ke callback 'static.
        let weak = s.as_weak();
        s.on_begin_drag(move || {
            if let Some(settings) = weak.upgrade() {
                let _ = platform::begin_window_drag(settings.window());
            }
        });
    }
    {
        let weak = m.as_weak();
        s.on_save_weather(move |a0, a1, a2| { if let Some(m) = weak.upgrade() { m.invoke_settings_save_weather(a0, a1, a2); } });
    }
    {
        let weak = m.as_weak();
        s.on_search_city(move |a0| { if let Some(m) = weak.upgrade() { m.invoke_settings_search_city(a0); } });
    }
    {
        let weak = m.as_weak();
        s.on_test_adhan(move || { if let Some(m) = weak.upgrade() { m.invoke_settings_test_adhan(); } });
    }
    {
        let weak = m.as_weak();
        s.on_pause_adhan(move |a0| { if let Some(m) = weak.upgrade() { m.invoke_settings_pause_adhan(a0); } });
    }
    {
        let weak = m.as_weak();
        s.on_stop_adhan(move || { if let Some(m) = weak.upgrade() { m.invoke_adhan_stop(); } });
    }
    {
        let weak = m.as_weak();
        s.on_restart_app(move || { if let Some(m) = weak.upgrade() { m.invoke_settings_restart(); } });
    }
    {
        let weak = m.as_weak();
        s.on_save_appearance(move |a0, a1, a2, a3, a4, a5, a6, a7, a8, a9| { if let Some(m) = weak.upgrade() { m.invoke_settings_save_appearance(a0, a1, a2, a3, a4, a5, a6, a7, a8, a9); } });
    }
    {
        let weak = m.as_weak();
        s.on_preview_appearance(move |a0, a1, a2, a3, a4, a5, a6, a7, a8, a9| {
            if let Some(m) = weak.upgrade() {
                m.invoke_settings_preview_appearance(a0, a1, a2, a3, a4, a5, a6, a7, a8, a9);
            }
        });
    }
    {
        let weak = m.as_weak();
        s.on_preview_nudge_mood(move |mood| {
            if let Some(m) = weak.upgrade() {
                m.invoke_settings_preview_appearance(
                    m.get_appearance_theme(),
                    m.get_appearance_eye_theme(),
                    m.get_appearance_notch_theme(),
                    mood,
                    m.get_appearance_accent(),
                    m.get_appearance_custom_bg(),
                    m.get_appearance_custom_text(),
                    m.get_appearance_dark_bg(),
                    m.get_appearance_light_bg(),
                    m.get_behavior_scale().parse::<f32>().unwrap_or(1.0),
                );
            }
        });
    }
    {
        let weak = m.as_weak();
        s.on_save_nudge(move |a0, a1, a2, a3, a4, a5, a6, a7| { if let Some(m) = weak.upgrade() { m.invoke_settings_save_nudge(a0, a1, a2, a3, a4, a5, a6, a7); } });
    }
    {
        let weak = m.as_weak();
        s.on_test_nudge(move |a0, a1, a2, a3, a4| { if let Some(m) = weak.upgrade() { m.invoke_settings_test_nudge(a0, a1, a2, a3, a4); } });
    }
    {
        let weak = m.as_weak();
        s.on_save_greeting(move |a0, a1, a2, a3| { if let Some(m) = weak.upgrade() { m.invoke_settings_save_greeting(a0, a1, a2, a3); } });
    }
    {
        let weak = m.as_weak();
        s.on_test_greeting(move |a0, a1, a2| { if let Some(m) = weak.upgrade() { m.invoke_settings_test_greeting(a0, a1, a2); } });
    }
    {
        let weak = m.as_weak();
        s.on_save_conversation(move |a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10| { if let Some(m) = weak.upgrade() { m.invoke_settings_save_conversation(a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10); } });
    }
    {
        let weak = m.as_weak();
        s.on_test_conversation(move |a0, a1| { if let Some(m) = weak.upgrade() { m.invoke_settings_test_conversation(a0, a1); } });
    }
    {
        let weak = m.as_weak();
        s.on_save_focus_companion(move |a0, a1, a2, a3, a4| { if let Some(m) = weak.upgrade() { m.invoke_settings_save_focus_companion(a0, a1, a2, a3, a4); } });
    }
    {
        let weak = m.as_weak();
        s.on_test_focus_companion(move |a0, a1| { if let Some(m) = weak.upgrade() { m.invoke_settings_test_focus_companion(a0, a1); } });
    }
    {
        let weak = m.as_weak();
        s.on_save_behavior(move |a0, a1, a2, a3, a4| { if let Some(m) = weak.upgrade() { m.invoke_settings_save_behavior(a0, a1, a2, a3, a4); } });
    }
    {
        let weak = m.as_weak();
        s.on_save_autostart(move |a0| { if let Some(m) = weak.upgrade() { m.invoke_settings_save_autostart(a0); } });
    }
    {
        let weak = m.as_weak();
        s.on_save_position(move |a0| { if let Some(m) = weak.upgrade() { m.invoke_settings_save_position(a0); } });
    }
    {
        let weak = m.as_weak();
        s.on_save_motion(move |a0, a1, a2, a3| { if let Some(m) = weak.upgrade() { m.invoke_settings_save_motion(a0, a1, a2, a3); } });
    }
    {
        let weak = m.as_weak();
        s.on_test_dock_play(move || { if let Some(m) = weak.upgrade() { m.invoke_settings_test_dock_play(); } });
    }
    {
        let weak = m.as_weak();
        s.on_save_pages(move |a0| { if let Some(m) = weak.upgrade() { m.invoke_settings_save_pages(a0); } });
    }
    {
        let weak = m.as_weak();
        s.on_toggle_page(move |a0, a1| { if let Some(m) = weak.upgrade() { m.invoke_settings_toggle_page(a0, a1); } });
    }
    {
        let weak = m.as_weak();
        s.on_save_routines(move |a0, a1, a2, a3, a4, a5, a6, a7, a8| { if let Some(m) = weak.upgrade() { m.invoke_settings_save_routines(a0, a1, a2, a3, a4, a5, a6, a7, a8); } });
    }
    {
        let weak = m.as_weak();
        s.on_save_shortcuts(move |a0, a1, a2, a3| { if let Some(m) = weak.upgrade() { m.invoke_settings_save_shortcuts(a0, a1, a2, a3); } });
    }
}

/// Jendela Pengaturan memakai palet gelap sendiri (seperti versi Tauri) agar selalu terbaca, apa pun tema notch.
/// Hanya warna aksen yang diikuti dari pengaturan pengguna. Tiap jendela Slint punya instance global Theme sendiri.
pub(crate) fn sync_settings_theme(m: &NotchWindow, s: &SettingsWindow) {
    let to = Theme::get(s);
    let rgba = |r: u8, g: u8, b: u8, a: u8| slint::Color::from_argb_u8(a, r, g, b);
    let accent = Theme::get(m).get_accent();
    // Aksen terlalu gelap tidak terbaca di latar gelap: pakai aksen bawaan.
    let luma = 0.299 * f32::from(accent.red()) + 0.587 * f32::from(accent.green()) + 0.114 * f32::from(accent.blue());
    let (accent, on_accent) = if luma < 90.0 {
        (rgba(0x78, 0x8c, 0xff, 255), rgba(255, 255, 255, 255))
    } else if luma > 170.0 {
        (accent, rgba(0x10, 0x12, 0x18, 255))
    } else {
        (accent, rgba(255, 255, 255, 255))
    };
    to.set_body(rgba(0x14, 0x16, 0x1d, 255));
    to.set_text(rgba(0xec, 0xee, 0xf5, 255));
    to.set_text_dim(rgba(0xec, 0xee, 0xf5, 0xb8));
    to.set_text_faint(rgba(0xec, 0xee, 0xf5, 0x85));
    to.set_accent(accent);
    to.set_on_accent(on_accent);
    to.set_ok(rgba(0x7b, 0xdc, 0x9a, 255));
    to.set_chip(rgba(255, 255, 255, 0x16));
    to.set_chip_hover(rgba(255, 255, 255, 0x28));
    to.set_track(rgba(255, 255, 255, 0x1f));
}

/// Jendela Pengaturan yang sedang terbuka. Timer menjaga nilainya sama dengan NotchWindow selama jendela hidup;
/// saat jendela ditutup, `SettingsHandle` dilepas sehingga memori dan timer ikut bebas.
pub(crate) struct SettingsHandle {
    pub(crate) win: SettingsWindow,
    pub(crate) _sync: Timer,
    // Mencegah callback close yang tertunda melepas jendela yang sudah dibuka kembali.
    close_pending: Rc<Cell<bool>>,
}
pub(crate) type SettingsSlot = Rc<RefCell<Option<SettingsHandle>>>;

/// Buka (atau munculkan kembali) jendela Pengaturan terpisah, setara jendela Pengaturan versi Tauri.
pub(crate) fn open_settings_window(main: &NotchWindow, slot: &SettingsSlot) {
    // Ambil Weak dalam scope borrow singkat agar operasi show/focus tidak berjalan saat RefCell dipinjam.
    let existing = {
        slot.borrow().as_ref().map(|handle| (handle.win.as_weak(), handle.close_pending.clone()))
    };
    if let Some((existing, close_pending)) = existing {
        if let Some(win) = existing.upgrade() {
            // Pengguna membuka kembali jendela sebelum timer close lama berjalan.
            close_pending.set(false);
            sync_settings_theme(main, &win);
            sync_settings_window(main, &win);
            let _ = win.show();
            let _ = win.window().with_winit_window(|w| {
                w.set_minimized(false);
                w.focus_window();
            });
            request_settings_redraw(&win);
            return;
        }
        // Window lama sudah tidak hidup; batalkan juga timer close miliknya sebelum membuat yang baru.
        close_pending.set(false);
        slot.borrow_mut().take();
    }
    let win = match SettingsWindow::new() {
        Ok(win) => win,
        Err(error) => {
            eprintln!("[Settings] Gagal membuat jendela: {error}");
            return;
        }
    };
    sync_settings_theme(main, &win);
    sync_settings_window(main, &win);
    wire_settings_window(main, &win);
    let close_pending = Rc::new(Cell::new(false));
    {
        let slot = slot.clone();
        let pending = close_pending.clone();
        win.window().on_close_requested(move || {
            // Jangan lepas SettingsHandle sebelum callback selesai. Bila jendela dibuka lagi
            // dalam jeda ini, flag dibatalkan dan timer lama tidak boleh mengambil handle aktif.
            pending.set(true);
            let slot = slot.clone();
            let pending = pending.clone();
            Timer::single_shot(Duration::from_millis(50), move || {
                if pending.get() {
                    slot.borrow_mut().take();
                }
            });
            slint::CloseRequestResponse::HideWindow
        });
    }
    let sync = Timer::default();
    {
        let (main_weak, win_weak) = (main.as_weak(), win.as_weak());
        sync.start(TimerMode::Repeated, Duration::from_millis(250), move || {
            if let (Some(m), Some(s)) = (main_weak.upgrade(), win_weak.upgrade()) {
                sync_settings_theme(&m, &s);
                sync_settings_live_state(&m, &s);
            }
        });
    }
    if let Err(error) = win.show() {
        eprintln!("[Settings] Gagal menampilkan jendela: {error}");
        return;
    }
    // Posisikan jendela Pengaturan di tengah monitor pertama kali dibuat.
    // Ukuran Window Slint sudah berupa piksel fisik setelah show().
    let area = platform::work_area(None);
    let size = win.window().size();
    let x = area.x + (area.w - size.width as i32).max(0) / 2;
    let y = area.y + (area.h - size.height as i32).max(0) / 2;
    win.window().set_position(PhysicalPosition::new(x, y));
    request_settings_redraw(&win);
    *slot.borrow_mut() = Some(SettingsHandle { win, _sync: sync, close_pending });
}
