// @ts-nocheck
// Drag jendela, snapping, dan animasi ORB.

export function register(ctx: Record<string, any>): void {
  ctx.updatePositionUI = function updatePositionUI(custom: boolean, topY: number, cx?: number) {
      if (cx !== undefined)
          ctx.currentCx = cx;
      ctx.currentTy = topY;
      const shell = ctx.$("shell");
      const bottom = custom && ctx.isBottomTy(topY);
      const topOffset = topY - ctx.MON.y;
      // sisi layar: cx sudah di-snap ke tepi monitor (zona snap 70px)
      let side: "left" | "right" | null = null;
      if (custom && !bottom && ctx.currentCx !== -1) {
          if (ctx.currentCx - ctx.MON.x <= 70)
              side = "left";
          else if (ctx.MON.x + ctx.MON.w - ctx.currentCx <= 70)
              side = "right";
      }
      // Saat sedang ditahan dan mendekati tepi atas, hentikan efek drag lebih awal.
      // Kalau menunggu sampai koordinat benar-benar tersnap, animasi scale masih aktif
      // selama beberapa frame dan telinga #shell tampak terpisah dari badan #notch.
      const topApproach = ctx.isDraggingWindow && !bottom && side === null &&
          topOffset > 8 && topOffset <= Math.round(96 * ctx.CFG.scale);
      const isFloating = custom && !bottom && topOffset > 8;
      ctx.notch.classList.toggle("is-custom-pos", custom);
      ctx.notch.classList.toggle("is-floating", isFloating);
      ctx.notch.classList.toggle("is-bottom", bottom);
      ctx.notch.classList.toggle("is-top-approach", topApproach);
      shell.classList.toggle("is-floating", isFloating);
      shell.classList.toggle("is-bottom", bottom);
      shell.classList.toggle("is-top-approach", topApproach);
      const changed = ctx.notch.classList.contains("is-edge-left") !== (side === "left") ||
          ctx.notch.classList.contains("is-edge-right") !== (side === "right");
      ctx.notch.classList.toggle("is-edge-left", side === "left");
      ctx.notch.classList.toggle("is-edge-right", side === "right");
      shell.classList.toggle("is-edge-left", side === "left");
      shell.classList.toggle("is-edge-right", side === "right");
      // Ketika magnet ke tepi atas, hentikan efek mengecil/memantul saat drag.
      // Telinga lengkung tetap terlihat, tetapi badan notch tidak membentuk celah di tengah.
      const topDocked = !bottom && !isFloating && topOffset <= 8 && side === null;
      ctx.notch.classList.toggle("is-top", topDocked);
      shell.classList.toggle("is-top", topDocked);
      ctx.placeShell();
      if (changed)
          ctx.render();
      // bola menempel ke tepi (atas/bawah/kiri/kanan) -> beri denyut cincin sekali
      if (ctx.orbOn) {
          const dock = bottom ? "b" : side ? side : topDocked ? "t" : "";
          if (dock !== ctx.orbDock) {
              ctx.orbDock = dock;
              if (dock)
                  ctx.popOrb();
          }
      }
  };
  ctx.snapPosition = function snapPosition(rawCx: number, rawTy: number, curX: number, curY: number) {
      const L = ctx.MON.x, R = ctx.MON.x + ctx.MON.w, Tp = ctx.MON.y;
      // tepi bawah: pakai yang lebih kecil antara ukuran monitor dari Rust dan ukuran layar webview
      const B = ctx.MON.y + Math.min(ctx.MON.h, window.screen.height || ctx.MON.h);
      let cx = rawCx, ty = rawTy;
      const wasL = ctx.notch.classList.contains("is-edge-left");
      const wasR = ctx.notch.classList.contains("is-edge-right");
      const wasB = ctx.notch.classList.contains("is-bottom");
      const wasT = !wasB && !ctx.notch.classList.contains("is-floating");
      // Bawah ditentukan dari posisi KURSOR (kursor tidak bisa melewati tepi layar, jadi selalu tercapai).
      // Menghitung dari posisi pil gagal kalau titik pegang pil tidak sama dengan titik awal drag.
      const nearBottom = B - curY <= (wasB ? 90 : 48) || B - (rawTy + 28 * ctx.CFG.scale) <= (wasB ? 60 : 24);
      if (nearBottom) {
          ty = ctx.BOTTOM_TY;
          cx = rawCx; // di dasar layar: cx bebas, tidak ikut menjadi bubble sisi
      }
      else {
          if (rawTy - Tp <= (wasT ? 60 : 24))
              ty = Tp;
          if (rawCx - L <= (wasL ? 110 : 70) || curX - L <= (wasL ? 90 : 40))
              cx = L;
          else if (R - rawCx <= (wasR ? 110 : 70) || R - curX <= (wasR ? 90 : 40))
              cx = R;
      }
      return { cx, ty };
  };
  ctx.startWindowDrag = async function startWindowDrag(e: PointerEvent, targetEl: HTMLElement) {
      if (e.button !== 0)
          return; // hanya klik kiri
      try {
          const pos = await ctx.invoke<[
              number,
              number
          ]>("notch_start_move");
          const s = ctx.CFG.scale;
          ctx.startCx = ctx.notch.classList.contains("is-edge-left") ? ctx.MON.x + 30 * s
              : ctx.notch.classList.contains("is-edge-right") ? ctx.MON.x + ctx.MON.w - 30 * s
                  : pos[0];
          ctx.startTy = ctx.isBottomTy(pos[1]) ? ctx.MON.y + ctx.MON.h - 28 * s : pos[1];
          ctx.startScreenX = e.screenX;
          ctx.startScreenY = e.screenY;
          ctx.isDraggingWindow = true;
          ctx.syncPosBar();
          ctx.windowDragMoved = false;
          ctx.lastSent = { cx: NaN, ty: NaN };
          ctx.lastPtr = { x: e.screenX, y: e.screenY };
          ctx.$("shell").classList.add("moving");
          // is-dragging / is-orb baru dipasang setelah kursor benar-benar bergeser (lihat enterOrb),
          // supaya klik biasa tidak memicu animasi bola.
          targetEl.setPointerCapture(e.pointerId);
      }
      catch (err) {
          console.error("Gagal memulai drag window:", err);
      }
  };
  ctx.popOrb = function popOrb() {
      const shell = ctx.$("shell");
      shell.classList.remove("orb-pop");
      void shell.offsetWidth; // restart animasi
      shell.classList.add("orb-pop");
  };
  ctx.enterOrb = function enterOrb() {
      if (ctx.orbOn)
          return;
      ctx.orbOn = true;
      ctx.orbT0 = performance.now();
      ctx.orbPx = ctx.lastPtr.x;
      ctx.orbPy = ctx.lastPtr.y;
      ctx.orbVx = 0;
      ctx.orbVy = 0;
      ctx.orbDock = "";
      clearTimeout(ctx.orbOutTimer);
      const shell = ctx.$("shell");
      ctx.notch.classList.remove("is-orb-out");
      shell.classList.remove("is-orb-out");
      ctx.notch.classList.add("is-dragging", "is-orb");
      ctx.notch.style.setProperty("--orb-d", `${ctx.orbD()}px`);
      shell.classList.add("is-orb");
      ctx.placeShell();
      ctx.setHit(ctx.notch.dataset.view ?? "idle");
      ctx.orbRaf = requestAnimationFrame(ctx.orbFrame);
  };
  ctx.exitOrb = function exitOrb() {
      if (!ctx.orbOn)
          return;
      ctx.orbOn = false;
      cancelAnimationFrame(ctx.orbRaf);
      const shell = ctx.$("shell");
      ctx.notch.classList.remove("is-orb");
      shell.classList.remove("is-orb", "orb-pop");
      // kelas "-out" memberi transisi pegas saat bola kembali ke bentuk semula
      ctx.notch.classList.add("is-orb-out");
      shell.classList.add("is-orb-out");
      for (const v of ["--orb-rot", "--orb-sx", "--orb-sy", "--orb-d"])
          ctx.notch.style.removeProperty(v);
      clearTimeout(ctx.orbOutTimer);
      ctx.orbOutTimer = window.setTimeout(() => {
          ctx.notch.classList.remove("is-orb-out");
          shell.classList.remove("is-orb-out");
      }, 560);
  };
  ctx.orbFrame = function orbFrame() {
      if (!ctx.orbOn)
          return;
      ctx.applyDrag();
      const dxp = ctx.lastPtr.x - ctx.orbPx, dyp = ctx.lastPtr.y - ctx.orbPy;
      ctx.orbPx = ctx.lastPtr.x;
      ctx.orbPy = ctx.lastPtr.y;
      ctx.orbVx += (dxp - ctx.orbVx) * 0.22;
      ctx.orbVy += (dyp - ctx.orbVy) * 0.22;
      const amt = Math.min(Math.hypot(ctx.orbVx, ctx.orbVy) / (36 * ctx.CFG.scale), 1);
      const stretch = amt * (ctx.orbDock ? 0.3 : 1) * 0.32; // menempel di tepi: stretch diredam
      ctx.notch.style.setProperty("--orb-rot", `${Math.atan2(ctx.orbVy, ctx.orbVx).toFixed(3)}rad`);
      ctx.notch.style.setProperty("--orb-sx", (1 + stretch).toFixed(3));
      ctx.notch.style.setProperty("--orb-sy", (1 - stretch * 0.5).toFixed(3));
      ctx.orbRaf = requestAnimationFrame(ctx.orbFrame);
  };
  ctx.applyDrag = function applyDrag() {
      const dx = ctx.lastPtr.x - ctx.startScreenX;
      const dy = ctx.lastPtr.y - ctx.startScreenY;
      let rawCx = ctx.startCx + dx;
      let rawTy = ctx.startTy + dy;
      if (ctx.orbOn) {
          // geser titik pegang secara halus (260ms) sampai bola tepat berada di bawah kursor
          const t = Math.min(1, (performance.now() - ctx.orbT0) / 260);
          const k = 1 - Math.pow(1 - t, 3);
          rawCx += k * (ctx.startScreenX - ctx.startCx);
          rawTy += k * (ctx.startScreenY - ctx.orbD() / 2 - ctx.startTy);
      }
      const s = ctx.snapPosition(rawCx, rawTy, ctx.lastPtr.x, ctx.lastPtr.y);
      if (s.cx !== ctx.lastSent.cx || s.ty !== ctx.lastSent.ty) { // jangan kirim ulang yang sama
          ctx.lastSent = s;
          const sendTy = ctx.isBottomTy(s.ty) ? s.ty : Math.max(ctx.MON.y, s.ty - ctx.orbPad());
          ctx.invoke("notch_move_to", { cx: s.cx, ty: sendTy });
          ctx.updatePositionUI(true, s.ty, s.cx);
      }
  };
  ctx.onWindowPointerMove = function onWindowPointerMove(e: PointerEvent) {
      if (!ctx.isDraggingWindow)
          return;
      ctx.lastPtr = { x: e.screenX, y: e.screenY };
      const dx = e.screenX - ctx.startScreenX;
      const dy = e.screenY - ctx.startScreenY;
      if (!ctx.windowDragMoved && (Math.abs(dx) > 3 || Math.abs(dy) > 3)) {
          ctx.windowDragMoved = true;
          ctx.enterOrb(); // mulai jadi bola
      }
      if (ctx.windowDragMoved) {
          ctx.applyDrag();
          ctx.setBase("love");
      }
  };
  ctx.onWindowPointerUp = function onWindowPointerUp(e: PointerEvent, targetEl: HTMLElement) {
      if (!ctx.isDraggingWindow)
          return;
      ctx.isDraggingWindow = false;
      ctx.syncPosBar();
      ctx.exitOrb(); // bola -> kembali ke bentuk notch semula (tepat sebelum posisi dihitung ulang)
      ctx.notch.classList.remove("is-dragging");
      ctx.$("shell").classList.remove("moving");
      if (ctx.windowDragMoved)
          ctx.holdHover = true;
      try {
          targetEl.releasePointerCapture(e.pointerId);
      }
      catch { }
      if (ctx.windowDragMoved && !ctx.isBottomTy(ctx.lastSent.ty)) {
          ctx.invoke("notch_move_to", { cx: ctx.lastSent.cx, ty: ctx.lastSent.ty });
      }
      if (ctx.windowDragMoved) {
          ctx.suppressClickUntil = Date.now() + 350;
          ctx.insideSince = Date.now();
          ctx.invoke("notch_end_move", { save: true });
      }
      else {
          ctx.invoke("notch_end_move", { save: false });
      }
      // Drag selesai: hitung ulang posisi melayang dan area klik supaya tetap sinkron.
      if (ctx.windowDragMoved) {
          ctx.updatePositionUI(true, ctx.currentTy, ctx.currentCx);
          ctx.setHit(ctx.notch.dataset.view ?? "idle");
      }
      // Hitung target sekali lagi agar notch langsung masuk mode tepi yang benar.
      ctx.render();
  };
  ctx.resetPosition = async function resetPosition() {
      try {
          await ctx.invoke("notch_reset_position");
          ctx.updatePositionUI(false, 0, -1);
          ctx.flash("happy", 1200);
      }
      catch (err) {
          console.error("Gagal reset posisi notch:", err);
      }
  };
}

