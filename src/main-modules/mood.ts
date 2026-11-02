// @ts-nocheck
// Mood, aktivitas, dan perilaku tidur karakter.
// ---------- mood: dasar + sementara ----------
type Mood = "normal" | "happy" | "sleepy" | "surprised" | "angry" | "love" | "suspicious" | "sad";

export function register(ctx: Record<string, any>): void {
  ctx.setBase = function setBase(m: Mood) {
      ctx.baseMood = m;
      if (ctx.tempTimer === undefined)
          ctx.applyMood(m);
  };
  ctx.flash = function flash(m: Mood, ms = 1500) {
      ctx.applyMood(m);
      clearTimeout(ctx.tempTimer);
      ctx.tempTimer = window.setTimeout(() => { ctx.tempTimer = undefined; ctx.applyMood(ctx.baseMood); }, ms);
  };
  ctx.updateBase = function updateBase() {
      const hour = new Date().getHours();
      const idleLong = Date.now() - ctx.lastActivity > ctx.T.sleep_after_s * 1000;
      const night = hour >= 0 && hour < 5;
      // Keep awake aktif -> mata tidak mengantuk
      const shouldSleep = (idleLong || night) && !ctx.keepAwake;
      if (ctx.sleeping && !shouldSleep)
          ctx.flash("surprised", 900);
      ctx.sleeping = shouldSleep;
      ctx.setBase(shouldSleep || ctx.lowBattery ? "sleepy" : ctx.playing ? "happy" : ctx.defaultMood());
  };
  ctx.markActive = function markActive() { ctx.noteActivity(); ctx.lastActivity = Date.now(); if (ctx.sleeping)
      ctx.updateBase(); };
}

export function init(ctx: Record<string, any>): void {
  ctx.MOODS = ["normal", "happy", "sleepy", "surprised", "angry", "love", "suspicious", "sad"];
  ctx.defaultMood = (): Mood => {
      const m = ctx.CFG.appearance.default_mood as Mood;
      return ctx.MOODS.includes(m) ? m : "normal";
  };
  ctx.baseMood = ctx.defaultMood();
  ctx.tempTimer = undefined;
  ctx.sleeping = false;
  ctx.lastActivity = Date.now();
  ctx.applyMood = (m: Mood) => { ctx.eyes.dataset.mood = m; };
  setInterval(ctx.updateBase, 10_000);
  document.addEventListener("mousemove", ctx.markActive);
  document.addEventListener("keydown", ctx.markActive);
}
