// Alert kurs & harga saham: beri tahu lewat notch saat kurs Rupiah atau harga saham IDX menembus target.
//
//  - Aturan disimpan di localStorage "notch.priceAlerts" (maks. MAX_RULES), sekali pakai: setelah
//    tercapai aturan ditandai "tercapai" dan bisa diaktifkan lagi atau dihapus.
//  - Harga dicek tiap POLL_MS lewat command Rust `fetch_yahoo_chart` (sumber Yahoo Finance, sama dengan
//    halaman Currency) — hanya saat ada aturan aktif. Simbol di luar daftar Rust ditolak di sisi Rust.
//  - UI: tombol 🔔 + panel pada halaman Currency, disisipkan lewat DOM dari sini (index.html tidak diubah).
//  - Alert tampil sebagai kartu teks notch dan tidak pernah menimpa kartu/hub lain; yang belum sempat
//    tampil (mis. aplikasi ditutup) ditampilkan saat aplikasi dibuka lagi.
//  - Event Tauri opsional: "price-alert-test" (coba tampilkan contoh).
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { setCardEmoji } from "./card-emoji";
import "../css/price-alert.css";

export type PriceAlertHooks = {
  canShow: () => boolean;                                  // notch bebas (tak ada kartu/hub/adzan)
  card: (title: string, sub: string, ms: number, emoji?: string) => boolean | void; // true = tampil sebagai awan
  mood: (m: string, ms: number) => void;                   // ekspresi mata sementara
};

type Kind = "fx" | "stock";
type Dir = "above" | "below";
type Rule = {
  id: string; kind: Kind; code: string; dir: Dir; target: number; created: number;
  firedAt?: number; firedPrice?: number; shown?: boolean;
};

const KEY = "notch.priceAlerts";
const MAX_RULES = 30;
const POLL_MS = 5 * 60_000;
const FIRST_POLL_MS = 15_000;
const SHOW_MS = 8000;
const PRICE_CACHE_MS = 60_000;

// Harus sama dengan ALLOWED_FX_CODES / ALLOWED_SYMBOLS di src/currency.rs (dan CURRENCIES / IDX_WATCHLIST di currency.ts).
const FX: Array<[string, string]> = [
  ["USD", "Dolar AS"], ["SGD", "Dolar Singapura"], ["EUR", "Euro"], ["JPY", "Yen Jepang"], ["MYR", "Ringgit Malaysia"],
  ["CNY", "Yuan Tiongkok"], ["GBP", "Pound Inggris"], ["AUD", "Dolar Australia"], ["SAR", "Riyal Saudi"], ["KRW", "Won Korea"],
  ["THB", "Baht Thailand"], ["INR", "Rupee India"], ["CHF", "Franc Swiss"], ["HKD", "Dolar Hong Kong"], ["CAD", "Dolar Kanada"],
];
const STOCKS = ["BBCA", "BBRI", "BMRI", "BBNI", "TLKM", "ASII", "ANTM", "GOTO", "ICBP", "MDKA", "BRPT", "INDF"];

const symbolOf = (r: { kind: Kind; code: string }) => (r.kind === "fx" ? `${r.code}IDR=X` : `${r.code}.JK`);
const validCode = (kind: Kind, code: string) =>
  kind === "fx" ? FX.some(([c]) => c === code) : STOCKS.includes(code);

const fmtPrice = (v: number) =>
  `Rp ${new Intl.NumberFormat("id-ID", { maximumFractionDigits: v < 1000 ? 2 : 0 }).format(v)}`;
const dirSign = (d: Dir) => (d === "above" ? "≥" : "≤");

// "16.250" / "16250" / "16250,5" / "105,32" -> angka. Koma = desimal; titik = pemisah ribuan bila diikuti tepat 3 digit.
function parseTarget(raw: string): number | null {
  let s = raw.replace(/rp\.?/i, "").replace(/\s/g, "");
  if (!s) return null;
  if (s.includes(",")) s = s.replace(/\./g, "").replace(",", ".");
  else if (/^\d{1,3}(\.\d{3})+$/.test(s)) s = s.replace(/\./g, "");
  if (!/^\d+(\.\d+)?$/.test(s)) return null;
  const n = Number(s);
  return Number.isFinite(n) && n > 0 ? n : null;
}

