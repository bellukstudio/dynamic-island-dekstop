// @ts-nocheck
// Pemeriksaan versi dan notifikasi update.
// Aturan update dari Firestore (notch_config/update_config):
//  - versi terpasang  <  latest_version            -> WAJIB update (tanpa tombol "Nanti")
//  - versi terpasang  >= latest_version dan  < new_version -> OPSIONAL (boleh "Nanti")
//  - versi terpasang  >= new_version               -> tidak ada banner
// latest_version berfungsi sebagai versi minimum yang masih diizinkan, new_version adalah rilis terbaru.
type UpdateDecision = {
    kind: "required" | "optional" | "none";
    target: string;
};

export function register(ctx: Record<string, any>): void {
  ctx.safeUpdateUrl = function safeUpdateUrl(raw: string): string | null {
      const value = raw.trim();
      if (!value)
          return null;
      try {
          const candidate = /^[a-z][a-z\d+.-]*:\/\//i.test(value) ? value : `https://${value}`;
          const url = new URL(candidate);
          if (url.protocol !== "https:" && url.protocol !== "http:")
              return null;
          return url.href;
      }
      catch {
          return null;
      }
  };
  ctx.showUpdateBanner = function showUpdateBanner(currentVersion: string, remote: RemoteUpdateConfig, decision: UpdateDecision) {
      ctx.updateTargetUrl = ctx.safeUpdateUrl(remote.link_update) ?? "";
      ctx.updateRequired = decision.kind === "required";
      ctx.updateShownAt = Date.now();
      ctx.$("update-title").textContent = ctx.updateRequired ? "Pembaruan wajib" : "Pembaruan tersedia";
      ctx.$("update-current-version").textContent = `Versi saat ini ${currentVersion}`;
      ctx.$("update-new-version").textContent = ctx.updateRequired ? `Wajib update ke versi ${decision.target} untuk terus memakai Notch.`
          : `Versi ${decision.target} tersedia untuk diunduh.`;
      document.querySelector(".l-update")?.classList.toggle("is-required", ctx.updateRequired);
      // update wajib tidak bisa ditunda: tombol "Nanti" disembunyikan
      (ctx.$("update-later") as HTMLElement).style.display = ctx.updateRequired ? "none" : "";
      const openButton = ctx.$("update-open") as HTMLButtonElement;
      openButton.disabled = !ctx.updateTargetUrl;
      openButton.title = ctx.updateTargetUrl ? `Buka ${ctx.updateTargetUrl}` : "Tautan update belum valid";
      // Jangan memotong kartu pengingat/perangkat yang sedang tampil.
      if (ctx.override === null || ctx.override === "hub" || ctx.override === "update") {
          ctx.hubByHover = false;
          ctx.expand("update", 0); // banner tetap tampil sampai pengguna memilih Nanti atau membuka rilis
      }
  };
  ctx.compareVersionStrings = function compareVersionStrings(left: string, right: string): number | null {
      const parse = (value: string): number[] | null => {
          const clean = value.trim().replace(/^v/i, "").split(/[+-]/, 1)[0];
          if (!clean || !/^\d+(?:\.\d+)*$/.test(clean))
              return null;
          return clean.split(".").map((part) => Number(part));
      };
      const a = parse(left);
      const b = parse(right);
      if (!a || !b)
          return null;
      const length = Math.max(a.length, b.length);
      for (let i = 0; i < length; i++) {
          const av = a[i] ?? 0;
          const bv = b[i] ?? 0;
          if (av > bv)
              return 1;
          if (av < bv)
              return -1;
      }
      return 0;
  };
  // Ringkasan update: muncul SEKALI saja, saat versi terpasang == new_version dan field notes terisi.
  // Penanda "sudah tampil" disimpan per versi di localStorage. Kalau notes kosong, penanda tidak
  // ditulis, jadi ringkasan tetap muncul begitu notes ditambahkan di Firestore.
  ctx.maybeShowWhatsNew = async function maybeShowWhatsNew(installed: string, config: RemoteUpdateConfig) {
      const SEEN_KEY = "notch.whatsnew.seen";
      // Di konsol: __resetWhatsNew() menghapus penanda "sudah tampil" supaya bisa dites ulang.
      window.__resetWhatsNew = () => { try { localStorage.removeItem(SEEN_KEY); } catch { /* abaikan */ } };
      if (ctx.compareVersionStrings(installed, config.new_version) !== 0) {
          console.info(`[WhatsNew] Dilewati: versi terpasang ${installed} != new_version ${config.new_version}.`);
          return;
      }
      const notes = (config.notes ?? "").trim();
      if (!notes) {
          console.info("[WhatsNew] Dilewati: field notes di Firestore kosong / tidak ada.");
          return;
      }
      let seen = "";
      try { seen = localStorage.getItem(SEEN_KEY) ?? ""; } catch { /* abaikan */ }
      if (seen === installed) {
          console.info(`[WhatsNew] Dilewati: ringkasan versi ${installed} sudah pernah tampil. Jalankan __resetWhatsNew() untuk tes ulang.`);
          return;
      }
      console.info("[WhatsNew] Membuka popup ringkasan update...");
      try {
          await ctx.invoke("open_whats_new", { version: installed, notes });
          try { localStorage.setItem(SEEN_KEY, installed); } catch { /* abaikan */ }
      }
      catch (error) {
          console.warn("[WhatsNew] Gagal membuka jendela ringkasan (cek: Rust sudah di-restart? whats-new.html ada?):", error);
      }
  };
  // Pengingat update opsional: jendela "whats-new" dipakai ulang dengan mode "reminder",
  // berisi catatan rilis versi yang BELUM terpasang (bukan versi yang sudah dipasang seperti
  // maybeShowWhatsNew di atas) + tombol "Perbarui sekarang". Dibatasi sekali per REMIND_INTERVAL_MS
  // untuk target versi yang sama, supaya tidak mengganggu tiap kali app dibuka.
  ctx.maybeShowUpdateReminder = async function maybeShowUpdateReminder(installed: string, config: RemoteUpdateConfig, decision: UpdateDecision) {
      const REMIND_KEY = "notch.whatsnew.remind";
      const REMIND_INTERVAL_MS = 24 * 60 * 60 * 1000; // 24 jam
      // Di konsol: __resetUpdateReminder() menghapus penanda supaya bisa dites ulang.
      window.__resetUpdateReminder = () => { try { localStorage.removeItem(REMIND_KEY); } catch { /* abaikan */ } };
      const notes = (config.notes ?? "").trim();
      if (!notes) {
          console.info("[WhatsNew] Pengingat update dilewati: field notes di Firestore kosong / tidak ada.");
          return;
      }
      let last: { target?: string; at?: number } = {};
      try { last = JSON.parse(localStorage.getItem(REMIND_KEY) ?? "{}"); } catch { /* abaikan */ }
      if (last.target === decision.target && typeof last.at === "number" && Date.now() - last.at < REMIND_INTERVAL_MS) {
          console.info(`[WhatsNew] Pengingat update dilewati: versi ${decision.target} baru saja diingatkan.`);
          return;
      }
      console.info(`[WhatsNew] Menampilkan pengingat update ${installed} -> ${decision.target}...`);
      try {
          await ctx.invoke("open_whats_new", {
              version: decision.target,
              notes,
              mode: "reminder",
              updateUrl: ctx.updateTargetUrl || null,
          });
          try { localStorage.setItem(REMIND_KEY, JSON.stringify({ target: decision.target, at: Date.now() })); } catch { /* abaikan */ }
      } catch (error) {
          console.warn("[WhatsNew] Gagal membuka pengingat update:", error);
      }
  };
  ctx.decideUpdate = function decideUpdate(installed: string, config: RemoteUpdateConfig): UpdateDecision | null {
      const vsLatest = ctx.compareVersionStrings(installed, config.latest_version);
      if (vsLatest !== null && vsLatest < 0) {
          const newVsLatest = ctx.compareVersionStrings(config.new_version, config.latest_version);
          const target = newVsLatest !== null && newVsLatest >= 0 ? config.new_version : config.latest_version;
          return { kind: "required", target };
      }
      const offered = ctx.compareVersionStrings(config.new_version, installed);
      if (offered === null)
          return null; // format versi tidak valid
      return offered > 0 ? { kind: "optional", target: config.new_version } : { kind: "none", target: installed };
  };
}

