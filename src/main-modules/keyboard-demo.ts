// @ts-nocheck
// Pintasan keyboard dan demo manual.

export function register(ctx: Record<string, any>): void {
}

export function init(ctx: Record<string, any>): void {
  ctx.keyMoods = ["normal", "happy", "sleepy", "surprised", "angry", "love", "suspicious"];
  ctx.demo = Object.keys(ctx.ICONS);
  ctx.demoI = 0;
  document.addEventListener("keydown", (e) => {
      // jangan jalankan pintasan demo saat mengetik (mis. angka di kolom Currency)
      if (ctx.typing() || e.target instanceof HTMLSelectElement)
          return;
      const k = e.key.toLowerCase();
      const i = Number(e.key) - 1;
      if (i >= 0 && i < ctx.keyMoods.length)
          ctx.flash(ctx.keyMoods[i], 3000);
      if (k === "b") {
          void ctx.openBtManual();
      }
      if (k === "c")
          ctx.handleBattery(80, false);
      if (k === "v")
          ctx.handleBattery(80, true);
      if (k === "l")
          ctx.handleBattery(10, false);
      if (k === "d") {
          const kind = ctx.demo[ctx.demoI++ % ctx.demo.length];
          ctx.showDevice(kind, `Contoh ${ctx.LABELS[kind]}`, ctx.demoI % 4 !== 0);
      }
      if (k === "w") {
          ctx.showWeather({ temp: 27, feels: 30, humidity: 82, code: 63, is_day: true, rain_in: 30 }, true);
          ctx.openHub(ctx.pageOf("weather"), 5000);
      }
      if (k === "h")
          ctx.openHub(ctx.page, 5000);
      if (k === "t" && ctx.clockOn) {
          ctx.idleFace = ctx.idleFace === "clock" ? "eyes" : "clock";
          ctx.saveFace(ctx.idleFace);
          ctx.render();
      }
      if (k === "m") {
          if (ctx.notch.hasAttribute("data-priv"))
              ctx.notch.removeAttribute("data-priv");
          else {
              ctx.notch.dataset.priv = "cam mic";
              ctx.showDevice("camera", "Kamera aktif", true, "Dipakai demo");
          }
      }
      if (k === "u")
          ctx.showHud("volume", 20 + Math.random() * 80, false);
      if (k === "i")
          ctx.showHud("brightness", 10 + Math.random() * 90, false);
      if (k === "p") {
          ctx.remaining = 3;
          ctx.running = true;
          ctx.endAt = Date.now() + 3000;
          ctx.openHub(ctx.pageOf("timer"), 6000);
      }
      if (e.key === "ArrowRight" && ctx.override === "hub")
          ctx.setPage(ctx.page + 1);
      if (e.key === "ArrowLeft" && ctx.override === "hub")
          ctx.setPage(ctx.page - 1);
      if (k === "k" && ctx.clockOn)
          ctx.openCalendar();
      if (ctx.override === "cal") {
          if (e.key === "ArrowRight")
              ctx.shiftMonth(1);
          if (e.key === "ArrowLeft")
              ctx.shiftMonth(-1);
          if (e.key === "Escape")
              ctx.openHub(ctx.pageOf("clock"));
      }
  });
}