// ---------- penyimpanan ----------
function loadRules(): Rule[] {
  try {
    const data = JSON.parse(localStorage.getItem(KEY) || "[]");
    if (!Array.isArray(data)) return [];
    return data
      .filter((r: any) =>
        r && (r.kind === "fx" || r.kind === "stock") && typeof r.code === "string" && validCode(r.kind, r.code) &&
        (r.dir === "above" || r.dir === "below") && Number.isFinite(r.target) && r.target > 0 && typeof r.id === "string")
      .slice(0, MAX_RULES)
      .map((r: any): Rule => ({
        id: r.id, kind: r.kind, code: r.code, dir: r.dir, target: r.target, created: Number(r.created) || Date.now(),
        firedAt: Number.isFinite(r.firedAt) ? r.firedAt : undefined,
        firedPrice: Number.isFinite(r.firedPrice) ? r.firedPrice : undefined,
        shown: r.shown === true,
      }));
  } catch { return []; }
}

// ---------- harga ----------
const priceCache = new Map<string, { price: number; at: number }>();

async function fetchPrice(symbol: string, fresh = false): Promise<number | null> {
  const hit = priceCache.get(symbol);
  if (!fresh && hit && Date.now() - hit.at < PRICE_CACHE_MS) return hit.price;
  const now = Math.floor(Date.now() / 1000);
  try {
    const data = await invoke<any>("fetch_yahoo_chart", { symbol, period1: now - 7 * 86400, period2: now + 86400 });
    const result = data?.chart?.result?.[0];
    let price = Number(result?.meta?.regularMarketPrice);
    if (!Number.isFinite(price) || price <= 0) {
      const closes: unknown[] = result?.indicators?.quote?.[0]?.close ?? [];
      price = NaN;
      for (let i = closes.length - 1; i >= 0; i--) {
        const c = closes[i];
        if (typeof c === "number" && Number.isFinite(c) && c > 0) { price = c; break; }
      }
    }
    if (Number.isFinite(price) && price > 0) {
      priceCache.set(symbol, { price, at: Date.now() });
      return price;
    }
  } catch (error) {
    console.warn(`[PriceAlert] Harga ${symbol} gagal dimuat:`, error);
  }
  return null;
}

