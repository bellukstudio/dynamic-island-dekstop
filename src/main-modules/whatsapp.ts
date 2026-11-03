import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// Satu jendela WhatsApp: halaman ini adalah bar tab + layar PIN + pengaturan.
// Saat sebuah akun terbuka, sisi Rust menyusutkan webview ini jadi 48px (hanya bar tab) dan
// menampilkan webview WhatsApp Web akun itu tepat di bawahnya.

interface AccountStatus {
  configured: boolean;
  locked: boolean;
  open: boolean;
}

interface WhatsappStatus {
  notifications: boolean;
  background_sessions: boolean;
  preview_messages: boolean;
  blur_mode: string;
  view: number; // 0 = panel penuh, 1/2 = akun tampil di bawah bar
  accounts: [AccountStatus, AccountStatus];
}

interface WhatsappNotification {
  account: number;
  sender?: string | null;
  preview?: string | null;
}

type Tab = "1" | "2" | "settings";

const root = document.getElementById("app");
if (!root) throw new Error("Elemen aplikasi WhatsApp tidak ditemukan");

const style = document.createElement("style");
style.textContent = `
  html, body { margin: 0 !important; padding: 0 !important; height: 100%; overflow: hidden; display: block !important;
    background: #0f1612; color: #eef4f1; font: 14px/1.45 system-ui, "Segoe UI", sans-serif; }
  .wa-root { display: flex; flex-direction: column; height: 100vh; }
  .wa-bar { flex: none; height: 48px; box-sizing: border-box; display: flex; align-items: center; gap: 8px;
    padding: 0 10px; background: #141d19; border-bottom: 1px solid #26352d; }
  .wa-tabs { display: flex; gap: 4px; flex: 1; min-width: 0; }
  .wa-tab { display: inline-flex; align-items: center; gap: 8px; height: 34px; padding: 0 14px; border: 0;
    border-radius: 8px; background: transparent; color: #9fb3a8; font: inherit; font-weight: 600; cursor: pointer; }
  .wa-tab:hover { background: #1b2822; color: #eef4f1; }
  .wa-tab.active { background: #243730; color: #fff; }
  .wa-dot { width: 8px; height: 8px; border-radius: 50%; background: #4a5b52; }
  .wa-tab[data-unlocked="1"] .wa-dot { background: #25d366; }
  .wa-tab.has-new .wa-dot { background: #ffb020; box-shadow: 0 0 0 3px #ffb02033; }
  .wa-actions { display: flex; gap: 6px; }
  .wa-btn { height: 34px; padding: 0 14px; border: 0; border-radius: 8px; background: #26352d; color: #eef4f1;
    font: inherit; font-weight: 600; cursor: pointer; }
  .wa-btn:hover { background: #31453a; }
  .wa-btn.primary { background: #25d366; color: #06210f; }
  .wa-btn.primary:hover { background: #3be07b; }
  .wa-btn:disabled { opacity: .55; cursor: default; }
  .wa-body { flex: 1; overflow: auto; padding: 28px 20px; }
  .wa-card { max-width: 520px; margin: 0 auto 16px; padding: 20px; border: 1px solid #26352d; border-radius: 12px;
    background: #17211c; }
  .wa-card h2 { margin: 0 0 4px; font-size: 18px; }
  .wa-card p { margin: 0 0 14px; color: #9fb3a8; }
  .wa-row { display: flex; flex-wrap: wrap; gap: 8px; align-items: center; }
  .wa-input { flex: 1; min-width: 130px; height: 36px; box-sizing: border-box; padding: 0 12px; border: 1px solid #2d3f35;
    border-radius: 8px; background: #101814; color: inherit; font: inherit; }
  .wa-input:focus { outline: 2px solid #25d36688; border-color: #25d366; }
  .wa-field { display: flex; justify-content: space-between; align-items: center; gap: 12px; margin-top: 12px; }
  .wa-field select { height: 34px; border: 1px solid #2d3f35; border-radius: 8px; background: #101814; color: inherit; padding: 0 8px; }
  .wa-toggle { display: flex; align-items: center; gap: 8px; margin-top: 14px; cursor: pointer; }
  .wa-status { max-width: 520px; margin: 0 auto; min-height: 20px; color: #ff8a80; }
  .wa-status.ok { color: #25d366; }
  .wa-foot { max-width: 520px; margin: 8px auto 0; color: #6f8478; font-size: 12px; }
  .wa-toast { position: fixed; left: 50%; bottom: 20px; transform: translateX(-50%); padding: 9px 14px;
    border: 1px solid #3b5747; border-radius: 10px; background: #202d26; box-shadow: 0 8px 28px #0008; }
  .wa-toast[hidden] { display: none; }
  .wa-acc-line { display: flex; justify-content: space-between; align-items: center; padding: 8px 0; border-top: 1px solid #22302a; }
  .wa-acc-line:first-of-type { border-top: 0; }
`;
document.head.appendChild(style);

