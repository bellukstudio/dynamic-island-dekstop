import { listen } from "@tauri-apps/api/event";

// Pesan WhatsApp baru ditampilkan DI NOTCH (awan bila notch menempel di kiri/kanan/bawah,
// kartu teks bila di atas), memakai mekanisme yang sama dengan notifikasi lain di notch.

export type WhatsappNoticeHooks = {
  /** Notch sedang bebas (tidak rapat, adzan, atau kartu lain)? */
  canShow: () => boolean;
  show: (title: string, sub: string, ms: number) => void;
  mood?: (m: string, ms: number) => void;
};

const SHOW_MS = 5000;
const RETRY_MS = 2000;   // coba lagi bila notch sedang dipakai kartu lain
const MAX_RETRY = 10;    // menyerah setelah ~20 detik supaya tidak muncul terlambat
const DEDUPE_MS = 4000;  // lonjakan pesan beruntun dari akun yang sama dihitung satu
const MAX_PREVIEW_LENGTH = 44;  // satu baris di notch (~40 karakter pada kartu)
const MAX_WORD_LENGTH = 20;     // satu "kata" tanpa spasi (mis. "dkdkdkdk...") dipotong di sini

// Awan notch hanya melebar sampai maxWidth, jadi string panjang tanpa spasi tidak bisa
// dipatah browser dan meluap keluar awan/kartu. Gaya ini memaksa satu baris dengan elipsis.
const STYLE_ID = "wa-notice-wrap-style";
function ensureWrapStyle(): void {
  if (document.getElementById(STYLE_ID)) return;
  const style = document.createElement("style");
  style.id = STYLE_ID;
  style.textContent = `
    .moment-bubble .mb-text, .moment-bubble .mb-sub, #dev-sub.wa-oneline {
      display: block; min-width: 0; max-width: 100%; white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
    }
  `;
  document.head.appendChild(style);
}

const UNREAD_LABEL = /^\s*\d+\s+(?:unread messages?|pesan belum dibaca)\s*/i;

/** Rapikan teks pesan: buang spasi berlebih, potong kata terlalu panjang, batasi total panjang. */
function tidyMessage(raw: string): string {
  const words = raw
    .replace(/\s+/g, " ")
    .trim()
    .split(" ")
    .map((word) => {
      const chars = Array.from(word);
      return chars.length > MAX_WORD_LENGTH ? `${chars.slice(0, MAX_WORD_LENGTH).join("")}…` : word;
    });
  const chars = Array.from(words.join(" "));
  return chars.length > MAX_PREVIEW_LENGTH ? `${chars.slice(0, MAX_PREVIEW_LENGTH - 1).join("").trimEnd()}…` : chars.join("");
}

interface WhatsappNotification {
  account: number;
  sender?: string | null;
  preview?: string | null;
}

export async function initWhatsappNotifications(h: WhatsappNoticeHooks): Promise<void> {
  ensureWrapStyle();
  const lastEvent = new Map<number, number>();
  let pending: WhatsappNotification[] = [];
  let retryTimer = 0;
  let tries = 0;

  const flush = (): void => {
    retryTimer = 0;
    if (pending.length === 0) return;
    if (!h.canShow()) {
      if (++tries > MAX_RETRY) {
        pending = [];
        tries = 0;
        return;
      }
      retryTimer = window.setTimeout(flush, RETRY_MS);
      return;
    }
    tries = 0;
    const notices = pending;
    pending = [];
    const accounts = [...new Set(notices.map(({ account }) => account))].sort();
    if (notices.length === 1) {
      const notice = notices[0];
      const sender = notice.sender?.replace(UNREAD_LABEL, "").trim();
      const preview = notice.preview?.replace(UNREAD_LABEL, "").trim();
      const message = sender && preview ? `${sender}: ${preview}` : sender || preview || `Akun ${notice.account}`;
      const sub = tidyMessage(message);
      h.show(`Pesan WhatsApp baru · Akun ${notice.account}`, sub, SHOW_MS);
    } else {
      const sub = accounts.length > 1 ? "Akun 1 dan Akun 2" : `Akun ${accounts[0]}`;
      h.show("Pesan WhatsApp baru", sub, SHOW_MS);
    }
    h.mood?.("happy", SHOW_MS);
  };

  await listen<WhatsappNotification>("whatsapp-notification", ({ payload }) => {
    const account = payload.account;
    if (account !== 1 && account !== 2) return;
    const now = Date.now();
    if (now - (lastEvent.get(account) ?? 0) < DEDUPE_MS) return;
    lastEvent.set(account, now);
    pending.push(payload);
    if (!retryTimer) flush();
  });
}