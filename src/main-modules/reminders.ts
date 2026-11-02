// @ts-nocheck
// Pengingat, audio alarm, dan date picker.
// ---------- pengingat (reminder) ----------
type RepeatMode = "none" | "daily" | "weekly" | "monthly";
type Reminder = {
    id: string;
    text: string;
    due_at: number;
    done: boolean;
    notified: boolean;
    repeat_mode?: RepeatMode | string;
    repeat_daily?: boolean; // kompatibilitas dengan data lama
    repeat_anchor_at: number;
    repeat_day_of_month?: number;
    repeat_timezone_offset_minutes?: number;
    repeat_weekdays?: number; // bitmask: bit 0 = Minggu ... bit 6 = Sabtu (Date.getDay)
};

export function register(ctx: Record<string, any>): void {
    ctx.beep = function beep() {
        try {
            ctx.audioCtx ??= new AudioContext();
            const ctx = ctx.audioCtx;
            if (ctx.state === "suspended")
                void ctx.resume();
            const t = ctx.currentTime;
            [0, 0.2].forEach((off) => {
                const o = ctx.createOscillator();
                const g = ctx.createGain();
                o.type = "sine";
                o.frequency.value = 880;
                g.gain.setValueAtTime(0.0001, t + off);
                g.gain.exponentialRampToValueAtTime(0.25, t + off + 0.02);
                g.gain.exponentialRampToValueAtTime(0.0001, t + off + 0.15);
                o.connect(g).connect(ctx.destination);
                o.start(t + off);
                o.stop(t + off + 0.16);
            });
        }
        catch { /* audio tidak tersedia: kartu tetap tampil */ }
    };
    // Lebar kartu pengingat mengikuti panjang teks (min 340, maks 480). Lebih panjang dari itu teks pindah
    // baris (maks 3 baris) dan kartu bertambah tinggi. Dulu dilakukan fitRemCard di main.ts, hilang saat refactor.
    const REM_MIN_W = 340, REM_MAX_W = 480, REM_MIN_H = 74, REM_MAX_LINES = 3;
    ctx.fitRemCard = function fitRemCard(text: string, sub: string) {
        const titleEl = ctx.$("rem-text");
        const subEl = ctx.$("rem-sub");
        ctx.remMeasureCtx ??= document.createElement("canvas").getContext("2d");
        const g = ctx.remMeasureCtx;
        const fontOf = (el: HTMLElement) => { const cs = getComputedStyle(el); return `${cs.fontWeight} ${cs.fontSize} ${cs.fontFamily}`; };
        const titleFont = fontOf(titleEl);
        const subFont = fontOf(subEl);
        const measure = (s: string, font: string) => { if (!g) return s.length * 8; g.font = font; return g.measureText(s).width; };
        // lebar selain teks: padding 2x16 + ikon 42 + 2 jarak 12 + tombol (diukur; cadangan 132) + kelonggaran
        const btnW = ["rem-snooze", "rem-stop"].reduce((a, id) => a + (document.getElementById(id)?.offsetWidth || 0), 0);
        const overhead = 32 + 42 + 24 + (btnW > 0 ? btnW + 8 : 132) + 10;
        const screenMax = Math.floor(ctx.MON.w / ctx.CFG.scale) - 60;
        const maxW = Math.max(REM_MIN_W, Math.min(REM_MAX_W, screenMax));
        const textW = measure(text, titleFont);
        const need = Math.max(textW, measure(sub, subFont)) + overhead;
        const width = Math.round(ctx.clamp(need, REM_MIN_W, maxW));
        const inner = width - overhead;
        // hitung jumlah baris: bungkus per kata, kata yang lebih panjang dari baris dipecah per huruf
        let lines = 1;
        if (textW > inner) {
            const space = measure(" ", titleFont);
            let used = 0;
            lines = 1;
            for (const word of text.split(/\s+/).filter(Boolean)) {
                const w = measure(word, titleFont);
                if (w > inner) {
                    for (const ch of word) {
                        const cw = measure(ch, titleFont);
                        if (used + cw > inner && used > 0) { lines++; used = 0; }
                        used += cw;
                    }
                    used += space;
                    continue;
                }
                if (used > 0 && used + w > inner) { lines++; used = 0; }
                used += w + space;
            }
            lines = Math.min(REM_MAX_LINES, lines);
        }
        const height = lines === 1 ? REM_MIN_H : Math.max(REM_MIN_H, Math.ceil(30 + lines * 17.5 + 14));
        ctx.notch.style.setProperty("--rem-w", `${width}px`);
        ctx.notch.style.setProperty("--rem-h", `${height}px`);
        ctx.notch.classList.toggle("rem-wrap", lines > 1);
        // sama seperti compactLyrics: ukuran dasar diperbarui lalu diskalakan; +20/+6 = ruang telinga notch
        ctx.BASE_SIZES.rem = [width + 20, height + 6];
        ctx.BASE_HIT.rem = [width, height];
        ctx.SIZES.rem = [Math.round((width + 20) * ctx.CFG.scale), Math.round((height + 6) * ctx.CFG.scale)];
        ctx.HIT.rem = [Math.round(width * ctx.CFG.scale), Math.round(height * ctx.CFG.scale)];
        if (ctx.notch.dataset.view === "rem") {
            ctx.placeShell("rem");
            ctx.setHit("rem");
        }
    };
    ctx.showRing = function showRing() {
        const list = [...ctx.ringing.values()];
        if (list.length === 0) {
            if (ctx.remUsesBubble) {
                ctx.remUsesBubble = false;
                ctx.momentBubble?.hideSticky();
            }
            if (ctx.override === "rem")
                ctx.collapse();
            return;
        }
        const r = list[0];
        const sub = list.length > 1
            ? `Pengingat · +${list.length - 1} lagi`
            : `Pengingat${r.due_at ? " · " + ctx.fmtDue(r.due_at) : ""}`;
        ctx.hubByHover = false;
        // Notch menempel di sisi/dasar layar: pengingat tampil sebagai awan percakapan (lengkap dengan
        // tombol Tunda/Selesai), bukan kartu lebar. Awan adzan memakai slot yang sama, jadi saat adzan
        // berbunyi atau kartu sudah tampil, tetap pakai kartu.
        if (!ctx.adhanUsesBubble && ctx.override !== "rem") {
            ctx.remUsesBubble = ctx.momentBubble?.showSticky(r.text, sub, {
                actions: [
                    { label: "Tunda", onClick: () => ctx.endRing("snooze") },
                    { label: "Selesai", primary: true, onClick: () => ctx.endRing("stop") },
                ],
                // notch dipindah dari sisi/dasar ke atas saat berbunyi: awan hilang, ganti jadi kartu
                onLost: () => { ctx.remUsesBubble = false; if (ctx.ringing.size) ctx.showRing(); },
            }) === true;
            if (ctx.remUsesBubble)
                return;
        }
        ctx.remUsesBubble = false;
        ctx.$("rem-text").textContent = r.text;
        ctx.$("rem-sub").textContent = sub;
        ctx.fitRemCard(r.text, sub);
        ctx.expand("rem", 0); // tanpa timer: tidak hilang sendiri
    };
    ctx.endRing = function endRing(action: "stop" | "snooze") {
        const r = [...ctx.ringing.values()][0];
        if (!r)
            return;
        ctx.ringing.delete(r.id);
        const call = action === "stop"
            ? ctx.invoke<Reminder[]>("reminders_stop", { id: r.id })
            : ctx.invoke<Reminder[]>("reminders_snooze", { id: r.id, minutes: 5 });
        call.then(ctx.paintReminders).catch(() => { });
        ctx.flash(action === "stop" ? "happy" : "sleepy", 1200);
        ctx.showRing(); // tampilkan pengingat berikutnya, atau tutup kalau sudah habis
    };
    ctx.paintReminders = function paintReminders(items: Reminder[]) {
        // sinkronkan kartu: pengingat yang diselesaikan/dihapus/ditunda dari menu ikut hilang
        let changed = false;
        for (const id of [...ctx.ringing.keys()]) {
            const it = items.find((r) => r.id === id);
            if (!it || it.done || it.due_at > Date.now()) {
                ctx.ringing.delete(id);
                changed = true;
            }
        }
        if (changed)
            ctx.showRing();
        const list = document.getElementById("rm-list");
        if (!list)
            return; // halaman reminder dimatikan di config
        const left = items.filter((r) => !r.done).length;
        ctx.$("rm-count").textContent = left ? String(left) : "";
        list.replaceChildren();
        if (items.length === 0) {
            const empty = document.createElement("div");
            empty.className = "rm-empty";
            empty.textContent = "Belum ada pengingat";
            list.appendChild(empty);
            return;
        }
        const now = Date.now();
        items.forEach((r) => {
            const overdue = !r.done && r.due_at > 0 && r.due_at <= now;
            const row = document.createElement("button"); // button: tidak memicu geser halaman
            row.className = "rm-item" + (r.done ? " done" : "") + (overdue ? " overdue" : "");
            row.title = r.text;
            const dot = document.createElement("span");
            dot.className = "dot";
            const txt = document.createElement("span");
            txt.className = "txt";
            txt.textContent = r.text;
            row.append(dot, txt);
            if (r.due_at) {
                const due = document.createElement("span");
                due.className = "due";
                const mode: RepeatMode = r.repeat_mode === "monthly"
                    ? "monthly"
                    : r.repeat_mode === "weekly" && (r.repeat_weekdays || 0) !== 0
                        ? "weekly"
                        : (r.repeat_mode === "daily" || r.repeat_daily ? "daily" : "none");
                const repeatLabel = mode === "daily"
                    ? " · ↻ Harian"
                    : mode === "weekly"
                        ? ` · ↻ ${ctx.weekdaysLabel(r.repeat_weekdays || 0)}`
                        : mode === "monthly"
                            ? ` · ↻ Tanggal ${r.repeat_day_of_month || new Date(r.repeat_anchor_at || r.due_at).getDate()}/bln`
                            : "";
                due.textContent = `${ctx.fmtDue(r.due_at)}${repeatLabel}`;
                row.appendChild(due);
            }
            const x = document.createElement("span");
            x.className = "x";
            x.textContent = "×";
            row.appendChild(x);
            row.addEventListener("click", (e) => {
                e.stopPropagation();
                if ((e.target as HTMLElement).closest(".x")) {
                    ctx.invoke<Reminder[]>("reminders_remove", { id: r.id }).then(ctx.paintReminders).catch(() => { });
                }
                else {
                    ctx.invoke<Reminder[]>("reminders_toggle", { id: r.id }).then(ctx.paintReminders).catch(() => { });
                    if (!r.done)
                        ctx.flash("happy", 900);
                }
            });
            list.appendChild(row);
        });
    };
    ctx.refreshReminders = function refreshReminders() {
        ctx.invoke<Reminder[]>("reminders_list").then(ctx.paintReminders).catch(() => { });
    };
    ctx.paintWhen = function paintWhen() {
        ctx.$("rm-when-txt").textContent = ctx.pickAt ? ctx.fmtDue(ctx.pickAt) : "";
        ctx.$("rm-when").classList.toggle("set", ctx.pickAt !== 0);
    };
    const DAY_SHORT = ["Min", "Sen", "Sel", "Rab", "Kam", "Jum", "Sab"];
    const DEFAULT_DAYS = [1, 2, 3, 4, 5]; // Senin-Jumat
    ctx.DEFAULT_REPEAT_DAYS = DEFAULT_DAYS;
    ctx.daysToMask = (days: number[]): number => days.reduce((m, d) => m | (1 << d), 0) & 0x7f;
    ctx.weekdaysLabel = function weekdaysLabel(mask: number): string {
        const days = [1, 2, 3, 4, 5, 6, 0].filter((d) => mask & (1 << d)); // urut Senin..Minggu
        if (days.length === 7)
            return "Setiap hari";
        if (days.length === 5 && !(mask & 1) && !(mask & (1 << 6)))
            return "Hari kerja";
        if (days.length === 2 && (mask & 1) && (mask & (1 << 6)))
            return "Akhir pekan";
        return days.map((d) => DAY_SHORT[d]).join(", ");
    };
    ctx.paintRepeatDays = function paintRepeatDays() {
        const weekly = ctx.repeatMode === "weekly";
        const row = document.getElementById("pk-days");
        if (row) {
            row.hidden = !weekly;
            row.classList.remove("invalid");
            row.querySelectorAll<HTMLButtonElement>("button[data-day]").forEach((b) => {
                const on = ctx.repeatDays.includes(Number(b.dataset.day));
                b.classList.toggle("on", on);
                b.setAttribute("aria-pressed", String(on));
            });
        }
        const changed = ctx.notch.classList.contains("reminder-weekly") !== weekly;
        ctx.notch.classList.toggle("reminder-weekly", weekly);
        if (changed)
            ctx.syncHubDynamicHeight?.();
    };
    ctx.paintRepeatMode = function paintRepeatMode() {
        const select = document.getElementById("pk-repeat-mode") as HTMLSelectElement | null;
        if (select)
            select.value = ctx.repeatMode;
        ctx.paintRepeatDays();
    };
    ctx.formatDmy = function formatDmy(date: Date): string {
        const dd = String(date.getDate()).padStart(2, "0");
        const mm = String(date.getMonth() + 1).padStart(2, "0");
        const yyyy = String(date.getFullYear()).padStart(4, "0");
        return `${dd}/${mm}/${yyyy}`;
    };
    ctx.formatDmyInput = function formatDmyInput(value: string): string {
        const digits = value.replace(/\D/g, "").slice(0, 8);
        if (digits.length < 2)
            return digits;
        if (digits.length === 2)
            return `${digits}/`;
        if (digits.length < 4)
            return `${digits.slice(0, 2)}/${digits.slice(2)}`;
        if (digits.length === 4)
            return `${digits.slice(0, 2)}/${digits.slice(2, 4)}/`;
        return `${digits.slice(0, 2)}/${digits.slice(2, 4)}/${digits.slice(4)}`;
    };
    ctx.parseDmy = function parseDmy(value: string): {
        year: number;
        month: number;
        day: number;
    } | null {
        const match = /^(\d{2})\/(\d{2})\/(\d{4})$/.exec(value.trim());
        if (!match)
            return null;
        const day = Number(match[1]);
        const month = Number(match[2]);
        const year = Number(match[3]);
        if (year < 1000 || month < 1 || month > 12 || day < 1 || day > 31)
            return null;
        const check = new Date(year, month - 1, day);
        if (check.getFullYear() !== year || check.getMonth() !== month - 1 || check.getDate() !== day)
            return null;
        return { year, month, day };
    };
    ctx.setPkDateFromInput = function setPkDateFromInput(): boolean {
        if (!ctx.pkDateInput)
            return false;
        const parsed = ctx.parseDmy(ctx.pkDateInput.value);
        if (!parsed)
            return false;
        // Konstruksi ulang tanggal agar tanggal 31 tidak overflow saat berpindah bulan.
        ctx.pk = new Date(parsed.year, parsed.month - 1, parsed.day, ctx.pk.getHours(), ctx.pk.getMinutes(), 0, 0);
        return true;
    };
    ctx.paintPick = function paintPick() {
        ctx.$("pk-h").textContent = String(ctx.pk.getHours()).padStart(2, "0");
        ctx.$("pk-m").textContent = String(ctx.pk.getMinutes()).padStart(2, "0");
    };
    ctx.syncHubDynamicHeight = function syncHubDynamicHeight() {
        const reminderExpanded = ctx.notch.classList.contains("reminder-picking");
        const notesExpanded = ctx.notch.classList.contains("notes-expanded");
        const currencyExpanded = ctx.names[ctx.page] === "currency";
        const ramadanExpanded = ctx.names[ctx.page] === "prayer" && ctx.notch.classList.contains("ramadan-enabled");
        const width = (currencyExpanded || ramadanExpanded) ? 780 : ctx.BASE_SIZES.hub[0];
        const weeklyExpanded = reminderExpanded && ctx.notch.classList.contains("reminder-weekly");
        const height = ramadanExpanded ? 540 : reminderExpanded ? (weeklyExpanded ? 270 : 230) : notesExpanded ? 300 : currencyExpanded ? 520 : ctx.BASE_SIZES.hub[1];
        const hitWidth = (currencyExpanded || ramadanExpanded) ? 770 : ctx.BASE_HIT.hub[0];
        const hitHeight = ramadanExpanded ? 580 : reminderExpanded ? (weeklyExpanded ? 282 : 242) : notesExpanded ? 330 : currencyExpanded ? 560 : ctx.BASE_HIT.hub[1];
        ctx.SIZES.hub[0] = Math.round(width * ctx.CFG.scale);
        ctx.SIZES.hub[1] = Math.round(height * ctx.CFG.scale);
        ctx.HIT.hub[0] = Math.round(hitWidth * ctx.CFG.scale);
        ctx.HIT.hub[1] = Math.round(hitHeight * ctx.CFG.scale);
        // Variabel CSS ditempatkan langsung pada notch supaya track, page, dan nav ikut melebar.
        // "ramadan-expanded" harus ikut berpindah halaman; kalau tertinggal, CSS (width: 780px !important)
        // tetap melebarkan hub padahal jendela native sudah kembali ke ukuran normal.
        ctx.notch.classList.toggle("ramadan-expanded", ramadanExpanded);
        ctx.notch.classList.toggle("currency-expanded", currencyExpanded || ramadanExpanded);
        ctx.notch.style.setProperty("--hub-w", `${width}px`);
        ctx.notch.style.setProperty("--hub-h", `${height}px`);
        if (ctx.notch.dataset.view === "hub") {
            void ctx.syncNativeWindowSize("hub").then(() => {
                if (ctx.notch.dataset.view !== "hub")
                    return;
                ctx.placeShell("hub");
                ctx.setHit("hub");
            }).catch((error) => console.error("Gagal mengubah ukuran panel Currency:", error));
        }
    };
    ctx.setReminderPickerExpanded = function setReminderPickerExpanded(expanded: boolean) {
        ctx.notch.classList.toggle("reminder-picking", expanded);
        ctx.syncHubDynamicHeight();
    };
    ctx.openPick = function openPick() {
        if (ctx.pickAt) {
            ctx.pk = new Date(ctx.pickAt);
            if (ctx.pkDateInput)
                ctx.pkDateInput.value = ctx.formatDmy(ctx.pk);
        }
        else {
            // Tanggal default selalu hari ini; waktu awal dibulatkan ke kelipatan 5 menit berikutnya.
            const today = new Date();
            if (ctx.pkDateInput)
                ctx.pkDateInput.value = ctx.formatDmy(today);
            ctx.pk = new Date(today);
            ctx.pk.setMinutes(Math.ceil(ctx.pk.getMinutes() / 5) * 5, 0, 0);
        }
        ctx.pkDateInput?.classList.remove("invalid");
        ctx.pkDateInput?.removeAttribute("aria-invalid");
        ctx.paintPick();
        ctx.paintRepeatMode();
        ctx.remPage?.classList.add("picking");
        ctx.setReminderPickerExpanded(true);
    };
    ctx.addReminder = function addReminder() {
        const input = ctx.$("rm-input") as HTMLInputElement;
        const text = input.value.trim();
        if (!text)
            return;
        const mode: RepeatMode = ctx.pickAt > 0 ? ctx.repeatMode : "none";
        if (mode === "weekly" && ctx.repeatDays.length === 0) {
            // minimal satu hari harus dipilih
            ctx.openPick?.();
            document.getElementById("pk-days")?.classList.add("invalid");
            return;
        }
        const timezoneOffsetMinutes = ctx.pickAt > 0 ? new Date(ctx.pickAt).getTimezoneOffset() : new Date().getTimezoneOffset();
        ctx.invoke<Reminder[]>("reminders_add", {
            text,
            dueAt: ctx.pickAt,
            repeatMode: mode,
            timezoneOffsetMinutes,
            repeatWeekdays: mode === "weekly" ? ctx.daysToMask(ctx.repeatDays) : 0,
        })
            .then((items) => {
                ctx.paintReminders(items);
                input.value = "";
                ctx.pickAt = 0;
                ctx.repeatMode = "none";
                ctx.repeatDays = [...DEFAULT_DAYS];
                ctx.paintWhen();
                ctx.paintRepeatMode();
                ctx.closePick();
                ctx.flash("happy", 900);
            })
            .catch(() => { });
    };
}

