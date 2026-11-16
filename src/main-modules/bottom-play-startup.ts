// @ts-nocheck
// Bottom-play dan inisialisasi akhir aplikasi.

export function register(ctx: Record<string, any>): void {
}

export function init(ctx: Record<string, any>): void {
    // Animasi gerak acak saat Notch menempel di dasar/sisi layar.
    // Jangan hilangkan inisialisasi ini: bottom-play.ts tidak aktif tanpa hook dari main.ts.
    ctx.initBottomPlay({
        shell: ctx.$("shell"),
        notch,
        busy: () => ctx.inside || ctx.override !== null || ctx.isDraggingWindow || ctx.holdHover ||
            ctx.notch.classList.contains("dropping") || ctx.notch.classList.contains("greeting-active") ||
            // kelas "currency-expanded" bisa tertinggal setelah hub menutup di halaman Currency, sehingga
            // animasi bottom-play terblokir selamanya; cukup cek hub Currency yang benar-benar terbuka.
            ctx.isCurrencyHub(),
        playing: () => ctx.playing,
        calm: () => ctx.sleeping || ctx.lowBattery,
        mood: (m, ms) => ctx.flash(m as Mood, ms),
        getCx: () => ctx.currentCx,
        // Pindahkan jendela native tanpa mengubah posisi tersimpan pengguna.
        setCx: (cx) => {
            ctx.updatePositionUI(true, ctx.BOTTOM_TY, cx);
            return Promise.resolve(ctx.invoke("notch_move_to", { cx, ty: ctx.BOTTOM_TY }));
        },
        range: () => {
            const [vw] = ctx.SIZES[ctx.notch.dataset.view ?? "bottomIdle"] ?? ctx.SIZES.bottomIdle;
            return [ctx.MON.x + vw / 2, ctx.MON.x + ctx.MON.w - vw / 2];
        },
        dock: () => {
            if (ctx.notch.classList.contains("is-bottom"))
                return "bottom";
            const view = ctx.notch.dataset.view;
            if (view === "edgeIdle" || view === "edgeMedia") {
                if (ctx.notch.classList.contains("is-edge-left"))
                    return "left";
                if (ctx.notch.classList.contains("is-edge-right"))
                    return "right";
            }
            return null;
        },
        getTy: () => ctx.currentTy,
        setTy: (ty) => {
            const left = ctx.notch.classList.contains("is-edge-left");
            const right = ctx.notch.classList.contains("is-edge-right");
            const cx = left ? ctx.MON.x + 30 * ctx.CFG.scale
                : right ? ctx.MON.x + ctx.MON.w - 30 * ctx.CFG.scale
                    : ctx.currentCx;
            ctx.updatePositionUI(true, ty, cx);
            return Promise.resolve(ctx.invoke("notch_move_to", { cx, ty }));
        },
        verticalRange: () => {
            const [, vh] = ctx.SIZES[ctx.notch.dataset.view ?? "edgeIdle"] ?? ctx.SIZES.edgeIdle;
            const margin = Math.round(18 * ctx.CFG.scale);
            return [ctx.MON.y + margin, ctx.MON.y + ctx.MON.h - vh - margin];
        },
        hit: () => ctx.setHit(ctx.notch.dataset.view ?? "idle"),
    });
    if (ctx.clockOn)
        ctx.startClock();
    void ctx.syncWindow(); // jendela native diset sekali ke ukuran tetap

    ctx.render();
    ctx.updateBase();
}
