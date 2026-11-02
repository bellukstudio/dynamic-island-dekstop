// @ts-nocheck
// Geometri jendela native, ukuran, posisi shell, dan hitbox.

export function register(ctx: Record<string, any>): void {
    ctx.syncNativeWindowSize = function syncNativeWindowSize(view = ctx.notch.dataset.view ?? "idle"): Promise<void> {
        const [w, h] = ctx.winLogical(view);
        ctx.desiredNativeSize = { key: `${w}x${h}`, w, h, view };
        if (ctx.nativeSizePromise)
            return ctx.nativeSizePromise;
        if (ctx.desiredNativeSize.key === ctx.nativeSizeKey)
            return Promise.resolve();
        const run = async () => {
            // Serialisasi resize native agar klik menu yang cepat tidak meninggalkan ukuran jendela
            // dari halaman sebelumnya ketika pengguna sudah pindah ke halaman lain.
            while (ctx.desiredNativeSize && ctx.desiredNativeSize.key !== ctx.nativeSizeKey) {
                const target = ctx.desiredNativeSize;
                await ctx.invoke("set_size", { w: target.w, h: target.h });
                ctx.nativeSizeKey = target.key;
            }
        };
        const pending = run();
        ctx.nativeSizePromise = pending.finally(() => {
            ctx.nativeSizePromise = null;
            // Jika target berubah pada momen terakhir saat promise akan selesai, terapkan target terbaru.
            if (ctx.desiredNativeSize && ctx.desiredNativeSize.key !== ctx.nativeSizeKey) {
                void ctx.syncNativeWindowSize(ctx.desiredNativeSize.view).catch((error) => console.error("Gagal menyinkronkan ukuran jendela Notch:", error));
            }
        });
        return ctx.nativeSizePromise;
    };
    ctx.placeShell = function placeShell(view?: string) {
        const v = view ?? ctx.notch.dataset.view ?? "idle";
        const [W, H] = ctx.winLogical(v);
        const [vw, vh] = ctx.orbOn ? [ctx.orbD(), ctx.orbD()] : (ctx.SIZES[v] ?? ctx.SIZES.idle);
        const left = ctx.notch.classList.contains("is-edge-left");
        const right = ctx.notch.classList.contains("is-edge-right");
        const bottom = ctx.notch.classList.contains("is-bottom");
        let ox = 0, oy = 0;
        if (ctx.currentCx !== -1) {
            if (!left && !right) {
                const winCx = ctx.clamp(ctx.currentCx, ctx.MON.x + W / 2, ctx.MON.x + ctx.MON.w - W / 2);
                const want = ctx.clamp(ctx.currentCx, ctx.MON.x + vw / 2, ctx.MON.x + ctx.MON.w - vw / 2);
                ox = want - winCx;
            }
            if (!bottom) {
                const pad = ctx.orbPad();
                const winTy = ctx.clamp(ctx.currentTy - pad, ctx.MON.y, ctx.MON.y + ctx.MON.h - H);
                const wantTy = Math.min(ctx.currentTy, ctx.MON.y + ctx.MON.h - vh);
                oy = wantTy - winTy;
            }
        }
        // Posisi melayang diberi jarak di mode biasa; saat ORB aktif pusat bola mengikuti kursor.
        if (ctx.notch.classList.contains("is-floating") && !ctx.orbOn) {
            const gap = Math.round(14 * ctx.CFG.scale);
            oy += Math.max(0, Math.min(gap, H - (oy + vh)));
        }
        ctx.shellOx = ox;
        ctx.shellOy = oy;
        const shell = ctx.$("shell");
        shell.style.setProperty("--ox", `${ox}px`);
        shell.style.setProperty("--oy", `${oy}px`);
    };
    ctx.hitRect = function hitRect(view: string, withBubble = false) {
        const [W, H] = ctx.winLogical(view);
        let [hw, hh] = ctx.HIT[view] ?? ctx.HIT.idle;
        if (ctx.orbOn) {
            hw = Math.max(hw, ctx.orbD());
            hh = Math.max(hh, ctx.orbD());
        }
        const left = ctx.notch.classList.contains("is-edge-left");
        const right = ctx.notch.classList.contains("is-edge-right");
        const bottom = ctx.notch.classList.contains("is-bottom");
        const x = left ? 0 : right ? W - hw : (W - hw) / 2 + ctx.shellOx;
        const y = bottom ? H - hh : ctx.shellOy;
        const r = { x: Math.round(x), y: Math.round(y), w: hw, h: hh };
        // Awan pengingat punya tombol: gabungkan areanya ke hitbox native (hanya untuk klik, bukan untuk hover notch).
        const b = withBubble ? ctx.momentBubble?.rect?.() : null;
        if (b) {
            const x0 = Math.max(0, Math.min(r.x, b.x)), y0 = Math.max(0, Math.min(r.y, b.y));
            const x1 = Math.min(W, Math.max(r.x + r.w, b.x + b.w)), y1 = Math.min(H, Math.max(r.y + r.h, b.y + b.h));
            return { x: x0, y: y0, w: x1 - x0, h: y1 - y0 };
        }
        return r;
    };
    ctx.setHit = function setHit(view: string) {
        const r = ctx.hitRect(view, true);
        ctx.invoke("set_hit_area", { x: r.x, y: r.y, w: r.w, h: r.h });
    };
    ctx.syncWindow = async function syncWindow() {
        // Ambil geometri monitor dari sisi Rust (jika tersedia)
        try {
            ctx.MON = await ctx.invoke<typeof ctx.MON>("notch_monitor");
        }
        catch {
            // Fallback: pakai ukuran layar bawaan
        }
        // Jika posisi notch belum diatur (-1), default ke tengah atas monitor
        if (ctx.currentCx === -1) {
            ctx.currentCx = ctx.MON.x + Math.round(ctx.MON.w / 2);
        }
        try {
            await ctx.syncNativeWindowSize(ctx.notch.dataset.view ?? "idle");
        }
        catch (e) {
            console.error("Gagal menyinkronkan ukuran jendela Notch:", e);
        }
        ctx.updatePositionUI(ctx.notch.classList.contains("is-custom-pos"), ctx.currentTy, ctx.currentCx);
    };
}

