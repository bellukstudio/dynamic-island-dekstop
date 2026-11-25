//! Prototipe Notch dengan Slint. Target utama: Windows.
//! Di OS lain hanya untuk uji kompilasi (posisi kursor dan jam memakai fallback).

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use notch_core::battery::{BatteryMonitor, Reading};
use notch_core::calendar::{self, Date, Holidays, Item, View as CalView};
use notch_core::clipboard::ClipboardMonitor;
use notch_core::config::Config;
use notch_core::decor;
use notch_core::eyes::{self, Anim, EyeRenderer, Face, Morph, Mood, MoodState};
use notch_core::layout::{self, Dock};
use notch_core::notes::{Note, NotesStore};
use notch_core::pomodoro::{Phase, Pomodoro};
use notch_core::prayer::{self, Day as PrayerDay};
use notch_core::quick::{self, Quick};
use notch_core::reminder::{self, Reminder};
use notch_core::stats::StatsMonitor;
use slint::Model;
use notch_core::theme::{Palette, Rgba};
use slint::winit_030::{winit, EventResult, WinitWindowAccessor};
use slint::{
    ComponentHandle, Global, Image, ModelRc, PhysicalPosition, PhysicalSize, Rgba8Pixel,
    SharedPixelBuffer, SharedString, Timer, TimerMode, VecModel, Weak,
};

mod privacy;
mod shelf;
mod tray;
mod clip;
mod hotkeys;
mod colorpick;
mod compress;
mod convert;
mod transfer;
mod android;
mod android_mirror;
mod android_ui;
mod compress_ui;
mod convert_ui;
mod transfer_ui;
mod autostart;
mod speech;
mod media;
mod platform;
mod appearance;
mod convert_store;
mod currency_ui;
mod eye_state;
mod formatting;
mod notes_ui;
mod panels;
mod prayer_ui;
mod reminder_ui;
mod settings_window;
mod shell;
mod window_behavior;
mod window_size;

use appearance::*;
use convert_store::*;
use currency_ui::*;
use eye_state::*;
use formatting::*;
use notes_ui::*;
use panels::*;
use prayer_ui::*;
use reminder_ui::*;
use settings_window::*;
use shell::*;
use window_behavior::*;
use window_size::*;

slint::include_modules!();

