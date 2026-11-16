// @ts-nocheck
// Halaman hub, navigasi, swipe, dan scroll.

export function register(ctx: Record<string, any>): void {
  ctx.rebuildHubPages = function rebuildHubPages(enabledPages: string[]) {
      const previousName = ctx.names[ctx.page];
      const validNames = [...new Set(enabledPages.filter((name) => ctx.allHubPages.some((el) => el.dataset.page === name)))];
      // Jangan biarkan hub tanpa halaman ketika pengguna sedang mengubah checkbox di Settings.
      if (validNames.length === 0) {
          const fallback = ctx.allHubPages.find((el) => el.dataset.page === "media") ?? ctx.allHubPages[0];
          if (fallback?.dataset.page)
              validNames.push(fallback.dataset.page);
      }
      const active = new Set(validNames);
      for (const el of ctx.allHubPages) {
          if (active.has(el.dataset.page ?? ""))
              el.style.removeProperty("display");
          else
              el.style.display = "none";
      }
      // Urutan slide harus sama dengan daftar dari config.toml; halaman nonaktif ditempatkan di belakang.
      for (const name of validNames) {
          const el = ctx.allHubPages.find((candidate) => candidate.dataset.page === name);
          if (el)
              ctx.track.appendChild(el);
      }
      for (const el of ctx.allHubPages) {
          if (!active.has(el.dataset.page ?? ""))
              ctx.track.appendChild(el);
      }
      ctx.names = validNames;
      ctx.pageCount = ctx.names.length;
      ctx.dotsEl.replaceChildren();
      for (let i = 0; i < ctx.pageCount; i++) {
          const d = document.createElement("span");
          d.className = "dot";
          d.addEventListener("click", (e) => { e.stopPropagation(); ctx.setPage(i); });
          ctx.dotsEl.appendChild(d);
      }
      // initHubNav mengganti item lama, jadi checkbox preview berulang tidak membuat ikon duplikat.
      ctx.setNavActive = ctx.initHubNav(ctx.names, (i) => ctx.setPage(i));
      const priorIndex = previousName ? ctx.names.indexOf(previousName) : -1;
      ctx.page = priorIndex >= 0 ? priorIndex : Math.min(Math.max(ctx.page, 0), Math.max(0, ctx.pageCount - 1));
      ctx.setPage(ctx.page);
  };
  ctx.setPage = function setPage(i: number) {
      ctx.page = Math.max(0, Math.min(ctx.pageCount - 1, i));
      ctx.track.style.setProperty("--page", String(ctx.page));
      ctx.track.style.setProperty("--drag", "0px");
      ctx.dotsEl.querySelectorAll(".dot").forEach((d, n) => d.classList.toggle("active", n === ctx.page));
      ctx.setNavActive(ctx.page);
      // Halaman Currency memerlukan ruang tambahan agar hasil konversi dan ringkasan saham
      // tidak tertutup oleh batas panel. Ukuran CSS dan hitbox native diselaraskan bersamaan.
      ctx.syncHubDynamicHeight();
      if (ctx.names[ctx.page] !== "notes" && ctx.notch.classList.contains("notes-expanded")) {
          ctx.setNotesExpanded(false);
      }
      if (ctx.names[ctx.page] === "quick" || ctx.names[ctx.page] === "tools")
          ctx.refreshQuick();
      if (ctx.names[ctx.page] === "clipboard")
          ctx.refreshClip();
      if (ctx.names[ctx.page] === "shelf")
          ctx.refreshShelf();
      if (ctx.names[ctx.page] === "reminder")
          ctx.refreshReminders();
      if (ctx.names[ctx.page] === "notes")
          ctx.refreshNotes();
  };
  ctx.openHub = function openHub(p: number, ms = 0) {
      if (ctx.override === "rem")
          return; // pengingat aktif tidak boleh tertimpa hub
      ctx.setPage(p);
      ctx.hubByHover = ms === 0;
      ctx.closeAt = 0;
      ctx.expand("hub", ms);
  };
  ctx.endDrag = function endDrag() {
      if (!ctx.dragging)
          return;
      ctx.dragging = false;
      ctx.track.classList.remove("dragging");
      if (ctx.dx < -50)
          ctx.setPage(ctx.page + 1);
      else if (ctx.dx > 50)
          ctx.setPage(ctx.page - 1);
      else
          ctx.setPage(ctx.page);
      ctx.dx = 0;
      setTimeout(() => { ctx.moved = false; }, 0);
  };
}

