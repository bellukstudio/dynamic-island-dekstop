//! Halaman Pengingat: pemilihan hari dan daftar pengingat.

use crate::*;

pub(crate) fn weekday_selection(mask: u8) -> Vec<bool> {
    (0..7).map(|day| mask & (1 << day) != 0).collect()
}

pub(crate) fn show_reminders(model: &VecModel<ReminderItem>, reminders: Vec<Reminder>, offset: i32) {
    let items = reminders.into_iter().map(|r| {
        let due = if r.due_at <= 0 {
            "Tanpa waktu".to_string()
        } else {
            let (year, month, day, hour, minute) = reminder::local_parts(r.due_at, offset);
            let when = format!("{day:02}/{month:02}/{year} {hour:02}:{minute:02}");
            let repeat = match reminder::effective_repeat_mode(&r) {
                "daily" => " · Harian",
                "weekly" => " · Mingguan",
                "monthly" => " · Bulanan",
                _ => "",
            };
            format!("{when}{repeat}")
        };
        ReminderItem { id: r.id.into(), text: r.text.into(), due: due.into(), done: r.done }
    }).collect::<Vec<_>>();
    model.set_vec(items);
}