export function init(ctx: Record<string, any>): void {
  ctx.isDraggingWindow = false;
  ctx.startScreenX = 0;
  ctx.startScreenY = 0;
  ctx.startCx = 0;
  ctx.startTy = 0;
  ctx.windowDragMoved = false;
  ctx.suppressClickUntil = 0;
  ctx.lastSent = { cx: NaN, ty: NaN };
  ctx.orbFx = document.createElement("div");
  ctx.orbFx.id = "orb-fx";
  ctx.orbFx.setAttribute("aria-hidden", "true");
  ctx.$("shell").appendChild(ctx.orbFx);
  ctx.NO_DRAG = "button, input, textarea, select, a, .dot, .bar";
  ctx.notch.addEventListener("pointerdown", (e) => {
      if ((e.target as HTMLElement).closest(ctx.NO_DRAG))
          return;
      ctx.startWindowDrag(e, ctx.notch);
  });
  ctx.notch.addEventListener("pointermove", ctx.onWindowPointerMove);
  ctx.notch.addEventListener("pointerup", (e) => ctx.onWindowPointerUp(e, ctx.notch));
  ctx.notch.addEventListener("pointercancel", (e) => ctx.onWindowPointerUp(e, ctx.notch));
  // Tombol reset posisi
  ctx.$("notch-reset-btn").addEventListener("click", (e) => {
      e.stopPropagation();
      ctx.resetPosition();
  });
  ctx.$("q-reset-pos").addEventListener("click", (e) => {
      e.stopPropagation();
      ctx.resetPosition();
  });
  // Dengarkan perubahan posisi dari tray atau jendela pengaturan
  ctx.listen<{
      custom: boolean;
      x?: number;
      y?: number;
  }>("position-changed", (e) => {
      const p = e.payload;
      ctx.updatePositionUI(p?.custom ?? false, p?.y ?? 0, p?.x);
  });
  // Inisialisasi status posisi saat startup
  ctx.invoke<{
      custom: boolean;
      x: number;
      y: number;
  }>("notch_get_position")
      .then((p) => {
      if (p.custom) {
          // Posisi kustom: pakai cx & ty yang tersimpan
          ctx.updatePositionUI(true, p.y, p.x);
      }
      else {
          // Posisi default: pusatkan notch di tengah atas monitor (MON.w / 2)
          const centerCx = ctx.MON.x + Math.round(ctx.MON.w / 2);
          ctx.updatePositionUI(false, 0, centerCx);
      }
  })
      .catch(() => {
      // Fallback: pusatkan menggunakan screen.width
      const centerCx = Math.round(window.screen.width / 2);
      ctx.updatePositionUI(false, 0, centerCx);
  });
}
