// @ts-nocheck
// Integrasi greeting, moments, prayer, eye break, dan price alert.

export function register(ctx: Record<string, any>): void {
}

export function init(ctx: Record<string, any>): void {
    ctx.initNudge({
        lastMove: () => ctx.lastCursorMove, // gerakan pointer terakhir, dari event "cursor" Rust
        playing: () => ctx.playing,
        canShow: () => !ctx.meetingMode && ctx.override === null, // jangan menimpa kartu/hub/rapat
        card: (title, sub, ms, emoji) => {
            // Notch di sisi kiri/kanan/dasar: pesan "pointer diam" tampil sebagai awan percakapan,
            // bukan kartu lebar. ms = 0 (until_move) berarti awan bertahan sampai pointer bergerak lagi.
            if (ctx.momentBubble?.show(emoji ? `${title} ${emoji}` : title, sub, ms) === true)
                return;
            ctx.notch.classList.remove("greeting-active");
            ctx.$("dev-icon").classList.remove("off", "play");
            ctx.$("dev-chip").classList.remove("off");
            ctx.$("dev-name").textContent = title;
            ctx.$("dev-sub").textContent = sub;
            ctx.hubByHover = false;
            ctx.expand("dev", ms);
        },
        returnCard: (title, sub, ms) => {
            // Sapaan saat pointer kembali tidak boleh menimpa kartu lain yang sedang dipakai.
            const isIdleNudge = ctx.override === "dev" && ctx.$("dev-sub").textContent.startsWith("Pointer diam");
            if (ctx.meetingMode || (ctx.override !== null && !isIdleNudge))
                return;
            // Dock kiri/kanan/dasar: sapaan juga lewat awan (menggantikan awan "pointer diam" yang masih tampil).
            if (ctx.momentBubble?.show(title, sub, ms) === true)
                return;
            ctx.notch.classList.remove("greeting-active");
            ctx.$("dev-icon").classList.remove("off", "play");
            ctx.$("dev-chip").classList.add("off");
            ctx.$("dev-name").textContent = title;
            ctx.$("dev-sub").textContent = sub;
            ctx.hubByHover = false;
            ctx.expand("dev", ms);
        },
        mood: (m, ms) => ctx.flash(m as Mood, ms),
    });
    ctx.greetingApi = ctx.initGreeting({
        canShow: () => !ctx.meetingMode && ctx.override === null && !ctx.isDraggingWindow && !ctx.holdHover,
        isCard: () => ctx.override === "dev",
        show: (ms) => { ctx.hubByHover = false; ctx.expand("dev", ms); },
        mood: (m, ms) => ctx.flash(m as Mood, ms),
        showBubble: (text, sub, ms) => ctx.momentBubble?.show(text, sub, ms) === true,
        hideBubble: () => ctx.momentBubble?.hide(),
        isMeeting: () => !!ctx.meetingMode,
    });
    // Conversation Moments + Focus Companion: komentar teks memakai kartu yang sama, tidak pernah menimpa
    // adzan, pengingat, greeting, hub, atau kartu perangkat (menunggu antrean sampai notch bebas).
    ctx.initMoments({
        canShow: () => !ctx.meetingMode && ctx.override === null && !ctx.prayerAdhanActive && !ctx.isDraggingWindow && !ctx.holdHover && !ctx.inside,
        say: (text, sub, ms) => {
            // Gunakan awan percakapan saat notch di sisi kiri/dasar; selain itu tetap gunakan kartu lama.
            if (!ctx.momentBubble?.show(text, sub, ms)) ctx.greetingApi?.say(text, sub, ms);
        },
    });
    ctx.notchFree = () => !ctx.meetingMode && ctx.override === null && !ctx.prayerAdhanActive && !ctx.isDraggingWindow && !ctx.holdHover && !ctx.inside;
    ctx.showTextCard = (title: string, sub: string, ms: number, emoji?: string) => {
        ctx.notch.classList.remove("greeting-active");
        ctx.$("dev-icon").classList.remove("off", "play");
        ctx.$("dev-chip").classList.remove("off");
        ctx.$("dev-name").textContent = title;
        ctx.$("dev-sub").textContent = sub;
        ctx.$("dev-sub").classList.remove("wa-oneline");
        // Tanpa emoji, lingkaran ikon memakai sisa ikon perangkat sebelumnya (tampak kosong).
        // Dengan emoji: pakai gaya ikon "nudge" (emoji di lingkaran kiri, lencana centang kanan disembunyikan).
        if (emoji) {
            const icon = ctx.$("dev-icon");
            icon.dataset.kind = "nudge";
            icon.dataset.count = "1";
            const span = document.createElement("span");
            span.className = "emo";
            span.textContent = emoji;
            icon.replaceChildren(span);
        }
        ctx.hubByHover = false;
        ctx.expand("dev", ms);
    };
    // Pesan singkat: awan percakapan bila notch di kiri/kanan/dasar, selain itu kartu teks.
    // Mengembalikan true bila tampil sebagai awan.
    ctx.showNotice = (title: string, sub: string, ms: number, emoji?: string): boolean => {
        if (ctx.momentBubble?.show(emoji ? `${title} ${emoji}` : title, sub, ms) === true)
            return true;
        ctx.showTextCard(title, sub, ms, emoji);
        return false;
    };
    // PiP Chrome/Edge yang menempel di notch (pip.rs): panel media membesar memuat PiP, PiP tampil
    // bersama panel dan tersembunyi saat notch menutup.
    ctx.pipDocked = false;
    ctx.pipSlot = 0;     // tinggi slot PiP dalam satuan dasar (sebelum skala), termasuk jarak
    ctx.pipHover = false;
    ctx.pipShownKey = "";
    // Jeda sebelum PiP muncul setelah panel media mulai terbuka (samakan dengan durasi transisi CSS notch).
    // UI notch (panel media) tampil dulu, baru PiP menyusul.
    const PIP_REVEAL_DELAY_MS = 380;
    ctx.pipRevealed = false;   // true = PiP boleh langsung tampil tanpa menunggu animasi panel
    ctx.pipRevealTimer = 0;
    // Saat panel membesar, isi halaman media ikut tersebar ke seluruh tinggi baru dan menutupi slot PiP.
    // Bekukan tinggi halaman media pada ukuran aslinya (menempel di atas); slot di bawahnya dibiarkan kosong.
    ctx.pipFreezeMedia = (on: boolean) => {
        const el = (ctx.allHubPages ?? []).find((p: HTMLElement) => p.dataset.page === "media");
        if (!el)
            return;
        if (!on) {
            el.style.removeProperty("height");
            el.style.removeProperty("max-height");
            el.style.removeProperty("align-self");
            delete el.dataset.pipH;
            return;
        }
        if (el.dataset.pipH)
            return; // sudah dibekukan; ukuran asli sudah tersimpan
        const h = el.offsetHeight; // ukur SEBELUM tinggi panel diubah
        if (h <= 0)
            return;
        el.dataset.pipH = String(h);
        el.style.setProperty("height", `${h}px`, "important");
        el.style.setProperty("max-height", `${h}px`, "important");
        el.style.setProperty("align-self", "flex-start", "important");
    };
    ctx.pipExtra = () => (ctx.pipDocked && ctx.names[ctx.page] === "media" ? ctx.pipSlot : 0);
    ctx.pipSync = (view?: string) => {
        const v = view ?? ctx.notch.dataset.view ?? "idle";
        const show = ctx.pipDocked && v === "hub" && ctx.names[ctx.page] === "media";
        // Tepat setelah PiP dijatuhkan, panel media belum sempat terbuka: jangan kirim "sembunyi" dulu,
        // supaya PiP tidak berkedip hilang sebelum panelnya muncul. (Dicek SEBELUM reset jeda di bawah,
        // supaya drop pertama tetap langsung tampil.)
        if (!show && ctx.pipAwaitUntil && Date.now() < ctx.pipAwaitUntil)
            return;
        // Notch menutup / pindah halaman: batalkan jadwal tampil dan sembunyikan PiP seketika.
        if (!show) {
            clearTimeout(ctx.pipRevealTimer);
            ctx.pipRevealTimer = 0;
            ctx.pipRevealed = false;
        }
        if (show)
            ctx.pipAwaitUntil = 0;
        // Panel baru mulai terbuka: tunggu seluruh UI notch tampil dulu, baru PiP muncul.
        if (show && !ctx.pipRevealed) {
            if (!ctx.pipRevealTimer) {
                ctx.pipRevealTimer = window.setTimeout(() => {
                    ctx.pipRevealTimer = 0;
                    ctx.pipRevealed = true;
                    ctx.pipSync();
                }, PIP_REVEAL_DELAY_MS);
            }
            return;
        }
        // jarak PiP dari sisi atas panel: tinggi dasar panel media + jarak kecil (piksel logis nyata)
        const y = Math.round((ctx.BASE_SIZES.hub[1] + 6) * ctx.CFG.scale);
        const key = `${show}:${y}`;
        if (key === ctx.pipShownKey)
            return;
        ctx.pipShownKey = key;
        ctx.invoke("pip_set_visible", { visible: show, y }).catch((err) => console.warn("[PiP] pip_set_visible gagal:", err));
    };
    new MutationObserver(() => ctx.pipSync()).observe(ctx.notch, { attributes: true, attributeFilter: ["data-view"] });
    ctx.listen("pip-dock", (e: { payload: { docked: boolean; h?: number } }) => {
        const p = e.payload ?? { docked: false };
        ctx.pipDocked = !!p.docked;
        ctx.pipSlot = p.docked ? Math.round((p.h ?? 200) / ctx.CFG.scale) + 26 : 0; // 6 jarak atas + 20 ruang bawah
        if (!p.docked)
            ctx.pipHover = false;
        ctx.pipShownKey = "";
        ctx.pipAwaitUntil = p.docked ? Date.now() + 1500 : 0;
        // Drop pertama: PiP sudah terlihat (masa tunggu GRACE di Rust), jadi lewati jeda supaya tidak berkedip.
        ctx.pipRevealed = !!p.docked;
        clearTimeout(ctx.pipRevealTimer);
        ctx.pipRevealTimer = 0;
        if (p.docked) {
            try {
                ctx.openHub(ctx.pageOf("media"), ctx.T.hub_auto_ms);
            }
            catch (err) {
                console.warn("[PiP] gagal membuka panel media:", err);
            }
        }
        ctx.syncHubDynamicHeight();
        ctx.pipSync();
    }).catch((err) => console.warn("[PiP] listen pip-dock gagal:", err));
    ctx.listen("pip-hover", (e: { payload: { hover: boolean } }) => {
        ctx.pipHover = !!e.payload?.hover;
        if (ctx.pipHover)
            ctx.setInside(true);
        else if (ctx.pointerX >= 0)
            ctx.setInside(ctx.rectHas(ctx.pointerX, ctx.pointerY));
    }).catch((err) => console.warn("[PiP] listen pip-hover gagal:", err));
    ctx.initEyeBreak({
        lastMove: () => ctx.lastCursorMove,
        canShow: ctx.notchFree,
        view: () => ctx.notch.dataset.view ?? "idle",
        card: ctx.showNotice,
        mood: (m, ms) => ctx.flash(m as Mood, ms),
    });
    ctx.initPriceAlert({
        canShow: ctx.notchFree,
        card: ctx.showNotice,
        mood: (m, ms) => ctx.flash(m as Mood, ms),
    });
    // Jendela Android (terpisah): beri tahu notch utama saat unduhan file selesai.
    ctx.listen("android-download-done", (e: { payload: { name: string; path: string } }) => {
        const { name } = e.payload;
        ctx.showNotice("Unduhan selesai", `${name} disimpan ke PC`, 6000, "📥");
        ctx.flash("happy", 6000);
    }).catch((err) => console.error("[Notch] listen android-download-done gagal:", err));
    // Pengingat jadwal shalat memakai kartu khusus. Saat adzan diputar, kartu ditahan
    // sampai audio selesai dan media yang sebelumnya berjalan dilanjutkan kembali.
    ctx.initPrayer({
        card: (title, sub, ms) => {
            // Notch di kiri/kanan/dasar: pengingat tampil sebagai awan percakapan; selain itu kartu shalat.
            if (ctx.momentBubble?.show(title, sub, ms) === true)
                return;
            ctx.$("pr-alert-title").textContent = title;
            ctx.$("pr-alert-sub").textContent = sub;
            ctx.$("pr-alert-status").textContent = "PENGINGAT";
            ctx.hubByHover = false;
            ctx.expand("prayerAlert", ms);
        },
        mood: (m, ms) => ctx.flash(m as Mood, ms),
        adhanStart: (prayerName, sub) => {
            if (ctx.prayerAdhanActive)
                return;
            ctx.prayerAdhanActive = true;
            ctx.adhanFace?.start();
            ctx.resumeMediaAfterAdhan = ctx.playing;
            clearTimeout(ctx.hideTimer);
            ctx.hubByHover = false;
            // Notch di kiri/kanan/dasar: pakai awan percakapan yang menempel (tidak hilang oleh klik/hover/timer).
            // Notch di atas/melayang: pakai kartu prayerAlert yang juga tidak bisa ditutup klik.
            ctx.adhanUsesBubble = ctx.momentBubble?.showSticky(`Waktu ${prayerName}`, `${sub} · Adzan sedang diputar`) === true;
            if (!ctx.adhanUsesBubble) {
                ctx.$("pr-alert-title").textContent = `Waktu ${prayerName}`;
                ctx.$("pr-alert-sub").textContent = `${sub} · Adzan sedang diputar`;
                ctx.$("pr-alert-status").textContent = "ADZAN";
                ctx.override = "prayerAlert";
                ctx.render(); // ms = 0: alert tetap terbuka hingga adhanEnd dipanggil.
            }
            if (ctx.resumeMediaAfterAdhan) {
                void ctx.invoke("media_control", { action: "pause" }).catch((err) => console.warn("Tidak bisa menjeda media saat adzan:", err));
            }
        },
        ramadanExpanded: (expanded) => {
            // Hanya mencatat pengaturan (aktif/tidak). Class "ramadan-expanded" yang melebarkan notch
            // dihitung di syncHubDynamicHeight() dan hanya ada selama halaman shalat yang terbuka.
            ctx.notch.classList.toggle("ramadan-enabled", expanded);
            // Ukuran panel berubah tepat di bawah kursor: tahan hover sebentar supaya hub tidak
            // tertutup sendiri saat tepinya bergeser (terasa seperti tombol tidak merespons).
            ctx.settleUntil = Date.now() + 500;
            ctx.closeAt = 0;
            ctx.syncHubDynamicHeight();
        },
        adhanEnd: () => {
            if (!ctx.prayerAdhanActive)
                return;
            ctx.prayerAdhanActive = false;
            ctx.adhanFace?.stop();
            ctx.momentBubble?.hideSticky();
            ctx.adhanUsesBubble = false;
            // awan adzan memakai slot sticky yang sama dengan awan pengingat: pulihkan pengingat yang masih berbunyi
            if (ctx.remUsesBubble && ctx.ringing?.size) {
                ctx.remUsesBubble = false;
                ctx.showRing();
            }
            clearTimeout(ctx.hideTimer);
            if (ctx.override === "prayerAlert") {
                ctx.override = null;
                ctx.hubByHover = false;
                ctx.render();
            }
            const resume = ctx.resumeMediaAfterAdhan;
            ctx.resumeMediaAfterAdhan = false;
            if (resume) {
                void ctx.invoke("media_control", { action: "play" }).catch((err) => console.warn("Tidak bisa melanjutkan media setelah adzan:", err));
            }
        },
    });
}