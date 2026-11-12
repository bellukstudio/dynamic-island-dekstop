// @ts-nocheck
// Deteksi hover, pointer, dan watchdog hub.

export function register(ctx: Record<string, any>): void {
    ctx.setInside = function setInside(v: boolean) {
        // Hindari hover bolak-balik ketika ukuran native window sedang beranimasi.
        // Namun, jadwalkan sinkronisasi ulang agar status hover tidak tertinggal setelah animasi.
        if (Date.now() < ctx.settleUntil) {
            window.clearTimeout(ctx.hoverSyncTimer);
            ctx.hoverSyncTimer = window.setTimeout(() => {
                if (ctx.pointerX >= 0 && ctx.pointerY >= 0) {
                    ctx.setInside(ctx.rectHas(ctx.pointerX, ctx.pointerY));
                }
            }, Math.max(25, ctx.settleUntil - Date.now() + 25));
            return;
        }
        if (v === ctx.inside)
            return;
        if (!v)
            ctx.holdHover = false;
        ctx.inside = v;
        ctx.insideSince = v ? Date.now() : 0;
        if (v && ctx.selectChosen) {
            ctx.selectHoldUntil = 0;
            ctx.selectChosen = false;
        }
        ctx.syncPosBar();
        if (v)
            ctx.flash("surprised", 900);
        ctx.render();
    };
}

export function init(ctx: Record<string, any>): void {
    ctx.pointerX = -1;
    ctx.pointerY = -1;
    ctx.hoverSyncTimer = undefined;
    ctx.posBar = ctx.$("notch-pos-bar");
    ctx.rectHas = (x: number, y: number) => {
        const r = ctx.notch.getBoundingClientRect();
        if (x >= r.left && x <= r.right && y >= r.top && y <= r.bottom)
            return true;
        // tombol pusatkan mengambang di luar notch: kursor di atasnya (dan celah menuju ke sana) tetap dianggap "di dalam"
        if (ctx.inside && !ctx.isDraggingWindow) {
            const b = ctx.posBar.getBoundingClientRect();
            const top = Math.min(b.top, r.bottom), bottom = Math.max(b.bottom, r.top);
            return x >= b.left && x <= b.right && y >= top && y <= bottom;
        }
        return false;
    };
    ctx.syncPosBar = () => ctx.$("shell").classList.toggle("pos-visible", ctx.inside && !ctx.isDraggingWindow);
    window.addEventListener("pointermove", (e) => {
        ctx.pointerX = e.clientX;
        ctx.pointerY = e.clientY;
        ctx.setInside(ctx.rectHas(ctx.pointerX, ctx.pointerY));
    });
    window.addEventListener("pointerup", (e) => {
        ctx.pointerX = e.clientX;
        ctx.pointerY = e.clientY;
        ctx.setInside(ctx.rectHas(ctx.pointerX, ctx.pointerY));
    });
    ctx.notch.addEventListener("mouseenter", () => ctx.setInside(true));
    document.documentElement.addEventListener("mouseleave", () => { if (!ctx.pipHover) ctx.setInside(false); });
    window.addEventListener("blur", () => { if (!ctx.pipHover) ctx.setInside(false); });
    // watchdog: buka/tutup hub dengan hysteresis
    setInterval(() => {
        const now = Date.now();
        if (ctx.dragging || ctx.isDraggingWindow || now < ctx.settleUntil)
            return;
        if (ctx.override === "hub" || ctx.override === "lyr" || ctx.override === "cal") {
            if (ctx.inside) {
                if (!ctx.hubByHover) {
                    clearTimeout(ctx.hideTimer);
                    ctx.hubByHover = true;
                }
                ctx.closeAt = 0;
            }
            else if (ctx.hubByHover) {
                if (ctx.holdOpen()) {
                    ctx.closeAt = 0;
                } // sedang mengetik / dropdown terbuka: jangan tutup
                else if (!ctx.closeAt)
                    ctx.closeAt = now + ctx.T.hover_close_ms;
                else if (now >= ctx.closeAt) {
                    ctx.closeAt = 0;
                    ctx.collapse();
                }
            }
        }
        else {
            ctx.closeAt = 0;
            if (!ctx.override && ctx.inside && !ctx.holdHover && !ctx.meetingMode && !ctx.prayerAdhanActive && now - ctx.insideSince > ctx.T.hover_open_ms)
                ctx.openHub(ctx.homePage());
        }
    }, 250);
    // ---------- nonaktifkan klik kanan (menu bawaan webview) ----------
    // fase capture: menu bawaan diblokir di mana pun; handler lain (mis. klik kanan item shelf) tetap jalan
    window.addEventListener("contextmenu", (e) => e.preventDefault(), true);
}