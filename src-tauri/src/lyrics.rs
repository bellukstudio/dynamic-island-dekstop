// @ts-nocheck
// Lirik, sinkronisasi, dan lebar compact lyrics.
// ---------- lirik lagu (diperluas dari halaman media) ----------
type LyLine = {
    t: number;
    text: string;
};
type Lyrics = {
    synced: boolean;
    lines: LyLine[];
};

export function register(ctx: Record<string, any>): void {
  ctx.onMediaForLyrics = function onMediaForLyrics(m: Media) {
      const nowMs = performance.now();
      const playingNow = m.active && m.playing;
      // Posisi dari pemutar datang tiap ±1 detik dan sedikit bergetar (snapshot SMTC kasar). Bila lagu terus
      // berjalan dan selisihnya kecil, koreksi hanya sebagian supaya lirik tidak maju-mundur; selisih besar
      // (seek, ganti lagu, jeda) langsung diikuti.
      const predicted = ctx.nowPos();
      const drift = m.position - predicted;
      if (ctx.clockPlaying && playingNow && m.length === ctx.clockLen && Math.abs(drift) < 1.0) {
          ctx.clockPos = predicted + drift * 0.4;
      }
      else {
          ctx.clockPos = m.position;
      }
      ctx.clockLen = m.length;
      ctx.clockAt = nowMs;
      ctx.clockPlaying = playingNow;
      // Pencarian lirik untuk compact view tetap aktif meskipun halaman Lyrics
      // tidak dimasukkan ke daftar halaman konfigurasi.
      const key = m.active && m.title ? `${m.artist}|${m.title}` : "";
      if (key === ctx.lyKey) {
          ctx.scheduleLyricTick(); // jam baru: jadwalkan ulang pergantian baris
          return;
      }
      ctx.lyKey = key;
      ctx.ly = null;
      const token = ++ctx.lyToken;
      if (!key) {
          ctx.lyState = "idle";
          ctx.updateCompactLyric();
          ctx.renderLyrics();
          return;
      }
      ctx.lyState = "loading";
      ctx.renderLyrics();
      ctx.invoke<Lyrics | null>("lyrics_fetch", { title: m.title, artist: m.artist, length: m.length })
          .then((r) => {
          if (token !== ctx.lyToken)
              return; // lagu sudah ganti
          ctx.ly = r && r.lines.length ? r : null;
          ctx.lyState = ctx.ly ? "ok" : "none";
          ctx.renderLyrics();
          ctx.scheduleLyricTick();
      })
          .catch(() => {
          if (token !== ctx.lyToken)
              return;
          ctx.lyState = "none";
          ctx.renderLyrics();
      });
  };
  ctx.renderLyrics = function renderLyrics() {
      document.getElementById("m-lyrics")?.classList.toggle("off", ctx.lyState === "idle" || ctx.lyState === "none");
      ctx.lyLines.classList.toggle("plain", !!ctx.ly && !ctx.ly.synced);
      ctx.lyLines.replaceChildren();
      ctx.lyIdx = -2;
      ctx.updateCompactLyric();
      if (ctx.lyState !== "ok" || !ctx.ly) {
          const d = document.createElement("div");
          d.className = "ly-msg";
          d.textContent =
              ctx.lyState === "loading" ? "Mencari lirik…"
                  : ctx.lyState === "none" ? "Lirik tidak ditemukan"
                      : "Putar musik untuk melihat lirik";
          ctx.lyLines.appendChild(d);
          return;
      }
      if (!ctx.ly.synced) {
          const note = document.createElement("div");
          note.className = "ly-note";
          note.textContent = "Lirik tanpa waktu — gulir otomatis hanya perkiraan";
          ctx.lyLines.appendChild(note);
      }
      ctx.ly.lines.forEach((l) => {
          const d = document.createElement("div");
          d.className = "ly-l";
          d.textContent = l.text || "♪";
          ctx.lyLines.appendChild(d);
      });
      if (ctx.notch.dataset.view === "lyr")
          ctx.tickLyrics(true);
      ctx.scheduleLyricTick();
  };
  // Pergantian baris dijadwalkan tepat pada waktunya (setTimeout ke baris berikutnya), bukan menunggu
  // polling 500 ms yang bisa terlambat setengah detik: pada lagu cepat (baris < 1-2 detik) itu terasa
  // sebagai lirik yang selalu "ketinggalan". Dijadwalkan ulang tiap jam berubah (update media, seek, ganti lirik).
  ctx.scheduleLyricTick = function scheduleLyricTick() {
      if (ctx.lyTimer) {
          clearTimeout(ctx.lyTimer);
          ctx.lyTimer = 0;
      }
      if (ctx.lyState !== "ok" || !ctx.ly?.synced || !ctx.clockPlaying)
          return;
      const next = ctx.ly.lines[ctx.lyIndex() + 1];
      if (!next)
          return;
      const wait = Math.max(15, Math.min(4000, (next.t - (ctx.nowPos() + ctx.LY_LEAD)) * 1000));
      ctx.lyTimer = window.setTimeout(() => {
          ctx.lyTimer = 0;
          // Panel lirik terbuka: perbarui sorotan + autoscroll. Selain itu cukup lirik compact
          // (tanpa menyentuh DOM panel lirik yang tersembunyi).
          if (ctx.notch.dataset.view === "lyr")
              ctx.tickLyrics();
          else
              ctx.updateCompactLyric();
          ctx.scheduleLyricTick();
      }, wait);
  };
  // Geser sinkron lirik: Alt + roda mouse di atas lirik. Naik = lirik lebih awal, turun = lebih lambat.
  ctx.adjustLyricLead = function adjustLyricLead(delta: number) {
      ctx.LY_LEAD = Math.round(Math.max(-2, Math.min(2, ctx.LY_LEAD + delta)) * 10) / 10;
      try {
          localStorage.setItem("notch.lyLead", String(ctx.LY_LEAD));
      }
      catch { /* penyimpanan tidak tersedia */ }
      if (ctx.notch.dataset.view === "lyr")
          ctx.tickLyrics(true);
      else
          ctx.updateCompactLyric();
      ctx.scheduleLyricTick();
      const v = ctx.LY_LEAD;
      ctx.momentBubble?.show("Sinkron lirik", v === 0 ? "Tepat waktu" : `${Math.abs(v).toFixed(1)} dtk lebih ${v > 0 ? "awal" : "lambat"}`, 1200);
  };
  ctx.lyIndex = function lyIndex(): number {
      if (!ctx.ly)
          return -1;
      const p = ctx.nowPos() + ctx.LY_LEAD;
      // Lirik tersinkronisasi menggunakan timestamp LRC.
      if (ctx.ly.synced) {
          let lo = 0, hi = ctx.ly.lines.length - 1, r = -1;
          while (lo <= hi) {
              const mid = (lo + hi) >> 1;
              if (ctx.ly.lines[mid].t <= p) {
                  r = mid;
                  lo = mid + 1;
              }
              else
                  hi = mid - 1;
          }
          return r;
      }
      // Lirik tanpa timestamp tidak bisa benar-benar sinkron; estimasikan dari progres lagu.
      if (ctx.clockLen <= 0 || !ctx.ly.lines.length)
          return 0;
      return Math.min(ctx.ly.lines.length - 1, Math.floor(Math.max(0, Math.min(0.999, p / ctx.clockLen)) * ctx.ly.lines.length));
  };
  ctx.measureLyric = function measureLyric(text: string): number {
      ctx.clCtx ??= document.createElement("canvas").getContext("2d");
      if (!ctx.clCtx)
          return text.length * 6.5;
      const cs = getComputedStyle(ctx.compactLyrics);
      ctx.clCtx.font = `${cs.fontWeight} ${cs.fontSize} ${cs.fontFamily}`;
      return ctx.clCtx.measureText(text).width;
  };
  ctx.fitCompactLyricWidth = function fitCompactLyricWidth(line: string) {
      // 48 = padding l-compact + ruang telinga; 16 = margin kiri/kanan lirik; 8 = kelonggaran
      const overhead = 48 + (ctx.cArtEl.offsetWidth || 22) + 16 + (ctx.compactBars?.offsetWidth || 18) + 8;
      const screenMax = Math.floor(ctx.MON.w / ctx.CFG.scale) - 60;
      const max = Math.max(ctx.CL_MIN, Math.min(ctx.CL_MAX, screenMax));
      const target = Math.round(ctx.clamp(ctx.measureLyric(line) + overhead, ctx.CL_MIN, max));
      if (Math.abs(target - ctx.clWidth) < 3)
          return;
      ctx.clWidth = target;
      ctx.notch.style.setProperty("--cl-w", String(target));
      ctx.BASE_SIZES.compactLyrics[0] = target + 20; // pil + 10px telinga tiap sisi
      ctx.BASE_HIT.compactLyrics[0] = target;
      ctx.SIZES.compactLyrics[0] = Math.round((target + 20) * ctx.CFG.scale);
      ctx.HIT.compactLyrics[0] = Math.round(target * ctx.CFG.scale);
      if (ctx.notch.dataset.view === "compactLyrics") {
          ctx.placeShell("compactLyrics"); // jendela native tetap, cukup tengahkan ulang + perbarui area klik
          ctx.setHit("compactLyrics");
      }
  };
  ctx.updateCompactLyric = function updateCompactLyric() {
      const visible = ctx.playing && ctx.lyState === "ok" && !!ctx.ly?.lines.length;
      ctx.compactLyrics.classList.toggle("visible", visible);
      if (!visible || !ctx.ly) {
          if (ctx.compactLyrics.textContent) ctx.compactLyrics.textContent = "";
          if (ctx.compactLyrics.hasAttribute("title")) ctx.compactLyrics.removeAttribute("title");
          return;
      }
      const i = ctx.lyIndex();
      const line = i >= 0 ? (ctx.ly.lines[i]?.text || "♪") : "♪";
      if (ctx.compactLyrics.textContent !== line) {
          ctx.compactLyrics.textContent = line;
          ctx.compactLyrics.title = line;
          ctx.fitCompactLyricWidth(line);
          ctx.compactLyrics.classList.remove("lyric-change");
          // Paksa animasi transisi berjalan kembali setiap pergantian baris.
          void ctx.compactLyrics.offsetWidth;
          ctx.compactLyrics.classList.add("lyric-change");
      }
  };
  ctx.centerLyric = function centerLyric(instant: boolean) {
      const el = ctx.lyLines.querySelector<HTMLElement>(".ly-l.cur");
      if (!el)
          return;
      ctx.lyScroll.scrollTo({
          top: el.offsetTop - ctx.lyScroll.clientHeight / 2 + el.offsetHeight / 2,
          behavior: instant ? "auto" : "smooth",
      });
  };
  ctx.tickLyrics = function tickLyrics(force = false) {
      ctx.updateCompactLyric();
      if (ctx.lyState !== "ok" || !ctx.ly)
          return;
      const manual = Date.now() - ctx.lyManualAt < 4000; // pengguna baru menggulir: jangan ditarik balik
      if (!ctx.ly.synced) {
          if (ctx.clockLen <= 0 || (manual && !force))
              return;
          const max = ctx.lyScroll.scrollHeight - ctx.lyScroll.clientHeight;
          ctx.lyScroll.scrollTop = Math.max(0, Math.min(1, ctx.nowPos() / ctx.clockLen)) * max;
          return;
      }
      const i = ctx.lyIndex();
      if (i === ctx.lyIdx && !force)
          return;
      ctx.lyIdx = i;
      ctx.lyLines.querySelector(".cur")?.classList.remove("cur");
      const el = i >= 0 ? ctx.lyLines.querySelectorAll(".ly-l")[i] : undefined;
      el?.classList.add("cur");
      if (!manual || force)
          ctx.centerLyric(force);
  };
  ctx.openLyrics = function openLyrics() {
      ctx.hubByHover = true; // menutup otomatis saat kursor keluar, seperti hub
      ctx.closeAt = 0;
      ctx.expand("lyr", 0);
      ctx.tickLyrics(true);
  };
}