export function init(ctx: Record<string, any>): void {
  ctx.$("update-later").addEventListener("click", (event) => {
      event.stopPropagation();
      if (ctx.updateRequired)
          return; // update wajib tidak boleh ditunda
      ctx.collapse();
  });
  ctx.$("update-open").addEventListener("click", async (event) => {
      event.stopPropagation();
      const button = event.currentTarget as HTMLButtonElement;
      const url = ctx.updateTargetUrl;
      if (!url || button.disabled)
          return;
      // link_update sebaiknya URL halaman rilis GitHub yang lengkap.
      // window.open() tidak membuka browser di WebView Tauri, jadi dibuka lewat Tauri (browser bawaan sistem).
      button.disabled = true;
      const result = await ctx.openExternalUrl(url);
      button.disabled = !ctx.updateTargetUrl;
      if (result.ok) {
          ctx.updateShownAt = Date.now(); // update wajib: beri waktu mengunduh sebelum banner muncul lagi
          ctx.collapse();
          return;
      }
      console.warn("[Update] Gagal membuka browser:", result.error);
      let copied = false;
      try {
          await navigator.clipboard.writeText(url);
          copied = true;
      }
      catch { /* clipboard tidak tersedia */ }
      // banner tetap tampil supaya pengguna tahu apa yang terjadi, bukan diam saja
      ctx.$("update-current-version").textContent = copied
          ? "Browser tidak bisa dibuka · tautan disalin, tempel di browser"
          : "Browser tidak bisa dibuka · buka tautan rilis secara manual";
  });
  ctx.updateTargetUrl = "";
  ctx.updateRequired = false;
  ctx.updateShownAt = 0;
  ctx.UPDATE_RESHOW_MS = 60000;
  ctx.pendingRequiredUpdate = null;
  setInterval(() => {
      const p = ctx.pendingRequiredUpdate;
      if (!p || ctx.override !== null || ctx.inside || ctx.isDraggingWindow || ctx.holdHover)
          return;
      if (Date.now() - ctx.updateShownAt < ctx.UPDATE_RESHOW_MS)
          return;
      ctx.showUpdateBanner(p.installed, p.config, p.decision);
  }, 5000);
  void ctx.loadRemoteUpdateConfig()
      .then(async (config) => {
      window.__NOTCH_UPDATE_CONFIG__ = config;
      window.dispatchEvent(new CustomEvent<RemoteUpdateConfig>("notch:firebase-update-config", { detail: config }));
      console.info("[Firebase] Konfigurasi update berhasil dimuat:", config);
      try {
          const currentVersion = await ctx.getVersion();
          const decision = ctx.decideUpdate(currentVersion, config);
          if (decision === null) {
              console.warn("[Firebase] Format versi tidak valid; banner update dilewati.", {
                  installed: currentVersion,
                  latest: config.latest_version,
                  offered: config.new_version,
              });
              return;
          }
          console.info(`[WhatsNew] Terpasang ${currentVersion}, keputusan update: ${decision.kind}, new_version ${config.new_version}, notes ${config.notes ? "ada" : "KOSONG"}.`);
          if (decision.kind === "required") {
              console.warn(`[Firebase] Update WAJIB: ${currentVersion} < versi minimum ${config.latest_version} -> ${decision.target}`);
              ctx.pendingRequiredUpdate = { installed: currentVersion, config, decision };
              ctx.showUpdateBanner(currentVersion, config, decision);
          }
          else if (decision.kind === "optional") {
              console.info(`[Firebase] Update opsional tersedia: ${currentVersion} -> ${decision.target}`);
              ctx.showUpdateBanner(currentVersion, config, decision);
              // showUpdateBanner mengisi ctx.updateTargetUrl dulu, baru popup pengingat dibuka.
              void ctx.maybeShowUpdateReminder(currentVersion, config, decision);
          }
          else {
              console.info(`[Firebase] Aplikasi sudah terbaru (terpasang ${currentVersion}).`);
              void ctx.maybeShowWhatsNew(currentVersion, config);
          }
      }
      catch (error) {
          console.warn("[Firebase] Tidak bisa membaca versi aplikasi Tauri:", error);
      }
  })
      .catch((error: unknown) => {
      console.warn("[Firebase] Gagal memuat notch_config/update_config:", error);
  });
}