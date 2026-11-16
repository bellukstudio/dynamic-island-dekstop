// @ts-nocheck
// Konfigurasi, state bersama, ukuran dasar, dan tema.
// ---------- konfigurasi (dari config.toml lewat Rust) ----------
type Appearance = {
    theme: string; // "auto" | "dark" | "light" | "custom"
    background: string; // dipakai saat theme = "custom"
    text: string; // kosong = otomatis kontras
    dark_bg: string;
    light_bg: string;
    accent: string; // kosong = otomatis dari cover album
    default_mood: string; // ekspresi mata bawaan saat idle
    eye_theme?: string; // skin mata: aurora | neon | ... | kawaii | boba | galaxy | frost
    notch_theme?: string; // tema notch: default | aurora | obsidian | neon | sakura | matcha
};
type Cfg = {
    scale: number;
    pages: string[];
    timing: {
        hub_auto_ms: number;
        card_ms: number;
        hud_ms: number;
        hover_open_ms: number;
        hover_close_ms: number;
        sleep_after_s: number;
    };
    pomodoro: {
        focus_min: number;
        break_min: number;
        long_break_min: number;
        rounds: number;
    };
    appearance: Appearance;
};
// ---------- state bersama ----------
type View = "bt" | "dev" | "hub" | "hud" | "lyr" | "cal" | "rem" | "prayerAlert" | "update";

export function register(ctx: Record<string, any>): void {
  ctx.applyTheme = function applyTheme() {
      const a = ctx.CFG.appearance;
      const root = document.documentElement;
      const st = root.style;
      // Tema notch siap pakai (css/notch-themes.css). "default"/kosong = warna manual seperti biasa.
      const nt = a.notch_theme && a.notch_theme !== "default" ? a.notch_theme : "";
      const acc = ctx.hexRgb(a.accent);
      if (nt) {
          root.dataset.notchTheme = nt;
          // CSS tema yang mengatur --bg/--fg/--fg-rgb: buang nilai inline lama supaya tidak menimpa.
          st.removeProperty("--bg");
          st.removeProperty("--fg");
          st.removeProperty("--fg-rgb");
      }
      else {
          delete root.dataset.notchTheme;
          const light = a.theme === "light" || (a.theme === "auto" && ctx.osLight.matches);
          const bgHex = a.theme === "custom" ? a.background : light ? a.light_bg : a.dark_bg;
          const bg = ctx.hexRgb(bgHex) ?? [0, 0, 0];
          const fg = ctx.hexRgb(a.text) ?? (ctx.luma(bg) > 0.55 ? [17, 17, 17] : [255, 255, 255]);
          st.setProperty("--bg", `rgb(${bg.join(",")})`);
          st.setProperty("--fg", `rgb(${fg.join(",")})`);
          st.setProperty("--fg-rgb", fg.join(","));
      }
      // Aksen manual selalu menang atas aksen tema.
      root.toggleAttribute("data-accent-fixed", !!acc);
      if (acc)
          st.setProperty("--accent", acc.join(", "));
      else
          st.removeProperty("--accent");
      ctx.eyes.dataset.eyeTheme = a.eye_theme || "aurora"; // skin mata dari Pengaturan
  };
  ctx.applyScale = function applyScale(v: number) {
      ctx.CFG.scale = Math.min(1.5, Math.max(0.5, v));
      Object.assign(ctx.SIZES, ctx.scaleMap(ctx.BASE_SIZES));
      Object.assign(ctx.HIT, ctx.scaleMap(ctx.BASE_HIT));
      ctx.syncHubDynamicHeight();
      document.documentElement.style.setProperty("--s", String(ctx.CFG.scale));
      ctx.lastView = ""; // paksa ukuran jendela & area klik dihitung ulang
      void ctx.syncWindow();
      ctx.render();
  };
}

