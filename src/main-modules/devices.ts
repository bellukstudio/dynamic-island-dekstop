// @ts-nocheck
// Bluetooth, baterai, dan perangkat terhubung.
// ---------- bluetooth ----------
type BtDev = {
    name: string;
    battery: number | null;
};

export function register(ctx: Record<string, any>): void {
  ctx.paintBt = function paintBt(name: string, battery: number | null, connected: boolean) {
      ctx.$("bt-name").textContent = name;
      ctx.$("bt-sub").textContent = !connected
          ? "Disconnected"
          : battery != null ? `Connected · ${battery}%` : "Connected";
      ctx.$("bt-ring").style.setProperty("--pct", String(connected ? (battery ?? 100) : 0));
  };
  ctx.paintBtNone = function paintBtNone() {
      ctx.$("bt-name").textContent = "Bluetooth";
      ctx.$("bt-sub").textContent = "Tidak ada perangkat";
      ctx.$("bt-ring").style.setProperty("--pct", "0");
  };
  ctx.refreshBt = 
  // isi kartu dari keadaan nyata (termasuk perangkat yang sudah terhubung sebelum app dibuka)
  async function refreshBt() {
      try {
          const list = await ctx.invoke<BtDev[]>("bt_list");
          if (list.length)
              ctx.paintBt(list[0].name, list[0].battery, true);
          else
              ctx.paintBtNone();
      }
      catch {
          ctx.paintBtNone();
      }
  };
  ctx.openBtManual = 
  // dibuka manual (tombol B): tampilkan keadaan saat ini
  async function openBtManual() {
      await ctx.refreshBt();
      ctx.hubByHover = false;
      ctx.expand("bt");
  };
  ctx.handleBattery = function handleBattery(percent: number, charging: boolean) {
      ctx.noteBattery(percent, charging); // Conversation Moments
      if (ctx.wasCharging === false && charging)
          ctx.flash("love", 3000);
      ctx.wasCharging = charging;
      const low = percent <= 15 && !charging;
      if (low !== ctx.lowBattery) {
          ctx.lowBattery = low;
          ctx.updateBase();
      }
      ctx.$("bat-pct").textContent = `${percent}%`;
      ctx.$("bat-sub").textContent = charging ? "Sedang mengisi" : low ? "Baterai rendah" : "Tidak mengisi";
      const fill = ctx.$("bat-fill");
      fill.style.width = `${percent}%`;
      fill.classList.toggle("low", low);
      fill.classList.toggle("charging", charging);
  };
  ctx.showDevice = function showDevice(kind: string, name: string, connected: boolean, sub?: string) {
      ctx.notch.classList.remove("greeting-active"); // kartu perangkat tidak boleh memakai layer greeting
      const icon = ctx.$("dev-icon");
      icon.dataset.kind = ctx.ICONS[kind] ? kind : "usb";
      icon.innerHTML = `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">${ctx.ICONS[kind] ?? ctx.ICONS.usb}</svg>`;
      icon.classList.toggle("off", !connected);
      ctx.$("dev-chip").classList.toggle("off", !connected);
      ctx.$("dev-chip").innerHTML = connected
          ? `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12l5 5L20 7"/></svg>`
          : `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"><path d="M6 6L18 18M18 6L6 18"/></svg>`;
      const deviceSub = sub ?? `${ctx.LABELS[kind] ?? "Perangkat"} · ${connected ? "Terhubung" : "Dilepas"}`;
      ctx.$("dev-name").textContent = name;
      ctx.$("dev-sub").textContent = deviceSub;
      ctx.hubByHover = false;
      // Notifikasi colok/cabut memakai awan ketika notch dock di sisi/bawah.
      // Jika tidak memungkinkan, pertahankan kartu perangkat yang sudah ada.
      const shownAsBubble = !ctx.meetingMode &&
          ctx.momentBubble?.show(name, deviceSub, connected ? ctx.T.card_ms : 2000) === true;
      if (!shownAsBubble) ctx.expand("dev", connected ? ctx.T.card_ms : 2000);
      icon.classList.remove("play");
      void icon.offsetWidth;
      if (connected)
          icon.classList.add("play");
      if (!connected)
          ctx.flash("surprised", 900);
      else
          ctx.flash(kind === "charger" ? "love" : "happy", 2500);
  };
}

