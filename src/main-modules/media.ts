// @ts-nocheck
// Artwork media, kontrol pemutar, dan seek bar.
// ---------- media ----------
type Media = {
    active: boolean;
    title: string;
    artist: string;
    playing: boolean;
    position: number;
    length: number;
};

export function register(ctx: Record<string, any>): void {
  ctx.showDefaultArt = function showDefaultArt() {
      if (ctx.artIsDefault)
          return;
      ctx.artToken++;
      ctx.artIsDefault = true;
      ctx.artEl.onload = null;
      for (const el of [ctx.artEl, ctx.cArtEl, ctx.lyArtEl])
          el.removeAttribute("data-icon");
      ctx.artEl.src = ctx.DEFAULT_ART;
      ctx.cArtEl.src = ctx.DEFAULT_ART;
      ctx.lyArtEl.src = ctx.DEFAULT_ART;
      ctx.edgeDiscImg.src = ctx.DEFAULT_ART;
      ctx.artEl.style.visibility = ctx.cArtEl.style.visibility = "visible";
      if (!ctx.fixedAccent())
          document.documentElement.style.removeProperty("--accent"); // kembali ke warna bawaan
  };
  ctx.stripBlackTile = function stripBlackTile(c: HTMLCanvasElement): boolean {
      const n = c.width;
      const ctx = c.getContext("2d", { willReadFrequently: true })!;
      const im = ctx.getImageData(0, 0, n, n);
      const d = im.data;
      const luma = (p: number) => 0.299 * d[p] + 0.587 * d[p + 1] + 0.114 * d[p + 2];
      const isDark = (k: number) => d[k * 4 + 3] < 40 || luma(k * 4) < 36;
      let edgeDark = 0; // hampir seluruh tepi harus gelap
      for (let i = 0; i < n; i++) {
          edgeDark += +isDark(i) + +isDark((n - 1) * n + i) + +isDark(i * n) + +isDark(i * n + n - 1);
      }
      if (edgeDark < n * 4 * 0.92)
          return false;
      const gone = new Uint8Array(n * n);
      const stack: number[] = [];
      const push = (k: number) => { if (!gone[k] && isDark(k)) {
          gone[k] = 1;
          stack.push(k);
      } };
      for (let i = 0; i < n; i++) {
          push(i);
          push((n - 1) * n + i);
          push(i * n);
          push(i * n + n - 1);
      }
      while (stack.length) {
          const k = stack.pop()!;
          const x = k % n, y = (k - x) / n;
          if (x > 0)
              push(k - 1);
          if (x < n - 1)
              push(k + 1);
          if (y > 0)
              push(k - n);
          if (y < n - 1)
              push(k + n);
      }
      let x0 = n, y0 = n, x1 = -1, y1 = -1, removed = 0;
      for (let k = 0; k < n * n; k++) {
          if (gone[k]) {
              removed++;
              continue;
          }
          const x = k % n, y = (k - x) / n;
          if (x < x0)
              x0 = x;
          if (x > x1)
              x1 = x;
          if (y < y0)
              y0 = y;
          if (y > y1)
              y1 = y;
      }
      if (x1 < 0)
          return false;
      const bw = x1 - x0 + 1, bh = y1 - y0 + 1;
      const cx = (x0 + x1) / 2, cy = (y0 + y1) / 2;
      // harus objek yang berdiri di tengah dengan margin, bukan gambar yang memenuhi bingkai
      if (removed < n * n * 0.12 || bw > n * 0.88 || bh > n * 0.88)
          return false;
      if (Math.abs(cx - n / 2) > n * 0.1 || Math.abs(cy - n / 2) > n * 0.1)
          return false;
      for (let k = 0; k < n * n; k++) {
          const p = k * 4;
          if (gone[k]) {
              d[p + 3] = 0;
              continue;
          }
          const x = k % n, y = (k - x) / n;
          const edge = (x > 0 && gone[k - 1]) || (x < n - 1 && gone[k + 1]) || (y > 0 && gone[k - n]) || (y < n - 1 && gone[k + n]);
          if (edge) { // lunturkan piksel gelap di pinggir objek supaya tidak ada garis hitam
              const l = luma(p);
              if (l < 90)
                  d[p + 3] = Math.round(255 * Math.max(0, Math.min(1, l / 90)));
          }
      }
      ctx.putImageData(im, 0, 0);
      const side = Math.ceil(Math.max(bw, bh) * 1.1); // perbesar objek supaya memenuhi kotak
      const tmp = document.createElement("canvas");
      tmp.width = tmp.height = n;
      tmp.getContext("2d")!.drawImage(c, cx - side / 2, cy - side / 2, side, side, 0, 0, n, n);
      ctx.clearRect(0, 0, n, n);
      ctx.drawImage(tmp, 0, 0);
      return true;
  };
  ctx.showArt = function showArt(src: string) {
      ctx.artIsDefault = false;
      const token = ++ctx.artToken;
      const img = new Image();
      img.onload = () => {
          if (token !== ctx.artToken)
              return; // sudah ada gambar yang lebih baru
          let small = src;
          let icon = false;
          try {
              const c = document.createElement("canvas");
              c.width = c.height = ctx.ART_PX;
              const side = Math.min(img.naturalWidth, img.naturalHeight);
              c.getContext("2d")!.drawImage(img, (img.naturalWidth - side) / 2, (img.naturalHeight - side) / 2, side, side, 0, 0, ctx.ART_PX, ctx.ART_PX);
              icon = ctx.stripBlackTile(c);
              small = c.toDataURL("image/webp", 0.85); // webp menjaga transparansi (jpeg mengubahnya jadi hitam)
              c.width = c.height = 0; // lepaskan bitmap canvas
          }
          catch { /* gambar lintas domain tidak bisa dibaca canvas: pakai aslinya */ }
          for (const el of [ctx.artEl, ctx.cArtEl, ctx.lyArtEl])
              el.toggleAttribute("data-icon", icon); // ikon aplikasi: tanpa tile gelap
          ctx.artEl.onload = () => ctx.accentFrom(ctx.artEl);
          ctx.artEl.src = small;
          ctx.cArtEl.src = small;
          ctx.lyArtEl.src = small;
          ctx.edgeDiscImg.src = small;
          ctx.artEl.style.visibility = ctx.cArtEl.style.visibility = "visible";
      };
      img.src = src;
  };
  ctx.seekRatio = function seekRatio(e: PointerEvent): number {
      const r = ctx.seekBar.getBoundingClientRect();
      const lw = ctx.seekBar.offsetWidth || r.width;
      const viewportSpace = Math.abs(r.width - lw * ctx.CFG.scale) < Math.abs(r.width - lw);
      const f = viewportSpace || ctx.CFG.scale === 1 ? 1 : ctx.CFG.scale;
      const left = r.left * f, width = r.width * f;
      return width > 0 ? ctx.clamp((e.clientX - left) / width, 0, 1) : 0;
  };
  ctx.previewSeek = function previewSeek(e: PointerEvent): number {
      const ratio = ctx.seekRatio(e);
      ctx.seekFill.style.width = `${ratio * 100}%`;
      ctx.$("t-pos").textContent = ctx.fmt(ratio * ctx.mediaLen);
      return ratio;
  };
  ctx.finishSeek = function finishSeek(e: PointerEvent, commit: boolean) {
      if (!ctx.seekDragging)
          return;
      ctx.seekDragging = false;
      ctx.seekBar.classList.remove("seeking");
      if (ctx.seekBar.hasPointerCapture(e.pointerId))
          ctx.seekBar.releasePointerCapture(e.pointerId);
      if (!commit || ctx.mediaLen <= 0)
          return;
      const pos = ctx.previewSeek(e) * ctx.mediaLen;
      ctx.seekHoldUntil = performance.now() + 1500;
      // jam lokal ikut loncat supaya lirik langsung menyesuaikan
      ctx.clockPos = pos;
      ctx.clockAt = performance.now();
      ctx.invoke("media_seek", { position: pos }).catch((err) => console.error("media_seek gagal:", err));
  };
}