export function initPriceAlert(h: PriceAlertHooks) {
  let rules = loadRules();
  const save = () => { try { localStorage.setItem(KEY, JSON.stringify(rules)); } catch { /* penyimpanan opsional */ } };

  // ---------- tampilan alert ----------
  const queue: Array<{ rule: Rule; price: number }> = [];
  let draining = false;

  function present(rule: Rule, price: number) {
    const name = rule.code;
    const title = `${name} ${rule.dir === "above" ? "naik ke" : "turun ke"} ${fmtPrice(price)}`;
    const sub = `Alert ${dirSign(rule.dir)} ${fmtPrice(rule.target)} tercapai · ${rule.kind === "fx" ? "kurs" : "saham"}`;
    const emoji = rule.kind === "fx" ? "💱" : rule.dir === "above" ? "📈" : "📉";
    const asBubble = h.card(title, sub, SHOW_MS, emoji) === true;
    if (!asBubble) setCardEmoji([emoji]);
    h.mood(rule.dir === "above" ? "happy" : "surprised", SHOW_MS + 2000);
    rule.shown = true;
    save();
  }

  function drain() {
    if (draining) return;
    draining = true;
    const step = () => {
      const item = queue[0];
      if (!item) { draining = false; return; }
      if (!h.canShow()) { window.setTimeout(step, 3000); return; }
      queue.shift();
      present(item.rule, item.price);
      window.setTimeout(step, SHOW_MS + 1500);
    };
    step();
  }
  const enqueue = (rule: Rule, price: number) => { queue.push({ rule, price }); drain(); };

  // alert yang sudah tercapai tapi belum sempat tampil (aplikasi ditutup) ditampilkan sekarang
  for (const r of rules) if (r.firedAt && !r.shown && r.firedPrice) enqueue(r, r.firedPrice);

  // ---------- pemeriksaan berkala ----------
  let checking = false;
  async function checkAll() {
    if (checking) return;
    checking = true;
    try {
      const active = rules.filter((r) => !r.firedAt);
      if (!active.length) return;
      const prices = new Map<string, number>();
      for (const symbol of new Set(active.map(symbolOf))) {
        const p = await fetchPrice(symbol, true);
        if (p !== null) prices.set(symbol, p);
      }
      let changed = false;
      for (const r of active) {
        if (r.firedAt) continue; // dicabut/ditandai selagi menunggu respons
        const p = prices.get(symbolOf(r));
        if (p === undefined) continue; // gagal memuat: coba lagi pada putaran berikutnya
        if (r.dir === "above" ? p >= r.target : p <= r.target) {
          r.firedAt = Date.now(); r.firedPrice = p; r.shown = false; changed = true;
          enqueue(r, p);
        }
      }
      if (changed) { save(); renderUi(); }
    } finally { checking = false; }
  }
  window.setTimeout(() => void checkAll(), FIRST_POLL_MS);
  window.setInterval(() => void checkAll(), POLL_MS);

  // ---------- UI pada halaman Currency ----------
  let panel: HTMLElement | null = null;
  let listEl: HTMLElement | null = null;
  let nowEl: HTMLElement | null = null;
  let msgEl: HTMLElement | null = null;
  let assetEl: HTMLSelectElement | null = null;
  let dirEl: HTMLSelectElement | null = null;
  let targetEl: HTMLInputElement | null = null;
  let bellEl: HTMLElement | null = null;

  const el = <K extends keyof HTMLElementTagNameMap>(tag: K, cls?: string, text?: string) => {
    const e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text !== undefined) e.textContent = text;
    return e;
  };
  const say = (text: string, bad = false) => {
    if (!msgEl) return;
    msgEl.textContent = text;
    msgEl.dataset.bad = bad ? "1" : "0";
  };
  const currentAsset = (): { kind: Kind; code: string } | null => {
    const v = assetEl?.value ?? "";
    const [kind, code] = v.split(":");
    return (kind === "fx" || kind === "stock") && validCode(kind, code) ? { kind, code } : null;
  };

  async function refreshNow() {
    const a = currentAsset();
    if (!a || !nowEl) return;
    const symbol = symbolOf(a);
    nowEl.textContent = "Harga sekarang: memuat…";
    const p = await fetchPrice(symbol);
    if (!nowEl || currentAsset()?.code !== a.code) return; // pilihan sudah berganti
    nowEl.textContent = p === null ? "Harga sekarang: belum tersedia" : `Harga sekarang: ${fmtPrice(p)}`;
    if (p !== null && targetEl) targetEl.placeholder = `mis. ${fmtPrice(p).replace("Rp ", "")}`;
  }

  function renderUi() {
    const activeCount = rules.filter((r) => !r.firedAt).length;
    if (bellEl) {
      bellEl.dataset.count = activeCount ? String(activeCount) : "";
      bellEl.title = activeCount ? `${activeCount} alert aktif` : "Alert harga & kurs";
    }
    if (!listEl) return;
    listEl.replaceChildren();
    if (!rules.length) {
      listEl.appendChild(el("span", "pa-empty", "Belum ada alert. Pilih kurs atau saham, tentukan target, lalu pasang."));
      return;
    }
    for (const r of [...rules].sort((a, b) => b.created - a.created)) {
      const row = el("div", "pa-row");
      row.dataset.fired = r.firedAt ? "1" : "0";
      row.appendChild(el("span", "pa-ico", r.kind === "fx" ? "💱" : r.dir === "above" ? "📈" : "📉"));
      const copy = el("span", "pa-copy");
      copy.appendChild(el("b", "", `${r.code} ${dirSign(r.dir)} ${fmtPrice(r.target)}`));
      const state = r.firedAt
        ? `Tercapai ${fmtPrice(r.firedPrice ?? r.target)} · ${new Date(r.firedAt).toLocaleDateString("id-ID", { day: "numeric", month: "short" })}`
        : r.kind === "fx" ? "Aktif · kurs ke Rupiah" : "Aktif · saham IDX";
      copy.appendChild(el("small", "", state));
      row.appendChild(copy);
      const rearm = el("button", "pa-mini", "↺");
      rearm.type = "button"; rearm.title = "Aktifkan lagi"; rearm.hidden = !r.firedAt; rearm.dataset.act = "rearm"; rearm.dataset.id = r.id;
      const del = el("button", "pa-mini", "✕");
      del.type = "button"; del.title = "Hapus"; del.dataset.act = "del"; del.dataset.id = r.id;
      row.append(rearm, del);
      listEl.appendChild(row);
    }
  }

  async function addRule() {
    const a = currentAsset();
    const dir: Dir = dirEl?.value === "below" ? "below" : "above";
    const target = parseTarget(targetEl?.value ?? "");
    if (!a) return say("Pilih kurs atau saham dulu.", true);
    if (target === null) return say("Isi target dengan angka, mis. 16.500", true);
    if (rules.length >= MAX_RULES) return say(`Maksimal ${MAX_RULES} alert. Hapus yang tidak dipakai.`, true);
    if (rules.some((r) => !r.firedAt && r.kind === a.kind && r.code === a.code && r.dir === dir && r.target === target)) {
      return say("Alert yang sama sudah aktif.", true);
    }
    say("Memeriksa harga…");
    const price = await fetchPrice(symbolOf(a), true);
    if (price !== null && (dir === "above" ? price >= target : price <= target)) {
      return say(`Harga sekarang ${fmtPrice(price)} sudah ${dir === "above" ? "di atas" : "di bawah"} target itu.`, true);
    }
    rules.push({ id: `${Date.now().toString(36)}${Math.random().toString(36).slice(2, 6)}`, kind: a.kind, code: a.code, dir, target, created: Date.now() });
    save();
    if (targetEl) targetEl.value = "";
    renderUi();
    say(price === null ? "Alert dipasang (harga sekarang belum bisa dicek)." : "Alert dipasang. Dicek tiap 5 menit.");
    void checkAll();
  }

  function onListClick(e: Event) {
    const btn = (e.target as HTMLElement).closest<HTMLButtonElement>("button[data-act]");
    if (!btn) return;
    e.preventDefault(); e.stopPropagation();
    const rule = rules.find((r) => r.id === btn.dataset.id);
    if (!rule) return;
    if (btn.dataset.act === "del") rules = rules.filter((r) => r !== rule);
    else if (btn.dataset.act === "rearm") { rule.firedAt = undefined; rule.firedPrice = undefined; rule.shown = false; }
    save(); renderUi();
  }

  function buildPanel(page: HTMLElement) {
    const p = el("div", "pa-panel");
    p.hidden = true;
    p.setAttribute("role", "dialog");
    p.setAttribute("aria-label", "Alert harga dan kurs");
    // klik/seret di dalam panel tidak boleh sampai ke notch (yang bisa menutup/menggeser hub)
    for (const ev of ["click", "pointerdown", "mousedown"]) p.addEventListener(ev, (e) => e.stopPropagation());

    const head = el("div", "pa-head");
    const title = el("div", "pa-title");
    title.append(el("strong", "", "Alert harga & kurs"), el("small", "", "Dicek tiap 5 menit · sumber Yahoo Finance"));
    const close = el("button", "pa-mini", "✕"); close.type = "button"; close.title = "Tutup";
    close.addEventListener("click", (e) => { e.preventDefault(); e.stopPropagation(); setOpen(false); });
    head.append(title, close);

    const form = el("div", "pa-form");
    assetEl = el("select", "pa-asset");
    const gFx = document.createElement("optgroup"); gFx.label = "Kurs → Rupiah";
    for (const [c, n] of FX) { const o = el("option", "", `${c} · ${n}`); o.value = `fx:${c}`; gFx.appendChild(o); }
    const gSt = document.createElement("optgroup"); gSt.label = "Saham IDX";
    for (const c of STOCKS) { const o = el("option", "", c); o.value = `stock:${c}`; gSt.appendChild(o); }
    assetEl.append(gFx, gSt);
    assetEl.addEventListener("change", () => { say(""); void refreshNow(); });

    dirEl = el("select", "pa-dir");
    const up = el("option", "", "Naik ke ≥"); up.value = "above";
    const down = el("option", "", "Turun ke ≤"); down.value = "below";
    dirEl.append(up, down);

    targetEl = el("input", "pa-target");
    targetEl.type = "text"; targetEl.inputMode = "decimal"; targetEl.autocomplete = "off"; targetEl.placeholder = "Target (Rp)";
    targetEl.maxLength = 18;
    // pintasan demo di main.ts/nudge.ts tidak boleh ikut terpicu saat mengetik
    targetEl.addEventListener("keydown", (e) => {
      e.stopPropagation();
      if (e.key === "Enter") { e.preventDefault(); void addRule(); }
      if (e.key === "Escape") { e.preventDefault(); setOpen(false); }
    });

    const add = el("button", "pa-add", "Pasang"); add.type = "button";
    add.addEventListener("click", (e) => { e.preventDefault(); e.stopPropagation(); void addRule(); });
    form.append(assetEl, dirEl, targetEl, add);

    nowEl = el("div", "pa-now", "");
    msgEl = el("div", "pa-msg", ""); msgEl.dataset.bad = "0";
    listEl = el("div", "pa-list");
    listEl.addEventListener("click", onListClick);

    p.append(head, form, nowEl, msgEl, listEl);
    page.appendChild(p);
    panel = p;
    renderUi();
  }

  function setOpen(open: boolean) {
    if (!panel) return;
    panel.hidden = !open;
    bellEl?.setAttribute("aria-expanded", String(open));
    if (open) {
      // mulai dari mata uang yang sedang dilihat di konverter
      const shown = (document.getElementById("cu-target") as HTMLSelectElement | null)?.value;
      if (shown && assetEl && FX.some(([c]) => c === shown)) assetEl.value = `fx:${shown}`;
      say("");
      void refreshNow();
      targetEl?.focus();
    }
  }

  // halaman Currency bisa dibuat ulang (daftar halaman diubah dari Pengaturan): sisipkan lagi bila hilang
  function ensureUi() {
    const page = document.querySelector<HTMLElement>(".p-currency");
    const actions = page?.querySelector<HTMLElement>(".cu-head-actions");
    if (!page || !actions) return;
    let changed = false;
    if (!bellEl || !bellEl.isConnected) {
      changed = true;
      const bell = el("button", "pa-btn"); bell.type = "button";
      bell.setAttribute("aria-label", "Alert harga dan kurs"); bell.setAttribute("aria-expanded", "false");
      bell.innerHTML = `<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M6 8a6 6 0 0 1 12 0c0 7 3 9 3 9H3s3-2 3-9M10.3 21a1.94 1.94 0 0 0 3.4 0"/></svg>`;
      bell.addEventListener("click", (e) => { e.preventDefault(); e.stopPropagation(); setOpen(!!panel?.hidden); });
      actions.prepend(bell);
      bellEl = bell;
    }
    if (!panel || !panel.isConnected) { buildPanel(page); changed = true; }
    if (changed) renderUi(); // dulu renderUi() membangun ulang seluruh daftar tiap 2 dtk walau tak ada yang berubah
  }
  ensureUi();
  window.setInterval(ensureUi, 2000);

  // tombol "Coba tampilkan" (mis. dari Pengaturan)
  void listen("price-alert-test", () => {
    present({ id: "test", kind: "fx", code: "USD", dir: "above", target: 16500, created: Date.now() }, 16512);
  }).catch((err) => console.warn("[PriceAlert] Listener tes gagal:", err));
}