export function init(ctx: Record<string, any>): void {
    ctx.fmtDue = (ms: number) => {
        if (!ms)
            return "";
        const d = new Date(ms);
        const sameYear = d.getFullYear() === new Date().getFullYear();
        return d.toLocaleString("id-ID", {
            day: "numeric", month: "short",
            year: sameYear ? undefined : "numeric",
            hour: "2-digit", minute: "2-digit",
        });
    };
    ctx.ringing = new Map<string, Reminder>();
    ctx.audioCtx = null;
    document.getElementById("rem-stop")?.addEventListener("click", (e) => { e.stopPropagation(); ctx.endRing("stop"); });
    document.getElementById("rem-snooze")?.addEventListener("click", (e) => { e.stopPropagation(); ctx.endRing("snooze"); });
    document.getElementById("rm-clear-done")?.addEventListener("click", (e) => {
        e.stopPropagation();
        ctx.invoke<Reminder[]>("reminders_clear_done").then(ctx.paintReminders).catch(() => { });
    });
    ctx.remPage = document.querySelector<HTMLElement>(".p-reminder");
    ctx.pickAt = 0;
    ctx.pk = new Date();
    ctx.repeatMode = "none";
    ctx.repeatDays = [...ctx.DEFAULT_REPEAT_DAYS];
    ctx.pkDateInput = document.getElementById("pk-date-input") as HTMLInputElement | null;
    ctx.closePick = () => {
        ctx.remPage?.classList.remove("picking");
        ctx.setReminderPickerExpanded(false);
    };
    document.getElementById("rm-when")?.addEventListener("click", (e) => {
        e.stopPropagation();
        if (ctx.remPage?.classList.contains("picking"))
            ctx.closePick();
        else
            ctx.openPick();
    });
    document.querySelectorAll<HTMLElement>("[data-pk]").forEach((b) => b.addEventListener("click", (e) => {
        e.stopPropagation();
        const [unit, sign] = [b.dataset.pk![0], b.dataset.pk![1] === "+" ? 1 : -1];
        if (unit === "h")
            ctx.pk.setHours(ctx.pk.getHours() + sign);
        else if (unit === "m")
            ctx.pk.setMinutes(ctx.pk.getMinutes() + sign * 5);
        ctx.paintPick();
    }));
    // Sisipkan garis miring otomatis setelah DD dan MM, sambil menjaga posisi caret.
    ctx.pkDateInput?.addEventListener("input", () => {
        const oldValue = ctx.pkDateInput.value;
        const oldCaret = ctx.pkDateInput.selectionStart ?? oldValue.length;
        const digitsBeforeCaret = oldValue.slice(0, oldCaret).replace(/\D/g, "").length;
        const formatted = ctx.formatDmyInput(oldValue);
        ctx.pkDateInput.value = formatted;
        let nextCaret = digitsBeforeCaret;
        if (digitsBeforeCaret === 2 && formatted.length >= 3)
            nextCaret = 3;
        else if (digitsBeforeCaret === 4 && formatted.length >= 6)
            nextCaret = 6;
        else if (digitsBeforeCaret > 2 && digitsBeforeCaret < 4)
            nextCaret = digitsBeforeCaret + 1;
        else if (digitsBeforeCaret > 4)
            nextCaret = digitsBeforeCaret + 2;
        nextCaret = Math.min(nextCaret, formatted.length);
        ctx.pkDateInput.setSelectionRange(nextCaret, nextCaret);
        ctx.pkDateInput.classList.remove("invalid");
        ctx.pkDateInput.removeAttribute("aria-invalid");
        const parsed = ctx.parseDmy(formatted);
        if (parsed) {
            ctx.pk = new Date(parsed.year, parsed.month - 1, parsed.day, ctx.pk.getHours(), ctx.pk.getMinutes(), 0, 0);
        }
    });
    ctx.pkDateInput?.addEventListener("keydown", (e) => {
        e.stopPropagation();
        // Backspace tepat setelah slash menghapus digit di depannya, bukan slash otomatis.
        if (e.key === "Backspace" && ctx.pkDateInput.selectionStart === ctx.pkDateInput.selectionEnd) {
            const caret = ctx.pkDateInput.selectionStart ?? 0;
            if (caret > 0 && ctx.pkDateInput.value[caret - 1] === "/") {
                e.preventDefault();
                const digits = ctx.pkDateInput.value.replace(/\D/g, "");
                const digitsBefore = ctx.pkDateInput.value.slice(0, caret - 1).replace(/\D/g, "").length;
                const removeAt = Math.max(0, digitsBefore - 1);
                const nextDigits = digits.slice(0, removeAt) + digits.slice(removeAt + 1);
                ctx.pkDateInput.value = ctx.formatDmyInput(nextDigits);
                const nextCaret = Math.min(removeAt, ctx.pkDateInput.value.length);
                ctx.pkDateInput.setSelectionRange(nextCaret, nextCaret);
                ctx.pkDateInput.dispatchEvent(new Event("input", { bubbles: true }));
            }
        }
    });
    document.getElementById("pk-repeat-mode")?.addEventListener("change", (e) => {
        e.stopPropagation();
        const value = (e.currentTarget as HTMLSelectElement).value;
        ctx.repeatMode = value === "daily" || value === "weekly" || value === "monthly" ? value : "none";
        ctx.paintRepeatDays();
    });
    document.querySelectorAll<HTMLButtonElement>("#pk-days button[data-day]").forEach((b) => b.addEventListener("click", (e) => {
        e.stopPropagation();
        const day = Number(b.dataset.day);
        ctx.repeatDays = ctx.repeatDays.includes(day) ? ctx.repeatDays.filter((d: number) => d !== day) : [...ctx.repeatDays, day];
        ctx.paintRepeatDays();
    }));
    document.getElementById("pk-ok")?.addEventListener("click", (e) => {
        e.stopPropagation();
        if (ctx.repeatMode === "weekly" && ctx.repeatDays.length === 0) {
            document.getElementById("pk-days")?.classList.add("invalid");
            return;
        }
        if (!ctx.setPkDateFromInput()) {
            ctx.pkDateInput?.classList.add("invalid");
            ctx.pkDateInput?.setAttribute("aria-invalid", "true");
            ctx.pkDateInput?.focus();
            return;
        }
        ctx.pk.setSeconds(0, 0);
        ctx.pickAt = ctx.pk.getTime();
        ctx.pkDateInput?.classList.remove("invalid");
        ctx.pkDateInput?.removeAttribute("aria-invalid");
        ctx.paintWhen();
        // Jika teks pengingat sudah diisi, tombol Atur sekaligus menyimpan item ke daftar.
        // Jika input masih kosong, simpan pilihan tanggal/jam sementara dan tutup picker;
        // item bisa dibuat setelah teks diisi dengan tombol tambah.
        const reminderText = (document.getElementById("rm-input") as HTMLInputElement | null)?.value.trim();
        if (reminderText) {
            ctx.addReminder();
            return;
        }
        ctx.closePick();
    });
    document.getElementById("pk-clear")?.addEventListener("click", (e) => {
        e.stopPropagation();
        ctx.pickAt = 0;
        ctx.pkDateInput?.classList.remove("invalid");
        ctx.pkDateInput?.removeAttribute("aria-invalid");
        ctx.repeatMode = "none";
        ctx.repeatDays = [...ctx.DEFAULT_REPEAT_DAYS];
        ctx.paintWhen();
        ctx.paintRepeatMode();
        ctx.closePick();
    });
    document.getElementById("rm-add-btn")?.addEventListener("click", (e) => { e.stopPropagation(); ctx.addReminder(); });
    document.getElementById("rm-input")?.addEventListener("keydown", (e) => {
        e.stopPropagation();
        if ((e as KeyboardEvent).key === "Enter")
            ctx.addReminder();
    });
    ctx.paintWhen();
    ctx.paintRepeatMode();
    // dikirim Rust saat jatuh tempo, lalu diulang tiap 20 detik sampai dihentikan
    ctx.listen<Reminder>("reminder-due", (e) => {
        const r = e.payload;
        const first = !ctx.ringing.has(r.id);
        ctx.ringing.set(r.id, r);
        ctx.beep();
        if (first || ctx.override === null) {
            ctx.flash("surprised", 2500);
            ctx.showRing();
        }
        else {
            ctx.flash("surprised", 1200);
        }
        ctx.refreshReminders();
    });
    ctx.refreshReminders();
}