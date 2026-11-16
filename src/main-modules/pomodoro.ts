// @ts-nocheck
// Timer dan Pomodoro.
// ---------- timer / pomodoro ----------
type Phase = "focus" | "break" | "long";

export function register(ctx: Record<string, any>): void {
  ctx.renderPom = function renderPom() {
      if (!document.getElementById("pom-time"))
          return; // halaman timer dimatikan di config
      const m = Math.floor(ctx.remaining / 60), s = ctx.remaining % 60;
      ctx.$("pom-time").textContent = `${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
      ctx.$("pom-label").textContent =
          ctx.phase === "focus" ? `Fokus · ronde ${ctx.round}/${ctx.POM.rounds}` : ctx.phase === "break" ? "Istirahat" : "Istirahat panjang";
      const total = ctx.phaseLen(ctx.phase);
      const fill = ctx.$("pom-fill");
      fill.style.width = `${((total - ctx.remaining) / total) * 100}%`;
      fill.classList.toggle("rest", ctx.phase !== "focus");
      // ikon play/pause diatur langsung supaya tidak ikut status media
      const t = ctx.$("pom-toggle");
      t.classList.toggle("running", ctx.running);
      t.querySelector<HTMLElement>(".i-play")!.style.display = ctx.running ? "none" : "block";
      t.querySelector<HTMLElement>(".i-pause")!.style.display = ctx.running ? "block" : "none";
  };
  ctx.nextPhase = function nextPhase(auto: boolean) {
      ctx.running = false;
      const finished = ctx.phase;
      if (ctx.phase === "focus")
          ctx.phase = ctx.round >= ctx.POM.rounds ? "long" : "break";
      else {
          if (ctx.phase === "long")
              ctx.round = 1;
          else
              ctx.round++;
          ctx.phase = "focus";
      }
      ctx.remaining = ctx.phaseLen(ctx.phase);
      ctx.renderPom();
      if (auto) {
          // waktu habis: buka halaman timer dan beri tahu lewat mata
          ctx.hubByHover = false;
          ctx.openHub(ctx.pageOf("timer"), 6000);
          ctx.flash(finished === "focus" ? "love" : "surprised", 3000);
          ctx.focusFinished(finished, ctx.phase, ctx.round, ctx.POM.rounds); // Focus Companion
      }
  };
}

export function init(ctx: Record<string, any>): void {
  ctx.POM = ctx.CFG.pomodoro;
  ctx.phase = "focus";
  ctx.round = 1;
  ctx.running = false;
  ctx.endAt = 0;
  ctx.phaseLen = (p: Phase) => (p === "focus" ? ctx.POM.focus_min : p === "break" ? ctx.POM.break_min : ctx.POM.long_break_min) * 60;
  ctx.remaining = ctx.phaseLen("focus");
  setInterval(() => {
      if (!ctx.running)
          return;
      ctx.remaining = Math.max(0, Math.round((ctx.endAt - Date.now()) / 1000));
      ctx.renderPom();
      ctx.focusTick(ctx.phase, ctx.round, ctx.phaseLen(ctx.phase), ctx.remaining, ctx.running); // Focus Companion: setengah sesi
      if (ctx.remaining <= 0)
          ctx.nextPhase(true);
  }, 1000);
  ctx.pomBtn = (id: string, fn: () => void) => ctx.$(id).addEventListener("click", (e) => { e.stopPropagation(); fn(); ctx.renderPom(); });
  ctx.pomBtn("pom-toggle", () => {
      ctx.running = !ctx.running;
      if (ctx.running) {
          ctx.endAt = Date.now() + ctx.remaining * 1000;
          // Focus Companion: hanya saat sesi fokus dimulai dari awal, bukan saat dilanjutkan setelah jeda
          if (ctx.phase === "focus" && ctx.remaining >= ctx.phaseLen("focus") - 2)
              ctx.focusStarted(ctx.round, ctx.POM.rounds);
      }
  });
  ctx.pomBtn("pom-reset", () => { ctx.running = false; ctx.remaining = ctx.phaseLen(ctx.phase); });
  ctx.pomBtn("pom-skip", () => ctx.nextPhase(false));
  ctx.renderPom();
}
