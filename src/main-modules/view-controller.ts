// @ts-nocheck
// Render view, ukuran adaptif, expand, dan collapse.

export function register(ctx: Record<string, any>): void {
  ctx.applySize = function applySize(view: string, previousView = ctx.appliedView || view) {
      const requestId = ++ctx.sizeRequestId;
      ctx.lastSizeRequestAt = Date.now();
      const [w, h] = ctx.SIZES[view];
      const [oldW, oldH] = ctx.SIZES[previousView] ?? [w, h];
      const isExpanding = w >= oldW && h >= oldH;
      const stillCurrent = () => requestId === ctx.sizeRequestId && ctx.lastView === view;
      ctx.placeShell(view);
      const finish = () => {
          if (!stillCurrent())
              return;
          // Resize native dijalankan sebelum hitbox akhir dipasang. Ketika menutup, finish
          // dipanggil setelah animasi CSS supaya panel yang sedang mengecil tidak terpotong.
          void ctx.syncNativeWindowSize(view).then(() => {
              if (!stillCurrent())
                  return;
              ctx.placeShell(view);
              ctx.setHit(view);
              ctx.appliedView = view;
          }).catch((error) => console.error("Gagal mengubah ukuran jendela Notch:", error));
      };
      // Membuka: ukuran jendela dan hitbox diperbesar segera. Menutup: tunggu animasi CSS selesai.
      if (isExpanding) {
          finish();
          return;
      }
      let fired = false;
      const go = () => {
          if (fired)
              return;
          fired = true;
          ctx.notch.removeEventListener("transitionend", onEnd);
          clearTimeout(fallback);
          finish();
      };
      const onEnd = (e: TransitionEvent) => {
          if (e.target === ctx.notch && (e.propertyName === "height" || e.propertyName === "width"))
              go();
      };
      ctx.notch.addEventListener("transitionend", onEnd);
      const fallback = window.setTimeout(go, 450);
  };
  ctx.render = function render() {
      const hasCompactLyrics = ctx.playing && ctx.lyState === "ok" && !!ctx.ly?.lines.length;
      const atSide = ctx.notch.classList.contains("is-edge-left") || ctx.notch.classList.contains("is-edge-right");
      const atBottom = ctx.notch.classList.contains("is-bottom");
      // Saat pointer baru masuk, tahan state mini sampai hover delay terpenuhi.
      // Dengan begitu expand langsung ke hub, bukan lewat tampilan horizontal perantara.
      const hoverPending = ctx.inside && Date.now() - ctx.insideSince < ctx.T.hover_open_ms;
      // Mode rapat: bentuk bubble dipertahankan walau di-hover, supaya tidak berganti ke wajah "idle"
      // (dan awan rapat tidak hilang-muncul) setiap kali kursor lewat.
      const bubble = (atSide || atBottom) && !ctx.override && !ctx.isDraggingWindow &&
          (!ctx.inside || hoverPending || ctx.holdHover || ctx.meetingMode);
      const view = ctx.override ?? (bubble
          ? (atSide ? (ctx.playing ? "edgeMedia" : "edgeIdle") : (ctx.playing ? "bottomMedia" : "bottomIdle"))
          : ctx.playing ? (hasCompactLyrics ? "compactLyrics" : "compact")
              : ctx.idleFace === "clock" ? "clock" : "idle");
      ctx.notch.dataset.view = view;
      ctx.notch.dataset.playing = String(ctx.playing);
      if (view !== ctx.lastView) {
          // Catat view asal agar saat collapse hitbox lama bisa dipertahankan hingga akhir.
          const previousView = ctx.appliedView || ctx.lastView || view;
          // Selama animasi berlangsung, tepi notch bisa menyapu di bawah kursor dan memicu
          // mouseenter/mouseleave berkali-kali. settleUntil dipakai untuk mengabaikan perubahan
          // hover itu (lihat setInside) sampai animasi selesai.
          ctx.settleUntil = Date.now() + 400;
          ctx.lastView = view;
          ctx.appliedView = "";
          ctx.applySize(view, previousView);
      }
  };
  ctx.typing = function typing(): boolean {
      const a = document.activeElement;
      return !!a && document.hasFocus() && ctx.notch.contains(a) &&
          (a.tagName === "INPUT" || a.tagName === "TEXTAREA");
  };
  ctx.expand = function expand(view: View, ms = 3000) {
      if (ctx.override === "rem" && view !== "rem")
          return; // pengingat aktif tidak boleh tertimpa
      // adzan berbunyi: hanya pengingat dan kartu adzan itu sendiri yang boleh mengambil alih notch
      if (ctx.prayerAdhanActive && view !== "prayerAlert" && view !== "rem")
          return;
      ctx.override = view;
      ctx.render();
      clearTimeout(ctx.hideTimer);
      if (ms > 0) {
          ctx.hideTimer = window.setTimeout(() => {
              if (view === "hub" && (ctx.inside || ctx.holdOpen()))
                  return;
              ctx.override = null;
              ctx.hubByHover = false;
              ctx.render();
          }, ms);
      }
  };
  ctx.collapse = function collapse() {
      if (ctx.prayerAdhanActive && ctx.override === "prayerAlert")
          return; // alert adzan hanya ditutup oleh adhanEnd
      // hub yang dibuka/ditahan pengguna (hover atau klik) lalu dilepas: ingat halaman terakhir.
      // Di halaman jam -> notch berganti dari mata ke jam + tanggal; di halaman lain -> kembali ke mata.
      // Hub otomatis (lagu baru, cuaca, timer) tidak mengubah apa-apa.
      const byUser = (ctx.override === "hub" || ctx.override === "cal") && ctx.hubByHover;
      clearTimeout(ctx.hideTimer);
      ctx.override = null;
      ctx.hubByHover = false;
      ctx.closeAt = 0;
      if (byUser && ctx.clockOn) {
          ctx.idleFace = ctx.names[ctx.page] === "clock" ? "clock" : "eyes";
          ctx.saveFace(ctx.idleFace);
      }
      ctx.render();
  };
}