export function init(ctx: Record<string, any>): void {
  ctx.artEl = ctx.$("art") as HTMLImageElement;
  ctx.cArtEl = ctx.$("c-art") as HTMLImageElement;
  ctx.lyArtEl = ctx.$("ly-art") as HTMLImageElement;
  ctx.lastTitle = "";
  ctx.compactLyrics = document.createElement("div");
  ctx.compactLyrics.id = "c-lyric";
  ctx.compactLyrics.className = "c-lyric";
  ctx.compactLyrics.setAttribute("aria-live", "polite");
  ctx.compactLayer = document.querySelector<HTMLElement>(".l-compact");
  ctx.compactBars = ctx.compactLayer?.querySelector<HTMLElement>(".bars");
  if (ctx.compactLayer && ctx.compactBars) {
      ctx.compactLayer.insertBefore(ctx.compactLyrics, ctx.compactBars);
  }
  ctx.edgeDisc = document.createElement("div");
  ctx.edgeDisc.className = "edge-disc";
  ctx.edgeDisc.setAttribute("aria-hidden", "true");
  ctx.edgeDiscImg = document.createElement("img");
  ctx.edgeDiscImg.alt = "";
  ctx.edgeDisc.appendChild(ctx.edgeDiscImg);
  ctx.notch.appendChild(ctx.edgeDisc);
  ctx.DEFAULT_ART = "data:image/svg+xml;utf8," +
      encodeURIComponent(`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 96 96"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#3a3a4a"/><stop offset="1" stop-color="#18181f"/></linearGradient></defs><rect width="96" height="96" fill="url(#g)"/><g fill="none" stroke="#8e8e9a" stroke-width="4" stroke-linecap="round" stroke-linejoin="round"><path d="M40 62V32l24-6v30"/><circle cx="34" cy="62" r="6"/><circle cx="58" cy="56" r="6"/></g></svg>`);
  ctx.artIsDefault = false;
  ctx.artToken = 0;
  ctx.ART_PX = 192;
  ctx.showDefaultArt();
  ctx.fmt = (s: number) => `${Math.floor(s / 60)}:${String(Math.floor(s % 60)).padStart(2, "0")}`;
  ctx.listen<Media>("media", (e) => {
      const m = e.payload;
      ctx.noteMedia({ active: m.active, playing: m.playing, title: m.title, artist: m.artist }); // Conversation Moments
      const wasPlaying = ctx.playing;
      ctx.playing = m.active && m.playing;
      ctx.$("m-title").textContent = m.active ? m.title || "Tanpa judul" : "Tidak ada media";
      ctx.$("m-artist").textContent = m.active ? m.artist : "Putar musik untuk memulai";
      ctx.$("ly-title").textContent = m.active ? m.title || "Tanpa judul" : "Tidak ada media";
      ctx.$("ly-artist").textContent = m.active ? m.artist : "";
      if (!m.active)
          ctx.showDefaultArt();
      ctx.mediaLen = m.active ? m.length : 0;
      // Selama bar digeser (atau sesaat setelah seek) posisi dari pemutar jangan menimpa tampilan,
      // karena pemutar butuh waktu sebelum melaporkan posisi barunya.
      if (!ctx.seekDragging && performance.now() > ctx.seekHoldUntil) {
          ctx.$("m-progress").style.width = m.length > 0 ? `${(m.position / m.length) * 100}%` : "0%";
          ctx.$("t-pos").textContent = ctx.fmt(m.position);
      }
      ctx.$("t-len").textContent = ctx.fmt(m.length);
      if (ctx.playing && m.title && m.title !== ctx.lastTitle) {
          if (!ctx.inside)
              ctx.openHub(ctx.pageOf("media"), ctx.T.hub_auto_ms);
          if (ctx.lastTitle)
              ctx.flash("surprised", 900);
      }
      else
          ctx.render();
      ctx.lastTitle = m.title;
      if (ctx.playing !== wasPlaying)
          ctx.updateBase();
      ctx.onMediaForLyrics(m);
  });
  ctx.listen<string | null>("media-art", (e) => {
      if (e.payload)
          ctx.showArt(e.payload);
      else
          ctx.showDefaultArt();
  });
  ctx.mediaLen = 0;
  ctx.seekDragging = false;
  ctx.seekHoldUntil = 0;
  ctx.seekFill = ctx.$("m-progress");
  ctx.seekBar = (ctx.seekFill.parentElement ?? ctx.seekFill) as HTMLElement;
  ctx.seekBar.addEventListener("pointerdown", (e) => {
      if (e.button !== 0 || ctx.mediaLen <= 0)
          return;
      e.stopPropagation();
      e.preventDefault();
      ctx.seekDragging = true;
      ctx.seekBar.setPointerCapture(e.pointerId);
      ctx.seekBar.classList.add("seeking");
      ctx.previewSeek(e);
  });
  ctx.seekBar.addEventListener("pointermove", (e) => {
      if (!ctx.seekDragging)
          return;
      e.stopPropagation();
      ctx.previewSeek(e);
  });
  ctx.seekBar.addEventListener("pointerup", (e) => { e.stopPropagation(); ctx.finishSeek(e, true); });
  ctx.seekBar.addEventListener("pointercancel", (e) => ctx.finishSeek(e, false));
  ctx.seekBar.addEventListener("click", (e) => e.stopPropagation());
  ctx.control = (action: string) => (ev: Event) => { ev.stopPropagation(); ctx.invoke("media_control", { action }); };
  ctx.$("m-prev").addEventListener("click", ctx.control("prev"));
  ctx.$("m-toggle").addEventListener("click", ctx.control("toggle"));
  ctx.$("m-next").addEventListener("click", ctx.control("next"));
}
