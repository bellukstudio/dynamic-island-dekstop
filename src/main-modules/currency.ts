// Currency page: IDR converter, month-to-date snapshot of selected IDX shares,
// and an expandable view with all IDR rates + a history chart per currency.
import { invoke } from "@tauri-apps/api/core";
// Data sources are public endpoints; unavailable data is shown as unavailable (never fabricated).
type RateResponse = { amount?: number; base?: string; date?: string; rates?: Record<string, number>; result?: string; time_last_update_utc?: string };
type StockPoint = { symbol: string; name: string; change: number; last: number };
type FxPoint = { time: number; close: number };

// Rentang grafik kurs: label tombol -> jumlah hari.
const FX_RANGES: Record<string, { days: number; label: string }> = {
  "1B": { days: 30, label: "30 hari" }, "3B": { days: 90, label: "3 bulan" },
  "6B": { days: 180, label: "6 bulan" }, "1T": { days: 365, label: "1 tahun" },
};

const CURRENCIES: Record<string, { name: string; digits: number }> = {
  USD: { name: "Dolar AS", digits: 4 }, SGD: { name: "Dolar Singapura", digits: 4 }, EUR: { name: "Euro", digits: 4 },
  JPY: { name: "Yen Jepang", digits: 4 }, MYR: { name: "Ringgit Malaysia", digits: 4 }, CNY: { name: "Yuan Tiongkok", digits: 4 },
  GBP: { name: "Pound Inggris", digits: 4 }, AUD: { name: "Dolar Australia", digits: 4 }, SAR: { name: "Riyal Saudi", digits: 4 },
  KRW: { name: "Won Korea", digits: 4 }, THB: { name: "Baht Thailand", digits: 4 }, INR: { name: "Rupee India", digits: 4 },
  CHF: { name: "Franc Swiss", digits: 4 }, HKD: { name: "Dolar Hong Kong", digits: 4 }, CAD: { name: "Dolar Kanada", digits: 4 },
};

let activeCurrencyRefresh: (() => void) | null = null;

const IDX_WATCHLIST = [
  ["BBCA.JK", "BBCA"], ["BBRI.JK", "BBRI"], ["BMRI.JK", "BMRI"], ["BBNI.JK", "BBNI"],
  ["TLKM.JK", "TLKM"], ["ASII.JK", "ASII"], ["ANTM.JK", "ANTM"], ["GOTO.JK", "GOTO"],
  ["ICBP.JK", "ICBP"], ["MDKA.JK", "MDKA"], ["BRPT.JK", "BRPT"], ["INDF.JK", "INDF"],
] as const;