export function init(ctx: Record<string, any>): void {
  ctx.lastView = "";
  ctx.appliedView = "";
  ctx.sizeRequestId = 0;
  ctx.lastSizeRequestAt = 0;
  // pengaman: kalau area klik tidak cocok dengan view saat ini, terapkan ulang
  setInterval(() => {
      if (ctx.lastView && ctx.appliedView !== ctx.lastView && Date.now() - ctx.lastSizeRequestAt > 2500) {
          ctx.applySize(ctx.lastView);
      }
  }, 1500);
  ctx.SELECT_IDLE_MS = 2500;
  ctx.SELECT_GRACE_MS = 1500;
  ctx.selectOpenedAt = 0;
  ctx.selectHeld = () => {
      const now = Date.now();
      if (ctx.selectHoldUntil <= now)
          return false;
      if (ctx.selectChosen)
          return true;
      return now - Math.max(ctx.selectOpenedAt, ctx.lastCursorMove) < ctx.SELECT_IDLE_MS;
  };
  ctx.holdOpen = () => ctx.typing() || ctx.selectHeld();
  ctx.holdSelect = () => { ctx.selectChosen = false; ctx.selectOpenedAt = Date.now(); ctx.selectHoldUntil = ctx.selectOpenedAt + 20000; ctx.closeAt = 0; };
  ctx.notch.addEventListener("mousedown", (e) => {
      if ((e.target as HTMLElement).closest?.("select"))
          ctx.holdSelect();
  }, true);
  ctx.notch.addEventListener("keydown", (e) => {
      if (!(e.target instanceof HTMLSelectElement))
          return;
      if (e.key === "Escape") {
          ctx.selectHoldUntil = 0;
          ctx.selectChosen = false;
      }
      else
          ctx.holdSelect();
  }, true);
  ctx.notch.addEventListener("change", (e) => {
      if (!(e.target instanceof HTMLSelectElement))
          return;
      // pilihan dibuat: beri waktu singkat melihat hasil; dilepas lebih awal jika kursor kembali ke notch
      ctx.selectChosen = true;
      ctx.selectHoldUntil = Date.now() + ctx.SELECT_GRACE_MS;
      ctx.closeAt = 0;
  }, true);
}
