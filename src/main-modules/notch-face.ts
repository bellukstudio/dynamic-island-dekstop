// @ts-nocheck
// Klik notch, kedip, tatapan, dan warna artwork.

export function register(ctx: Record<string, any>): void {
  ctx.blinkLoop = function blinkLoop() {
      const m = ctx.eyes.dataset.mood;
      if (m !== "love") {
          ctx.eyeEls.forEach((e) => e.classList.add("blink"));
          setTimeout(() => ctx.eyeEls.forEach((e) => e.classList.remove("blink")), 130);
      }
      setTimeout(ctx.blinkLoop, (m === "sleepy" ? 1500 : 2500) + Math.random() * 3500);
  };
  ctx.look = function look(dx: number, dy: number) {
      const max = 6;
      const dist = Math.hypot(dx, dy) || 1;
      const k = Math.min(1, dist / 300);
      // bulatkan ke 0,25px & lewati kalau tidak berubah: tiap setProperty = style recalc + repaint jendela transparan
      const lx = Math.round((dx / dist) * max * k * 4) / 4;
      const ly = Math.round(Math.max(-1, Math.min(1, (dy / dist) * max * k)) * 4) / 4;
      if (lx === ctx._lookX && ly === ctx._lookY)
          return;
      ctx._lookX = lx;
      ctx._lookY = ly;
      ctx.eyes.style.setProperty("--lx", `${lx}px`);
      ctx.eyes.style.setProperty("--ly", `${ly}px`);
  };
  ctx.accentFrom = function accentFrom(img: HTMLImageElement) {
      if (ctx.fixedAccent())
          return; // aksen tetap dari config
      try {
          const c = document.createElement("canvas");
          c.width = c.height = 16;
          const x = c.getContext("2d")!;
          x.drawImage(img, 0, 0, 16, 16);
          const d = x.getImageData(0, 0, 16, 16).data;
          let r = 0, g = 0, b = 0, w = 0;
          for (let i = 0; i < d.length; i += 4) {
              const mx = Math.max(d[i], d[i + 1], d[i + 2]);
              const mn = Math.min(d[i], d[i + 1], d[i + 2]);
              const s = (mx - mn) / (mx || 1);
              const wt = s * s + 0.05;
              r += d[i] * wt;
              g += d[i + 1] * wt;
              b += d[i + 2] * wt;
              w += wt;
          }
          r /= w;
          g /= w;
          b /= w;
          const k = 210 / (Math.max(r, g, b) || 1);
          document.documentElement.style.setProperty("--accent", `${Math.min(255, r * k) | 0}, ${Math.min(255, g * k) | 0}, ${Math.min(255, b * k) | 0}`);
      }
      catch { /* artwork lintas domain tidak bisa dibaca, pakai warna default */ }
  };
}

export function init(ctx: Record<string, any>): void {
  ctx.clicks = [];
  ctx.notch.addEventListener("click", (e) => {
      if (Date.now() < ctx.suppressClickUntil) {
          e.stopPropagation();
          return;
      }
      const now = Date.now();
      ctx.clicks = ctx.clicks.filter((t) => now - t < 2000).concat(now);
      if (ctx.clicks.length >= 5) {
          ctx.flash("angry", 2000);
          ctx.clicks = [];
      }
      else
          ctx.flash("happy", 1200);
      // kartu pengingat (rem) tidak ditutup lewat klik: hanya lewat tombol Tunda / Selesai
      if (ctx.prayerAdhanActive)
          return; // adzan sedang berbunyi: klik tidak menutup alert/awan dan tidak membuka hub
      if (ctx.override === "bt" || ctx.override === "dev" || ctx.override === "hud" || ctx.override === "prayerAlert")
          ctx.collapse();
      else if (!ctx.override)
          ctx.openHub(ctx.homePage());
  });
  ctx.notch.addEventListener("dblclick", () => ctx.flash("love", 2500));
  ctx.blinkLoop();
  ctx.lastCursorMove = 0;
  ctx.lastPos = { dx: 0, dy: 0 };
  ctx.listen<{
      dx: number;
      dy: number;
  }>("cursor", (e) => {
      const { dx, dy } = e.payload;
      if (Math.hypot(dx - ctx.lastPos.dx, dy - ctx.lastPos.dy) > 2) {
          ctx.lastCursorMove = Date.now();
          ctx.markActive();
      }
      ctx.lastPos = { dx, dy };
      const v = ctx.notch.dataset.view ?? "idle";
      const hr = ctx.hitRect(v);
      const [WW] = ctx.winLogical();
      ctx.look(dx - (hr.x + hr.w / 2 - WW / 2), dy - hr.y); // mata menghadap kursor dari tengah notch, bukan tengah jendela
      // Windows: hover dihitung dari posisi global (jendela click-through tidak menerima event mouse)
      if (ctx.isWin) {
          const px = WW / 2 + dx, py = dy + 12;
          ctx.setInside(px >= hr.x && px <= hr.x + hr.w && py >= hr.y && py <= hr.y + hr.h);
      }
  });
  setInterval(() => {
      if (Date.now() - ctx.lastCursorMove > 5000)
          ctx.look((Math.random() - 0.5) * 300, (Math.random() - 0.5) * 60);
  }, 2500);
}