export function init(ctx: Record<string, any>): void {
  ctx.FALLBACK = {
      scale: 1,
      pages: ["media", "clock", "weather", "battery", "timer", "quick", "tools", "stats", "reminder", "notes", "clipboard", "shelf", "lyrics", "privacy", "prayer"],
      timing: { hub_auto_ms: 4000, card_ms: 3500, hud_ms: 1500, hover_open_ms: 150, hover_close_ms: 450, sleep_after_s: 60 },
      pomodoro: { focus_min: 25, break_min: 5, long_break_min: 15, rounds: 4 },
      appearance: { theme: "auto", background: "#000000", text: "", dark_bg: "#000000", light_bg: "#f5f5f7", accent: "", default_mood: "normal", eye_theme: "aurora", notch_theme: "default" },
  };
  ctx.CFG = { ...ctx.FALLBACK, ...((window as any).__CFG__ ?? {}) };
  ctx.T = ctx.CFG.timing;
  ctx.featureOn = (name: string) => ctx.CFG.pages.includes(name);
  ctx.privacyOn = ctx.featureOn("privacy");
  ctx.names = [];
  ctx.pageCount = 0;
  ctx.page = 0;
  ctx.setNavActive = () => { };
  ctx.rebuildHubPagesFn = undefined;
  ctx.missing = new Map<string, HTMLElement>();
  ctx.$ = (id: string): HTMLElement => {
      const el = document.getElementById(id);
      if (el)
          return el;
      let stub = ctx.missing.get(id);
      if (!stub) {
          stub = document.createElement("div");
          ctx.missing.set(id, stub);
      }
      return stub;
  };
  ctx.notch = ctx.$("notch");
  // Pengaman: #notch overflow:hidden tidak boleh tergulir (mis. oleh scrollIntoView / fokus input),
  // karena isi pil akan bergeser keluar dan terlihat kosong.
  ctx.notch.addEventListener("scroll", () => { ctx.notch.scrollTop = 0; ctx.notch.scrollLeft = 0; });
  ctx.eyes = document.querySelector<HTMLElement>(".eyes")!;
  ctx.eyeEls = document.querySelectorAll<HTMLElement>(".eye");
  ctx.isWin = navigator.userAgent.includes("Windows");
  ctx.override = null;
  ctx.playing = false;
  ctx.hideTimer = undefined;
  ctx.lowBattery = false;
  ctx.keepAwake = false;
  ctx.prayerAdhanActive = false;
  ctx.resumeMediaAfterAdhan = false;
  ctx.inside = false;
  ctx.insideSince = 0;
  ctx.hubByHover = false;
  ctx.closeAt = 0;
  ctx.selectHoldUntil = 0;
  ctx.selectChosen = false;
  ctx.settleUntil = 0;
  ctx.holdHover = false;
  ctx.currentCx = -1;
  ctx.currentTy = 0;
  ctx.clockOn = ctx.featureOn("clock");
  ctx.idleFace = ctx.clockOn ? ctx.loadFace() : "eyes";
  ctx.BASE_SIZES = {
      idle: [150, 28],
      compact: [250, 40],
      compactLyrics: [360, 40],
      edgeIdle: [80, 80],
      edgeMedia: [80, 120],
      bottomIdle: [100, 44],
      bottomMedia: [140, 60],
      hub: [580, 202],
      bt: [360, 80],
      dev: [360, 80],
      rem: [360, 80],
      prayerAlert: [360, 80],
      hud: [320, 60],
      update: [460, 104],
      lyr: [420, 230],
      clock: [240, 28],
      cal: [420, 336],
  };
  ctx.BASE_HIT = {
      idle: [130, 24],
      compact: [230, 34],
      compactLyrics: [340, 34],
      edgeIdle: [60, 80],
      edgeMedia: [60, 120],
      bottomIdle: [80, 44],
      update: [460, 104],
      bottomMedia: [120, 60],
      hub: [560, 242], // mencakup tombol pusatkan yang mengambang di bawah hub
      bt: [340, 74],
      dev: [340, 74],
      rem: [340, 74],
      prayerAlert: [340, 74],
      hud: [300, 52],
      lyr: [400, 268], // mencakup tombol pusatkan di bawah
      clock: [220, 24],
      cal: [400, 360],
  };
  ctx.scaleMap = (m: Record<string, [
      number,
      number
  ]>) => Object.fromEntries(Object.entries(m).map(([k, [w, h]]) => [k, [Math.round(w * ctx.CFG.scale), Math.round(h * ctx.CFG.scale)]])) as Record<string, [
      number,
      number
  ]>;
  ctx.SIZES = ctx.scaleMap(ctx.BASE_SIZES);
  ctx.HIT = ctx.scaleMap(ctx.BASE_HIT);
  document.documentElement.style.setProperty("--s", String(ctx.CFG.scale));
  ctx.hexRgb = (h: string): [
      number,
      number,
      number
  ] | null => {
      const m = /^#?([0-9a-f]{6}|[0-9a-f]{3})$/i.exec((h ?? "").trim());
      if (!m)
          return null;
      let v = m[1];
      if (v.length === 3)
          v = v.split("").map((c) => c + c).join("");
      const n = parseInt(v, 16);
      return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
  };
  ctx.luma = (c: number[]) => (0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2]) / 255;
  // Aksen dianggap tetap bila diisi manual ATAU memakai tema notch (aksen dari tema, bukan dari cover album).
  ctx.fixedAccent = () => ctx.hexRgb(ctx.CFG.appearance.accent) !== null ||
      (!!ctx.CFG.appearance.notch_theme && ctx.CFG.appearance.notch_theme !== "default");
  ctx.osLight = matchMedia("(prefers-color-scheme: light)");
  ctx.applyTheme();
  ctx.osLight.addEventListener("change", ctx.applyTheme); // ikut berubah saat tema OS diganti
  
  // dari jendela pengaturan: pratinjau langsung / hasil simpan
  ctx.listen<{
      appearance?: Appearance;
      scale?: number;
      pages?: string[];
  }>("config-changed", (e) => {
      const p = e.payload;
      if (!p)
          return;
      if (p.appearance) {
          ctx.CFG.appearance = p.appearance;
          ctx.applyTheme();
      }
      if (typeof p.scale === "number" && p.scale !== ctx.CFG.scale)
          ctx.applyScale(p.scale);
      // Perubahan halaman dipratinjau langsung dari Settings. Jangan membuang elemen DOM:
      // simpan semua halaman lalu tampilkan/urutkan yang aktif agar halaman bisa diaktifkan lagi.
      if (Array.isArray(p.pages)) {
          const nextPages = p.pages.filter((name): name is string => typeof name === "string");
          const pagesChanged = nextPages.length !== ctx.CFG.pages.length ||
              nextPages.some((name, i) => name !== ctx.CFG.pages[i]);
          if (pagesChanged) {
              const clockWasOn = ctx.clockOn;
              ctx.CFG.pages = [...nextPages];
              ctx.clockOn = ctx.featureOn("clock");
              if (!ctx.clockOn && ctx.idleFace === "clock") {
                  ctx.idleFace = "eyes";
              }
              else if (!clockWasOn && ctx.clockOn) {
                  ctx.idleFace = ctx.loadFace();
              }
              ctx.rebuildHubPagesFn?.(ctx.CFG.pages);
          }
          const on = ctx.CFG.pages.includes("privacy");
          if (on !== ctx.privacyOn) {
              ctx.privacyOn = on;
              ctx.paintPrivacy();
          }
      }
  });
}