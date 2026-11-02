// Color Picker global: shortcut (Ctrl+Alt+Shift+P) langsung mengaktifkan eyedropper layar; klik/Enter mengambil
// warna dan HEX langsung tersalin. Esc atau shortcut lagi membatalkan. Panel hanya cadangan bila eyedropper gagal.
export function register(ctx: Record<string, any>): void {
    ctx.colorPicker = {
        panel: null as HTMLElement | null,
        lastHex: "",
        previousView: "idle",
        opening: false,
        picking: false,
        closeTimer: 0,
    };
}

export function init(ctx: Record<string, any>): void {
    const state = ctx.colorPicker;
    if (!state) return;

    const panel = document.getElementById("notch-color-picker");
    if (!panel) {
        console.warn("[Color Picker] Elemen #notch-color-picker tidak ditemukan di index.html.");
        return;
    }

    state.panel = panel;

    const status = panel.querySelector<HTMLElement>(".cp-status");
    const result = panel.querySelector<HTMLElement>(".cp-result");
    const swatch = panel.querySelector<HTMLElement>(".cp-swatch");
    const hexValue = panel.querySelector<HTMLElement>(".cp-hex");
    const pickButton = panel.querySelector<HTMLButtonElement>('[data-action="pick"]');
    const copyButton = panel.querySelector<HTMLButtonElement>('[data-action="copy"]');

    const setPanelHitArea = async (): Promise<void> => {
        // Gunakan posisi aktual panel setelah native window selesai diperbesar.
        const rect = panel.getBoundingClientRect();

        await ctx.invoke("set_hit_area", {
            x: Math.max(0, Math.floor(rect.left)),
            y: Math.max(0, Math.floor(rect.top)),
            w: Math.max(1, Math.ceil(rect.width)),
            h: Math.max(1, Math.ceil(rect.height)),
        });
    };

    const restoreWindow = async (): Promise<void> => {
        const previousView = state.previousView || "idle";

        try {
            ctx.placeShell?.(previousView);
            await ctx.syncNativeWindowSize?.(previousView);
            ctx.placeShell?.(previousView);
            ctx.setHit?.(previousView);
        } catch (error) {
            console.error("[Color Picker] Gagal mengembalikan ukuran window:", error);
        }
    };

    const close = async (): Promise<void> => {
        if (panel.hidden) return;

        panel.hidden = true;
        await restoreWindow();
    };

    panel.querySelector<HTMLButtonElement>(".cp-close")?.addEventListener("click", (event) => {
        event.preventDefault();
        event.stopPropagation();
        void close();
    });

    const copyHex = async (): Promise<void> => {
        if (!state.lastHex) return;

        try {
            await ctx.invoke("color_copy", { text: state.lastHex });
            if (status) status.textContent = `${state.lastHex} · Tersalin`;
        } catch (error) {
            if (status) status.textContent = `Gagal menyalin: ${String(error)}`;
        }
    };

    copyButton?.addEventListener("click", (event) => {
        event.preventDefault();
        event.stopPropagation();
        void copyHex();
    });

    pickButton?.addEventListener("click", async (event) => {
        event.preventDefault();
        event.stopPropagation();

        const EyeDropperApi = (window as any).EyeDropper;
        if (!EyeDropperApi) {
            if (status) status.textContent = "Eyedropper tidak didukung WebView ini.";
            if (result) result.hidden = false;
            return;
        }

        pickButton.disabled = true;
        pickButton.textContent = "Pilih area di layar…";

        try {
            const picked = await new EyeDropperApi().open();
            state.lastHex = String(picked.sRGBHex || "").toUpperCase();

            if (swatch) swatch.style.backgroundColor = state.lastHex;
            if (hexValue) hexValue.textContent = state.lastHex;
            if (result) result.hidden = false;
            if (copyButton) copyButton.hidden = false;

            // Salin otomatis setelah warna berhasil diambil, lalu tutup panel.
            await copyHex();
            ctx.flash?.("happy", 1200);
            void close();
            feedback(state.lastHex, "Warna tersalin ke clipboard", 2500, "🎨");
        } catch (error: any) {
            if (error?.name !== "AbortError" && status) {
                status.textContent = `Tidak dapat mengambil warna: ${String(error)}`;
            }
        } finally {
            pickButton.disabled = false;
            pickButton.textContent = "Pilih warna";
        }
    });

    const open = async (): Promise<void> => {
        if (state.opening || !panel.hidden) return;

        state.opening = true;
        state.previousView = ctx.notch?.dataset.view || "idle";
        panel.hidden = false;

        try {
            // Siapkan layout dan ukuran native yang cukup untuk menampilkan panel.
            ctx.placeShell?.("hub");
            await ctx.syncNativeWindowSize?.("hub");
            ctx.placeShell?.("hub");

            // Beri native window area klik tepat di atas panel, bukan seluruh layar.
            await setPanelHitArea();

            if (status && !state.picking) status.textContent = "Tekan “Pilih warna”, lalu klik warna di layar.";
        } catch (error) {
            console.error("[Color Picker] Gagal membuka panel:", error);
            if (status) status.textContent = "Gagal mengaktifkan area klik Color Picker.";
        } finally {
            state.opening = false;
        }
    };

    // ---- Mode langsung: shortcut -> loupe native mengikuti pointer -> klik -> tersalin ----
    // Tidak ada panel: loupe digambar backend (colorpick.rs). Panel hanya cadangan bila picker native tidak ada.
    const feedback = (title: string, sub: string, ms: number, emoji?: string): void => {
        // Awan percakapan saat notch di kiri/kanan/dasar; selain itu kartu teks dengan ikon emoji.
        if (ctx.momentBubble?.show(emoji ? `${title} ${emoji}` : title, sub, ms) === true) return;
        ctx.showTextCard?.(title, sub, ms, emoji);
    };

    const pickWithEyeDropper = async (): Promise<string | null> => {
        const EyeDropperApi = (window as any).EyeDropper;
        if (!EyeDropperApi) throw new Error("NO_EYEDROPPER");
        try {
            const picked = await new EyeDropperApi().open();
            return String(picked.sRGBHex || "").toUpperCase() || null;
        } catch (error: any) {
            if (error?.name === "AbortError") return null;
            throw error;
        }
    };

    const startInstantPick = async (): Promise<void> => {
        if (state.picking) {
            // shortcut ditekan lagi saat aktif = batal
            await ctx.invoke("color_pick_cancel").catch(() => {});
            return;
        }
        state.picking = true;

        try {
            let hex: string | null = null;
            try {
                hex = await ctx.invoke("color_pick_screen");
            } catch (error) {
                if (String(error).includes("unsupported")) hex = await pickWithEyeDropper();
                else throw error;
            }
            if (!hex) return; // dibatalkan

            hex = String(hex).toUpperCase();
            state.lastHex = hex;
            await ctx.invoke("color_copy", { text: hex });
            ctx.flash?.("happy", 1200);
            feedback(hex, "Warna tersalin ke clipboard", 2500, "🎨");
        } catch (error: any) {
            const msg = String(error?.message ?? error);
            if (msg === "NO_EYEDROPPER" || error?.name === "NotAllowedError") {
                void open(); // cadangan: panel dengan tombol "Pilih warna"
            } else {
                console.error("[Color Picker]", error);
                feedback("Color Picker gagal", msg.slice(0, 80), 4000);
            }
        } finally {
            state.picking = false;
        }
    };

    // Event shortcut dikirim dari backend Tauri.
    void ctx.listen("color-picker-shortcut", () => {
        // Panel cadangan sedang terbuka -> shortcut menutupnya; selain itu mulai / batalkan picker.
        if (!panel.hidden && !state.picking) {
            void close();
            return;
        }
        void startInstantPick();
    });
}