fn main() -> Result<(), slint::PlatformError> {
    if let Some(resource_dir) = std::env::current_exe().ok().and_then(|path| path.parent().map(std::path::Path::to_path_buf)) {
        notch_tools::pdf::set_resource_dir(resource_dir.clone());
        notch_tools::compress::set_resource_dir(resource_dir);
    }
    // Config dibaca saja (tidak pernah ditulis/dibuat dari sini).
    let cfg = Config::load_readonly(&notch_core::paths::config_path());

    let palette = Palette::from_appearance(&cfg.appearance, platform::os_light());
    // Posisi notch: kustom dari config (pusat x, atas y; piksel fisik) atau None = tengah atas.
    // Tombol "Pusatkan" hanya mengembalikannya ke tengah selama sesi ini (config tidak ditulis dari sini).
    let shell = Rc::new(Shell {
        pos: Cell::new(cfg.position.custom.then_some((cfg.position.x, cfg.position.y))),
        monitor_hint: Cell::new((cfg.position.custom
            && (cfg.position.monitor_center_x != 0 || cfg.position.monitor_center_y != 0))
            .then_some((cfg.position.monitor_center_x, cfg.position.monitor_center_y))),
        theme: RefCell::new(cfg.appearance.notch_theme.clone()),
        body: RefCell::new(palette.body),
        scale: RefCell::new(cfg.scale as f32),
        memo: Cell::new((None, 0)),
    });

    let appearance_eye: Rc<RefCell<Option<Rc<RefCell<EyeCtx>>>>> = Rc::new(RefCell::new(None));
    let greeting_voice_runtime = Rc::new(RefCell::new(cfg.greeting.voice.clone()));
    let sleep_after_s = Rc::new(Cell::new(cfg.timing.sleep_after_s));
    let hide_fullscreen = Rc::new(Cell::new(cfg.hide_fullscreen));
    let win = NotchWindow::new()?;
    // Sembunyikan frame pertama sampai posisi, ukuran, dock, dan hit-test native selesai dipasang.
    // Tanpa ini, dock samping sempat menampilkan badan/top-layout berbentuk persegi pada startup.
    win.set_geometry_hold(true);
    // Pulihkan wajah idle sebelum geometri dan hit-test awal dipasang.
    // Jika dibaca setelah Shell::place, region native masih selebar mode mata (130 px)
    // sementara isi Slint sudah menggambar jam (220 px), sehingga jam terpotong saat startup.
    let idle_clock = std::fs::read_to_string(notch_core::paths::data_file("idle_face.txt"))
        .map(|value| value.trim() == "clock")
        .unwrap_or(false);
    win.set_idle_clock(idle_clock);
    if idle_clock {
        // Samakan state halaman dengan wajah jam yang dipulihkan, dan tahan render pertama
        // sampai native window selesai dipasang pada ukuran 360 px (hindari frame jam terpotong).
        win.set_page_id("clock".into());
        win.set_geometry_hold(true);
    }
    // Pengaturan dibuka sebagai jendela terpisah (bukan halaman di dalam notch): tab Pengaturan, tray, dan shortcut memakai jalur ini.
    let settings_slot: SettingsSlot = Rc::default();
    let open_settings: Rc<dyn Fn()> = {
        let (weak, slot) = (win.as_weak(), settings_slot.clone());
        let opening = Rc::new(Cell::new(false));
        Rc::new(move || {
            if opening.replace(true) {
                return;
            }
            let (weak, slot, opening) = (weak.clone(), slot.clone(), opening.clone());
            Timer::single_shot(Duration::from_millis(1), move || {
                if let Some(w) = weak.upgrade() {
                    open_settings_window(&w, &slot);
                }
                opening.set(false);
            });
        })
    };
    {
        let open = open_settings.clone();
        win.on_open_settings(move || open());
    }
    win.set_convert_history_text(format_convert_history().into());
    win.set_convert_source_preview(Image::default());
    win.set_convert_result_preview(Image::default());
    let transfer_state = Arc::new(Mutex::new(transfer::TransferState::new(notch_core::paths::data_file("Notch"))));
    let android_state = Arc::new(Mutex::new(android::AndroidState::new()));
    let mirror_process = Arc::new(Mutex::new(android_mirror::MirrorProcess::new()));
    win.set_convert_selected_path("".into());
    win.set_compress_selected_path("".into());
    win.set_compress_status("Pilih gambar JPG/PNG/BMP, PDF, atau dokumen DOCX/XLSX/PPTX untuk dikompres".into());
    let (hotkey_rx, hotkey_config_tx, _hotkey_thread) = hotkeys::start(&cfg.shortcuts);
    privacy::start(win.as_weak());
    apply_theme(&win, &palette);
    win.set_ui_scale(cfg.scale as f32);
    shell.place(&win, COLLAPSED);
    // Perubahan pesan inline harus memperbarui native hit-test region: dock samping hanya memakai
    // bounding box notch + SpeechBubble, bukan seluruh window transparan 300x262.
    {
        let weak = win.as_weak();
        let shell = shell.clone();
        win.on_region_layout_changed(move || {
            let Some(w) = weak.upgrade() else { return };
            if w.get_expanded() || w.get_dragging() || w.get_geometry_hold() { return; }
            let size = if w.get_greeting_inline_visible() && w.get_dock() < 2 {
                GREETING_INLINE
            } else {
                COLLAPSED
            };
            // Saat transisi hub -> compact, native window sengaja tetap besar selama 330 ms.
            // Jangan memotongnya lebih awal; shrink_window_synced akan memasang region setelah animasi.
            if !window_needs_resize(&w, size) {
                shell.place(&w, size);
            }
        });
    }
    // Shell::place pertama berjalan sebelum event loop Winit membuat HWND.
    // Ulangi sekali setelah native window siap agar posisi awal (tengah atas monitor) benar-benar diterapkan.
    {
        let weak = win.as_weak();
        let shell = shell.clone();
        Timer::single_shot(Duration::from_millis(350), move || {
            if let Some(w) = weak.upgrade() {
                shell.memo.set((None, 0));
                shell.place(&w, COLLAPSED);
                // Semua mode dock ditampilkan setelah geometri native siap, bukan hanya idle-clock.
                w.set_geometry_hold(false);
                platform::hide_from_taskbar(w.window());
                w.window().request_redraw();
            }
        });
    }

    // Sembunyikan notch saat aplikasi foreground benar-benar menutupi seluruh monitor.
    // Timer tetap aktif ketika window disembunyikan agar notch muncul kembali setelah fullscreen berakhir.
    let fullscreen_hidden = Rc::new(Cell::new(false));
    let _fullscreen_timer = start_fullscreen_watch(&win, &shell, &hide_fullscreen, &fullscreen_hidden);
    start_native_drag(&win, &shell);

    // Liquid movement: timer selalu hidup agar toggle dan interval dapat diterapkan tanpa restart.
    // Nilai runtime dibagikan dengan handler pengaturan; tidak perlu membuat timer baru.
    let motion_runtime = Rc::new((
        Cell::new(cfg.motion.enabled),
        Cell::new(cfg.motion.interval_secs.clamp(60, 3_600)),
        Cell::new(cfg.motion.max_moves.clamp(1, 10)),
    ));
    let liquid_move_timer = Timer::default();
    {
        let weak = win.as_weak();
        let shell = shell.clone();
        let runtime = motion_runtime.clone();
        let hidden_by_fullscreen = fullscreen_hidden.clone();
        let started_at = Instant::now();
        let mut last_move = Instant::now();
        let mut moves = 0u32;
        let mut was_blocked = false;
        let mut animation: Option<(PhysicalPosition, PhysicalPosition, Instant)> = None;
        const MOVE_DURATION: Duration = Duration::from_millis(2_800);

        liquid_move_timer.start(TimerMode::Repeated, Duration::from_millis(16), move || {
            let Some(w) = weak.upgrade() else { return };

            if !runtime.0.get() {
                if animation.take().is_some() {
                    let position = w.window().position();
                    let size = w.window().size();
                    shell.pos.set(Some((position.x + size.width as i32 / 2, position.y)));
                }
                // Batas perpindahan dihitung ulang setiap kali fitur diaktifkan kembali.
                moves = 0;
                last_move = Instant::now();
                return;
            }
            let interval = runtime.1.get().clamp(60, 3_600);
            let max_moves = runtime.2.get().clamp(1, 10);

            let blocked = hidden_by_fullscreen.get()
                || w.get_expanded()
                || w.get_play_interaction_active()
                || w.get_nudge_visible()
                || w.get_reminder_alert_visible()
                || !w.get_meeting_finished().is_empty()
                || w.get_meeting_active()
                || w.get_adhan_active();
            if blocked {
                // Jangan biarkan gerakan otomatis mengambil alih pointer atau menggeser
                // notch ketika kartu/pengingat/rapat/adzan sedang tampil.
                if !was_blocked {
                    last_move = Instant::now();
                    was_blocked = true;
                }
                if animation.take().is_some() {
                    let position = w.window().position();
                    let size = w.window().size();
                    shell.pos.set(Some((position.x + size.width as i32 / 2, position.y)));
                }
                return;
            }
            if was_blocked {
                // Mulai ulang hitungan interval setelah interaksi berakhir; jangan langsung
                // memindahkan notch pada frame pertama setelah pointer keluar.
                last_move = Instant::now();
                was_blocked = false;
            }

            if let Some((from, to, began)) = animation {
                let progress = (began.elapsed().as_secs_f32() / MOVE_DURATION.as_secs_f32()).clamp(0.0, 1.0);
                // Smoothstep (ease-in-out): akselerasi dan perlambatan lembut di kedua ujung.
                let eased = progress * progress * (3.0 - 2.0 * progress);
                let x = from.x as f32 + (to.x - from.x) as f32 * eased;
                let y = from.y as f32 + (to.y - from.y) as f32 * eased;
                w.window().set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32));

                if progress >= 1.0 {
                    w.window().set_position(to);
                    shell.place(&w, COLLAPSED);
                    animation = None;
                    last_move = Instant::now();
                    moves = moves.saturating_add(1);
                }
                return;
            }

            if moves >= max_moves || last_move.elapsed() < Duration::from_secs(u64::from(interval)) {
                return;
            }

            let current = w.window().position();
            let size = w.window().size();
            // Gunakan pusat jendela agar monitor terdeteksi benar pada konfigurasi multi-monitor,
            // termasuk monitor yang berada di sebelah kiri/atas dengan koordinat negatif.
            let monitor_center = (
                current.x + size.width as i32 / 2,
                current.y + size.height as i32 / 2,
            );
            let work = platform::work_area(Some(monitor_center));
            // Margin dalam logical pixels agar konsisten pada monitor dengan DPI berbeda.
            let margin = (36.0 * w.window().scale_factor()).round() as i32;
            // Posisi kustom Shell memakai titik tengah horizontal dan sisi atas vertikal.
            let half_width = size.width as i32 / 2;
            let min_x = work.x + half_width + margin;
            let max_x = (work.x + work.w - half_width - margin).max(min_x);
            let min_y = work.y + margin;
            let max_y = (work.y + work.h - size.height as i32 - margin).max(min_y);

            let seed = started_at.elapsed().as_nanos() as u64
                ^ (moves as u64).wrapping_mul(0x9E3779B97F4A7C15);
            let mut value = seed ^ (seed >> 30);
            value = value.wrapping_mul(0xBF58476D1CE4E5B9);
            value ^= value >> 27;
            let rx = (value % (max_x - min_x + 1).max(1) as u64) as i32;
            value = value.wrapping_mul(0x94D049BB133111EB);
            let ry = (value % (max_y - min_y + 1).max(1) as u64) as i32;
            let target_pos = (min_x + rx, min_y + ry);
            let target = layout::place(&work, size.width as i32, size.height as i32, Some(target_pos)).0;
            let from = PhysicalPosition::new(current.x, current.y);
            let to = PhysicalPosition::new(target.0, target.1);

            shell.pos.set(Some(target_pos));
            if from == to {
                shell.place(&w, COLLAPSED);
                last_move = Instant::now();
                moves = moves.saturating_add(1);
            } else {
                animation = Some((from, to, Instant::now()));
            }
        });
    }

    // Hanya halaman hub yang boleh menjadi tab. Convert, Compress, Transfer, dan Android
    // adalah alat berjendela terpisah yang dibuka melalui pintasan, bukan menu hub.
    // Lirik tetap berada di halaman Media (tombol "Lirik"), seperti di Tauri.
    let standalone_tools = ["convert", "compress", "transfer", "android"];
    let pages: Vec<String> = cfg.pages.iter()
        .filter(|p| p.as_str() != "lyrics" && !standalone_tools.contains(&p.as_str()))
        .cloned()
        .collect();
    let pages = if pages.is_empty() { vec!["media".to_string()] } else { pages };
    win.set_tabs(ModelRc::from(Rc::new(VecModel::from(
        pages.iter().map(|p| SharedString::from(p.as_str())).collect::<Vec<_>>(),
    ))));
    win.set_page_id(pages[0].as_str().into());
    win.set_shortcut_hub(cfg.shortcuts.toggle_hub.clone().into());
    win.set_shortcut_settings(cfg.shortcuts.open_settings.clone().into());
    win.set_shortcut_pomodoro(cfg.shortcuts.toggle_pomodoro.clone().into());
    win.set_shortcut_prayer(cfg.shortcuts.toggle_prayer.clone().into());
    win.set_appearance_theme(cfg.appearance.theme.clone().into());
    win.set_appearance_eye_theme(cfg.appearance.eye_theme.clone().into());
    win.set_appearance_notch_theme(cfg.appearance.notch_theme.clone().into());
    win.set_appearance_mood(cfg.appearance.default_mood.clone().into());
    win.set_appearance_accent(cfg.appearance.accent.clone().into());
    win.set_appearance_custom_bg(cfg.appearance.background.clone().into());
    win.set_appearance_custom_text(cfg.appearance.text.clone().into());
    win.set_appearance_dark_bg(cfg.appearance.dark_bg.clone().into());
    win.set_appearance_light_bg(cfg.appearance.light_bg.clone().into());
    win.set_prayer_province(cfg.prayer.provinsi.clone().into());
    win.set_prayer_city(cfg.prayer.kabkota.clone().into());
    win.set_prayer_mode(cfg.prayer.mode.clone().into());
    win.set_prayer_remind_before(cfg.prayer.remind_before.to_string().into());
    win.set_prayer_remind_min(cfg.prayer.remind_min.to_string().into());
    win.set_pomodoro_focus(cfg.pomodoro.focus_min.to_string().into());
    win.set_pomodoro_break(cfg.pomodoro.break_min.to_string().into());
    win.set_pomodoro_long_break(cfg.pomodoro.long_break_min.to_string().into());
    win.set_pomodoro_rounds(cfg.pomodoro.rounds.to_string().into());
    win.set_behavior_scale(format!("{:.2}", cfg.scale).into());
    win.set_behavior_hover_open(cfg.timing.hover_open_ms.to_string().into());
    win.set_behavior_hover_close(cfg.timing.hover_close_ms.to_string().into());
    win.set_behavior_sleep_after(cfg.timing.sleep_after_s.to_string().into());
    win.set_behavior_hide_fullscreen(cfg.hide_fullscreen);
    win.set_autostart_enabled(autostart::is_enabled());
    win.set_motion_enabled(cfg.motion.enabled);
    win.set_dock_play_enabled(cfg.motion.dock_play_enabled);
    win.set_motion_interval(cfg.motion.interval_secs.to_string().into());
    win.set_motion_max_moves(cfg.motion.max_moves.to_string().into());
    win.set_nudge_enabled(cfg.nudge.enabled);
    win.set_nudge_idle_min(cfg.nudge.idle_min.to_string().into());
    win.set_nudge_message(cfg.nudge.message.clone().into());
    win.set_nudge_emoji(cfg.nudge.emoji.clone().into());
    win.set_nudge_mood(cfg.nudge.mood.clone().into());
    win.set_nudge_show_ms((cfg.nudge.show_ms / 1_000).to_string().into());
    win.set_nudge_only_silent(cfg.nudge.only_silent);
    win.set_nudge_until_move(cfg.nudge.until_move);
    win.set_greeting_enabled(cfg.greeting.enabled);
    win.set_greeting_mode(cfg.greeting.mode.clone().into());
    win.set_greeting_voice(cfg.greeting.voice.clone().into());
    win.set_greeting_custom_text(cfg.greeting.custom_text.clone().into());
    win.set_conversation_enabled(cfg.conversation_moments.enabled);
    win.set_conversation_on_play(cfg.conversation_moments.on_play);
    win.set_conversation_on_track_change(cfg.conversation_moments.on_track_change);
    win.set_conversation_on_pause(cfg.conversation_moments.on_pause);
    win.set_conversation_on_return(cfg.conversation_moments.on_return);
    win.set_conversation_on_weather_change(cfg.conversation_moments.on_weather_change);
    win.set_conversation_on_rain_soon(cfg.conversation_moments.on_rain_soon);
    win.set_conversation_on_low_battery(cfg.conversation_moments.on_low_battery);
    win.set_conversation_on_charging(cfg.conversation_moments.on_charging);
    win.set_conversation_return_after_min(cfg.conversation_moments.return_after_min.to_string().into());
    win.set_conversation_cooldown_min(cfg.conversation_moments.cooldown_min.to_string().into());
    win.set_focus_companion_enabled(cfg.focus_companion.enabled);
    win.set_focus_companion_on_start(cfg.focus_companion.on_start);
    win.set_focus_companion_on_halfway(cfg.focus_companion.on_halfway);
    win.set_focus_companion_on_focus_complete(cfg.focus_companion.on_focus_complete);
    win.set_focus_companion_on_break_end(cfg.focus_companion.on_break_end);
    win.set_hub_pages(pages.join(", ").into());
    let initial_position_mode = if !cfg.position.custom {
        "top"
    } else if cfg.position.x == notch_core::layout::LEFT_TX {
        "left"
    } else if cfg.position.x == notch_core::layout::RIGHT_TX {
        "right"
    } else if cfg.position.y >= notch_core::layout::BOTTOM_TY / 2 {
        "bottom"
    } else {
        "floating"
    };
    win.set_position_mode(initial_position_mode.into());
    win.set_active_pages(ModelRc::from(Rc::new(VecModel::from(pages.iter().map(|page| SharedString::from(page.as_str())).collect::<Vec<_>>()))));
    {
        let weak = win.as_weak();
        let config_path = notch_core::paths::config_path();
        win.on_settings_save_pages(move |value| {
            let allowed = ["media", "clock", "weather", "battery", "timer", "quick", "tools", "stats", "reminder", "notes", "clipboard", "shelf", "lyrics", "privacy", "prayer", "settings", "currency"];
            let mut pages = Vec::<String>::new();
            for item in value.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                if !allowed.contains(&item) {
                    if let Some(w) = weak.upgrade() {
                        w.set_pages_save_status(format!("ID halaman tidak dikenal: {item}").into());
                    }
                    return;
                }
                if !pages.iter().any(|p| p == item) {
                    pages.push(item.to_string());
                }
            }
            if pages.is_empty() || !pages.iter().any(|p| p == "media") {
                if let Some(w) = weak.upgrade() {
                    w.set_pages_save_status("Minimal sertakan halaman media.".into());
                }
                return;
            }
            let mut next = Config::load_readonly(&config_path);
            next.pages = pages.clone();
            next.pages_version = notch_core::config::PAGES_VERSION;
            match next.save(&config_path) {
                Ok(()) => {

                    let tabs: Vec<String> = pages.iter().filter(|p| p.as_str() != "lyrics" && !["convert", "compress", "transfer", "android"].contains(&p.as_str())).cloned().collect();
                    let tabs = if tabs.is_empty() { vec!["media".to_string()] } else { tabs };
                    if let Some(w) = weak.upgrade() {
                        let current = w.get_page_id().to_string();
                        w.set_tabs(ModelRc::from(Rc::new(VecModel::from(
                            tabs.iter().map(|p| SharedString::from(p.as_str())).collect::<Vec<_>>(),
                        ))));
                        if !tabs.iter().any(|p| p == &current) {
                            w.set_page_id(tabs[0].as_str().into());
                        }
                        w.set_hub_pages(pages.join(", ").into());
                        w.set_active_pages(ModelRc::from(Rc::new(VecModel::from(pages.iter().map(|page| SharedString::from(page.as_str())).collect::<Vec<_>>()))));
                        w.set_pages_save_status("Halaman hub tersimpan dan langsung diperbarui.".into());
                    }
                }
                Err(error) => {
                    if let Some(w) = weak.upgrade() {
                        w.set_pages_save_status(format!("Gagal menyimpan: {error}").into());
                    }
                }
            }
        });
    }

    // Animasi perpindahan jendela native saat pengguna mengganti docking.
    // Timer berjalan hanya selama transisi; state baru menggantikan animasi sebelumnya.
    let dock_move_state = Rc::new(RefCell::new(None::<(i32, i32, i32, i32, Instant)>));
    let dock_move_timer = Rc::new(Timer::default());

    {
        let weak = win.as_weak();
        let config_path = notch_core::paths::config_path();
        let shell = shell.clone();
        let move_state = dock_move_state.clone();
        let move_timer = dock_move_timer.clone();
        win.on_settings_save_position(move |mode| {
            let mode = mode.to_string();
            if !["top", "floating", "bottom", "left", "right"].contains(&mode.as_str()) {
                if let Some(w) = weak.upgrade() { w.set_behavior_save_status("Mode posisi tidak valid".into()); }
                return;
            }
            // Terapkan preset pada monitor tempat notch sedang berada, bukan selalu monitor utama.
            let current_monitor = weak.upgrade().and_then(|w| {
                let position = w.window().position();
                let size = w.window().size();
                (size.width > 0 && size.height > 0).then_some((
                    position.x + size.width as i32 / 2,
                    position.y + size.height as i32 / 2,
                ))
            });
            let work = platform::work_area(current_monitor);
            let center_x = work.x + work.w / 2;
            let center_y = work.y + work.h / 2;
            let next_pos = match mode.as_str() {
                "top" => None,
                // Mode melayang tetap dekat tepi atas, dengan jarak visual sekitar 10 px.
                "floating" => Some((center_x, work.y + 10)),
                "bottom" => Some((center_x, notch_core::layout::BOTTOM_TY)),
                "left" => Some((notch_core::layout::LEFT_TX, center_y)),
                "right" => Some((notch_core::layout::RIGHT_TX, center_y)),
                _ => None,
            };
            let mut next = Config::load_readonly(&config_path);
            match next_pos {
                Some((x, y)) => {
                    next.position.custom = true;
                    next.position.x = x;
                    next.position.y = y;
                    // Anchor monitor disimpan untuk semua preset agar bottom sentinel tidak
                    // dipakai sebagai koordinat monitor saat aplikasi dibuka ulang.
                    next.position.monitor_center_x = center_x;
                    next.position.monitor_center_y = center_y;
                }
                None => {
                    next.position.custom = false;
                    next.position.x = 0;
                    next.position.y = 0;
                    next.position.monitor_center_x = 0;
                    next.position.monitor_center_y = 0;
                }
            }
            match next.save(&config_path) {
                Ok(()) => {

                    shell.pos.set(next_pos);
                    shell.monitor_hint.set(next_pos.map(|_| (center_x, center_y)));
                    if let Some(w) = weak.upgrade() {
                        let from = w.window().position();
                        let size = if w.get_expanded() { expanded_size(&w) } else { COLLAPSED };
                        shell.place(&w, size);
                        let target = w.window().position();
                        if (from.x, from.y) != (target.x, target.y) {
                            w.window().set_position(from);
                            move_state.replace(Some((from.x, from.y, target.x, target.y, Instant::now())));
                            move_timer.start(TimerMode::Repeated, Duration::from_millis(16), {
                                let state = move_state.clone();
                                let weak = weak.clone();
                                let timer_weak = Rc::downgrade(&move_timer);
                                move || {
                                    let Some((sx, sy, tx, ty, started_at)) = *state.borrow() else {
                                        if let Some(timer) = timer_weak.upgrade() { timer.stop(); }
                                        return;
                                    };
                                    let Some(w) = weak.upgrade() else {
                                        state.replace(None);
                                        if let Some(timer) = timer_weak.upgrade() { timer.stop(); }
                                        return;
                                    };
                                    let t = (started_at.elapsed().as_secs_f32() / 0.360).clamp(0.0, 1.0);
                                    let eased = 1.0 - (1.0 - t).powi(3);
                                    let x = sx as f32 + (tx - sx) as f32 * eased;
                                    let y = sy as f32 + (ty - sy) as f32 * eased;
                                    w.window().set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32));
                                    if t >= 1.0 {
                                        state.replace(None);
                                        if let Some(timer) = timer_weak.upgrade() { timer.stop(); }
                                    }
                                }
                            });
                        }
                        w.set_position_mode(mode.clone().into());
                        w.set_behavior_save_status(format!("Posisi notch disimpan: {mode}").into());
                    }
                }
                Err(error) => { if let Some(w) = weak.upgrade() { w.set_behavior_save_status(format!("Gagal menyimpan posisi: {error}").into()); } }
            }
        });
    }
    {
        let weak = win.as_weak();
        let config_path = notch_core::paths::config_path();
        win.on_settings_toggle_page(move |id, enabled| {
            let allowed = ["media", "clock", "weather", "battery", "timer", "quick", "tools", "stats", "reminder", "notes", "clipboard", "shelf", "lyrics", "privacy", "prayer", "settings", "currency"];
            let id = id.to_string();
            if !allowed.contains(&id.as_str()) { return; }
            let mut next = Config::load_readonly(&config_path);
            if enabled { if !next.pages.iter().any(|page| page == &id) { next.pages.push(id.clone()); } }
            else {
                if id == "media" { if let Some(w) = weak.upgrade() { w.set_pages_save_status("Halaman Musik wajib tetap aktif.".into()); } return; }
                next.pages.retain(|page| page != &id);
            }
            next.pages_version = notch_core::config::PAGES_VERSION;
            match next.save(&config_path) {
                Ok(()) => {

                    let tabs = next.pages.iter().filter(|page| page.as_str() != "lyrics" && !["convert", "compress", "transfer", "android"].contains(&page.as_str())).cloned().collect::<Vec<_>>();
                    let tabs = if tabs.is_empty() { vec!["media".to_string()] } else { tabs };
                    if let Some(w) = weak.upgrade() {
                        let current = w.get_page_id().to_string();
                        w.set_tabs(ModelRc::from(Rc::new(VecModel::from(tabs.iter().map(|page| SharedString::from(page.as_str())).collect::<Vec<_>>()))));
                        if !tabs.iter().any(|page| page == &current) { w.set_page_id(tabs[0].as_str().into()); }
                        w.set_hub_pages(tabs.join(", ").into());
                        w.set_active_pages(ModelRc::from(Rc::new(VecModel::from(tabs.iter().map(|page| SharedString::from(page.as_str())).collect::<Vec<_>>()))));
                        w.set_pages_save_status(format!("{} {}", if enabled { "Diaktifkan:" } else { "Dinonaktifkan:" }, id).into());
                    }
                }
                Err(error) => { if let Some(w) = weak.upgrade() { w.set_pages_save_status(format!("Gagal menyimpan: {error}").into()); } }
            }
        });
    }
    {
        let weak = win.as_weak();
        let config_path = notch_core::paths::config_path();
        let hotkey_config_tx = hotkey_config_tx.clone();
        win.on_settings_save_shortcuts(move |hub, settings, pomo, prayer| {
            let mut next = Config::load_readonly(&config_path);
            next.shortcuts.toggle_hub = hub.to_string();
            next.shortcuts.open_settings = settings.to_string();
            next.shortcuts.toggle_pomodoro = pomo.to_string();
            next.shortcuts.toggle_prayer = prayer.to_string();
            next.normalize();
            if let Err(error) = hotkeys::validate_shortcuts(&next.shortcuts) {
                if let Some(w) = weak.upgrade() {
                    w.set_settings_save_status(error.into());
                }
                return;
            }
            match next.save(&config_path) {
                Ok(()) => {

                    let _ = hotkey_config_tx.send(next.shortcuts.clone());
                    if let Some(w) = weak.upgrade() {
                        w.set_shortcut_hub(next.shortcuts.toggle_hub.clone().into());
                        w.set_shortcut_settings(next.shortcuts.open_settings.clone().into());
                        w.set_shortcut_pomodoro(next.shortcuts.toggle_pomodoro.clone().into());
                        w.set_shortcut_prayer(next.shortcuts.toggle_prayer.clone().into());
                        w.set_settings_save_status("Tersimpan. Shortcut baru langsung aktif.".into());
                    }
                }
                Err(error) => {
                    if let Some(w) = weak.upgrade() {
                        w.set_settings_save_status(format!("Gagal menyimpan: {error}").into());
                    }
                }
            }
        });
    }

    {
        let weak = win.as_weak();
        win.on_settings_save_autostart(move |enabled| {
            match autostart::set(enabled) {
                Ok(actual) => {
                    if let Some(w) = weak.upgrade() {
                        w.set_autostart_enabled(actual);
                        w.set_autostart_save_status(if actual {
                            "Notch akan berjalan otomatis saat login.".into()
                        } else {
                            "Jalan otomatis dinonaktifkan.".into()
                        });
                    }
                }
                Err(error) => {
                    if let Some(w) = weak.upgrade() {
                        w.set_autostart_enabled(autostart::is_enabled());
                        w.set_autostart_save_status(format!("Gagal mengubah jalan otomatis: {error}").into());
                    }
                }
            }
        });
    }

    win.set_emoji_coffee(load_png(include_bytes!("../assets/emoji/2615.png")));
    win.set_emoji_tomato(load_png(include_bytes!("../assets/emoji/1f345.png")));
    win.set_emoji_moon(load_png(include_bytes!("../assets/emoji/1f319.png")));

    // --- State halaman ---
    let app_started_at = Instant::now();
    let stats_timer = Rc::new(Timer::default());
    let battery_timer = Rc::new(Timer::default());
    let clipboard_timer = Rc::new(Timer::default());
    let reminder_timer = Timer::default();
    // Aksen otomatis aktif saat nilai accent kosong; nilai manual selalu dipertahankan.
    media::start(win.as_weak(), col(palette.accent), col(palette.on_accent));
    let prayer_timer = Timer::default();
    let meeting_timer = Timer::default();
    // Indikator gelombang audio Adzan hanya bergerak selama playback aktif.
    let adhan_anim_timer = Timer::default();
    {
        let weak = win.as_weak();
        adhan_anim_timer.start(TimerMode::Repeated, Duration::from_millis(130), move || {
            if let Some(w) = weak.upgrade() {
                if w.get_adhan_active() {
                    w.set_adhan_phase((w.get_adhan_phase() + 1) % 3);
                }
            }
        });
    }
    // Durasi rapat dihitung dari transisi privasi aktif -> nonaktif.
    {
        let weak = win.as_weak();
        let started = Rc::new(Cell::new(None::<Instant>));
        let finished_at = Rc::new(Cell::new(None::<Instant>));
        meeting_timer.start(TimerMode::Repeated, Duration::from_secs(1), move || {
            let Some(w) = weak.upgrade() else { return };
            if w.get_meeting_active() {
                if started.get().is_none() {
                    started.set(Some(Instant::now()));
                    w.set_meeting_finished("".into());
                    finished_at.set(None);
                }
                if let Some(at) = started.get() {
                    let secs = at.elapsed().as_secs();
                    let label = if secs >= 3600 {
                        format!("{:02}:{:02}:{:02}", secs / 3600, (secs % 3600) / 60, secs % 60)
                    } else {
                        format!("{:02}:{:02}", secs / 60, secs % 60)
                    };
                    w.set_meeting_elapsed(label.into());
                }
            } else if let Some(at) = started.take() {
                let secs = at.elapsed().as_secs();
                let mins = secs / 60;
                let rem = secs % 60;
                let label = if mins == 0 { format!("{secs} detik") } else if rem == 0 { format!("{mins} menit") } else { format!("{mins} menit {rem} detik") };
                let finished_label = format!("Rapat selesai, {label}");
                w.set_prayer_status(finished_label.clone().into());
                w.set_meeting_finished(finished_label.into());
                w.set_meeting_elapsed("00:00".into());
                finished_at.set(Some(Instant::now()));
            } else if let Some(at) = finished_at.get() {
                if at.elapsed() >= Duration::from_secs(6) {
                    w.set_meeting_finished("".into());
                    finished_at.set(None);
                }
            }
        });
    }
    let weather_timer = Rc::new(Timer::default());
    let monitor: Rc<RefCell<Option<StatsMonitor>>> = Rc::new(RefCell::new(None));
    let battery = Rc::new(BatteryMonitor::new());
    let notes_path = notch_core::paths::data_file("notes.json");
    let notes_store: Rc<RefCell<Option<NotesStore>>> = Rc::new(RefCell::new(None));
    let notes_model = Rc::new(VecModel::<NoteItem>::default());
    win.set_notes(ModelRc::from(notes_model.clone()));

    // ----- Currency -----
    let currency_path = notch_core::paths::data_file("currency_alerts.json");
    let currency = Rc::new(RefCell::new(CurrencyState::new(notch_core::currency::load_alerts(&currency_path))));
    let currency_fetch = Arc::new(Mutex::new(CurrencyFetch::default()));
    let currency_busy = Arc::new(AtomicBool::new(false));
    let stock_gainers = Rc::new(VecModel::<StockRow>::default());
    let stock_losers = Rc::new(VecModel::<StockRow>::default());
    let currency_alert_model = Rc::new(VecModel::<CurrencyAlertItem>::default());
    win.set_stock_gainers(ModelRc::from(stock_gainers.clone()));
    win.set_stock_losers(ModelRc::from(stock_losers.clone()));
    win.set_currency_alerts(ModelRc::from(currency_alert_model.clone()));
    paint_currency(&win, &currency.borrow(), &stock_gainers, &stock_losers, &currency_alert_model);
    // Kurs dan saham memiliki cache 10 menit masing-masing; kegagalan satu sumber
    // tidak memaksa data dari sumber lain untuk diunduh ulang.
    let currency_request: Rc<dyn Fn(bool, bool)> = {
        let (weak, state, fetch, busy) = (win.as_weak(), currency.clone(), currency_fetch.clone(), currency_busy.clone());
        let (gm, lm, am) = (stock_gainers.clone(), stock_losers.clone(), currency_alert_model.clone());
        Rc::new(move |force: bool, include_stocks: bool| {
            let (load_rates, load_stocks) = {
                let s = state.borrow();
                let fresh = |loaded: Option<Instant>| loaded.map(|t| t.elapsed() < Duration::from_secs(600)).unwrap_or(false);
                (!fresh(s.rates_loaded_at) || force, include_stocks && (!fresh(s.stocks_loaded_at) || force))
            };
            if !load_rates && !load_stocks {
                return;
            }
            if busy.swap(true, Ordering::SeqCst) {
                return;
            }
            state.borrow_mut().loading = true;
            if let Some(w) = weak.upgrade() {
                paint_currency(&w, &state.borrow(), &gm, &lm, &am);
            }
            let (weak, fetch, busy) = (weak.clone(), fetch.clone(), busy.clone());
            std::thread::spawn(move || {
                let rates = if load_rates { Some(notch_core::currency::fetch_rates()) } else { None };
                let stock_result = if load_stocks { Some(notch_core::currency::fetch_stocks()) } else { None };
                if let Ok(mut guard) = fetch.lock() {
                    guard.rates = rates;
                    guard.stocks = stock_result;
                }
                busy.store(false, Ordering::SeqCst);
                let _ = weak.upgrade_in_event_loop(|w| w.invoke_currency_data_ready());
            });
        })
    };
    {
        let (weak, state, fetch) = (win.as_weak(), currency.clone(), currency_fetch.clone());
        let (gm, lm, am, path) = (stock_gainers.clone(), stock_losers.clone(), currency_alert_model.clone(), currency_path.clone());
        win.on_currency_data_ready(move || {
            let Some(w) = weak.upgrade() else { return };
            let (rates, stocks) = match fetch.lock() {
                Ok(mut g) => (g.rates.take(), g.stocks.take()),
                Err(_) => (None, None),
            };
            let mut s = state.borrow_mut();
            s.loading = false;
            match rates {
                Some(Ok(r)) => {
                    let (h, mi, _, _, _) = platform::local_time();
                    s.rates = Some(r);
                    s.rates_error.clear();
                    s.rates_warning.clear();
                    s.rates_loaded_at = Some(Instant::now());
                    s.updated = format!("Diperbarui {h:02}:{mi:02}");
                }
                Some(Err(e)) => {
                    if s.rates.is_none() {
                        s.rates_error = format!("Kurs belum tersedia: {e}");
                    } else {
                        s.rates_warning = format!("Pembaruan gagal · memakai kurs terakhir ({e})");
                    }
                }
                None => {}
            }
            match stocks {
                Some(Ok(v)) => {
                    s.stocks = v;
                    s.stock_error.clear();
                    s.stocks_loaded_at = Some(Instant::now());
                }
                Some(Err(e)) => {
                    s.stocks_loaded_at = Some(Instant::now());
                    if s.stocks.is_empty() {
                        s.stock_error = e;
                    }
                }
                None => {}
            }
            fire_due_currency_alert(&w, &mut s, &path);
            paint_currency(&w, &s, &gm, &lm, &am);
        });
    }
    {
        let req = currency_request.clone();
        win.on_currency_refresh(move || req(true, true));
    }
    {
        let (weak, state) = (win.as_weak(), currency.clone());
        let (gm, lm, am) = (stock_gainers.clone(), stock_losers.clone(), currency_alert_model.clone());
        win.on_currency_amount_edited(move |text| {
            let Some(w) = weak.upgrade() else { return };
            let digits: String = text.chars().filter(char::is_ascii_digit).collect();
            let digits = digits.trim_start_matches('0');
            let mut s = state.borrow_mut();
            s.amount = digits.chars().take(15).collect();
            paint_currency(&w, &s, &gm, &lm, &am);
        });
    }
    {
        let (weak, state) = (win.as_weak(), currency.clone());
        let (gm, lm, am) = (stock_gainers.clone(), stock_losers.clone(), currency_alert_model.clone());
        win.on_currency_quick(move |i| {
            let Some(w) = weak.upgrade() else { return };
            let mut s = state.borrow_mut();
            s.amount = CURRENCY_QUICK.get(i.max(0) as usize).copied().unwrap_or(100_000).to_string();
            paint_currency(&w, &s, &gm, &lm, &am);
        });
    }
    {
        let (weak, state) = (win.as_weak(), currency.clone());
        let (gm, lm, am) = (stock_gainers.clone(), stock_losers.clone(), currency_alert_model.clone());
        win.on_currency_step_target(move |n| {
            let Some(w) = weak.upgrade() else { return };
            let mut s = state.borrow_mut();
            let len = notch_core::currency::CURRENCIES.len() as i32;
            s.target = ((s.target as i32 + n).rem_euclid(len)) as usize;
            paint_currency(&w, &s, &gm, &lm, &am);
        });
    }
    {
        let (weak, state) = (win.as_weak(), currency.clone());
        let (gm, lm, am) = (stock_gainers.clone(), stock_losers.clone(), currency_alert_model.clone());
        win.on_currency_select_target(move |code, days| {
            let Some(w) = weak.upgrade() else { return };
            let selected_code = code.to_string();
            {
                let mut s = state.borrow_mut();
                if let Some(index) = notch_core::currency::CURRENCIES.iter().position(|(candidate, _)| *candidate == selected_code.as_str()) {
                    s.target = index;
                    paint_currency(&w, &s, &gm, &lm, &am);
                }
            }
            if w.get_currency_expanded() {
                w.invoke_currency_chart_request(code, days);
            }
        });
    }
    let currency_chart_cache = Arc::new(Mutex::new(
        std::collections::HashMap::<String, (Instant, Vec<(i64, f64)>, String)>::new()
    ));
    {
        // Cache riwayat 10 menit per mata uang/periode; token mencegah respons lama menimpa pilihan terbaru.
        let token = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let cache = currency_chart_cache.clone();
        let weak = win.as_weak();
        win.on_currency_chart_request(move |code, days| {
            let Some(w) = weak.upgrade() else { return };
            w.set_fx_chart_line("".into());
            w.set_fx_chart_area("".into());
            w.set_fx_chart_points(ModelRc::from(Rc::new(VecModel::<FxChartPoint>::default())));
            w.set_fx_chart_last("".into());
            w.set_fx_chart_change("".into());
            w.set_fx_chart_max("".into());
            w.set_fx_chart_y1("".into());
            w.set_fx_chart_y2("".into());
            w.set_fx_chart_min("".into());
            w.set_fx_chart_date_start("".into());
            w.set_fx_chart_date_mid("".into());
            w.set_fx_chart_date_end("".into());
            w.set_fx_chart_dot_y(50.0);
            w.set_fx_chart_status("Memuat grafik…".into());
            let mine = token.fetch_add(1, Ordering::SeqCst) + 1;
            let (token, weak, cache, code, days) = (
                token.clone(), w.as_weak(), cache.clone(), code.to_string(), i64::from(days.clamp(7, 730))
            );
            std::thread::spawn(move || {
                let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64).unwrap_or(0);
                let cache_key = format!("{code}:{days}");
                let cached = cache.lock().ok().and_then(|entries| {
                    entries.get(&cache_key)
                        .filter(|(loaded_at, _, _)| loaded_at.elapsed() < Duration::from_secs(600))
                        .cloned()
                });
                let history = match cached {
                    Some((_, points, source)) => Ok((points, source)),
                    None => notch_core::currency::fetch_fx_history_with_source(
                        &code, now - days * 86_400, now + 86_400
                    ).map(|(points, source)| {
                        let source = source.to_string();
                        if let Ok(mut entries) = cache.lock() {
                            entries.insert(cache_key, (Instant::now(), points.clone(), source.clone()));
                        }
                        (points, source)
                    }),
                };
                let result = history.and_then(|(pts, source)| {
                    if pts.len() < 2 { return Err("Riwayat kosong".to_string()); }
                    let count = pts.len();
                    let first_time = pts[0].0;
                    let middle_time = pts[pts.len() / 2].0;
                    let last_time = pts[pts.len() - 1].0;
                    let chart = notch_core::currency::build_fx_chart(&pts)
                        .ok_or_else(|| "Riwayat kosong".to_string())?;
                    let span = chart.max - chart.min;
                    let pad = if span.abs() <= f64::EPSILON { chart.max.abs() * 0.01 } else { span * 0.14 };
                    let axis_min = chart.min - pad;
                    let axis_max = chart.max + pad;
                    let axis_span = (axis_max - axis_min).max(f64::EPSILON);
                    let axis = [
                        fx_axis_label(axis_max, axis_span),
                        fx_axis_label(axis_min + axis_span * 2.0 / 3.0, axis_span),
                        fx_axis_label(axis_min + axis_span / 3.0, axis_span),
                        fx_axis_label(axis_min, axis_span),
                    ];
                    let dot_y = if span.abs() <= f64::EPSILON {
                        50.0
                    } else {
                        94.0 - (chart.last - axis_min) / axis_span * 88.0
                    };
                    let dates = [
                        notch_core::currency::short_fx_date_from_unix(first_time),
                        notch_core::currency::short_fx_date_from_unix(middle_time),
                        notch_core::currency::short_fx_date_from_unix(last_time),
                    ];
                    let t0 = first_time as f64;
                    let t1 = last_time as f64;
                    let time_span = (t1 - t0).max(1.0);
                    let chart_points = pts.iter().enumerate().map(|(index, (timestamp, value))| {
                        let x = (*timestamp as f64 - t0) / time_span;
                        let left = if index == 0 {
                            0.0
                        } else {
                            ((pts[index - 1].0 as f64 + *timestamp as f64) * 0.5 - t0) / time_span
                        };
                        let right = if index + 1 == pts.len() {
                            1.0
                        } else {
                            ((*timestamp as f64 + pts[index + 1].0 as f64) * 0.5 - t0) / time_span
                        };
                        let y = if span.abs() <= f64::EPSILON {
                            50.0
                        } else {
                            94.0 - (*value - axis_min) / axis_span * 88.0
                        };
                        FxChartPoint {
                            x: x as f32,
                            y: y as f32,
                            left: left as f32,
                            right: right as f32,
                            date: notch_core::currency::full_fx_date_from_unix(*timestamp).into(),
                            value: fx_idr(*value).into(),
                        }
                    }).collect::<Vec<_>>();
                    Ok((chart, count, axis, dates, dot_y, chart_points, source))
                });
                if token.load(Ordering::SeqCst) != mine { return; }
                let _ = weak.upgrade_in_event_loop(move |w| match result {
                    Ok((chart, count, axis, dates, dot_y, chart_points, source)) => {
                        let pct = if chart.first > 0.0 {
                            (chart.last - chart.first) / chart.first * 100.0
                        } else { 0.0 };
                        let period = match days {
                            30 => "30 hari", 90 => "3 bulan", 180 => "6 bulan", 365 => "1 tahun",
                            _ => "periode dipilih",
                        };
                        w.set_fx_chart_line(chart.line.into());
                        w.set_fx_chart_area(chart.area.into());
                        w.set_fx_chart_points(ModelRc::from(Rc::new(VecModel::from(chart_points))));
                        w.set_fx_chart_status(format!("Kurs penutupan harian · {count} titik data · sumber {source}").into());
                        w.set_fx_chart_max(axis[0].clone().into());
                        w.set_fx_chart_y1(axis[1].clone().into());
                        w.set_fx_chart_y2(axis[2].clone().into());
                        w.set_fx_chart_min(axis[3].clone().into());
                        w.set_fx_chart_date_start(dates[0].clone().into());
                        w.set_fx_chart_date_mid(dates[1].clone().into());
                        w.set_fx_chart_date_end(dates[2].clone().into());
                        w.set_fx_chart_dot_y(dot_y as f32);
                        w.set_fx_chart_last(fx_idr(chart.last).into());
                        w.set_fx_chart_change(format!(
                            "{}{:.2}% · {period}",
                            if pct >= 0.0 { "+" } else { "" }, pct
                        ).replace('.', ",").into());
                        w.set_fx_chart_up(pct >= 0.0);
                    }
                    Err(error) => {
                        w.set_fx_chart_line("".into());
                        w.set_fx_chart_area("".into());
                        w.set_fx_chart_points(ModelRc::from(Rc::new(VecModel::<FxChartPoint>::default())));
                        w.set_fx_chart_last("".into());
                        w.set_fx_chart_change("".into());
                        w.set_fx_chart_max("".into());
                        w.set_fx_chart_y1("".into());
                        w.set_fx_chart_y2("".into());
                        w.set_fx_chart_min("".into());
                        w.set_fx_chart_date_start("".into());
                        w.set_fx_chart_date_mid("".into());
                        w.set_fx_chart_date_end("".into());
                        w.set_fx_chart_dot_y(50.0);
                        w.set_fx_chart_status(format!("Grafik {code} belum tersedia ({error})").into());
                    }
                });
            });
        });
    }
    {
        let (weak, state, path, req) = (win.as_weak(), currency.clone(), currency_path.clone(), currency_request.clone());
        let (gm, lm, am) = (stock_gainers.clone(), stock_losers.clone(), currency_alert_model.clone());
        win.on_currency_add_alert(move |above, text, is_stock, code, repeat| {
            let Some(w) = weak.upgrade() else { return };
            let digits: String = text.chars().filter(char::is_ascii_digit).collect();
            let Ok(target) = digits.parse::<f64>() else { return };
            if target <= 0.0 {
                return;
            }
            let code = code.to_string();
            let kind = if is_stock {
                notch_core::currency::AlertKind::Stock
            } else {
                notch_core::currency::AlertKind::Currency
            };
            let valid_code = match kind {
                notch_core::currency::AlertKind::Stock => notch_core::currency::is_allowed_stock_code(&code),
                notch_core::currency::AlertKind::Currency => notch_core::currency::CURRENCIES.iter().any(|(item, _)| *item == code),
            };
            if !valid_code {
                return;
            }
            let mut s = state.borrow_mut();
            if s.alerts.iter().any(|item| item.code == code && item.kind == kind && item.above == above && item.target == target && item.repeat == repeat) {
                return;
            }
            let id = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis().to_string())
                .unwrap_or_default();
            s.alerts.push(notch_core::currency::Alert {
                id,
                code,
                kind,
                above,
                target,
                repeat,
                armed: true,
            });
            s.alerts.truncate(20);
            notch_core::currency::save_alerts(&path, &s.alerts);
            paint_currency(&w, &s, &gm, &lm, &am);
            drop(s);
            // Saat memasang alert saham, pastikan snapshot harga seluruh saham IDX segera diminta.
            if is_stock {
                req(false, true);
            }
        });
    }
    {
        let (weak, state, path) = (win.as_weak(), currency.clone(), currency_path.clone());
        let (gm, lm, am) = (stock_gainers.clone(), stock_losers.clone(), currency_alert_model.clone());
        win.on_currency_remove_alert(move |id| {
            let Some(w) = weak.upgrade() else { return };
            let mut s = state.borrow_mut();
            s.alerts.retain(|a| a.id != id.as_str());
            notch_core::currency::save_alerts(&path, &s.alerts);
            paint_currency(&w, &s, &gm, &lm, &am);
        });
    }
    // Pengingat kurs dan saham berjalan walau halaman tidak dibuka: cek cache tiap 30 dtk, ambil data baru tiap 10 menit.
    let currency_alert_timer = Timer::default();
    {
        let (weak, state, req, path) = (win.as_weak(), currency.clone(), currency_request.clone(), currency_path.clone());
        let (gm, lm, am) = (stock_gainers.clone(), stock_losers.clone(), currency_alert_model.clone());
        let ticks = Cell::new(0u32);
        currency_alert_timer.start(TimerMode::Repeated, Duration::from_secs(30), move || {
            let Some(w) = weak.upgrade() else { return };
            let mut s = state.borrow_mut();
            if s.alerts.is_empty() {
                return;
            }
            ticks.set(ticks.get() + 1);
            let has_stock_alert = s.alerts.iter().any(|alert| alert.kind == notch_core::currency::AlertKind::Stock);
            let stocks_stale = has_stock_alert && s.stocks_loaded_at
                .map(|loaded| loaded.elapsed() >= Duration::from_secs(600))
                .unwrap_or(true);
            if s.rates.is_none() || ticks.get() % 20 == 0 || stocks_stale {
                drop(s);
                req(false, has_stock_alert);
                return;
            }
            if fire_due_currency_alert(&w, &mut s, &path) {
                paint_currency(&w, &s, &gm, &lm, &am);
            }
        });
    }
    let shelf_store = Rc::new(RefCell::new(shelf::Store::open(notch_core::paths::data_file("shelf.json"))));
    let shelf_model = Rc::new(VecModel::<ShelfItem>::default());
    let shelf_items = shelf_store.borrow().list();
    shelf_model.set_vec(shelf_items.into_iter().map(|item| ShelfItem {
        path: item.path.into(), name: item.name.into(), dir: item.dir,
    }).collect::<Vec<_>>());
    win.set_shelf_items(ModelRc::from(shelf_model.clone()));
    let clipboard_monitor = Rc::new(if pages.iter().any(|page| page == "clipboard") {
        match ClipboardMonitor::start() {
            Ok(monitor) => Some(monitor),
            Err(error) => {
                eprintln!("[Clipboard] {error}");
                None
            }
        }
    } else {
        None
    });
    let clipboard_model = Rc::new(VecModel::<ClipboardItem>::default());
    win.set_clipboard_items(ModelRc::from(clipboard_model.clone()));
    win.set_clipboard_available(clipboard_monitor.is_some());
    show_clipboard(&clipboard_model, clipboard_monitor.as_ref().as_ref());
    let reminder_model = Rc::new(VecModel::<ReminderItem>::default());
    win.set_reminders(ModelRc::from(reminder_model.clone()));
    let timezone_offset = local_timezone_offset_minutes();
    let weekday_mask = Rc::new(Cell::new(62u8)); // Senin-Jumat, sama dengan pilihan awal Tauri
    win.set_reminder_weekdays_mask(62);
    win.set_reminder_weekday_selected(ModelRc::from(Rc::new(VecModel::from(weekday_selection(62)))));
    show_reminders(&reminder_model, reminder::list(), timezone_offset);
    let active_reminder = Rc::new(RefCell::new(None::<String>));
    let prayer_prefs=Rc::new(RefCell::new(prayer::preferences()));
    win.set_prayer_city(prayer_prefs.borrow().kabkota.clone().into());win.set_prayer_province(prayer_prefs.borrow().provinsi.clone().into());
    win.set_prayer_mode(prayer_prefs.borrow().mode.clone().into());win.set_prayer_remind_before(if prayer_prefs.borrow().remind_before { "true" } else { "false" }.into());win.set_prayer_remind_min(prayer_prefs.borrow().remind_min.to_string().into());
    let ramadan=Rc::new(Cell::new(load_ramadan()));win.set_ramadan_enabled(ramadan.get());
    let dhikr_idx=Rc::new(Cell::new(0usize));
    let dhikr=[("أَسْتَغْفِرُ اللّٰهَ","Astaghfirullah","Aku memohon ampun kepada Allah. Dibaca 3x"),("اللّٰهُمَّ رَبَّ هٰذِهِ الدَّعْوَةَ التَّامَّةَ","Allahumma rabba hadzihid-da'watit-tammah","Doa setelah adzan"),("سُبْحَانَ اللّٰهِ وَبِحَمْدِهِ","Subhanallahi wa bihamdihi","Maha Suci Allah dan segala puji bagi-Nya. Dibaca 33x")];
    let set_dhikr:Rc<dyn Fn(&NotchWindow,usize)>=Rc::new(move|w,i|{let x=dhikr[i%3];w.set_dhikr_arabic(x.0.into());w.set_dhikr_latin(x.1.into());w.set_dhikr_meaning(x.2.into());w.set_dhikr_idx((i%3) as i32);});set_dhikr(&win,0);
    let prayer_busy=Arc::new(AtomicBool::new(false));let prayer_data=Arc::new(Mutex::new(PrayerSchedule::default()));
    let prayer_fired=Rc::new(RefCell::new(std::collections::HashSet::<String>::new()));
    let weather_busy = Arc::new(AtomicBool::new(false));
    let weather_location = Rc::new(RefCell::new((cfg.weather.lat, cfg.weather.lon, cfg.weather.city.clone())));
    win.set_weather_city(cfg.weather.city.clone().into());
    win.set_weather_lat(cfg.weather.lat.to_string().into());
    win.set_weather_lon(cfg.weather.lon.to_string().into());
    win.set_weather_status("Cuaca dimuat saat halaman dibuka".into());
    {
        let weak = win.as_weak();
        let busy = weather_busy.clone();
        let location = weather_location.clone();
        win.on_weather_refresh(move || {
            let (lat, lon, city) = location.borrow().clone();
            refresh_weather(weak.clone(), busy.clone(), lat, lon, city);
        });
    }
    let quick_busy = Arc::new(AtomicBool::new(false));
    let keep_awake = Arc::new(AtomicBool::new(false));

    // Setiap halaman hanya bekerja saat notch melebar DAN halamannya terlihat (seperti versi Tauri):
    // Stats disampel tiap 1 dtk, Baterai dibaca tiap 5 dtk, Quick/Tools dibaca ulang saat dibuka,
    // Catatan dimuat ulang dari disk setiap dibuka (perubahan dari versi Tauri ikut terbaca).
    let sync_pages: Rc<dyn Fn()> = {
        let weak = win.as_weak();
        let (stats_timer, battery_timer, clipboard_timer, weather_timer) = (stats_timer.clone(), battery_timer.clone(), clipboard_timer.clone(), weather_timer.clone());
        let (monitor, battery) = (monitor.clone(), battery.clone());
        let (notes_store, notes_model, notes_path) =
            (notes_store.clone(), notes_model.clone(), notes_path.clone());
        let (clipboard_model, clipboard_monitor) = (clipboard_model.clone(), clipboard_monitor.clone());
        let currency_request = currency_request.clone();
        let weather_busy = weather_busy.clone();
        let weather_location = weather_location.clone();
        let quick_busy = quick_busy.clone();
        let keep_awake = keep_awake.clone();
        Rc::new(move || {
            let Some(w) = weak.upgrade() else { return };
            let visible = w.get_expanded();
            let page = w.get_page_id();
            let page = page.as_str();

            // Stats
            if visible && page == "stats" {
                if !stats_timer.running() {
                    monitor.borrow_mut().get_or_insert_with(StatsMonitor::new).resume();
                    let weak_s = weak.clone();
                    let monitor = monitor.clone();
                    stats_timer.start(TimerMode::Repeated, Duration::from_secs(1), move || {
                        let Some(w) = weak_s.upgrade() else { return };
                        if let Some(m) = monitor.borrow_mut().as_mut() {
                            let s = m.sample();
                            w.set_cpu(s.cpu);
                            w.set_ram(s.ram);
                            w.set_disk(s.disk);
                            w.set_cpu_text(format!("{:.0}%", s.cpu).into());
                            w.set_ram_text(format!("{:.1} / {:.1} GiB", gib(s.ram_used), gib(s.ram_total)).into());
                            w.set_disk_text(format!("{:.0} / {:.0} GiB", gib(s.disk_used), gib(s.disk_total)).into());
                            w.set_net_text(format!("\u{2193} {}   \u{2191} {}", fmt_rate(s.down), fmt_rate(s.up)).into());
                            w.set_sys_text(format!("{} threads \u{00B7} {} proses", s.cpu_cores, s.processes).into());
                            // Tampilan halaman Stats (sama dengan quick-stats.ts di Tauri)
                            let freq = if s.cpu_freq_mhz > 0 { format!(" \u{00B7} {:.1} GHz", s.cpu_freq_mhz as f64 / 1000.0) } else { String::new() };
                            w.set_stats(StatsView {
                                host: s.host_name.into(),
                                cpu: s.cpu, ram: s.ram, swap: s.swap, disk: s.disk,
                                cpu_v: format!("{:.0}%", s.cpu).into(),
                                ram_v: format!("{:.0}%", s.ram).into(),
                                swap_v: format!("{:.0}%", s.swap).into(),
                                disk_v: format!("{:.0}%", s.disk).into(),
                                cpu_meta: format!("{} \u{00B7} {} thread{}", s.cpu_name, s.cpu_cores, freq).into(),
                                ram_meta: format!("{:.1} / {:.1} GiB digunakan", gib(s.ram_used), gib(s.ram_total)).into(),
                                swap_meta: format!("{:.1} / {:.1} GiB digunakan", gib(s.swap_used), gib(s.swap_total)).into(),
                                disk_meta: if s.disk_total > 0.0 {
                                    format!("{:.0} / {:.0} GiB \u{00B7} {}", gib(s.disk_used), gib(s.disk_total), s.disk_label).into()
                                } else { "Penyimpanan N/A".into() },
                                down: format!("\u{2193} {}", fmt_rate(s.down)).into(),
                                up: format!("\u{2191} {}", fmt_rate(s.up)).into(),
                                processes: format!("{} proses", s.processes).into(),
                                uptime: format!("Aktif {}", fmt_uptime(s.uptime_secs)).into(),
                                os: format!("{} {}", s.os_name, s.os_version).trim().to_string().into(),
                                kernel: s.kernel_version.into(),
                            });
                        }
                    });
                }
            } else {
                stats_timer.stop();
            }

            // Baterai
            if visible && page == "battery" {
                if !battery_timer.running() {
                    apply_battery(&w, battery.read());
                    let weak_b = weak.clone();
                    let battery = battery.clone();
                    battery_timer.start(TimerMode::Repeated, Duration::from_secs(5), move || {
                        if let Some(w) = weak_b.upgrade() {
                            apply_battery(&w, battery.read());
                        }
                    });
                }
            } else {
                battery_timer.stop();
            }

            // Quick / Tools
            if visible && (page == "quick" || page == "tools") {
                refresh_quick(weak.clone(), quick_busy.clone(), keep_awake.clone());
            }

            // Catatan
            if visible && page == "notes" {
                let store = NotesStore::open(&notes_path);
                show_notes(&notes_model, store.list());
                *notes_store.borrow_mut() = Some(store);
            }

            // Clipboard: polling UI hanya berjalan saat halaman terlihat.
            if visible && page == "clipboard" {
                if !clipboard_timer.running() {
                    show_clipboard(&clipboard_model, clipboard_monitor.as_ref().as_ref());
                    let model = clipboard_model.clone();
                    let monitor = clipboard_monitor.clone();
                    clipboard_timer.start(TimerMode::Repeated, Duration::from_millis(500), move || {
                        show_clipboard(&model, monitor.as_ref().as_ref());
                    });
                }
            } else {
                clipboard_timer.stop();
            }

            // Currency: muat kurs + saham saat halaman dibuka (cache 10 menit).
            if visible && page == "currency" {
                currency_request(false, true);
            }

            // Cuaca: ambil data saat halaman terlihat, lalu perbarui tiap lima menit.
            if visible && page == "weather" {
                if !weather_timer.running() {
                    let (lat, lon, city) = weather_location.borrow().clone();
                    refresh_weather(weak.clone(), weather_busy.clone(), lat, lon, city.clone());
                    let weak_w = weak.clone();
                    let busy_w = weather_busy.clone();
                    weather_timer.start(TimerMode::Repeated, Duration::from_secs(5 * 60), move || {
                        refresh_weather(weak_w.clone(), busy_w.clone(), lat, lon, city.clone());
                    });
                }
            } else {
                weather_timer.stop();
            }
        })
    };

    {
        let sync = sync_pages.clone();
        let (weak_pc, shell_pc) = (win.as_weak(), shell.clone());
        win.on_page_changed(move |_| {
            sync();
            if let Some(w) = weak_pc.upgrade() {
                if w.get_expanded() {
                    shell_pc.place(&w, expanded_size(&w));
                }
            }
        });
        let (weak_nx, shell_nx) = (win.as_weak(), shell.clone());
        win.on_notes_expand_toggled(move |_| {
            if let Some(w) = weak_nx.upgrade() {
                if w.get_expanded() {
                    shell_nx.place(&w, expanded_size(&w));
                }
            }
        });
    }
    {
        let (store, model, path) = (notes_store.clone(), notes_model.clone(), notes_path.clone());
        win.on_add_note(move |text| {
            let mut guard = store.borrow_mut();
            let s = guard.get_or_insert_with(|| NotesStore::open(&path));
            show_notes(&model, s.add(text.as_str()));
        });
    }
    let note_details: Rc<RefCell<std::collections::HashMap<String, NoteDetailWindow>>> = Rc::default();
    {
        let (weak, store, model, path, details) =
            (win.as_weak(), notes_store.clone(), notes_model.clone(), notes_path.clone(), note_details.clone());
        win.on_open_note(move |id| {
            let accent = weak.upgrade().map(|w| Theme::get(&w).get_accent()).unwrap_or(slint::Color::from_rgb_u8(59, 130, 246));
            open_note_detail(id.as_str(), &store, &path, &model, &details, accent);
        });
    }
    {
        let (store, model, path) = (notes_store.clone(), notes_model.clone(), notes_path.clone());
        win.on_clear_notes(move || {
            let mut guard = store.borrow_mut();
            let s = guard.get_or_insert_with(|| NotesStore::open(&path));
            show_notes(&model, s.clear());
        });
    }
    {
        let (store, model, path) = (notes_store.clone(), notes_model.clone(), notes_path.clone());
        win.on_save_note(move |id, text| {
            let mut guard = store.borrow_mut();
            let s = guard.get_or_insert_with(|| NotesStore::open(&path));
            show_notes(&model, s.update(id.as_str(), text.as_str()));
        });
    }
    {
        let (store, model, path) = (notes_store.clone(), notes_model.clone(), notes_path.clone());
        win.on_remove_note(move |id| {
            let mut guard = store.borrow_mut();
            let s = guard.get_or_insert_with(|| NotesStore::open(&path));
            show_notes(&model, s.remove(id.as_str()));
        });
    }
    {
        let store = shelf_store.clone();
        let model = shelf_model.clone();
        let weak = win.as_weak();
        win.on_shelf_add(move || {
            let selected = rfd::FileDialog::new().pick_files();
            let Some(paths) = selected else { return; };
            let paths = paths.into_iter().map(|path| path.to_string_lossy().to_string()).collect();
            let items = store.borrow_mut().add(paths);
            model.set_vec(items.into_iter().map(|item| ShelfItem {
                path: item.path.into(), name: item.name.into(), dir: item.dir,
            }).collect::<Vec<_>>());
            if let Some(w) = weak.upgrade() {
                w.set_shelf_status(format!("{} item tersimpan. File asli tidak dipindahkan.", model.row_count()).into());
            }
        });
    }
    {
        let store = shelf_store.clone();
        let model = shelf_model.clone();
        let weak = win.as_weak();
        win.on_shelf_remove(move |path| {
            let items = store.borrow_mut().remove(path.as_str());
            model.set_vec(items.into_iter().map(|item| ShelfItem {
                path: item.path.into(), name: item.name.into(), dir: item.dir,
            }).collect::<Vec<_>>());
            if let Some(w) = weak.upgrade() { w.set_shelf_status(format!("Item dihapus dari Shelf · {} tersisa", model.row_count()).into()); }
        });
    }
    {
        let store = shelf_store.clone();
        let model = shelf_model.clone();
        let weak = win.as_weak();
        win.on_shelf_clear(move || {
            let items = store.borrow_mut().clear();
            model.set_vec(items.into_iter().map(|item| ShelfItem {
                path: item.path.into(), name: item.name.into(), dir: item.dir,
            }).collect::<Vec<_>>());
            if let Some(w) = weak.upgrade() { w.set_shelf_status("Shelf dikosongkan. File asli tetap aman.".into()); }
        });
    }
    win.on_shelf_open(|path| shelf::open(path.as_str()));
    win.on_shelf_reveal(|path| shelf::reveal(path.as_str()));

    {
      let weak=win.as_weak();let prefs=prayer_prefs.clone();
      win.on_media_control(|action| media::control(&action));
    win.on_adhan_stop(|| media::stop_adhan());
    {
      let enabled=ramadan.clone();
      let idx=dhikr_idx.clone();
      let set=set_dhikr.clone();
      let weak=win.as_weak();
      win.on_adhan_finished(move||{
        if !enabled.get(){return;}
        let i=1;
        idx.set(i);
        if let Some(w)=weak.upgrade(){set(&w,i);}
      });
    }
    win.on_media_seek(|position| media::seek(f64::from(position)));
      win.on_prayer_set_mode(move|v|{let mut p=prefs.borrow_mut();p.mode=v.to_string();if let Err(e)=prayer::save_preferences(&p.mode,p.remind_before,p.remind_min){eprintln!("[Prayer] {e}");}if let Some(w)=weak.upgrade(){w.set_prayer_mode(p.mode.clone().into());}});
    }
    {
      let weak=win.as_weak();let prefs=prayer_prefs.clone();
      win.on_prayer_set_remind(move|v|{let mut p=prefs.borrow_mut();p.remind_before=v;if let Err(e)=prayer::save_preferences(&p.mode,p.remind_before,p.remind_min){eprintln!("[Prayer] {e}");}if let Some(w)=weak.upgrade(){w.set_prayer_remind_before(if v { "true" } else { "false" }.into());}});
    }
    {
      let weak=win.as_weak();let prefs=prayer_prefs.clone();
      win.on_prayer_set_remind_min(move|v|{let mut p=prefs.borrow_mut();p.remind_min=if v>=5&&p.remind_min as i32+v>60{5}else{(p.remind_min as i32+v).clamp(1,60)as u32};if let Err(e)=prayer::save_preferences(&p.mode,p.remind_before,p.remind_min){eprintln!("[Prayer] {e}");}if let Some(w)=weak.upgrade(){w.set_prayer_remind_min(p.remind_min.to_string().into());}});
    }
    {
      let weak=win.as_weak();let enabled=ramadan.clone();let shell_r=shell.clone();
      win.on_prayer_toggle_ramadan(move||{let v=!enabled.get();enabled.set(v);let path=notch_core::paths::data_file("ramadan.json");if let Some(d)=path.parent(){let _=std::fs::create_dir_all(d);}let _=std::fs::write(path,if v{"true"}else{"false"});if let Some(w)=weak.upgrade(){w.set_ramadan_enabled(v);if w.get_expanded(){shell_r.place(&w,expanded_size(&w));}}});
    }
    {
      let weak=win.as_weak();let idx=dhikr_idx.clone();let set=set_dhikr.clone();
      let (weak_p,idx_p,set_p)=(weak.clone(),idx.clone(),set.clone());
      win.on_prayer_dhikr_prev(move||{let i=(idx_p.get()+2)%3;idx_p.set(i);if let Some(w)=weak_p.upgrade(){set_p(&w,i);}});
      win.on_prayer_dhikr_next(move||{let i=(idx.get()+1)%3;idx.set(i);if let Some(w)=weak.upgrade(){set(&w,i);}});
    }

    {
        let model = reminder_model.clone();
        let weak = win.as_weak();
        win.on_reminder_add(move |text, date_time, repeat_mode, weekdays_mask| {
            let text = text.to_string();
            let date_time = date_time.to_string();
            let mode = repeat_mode.to_string();
            if text.trim().is_empty() {
                if let Some(w) = weak.upgrade() { w.set_reminder_status("Isi deskripsi pengingat terlebih dahulu.".into()); }
                return;
            }
            if mode != "none" && date_time.trim().is_empty() {
                if let Some(w) = weak.upgrade() {
                    w.set_reminder_status("Pengulangan memerlukan tanggal dan waktu.".into());
                }
                return;
            }
            let due_at = if date_time.trim().is_empty() {
                0
            } else {
                match reminder::parse_local_datetime(&date_time, timezone_offset) {
                    Some(value) => value,
                    None => {
                        if let Some(w) = weak.upgrade() { w.set_reminder_status("Format tanggal/waktu harus DD/MM/YYYY HH:MM.".into()); }
                        return;
                    }
                }
            };
            if mode == "weekly" && (weekdays_mask & 0x7f) == 0 {
                if let Some(w) = weak.upgrade() { w.set_reminder_status("Pilih minimal satu hari untuk pengulangan mingguan.".into()); }
                return;
            }
            let items = reminder::add(
                text, due_at, mode, timezone_offset,
                Some((weekdays_mask as u8) & 0x7f),
            );
            show_reminders(&model, items, timezone_offset);
            if let Some(w) = weak.upgrade() {
                w.set_reminder_status("Pengingat berhasil ditambahkan.".into());
                w.set_reminder_draft("".into());
                w.set_reminder_date_time("".into());
                w.set_reminder_repeat_mode("none".into());
            }
        });
    }
    {
        let model = reminder_model.clone();
        win.on_reminder_toggle(move |id| {
            show_reminders(&model, reminder::toggle(id.to_string()), timezone_offset);
        });
    }
    {
        let model = reminder_model.clone();
        win.on_reminder_remove(move |id| {
            show_reminders(&model, reminder::remove(id.to_string()), timezone_offset);
        });
    }
    {
        let model = reminder_model.clone();
        win.on_reminder_clear_done(move || {
            show_reminders(&model, reminder::clear_done(), timezone_offset);
        });
    }
    {
        let mask = weekday_mask.clone();
        let weak = win.as_weak();
        win.on_reminder_toggle_weekday(move |day| {
            if (0..=6).contains(&day) {
                let next = mask.get() ^ (1 << day);
                mask.set(next);
                if let Some(w) = weak.upgrade() {
                    w.set_reminder_weekdays_mask(i32::from(next));
                    w.set_reminder_weekday_selected(ModelRc::from(Rc::new(VecModel::from(weekday_selection(next)))));
                }
            }
        });
    }

    {
        let monitor = clipboard_monitor.clone();
        win.on_clipboard_copy(move |text| {
            if let Some(monitor) = monitor.as_ref() {
                monitor.copy(text.to_string());
            }
        });
    }
    {
        let monitor = clipboard_monitor.clone();
        let model = clipboard_model.clone();
        win.on_clipboard_clear(move || {
            if let Some(monitor) = monitor.as_ref() {
                monitor.clear();
            }
            show_clipboard(&model, monitor.as_ref().as_ref());
        });
    }

    // --- Reminder: data and recurrence rules use the same JSON schema as Tauri ---
    {
        let weak = win.as_weak();
        let shell = shell.clone();
        let model = reminder_model.clone();
        let active = active_reminder.clone();
        reminder_timer.start(TimerMode::Repeated, Duration::from_secs(5), move || {
            let items = reminder::list();
            if let Some(w) = weak.upgrade() {
                if w.get_page_id() == "reminder" {
                    show_reminders(&model, items.clone(), timezone_offset);
                }
                let due = reminder::due_now();
                let active_id = active.borrow().clone();
                let active_due = active_id.as_ref().and_then(|id| due.iter().find(|r| &r.id == id));
                if active_id.is_some() && active_due.is_none() {
                    *active.borrow_mut() = None;
                    w.set_reminder_alert_visible(false);
                }
                // Jangan menimpa rapat, adzan, atau interaksi hub yang sedang berlangsung.
                // Reminder tetap berstatus due dan akan ditampilkan pada siklus timer berikutnya.
                let can_interrupt = !w.get_expanded()
                    && !w.get_meeting_active()
                    && w.get_meeting_finished().is_empty()
                    && !w.get_adhan_active();
                if active.borrow().is_none() && can_interrupt {
                    if let Some(r) = due.first() {
                        *active.borrow_mut() = Some(r.id.clone());
                        w.set_page_id("reminder".into());
                        w.set_reminder_alert_visible(true);
                        w.set_reminder_alert_text(r.text.clone().into());
                        let when = if r.due_at > 0 {
                            let (y, m, d, h, min) = reminder::local_parts(r.due_at, timezone_offset);
                            format!("{d:02}/{m:02}/{y} {h:02}:{min:02}")
                        } else {
                            "Pengingat".to_string()
                        };
                        w.set_reminder_alert_sub(when.into());
                        // Halaman Pengingat lebih besar dari EXPANDED; ukuran jendela harus mengikuti badan hub.
                        shell.place(&w, expanded_size(&w));
                        w.set_expanded(true);
                        show_reminders(&model, items, timezone_offset);
                    }
                }
            }
        });
    }
    {
        let weak = win.as_weak();
        let active = active_reminder.clone();
        let model = reminder_model.clone();
        win.on_reminder_stop_alert(move || {
            let id = active.borrow_mut().take();
            if let Some(id) = id { let _ = reminder::stop(id); }
            if let Some(w) = weak.upgrade() { w.set_reminder_alert_visible(false); }
            show_reminders(&model, reminder::list(), timezone_offset);
        });
    }
    {
        let weak = win.as_weak();
        let active = active_reminder.clone();
        let model = reminder_model.clone();
        win.on_reminder_snooze_alert(move || {
            let id = active.borrow_mut().take();
            if let Some(id) = id { let _ = reminder::snooze(id, 5); }
            if let Some(w) = weak.upgrade() { w.set_reminder_alert_visible(false); }
            show_reminders(&model, reminder::list(), timezone_offset);
        });
    }


    let fetch_prayer:Rc<dyn Fn(bool)>={
      let busy=prayer_busy.clone();let data=prayer_data.clone();let prefs=prayer_prefs.clone();let weak=win.as_weak();let offset=timezone_offset;
      Rc::new(move|refresh|{if busy.swap(true,Ordering::SeqCst){return;}let p=prefs.borrow().clone();let busy=busy.clone();let data=data.clone();let weak=weak.clone();
       std::thread::spawn(move||{let now=reminder::now_ms();let(y,m,_,_,_)=reminder::local_parts(now,offset);let cur=prayer::month(&p.provinsi,&p.kabkota,y as i32,m as u32,refresh);let(ny,nm,_,_,_)=reminder::local_parts(now+86_400_000,offset);let nxt=if ny!=y||nm!=m{prayer::month(&p.provinsi,&p.kabkota,ny as i32,nm as u32,false)}else{cur.clone()};let err=cur.as_ref().err().cloned().unwrap_or_default();let v=PrayerSchedule{current:cur.unwrap_or_default(),next:nxt.unwrap_or_default(),error:err};if let Ok(mut g)=data.lock(){*g=v;}busy.store(false,Ordering::SeqCst);let _=weak.upgrade_in_event_loop(|w|w.invoke_prayer_data_ready());});
      })
    };
    fetch_prayer(false);
    {let f=fetch_prayer.clone();win.on_prayer_refresh(move||f(true));}
    {
      let weak=win.as_weak();let data=prayer_data.clone();let ramadan=ramadan.clone();let prefs=prayer_prefs.clone();let fired=prayer_fired.clone();let offset=timezone_offset;
      prayer_timer.start(
          TimerMode::Repeated,
          Duration::from_secs(1),
          move || {
              let Ok(schedule) = data.lock() else {
                  return;
              };
              let now = reminder::now_ms();
              let (year, month, day_number, _, _) = reminder::local_parts(now, offset);

              let Some(window) = weak.upgrade() else {
                  return;
              };

              if window.get_page_id() == "prayer" {
                  render_prayer(&window, &schedule, offset, ramadan.get());
              }

              if let Some(day) = prayer_find(&schedule.current, day_number) {
                  let preferences = prefs.borrow().clone();
                  let prayer_times = [
                      ("Subuh", &day.subuh),
                      ("Dzuhur", &day.dzuhur),
                      ("Ashar", &day.ashar),
                      ("Maghrib", &day.maghrib),
                      ("Isya", &day.isya),
                  ];

                  for (name, time) in prayer_times {
                      let Some(at) = prayer_at((year, month, day_number), time, offset) else {
                          continue;
                      };

                      let triggers = [
                          ("pre", at - i64::from(preferences.remind_min) * 60_000),
                          ("at", at),
                      ];

                      for (kind, trigger) in triggers {
                          if kind == "pre" && !preferences.remind_before {
                              continue;
                          }
                          if now < trigger || now >= trigger + 600_000 {
                              continue;
                          }

                          let key = format!("{year}-{month}-{day_number}-{name}-{kind}");
                          if !fired.borrow_mut().insert(key) {
                              continue;
                          }

                          if kind == "pre" {
                              window.set_prayer_status(
                                  format!(
                                      "Menjelang {name} · {} menit lagi",
                                      preferences.remind_min
                                  )
                                  .into(),
                              );
                              continue;
                          }

                          window.set_prayer_status(
                              format!("Waktu {name} telah tiba").into(),
                          );

                          if preferences.mode == "tone" {
                              #[cfg(windows)]
                              unsafe {
                                  let _ = windows::Win32::System::Diagnostics::Debug::Beep(880, 250);
                              }
                          } else if preferences.mode == "adzan" {
                              window.set_prayer_status(
                                  format!("Adzan {name} sedang diputar").into(),
                              );
                              media::play_adhan(window.as_weak(), name.to_string());
                          }

                          if ramadan.get() {
                              window.set_dhikr_arabic(
                                  "اللّٰهُمَّ رَبَّ هٰذِهِ الدَّعْوَةَ التَّامَّةَ".into(),
                              );
                              window.set_dhikr_latin(
                                  "Allahumma rabba hadzihid-da'watit-tammah".into(),
                              );
                              window.set_dhikr_meaning(
                                  "Dzikir setelah adzan".into(),
                              );
                          }
                      }
                  }
              }
          },
      );
    }
    {
      let weak=win.as_weak();let data=prayer_data.clone();let ramadan=ramadan.clone();let offset=timezone_offset;
      win.on_prayer_data_ready(move||{if let Ok(s)=data.lock(){if let Some(w)=weak.upgrade(){render_prayer(&w,&s,offset,ramadan.get());}}});
    }

    // --- Kalender + hari libur Indonesia ---
    // Cache `holidays.json` dibaca sekarang (cepat); unduhan baru jalan di thread latar dan hasilnya
    // diserahkan lewat slot bersama + callback `holidays-ready` supaya state tetap di thread UI.
    let hol_path = notch_core::paths::data_file("holidays.json");
    let hol = Rc::new(RefCell::new(Holidays::new()));
    if let Some((_, items)) = calendar::read_cache(&hol_path) {
        hol.borrow_mut().ingest(&items);
    }
    let cal_view = Rc::new(RefCell::new(CalView::new(today(), &hol.borrow())));
    let cal_model = Rc::new(VecModel::<CalCell>::default());
    win.set_cal_cells(ModelRc::from(cal_model.clone()));
    win.set_cal_weekdays(ModelRc::from(Rc::new(VecModel::from(
        calendar::weekday_headers().iter().map(|s| SharedString::from(*s)).collect::<Vec<_>>(),
    ))));
    let redraw_cal: Rc<dyn Fn()> = {
        let (weak, model, view, hol) = (win.as_weak(), cal_model.clone(), cal_view.clone(), hol.clone());
        Rc::new(move || {
            if let Some(w) = weak.upgrade() {
                show_calendar(&w, &model, &view.borrow(), &hol.borrow());
            }
        })
    };
    let hol_slot: Arc<Mutex<Option<Vec<Item>>>> = Arc::new(Mutex::new(None));
    let hol_busy = Arc::new(AtomicBool::new(false));
    let start_fetch: Rc<dyn Fn()> = {
        let (weak, slot, busy, path) = (win.as_weak(), hol_slot.clone(), hol_busy.clone(), hol_path.clone());
        Rc::new(move || {
            if busy.swap(true, Ordering::SeqCst) {
                return;
            }
            let (weak, slot, busy, path) = (weak.clone(), slot.clone(), busy.clone(), path.clone());
            std::thread::spawn(move || {
                // gagal (offline, belum ada cache) = tetap memakai data bawaan
                if let Ok(items) = calendar::fetch(&path) {
                    if let Ok(mut g) = slot.lock() {
                        *g = Some(items);
                    }
                    let _ = weak.upgrade_in_event_loop(|w| w.invoke_holidays_ready());
                }
                busy.store(false, Ordering::SeqCst);
            });
        })
    };
    {
        let (hol, slot, redraw) = (hol.clone(), hol_slot.clone(), redraw_cal.clone());
        win.on_holidays_ready(move || {
            let items = slot.lock().ok().and_then(|mut g| g.take());
            if let Some(items) = items {
                hol.borrow_mut().ingest(&items);
                redraw();
            }
        });
    }
    {
        let (view, hol, redraw, fetch) = (cal_view.clone(), hol.clone(), redraw_cal.clone(), start_fetch.clone());
        win.on_cal_opened(move || {
            view.borrow_mut().go_today(today(), &hol.borrow());
            redraw();
            fetch(); // paling sering sekali per 12 jam (cache)
        });
    }
    {
        let (view, hol, redraw) = (cal_view.clone(), hol.clone(), redraw_cal.clone());
        win.on_cal_prev(move || {
            view.borrow_mut().shift_month(-1, today(), &hol.borrow());
            redraw();
        });
    }
    {
        let (view, hol, redraw) = (cal_view.clone(), hol.clone(), redraw_cal.clone());
        win.on_cal_next(move || {
            view.borrow_mut().shift_month(1, today(), &hol.borrow());
            redraw();
        });
    }
    {
        let (view, hol, redraw) = (cal_view.clone(), hol.clone(), redraw_cal.clone());
        win.on_cal_today(move || {
            view.borrow_mut().go_today(today(), &hol.borrow());
            redraw();
        });
    }
    {
        let (view, redraw) = (cal_view.clone(), redraw_cal.clone());
        win.on_cal_select(move |i| {
            view.borrow_mut().select(i.max(0) as usize);
            redraw();
        });
    }
    start_fetch(); // data libur terbaru di latar belakang sejak aplikasi mulai

    // --- Quick / Tools: aksi jalan di thread lain (bisa sampai 4 detik) ---
    {
        let weak = win.as_weak();
        let shell = shell.clone();
        let keep_awake = keep_awake.clone();
        win.on_quick_toggle(move |kind| {
            if kind.as_str() == "center" {
                // Kembali ke tengah atas (hanya untuk sesi ini; config tidak ditulis dari Slint).
                shell.pos.set(None);
                shell.monitor_hint.set(None);
                if let Some(w) = weak.upgrade() {
                    shell.place(&w, EXPANDED);
                }
                return;
            }
            let weak = weak.clone();
            let kind = kind.to_string();
            let keep_awake = keep_awake.clone();
            std::thread::spawn(move || {
                let q = quick::toggle(&kind);
                keep_awake.store(q.awake == Some(true), Ordering::Relaxed);
                let _ = weak.upgrade_in_event_loop(move |w| apply_quick(&w, &q));
            });
        });
    }

    // --- Hover dengan jeda dari config: timing.hover_open_ms / hover_close_ms ---
    let open_timer = Rc::new(Timer::default());
    let close_timer = Rc::new(Timer::default());
    let shrink_timer = Rc::new(Timer::default());
    // Pembukaan hub yang ditunda satu frame agar notch sempat disembunyikan sebelum jendela membesar.
    let expand_pending = Rc::new(Timer::default());
    let open_ms = Rc::new(Cell::new(u64::from(cfg.timing.hover_open_ms)));
    let close_ms = Rc::new(Cell::new(u64::from(cfg.timing.hover_close_ms)));
    let expand: Rc<dyn Fn()> = {
        let weak = win.as_weak();
        let shrink = shrink_timer.clone();
        let pending = expand_pending.clone();
        let sync = sync_pages.clone();
        let shell = shell.clone();
        Rc::new(move || {
            let Some(w) = weak.upgrade() else { return };
            shrink.stop();
            pending.stop();
            // Dengan wajah jam, halaman home harus ditetapkan sebelum ukuran native dihitung.
            if w.get_idle_clock() {
                w.set_page_id("clock".into());
            }
            // Hover saat greeting tampil: hub menggantikan greeting. Flag harus dibersihkan, kalau tidak kartu 385 px
            // muncul lagi di jendela 300 px begitu hub menciut.
            w.set_greeting_inline_visible(false);
            let size = expanded_size(&w);
            if !w.get_expanded() && window_needs_resize(&w, size) {
                // Jendela akan membesar sementara notch (mata) masih tampak: frame lama akan tampil di koordinat
                // baru sebagai "mata hantu". Sembunyikan notch dulu, beri satu frame untuk tergambar, baru besarkan.
                w.set_geometry_hold(true);
                let (weak, shell, sync) = (weak.clone(), shell.clone(), sync.clone());
                // 40 ms = beberapa frame agar frame tanpa notch sempat tergambar dan sampai ke layar; lalu tunggu
                // komposisi DWM berikutnya (seperti shrink_window_synced) dan besarkan jendela tepat setelahnya.
                pending.start(TimerMode::SingleShot, Duration::from_millis(40), move || {
                    platform::wait_vsync();
                    finish_expand(&weak, &shell, &*sync);
                });
            } else {
                finish_expand(&weak, &shell, &*sync);
            }
        })
    };
    let collapse: Rc<dyn Fn()> = {
        let weak = win.as_weak();
        let shrink = shrink_timer.clone();
        let pending = expand_pending.clone();
        let sync = sync_pages.clone();
        let shell = shell.clone();
        Rc::new(move || {
            let Some(w) = weak.upgrade() else { return };
            // Kursor keluar sebelum hub sempat dibuka: batalkan pembukaan yang tertunda dan tampilkan notch lagi.
            if pending.running() {
                pending.stop();
                w.set_geometry_hold(false);
            }
            // Hub yang ditutup pengguna: bila halaman terakhir adalah Jam, notch berganti dari mata ke jam + tanggal;
            // di halaman lain kembali ke mata (sama dengan collapse() di view-controller.ts Tauri).
            if w.get_expanded() && w.get_active_pages().iter().any(|p| p == "clock") {
                let clock = w.get_page_id() == "clock";
                if w.get_idle_clock() != clock {
                    w.set_idle_clock(clock);
                    let _ = std::fs::write(notch_core::paths::data_file("idle_face.txt"), if clock { "clock" } else { "eyes" });
                }
            }
            w.set_notes_expanded(false);
            w.set_expanded(false);
            w.set_cal_open(false); // kalender dan lirik ikut menutup saat notch menciut
            w.set_lyrics_open(false);
            sync();
            // jendela baru diperkecil setelah animasi menciut selesai
            let weak2 = weak.clone();
            let shell = shell.clone();
            // 300 ms animasi menciut + sedikit jeda, baru jendela diperkecil.
            shrink.start(TimerMode::SingleShot, Duration::from_millis(330), move || {
                shrink_window_synced(weak2.clone(), shell.clone());
            });
        })
    };
    {
        let weak = win.as_weak();
        let (open_timer, close_timer) = (open_timer.clone(), close_timer.clone());
        let (open_ms, close_ms) = (open_ms.clone(), close_ms.clone());
        let expand_on_hover = expand.clone();
        let collapse_on_hover = collapse.clone();
        win.on_hover_changed(move |hovered| {
            let Some(w) = weak.upgrade() else { return };
            w.set_play_interaction_active(hovered);
            if hovered {
                close_timer.stop();
                if !w.get_expanded() {
                    let e = expand_on_hover.clone();
                    open_timer.start(TimerMode::SingleShot, Duration::from_millis(open_ms.get()), move || e());
                }
            } else {
                open_timer.stop();
                if w.get_expanded() {
                    let c = collapse_on_hover.clone();
                    let weak_check = weak.clone();
                close_timer.start(TimerMode::SingleShot, Duration::from_millis(close_ms.get()), move || {
                    let Some(w) = weak_check.upgrade() else { return };
                    // Slint Flickable dapat mengambil alih pointer saat roda mouse digulir.
                    // Pada halaman Clipboard, event hover sementara bisa melaporkan keluar
                    // meskipun kursor masih berada di dalam window. Jangan ciutkan notch
                    // selama pointer masih berada di batas window; hover berikutnya akan
                    // tetap mengatur penutupan normal ketika pointer benar-benar keluar.
                    if w.get_page_id() == "clipboard" {
                        if let Some((cx, cy)) = platform::cursor_pos() {
                            let p = w.window().position();
                            let s = w.window().size();
                            if cx >= p.x && cx < p.x + s.width as i32
                                && cy >= p.y && cy < p.y + s.height as i32
                            {
                                return;
                            }
                        }
                    }
                    c();
                });
                }
            }
        });
    }

    // --- Shelf: drag file MASUK (dari Explorer) dan KELUAR (dari tile Shelf) ---
    // Slint hanya menyimpan SATU callback on_winit_window_event (pendaftaran baru menimpa yang lama), jadi semua
    // pendengar event jendela (Shelf, Convert) didaftarkan ke `winit_handlers` dan dipanggil lewat satu dispatcher.
    let winit_handlers: Rc<RefCell<Vec<Box<dyn FnMut(&winit::event::WindowEvent)>>>> = Rc::new(RefCell::new(Vec::new()));
    {
        let handlers = winit_handlers.clone();
        win.window().on_winit_window_event(move |_window, event| {
            for handler in handlers.borrow_mut().iter_mut() {
                handler(event);
            }
            EventResult::Propagate
        });
    }
    // True selama kita sendiri yang menyeret file keluar: event hover/drop dari drag itu diabaikan.
    let shelf_drag_out = Rc::new(Cell::new(false));
    // True bila hub dibuka oleh drag file (bukan oleh pengguna), supaya ditutup otomatis setelah selesai.
    let shelf_opened_by_drag = Rc::new(Cell::new(false));
    // Tutup hub yang dibuka oleh drag setelah jeda, kecuali pointer masih di dalam jendela
    // (hover Slint yang menutupnya saat pointer benar-benar keluar).
    let shelf_close_later: Rc<dyn Fn(u64)> = {
        let weak = win.as_weak();
        let close_timer = close_timer.clone();
        let collapse = collapse.clone();
        let opened = shelf_opened_by_drag.clone();
        Rc::new(move |delay_ms| {
            let weak = weak.clone();
            let collapse = collapse.clone();
            let opened = opened.clone();
            close_timer.start(TimerMode::SingleShot, Duration::from_millis(delay_ms), move || {
                let Some(w) = weak.upgrade() else { return };
                if !opened.get() || !w.get_expanded() { return; }
                if let Some((cx, cy)) = platform::cursor_pos() {
                    let (p, z) = (w.window().position(), w.window().size());
                    if cx >= p.x && cx < p.x + z.width as i32 && cy >= p.y && cy < p.y + z.height as i32 { return; }
                }
                opened.set(false);
                collapse();
            });
        })
    };
    {
        let store = shelf_store.clone();
        let model = shelf_model.clone();
        let weak = win.as_weak();
        let (open_timer, close_timer, shrink_timer) = (open_timer.clone(), close_timer.clone(), shrink_timer.clone());
        let (sync, shell) = (sync_pages.clone(), shell.clone());
        let (drag_out, opened_by_drag, close_later) = (shelf_drag_out.clone(), shelf_opened_by_drag.clone(), shelf_close_later.clone());
        // Buka hub di halaman Shelf SEKARANG: ubah halaman, perbesar jendela OS, lalu expand. Mengubah properti dari Rust
        // tidak memicu callback page-changed/hover Slint, jadi ukuran jendela dan sinkron halaman dilakukan di sini.
        let open_shelf = {
            let (weak, shell, sync) = (weak.clone(), shell.clone(), sync.clone());
            move || {
                let Some(w) = weak.upgrade() else { return };
                shrink_timer.stop();
                w.set_page_id("shelf".into());
                shell.place(&w, expanded_size(&w));
                w.set_expanded(true);
                sync();
            }
        };
        winit_handlers.borrow_mut().push(Box::new(move |event| {
            let Some(w) = weak.upgrade() else { return };
            match event {
                winit::event::WindowEvent::HoveredFile(_) => {
                    if drag_out.get() || !shelf_accepts_drop(&w) { return; }
                    open_timer.stop();
                    close_timer.stop();
                    if !(w.get_expanded() && w.get_page_id() == "shelf") {
                        opened_by_drag.set(!w.get_expanded());
                        open_shelf();
                    }
                    w.set_shelf_dropping(true);
                    w.set_shelf_status("Lepaskan file atau folder untuk menyimpannya ke Shelf".into());
                }
                winit::event::WindowEvent::HoveredFileCancelled => {
                    if drag_out.get() { return; }
                    w.set_shelf_dropping(false);
                    w.set_shelf_status("Seret file ke Notch · file asli tetap aman".into());
                    // File keluar dari jendela tanpa dilepas: tutup lagi bila hub dibuka oleh drag ini.
                    if opened_by_drag.get() { close_later(700); }
                }
                winit::event::WindowEvent::DroppedFile(path) => {
                    if drag_out.get() || !shelf_accepts_drop(&w) { return; }
                    // Winit mengirim satu DroppedFile per path; hub dipastikan terbuka walau tanpa HoveredFile.
                    let items = store.borrow_mut().add(vec![path.to_string_lossy().to_string()]);
                    model.set_vec(items.into_iter().map(|item| ShelfItem {
                        path: item.path.into(), name: item.name.into(), dir: item.dir,
                    }).collect::<Vec<_>>());
                    close_timer.stop();
                    if !(w.get_expanded() && w.get_page_id() == "shelf") {
                        opened_by_drag.set(!w.get_expanded());
                        open_shelf();
                    }
                    w.set_shelf_dropping(false);
                    w.set_shelf_status(format!("File ditambahkan · {} item tersimpan · file asli tetap di lokasi semula", model.row_count()).into());
                    // Beri waktu melihat hasilnya, lalu tutup bila hub dibuka oleh drag ini dan pointer sudah keluar.
                    if opened_by_drag.get() { close_later(2500); }
                }
                _ => {}
            }
        }));
    }

    // Native drag-out memakai crate drag-rs (OLE DoDragDrop, memblokir sampai drop selesai).
    #[cfg(windows)]
    {
        const DRAG_ICON: &[u8] = &[137,80,78,71,13,10,26,10,0,0,0,13,73,72,68,82,0,0,0,1,0,0,0,1,8,6,0,0,0,31,21,196,137,0,0,0,13,73,68,65,84,120,156,99,96,24,5,0,0,1,0,1,90,44,12,72,0,0,0,0,73,69,78,68,174,66,96,130];
        let weak_window = win.as_weak();
        let (drag_out, close_later, opened_by_drag) = (shelf_drag_out.clone(), shelf_close_later.clone(), shelf_opened_by_drag.clone());
        win.on_shelf_drag(move |raw_path| {
            if drag_out.get() { return; }
            let path = std::path::PathBuf::from(raw_path.as_str());
            if !path.exists() { return; }
            let path = match std::fs::canonicalize(path) { Ok(path) => path, Err(_) => return };
            // Upgrade weak handle di dalam callback agar tidak menyimpan borrow
            // dari `win.window()` ke closure yang harus hidup selama 'static.
            let Some(window_component) = weak_window.upgrade() else { return };
            let dropped = Arc::new(AtomicBool::new(false));
            let dropped_cb = dropped.clone();
            drag_out.set(true);
            let started = window_component.window().with_winit_window(|window| {
                let item = drag::DragItem::Files(vec![path]);
                let preview = drag::Image::Raw(DRAG_ICON.to_vec());
                drag::start_drag(
                    window,
                    item,
                    preview,
                    move |result, _cursor| { dropped_cb.store(matches!(result, drag::DragResult::Dropped), Ordering::Relaxed); },
                    drag::Options::default(),
                )
            });
            drag_out.set(false);
            match started {
                Some(Ok(())) => {
                    window_component.set_shelf_status(if dropped.load(Ordering::Relaxed) {
                        "File dikirim ke tujuan · file asli tetap tersimpan di Shelf".into()
                    } else {
                        "Seret dibatalkan · file tetap di Shelf".into()
                    });
                    // Selama drag modal Slint tidak menerima event pointer, jadi hub dapat tertinggal terbuka.
                    // Hanya hub yang dibuka oleh drag masuk yang ditutup otomatis; hub buka manual dibiarkan.
                    if opened_by_drag.get() { close_later(1200); }
                }
                Some(Err(error)) => {
                    eprintln!("Shelf drag-out gagal: {error:?}");
                    window_component.set_shelf_status("Gagal menyeret file keluar".into());
                }
                None => {}
            }
        });
    }

    // --- Jam: cek tiap detik, tapi hanya set properti kalau teks berubah ---
    let clock_timer = Timer::default();
    {
        let weak = win.as_weak();
        let last = Rc::new(Cell::new((99u32, 99u32)));
        let last_sec = Rc::new(Cell::new(99u32));
        let tick = move || {
            let Some(w) = weak.upgrade() else { return };
            let (h, m, d, mo, y) = platform::local_time();
            let (sec, wday) = platform::local_sec_wday();
            if last_sec.get() != sec {
                last_sec.set(sec);
                w.set_sec_text(format!("{sec:02}").into());
            }
            if last.get() != (h, m) {
                last.set((h, m));
                w.set_clock_text(format!("{h:02}:{m:02}").into());
                w.set_day_text(ID_DAYS[(wday % 7) as usize].into());
                w.set_date_short(format!("{d} {}", SHORT_MONTHS[(mo.clamp(1, 12) - 1) as usize]).into());
                w.set_date_text(format!("{d} {} {y}", ID_MONTHS[(mo.clamp(1, 12) - 1) as usize]).into());
            }
        };
        tick();
        clock_timer.start(TimerMode::Repeated, Duration::from_secs(1), tick);
    }

    // --- Pomodoro (state machine dari notch-core, sama dengan pomodoro.ts) ---
    let pomo = Rc::new(RefCell::new(Pomodoro::new(&cfg.pomodoro)));
    let focus_companion_runtime = Rc::new(RefCell::new(cfg.focus_companion.clone()));
    let conversation_runtime = Rc::new(RefCell::new(cfg.conversation_moments.clone()));
    let companion_focus_started = Rc::new(Cell::new(false));
    let companion_halfway_shown = Rc::new(Cell::new(false));
    let companion_hide_timer = Rc::new(Timer::default());
    // Focus Companion memakai antrean singkat agar tidak menimpa greeting, pengingat,
    // kartu Conversation Moments, atau overlay lain yang sedang tampil.
    let companion_pending = Rc::new(RefCell::new(None::<(String, String, Instant)>));
    show_pomo(&win, &pomo.borrow());
    let pomo_timer = Timer::default();
    {
        let weak = win.as_weak();
        let pomo = pomo.clone();
        let companion = focus_companion_runtime.clone();
        let focus_started = companion_focus_started.clone();
        let halfway_shown = companion_halfway_shown.clone();
        let hide_timer = companion_hide_timer.clone();
        let pending = companion_pending.clone();
        pomo_timer.start(TimerMode::Repeated, Duration::from_secs(1), move || {
            let mut p = pomo.borrow_mut();
            let finished = if p.running() { p.tick(Instant::now()) } else { None };
            let phase = p.phase();

            // Event otomatis hanya dipicu ketika fase benar-benar selesai, bukan saat tombol Skip ditekan.
            if let Some(completed_phase) = finished {
                let settings = companion.borrow().clone();
                if settings.enabled {
                    let message = match completed_phase {
                        Phase::Focus if settings.on_focus_complete => Some(("Fokus selesai! Kerja bagus. 🎉", "Focus Companion · waktunya istirahat")),
                        Phase::Break | Phase::Long if settings.on_break_end => Some(("Istirahat selesai. Siap fokus lagi? 🚀", "Focus Companion · sesi fokus berikutnya")),
                        _ => None,
                    };
                    if let Some((message, detail)) = message {
                        if let Some(w) = weak.upgrade() {
                            if !w.get_meeting_active() && !w.get_adhan_active() && pending.borrow().is_none() {
                                *pending.borrow_mut() = Some((message.to_string(), detail.to_string(), Instant::now() + Duration::from_secs(60)));
                            }
                        }
                    }
                }
                if completed_phase == Phase::Focus {
                    focus_started.set(false);
                    halfway_shown.set(false);
                } else if matches!(completed_phase, Phase::Break | Phase::Long) {
                    focus_started.set(false);
                    halfway_shown.set(false);
                }
            }

            // Sesi fokus dimulai saat timer pertama kali dijalankan, bukan saat resume setelah jeda.
            if phase == Phase::Focus && p.running() && !focus_started.get() {
                focus_started.set(true);
                halfway_shown.set(false);
                let settings = companion.borrow().clone();
                if settings.enabled && settings.on_start {
                    if let Some(w) = weak.upgrade() {
                        if !w.get_meeting_active() && !w.get_adhan_active() && pending.borrow().is_none() {
                            *pending.borrow_mut() = Some((
                                "Yuk, fokus satu hal dulu. Kamu pasti bisa! 🎯".to_string(),
                                "Focus Companion · sesi fokus dimulai".to_string(),
                                Instant::now() + Duration::from_secs(60),
                            ));
                        }
                    }
                }
            }

            if phase == Phase::Focus && p.running() && p.progress() >= 0.5 && !halfway_shown.get() {
                halfway_shown.set(true);
                let settings = companion.borrow().clone();
                if settings.enabled && settings.on_halfway {
                    if let Some(w) = weak.upgrade() {
                        if !w.get_meeting_active() && !w.get_adhan_active() && pending.borrow().is_none() {
                            *pending.borrow_mut() = Some((
                                "Sudah separuh jalan! Pertahankan ritmenya. 💪".to_string(),
                                "Focus Companion · 50% sesi fokus".to_string(),
                                Instant::now() + Duration::from_secs(60),
                            ));
                        }
                    }
                }
            }

            if pending.borrow().as_ref().is_some_and(|(_, _, expires)| Instant::now() >= *expires) {
                pending.borrow_mut().take();
            }
            if let Some(w) = weak.upgrade() {
                // Jangan memutar pesan fokus yang tertahan selama rapat atau Adzan.
                if w.get_meeting_active() || w.get_adhan_active() {
                    pending.borrow_mut().take();
                }
                let can_show = !w.get_expanded() && !w.get_nudge_visible()
                    && !w.get_reminder_alert_visible() && !w.get_meeting_active()
                    && w.get_meeting_finished().is_empty() && !w.get_adhan_active();
                if can_show {
                    if let Some((message, detail, _)) = pending.borrow_mut().take() {
                        w.set_nudge_message(message.into());
                        w.set_nudge_detail(detail.into());
                        w.set_nudge_visible(true);
                        let weak_hide = weak.clone();
                        hide_timer.start(TimerMode::SingleShot, Duration::from_secs(5), move || {
                            if let Some(w) = weak_hide.upgrade() { w.set_nudge_visible(false); }
                        });
                    }
                }
                show_pomo(&w, &p);
            }
        });
    }
    {
        let (weak, pomo) = (win.as_weak(), pomo.clone());
        win.on_toggle_pomo(move || {
            let mut p = pomo.borrow_mut();
            p.toggle(Instant::now());
            if let Some(w) = weak.upgrade() { show_pomo(&w, &p); }
        });
    }
    {
        let (weak, pomo) = (win.as_weak(), pomo.clone());
        let focus_started = companion_focus_started.clone();
        let halfway_shown = companion_halfway_shown.clone();
        win.on_reset_pomo(move || {
            let mut p = pomo.borrow_mut();
            p.reset();
            focus_started.set(false);
            halfway_shown.set(false);
            if let Some(w) = weak.upgrade() { show_pomo(&w, &p); }
        });
    }
    {
        let (weak, pomo) = (win.as_weak(), pomo.clone());
        let focus_started = companion_focus_started.clone();
        let halfway_shown = companion_halfway_shown.clone();
        win.on_skip_pomo(move || {
            let mut p = pomo.borrow_mut();
            p.skip();
            focus_started.set(false);
            halfway_shown.set(false);
            if let Some(w) = weak.upgrade() { show_pomo(&w, &p); }
        });
    }

    // Conversation Moments: amati transisi media dan aktivitas pointer secara native.
    // Cooldown bersama mencegah beberapa pemicu menumpuk menjadi spam komentar.
    let conversation_timer = Timer::default();
    {
        let weak = win.as_weak();
        let settings = conversation_runtime.clone();
        let eye = appearance_eye.clone();
        let hide_timer = Rc::new(Timer::default());
        let battery = battery.clone();
        // Momen yang terjadi ketika hub/kartu/Adzan sedang aktif menunggu sebentar,
        // mengikuti perilaku antrean di implementasi Tauri.
        let mut pending_event: Option<(&'static str, &'static str, Instant)> = None;
        let state = Rc::new(RefCell::new((
            false,
            String::new(),
            None::<Instant>,
            eye.borrow().as_ref().map(|ctx| ctx.borrow().last_move).unwrap_or(0.0),
            None::<bool>,
            None::<bool>,
            String::new(),
            false,
        )));
        conversation_timer.start(TimerMode::Repeated, Duration::from_secs(1), move || {
            let Some(w) = weak.upgrade() else { return };
            let cfg = settings.borrow().clone();
            let playing = w.get_media_playing();
            let title = w.get_media_title().to_string();
            let pointer_move = eye.borrow().as_ref().map(|ctx| ctx.borrow().last_move).unwrap_or(0.0);
            let battery_reading = battery.read();
            let weather_condition = w.get_weather_condition().to_string();
            let weather_rain = w.get_weather_rain().to_string();
            let mut state = state.borrow_mut();
            let (last_playing, last_title, last_message, last_move, was_low_battery, was_charging, last_weather, was_rain_soon) = &mut *state;
            let mut event: Option<(&'static str, &'static str)> = None;

            if playing && !*last_playing && cfg.on_play {
                event = Some(("Musik mulai diputar. Semoga playlist-nya bikin semangat! 🎵", "Conversation Moments · musik dimulai"));
            } else if playing && !title.is_empty() && title != *last_title && cfg.on_track_change {
                event = Some(("Lagu baru, suasana baru. 🎶", "Conversation Moments · lagu berganti"));
            } else if !playing && *last_playing && cfg.on_pause {
                event = Some(("Musik berhenti dulu. Mau istirahat sebentar?", "Conversation Moments · media dijeda"));
            }

            if pointer_move > *last_move + 0.001 {
                // last_move adalah timestamp gerakan sebelumnya (detik sejak startup).
                // Ukur sampai waktu sekarang, bukan selisih dua event pointer yang berdekatan.
                let idle_secs = (app_started_at.elapsed().as_secs_f32() - *last_move).max(0.0);
                if event.is_none() && cfg.on_return && idle_secs >= cfg.return_after_min as f32 * 60.0 {
                    event = Some(("Akhirnya kembali! Siap melanjutkan aktivitas? 👋", "Conversation Moments · kembali setelah lama tidak aktif"));
                }
                *last_move = pointer_move;
            }

            *last_playing = playing;
            if !title.is_empty() && title != "Tidak ada media" {
                *last_title = title;
            } else if !playing {
                *last_title = String::new();
            }

            // Perubahan kondisi dan prakiraan hujan hanya dinilai setelah data cuaca valid tersedia.
            if !weather_condition.is_empty()
                && weather_condition != "Memuat cuaca..."
                && weather_condition != "Data cuaca belum tersedia"
            {
                if !last_weather.is_empty() && weather_condition != *last_weather
                    && cfg.on_weather_change && event.is_none()
                {
                    event = Some(("Cuaca berubah. Sesuaikan aktivitasmu dengan kondisi di luar, ya! 🌤️", "Conversation Moments · kondisi cuaca berubah"));
                }
                *last_weather = weather_condition;
            }
            let rain_soon = weather_rain.starts_with("Hujan dalam ");
            if rain_soon && !*was_rain_soon && cfg.on_rain_soon && event.is_none() {
                event = Some(("Sepertinya hujan akan turun. Jangan lupa siapkan payung! ☔", "Conversation Moments · hujan segera"));
            }
            *was_rain_soon = rain_soon;

            // Status baterai dibaca ringan; komentar hanya muncul pada transisi menuju kondisi.
            if let Reading::Present(value) = battery_reading {
                // Sampel pertama hanya menjadi baseline; baterai yang memang sudah lemah
                // ketika aplikasi dibuka tidak dianggap sebagai transisi baru.
                let low = value.percent <= 15 && !value.charging;
                if let Some(previous_low) = *was_low_battery {
                    if low && !previous_low && cfg.on_low_battery && event.is_none() {
                        event = Some(("Baterai tinggal 15%. Saatnya mencari charger, ya! 🔋", "Conversation Moments · baterai lemah"));
                    }
                }
                if value.charging && *was_charging == Some(false) && cfg.on_charging && event.is_none() {
                    event = Some(("Laptop mulai mengisi daya. Energi kembali penuh! ⚡", "Conversation Moments · mulai mengisi daya"));
                }
                *was_low_battery = Some(low);
                *was_charging = Some(value.charging);
            }

            if w.get_meeting_active() {
                // Sama seperti Tauri: momen yang tertahan selama rapat dibuang.
                pending_event = None;
            }
            if cfg.enabled {
                if let Some((message, detail)) = event {
                    // Satu momen pending cukup agar pemicu tidak menimpa kartu yang sedang tampil.
                    if pending_event.is_none() {
                        pending_event = Some((message, detail, Instant::now() + Duration::from_secs(30)));
                    }
                }
            } else {
                pending_event = None;
            }
            if pending_event.is_some_and(|(_, _, expires)| Instant::now() >= expires) {
                pending_event = None;
            }
            let cooldown_ok = last_message.map(|at| at.elapsed() >= Duration::from_secs(u64::from(cfg.cooldown_min) * 60)).unwrap_or(true);
            let can_show = !w.get_expanded() && !w.get_nudge_visible()
                && !w.get_reminder_alert_visible() && !w.get_meeting_active()
                && w.get_meeting_finished().is_empty() && !w.get_adhan_active();
            if cfg.enabled && cooldown_ok && can_show {
                if let Some((message, detail, _)) = pending_event.take() {
                    w.set_nudge_message(message.into());
                    w.set_nudge_detail(detail.into());
                    w.set_nudge_visible(true);
                    if let Some(ctx) = eye.borrow().as_ref() {
                        ctx.borrow_mut().mood.flash(Mood::Happy, 5_500, Instant::now());
                    }
                    *last_message = Some(Instant::now());
                    let weak_hide = weak.clone();
                    hide_timer.start(TimerMode::SingleShot, Duration::from_secs(5), move || {
                        if let Some(w) = weak_hide.upgrade() { w.set_nudge_visible(false); }
                    });
                }
            }
        });
    }

    compress_ui::setup(&win);
    let convert = convert_ui::setup(&win);
    transfer_ui::setup(&win, &transfer_state);
    android_ui::setup(&win, &android_state, &mirror_process);

    let nudge_runtime_config = Rc::new(RefCell::new(cfg.nudge.clone()));
    {
        let weak = win.as_weak();
        let runtime_config = nudge_runtime_config.clone();
        let config_path = notch_core::paths::config_path();
        win.on_settings_save_nudge(move |enabled, idle, message, emoji, mood, show, only_silent, until_move| {
            let idle = idle.to_string().trim().parse::<u32>().ok().filter(|v| (1..=240).contains(v));
            let show = show.to_string().trim().parse::<u32>().ok().filter(|v| (2..=30).contains(v)).map(|seconds| seconds * 1_000);
            let message = message.to_string().trim().to_string();
            let emoji = emoji.to_string().trim().to_string();
            let mood = mood.to_string().trim().to_ascii_lowercase();
            let allowed_moods = ["normal", "happy", "sleepy", "surprised", "angry", "love", "suspicious", "sad"];
            let (Some(idle), Some(show)) = (idle, show) else {
                if let Some(w) = weak.upgrade() { w.set_nudge_save_status("Idle harus 1–240 menit dan durasi 2–30 detik.".into()); }
                return;
            };
            if message.is_empty() || message.chars().count() > 80 || emoji.is_empty() || emoji.chars().count() > 12 || !allowed_moods.contains(&mood.as_str()) {
                if let Some(w) = weak.upgrade() { w.set_nudge_save_status("Pesan wajib 1–80 karakter, emoji 1–12 karakter, dan ekspresi harus valid.".into()); }
                return;
            }
            let mut next = Config::load_readonly(&config_path);
            next.nudge.enabled = enabled;
            next.nudge.idle_min = idle;
            next.nudge.message = message;
            next.nudge.emoji = emoji;
            next.nudge.mood = mood;
            next.nudge.show_ms = show;
            next.nudge.only_silent = only_silent;
            next.nudge.until_move = until_move;
            next.nudge.sanitize();
            match next.save(&config_path) {
                Ok(()) => {

                    *runtime_config.borrow_mut() = next.nudge.clone();
                    if let Some(w) = weak.upgrade() {
                        w.set_nudge_enabled(next.nudge.enabled);
                        w.set_nudge_idle_min(next.nudge.idle_min.to_string().into());
                        w.set_nudge_message(next.nudge.message.clone().into());
                        w.set_nudge_emoji(next.nudge.emoji.clone().into());
                        w.set_nudge_mood(next.nudge.mood.clone().into());
                        w.set_nudge_show_ms((next.nudge.show_ms / 1_000).to_string().into());
                        w.set_nudge_only_silent(next.nudge.only_silent);
                        w.set_nudge_until_move(next.nudge.until_move);
                        w.set_nudge_save_status("Pengaturan sapaan otomatis tersimpan.".into());
                    }
                }
                Err(error) => if let Some(w) = weak.upgrade() {
                    w.set_nudge_save_status(format!("Gagal menyimpan: {error}").into());
                }
            }
        });
    }

    {
        let eye = appearance_eye.clone();
        win.on_settings_preview_nudge_mood(move |value| {
            let mood = value.to_string();
            let Some(mood) = Mood::parse(&mood) else { return };
            if let Some(eye_state) = eye.borrow().as_ref() {
                eye_state.borrow_mut().mood.flash(mood, 2_500, Instant::now());
            }
        });
    }

    {
        let weak = win.as_weak();
        let config_path = notch_core::paths::config_path();
        let greeting_voice_runtime = greeting_voice_runtime.clone();
        win.on_settings_save_greeting(move |enabled, mode, voice, custom_text| {
            let mode = mode.to_string();
            let voice = voice.to_string();
            let custom_text = custom_text.to_string().trim().to_string();
            if !["random", "auto", "halo", "morning", "noon", "afternoon", "evening", "custom"].contains(&mode.as_str()) {
                if let Some(w) = weak.upgrade() { w.set_greeting_save_status("Mode greeting tidak valid.".into()); }
                return;
            }
            if !["cewek", "cowok", "anime-cewek", "anime-cowok", "system"].contains(&voice.as_str()) {
                if let Some(w) = weak.upgrade() { w.set_greeting_save_status("Mode suara tidak valid.".into()); }
                return;
            }
            if custom_text.chars().count() > 120 || (mode == "custom" && custom_text.is_empty()) {
                if let Some(w) = weak.upgrade() { w.set_greeting_save_status("Teks greeting kustom wajib diisi dan maksimal 120 karakter.".into()); }
                return;
            }
            let mut next = Config::load_readonly(&config_path);
            next.greeting.enabled = enabled;
            next.greeting.mode = mode;
            next.greeting.voice = voice;
            next.greeting.custom_text = custom_text;
            next.greeting.sanitize();
            match next.save(&config_path) {
                Ok(()) => {

                    *greeting_voice_runtime.borrow_mut() = next.greeting.voice.clone();
                    if let Some(w) = weak.upgrade() {
                        w.set_greeting_enabled(next.greeting.enabled);
                        w.set_greeting_mode(next.greeting.mode.clone().into());
                        w.set_greeting_voice(next.greeting.voice.clone().into());
                        w.set_greeting_custom_text(next.greeting.custom_text.clone().into());
                        w.set_greeting_save_status("Preferensi greeting tersimpan.".into());
                    }
                }
                Err(error) => if let Some(w) = weak.upgrade() {
                    w.set_greeting_save_status(format!("Gagal menyimpan greeting: {error}").into());
                }
            }
        });
    }

    {
        let weak = win.as_weak();
        let config_path = notch_core::paths::config_path();
        let runtime = conversation_runtime.clone();
        win.on_settings_save_conversation(move |enabled, on_play, on_track_change, on_pause, on_return, on_weather_change, on_rain_soon, on_low_battery, on_charging, return_after, cooldown| {
            let return_after = return_after.parse::<u32>().ok().filter(|v| (5..=120).contains(v));
            let cooldown = cooldown.parse::<u32>().ok().filter(|v| (1..=60).contains(v));
            let (Some(return_after), Some(cooldown)) = (return_after, cooldown) else {
                if let Some(w) = weak.upgrade() { w.set_conversation_save_status("Durasi kembali harus 5–120 menit dan jeda komentar 1–60 menit.".into()); }
                return;
            };
            let mut next = Config::load_readonly(&config_path);
            next.conversation_moments.enabled = enabled;
            next.conversation_moments.on_play = on_play;
            next.conversation_moments.on_track_change = on_track_change;
            next.conversation_moments.on_pause = on_pause;
            next.conversation_moments.on_return = on_return;
            next.conversation_moments.on_weather_change = on_weather_change;
            next.conversation_moments.on_rain_soon = on_rain_soon;
            next.conversation_moments.on_low_battery = on_low_battery;
            next.conversation_moments.on_charging = on_charging;
            next.conversation_moments.return_after_min = return_after;
            next.conversation_moments.cooldown_min = cooldown;
            next.conversation_moments.sanitize();
            match next.save(&config_path) {
                Ok(()) => {

                    *runtime.borrow_mut() = next.conversation_moments.clone();
                    if let Some(w) = weak.upgrade() {
                        w.set_conversation_enabled(next.conversation_moments.enabled);
                        w.set_conversation_on_play(next.conversation_moments.on_play);
                        w.set_conversation_on_track_change(next.conversation_moments.on_track_change);
                        w.set_conversation_on_pause(next.conversation_moments.on_pause);
                        w.set_conversation_on_return(next.conversation_moments.on_return);
                        w.set_conversation_on_weather_change(next.conversation_moments.on_weather_change);
                        w.set_conversation_on_rain_soon(next.conversation_moments.on_rain_soon);
                        w.set_conversation_on_low_battery(next.conversation_moments.on_low_battery);
                        w.set_conversation_on_charging(next.conversation_moments.on_charging);
                        w.set_conversation_return_after_min(return_after.to_string().into());
                        w.set_conversation_cooldown_min(cooldown.to_string().into());
                        w.set_conversation_save_status("Preferensi Conversation Moments tersimpan.".into());
                    }
                }
                Err(error) => if let Some(w) = weak.upgrade() {
                    w.set_conversation_save_status(format!("Gagal menyimpan Conversation Moments: {error}").into());
                }
            }
        });
    }

    {
        let weak = win.as_weak();
        let config_path = notch_core::paths::config_path();
        let runtime = focus_companion_runtime.clone();
        win.on_settings_save_focus_companion(move |enabled, on_start, on_halfway, on_focus_complete, on_break_end| {
            let mut next = Config::load_readonly(&config_path);
            next.focus_companion.enabled = enabled;
            next.focus_companion.on_start = on_start;
            next.focus_companion.on_halfway = on_halfway;
            next.focus_companion.on_focus_complete = on_focus_complete;
            next.focus_companion.on_break_end = on_break_end;
            match next.save(&config_path) {
                Ok(()) => {

                    *runtime.borrow_mut() = next.focus_companion.clone();
                    if let Some(w) = weak.upgrade() {
                        w.set_focus_companion_enabled(next.focus_companion.enabled);
                        w.set_focus_companion_on_start(next.focus_companion.on_start);
                        w.set_focus_companion_on_halfway(next.focus_companion.on_halfway);
                        w.set_focus_companion_on_focus_complete(next.focus_companion.on_focus_complete);
                        w.set_focus_companion_on_break_end(next.focus_companion.on_break_end);
                        w.set_focus_companion_save_status("Preferensi Focus Companion tersimpan.".into());
                    }
                }
                Err(error) => if let Some(w) = weak.upgrade() {
                    w.set_focus_companion_save_status(format!("Gagal menyimpan Focus Companion: {error}").into());
                }
            }
        });
    }

    {
        let weak = win.as_weak();
        let eye = appearance_eye.clone();
        let shell = shell.clone();
        let timer = Timer::default();
        win.on_settings_test_greeting(move |mode, custom_text, voice_preset| {
            let Some(w) = weak.upgrade() else { return };
            let mode = mode.to_string();
            let custom_text = custom_text.to_string().trim().to_string();
            let voice_preset = voice_preset.to_string();
            if !["cewek", "cowok", "anime-cewek", "anime-cowok", "system"].contains(&voice_preset.as_str()) {
                w.set_greeting_save_status("Preset suara tidak valid.".into());
                return;
            }
            let (hour, _, _, _, _) = platform::local_time();
            let greeting = match speech::greeting_content(&mode, &custom_text, hour) {
                Some(greeting) => greeting,
                None => {
                    w.set_greeting_save_status("Mode greeting tidak dikenal atau teks kustom kosong.".into());
                    return;
                }
            };
            // Pratinjau memakai preset yang sedang dipilih di UI, meskipun belum disimpan.
            let selected_voice = voice_preset;
            let speech_voice = selected_voice.clone();
            let greeting_duration_ms = speech::estimated_duration_ms(&greeting.spoken);
            let _ = speech::speak(greeting.spoken.clone(), speech_voice);
            w.set_greeting_message(greeting.message.into());
            w.set_greeting_detail(greeting.detail.into());
            // Greeting memakai layer tersendiri agar tidak menimpa isi notifikasi Nudge.
            if w.get_dock() < 2 {
                w.set_greeting_speech_visible(false);
                w.set_nudge_visible(false);
                show_greeting_inline(weak.clone(), shell.clone());
            } else {
                w.set_greeting_inline_visible(false);
                w.set_nudge_visible(false);
                w.set_greeting_speech_visible(true);
                shell.place(&w, COLLAPSED);
            }
            if let Some(eye_state) = eye.borrow().clone() {
                eye_state.borrow_mut().mood.flash(Mood::Happy, greeting_duration_ms, Instant::now());
            }
            w.set_greeting_save_status("Pratinjau teks dan audio diputar. Pengaturan belum disimpan.".into());
            let weak = weak.clone();
            let shell_hide = shell.clone();
            timer.start(TimerMode::SingleShot, Duration::from_millis(greeting_duration_ms + 600), move || {
                if let Some(w) = weak.upgrade() {
                    let was_inline = w.get_greeting_inline_visible() && !w.get_expanded();
                    w.set_greeting_inline_visible(false);
                    if was_inline {
                        shrink_window_synced(weak.clone(), shell_hide.clone());
                    }
                    w.set_greeting_speech_visible(false);
                    w.set_nudge_visible(false);
                    if w.get_dock() >= 2 && !w.get_expanded() {
                        shell_hide.place(&w, COLLAPSED);
                    }
                }
            });
        });
    }

    {
        let weak = win.as_weak();
        let eye = appearance_eye.clone();
        let timer = Timer::default();
        win.on_settings_test_conversation(move |message, detail| {
            let Some(w) = weak.upgrade() else { return };
            let message = message.to_string().trim().to_string();
            let detail = detail.to_string().trim().to_string();
            if message.is_empty() || message.chars().count() > 100 || detail.chars().count() > 100 {
                w.set_conversation_save_status("Pesan pratinjau wajib diisi dan maksimal 100 karakter.".into());
                return;
            }
            w.set_nudge_message(message.into());
            w.set_nudge_detail(format!("{}  ·  Pratinjau Conversation Moments", detail).into());
            if let Some(eye_state) = eye.borrow().clone() {
                eye_state.borrow_mut().mood.flash(Mood::Happy, 5_500, Instant::now());
            }
            w.set_nudge_visible(true);
            w.set_conversation_save_status("Pratinjau ditampilkan tanpa menyimpan pengaturan.".into());
            let weak = weak.clone();
            timer.start(TimerMode::SingleShot, Duration::from_millis(5_000), move || {
                if let Some(w) = weak.upgrade() { w.set_nudge_visible(false); }
            });
        });
    }

    {
        let weak = win.as_weak();
        let eye = appearance_eye.clone();
        let timer = Timer::default();
        win.on_settings_test_focus_companion(move |message, detail| {
            let Some(w) = weak.upgrade() else { return };
            let message = message.to_string().trim().to_string();
            let detail = detail.to_string().trim().to_string();
            if message.is_empty() || message.chars().count() > 100 || detail.chars().count() > 100 {
                w.set_focus_companion_save_status("Pesan pratinjau wajib diisi dan maksimal 100 karakter.".into());
                return;
            }
            w.set_nudge_message(message.into());
            w.set_nudge_detail(format!("{}  ·  Pratinjau Focus Companion", detail).into());
            if let Some(eye_state) = eye.borrow().clone() {
                eye_state.borrow_mut().mood.flash(Mood::Happy, 5_500, Instant::now());
            }
            w.set_nudge_visible(true);
            w.set_focus_companion_save_status("Pratinjau ditampilkan tanpa menyimpan pengaturan.".into());
            let weak = weak.clone();
            timer.start(TimerMode::SingleShot, Duration::from_millis(5_000), move || {
                if let Some(w) = weak.upgrade() { w.set_nudge_visible(false); }
            });
        });
    }

    {
        let weak = win.as_weak();
        let eye = appearance_eye.clone();
        let collapse_preview = collapse.clone();
        let timer = Timer::default();
        win.on_settings_test_nudge(move |message, emoji, mood, show, _until_move| {
            let Some(w) = weak.upgrade() else { return };
            let message = message.to_string().trim().to_string();
            let emoji = emoji.to_string().trim().to_string();
            let mood = mood.to_string();
            // Form Pengaturan memakai detik (2–30), sama seperti handler penyimpanan.
            // Sebelumnya pratinjau menganggap nilai tersebut milidetik sehingga nilai 6 langsung ditolak.
            let Some(show_seconds) = show.to_string().parse::<u32>().ok().filter(|v| (2..=30).contains(v)) else {
                w.set_nudge_save_status("Durasi pratinjau harus 2–30 detik.".into());
                return;
            };
            let show_ms = show_seconds * 1_000;
            if message.is_empty() || message.chars().count() > 80 || emoji.chars().count() > 12
                || !["normal", "happy", "sleepy", "surprised", "angry", "love", "suspicious", "sad"].contains(&mood.as_str()) {
                w.set_nudge_save_status("Periksa pesan (1–80 karakter), emoji (maks. 12), dan ekspresi.".into());
                return;
            }
            // Preview harus terlihat walaupun hub sedang terbuka; gunakan jalur collapse yang sama
            // dengan hover-out agar dock samping kembali menyediakan ruang untuk SpeechBubble.
            let was_expanded = w.get_expanded();
            if was_expanded {
                collapse_preview();
            }
            w.set_nudge_message(message.into());
            w.set_nudge_detail(format!("{}  ·  Pratinjau sapaan otomatis", emoji).into());
            let eye_state = eye.borrow().clone();
            if let Some(eye_state) = eye_state {
                eye_state.borrow_mut().mood.flash(Mood::parse(&mood).unwrap_or(Mood::Sad), u64::from(show_ms) + 5_000, Instant::now());
            }
            w.set_nudge_visible(true);
            w.set_nudge_save_status("Pratinjau memakai nilai yang sedang diedit; belum menyimpan pengaturan.".into());
            let weak = weak.clone();
            timer.start(TimerMode::SingleShot, Duration::from_millis(show_ms as u64), move || {
                if let Some(w) = weak.upgrade() {
                    w.set_nudge_visible(false);
                }
            });
        });
    }

    {
        let weak = win.as_weak();
        let location = weather_location.clone();
        let config_path = notch_core::paths::config_path();
        win.on_settings_save_weather(move |city, lat, lon| {
            let city = city.to_string().trim().to_string();
            let Some(lat) = lat.to_string().trim().parse::<f64>().ok().filter(|v| v.is_finite() && (-90.0..=90.0).contains(v)) else {
                if let Some(w) = weak.upgrade() { w.set_weather_save_status("Latitude harus angka antara -90 dan 90.".into()); }
                return;
            };
            let Some(lon) = lon.to_string().trim().parse::<f64>().ok().filter(|v| v.is_finite() && (-180.0..=180.0).contains(v)) else {
                if let Some(w) = weak.upgrade() { w.set_weather_save_status("Longitude harus angka antara -180 dan 180.".into()); }
                return;
            };
            if city.is_empty() || city.len() > 80 {
                if let Some(w) = weak.upgrade() { w.set_weather_save_status("Nama kota wajib diisi (maksimal 80 karakter).".into()); }
                return;
            }
            let mut next = Config::load_readonly(&config_path);
            next.weather.city = city.clone();
            next.weather.lat = lat;
            next.weather.lon = lon;
            match next.save(&config_path) {
                Ok(()) => {

                    *location.borrow_mut() = (lat, lon, city.clone());
                    if let Some(w) = weak.upgrade() {
                        w.set_weather_city(city.clone().into());
                        w.set_weather_lat(lat.to_string().into());
                        w.set_weather_lon(lon.to_string().into());
                        w.set_weather_save_status("Lokasi tersimpan. Memperbarui cuaca...".into());
                        w.invoke_weather_refresh();
                    }
                }
                Err(error) => if let Some(w) = weak.upgrade() {
                    w.set_weather_save_status(format!("Gagal menyimpan: {error}").into());
                }
            }
        });
    }
    {
        let weak = win.as_weak();
        let shell = shell.clone();
        let eye_runtime = appearance_eye.clone();
        let sleep_after_s = sleep_after_s.clone();
        let config_path = notch_core::paths::config_path();
        win.on_settings_save_appearance(move |theme, eye, notch, mood, accent, custom_bg, custom_text, dark, light, scale| {
            let mut next = Config::load_readonly(&config_path);
            let scale = f64::from(scale);
            if !(0.5..=1.5).contains(&scale) {
                if let Some(w) = weak.upgrade() { w.set_appearance_save_status("Skala notch harus berada di antara 0.50× dan 1.50×".into()); }
                return;
            }
            next.scale = scale;
            next.appearance.theme = theme.to_string();
            next.appearance.eye_theme = eye.to_string();
            next.appearance.notch_theme = notch.to_string();
            next.appearance.default_mood = mood.to_string();
            next.appearance.accent = accent.to_string();
            next.appearance.background = custom_bg.to_string();
            next.appearance.text = custom_text.to_string();
            next.appearance.dark_bg = dark.to_string();
            next.appearance.light_bg = light.to_string();
            next.normalize();
            if !["auto", "dark", "light", "custom"].contains(&next.appearance.theme.as_str()) { if let Some(w) = weak.upgrade() { w.set_appearance_save_status("Tema harus auto, dark, light, atau custom".into()); } return; }
            let valid_hex = |v: &str| v.is_empty() || (v.len() == 7 && v.starts_with("#") && v[1..].bytes().all(|b| b.is_ascii_hexdigit()));
            if !valid_hex(&next.appearance.accent) || !valid_hex(&next.appearance.background) || !valid_hex(&next.appearance.text) || !valid_hex(&next.appearance.dark_bg) || !valid_hex(&next.appearance.light_bg) { if let Some(w) = weak.upgrade() { w.set_appearance_save_status("Warna harus memakai format HEX #RRGGBB".into()); } return; }
            match next.save(&config_path) {
                Ok(()) => {

                    let palette = Palette::from_appearance(&next.appearance, platform::os_light());
                    *shell.theme.borrow_mut() = next.appearance.notch_theme.clone();
                    *shell.body.borrow_mut() = palette.body;
                    *shell.scale.borrow_mut() = scale as f32;
                    shell.memo.set((None, 0));
                    if let Some(w) = weak.upgrade() {
                        apply_theme(&w, &palette);
                        w.set_ui_scale(scale as f32);
                        let size = if w.get_expanded() { expanded_size(&w) } else { COLLAPSED };
                        shell.place(&w, size);
                        if let Some(eye) = eye_runtime.borrow().as_ref() {
                            let mut eye = eye.borrow_mut();
                            let skin = eyes::skin_index(&next.appearance.eye_theme);
                            let (renderer, canvas_size) = eye_renderer_for_dock(&next.appearance.eye_theme, w.get_dock());
                            eye.renderer = renderer;
                            eye.canvas_size = canvas_size;
                            eye.skin = skin;
                            let mood = Mood::parse(&next.appearance.default_mood).unwrap_or(Mood::Normal);
                            eye.mood = MoodState::new(mood, next.timing.sleep_after_s, Instant::now());
                            sleep_after_s.set(next.timing.sleep_after_s);
                            eye.cur = mood;
                            eye.cur_since = 0.0;
                            eye.morph = Morph::new(Face::for_mood(mood, skin));
                            eye.last_key = None;
                        }
                        w.set_appearance_theme(next.appearance.theme.clone().into());
                        w.set_appearance_eye_theme(next.appearance.eye_theme.clone().into());
                        w.set_appearance_notch_theme(next.appearance.notch_theme.clone().into());
                        w.set_appearance_mood(next.appearance.default_mood.clone().into());
                        w.set_appearance_accent(next.appearance.accent.clone().into());
                        w.set_appearance_custom_bg(next.appearance.background.clone().into());
                        w.set_appearance_custom_text(next.appearance.text.clone().into());
                        w.set_appearance_dark_bg(next.appearance.dark_bg.clone().into());
                        w.set_appearance_light_bg(next.appearance.light_bg.clone().into());
                        w.set_behavior_scale(format!("{scale:.2}").into());
                        w.set_appearance_save_status("Tampilan dan ukuran tersimpan dan langsung diterapkan.".into());
                    }
                },
                Err(error) => { if let Some(w) = weak.upgrade() { w.set_appearance_save_status(format!("Gagal menyimpan: {error}").into()); } }
            }
        });
    }

    {
        let weak = win.as_weak();
        let shell = shell.clone();
        let eye_runtime = appearance_eye.clone();
        let sleep_after_s = sleep_after_s.clone();
        let preview_config = cfg.clone();
        win.on_settings_preview_appearance(move |theme, eye, notch, mood, accent, custom_bg, custom_text, dark, light, scale| {
            let mut next = preview_config.clone();
            next.appearance.theme = theme.to_string();
            next.appearance.eye_theme = eye.to_string();
            next.appearance.notch_theme = notch.to_string();
            next.appearance.default_mood = mood.to_string();
            next.appearance.accent = accent.to_string();
            next.appearance.background = custom_bg.to_string();
            next.appearance.text = custom_text.to_string();
            next.appearance.dark_bg = dark.to_string();
            next.appearance.light_bg = light.to_string();
            let scale = f64::from(scale);
            if !(0.5..=1.5).contains(&scale) {
                return;
            }
            next.scale = scale;
            next.normalize();
            if !["auto", "dark", "light", "custom"].contains(&next.appearance.theme.as_str()) {
                return;
            }
            let valid_hex = |v: &str| v.is_empty() || (v.len() == 7 && v.starts_with("#") && v[1..].bytes().all(|b| b.is_ascii_hexdigit()));
            if !valid_hex(&next.appearance.accent) || !valid_hex(&next.appearance.background) || !valid_hex(&next.appearance.text) || !valid_hex(&next.appearance.dark_bg) || !valid_hex(&next.appearance.light_bg) {
                return;
            }

            // Preview only: update the running notch and the settings mirror, but never write config.toml.
            let palette = Palette::from_appearance(&next.appearance, platform::os_light());
            *shell.theme.borrow_mut() = next.appearance.notch_theme.clone();
            *shell.body.borrow_mut() = palette.body;
            *shell.scale.borrow_mut() = scale as f32;
            shell.memo.set((None, 0));
            if let Some(w) = weak.upgrade() {
                apply_theme(&w, &palette);
                w.set_ui_scale(scale as f32);
                let size = if w.get_expanded() { expanded_size(&w) } else { COLLAPSED };
                shell.place(&w, size);

                if let Some(eye) = eye_runtime.borrow().as_ref() {
                    let mut eye = eye.borrow_mut();
                    let skin = eyes::skin_index(&next.appearance.eye_theme);
                    let (renderer, canvas_size) = eye_renderer_for_dock(&next.appearance.eye_theme, w.get_dock());
                    eye.renderer = renderer;
                    eye.canvas_size = canvas_size;
                    eye.skin = skin;
                    let mood = Mood::parse(&next.appearance.default_mood).unwrap_or(Mood::Normal);
                    eye.mood = MoodState::new(mood, next.timing.sleep_after_s, Instant::now());
                    sleep_after_s.set(next.timing.sleep_after_s);
                    eye.cur = mood;
                    eye.cur_since = 0.0;
                    eye.morph = Morph::new(Face::for_mood(mood, skin));
                    eye.last_key = None;
                }

                w.set_appearance_theme(next.appearance.theme.clone().into());
                w.set_appearance_eye_theme(next.appearance.eye_theme.clone().into());
                w.set_appearance_notch_theme(next.appearance.notch_theme.clone().into());
                w.set_appearance_mood(next.appearance.default_mood.clone().into());
                w.set_appearance_accent(next.appearance.accent.clone().into());
                w.set_appearance_custom_bg(next.appearance.background.clone().into());
                w.set_appearance_custom_text(next.appearance.text.clone().into());
                w.set_appearance_dark_bg(next.appearance.dark_bg.clone().into());
                w.set_appearance_light_bg(next.appearance.light_bg.clone().into());
                w.set_behavior_scale(format!("{scale:.2}").into());
                w.set_appearance_save_status("Pratinjau aktif — belum disimpan.".into());
            }
        });
    }

    {
        let weak = win.as_weak();
        win.on_settings_test_dock_play(move || {
            let Some(w) = weak.upgrade() else { return };
            if !w.get_dock_play_enabled() {
                w.set_motion_save_status("Aktifkan Bottom Play / Side Play terlebih dahulu.".into());
                return;
            }
            if !matches!(w.get_dock(), 2 | 3 | 4) {
                w.set_motion_save_status("Tes tersedia saat notch berada di dock bawah, kiri, atau kanan.".into());
                return;
            }
            w.set_play_test_request(true);
            w.set_motion_save_status("Tes liquid siap. Tutup pengaturan agar animasi dimulai.".into());
        });
    }

    {
        let weak = win.as_weak();
        let config_path = notch_core::paths::config_path();
        let runtime = motion_runtime.clone();
        win.on_settings_save_motion(move |enabled, interval, maximum, dock_play_enabled| {
            let interval = interval.parse::<u32>().ok().filter(|v| (60..=3600).contains(v));
            let maximum = maximum.parse::<u32>().ok().filter(|v| (1..=10).contains(v));
            let (Some(interval), Some(maximum)) = (interval, maximum) else {
                if let Some(w) = weak.upgrade() {
                    w.set_motion_save_status("Interval harus 60–3600 detik dan batas perpindahan 1–10.".into());
                }
                return;
            };
            let mut next = Config::load_readonly(&config_path);
            next.motion.enabled = enabled;
            next.motion.interval_secs = interval;
            next.motion.max_moves = maximum;
            next.motion.dock_play_enabled = dock_play_enabled;
            match next.save(&config_path) {
                Ok(()) => {

                    runtime.0.set(enabled);
                    runtime.1.set(interval);
                    runtime.2.set(maximum);
                    if let Some(w) = weak.upgrade() {
                        w.set_motion_enabled(enabled);
                        w.set_dock_play_enabled(dock_play_enabled);
                        if !dock_play_enabled { w.set_play_test_request(false); }
                        w.set_motion_interval(interval.to_string().into());
                        w.set_motion_max_moves(maximum.to_string().into());
                        w.set_motion_save_status(if dock_play_enabled && enabled {
                            "Pengaturan tersimpan. Liquid Movement dan Bottom/Side Play langsung diterapkan.".into()
                        } else if enabled {
                            "Liquid Movement langsung diterapkan. Bottom/Side Play dinonaktifkan.".into()
                        } else {
                            "Liquid Movement dan Bottom/Side Play dinonaktifkan.".into()
                        });
                    }
                }
                Err(error) => {
                    if let Some(w) = weak.upgrade() {
                        w.set_motion_save_status(format!("Gagal menyimpan: {error}").into());
                    }
                }
            }
        });
    }

    {
        let weak = win.as_weak();
        let shell = shell.clone();
        let open_ms = open_ms.clone();
        let close_ms = close_ms.clone();
        let sleep_after_s = sleep_after_s.clone();
        let hide_fullscreen = hide_fullscreen.clone();
        let eye_runtime = appearance_eye.clone();
        let config_path = notch_core::paths::config_path();
        win.on_settings_save_behavior(move |scale, hover_open, hover_close, sleep_after, hide_fullscreen_enabled| {
            let mut next = Config::load_readonly(&config_path);
            let scale = scale.parse::<f64>().ok().filter(|v| (0.5..=1.5).contains(v));
            let hover_open = hover_open.parse::<u32>().ok().filter(|v| *v <= 2000);
            let hover_close = hover_close.parse::<u32>().ok().filter(|v| *v <= 5000);
            let sleep_after = sleep_after.parse::<u32>().ok().filter(|v| (10..=3600).contains(v));
            let (Some(scale), Some(hover_open), Some(hover_close), Some(sleep_after)) =
                (scale, hover_open, hover_close, sleep_after) else {
                if let Some(w) = weak.upgrade() {
                    w.set_behavior_save_status("Skala 0.5–1.5, hover buka 0–2000 ms, hover tutup 0–5000 ms, idle 10–3600 detik.".into());
                }
                return;
            };
            next.scale = scale;
            next.timing.hover_open_ms = hover_open;
            next.timing.hover_close_ms = hover_close;
            next.timing.sleep_after_s = sleep_after;
            next.hide_fullscreen = hide_fullscreen_enabled;
            match next.save(&config_path) {
                Ok(()) => {

                    open_ms.set(u64::from(hover_open));
                    close_ms.set(u64::from(hover_close));
                    sleep_after_s.set(sleep_after);
                    hide_fullscreen.set(hide_fullscreen_enabled);
                    *shell.scale.borrow_mut() = scale as f32;
                    shell.memo.set((None, 0));
                    if let Some(eye) = eye_runtime.borrow().as_ref() {
                        eye.borrow_mut().mood.set_sleep_after(sleep_after);
                    }
                    if let Some(w) = weak.upgrade() {
                        w.set_ui_scale(scale as f32);
                        let size = if w.get_expanded() { expanded_size(&w) } else { COLLAPSED };
                        shell.place(&w, size);
                        w.set_behavior_scale(format!("{scale:.2}").into());
                        w.set_behavior_hover_open(hover_open.to_string().into());
                        w.set_behavior_hover_close(hover_close.to_string().into());
                        w.set_behavior_sleep_after(sleep_after.to_string().into());
                        w.set_behavior_hide_fullscreen(hide_fullscreen_enabled);
                        w.set_behavior_save_status("Perilaku notch tersimpan dan langsung diterapkan.".into());
                    }
                }
                Err(error) => {
                    if let Some(w) = weak.upgrade() {
                        w.set_behavior_save_status(format!("Gagal menyimpan: {error}").into());
                    }
                }
            }
        });
    }

    {
        let weak = win.as_weak();
        let prefs = prayer_prefs.clone();
        let pomo = pomo.clone();
        let fetch_prayer = fetch_prayer.clone();
        let config_path = notch_core::paths::config_path();
        win.on_settings_save_routines(move |mode, remind, minutes, focus, rest, long, rounds, province, city| {
            let mut next = Config::load_readonly(&config_path);
            let mode = mode.to_string();
            if !["tone", "adzan", "silent"].contains(&mode.as_str()) { if let Some(w) = weak.upgrade() { w.set_settings_save_status("Mode shalat harus tone, adzan, atau silent".into()); } return; }
            let remind = remind.to_string();
            if !["true", "false"].contains(&remind.as_str()) { if let Some(w) = weak.upgrade() { w.set_settings_save_status("Pengingat harus true atau false".into()); } return; }
            let parse = |v: slint::SharedString, min: u32, max: u32| -> Option<u32> { let n = v.parse::<u32>().ok()?; if (min..=max).contains(&n) { Some(n) } else { None } };
            let Some(minutes) = parse(minutes, 1, 120) else { if let Some(w) = weak.upgrade() { w.set_settings_save_status("Menit pengingat harus 1–120".into()); } return; };
            let Some(focus) = parse(focus, 1, 180) else { if let Some(w) = weak.upgrade() { w.set_settings_save_status("Fokus Pomodoro harus 1–180 menit".into()); } return; };
            let Some(rest) = parse(rest, 1, 60) else { if let Some(w) = weak.upgrade() { w.set_settings_save_status("Istirahat harus 1–60 menit".into()); } return; };
            let Some(long) = parse(long, 1, 120) else { if let Some(w) = weak.upgrade() { w.set_settings_save_status("Istirahat panjang harus 1–120 menit".into()); } return; };
            let Some(rounds) = parse(rounds, 1, 12) else { if let Some(w) = weak.upgrade() { w.set_settings_save_status("Jumlah ronde harus 1–12".into()); } return; };
            let province = province.to_string().trim().to_string();
            let city = city.to_string().trim().to_string();
            if province.is_empty() || province.chars().count() > 80 || city.is_empty() || city.chars().count() > 100 {
                if let Some(w) = weak.upgrade() { w.set_settings_save_status("Provinsi wajib diisi (maks. 80 karakter) dan kabupaten/kota wajib diisi (maks. 100 karakter).".into()); }
                return;
            }
            next.prayer.provinsi = province;
            next.prayer.kabkota = city;
            next.prayer.mode = mode; next.prayer.remind_before = remind == "true"; next.prayer.remind_min = minutes;
            next.pomodoro.focus_min = focus; next.pomodoro.break_min = rest; next.pomodoro.long_break_min = long; next.pomodoro.rounds = rounds;
            match next.save(&config_path) {
                Ok(()) => {

                    *prefs.borrow_mut() = next.prayer.clone();
                    fetch_prayer(true);
                    let mut timer = pomo.borrow_mut();
                    timer.update_config(&next.pomodoro, Instant::now());
                    if let Some(w) = weak.upgrade() {
                        show_pomo(&w, &timer);
                        w.set_prayer_province(next.prayer.provinsi.clone().into());
                        w.set_prayer_city(next.prayer.kabkota.clone().into());
                        w.set_prayer_mode(next.prayer.mode.clone().into());
                        w.set_prayer_remind_before(next.prayer.remind_before.to_string().into());
                        w.set_prayer_remind_min(next.prayer.remind_min.to_string().into());
                        w.set_pomodoro_focus(next.pomodoro.focus_min.to_string().into());
                        w.set_pomodoro_break(next.pomodoro.break_min.to_string().into());
                        w.set_pomodoro_long_break(next.pomodoro.long_break_min.to_string().into());
                        w.set_pomodoro_rounds(next.pomodoro.rounds.to_string().into());
                        w.set_settings_save_status("Tersimpan dan langsung diterapkan ke timer shalat serta Pomodoro.".into());
                    }
                },
                Err(error) => { if let Some(w) = weak.upgrade() { w.set_settings_save_status(format!("Gagal menyimpan: {error}").into()); } }
            }
        });
    }

    // --- Global shortcut Windows: diproses di thread UI agar aman untuk state Slint ---
    let hotkey_poll_timer = Timer::default();
    {
        let (expand, collapse, pomo) = (expand.clone(), collapse.clone(), pomo.clone());
        let open_settings = open_settings.clone();
        let weak = win.as_weak();
        hotkey_poll_timer.start(TimerMode::Repeated, Duration::from_millis(60), move || {
            while let Ok(action) = hotkey_rx.try_recv() {
                match action {
                    hotkeys::Action::ToggleHub => {
                        if let Some(w) = weak.upgrade() {
                            if w.get_expanded() { collapse(); } else { expand(); }
                        }
                    }
                    hotkeys::Action::TogglePomodoro => {
                        let mut p = pomo.borrow_mut();
                        p.toggle(Instant::now());
                        if let Some(w) = weak.upgrade() { show_pomo(&w, &p); }
                    }
                    hotkeys::Action::OpenPrayer => {
                        if let Some(w) = weak.upgrade() {
                            w.set_page_id("prayer".into());
                            expand();
                        }
                    }
                    hotkeys::Action::ColorPicker => {
                        colorpick::toggle();
                    }
                    hotkeys::Action::OpenAndroid => {
                        if let Some(w) = weak.upgrade() {
                            w.set_page_id("android".into());
                            expand();
                        }
                    }
                    hotkeys::Action::OpenTransfer => {
                        if let Some(w) = weak.upgrade() {
                            w.set_page_id("transfer".into());
                            expand();
                        }
                    }
                    hotkeys::Action::OpenConvert => {
                        if let Some(w) = weak.upgrade() {
                            w.set_page_id("convert".into());
                            expand();
                        }
                    }
                    hotkeys::Action::OpenCompress => {
                        if let Some(w) = weak.upgrade() {
                            w.set_page_id("compress".into());
                            expand();
                        }
                    }
                    hotkeys::Action::OpenSettings => open_settings(),
                    hotkeys::Action::ShortcutRegistrationFailed => {
                        if let Some(w) = weak.upgrade() {
                            w.set_settings_save_status("Windows menolak salah satu shortcut karena sedang dipakai aplikasi lain atau sistem. Ganti kombinasinya.".into());
                        }
                    }
                }
            }
        });
    }

    // --- Mata: 8 mood x 20 skin, dirender Rust (notch-core/src/eyes.rs) ---
    let started = Instant::now();
    let skin = eyes::skin_index(&cfg.appearance.eye_theme);
    let default_mood = Mood::parse(&cfg.appearance.default_mood).unwrap_or(Mood::Normal);
    let (initial_eye_renderer, initial_eye_size) = eye_renderer_for_dock(&cfg.appearance.eye_theme, win.get_dock());
    let eye = Rc::new(RefCell::new(EyeCtx {
        renderer: initial_eye_renderer,
        canvas_size: initial_eye_size,
        skin,
        mood: MoodState::new(default_mood, cfg.timing.sleep_after_s, started),
        cur: default_mood,
        cur_since: 0.0,
        morph: Morph::new(Face::for_mood(default_mood, skin)),
        look: (0.0, 0.0),
        look_target: (0.0, 0.0),
        last_frame_ts: 0.0,
        last_cursor: (i32::MIN, i32::MIN),
        last_move: 0.0,
        next_random_look: 0.0,
        blink_until: 0.0,
        next_blink: 2.5,
        last_key: None,
        rng: Rng::new(),
    }));
    *appearance_eye.borrow_mut() = Some(eye.clone());

    // Sapaan otomatis saat pointer idle. Satu sapaan per periode diam; gerakan baru mengaktifkan periode berikutnya.
    let nudge_timer = Timer::default();
    {
        let weak = win.as_weak();
        let eye = eye.clone();
        let config = nudge_runtime_config.clone();
        // (aktif, waktu gerak terakhir, waktu pesan ditampilkan, sedang menampilkan kartu kembali)
        let state = Rc::new(RefCell::new((false, 0.0_f32, 0.0_f32, false)));
        let state_tick = state.clone();
        nudge_timer.start(TimerMode::Repeated, Duration::from_secs(5), move || {
            let Some(w) = weak.upgrade() else { return };
            let elapsed = started.elapsed().as_secs_f32();
            let last_move = eye.borrow().last_move;
            let cfg = config.borrow();
            let mut state = state_tick.borrow_mut();
            if last_move > state.1 + 0.001 {
                // Sapaan balik ditampilkan setiap kali periode diam berakhir dengan gerakan,
                // termasuk bila kartu awal sudah tertutup otomatis (until_move = false).
                let was_nudge_shown = state.0 && !state.3;
                let was_return_card = state.3;
                state.0 = false;
                state.1 = last_move;
                state.2 = 0.0;
                state.3 = false;
                if was_nudge_shown && !was_return_card && cfg.enabled {
                    state.0 = true;
                    state.2 = elapsed;
                    state.3 = true;
                    w.set_nudge_message("Akhirnya kamu kembali!".into());
                    w.set_nudge_detail("Senang melihatmu kembali".into());
                    eye.borrow_mut().mood.flash(Mood::Happy, u64::from(cfg.show_ms) + 5_000, Instant::now());
                    w.set_nudge_visible(true);
                } else {
                    w.set_nudge_visible(false);
                }
            }
            if !cfg.enabled {
                w.set_nudge_visible(false);
                state.0 = false;
                state.3 = false;
                return;
            }
            if state.0 {
                // Sapaan awal bertahan sampai pointer bergerak. Kartu "kembali" tetap
                // memiliki durasi sendiri, lalu menunggu periode idle berikutnya.
                if (state.3 || !cfg.until_move)
                    && elapsed - state.2 >= cfg.show_ms as f32 / 1000.0
                {
                    w.set_nudge_visible(false);
                    state.3 = false;
                }
                return;
            }
            let idle_secs = (elapsed - last_move).max(0.0);
            if idle_secs < cfg.idle_min as f32 * 60.0 { return; }
            if cfg.only_silent && w.get_media_playing() { return; }
            if w.get_expanded() || w.get_reminder_alert_visible() || w.get_adhan_active() || w.get_meeting_active() || w.get_meeting_finished() != "" { return; }
            state.0 = true;
            state.2 = elapsed;
            w.set_nudge_message(cfg.message.clone().into());
            w.set_nudge_detail(format!("{}  ·  Pointer diam {} menit", cfg.emoji, (idle_secs / 60.0).floor() as u32).into());
            eye.borrow_mut().mood.flash(Mood::parse(&cfg.mood).unwrap_or(Mood::Sad), u64::from(cfg.show_ms) + 5_000, Instant::now());
            w.set_nudge_visible(true);
        });
    }

    // Klik / dobel klik pada notch (Tauri: klik -> happy atau marah bila 5x dalam 2 dtk; dobel -> love)
    {
        let eye = eye.clone();
        let weak = win.as_weak();
        win.on_pill_clicked(move || {
            if weak.upgrade().map(|w| w.get_meeting_active()).unwrap_or(false) { return; }
            eye.borrow_mut().mood.click(Instant::now());
        });
    }
    {
        let eye = eye.clone();
        let weak = win.as_weak();
        win.on_pill_double_clicked(move || {
            if weak.upgrade().map(|w| w.get_meeting_active()).unwrap_or(false) { return; }
            eye.borrow_mut().mood.double_click(Instant::now());
        });
    }

    // Mood dasar (tidur saat idle/malam, musik, baterai) dinilai ulang tiap 10 dtk (mood.ts)
    let base_timer = Timer::default();
    {
        let eye = eye.clone();
        let keep_awake = keep_awake.clone();
        let weak = win.as_weak();
        base_timer.start(TimerMode::Repeated, Duration::from_secs(10), move || {
            if weak.upgrade().map(|w| w.get_meeting_active()).unwrap_or(false) { return; }
            let mut e = eye.borrow_mut();
            e.mood.keep_awake = keep_awake.load(Ordering::Relaxed);
            e.mood.update_base(Instant::now(), platform::local_time().0);
        });
    }

    // Baterai dibaca tiap 30 dtk untuk mood (mengisi -> love, lemah -> mengantuk)
    let mood_battery_timer = Timer::default();
    {
        let eye = eye.clone();
        let battery = battery.clone();
        let weak = win.as_weak();
        let tick = move || {
            if weak.upgrade().map(|w| w.get_meeting_active()).unwrap_or(false) { return; }
            if let Reading::Present(b) = battery.read() {
                eye.borrow_mut().mood.on_battery(b.percent, b.charging, Instant::now(), platform::local_time().0);
            }
        };
        tick();
        mood_battery_timer.start(TimerMode::Repeated, Duration::from_secs(30), tick);
    }

    // Tick mata ~60 fps agar tatapan dapat mengikuti pointer dengan interpolasi halus.
    let eye_timer = Timer::default();
    {
        let weak = win.as_weak();
        let eye = eye.clone();
        eye_timer.start(TimerMode::Repeated, Duration::from_millis(16), move || {
            let Some(w) = weak.upgrade() else { return };
            let now = Instant::now();
            let ts = started.elapsed().as_secs_f32();
            let hour = platform::local_time().0;
            let mut guard = eye.borrow_mut();
            let e = &mut *guard;

            // Tatapan: ikuti kursor; bila diam > 5 dtk, melirik acak tiap 2,5 dtk (notch-face.ts)
            if let Some((cx, cy)) = platform::cursor_pos() {
                let (px, py) = e.last_cursor;
                if (cx as f32 - px as f32).hypot(cy as f32 - py as f32) > 2.0 {
                    e.last_move = ts;
                    // Aktivitas kursor tidak boleh mengubah ekspresi ketika Mode Rapat aktif.
                    if !w.get_meeting_active() {
                        e.mood.activity(now, hour);
                    }
                }
                if (cx, cy) != e.last_cursor {
                    e.last_cursor = (cx, cy);
                    // relatif terhadap notch (bukan tengah layar), supaya benar juga untuk posisi kustom
                    let (wp, ws) = (w.window().position(), w.window().size());
                    let mid_x = wp.x + ws.width as i32 / 2;
                    e.look_target = eye_look_offset((cx - mid_x) as f32, (cy - wp.y) as f32, w.get_dock());
                }
            }
            if ts - e.last_move > 5.0 && ts >= e.next_random_look {
                e.next_random_look = ts + 2.5;
                e.look_target = eye_look_offset((e.rng.next() - 0.5) * 300.0, (e.rng.next() - 0.5) * 60.0, w.get_dock());
            }

            // Smooth-follow: filter eksponensial yang stabil terhadap variasi frame time.
            // Tau 85 ms menjaga respons terasa cepat, tetapi menghilangkan gerak patah saat pointer berubah.
            let frame_dt = if e.last_frame_ts > 0.0 {
                (ts - e.last_frame_ts).clamp(0.0, 0.1)
            } else {
                1.0 / 60.0
            };
            e.last_frame_ts = ts;
            let follow = 1.0 - (-frame_dt / 0.085).exp();
            e.look.0 += (e.look_target.0 - e.look.0) * follow;
            e.look.1 += (e.look_target.1 - e.look.1) * follow;
            if w.get_dock() == 3 || w.get_dock() == 4 {
                // Saat berpindah dari dock atas, jangan biarkan offset lama (hingga 6 px)
                // membuat mata samping keluar dari kanvas 36 px.
                e.look.0 = e.look.0.clamp(-2.0, 2.0);
            }
            if (e.look_target.0 - e.look.0).abs() < 0.015 {
                e.look.0 = e.look_target.0;
            }
            if (e.look_target.1 - e.look.1).abs() < 0.015 {
                e.look.1 = e.look_target.1;
            }

            // Mood: transisi 250 ms ke mood baru
            let m = e.mood.current(now);
            if m != e.cur {
                e.cur = m;
                e.cur_since = ts;
                let face = Face::for_mood(m, e.skin);
                e.morph.set_target(ts, face);
            }
            w.set_eyes_mood(e.cur.name().into());

            // Kedip acak 2,5-6 dtk (mengantuk 1,5-5 dtk); mata "love" tidak berkedip; kedip 130 ms
            if ts >= e.next_blink {
                if e.cur != Mood::Love {
                    e.blink_until = ts + 0.13;
                }
                let base = if e.cur == Mood::Sleepy { 1.5 } else { 2.5 };
                e.next_blink = ts + base + e.rng.next() * 3.5;
            }
            let blink = ts < e.blink_until;

            if w.get_expanded() {
                return; // mata tersembunyi saat hub terbuka: tidak perlu menggambar
            }

            let (canvas_w, canvas_h) = match w.get_dock() {
                0 => (EYES_W, EYES_TOP_H),
                // Dock samping: mata tetap berdampingan pada badan notch 40 x 80 px.
                3 | 4 => (36.0, 18.0),
                _ => (EYES_W, EYES_H),
            };
            if e.canvas_size != (canvas_w, canvas_h) {
                let skin_name = e.renderer.skin_name();
                e.renderer = EyeRenderer::new(skin_name, canvas_w, canvas_h);
                e.canvas_size = (canvas_w, canvas_h);
                e.last_key = None;
            }
            let sf = w.window().scale_factor();
            let anim = Anim { mood: e.cur, t: ts, t_mood: ts - e.cur_since, look: e.look, blink };
            let key = (
                (e.look.0 * 8.0) as i32,
                (e.look.1 * 8.0) as i32,
                blink,
                e.cur as usize,
                eyes::frame_key(&anim, e.renderer.skin_name(), !e.morph.settled(ts)),
                sf.to_bits(),
            );
            if e.last_key == Some(key) {
                return;
            }
            e.last_key = Some(key);
            let face = e.morph.current(ts);
            let buf = e.renderer.render(&face, &anim, sf);
            let (pw, ph) = e.renderer.size_px(sf);
            w.set_eyes_image(Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&buf, pw, ph)));
        });
    }

    // Bottom Play / Side Play native: mesin gerak organik berbasis waktu dan easing.
    // Semua transform tetap di dalam Window Slint; tidak menggeser native window.
    let dock_play_timer = Timer::default();
    {
        let weak = win.as_weak();
        let eye = eye.clone();
        let battery = battery.clone();
        // Sama dengan Tauri: gerakan pertama boleh muncul setelah jeda 4 detik.
        let mut next_at = Instant::now() + Duration::from_secs(4);
        // source: 0=random, 1=Liquid terjadwal, 2=tes manual.
        let mut active: Option<(u8, Instant, Duration, f32, u8)> = None;
        let mut last_choice: Option<u8> = None;
        let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
        // Setiap tiga menit jalankan tiga rangkaian Liquid, mengikuti jadwal Tauri.
        let mut next_liquid_at = Instant::now() + Duration::from_secs(180);
        let mut liquid_repetitions_left = 0u8;
        let mut next_liquid_run_at = Instant::now();
        // Pegas semi-implisit untuk perpindahan dan deformasi badan.
        // [posisi, kecepatan] sehingga target dapat berubah tanpa transform meloncat.
        let mut spring_offset = (0.0f32, 0.0f32);
        let mut spring_sx = (0.0f32, 0.0f32);
        let mut spring_sy = (0.0f32, 0.0f32);
        let mut last_frame = Instant::now();

        dock_play_timer.start(TimerMode::Repeated, Duration::from_millis(16), move || {
            let Some(w) = weak.upgrade() else { return };
            let now = Instant::now();
            let dock = w.get_dock();
            let supported = matches!(dock, 2 | 3 | 4);
            let blocked = !w.get_dock_play_enabled()
                || !supported
                || w.get_expanded()
                || w.get_play_interaction_active()
                || w.get_nudge_visible()
                || w.get_reminder_alert_visible()
                || !w.get_meeting_finished().is_empty()
                || w.get_meeting_active()
                || w.get_adhan_active();

            if blocked {
                // Liquid terjadwal/tes manual yang terpotong tetap antre, seperti catch(CANCEL) di Tauri.
                if let Some((kind, _, _, _, source)) = active.take() {
                    if kind == 0 {
                        if source == 1 {
                            liquid_repetitions_left = liquid_repetitions_left.saturating_add(1).min(3);
                            next_liquid_run_at = now + Duration::from_millis(2_500);
                        } else if source == 2 {
                            w.set_play_test_request(true);
                        }
                    }
                }
                // Hover, panel, meeting, Adzan, atau saklar mati menghentikan trik baru,
                // tetapi badan kembali netral dengan redaman agar tidak tersentak.
                let dt = now.saturating_duration_since(last_frame).as_secs_f32().clamp(0.001, 0.05);
                last_frame = now;
                let steps = (dt / 0.008).ceil().clamp(1.0, 8.0) as u32;
                let h = dt / steps as f32;
                for _ in 0..steps {
                    spring_offset.1 += (-105.0 * spring_offset.0 - 17.0 * spring_offset.1) * h;
                    spring_offset.0 += spring_offset.1 * h;
                    spring_sx.1 += (-165.0 * spring_sx.0 - 20.0 * spring_sx.1) * h;
                    spring_sx.0 += spring_sx.1 * h;
                    spring_sy.1 += (-165.0 * spring_sy.0 - 20.0 * spring_sy.1) * h;
                    spring_sy.0 += spring_sy.1 * h;
                }
                let ox = spring_offset.0.clamp(-90.0, 90.0);
                let sx = (1.0 + spring_sx.0).clamp(0.78, 1.25);
                let sy = (1.0 + spring_sy.0).clamp(0.78, 1.45);
                let side = dock == 3 || dock == 4;
                w.set_play_offset(ox);
                w.set_play_body_scale_x(sx);
                w.set_play_body_scale_y(sy);
                w.set_eyes_offset_x(if side { 0.0 } else { ox * 0.10 });
                w.set_eyes_offset_y(if side { ox * 0.10 } else { 0.0 });
                w.set_eyes_scale_x(sx);
                w.set_eyes_scale_y(sy);
                if ox.abs() < 0.08 && spring_offset.1.abs() < 0.5
                    && spring_sx.0.abs() < 0.001 && spring_sy.0.abs() < 0.001 {
                    spring_offset = (0.0, 0.0);
                    spring_sx = (0.0, 0.0);
                    spring_sy = (0.0, 0.0);
                }
                // Poll ulang setelah 1,5 detik seperti blocked retry di Bottom Play Tauri.
                next_at = now + Duration::from_millis(1_500);
                return;
            }

            // Pilih gerakan dengan variasi jeda. Musik membuat Notch lebih ekspresif;
            // mood mengantuk memperlambat gerakan tanpa mematikan fitur.
            let test_requested = w.get_play_test_request();
            if liquid_repetitions_left == 0 && now >= next_liquid_at {
                liquid_repetitions_left = 3;
                next_liquid_run_at = now;
            }
            let liquid_due = liquid_repetitions_left > 0 && now >= next_liquid_run_at;
            if active.is_none() && (now >= next_at || test_requested || liquid_due) {
                seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
                let roll = (seed >> 32) as u32;
                let low_battery = matches!(battery.read(), Reading::Present(value) if value.percent <= 15 && !value.charging);
                let mood_state = eye.borrow();
                let calm = mood_state.mood.is_sleeping() || mood_state.mood.low_battery || low_battery;
                drop(mood_state);
                let playing = w.get_media_playing();
                // Tes manual dan jadwal tiga rangkaian Liquid mendahului gerakan acak.
                let source = if test_requested { 2 } else if liquid_due { 1 } else { 0 };
                let choice = if test_requested {
                    w.set_play_test_request(false);
                    0
                } else if liquid_due {
                    liquid_repetitions_left = liquid_repetitions_left.saturating_sub(1);
                    if liquid_repetitions_left == 0 {
                        next_liquid_at = now + Duration::from_secs(180);
                    }
                    0
                } else {
                    // Bobot sama dengan NORMAL/MUSIC/CALM di bottom-play.ts.
                    // Pilihan terakhir dikeluarkan agar gerakan tidak berulang berturut-turut.
                    let weights: [u32; 8] = if calm {
                        [1, 0, 0, 0, 2, 3, 0, 3] // liquid, squish, sway, yawn
                    } else if playing {
                        [7, 3, 3, 2, 1, 0, 5, 0]
                    } else {
                        [6, 4, 3, 2, 2, 2, 1, 1]
                    };
                    let mut available = weights;
                    if let Some(previous) = last_choice { available[previous as usize] = 0; }
                    let total: u32 = available.iter().sum();
                    let mut pick = roll % total.max(1);
                    let mut selected = 0u8;
                    for (index, weight) in available.iter().enumerate() {
                        if pick < *weight { selected = index as u8; break; }
                        pick -= *weight;
                    }
                    selected
                };
                last_choice = Some(choice);
                // Durasi gerak dan ekspresi mengikuti tabel MOOD serta urutan TRICKS Tauri.
                let (duration_ms, mood_ms) = match choice {
                    0 => (4_800, 1_900), // liquid: beberapa langkah aliran
                    1 => (4_200, 2_600), // ghost: berjalan, menjulang, lalu menyusut
                    2 => (4_500, 2_600), // roam: pergi dan kembali
                    3 => (900, 1_200),   // wiggle
                    4 => (1_700, 900),   // squish
                    5 => (3_000, 2_200), // sway
                    6 => (2_100, 1_800), // dance
                    _ => (1_600, 2_400), // yawn
                };
                // Ekspresi ikut gerakan dan kembali ke mood dasar melalui MoodState,
                // sehingga renderer mata tetap menjadi satu-satunya sumber gambar wajah.
                let animation_mood = match choice {
                    0 | 2 | 3 => Mood::Happy,
                    1 | 4 => Mood::Surprised,
                    5 => Mood::Suspicious,
                    6 => Mood::Love,
                    _ => Mood::Sleepy,
                };
                eye.borrow_mut().mood.flash(animation_mood, mood_ms, now);
                let direction = if roll & 1 == 0 { -1.0 } else { 1.0 };
                active = Some((choice, now, Duration::from_millis(duration_ms), direction, source));
            }

            let Some((kind, started, duration, direction, source)) = active else {
                // Kembali ke bentuk netral memakai pegas, bukan menghapus transform seketika.
                let dt = now.saturating_duration_since(last_frame).as_secs_f32().clamp(0.001, 0.05);
                last_frame = now;
                for _ in 0..2 {
                    let h = dt * 0.5;
                    spring_offset.1 += (-105.0 * spring_offset.0 - 17.0 * spring_offset.1) * h;
                    spring_offset.0 += spring_offset.1 * h;
                    spring_sx.1 += (-165.0 * spring_sx.0 - 20.0 * spring_sx.1) * h;
                    spring_sx.0 += spring_sx.1 * h;
                    spring_sy.1 += (-165.0 * spring_sy.0 - 20.0 * spring_sy.1) * h;
                    spring_sy.0 += spring_sy.1 * h;
                }
                let ox = spring_offset.0.clamp(-90.0, 90.0);
                let sx = (1.0 + spring_sx.0).clamp(0.78, 1.25);
                let sy = (1.0 + spring_sy.0).clamp(0.78, 1.45);
                let side = dock == 3 || dock == 4;
                w.set_play_offset(ox);
                w.set_play_body_scale_x(sx);
                w.set_play_body_scale_y(sy);
                w.set_eyes_offset_x(if side { 0.0 } else { ox * 0.10 });
                w.set_eyes_offset_y(if side { ox * 0.10 } else { 0.0 });
                w.set_eyes_scale_x(sx);
                w.set_eyes_scale_y(sy);
                if ox.abs() < 0.08 && spring_offset.1.abs() < 0.5
                    && spring_sx.0.abs() < 0.001 && spring_sy.0.abs() < 0.001 {
                    spring_offset = (0.0, 0.0);
                    spring_sx = (0.0, 0.0);
                    spring_sy = (0.0, 0.0);
                }
                return;
            };

            let elapsed = now.saturating_duration_since(started).as_secs_f32();
            let total = duration.as_secs_f32().max(0.001);
            let t = (elapsed / total).clamp(0.0, 1.0);
            if t >= 1.0 {
                active = None;
                if kind == 0 && source == 1 && liquid_repetitions_left > 0 {
                    seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
                    next_liquid_run_at = now + Duration::from_secs(7 + u64::from((seed >> 32) as u32 % 15));
                }
                let low_battery = matches!(battery.read(), Reading::Present(value) if value.percent <= 15 && !value.charging);
                let mood_state = eye.borrow();
                let calm = mood_state.mood.is_sleeping() || mood_state.mood.low_battery || low_battery;
                drop(mood_state);
                let playing = w.get_media_playing();
                seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
                let fraction = ((seed >> 32) as u32) as f32 / u32::MAX as f32;
                // Jeda antar-gerakan sama dengan schedule() versi Tauri:
                // calm 10–18s, musik 5–9,5s, normal 7–14s.
                let delay = if calm { 10.0 + fraction * 8.0 }
                    else if playing { 5.0 + fraction * 4.5 }
                    else { 7.0 + fraction * 7.0 };
                next_at = now + Duration::from_secs_f32(delay);
                return;
            }

            // Envelope halus: setiap gerakan masuk dan kembali ke posisi netral tanpa hentakan.
            let envelope = (std::f32::consts::PI * t).sin().max(0.0);
            let wave = (std::f32::consts::TAU * t).sin();
            let fast_wave = (std::f32::consts::TAU * 3.0 * t).sin();
            let mut offset = 0.0;
            let mut sx = 1.0;
            let mut sy = 1.0;

            // Gerakan perjalanan memakai beberapa titik tujuan yang dibatasi di dalam
            // area transform Notch. Interpolasi smoothstep menghindari perpindahan patah.
            let smooth = |v: f32| {
                let v = v.clamp(0.0, 1.0);
                v * v * (3.0 - 2.0 * v)
            };
            let random_offset = |index: u32, limit: f32| {
                let mut value = seed
                    .wrapping_add(u64::from(index).wrapping_mul(0x9E37_79B9_7F4A_7C15));
                value ^= value >> 30;
                value = value.wrapping_mul(0xBF58_476D_1CE4_E5B9);
                value ^= value >> 27;
                value = value.wrapping_mul(0x94D0_49BB_1331_11EB);
                value ^= value >> 31;
                let unit = (value >> 40) as f32 / ((1u32 << 24) - 1) as f32;
                (unit * 2.0 - 1.0) * limit
            };
            let travel_segment = |progress: f32, count: u32, limit: f32, return_home: bool| {
                let scaled = progress.clamp(0.0, 1.0) * count as f32;
                let index = (scaled.floor() as u32).min(count.saturating_sub(1));
                let local = smooth(scaled - index as f32);
                let from = if index == 0 { 0.0 } else { random_offset(index, limit) };
                let to = if return_home && index + 1 == count {
                    0.0
                } else {
                    random_offset(index + 1, limit)
                };
                from + (to - from) * local
            };

            match kind {
                0 => { // LIQUID: 3–6 langkah pendek dengan arah dan jarak bervariasi.
                    let steps = 3 + (seed as u32 % 4);
                    offset = travel_segment(t, steps, 56.0, true);
                    sx += 0.055 * fast_wave * envelope;
                    sy -= 0.045 * fast_wave * envelope;
                    let ripple = (std::f32::consts::TAU * 5.0 * t).sin() * envelope;
                    sx += 0.035 * ripple;
                    sy -= 0.03 * ripple;
                }
                1 => { // GHOST: meluncur pelan sambil menjulang, lalu menyusut.
                    let lift = (std::f32::consts::PI * t).sin().max(0.0);
                    sx = 1.0 - 0.13 * lift;
                    sy = 1.0 + 0.38 * lift;
                    offset = direction * 42.0 * smooth(t);
                }
                2 => { // ROAM: satu atau dua perjalanan, kemudian kembali ke titik awal.
                    let legs = if seed & 1 == 0 { 2 } else { 3 };
                    offset = travel_segment(t, legs, 78.0, true);
                    sx += 0.035 * envelope;
                    sy -= 0.025 * envelope;
                }
                3 => { // WIGGLE: getaran berirama yang tetap lembut.
                    offset = 10.0 * fast_wave * envelope;
                    sx += 0.045 * fast_wave * envelope;
                    sy -= 0.035 * fast_wave * envelope;
                }
                4 => { // SQUISH: gepeng-memanjang bergantian.
                    let pulse = (std::f32::consts::TAU * 2.5 * t).sin() * envelope;
                    sx += 0.12 * pulse;
                    sy -= 0.10 * pulse;
                }
                5 => { // SWAY: ayunan lambat.
                    offset = 9.0 * wave * envelope;
                    sx += 0.025 * wave * envelope;
                    sy -= 0.02 * wave * envelope;
                }
                6 => { // DANCE: dua ketukan cepat saat musik aktif.
                    let beat = (std::f32::consts::TAU * 4.0 * t).sin();
                    offset = 13.0 * beat * envelope;
                    sx += 0.07 * beat * envelope;
                    sy -= 0.055 * beat * envelope;
                }
                _ => { // YAWN: meregang panjang, menahan, lalu lemas.
                    let stretch = (std::f32::consts::PI * t).sin().max(0.0);
                    sx = 1.0 - 0.07 * stretch;
                    sy = 1.0 + 0.22 * stretch;
                }
            }

            // Di dock samping, sumbu offset tetap mengikuti tepi secara vertikal.
            // Skala badan dan wajah dikendalikan bersama agar ekspresi tidak terlepas dari bentuk.
            let side = dock == 3 || dock == 4;
            // Spring step dengan substep 8 ms menjaga kestabilan ketika frame melambat.
            let dt = now.saturating_duration_since(last_frame).as_secs_f32().clamp(0.001, 0.05);
            last_frame = now;
            let target_x = offset.clamp(-90.0, 90.0);
            let target_sx = (sx - 1.0).clamp(-0.22, 0.25);
            let target_sy = (sy - 1.0).clamp(-0.22, 0.45);
            let steps = (dt / 0.008).ceil().clamp(1.0, 8.0) as u32;
            let h = dt / steps as f32;
            for _ in 0..steps {
                spring_offset.1 += (105.0 * (target_x - spring_offset.0) - 17.0 * spring_offset.1) * h;
                spring_offset.0 += spring_offset.1 * h;
                spring_sx.1 += (165.0 * (target_sx - spring_sx.0) - 20.0 * spring_sx.1) * h;
                spring_sx.0 += spring_sx.1 * h;
                spring_sy.1 += (165.0 * (target_sy - spring_sy.0) - 20.0 * spring_sy.1) * h;
                spring_sy.0 += spring_sy.1 * h;
            }
            let body_x = (1.0 + spring_sx.0).clamp(0.78, 1.25);
            let body_y = (1.0 + spring_sy.0).clamp(0.78, 1.45);
            let body_offset = spring_offset.0.clamp(-90.0, 90.0);
            w.set_play_offset(body_offset);
            w.set_play_body_scale_x(body_x);
            w.set_play_body_scale_y(body_y);
            w.set_eyes_offset_x(if side { 0.0 } else { body_offset * 0.10 });
            w.set_eyes_offset_y(if side { body_offset * 0.10 } else { 0.0 });
            w.set_eyes_scale_x(body_x);
            w.set_eyes_scale_y(body_y);
        });
    }

    {
        let weak = win.as_weak();
        let selected_paths = convert.selected_paths.clone();
        let last_output = convert.last_output.clone();
        let running = convert.running.clone();
        let preview_page = convert.preview_page.clone();
        let preview_path = convert.preview_path.clone();
        let weak_preview = weak.clone();
        winit_handlers.borrow_mut().push(Box::new(move |event| {
            if let winit::event::WindowEvent::DroppedFile(path) = event {
                // File lepas ke Shelf kecuali halaman Convert terbuka (lihat shelf_accepts_drop).
                if weak.upgrade().is_some_and(|w| shelf_accepts_drop(&w)) {
                    return;
                }
                if running.load(std::sync::atomic::Ordering::Acquire) {
                    if let Some(w) = weak.upgrade() {
                        w.set_convert_status("Selesaikan atau batalkan batch sebelum menambahkan file".into());
                    }
                    return;
                }
                if !path.is_file() {
                    return;
                }
                if convert::available_targets(path).is_empty() {
                    if let Some(w) = weak.upgrade() {
                        w.set_convert_status(format!("Format file tidak didukung: {}", path.display()).into());
                    }
                    return;
                }
                let raw = path.to_string_lossy().to_string();
                let mut selected = selected_paths.borrow_mut();
                if !selected.iter().any(|existing| existing.eq_ignore_ascii_case(&raw)) {
                    selected.push(raw.clone());
                }
                let paths = selected.clone();
                drop(selected);
                *last_output.lock().unwrap_or_else(|error| error.into_inner()) = None;
                if let Some(w) = weak.upgrade() {
                    w.set_convert_selected_path(paths.first().cloned().unwrap_or(raw).into());
                    w.set_convert_files_text(format_convert_selection(&paths).into());
                    if let Some(first) = paths.first() {
                        preview_page.set(0);
                        *preview_path.borrow_mut() = first.clone();
                        w.set_convert_preview_page_text("Memuat preview…".into());
                        request_convert_document_preview(weak_preview.clone(), first.clone(), 0, convert.preview_dpi.get());
                    }
                    w.set_convert_result_preview(Image::default());
                    w.set_convert_history_text(format_convert_history().into());
                    w.set_convert_progress_text("".into());
                    w.set_convert_status(format!("{} file siap dikonversi · file diterima dari Explorer", paths.len()).into());
                }
            }
        }));
    }

    // Sapaan otomatis native saat aplikasi mulai. Timer hidup selama event loop Slint berjalan.
    let startup_greeting_timer = Timer::default();
    let startup_greeting_hide_timer = Rc::new(Timer::default());
    if cfg.greeting.enabled {
        let weak = win.as_weak();
        let hide_timer = startup_greeting_hide_timer.clone();
        let shell = shell.clone();
        let greeting = speech::greeting_content(&cfg.greeting.mode, &cfg.greeting.custom_text, platform::local_time().0);
        let voice = cfg.greeting.voice.clone();
        startup_greeting_timer.start(TimerMode::SingleShot, Duration::from_millis(1_800), move || {
            let Some(w) = weak.upgrade() else { return };
            if w.get_meeting_active() || w.get_adhan_active() || w.get_expanded() { return; }
            if let Some(greeting) = greeting.as_ref() {
                w.set_greeting_message(greeting.message.clone().into());
                w.set_greeting_detail(greeting.detail.clone().into());
                // Greeting memakai kartu inline di dock atas dan speech bubble di dock bawah/samping.
                if w.get_dock() < 2 {
                    w.set_greeting_speech_visible(false);
                    w.set_nudge_visible(false);
                    show_greeting_inline(weak.clone(), shell.clone());
                } else {
                    w.set_greeting_inline_visible(false);
                    w.set_nudge_visible(false);
                    w.set_greeting_speech_visible(true);
                    shell.place(&w, COLLAPSED);
                }
                let greeting_duration_ms = speech::estimated_duration_ms(&greeting.spoken);
                let _ = speech::speak(greeting.spoken.clone(), voice.clone());
                let weak_hide = weak.clone();
                let shell_hide = shell.clone();
                hide_timer.start(TimerMode::SingleShot, Duration::from_millis(greeting_duration_ms + 600), move || {
                    if let Some(w) = weak_hide.upgrade() {
                        let was_inline = w.get_greeting_inline_visible() && !w.get_expanded();
                        w.set_greeting_inline_visible(false);
                        if was_inline {
                            shrink_window_synced(weak_hide.clone(), shell_hide.clone());
                        }
                        w.set_greeting_speech_visible(false);
                        w.set_nudge_visible(false);
                        if w.get_dock() >= 2 && !w.get_expanded() {
                            shell_hide.place(&w, COLLAPSED);
                        }
                    }
                });
            }
        });
    }

    // Koreksi geometri setelah native Winit window siap; shell.place() pertama terjadi sebelum event loop.
    {
        let weak = win.as_weak();
        let shell = shell.clone();
        Timer::single_shot(Duration::from_millis(350), move || {
            if let Some(w) = weak.upgrade() {
                shell.memo.set((None, 0));
                let size = if w.get_expanded() { expanded_size(&w) } else { COLLAPSED };
                shell.place(&w, size);
                if w.get_idle_clock() && !w.get_expanded() {
                    w.set_geometry_hold(false);
                }
                platform::hide_from_taskbar(w.window());
            }
        });
    }

    // Sembunyikan dari taskbar. Jendela winit baru bisa diakses setelah event loop berjalan, jadi diterapkan dari timer;
    // diulang beberapa kali karena shell Windows membuat tombol taskbar secara asinkron.
    for delay_ms in [100u64, 400, 1500] {
        let weak = win.as_weak();
        Timer::single_shot(Duration::from_millis(delay_ms), move || {
            if let Some(w) = weak.upgrade() {
                platform::hide_from_taskbar(w.window());
            }
        });
    }

    // --- Pengaturan: cari kota, tes adzan, mulai ulang (setara jendela Pengaturan Tauri) ---
    {
        let weak = win.as_weak();
        win.on_settings_search_city(move |query| {
            let query = query.trim().to_string();
            let Some(w) = weak.upgrade() else { return };
            if query.chars().count() < 2 {
                w.set_weather_hits(ModelRc::new(VecModel::<CityHit>::default()));
                w.set_weather_search_status("Ketik minimal 2 huruf nama kota.".into());
                return;
            }
            w.set_weather_search_status("Mencari...".into());
            let weak = weak.clone();
            std::thread::spawn(move || {
                let result = search_cities(&query);
                let _ = weak.upgrade_in_event_loop(move |w| match result {
                    Ok(hits) => {
                        let status = if hits.is_empty() {
                            "Kota tidak ditemukan. Coba ejaan lain atau isi koordinat manual."
                        } else {
                            "Pilih salah satu hasil."
                        };
                        let rows: Vec<CityHit> = hits
                            .into_iter()
                            .map(|(label, name, lat, lon)| CityHit { label: label.into(), name: name.into(), lat: lat.into(), lon: lon.into() })
                            .collect();
                        w.set_weather_hits(ModelRc::new(VecModel::from(rows)));
                        w.set_weather_search_status(status.into());
                    }
                    Err(error) => {
                        w.set_weather_hits(ModelRc::new(VecModel::<CityHit>::default()));
                        w.set_weather_search_status(format!("Pencarian gagal: {error}").into());
                    }
                });
            });
        });
        let weak = win.as_weak();
        win.on_settings_test_adhan(move || media::play_adhan(weak.clone(), "Tes".to_string()));
        win.on_settings_pause_adhan(|pause| media::pause_adhan(pause));
        win.on_settings_restart(|| {
            RESTART_REQUESTED.store(true, Ordering::SeqCst);
            let _ = slint::quit_event_loop();
        });
    }

    // --- Tray: Pengaturan, Pusatkan Notch, Keluar (sama dengan tray versi Tauri) ---
    let tray_handle: Rc<RefCell<Option<tray::Tray>>> = Rc::new(RefCell::new(None));
    let tray_retry = Rc::new(Timer::default());
    {
        let weak = win.as_weak();
        let open_settings = open_settings.clone();
        let on_action: Rc<dyn Fn(tray::Action)> = Rc::new(move |action| {
            let Some(w) = weak.upgrade() else { return };
            match action {
                tray::Action::Settings => open_settings(),
                // Jalur yang sama dengan pilihan posisi "top" di Pengaturan: disimpan ke config seperti di Tauri.
                tray::Action::Center => w.invoke_settings_save_position("top".into()),
                tray::Action::Quit => {
                    let _ = slint::quit_event_loop();
                }
            }
        });
        let try_install = {
            let holder = tray_handle.clone();
            move || -> bool {
                if holder.borrow().is_some() {
                    return true;
                }
                let on_action = on_action.clone();
                match tray::install(move |action| on_action(action)) {
                    Ok(installed) => {
                        *holder.borrow_mut() = Some(installed);
                        true
                    }
                    Err(error) => {
                        eprintln!("tray gagal dibuat (pengaturan tidak bisa dibuka lewat tray): {error}");
                        false
                    }
                }
            }
        };
        // Saat berjalan otomatis ketika login, tray kadang belum siap: coba lagi tiap 3 detik (maksimal 20 kali).
        if tray::SUPPORTED && !try_install() {
            let tries = Cell::new(0u32);
            let timer = Rc::downgrade(&tray_retry);
            tray_retry.start(TimerMode::Repeated, Duration::from_secs(3), move || {
                tries.set(tries.get() + 1);
                if try_install() || tries.get() >= 20 {
                    if let Some(timer) = timer.upgrade() {
                        timer.stop();
                    }
                }
            });
        }
    }

    let result = win.run();
    if RESTART_REQUESTED.load(Ordering::SeqCst) {
        // Lepas jendela dan tray lebih dulu supaya hotkey/ikon lama tidak bentrok dengan proses baru.
        drop(win);
        tray_handle.borrow_mut().take();
        match std::env::current_exe().and_then(|exe| std::process::Command::new(exe).spawn()) {
            Ok(_) => {}
            Err(error) => eprintln!("gagal memulai ulang Notch: {error}"),
        }
    }
    result
}
