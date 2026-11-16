// @ts-nocheck
// Cuaca dan HUD volume / brightness.
// ---------- cuaca ----------
type Weather = {
    temp: number;
    feels: number;
    humidity: number;
    code: number;
    is_day: boolean;
    rain_in: number | null;
};

export function register(ctx: Record<string, any>): void {
  ctx.describe = function describe(code: number, day: boolean): {
      kind: string;
      emoji: string;
      text: string;
  } {
      if (code === 0)
          return day
              ? { kind: "clear", emoji: "☀️", text: "Cerah" }
              : { kind: "night", emoji: "🌙", text: "Cerah (malam)" };
      if (code <= 2)
          return { kind: day ? "clear" : "night", emoji: day ? "🌤️" : "☁️", text: "Cerah berawan" };
      if (code === 3)
          return { kind: "cloud", emoji: "☁️", text: "Mendung" };
      if (code <= 48)
          return { kind: "fog", emoji: "🌫️", text: "Berkabut" };
      if (code <= 57)
          return { kind: "rain", emoji: "🌦️", text: "Gerimis" };
      if (code <= 67 || (code >= 80 && code <= 82))
          return { kind: "rain", emoji: "🌧️", text: "Hujan" };
      if (code >= 95)
          return { kind: "storm", emoji: "⛈️", text: "Badai petir" };
      return { kind: "cloud", emoji: "☁️", text: "Berawan" };
  };
  ctx.showWeather = function showWeather(w: Weather, alert = false) {
      const d = ctx.describe(w.code, w.is_day);
      ctx.$("wx-icon").dataset.kind = d.kind;
      ctx.$("wx-emoji").textContent = d.emoji;
      ctx.$("wx-temp").textContent = `${Math.round(w.temp)}°`;
      ctx.$("wx-desc").textContent = alert && w.rain_in ? `Hujan ${w.rain_in} menit lagi` : d.text;
      ctx.$("wx-feels").textContent = `Terasa ${Math.round(w.feels)}°`;
      ctx.$("wx-hum").textContent = `Lembap ${Math.round(w.humidity)}%`;
  };
  ctx.showHud = function showHud(kind: string, value: number, muted: boolean) {
      if (ctx.override === "rem")
          return; // jangan menimpa pengingat aktif
      if ((ctx.override === "hub" || ctx.override === "lyr" || ctx.override === "cal") && ctx.inside)
          return;
      const v = Math.max(0, Math.min(100, Math.round(value)));
      const icon = kind === "brightness" ? ctx.HUD_ICONS.sun
          : muted || v === 0 ? ctx.HUD_ICONS.mute
              : v < 50 ? ctx.HUD_ICONS.low
                  : ctx.HUD_ICONS.high;
      ctx.$("hud-icon").innerHTML = icon;
      const fill = ctx.$("hud-fill");
      fill.style.width = `${v}%`;
      fill.classList.toggle("muted", muted);
      ctx.$("hud-val").textContent = muted ? "mute" : String(v);
      ctx.hubByHover = false;
      const hudTitle = kind === "brightness" ? "Kecerahan" : "Volume";
      const hudSub = muted ? "Mute" : `${v}%`;
      // Saat notch dock di sisi/bawah, tampilkan HUD sebagai awan tanpa
      // mengubah bentuk notch menjadi kartu lebar. Posisi lain tetap memakai HUD lama.
      const shownAsBubble = ctx.momentBubble?.show(hudTitle, hudSub, ctx.T.hud_ms) === true;
      if (!shownAsBubble) ctx.expand("hud", ctx.T.hud_ms);
      if (kind === "volume") {
          if (muted || v === 0)
              ctx.flash("sleepy", 1500);
          else if (v >= 90)
              ctx.flash("surprised", 1200);
          else
              ctx.flash("happy", 1000);
      }
      else {
          ctx.flash(v < 25 ? "sleepy" : "happy", 1200);
      }
  };
}

export function init(ctx: Record<string, any>): void {
  ctx.listen<[
      Weather,
      boolean
  ]>("weather", (e) => {
      const [w, alert] = e.payload;
      ctx.showWeather(w, alert);
      ctx.noteWeather(w, alert, ctx.describe(w.code, w.is_day)); // Conversation Moments
      if (alert) {
          ctx.openHub(ctx.pageOf("weather"), 5000);
          ctx.flash("surprised", 1500);
      }
      else if (w.code === 0 && w.is_day) {
          ctx.flash("happy", 2000);
      }
  });
  ctx.HUD_ICONS = {
      mute: `<path d="M11 5 6 9H2v6h4l5 4z"/><path d="M23 9l-6 6M17 9l6 6"/>`,
      low: `<path d="M11 5 6 9H2v6h4l5 4z"/><path d="M15.5 8.5a5 5 0 0 1 0 7"/>`,
      high: `<path d="M11 5 6 9H2v6h4l5 4z"/><path d="M15.5 8.5a5 5 0 0 1 0 7M19 5a10 10 0 0 1 0 14"/>`,
      sun: `<circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"/>`,
  };
  ctx.listen<{
      kind: string;
      value: number;
      muted: boolean;
  }>("hud", (e) => ctx.showHud(e.payload.kind, e.payload.value, e.payload.muted));
}