let status: WhatsappStatus | null = null;
let tab: Tab = "1";
let busy = false;
let pendingRefresh = false;
let toastTimer = 0;
const hasNew: [boolean, boolean] = [false, false];

function accountOf(t: Tab): number {
  return t === "settings" ? 0 : Number(t);
}

function tabButton(t: Tab, label: string): string {
  const n = accountOf(t);
  const unlocked = n > 0 && status && !status.accounts[n - 1].locked ? "1" : "0";
  const dot = n > 0 ? `<i class="wa-dot"></i>` : "";
  return `<button class="wa-tab${tab === t ? " active" : ""}${n > 0 && hasNew[n - 1] ? " has-new" : ""}"
    data-tab="${t}" data-unlocked="${unlocked}" role="tab" aria-selected="${tab === t}">${dot}${label}</button>`;
}

function renderBar(): string {
  const n = accountOf(tab);
  const canLock = n > 0 && status?.accounts[n - 1].configured && !status.accounts[n - 1].locked;
  return `
    <header class="wa-bar">
      <nav class="wa-tabs" role="tablist">
        ${tabButton("1", "Akun 1")}
        ${tabButton("2", "Akun 2")}
        ${tabButton("settings", "Pengaturan")}
      </nav>
      <div class="wa-actions">
        ${canLock ? `<button class="wa-btn" data-lock="${n}">Kunci</button>` : ""}
      </div>
    </header>`;
}

function renderAccountBody(n: number): string {
  const account = status!.accounts[n - 1];
  if (!account.locked) {
    // Akun terbuka tetapi webview-nya belum tampil (mis. baru selesai dibuat).
    return `<section class="wa-card"><h2>Akun ${n}</h2><p>Terbuka.</p>
      <div class="wa-row"><button class="wa-btn primary" data-show="${n}">Tampilkan</button>
      <button class="wa-btn" data-lock="${n}">Kunci</button></div></section>`;
  }
  const confirm = account.configured ? "" :
    `<input class="wa-input" id="confirm" type="password" inputmode="numeric" autocomplete="new-password" maxlength="12" placeholder="Ulangi PIN" aria-label="Ulangi PIN akun ${n}" />`;
  return `
    <section class="wa-card">
      <h2>Akun ${n}</h2>
      <p>${account.configured
        ? "Terkunci. Masukkan PIN untuk membuka."
        : "Belum ada PIN. Buat PIN 4–12 angka, lalu pindai QR WhatsApp Web dari ponsel."}</p>
      <div class="wa-row">
        <input class="wa-input" id="pin" type="password" inputmode="numeric" autocomplete="new-password" maxlength="12"
          placeholder="${account.configured ? "PIN akun" : "PIN baru"}" aria-label="PIN akun ${n}" />
        ${confirm}
        <button class="wa-btn primary" data-open="${n}">${account.configured ? "Buka" : "Buat PIN & tautkan"}</button>
      </div>
    </section>`;
}