export function init(ctx: Record<string, any>): void {
  ctx.pagesEl = ctx.$("pages");
  ctx.track = ctx.$("track");
  ctx.dotsEl = ctx.$("dots");
  ctx.allHubPages = [...ctx.track.querySelectorAll<HTMLElement>(".page")];
  ctx.pageOf = (n: string) => Math.max(0, ctx.names.indexOf(n));
  ctx.homePage = () => (ctx.clockOn && ctx.idleFace === "clock" ? ctx.pageOf("clock") : ctx.page);
  ctx.rebuildHubPagesFn = ctx.rebuildHubPages;
  ctx.rebuildHubPages(ctx.CFG.pages);
  ctx.startX = 0;
  ctx.dx = 0;
  ctx.dragging = false;
  ctx.moved = false;
  // Geser halaman dengan drag mouse dimatikan: drag body sekarang memindahkan notch.
  // Ganti halaman lewat scroll/touchpad, ikon di bar atas, atau tombol ← →.
  window.addEventListener("pointermove", (e) => {
      if (!ctx.dragging)
          return;
      ctx.dx = e.clientX - ctx.startX;
      if (Math.abs(ctx.dx) > 6)
          ctx.moved = true;
      const atEdge = (ctx.page === 0 && ctx.dx > 0) || (ctx.page === ctx.pageCount - 1 && ctx.dx < 0);
      ctx.track.style.setProperty("--drag", `${atEdge ? ctx.dx * 0.3 : ctx.dx}px`);
  });
  window.addEventListener("pointerup", ctx.endDrag);
  window.addEventListener("pointercancel", ctx.endDrag);
  ctx.pagesEl.addEventListener("click", (e) => { if (ctx.moved)
      e.stopPropagation(); }, true);
  ctx.wheelLock = false;
  ctx.notch.addEventListener("wheel", (e) => {
      if (ctx.notch.dataset.view !== "hub")
          return;
      if ((e.target as HTMLElement).closest(".nv-list"))
          return; // nav ikon boleh di-scroll sendiri
      const sh = (e.target as HTMLElement).closest<HTMLElement>(".sh-list"); // shelf: scroll mendatar
      if (sh && sh.scrollWidth > sh.clientWidth) {
          sh.scrollLeft += e.deltaY || e.deltaX;
          return;
      }
      // Biarkan daftar panjang di dalam halaman (currency, catatan, clipboard, dll) di-scroll sendiri
      // selama belum mentok di ujungnya. Telusuri dari elemen yang di-hover naik ke atas sampai #notch,
      // cari kontainer vertikal yang BENAR-BENAR bisa digulir (bukan sekadar punya overflow:auto, tapi
      // isinya memang lebih panjang dari areanya). Ini generik -- tidak bergantung pada nama class
      // tertentu (cu-fx-list, cu-body-grid, nt-list, dll), jadi tidak mudah bolong lagi saat markup
      // berubah atau saat notch diperkecil dan kontainer pembungkus lain yang jadi scrollable.
      let scrollable: HTMLElement | null = null;
      for (let el = e.target as HTMLElement | null; el && el !== ctx.notch; el = el.parentElement) {
          const cs = getComputedStyle(el);
          if ((cs.overflowY === "auto" || cs.overflowY === "scroll") && el.scrollHeight > el.clientHeight + 1) {
              scrollable = el;
              break;
          }
      }
      if (scrollable) {
          const atTop = scrollable.scrollTop <= 0;
          const atBottom = scrollable.scrollTop + scrollable.clientHeight >= scrollable.scrollHeight - 1;
          const goingUp = e.deltaY < 0;
          // Hanya lanjut ganti halaman kalau kontainer sudah mentok di ujung yang searah arah scroll;
          // selain itu gulir kontainer ini sendiri dan jangan ganti halaman.
          if (!((goingUp && atTop) || (!goingUp && atBottom))) {
              scrollable.scrollTop += e.deltaY;
              return;
          }
      }
      const d = Math.abs(e.deltaX) > Math.abs(e.deltaY) ? e.deltaX : e.deltaY;
      if (Math.abs(d) < 10 || ctx.wheelLock)
          return;
      ctx.wheelLock = true;
      setTimeout(() => { ctx.wheelLock = false; }, 350);
      ctx.setPage(ctx.page + (d > 0 ? 1 : -1));
  }, { passive: true });
}