export function init(ctx: Record<string, any>): void {
    ctx.BOTTOM_TY = 100000;
    ctx.isBottomTy = (ty: number) => ty >= ctx.BOTTOM_TY / 2;
    ctx.clamp = (v: number, a: number, b: number) => Math.min(Math.max(v, a), b);
    ctx.WIN_BASE = [600, 336];
    ctx.CURRENCY_WIN_BASE = [800, 620];
    ctx.isCurrencyHub = (view = ctx.notch.dataset.view ?? "idle") => view === "hub" && ctx.names[ctx.page] === "currency";
    ctx.isRamadanHub = (view = ctx.notch.dataset.view ?? "idle") => view === "hub" && ctx.names[ctx.page] === "prayer" && ctx.notch.classList.contains("ramadan-enabled");
    ctx.winLogical = (view = ctx.notch.dataset.view ?? "idle"): [
        number,
        number
    ] => {
        const [w, h] = (ctx.isCurrencyHub(view) || ctx.isRamadanHub(view)) ? ctx.CURRENCY_WIN_BASE : ctx.WIN_BASE;
        return [Math.round(w * ctx.CFG.scale), Math.round(h * ctx.CFG.scale)];
    };
    ctx.nativeSizeKey = "";
    ctx.nativeSizePromise = null;
    ctx.desiredNativeSize = null;
    ctx.MON = { x: 0, y: 0, w: window.screen.width, h: window.screen.height };
    ctx.shellOx = 0;
    ctx.shellOy = 0;
    ctx.ORB_BASE = 48;
    ctx.orbD = () => Math.round(ctx.ORB_BASE * ctx.CFG.scale);
    ctx.orbOn = false;
    ctx.orbPad = () => (ctx.orbOn ? Math.round(28 * ctx.CFG.scale) : 0);
    ctx.orbT0 = 0;
    ctx.orbRaf = 0;
    ctx.orbPx = 0;
    ctx.orbPy = 0;
    ctx.orbVx = 0;
    ctx.orbVy = 0;
    ctx.orbDock = "";
    ctx.orbOutTimer = undefined;
    ctx.lastPtr = { x: 0, y: 0 };
}