function renderChangePin(): string {
  const s = status!;
  const options = ([1, 2] as const)
    .filter((n) => s.accounts[n - 1].configured)
    .map((n) => `<option value="${n}">Akun ${n}</option>`)
    .join("");
  if (!options) {
    return `<section class="wa-card"><h2>Ubah PIN</h2><p>Belum ada akun yang punya PIN. Buat PIN dari tab akun.</p></section>`;
  }
  return `
    <section class="wa-card">
      <h2>Ubah PIN</h2>
      <div class="wa-field"><label for="cp-acc">Akun</label><select id="cp-acc">${options}</select></div>
      <div class="wa-row" style="margin-top:12px">
        <input class="wa-input" id="cp-old" type="password" inputmode="numeric" autocomplete="off" maxlength="12" placeholder="PIN lama" aria-label="PIN lama" />
        <input class="wa-input" id="cp-new" type="password" inputmode="numeric" autocomplete="new-password" maxlength="12" placeholder="PIN baru" aria-label="PIN baru" />
        <input class="wa-input" id="cp-new2" type="password" inputmode="numeric" autocomplete="new-password" maxlength="12" placeholder="Ulangi PIN baru" aria-label="Ulangi PIN baru" />
        <button class="wa-btn primary" data-changepin>Simpan PIN</button>
      </div>
    </section>`;
}

async function submitChangePin(): Promise<void> {
  const get = (id: string) => document.getElementById(id) as HTMLInputElement;
  const account = Number((document.getElementById("cp-acc") as HTMLSelectElement).value);
  const oldPin = get("cp-old").value;
  const newPin = get("cp-new").value;
  if (!oldPin || !newPin) {
    setStatus("Isi PIN lama dan PIN baru.");
    return;
  }
  if (newPin !== get("cp-new2").value) {
    setStatus("Konfirmasi PIN baru tidak sama.");
    return;
  }
  await run(
    () => invoke("whatsapp_change_pin", { account, oldPin, newPin }),
    `PIN Akun ${account} diperbarui.`,
  );
}

function renderSettingsBody(): string {
  const s = status!;
  const line = (n: number): string => {
    const a = s.accounts[n - 1];
    const state = a.locked ? "Terkunci" : "Terbuka";
    return `<div class="wa-acc-line"><span>Akun ${n} · ${state}${a.configured ? "" : " · PIN belum dibuat"}</span>
      ${a.configured && !a.locked ? `<button class="wa-btn" data-lock="${n}">Kunci</button>` : ""}</div>`;
  };
  return `
    <section class="wa-card">
      <h2>Akun</h2>
      <p>Setiap akun memakai profil browser terpisah. PIN mengunci akses dari aplikasi ini.</p>
      ${line(1)}${line(2)}
    </section>
    <section class="wa-card">
      <h2>Privasi &amp; notifikasi</h2>
      <div class="wa-field"><label for="blur">Privasi chat</label>
        <select id="blur">
          <option value="all_hover">Blur semua · buka saat diarahkan</option>
          <option value="all_always">Blur semua · selalu</option>
          <option value="chat_hover">Blur isi chat · buka saat diarahkan</option>
          <option value="off">Tidak blur</option>
        </select></div>
      <label class="wa-toggle"><input id="notifications" type="checkbox" /> Notifikasi pesan baru di notch</label>
      <label class="wa-toggle"><input id="background-sessions" type="checkbox" /> Jaga sesi tetap hidup saat panel ditutup (notifikasi latar)</label>
      <label class="wa-toggle"><input id="preview-messages" type="checkbox" /> Tampilkan nama dan isi pesan di notch</label>
      <p>Sesi latar memakai RAM ratusan MB per akun. Saat dimatikan, menutup panel menghentikan kedua akun (RAM bebas) dan notifikasi hanya muncul selama akun terbuka; sesi login tetap tersimpan, hanya perlu memasukkan PIN lagi.</p>
      <p>Isi pesan hanya dibaca saat opsi ini aktif. Jika nonaktif, notifikasi tetap generik. Mode blur semua juga menyamarkan nama dan preview chat.</p>
    </section>
    ${renderChangePin()}
    <p class="wa-foot">Notifikasi mendeteksi kenaikan jumlah pesan belum dibaca.
      Sesi latar hanya hidup bila "Jaga sesi tetap hidup" dicentang; bila tidak, RAM bebas saat panel ditutup.</p>`;
}