export function init(ctx: Record<string, any>): void {
  ctx.paintBtNone();
  ctx.refreshBt();
  setTimeout(ctx.refreshBt, 4000); // pemantau butuh beberapa detik untuk pembacaan pertama
  
  ctx.listen<{
      name: string;
      battery: number | null;
      connected?: boolean;
  }>("bluetooth", (e) => {
      const { name, battery } = e.payload;
      const connected = e.payload.connected !== false; // event lama tanpa field = terhubung
      ctx.paintBt(name, battery, connected);
      ctx.hubByHover = false;
      // Notch di kiri/kanan/dasar: notifikasi Bluetooth tampil sebagai awan; selain itu kartu Bluetooth.
      const btSub = !connected ? "Bluetooth · Terputus" : battery != null ? `Bluetooth · Terhubung · ${battery}%` : "Bluetooth · Terhubung";
      const btBubble = !ctx.meetingMode && ctx.momentBubble?.show(name, btSub, ctx.T.card_ms) === true;
      if (!btBubble)
          ctx.expand("bt", ctx.T.card_ms);
      if (connected)
          ctx.flash("happy", 2500);
  });
  ctx.wasCharging = null;
  ctx.listen<{
      percent: number;
      charging: boolean;
  }>("battery", (e) => ctx.handleBattery(e.payload.percent, e.payload.charging));
  ctx.ICONS = {
      charger: `<path d="M13 2 4 14h7l-1 8 9-12h-7z"/>`,
      usb: `<path d="M9 2v6M15 2v6M6 8h12v4a6 6 0 0 1-12 0zM12 18v4"/>`,
      usbc: `<rect x="4" y="9" width="16" height="6" rx="3"/><path d="M8 12h8"/>`,
      storage: `<rect x="3" y="14" width="18" height="7" rx="2"/><path d="M5 14l2-9h10l2 9"/><path d="M7 17.5h.01M11 17.5h6"/>`,
      keyboard: `<rect x="2" y="6" width="20" height="12" rx="2"/><path d="M6 10h.01M10 10h.01M14 10h.01M18 10h.01M7 14h10"/>`,
      mouse: `<rect x="6" y="3" width="12" height="18" rx="6"/><path d="M12 7v4"/>`,
      audio: `<path d="M4 14v-2a8 8 0 0 1 16 0v2M4 14h3v6H5a1 1 0 0 1-1-1zM20 14h-3v6h2a1 1 0 0 0 1-1z"/>`,
      phone: `<rect x="7" y="2" width="10" height="20" rx="2"/><path d="M11 18h2"/>`,
      camera: `<path d="M4 8h3l2-3h6l2 3h3v11H4z"/><circle cx="12" cy="13" r="3.5"/>`,
      mic: `<rect x="9" y="3" width="6" height="11" rx="3"/><path d="M5 11a7 7 0 0 0 14 0M12 18v3"/>`,
      printer: `<path d="M7 9V3h10v6M7 17H4v-7h16v7h-3M7 14h10v7H7z"/>`,
  };
  ctx.LABELS = {
      charger: "Charger", usb: "USB", usbc: "USB Type-C", storage: "Penyimpanan",
      keyboard: "Keyboard", mouse: "Mouse", audio: "Audio", phone: "Ponsel",
      camera: "Kamera", mic: "Mikrofon", printer: "Printer",
  };
  ctx.listen<{
      kind: string;
      name: string;
      connected: boolean;
  }>("device", (e) => ctx.showDevice(e.payload.kind, e.payload.name, e.payload.connected));
}