export function initCurrencyPage(): () => void {
  // Idempoten: index.html dan main.ts boleh memanggil initializer tanpa membuat listener ganda.
  if (activeCurrencyRefresh) return activeCurrencyRefresh;

  const amountEl = document.getElementById("cu-amount") as HTMLInputElement | null;
  const targetEl = document.getElementById("cu-target") as HTMLSelectElement | null;
  const resultEl = document.getElementById("cu-result");
  const rateEl = document.getElementById("cu-rate");
  const updatedEl = document.getElementById("cu-updated");
  const refreshEl = document.getElementById("cu-refresh");
  const gainersEl = document.getElementById("cu-gainers");
  const losersEl = document.getElementById("cu-losers");
  const stockStatusEl = document.getElementById("cu-stock-status");
  const stockPeriodEl = document.getElementById("cu-stock-period");
  const badgeEl = document.getElementById("cu-badge");
  const quickEl = document.querySelector<HTMLElement>(".cu-quick");
  const pageEl = document.querySelector<HTMLElement>(".p-currency");
  const expandBtn = document.getElementById("cu-expand");
  const fxListEl = document.getElementById("cu-fx-list");
  const fxChartEl = document.getElementById("cu-fx-chart");
  const fxTitleEl = document.getElementById("cu-fx-title");
  const fxRateEl = document.getElementById("cu-fx-rate");
  const fxDeltaEl = document.getElementById("cu-fx-delta");
  const fxStatusEl = document.getElementById("cu-fx-status");
  const fxRangesEl = document.getElementById("cu-fx-ranges");
  if (!amountEl || !targetEl || !resultEl || !rateEl || !updatedEl || !refreshEl || !gainersEl || !losersEl || !stockStatusEl || !stockPeriodEl) {
    return () => {};
  }

  // Simpan elemen sebagai alias non-null setelah guard. TypeScript tidak selalu
  // mempertahankan narrowing variabel DOM yang ditangkap oleh fungsi nested.
  const amountInputEl = amountEl as HTMLInputElement;
  const targetSelectEl = targetEl as HTMLSelectElement;
  const resultElement = resultEl as HTMLElement;
  const rateElement = rateEl as HTMLElement;
  const updatedElement = updatedEl as HTMLElement;
  const refreshButton = refreshEl as HTMLElement;
  const gainersElement = gainersEl as HTMLElement;
  const losersElement = losersEl as HTMLElement;
  const stockStatusElement = stockStatusEl as HTMLElement;
  const stockPeriodElement = stockPeriodEl as HTMLElement;
  const MAX_DIGITS = 15;

  let rates: Record<string, number> | null = null;
  let ratesDate = "";
  let lastLoadedAt = 0;
  let loading: Promise<void> | null = null;
  let stockSnapshot: StockPoint[] | null = null;
  let stockLoadedAt = 0;

  const NO_DECIMALS = new Set(["IDR", "JPY", "KRW"]);
  const fmt = (value: number, currency: string) => new Intl.NumberFormat("id-ID", {
    style: "currency", currency, minimumFractionDigits: 0, maximumFractionDigits: NO_DECIMALS.has(currency) ? 0 : 2,
  }).format(value);
  // Format Rupiah: 1234567 -> "1.234.567"
  const groupDigits = (digits: string) => digits.replace(/\B(?=(\d{3})+(?!\d))/g, ".");
  const fmtIdr = (value: number) => `Rp ${new Intl.NumberFormat("id-ID", { maximumFractionDigits: value < 100 ? 2 : 0 }).format(value)}`;
  const digitsOf = (raw: string) => raw.replace(/\D/g, "").replace(/^0+(?=\d)/, "").slice(0, MAX_DIGITS);
  // Tempel angka bergaya Indonesia ("1.500.000,50") atau internasional ("1,500,000.50"): pecahan dibuang.
  const pastedDigits = (raw: string) => {
    let s = raw.replace(/rp\.?/i, "").replace(/\s/g, "");
    if (/[.,]\d{1,2}$/.test(s) && (s.match(/[.,]/g) || []).length >= 1) {
      const last = Math.max(s.lastIndexOf("."), s.lastIndexOf(","));
      const before = s.slice(0, last);
      const sepsBefore = (before.match(/[.,]/g) || []).length;
      const isDecimal = s[last] === "," ? true : (sepsBefore === 0 || /,/.test(before));
      if (isDecimal) s = before;
    }
    return digitsOf(s);
  };
  const prettyDate = (v: string) => /^\d{4}-\d{2}-\d{2}$/.test(v)
    ? new Date(`${v}T00:00:00`).toLocaleDateString("id-ID", { day: "numeric", month: "short", year: "numeric" })
    : v;
  const currentAmount = () => Number(digitsOf(amountInputEl.value) || "0");
  const safeText = (el: Element, text: string) => { el.textContent = text; };

  function paintConversion() {
    const target = targetSelectEl.value || "USD";
    const amount = currentAmount();
    const rate = rates?.[target];
    const opt = targetSelectEl.selectedOptions[0];
    if (badgeEl) badgeEl.textContent = (opt?.textContent || target).split("·")[0].trim();
    syncFx(target);
    quickEl?.querySelectorAll<HTMLButtonElement>("button[data-amt]").forEach(b => {
      b.classList.toggle("on", Number(b.dataset.amt) === amount);
    });
    if (!Number.isFinite(rate) || rate === undefined || rate <= 0) {
      safeText(resultElement, rates ? "Kurs tidak tersedia" : "Memuat kurs…");
      safeText(rateElement, "Periksa koneksi lalu tekan ↻");
      return;
    }
    safeText(resultElement, fmt(amount * rate, target));
    safeText(rateElement, `1 ${target} = ${fmtIdr(1 / rate)} · ${ratesDate || "kurs referensi"}`);
  }

  async function fetchJson(url: string): Promise<any> {
    const controller = new AbortController();
    const timeout = window.setTimeout(() => controller.abort(), 9000);
    try {
      const response = await fetch(url, { signal: controller.signal, cache: "no-store" });
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      return await response.json();
    } finally { window.clearTimeout(timeout); }
  }

  async function loadRates(force = false) {
    if (!force && rates && Date.now() - lastLoadedAt < 5 * 60_000) { paintConversion(); return; }
    safeText(resultElement, "Memuat kurs…");
    safeText(rateElement, "Mengambil kurs referensi terbaru…");

    try {
      const rateUrls = [
        "https://open.er-api.com/v6/latest/IDR",
        "https://api.frankfurter.dev/v1/latest?base=IDR",
        "https://api.frankfurter.app/latest?from=IDR",
      ];
      const merged: Record<string, number> = {};
      let firstDate = "";
      let lastError: unknown;

      // Sumber pertama yang valid dipakai; sumber berikutnya hanya melengkapi kode mata uang
      // yang belum tersedia. Nilai yang tidak valid tidak pernah digunakan untuk konversi.
      for (const url of rateUrls) {
        const missingCodes = Object.keys(CURRENCIES).filter(code => !Number.isFinite(merged[code]) || merged[code] <= 0);
        if (missingCodes.length === 0) break;
        try {
          const candidate = await fetchJson(url) as RateResponse;
          const sourceRates = candidate?.rates;
          if (!sourceRates || typeof sourceRates !== "object") throw new Error("Format data kurs tidak valid");
          for (const code of Object.keys(CURRENCIES)) {
            const value = Number(sourceRates[code]);
            if (Number.isFinite(value) && value > 0 && !Number.isFinite(merged[code])) merged[code] = value;
          }
          if (!firstDate) {
            const rawDate = String(candidate.date || candidate.time_last_update_utc || "");
            const isoDate = rawDate.match(/^\d{4}-\d{2}-\d{2}/)?.[0];
            const parsedDate = rawDate ? new Date(rawDate) : null;
            firstDate = isoDate || (parsedDate && Number.isFinite(parsedDate.getTime())
              ? parsedDate.toLocaleDateString("id-ID", { day: "2-digit", month: "short", year: "numeric" })
              : "");
          }
        } catch (error) {
          lastError = error;
        }
      }

      if (!Object.keys(merged).length) throw lastError || new Error("Sumber kurs tidak tersedia");
      rates = merged;
      ratesDate = prettyDate(firstDate);
      lastLoadedAt = Date.now();
      buildFxList();
      paintConversion();
      safeText(updatedElement, `Kurs referensi ${ratesDate || "terbaru"}`);
    } catch (error) {
      console.warn("[Currency] Gagal memuat kurs:", error);
      if (rates) {
        // Pertahankan kurs valid terakhir jika pembaruan sementara gagal.
        paintConversion();
        safeText(updatedElement, `Kurs terakhir ${ratesDate || "tersimpan"} · pembaruan gagal`);
      } else {
        safeText(resultElement, "Kurs belum tersedia");
        safeText(rateElement, "Koneksi gagal · tekan ↻ untuk mencoba lagi");
        safeText(updatedElement, "Kurs tidak dapat dimuat");
      }
    }
  }

  // ---------- Tampilan diperluas: daftar kurs Rupiah + grafik riwayat ----------
  // Pilihan mata uang dibagi dengan <select> konverter, jadi mengganti salah satunya
  // otomatis mengganti yang lain dan grafiknya.
  const fxIdr = (value: number) => `Rp ${new Intl.NumberFormat("id-ID", { maximumFractionDigits: value < 1000 ? 2 : 0 }).format(value)}`;
  const fxCache = new Map<string, { at: number; points: FxPoint[] }>();
  let fxRange = "1B";
  let fxShownCode = "";
  let fxToken = 0;
  let fxPoints: FxPoint[] = [];
  let fxExpanded = false;

  const idrPerUnit = (code: string): number | null => {
    const r = rates?.[code];
    return Number.isFinite(r) && r !== undefined && r > 0 ? 1 / r : null;
  };

  function buildFxList() {
    if (!fxListEl) return;
    fxListEl.replaceChildren();
    for (const [code, meta] of Object.entries(CURRENCIES)) {
      const value = idrPerUnit(code);
      const row = document.createElement("button");
      row.type = "button"; row.className = "cu-fx-row"; row.dataset.code = code;
      const who = document.createElement("span"); who.className = "cu-fx-who";
      const unit = document.createElement("b"); unit.textContent = `1 ${code}`;
      const name = document.createElement("em"); name.textContent = meta.name;
      who.append(unit, name);
      const price = document.createElement("span"); price.className = "cu-fx-price";
      price.textContent = value === null ? "—" : fxIdr(value);
      row.append(who, price);
      fxListEl.appendChild(row);
    }
    markFxActive(fxShownCode || targetSelectEl.value || "USD");
  }

  function markFxActive(code: string) {
    fxListEl?.querySelectorAll<HTMLElement>(".cu-fx-row").forEach(row => {
      const on = row.dataset.code === code;
      row.classList.toggle("on", on);
      if (on && fxExpanded) row.scrollIntoView({ block: "nearest" });
    });
  }

  function syncFx(code: string) {
    if (!fxChartEl) return;
    const value = idrPerUnit(code);
    if (fxRateEl) safeText(fxRateEl, value === null ? "Kurs belum tersedia" : `1 ${code} = ${fxIdr(value)}`);
    if (fxShownCode === code) return;
    fxShownCode = code;
    markFxActive(code);
    if (fxTitleEl) safeText(fxTitleEl, `${code} · ${CURRENCIES[code]?.name ?? code} → IDR`);
    if (fxExpanded) void loadFxHistory();
  }

  const parseYahooFx = (data: any): FxPoint[] => {
    const result = data?.chart?.result?.[0];
    const timestamps: number[] = result?.timestamp || [];
    const closes: unknown[] = result?.indicators?.quote?.[0]?.close || [];
    return closes
      .map((value, i) => ({ time: Number(timestamps[i] ?? 0), close: value }))
      .filter((p): p is FxPoint => Number.isFinite(p.time) && p.time > 0 && typeof p.close === "number" && Number.isFinite(p.close) && p.close > 0)
      .sort((a, b) => a.time - b.time);
  };

  async function fetchFxHistory(code: string, days: number): Promise<FxPoint[]> {
    const now = Math.floor(Date.now() / 1000);
    const period1 = now - days * 86400, period2 = now + 86400;
    try {
      // Sama seperti saham: Yahoo diakses lewat Rust agar tidak terblokir CORS.
      const points = parseYahooFx(await invoke<any>("fetch_yahoo_chart", { symbol: `${code}IDR=X`, period1, period2 }));
      if (points.length >= 2) return points;
    } catch (error) {
      console.warn(`[Currency] Riwayat ${code} dari Yahoo gagal, mencoba cadangan:`, error);
    }
    // Cadangan: Frankfurter (ECB). Tidak menyediakan semua mata uang (mis. SAR) -> tampil "tidak tersedia".
    const iso = (t: number) => new Date(t * 1000).toISOString().slice(0, 10);
    const json = await fetchJson(`https://api.frankfurter.dev/v1/${iso(period1)}..${iso(now)}?base=${code}&symbols=IDR`);
    const points = Object.entries((json?.rates || {}) as Record<string, { IDR?: number }>)
      .map(([day, r]) => ({ time: Date.parse(`${day}T00:00:00Z`) / 1000, close: Number(r?.IDR) }))
      .filter(p => Number.isFinite(p.time) && Number.isFinite(p.close) && p.close > 0)
      .sort((a, b) => a.time - b.time);
    if (points.length < 2) throw new Error("Riwayat kurs kosong");
    return points;
  }

  async function loadFxHistory() {
    if (!fxChartEl) return;
    const code = fxShownCode || targetSelectEl.value || "USD";
    const range = FX_RANGES[fxRange] ?? FX_RANGES["1B"];
    const key = `${code}:${range.days}`;
    const token = ++fxToken;
    const cached = fxCache.get(key);
    if (cached && Date.now() - cached.at < 10 * 60_000) {
      fxPoints = cached.points; drawFx(); return;
    }
    fxPoints = []; drawFx();
    if (fxStatusEl) safeText(fxStatusEl, "Memuat grafik…");
    try {
      const points = await fetchFxHistory(code, range.days);
      if (token !== fxToken) return; // pilihan sudah berganti
      fxCache.set(key, { at: Date.now(), points });
      fxPoints = points;
    } catch (error) {
      if (token !== fxToken) return;
      console.warn(`[Currency] Riwayat ${code} tidak tersedia:`, error);
      fxPoints = [];
      if (fxStatusEl) safeText(fxStatusEl, `Grafik ${code} belum tersedia · coba lagi nanti`);
      drawFx(); return;
    }
    drawFx();
  }

  const SVG_NS = "http://www.w3.org/2000/svg";
  function svgEl<K extends keyof SVGElementTagNameMap>(tag: K, attrs: Record<string, string | number> = {}): SVGElementTagNameMap[K] {
    const el = document.createElementNS(SVG_NS, tag);
    for (const [k, v] of Object.entries(attrs)) el.setAttribute(k, String(v));
    return el;
  }

  function drawFx() {
    if (!fxChartEl) return;
    fxChartEl.querySelector("svg")?.remove();
    fxChartEl.querySelector(".cu-fx-tip")?.remove();
    const W = fxChartEl.clientWidth, H = fxChartEl.clientHeight;
    const pts = fxPoints;
    if (fxDeltaEl) { fxDeltaEl.textContent = ""; fxDeltaEl.className = "cu-fx-delta"; }
    if (pts.length < 2 || W < 80 || H < 80) return;

    const range = FX_RANGES[fxRange] ?? FX_RANGES["1B"];
    const first = pts[0].close, last = pts[pts.length - 1].close;
    const changePct = (last - first) / first * 100;
    if (fxDeltaEl) {
      fxDeltaEl.className = `cu-fx-delta ${changePct >= 0 ? "cu-positive" : "cu-negative"}`;
      fxDeltaEl.textContent = `${changePct >= 0 ? "+" : ""}${changePct.toLocaleString("id-ID", { minimumFractionDigits: 2, maximumFractionDigits: 2 })}% · ${range.label}`;
    }
    if (fxStatusEl) safeText(fxStatusEl, `Kurs penutupan harian · ${pts.length} titik data · sumber Yahoo Finance`);

    const padL = 58, padR = 12, padT = 12, padB = 24;
    const plotW = W - padL - padR, plotH = H - padT - padB;
    let lo = Math.min(...pts.map(p => p.close)), hi = Math.max(...pts.map(p => p.close));
    const pad = (hi - lo) * 0.14 || hi * 0.01;
    lo -= pad; hi += pad;
    const t0 = pts[0].time, t1 = pts[pts.length - 1].time;
    const xOf = (t: number) => padL + (t1 === t0 ? 0 : (t - t0) / (t1 - t0)) * plotW;
    const yOf = (v: number) => padT + (1 - (v - lo) / (hi - lo)) * plotH;

    const svg = svgEl("svg", { width: W, height: H, viewBox: `0 0 ${W} ${H}`, class: "cu-fx-svg", role: "img", "aria-label": `Grafik kurs ${fxShownCode} terhadap Rupiah` });
    const defs = svgEl("defs");
    const grad = svgEl("linearGradient", { id: "cu-fx-grad", x1: 0, y1: 0, x2: 0, y2: 1 });
    const s1 = svgEl("stop", { offset: "0%" }); s1.style.stopColor = "rgb(var(--accent))"; s1.style.stopOpacity = ".38";
    const s2 = svgEl("stop", { offset: "100%" }); s2.style.stopColor = "rgb(var(--accent))"; s2.style.stopOpacity = "0";
    grad.append(s1, s2); defs.appendChild(grad); svg.appendChild(defs);

    const span = hi - lo;
    const labelDigits = span < 10 ? 2 : span < 100 ? 1 : 0;
    const labelFmt = new Intl.NumberFormat("id-ID", { minimumFractionDigits: labelDigits, maximumFractionDigits: labelDigits });
    for (let i = 0; i <= 3; i++) {
      const v = lo + (span * i) / 3, y = yOf(v);
      svg.appendChild(svgEl("line", { x1: padL, x2: W - padR, y1: y, y2: y, class: "cu-fx-grid" }));
      const label = svgEl("text", { x: padL - 8, y: y + 3.5, "text-anchor": "end", class: "cu-fx-axis" });
      label.textContent = labelFmt.format(v); svg.appendChild(label);
    }
    const dateFmt: Intl.DateTimeFormatOptions = range.days > 120 ? { month: "short", year: "2-digit" } : { day: "numeric", month: "short" };
    const fmtDate = (t: number) => new Date(t * 1000).toLocaleDateString("id-ID", dateFmt);
    [[t0, "start", padL], [(t0 + t1) / 2, "middle", padL + plotW / 2], [t1, "end", W - padR]].forEach(([t, anchor, x]) => {
      const label = svgEl("text", { x: x as number, y: H - 6, "text-anchor": anchor as string, class: "cu-fx-axis" });
      label.textContent = fmtDate(t as number); svg.appendChild(label);
    });

    const line = pts.map((p, i) => `${i ? "L" : "M"}${xOf(p.time).toFixed(1)} ${yOf(p.close).toFixed(1)}`).join(" ");
    const baseY = (padT + plotH).toFixed(1);
    svg.appendChild(svgEl("path", { d: `${line} L${xOf(t1).toFixed(1)} ${baseY} L${xOf(t0).toFixed(1)} ${baseY} Z`, fill: "url(#cu-fx-grad)" }));
    svg.appendChild(svgEl("path", { d: line, class: "cu-fx-line" }));

    const guide = svgEl("line", { y1: padT, y2: padT + plotH, class: "cu-fx-guide" }); guide.style.display = "none";
    const dot = svgEl("circle", { r: 4.5, class: "cu-fx-dot" });
    const endX = xOf(t1), endY = yOf(last);
    dot.setAttribute("cx", String(endX)); dot.setAttribute("cy", String(endY));
    svg.append(guide, dot);
    const hit = svgEl("rect", { x: padL, y: padT, width: plotW, height: plotH, fill: "transparent" });
    svg.appendChild(hit);
    fxChartEl.appendChild(svg);

    const tip = document.createElement("div"); tip.className = "cu-fx-tip"; tip.hidden = true;
    fxChartEl.appendChild(tip);
    const tipDate: Intl.DateTimeFormatOptions = { day: "numeric", month: "short", year: "numeric" };
    const moveTo = (ev: PointerEvent) => {
      const rect = svg.getBoundingClientRect();
      const t = t0 + ((ev.clientX - rect.left - padL) / plotW) * (t1 - t0);
      let best = 0, bestD = Infinity;
      pts.forEach((p, i) => { const d = Math.abs(p.time - t); if (d < bestD) { bestD = d; best = i; } });
      const p = pts[best], x = xOf(p.time), y = yOf(p.close);
      guide.style.display = ""; guide.setAttribute("x1", String(x)); guide.setAttribute("x2", String(x));
      dot.setAttribute("cx", String(x)); dot.setAttribute("cy", String(y));
      tip.hidden = false;
      tip.replaceChildren();
      const d = document.createElement("small"); d.textContent = new Date(p.time * 1000).toLocaleDateString("id-ID", tipDate);
      const v = document.createElement("b"); v.textContent = `1 ${fxShownCode} = ${fxIdr(p.close)}`;
      tip.append(d, v);
      const tw = tip.offsetWidth;
      tip.style.left = `${Math.min(Math.max(x - tw / 2, 4), W - tw - 4)}px`;
      tip.style.top = `${Math.max(y - 54, 2)}px`;
    };
    hit.addEventListener("pointermove", moveTo);
    hit.addEventListener("pointerdown", (e) => e.stopPropagation());
    hit.addEventListener("pointerleave", () => {
      guide.style.display = "none"; tip.hidden = true;
      dot.setAttribute("cx", String(endX)); dot.setAttribute("cy", String(endY));
    });
  }

  function setExpanded(on: boolean) {
    if (!pageEl || !fxChartEl) return;
    fxExpanded = on;
    pageEl.classList.toggle("cu-expanded", on);
    expandBtn?.setAttribute("aria-expanded", String(on));
    const label = expandBtn?.querySelector("span");
    if (label) label.textContent = on ? "Ringkas" : "Perluas";
    if (on) {
      if (!fxListEl?.childElementCount) buildFxList();
      syncFx(targetSelectEl.value || "USD");
      markFxActive(fxShownCode);
      requestAnimationFrame(() => void loadFxHistory());
    }
  }

  expandBtn?.addEventListener("click", (event) => { event.preventDefault(); event.stopPropagation(); setExpanded(!fxExpanded); });
  fxListEl?.addEventListener("click", (event) => {
    const row = (event.target as HTMLElement).closest<HTMLElement>(".cu-fx-row");
    const code = row?.dataset.code;
    if (!code) return;
    event.preventDefault(); event.stopPropagation();
    targetSelectEl.value = code;
    paintConversion(); // memperbarui konverter + memicu pergantian grafik
  });
  fxRangesEl?.addEventListener("click", (event) => {
    const btn = (event.target as HTMLElement).closest<HTMLButtonElement>("button[data-range]");
    if (!btn || !FX_RANGES[btn.dataset.range || ""]) return;
    event.preventDefault(); event.stopPropagation();
    fxRange = btn.dataset.range as string;
    fxRangesEl?.querySelectorAll("button").forEach(b => b.classList.toggle("on", b === btn));
    void loadFxHistory();
  });
  if (fxChartEl && typeof ResizeObserver !== "undefined") {
    let lastW = 0, lastH = 0;
    new ResizeObserver(() => {
      if (!fxExpanded || (fxChartEl.clientWidth === lastW && fxChartEl.clientHeight === lastH)) return;
      lastW = fxChartEl.clientWidth; lastH = fxChartEl.clientHeight;
      drawFx();
    }).observe(fxChartEl);
  }

  async function fetchStock(symbol: string, label: string, period1: number, period2: number): Promise<StockPoint | null> {
    try {
      // Yahoo Finance tidak mengizinkan fetch browser dari origin localhost melalui CORS.
      // Request dijalankan oleh Rust (reqwest), lalu JSON dikembalikan ke WebView via Tauri invoke.
      const data = await invoke<any>("fetch_yahoo_chart", { symbol, period1, period2 });
      const result = data?.chart?.result?.[0];
      const timestamps: number[] = result?.timestamp || [];
      const rawCloses: unknown[] = result?.indicators?.quote?.[0]?.close || [];
      const points = rawCloses
        .map((value, index) => ({ time: Number(timestamps[index] ?? 0), close: value }))
        .filter((point): point is { time: number; close: number } =>
          Number.isFinite(point.time) && point.time > 0 && typeof point.close === "number" && Number.isFinite(point.close) && point.close > 0)
        .sort((a, b) => a.time - b.time);
      if (points.length < 2) return null;
      const first = points[0].close, last = points[points.length - 1].close;
      return { symbol, name: label, change: (last - first) / first * 100, last };
    } catch (error) {
      console.warn(`[Currency] Gagal memuat saham ${symbol} lewat backend:`, error);
      return null;
    }
  }

  function paintStockGroup(el: HTMLElement, items: StockPoint[], positive: boolean) {
    el.replaceChildren();
    if (!items.length) {
      const empty = document.createElement("span");
      empty.className = "cu-muted";
      empty.textContent = "Data belum tersedia";
      el.appendChild(empty);
      return;
    }
    for (const item of items.slice(0, 3)) {
      const row = document.createElement("div");
      row.className = "cu-stock-row";
      const who = document.createElement("div"); who.className = "cu-stock-who";
      const name = document.createElement("b"); name.textContent = item.name;
      const price = document.createElement("em"); price.textContent = fmtIdr(item.last);
      who.append(name, price);
      const change = document.createElement("span");
      change.className = `cu-chg ${positive ? "cu-positive" : "cu-negative"}`;
      change.textContent = `${item.change > 0 ? "+" : ""}${item.change.toLocaleString("id-ID", { maximumFractionDigits: 2, minimumFractionDigits: 2 })}%`;
      row.append(who, change); el.appendChild(row);
    }
  }

  async function loadStocks(force = false) {
    if (!force && stockSnapshot && Date.now() - stockLoadedAt < 10 * 60_000) { paintStocks(stockSnapshot); return; }
    safeText(gainersElement, "Memuat…"); safeText(losersElement, "Memuat…");
    const now = new Date();
    const start = new Date(now.getFullYear(), now.getMonth(), 1);
    const period1 = Math.floor(start.getTime() / 1000);
    const period2 = Math.floor((now.getTime() + 24 * 60 * 60 * 1000) / 1000);
    stockPeriodElement.textContent = `MTD · ${start.toLocaleDateString("id-ID", { day: "numeric", month: "short" })}`;
    const results = await Promise.allSettled(IDX_WATCHLIST.map(([symbol, label]) => fetchStock(symbol, label, period1, period2)));
    const points = results
      .filter((result): result is PromiseFulfilledResult<StockPoint | null> => result.status === "fulfilled")
      .map(result => result.value)
      .filter((point): point is StockPoint => point !== null)
      .sort((a, b) => b.change - a.change);
    stockLoadedAt = Date.now();
    if (points.length < 2) {
      stockSnapshot = null;
      paintStockGroup(gainersElement, [], true); paintStockGroup(losersElement, [], false);
      safeText(stockStatusElement, "Data saham belum tersedia. Periksa koneksi atau akses sumber data.");
      return;
    }
    stockSnapshot = points;
    paintStocks(points);
  }

  function paintStocks(points: StockPoint[]) {
    paintStockGroup(gainersElement, points.filter(x => x.change > 0).slice(0, 3), true);
    paintStockGroup(losersElement, [...points].filter(x => x.change < 0).sort((a, b) => a.change - b.change).slice(0, 3), false);
    const month = new Date().toLocaleDateString("id-ID", { month: "long", year: "numeric" });
    safeText(stockStatusElement, `Pilihan saham IDX · ${month} · data Yahoo Finance; bukan seluruh bursa.`);
  }

  function refresh(force = false) {
    if (loading) return loading;
    loading = Promise.all([loadRates(force), loadStocks(force)]).then(() => {
      if (updatedElement.textContent?.startsWith("Kurs referensi")) {
        safeText(updatedElement, `Diperbarui ${new Date().toLocaleTimeString("id-ID", { hour: "2-digit", minute: "2-digit" })}`);
      }
    }).finally(() => { loading = null; });
    return loading;
  }

  // Input Rupiah: hanya angka, otomatis berpemisah titik ("1234567" -> "1.234.567"), posisi kursor dijaga.
  const setAmount = (digits: string) => { amountInputEl.value = groupDigits(digits); paintConversion(); };
  amountInputEl.addEventListener("input", () => {
    const raw = amountInputEl.value;
    const caret = amountInputEl.selectionStart ?? raw.length;
    const digitsBeforeCaret = raw.slice(0, caret).replace(/\D/g, "").length;
    const digits = digitsOf(raw);
    const formatted = groupDigits(digits);
    amountInputEl.value = formatted;
    let seen = 0, pos = 0;
    while (pos < formatted.length && seen < Math.min(digitsBeforeCaret, digits.length)) { if (/\d/.test(formatted[pos])) seen++; pos++; }
    amountInputEl.setSelectionRange(pos, pos);
    paintConversion();
  });
  amountInputEl.addEventListener("paste", (event) => {
    event.preventDefault();
    const text = event.clipboardData?.getData("text") ?? "";
    setAmount(pastedDigits(text));
  });
  amountInputEl.addEventListener("focus", () => amountInputEl.select());
  amountInputEl.addEventListener("blur", () => { if (!digitsOf(amountInputEl.value)) setAmount("0"); });
  quickEl?.addEventListener("click", (event) => {
    const btn = (event.target as HTMLElement).closest<HTMLButtonElement>("button[data-amt]");
    if (!btn) return;
    event.preventDefault(); event.stopPropagation();
    setAmount(String(btn.dataset.amt || "0"));
  });
  amountInputEl.value = groupDigits(digitsOf(amountInputEl.value) || "0");
  targetSelectEl.addEventListener("change", paintConversion);
  refreshButton.addEventListener("click", (event) => { event.preventDefault(); event.stopPropagation(); void refresh(true); });
  activeCurrencyRefresh = () => { void refresh(); };
  void refresh();
  return activeCurrencyRefresh;
}