function render(): void {
  if (!status) return;
  const n = accountOf(tab);
  // Webview akun tampil di bawah bar: halaman ini hanya setinggi bar, tak perlu isi.
  const compact = n > 0 && status.view === n;
  const body = compact ? "" : `
    <main class="wa-body">
      ${n > 0 ? renderAccountBody(n) : renderSettingsBody()}
      <p class="wa-status" id="status" role="status"></p>
      <div class="wa-toast" id="toast" hidden></div>
    </main>`;
  root!.innerHTML = `<div class="wa-root">${renderBar()}${body}</div>`;
  if (!compact) {
    const blur = document.getElementById("blur") as HTMLSelectElement | null;
    if (blur) blur.value = status.blur_mode;
    const notif = document.getElementById("notifications") as HTMLInputElement | null;
    if (notif) notif.checked = status.notifications;
    const background = document.getElementById("background-sessions") as HTMLInputElement | null;
    if (background) background.checked = status.background_sessions;
    const preview = document.getElementById("preview-messages") as HTMLInputElement | null;
    if (preview) preview.checked = status.preview_messages;
    (document.getElementById("pin") as HTMLInputElement | null)?.focus();
  }
}

function setStatus(message = "", ok = false): void {
  const el = document.getElementById("status");
  if (!el) return;
  el.textContent = message;
  el.classList.toggle("ok", ok);
}

async function refresh(): Promise<void> {
  status = await invoke<WhatsappStatus>("whatsapp_status");
  render();
}

async function run(action: () => Promise<void>, successMessage = ""): Promise<void> {
  if (busy) return;
  busy = true;
  setStatus("");
  try {
    await action();
    await refresh();
    if (successMessage) setStatus(successMessage, true);
  } catch (error) {
    try {
      await refresh();
    } catch (refreshError) {
      console.error("[WhatsApp] Gagal memuat ulang status:", refreshError);
    }
    setStatus(String(error));
  } finally {
    busy = false;
    if (pendingRefresh) {
      pendingRefresh = false;
      void refresh().catch((error) => console.error("[WhatsApp] Gagal memuat ulang status:", error));
    }
  }
}

async function selectTab(next: Tab): Promise<void> {
  const n = accountOf(next);
  // Rust jatuh ke tampilan 0 bila akun masih terkunci, sehingga form PIN yang tampil.
  // Tab lokal baru diganti setelah Rust berhasil, supaya UI tidak terjebak di tab yang
  // tidak sinkron dengan webview native (dulu: bar 48px tak bisa pindah tab lagi).
  await invoke("whatsapp_set_view", { view: n });
  tab = next;
  if (n > 0) hasNew[n - 1] = false;
}

async function submitPin(n: number): Promise<void> {
  const pinInput = document.getElementById("pin") as HTMLInputElement | null;
  const confirmInput = document.getElementById("confirm") as HTMLInputElement | null;
  const pin = pinInput?.value ?? "";
  if (!pin) {
    setStatus("Masukkan PIN terlebih dahulu.");
    pinInput?.focus();
    return;
  }
  const configured = status!.accounts[n - 1].configured;
  if (!configured && pin !== (confirmInput?.value ?? "")) {
    setStatus("Konfirmasi PIN tidak sama.");
    confirmInput?.focus();
    return;
  }
  await run(async () => {
    try {
      await invoke(configured ? "whatsapp_unlock" : "whatsapp_set_pin", { account: n, pin });
    } finally {
      if (pinInput) pinInput.value = "";
      if (confirmInput) confirmInput.value = "";
    }
  });
}