export function init(ctx: Record<string, any>): void {
  ctx.ly = null;
  ctx.lyKey = "";
  ctx.lyState = "idle";
  ctx.lyToken = 0;
  ctx.lyIdx = -2;
  ctx.LY_LEAD = 0.3;
  try {
      const saved = parseFloat(localStorage.getItem("notch.lyLead") ?? "");
      if (Number.isFinite(saved))
          ctx.LY_LEAD = Math.max(-2, Math.min(2, saved));
  }
  catch { /* penyimpanan tidak tersedia */ }
  ctx.lyTimer = 0;
  ctx.lyLines = ctx.$("ly-lines");
  ctx.lyScroll = ctx.$("ly-scroll");
  ctx.markManual = () => { ctx.lyManualAt = Date.now(); };
  ctx.lyScroll.addEventListener("wheel", ctx.markManual, { passive: true });
  ctx.lyScroll.addEventListener("pointerdown", ctx.markManual);
  ctx.clockPos = 0;
  ctx.clockLen = 0;
  ctx.clockAt = 0;
  ctx.clockPlaying = false;
  ctx.lyManualAt = 0;
  ctx.nowPos = () => ctx.clockPos + (ctx.clockPlaying ? (performance.now() - ctx.clockAt) / 1000 : 0);
  ctx.CL_MIN = 230;
  ctx.CL_MAX = 520;
  ctx.clWidth = 340;
  ctx.clCtx = null;
  // Lirik cukup diperbarui dua kali per detik; saat tidak ada lirik, hindari kerja DOM.
  setInterval(() => {
      if (ctx.notch.dataset.view === "lyr") {
          ctx.tickLyrics();
      } else if (ctx.playing && ctx.lyState === "ok" && ctx.ly?.lines.length) {
          ctx.updateCompactLyric();
      } else if (ctx.compactLyrics.classList.contains("visible")) {
          ctx.updateCompactLyric();
      }
  }, 500);
  if (!ctx.featureOn("lyrics"))
      document.getElementById("m-lyrics")?.remove();
  document.getElementById("m-lyrics")?.addEventListener("click", (e) => { e.stopPropagation(); ctx.openLyrics(); });
  ctx.$("ly-back").addEventListener("click", (e) => { e.stopPropagation(); ctx.openHub(ctx.pageOf("media")); });
  // Alt + roda mouse di atas lirik menggeser sinkron (naik = lebih awal, turun = lebih lambat).
  const lyWheel = (e: WheelEvent) => {
      if (!e.altKey)
          return;
      e.preventDefault();
      ctx.adjustLyricLead(e.deltaY < 0 ? 0.1 : -0.1);
  };
  ctx.lyScroll.addEventListener("wheel", lyWheel, { passive: false });
  ctx.compactLyrics?.addEventListener("wheel", lyWheel, { passive: false });
  ctx.renderLyrics(); // pesan awal "Putar musik..."
  
}