root.addEventListener("click", (event) => {
  const target = event.target as HTMLElement;
  const tabBtn = target.closest<HTMLButtonElement>("[data-tab]");
  const openBtn = target.closest<HTMLButtonElement>("[data-open]");
  const lockBtn = target.closest<HTMLButtonElement>("[data-lock]");
  const showBtn = target.closest<HTMLButtonElement>("[data-show]");
  if (target.closest("[data-changepin]")) {
    void submitChangePin();
    return;
  }
  if (tabBtn) {
    void run(() => selectTab(tabBtn.dataset.tab as Tab));
  } else if (openBtn) {
    void submitPin(Number(openBtn.dataset.open));
  } else if (showBtn) {
    void run(() => invoke("whatsapp_set_view", { view: Number(showBtn.dataset.show) }));
  } else if (lockBtn) {
    void run(() => invoke("whatsapp_lock", { account: Number(lockBtn.dataset.lock) }));
  }
});

root.addEventListener("keydown", (event) => {
  if (event.key !== "Enter") return;
  const target = event.target as HTMLElement;
  if (target.id === "pin" || target.id === "confirm") {
    void submitPin(accountOf(tab));
  }
});

root.addEventListener("change", (event) => {
  const target = event.target as HTMLElement;
  if (target.id === "notifications") {
    const enabled = (target as HTMLInputElement).checked;
    void run(() => invoke("whatsapp_set_notifications", { enabled }));
  } else if (target.id === "background-sessions") {
    const enabled = (target as HTMLInputElement).checked;
    void run(() => invoke("whatsapp_set_background", { enabled }));
  } else if (target.id === "preview-messages") {
    const enabled = (target as HTMLInputElement).checked;
    void run(() => invoke("whatsapp_set_preview", { enabled }));
  } else if (target.id === "blur") {
    const mode = (target as HTMLSelectElement).value;
    void run(() => invoke("whatsapp_set_blur", { mode }));
  }
});

function showToast(account: number): void {
  const toast = document.getElementById("toast");
  if (!toast) return;
  toast.textContent = `Pesan baru di Akun ${account}`;
  toast.hidden = false;
  window.clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => { toast.hidden = true; }, 5000);
}

// Rust mengubah status tanpa lewat UI ini (jendela dibuka lagi / semua akun dikunci saat ditutup):
// muat ulang supaya form PIN muncul tanpa perlu Ctrl+R.
void listen("whatsapp-state", () => {
  if (busy) {
    pendingRefresh = true;
    return;
  }
  void refresh()
    .then(() => {
      if (status && status.view !== 0) {
        tab = String(status.view) as Tab;
        render();
      }
    })
    .catch((error) => console.error("[WhatsApp] Gagal memuat ulang status:", error));
}).catch((error) => console.error("[WhatsApp] Gagal mendengarkan perubahan status:", error));

void listen<WhatsappNotification>("whatsapp-notification", ({ payload }) => {
  const n = payload.account;
  if (n !== 1 && n !== 2) return;
  // Akun yang sedang tampil tidak perlu penanda; akun lain diberi titik oranye di tabnya.
  if (status?.view !== n) {
    hasNew[n - 1] = true;
    document.querySelector(`[data-tab="${n}"]`)?.classList.add("has-new");
  }
  showToast(n);
}).catch((error) => console.error("[WhatsApp] Gagal mendengarkan notifikasi:", error));

void refresh()
  .then(() => {
    if (status && status.view !== 0) {
      tab = String(status.view) as Tab;
      render();
    }
  })
  .catch((error) => {
    root.textContent = String(error);
  });