// @ts-nocheck
// Riwayat clipboard dan indikator privasi.
// ---------- indikator mic / kamera ----------
type Privacy = {
    mic: boolean;
    cam: boolean;
    mic_app: string;
    cam_app: string;
};

export function register(ctx: Record<string, any>): void {
  ctx.paintClip = function paintClip(items: string[]) {
      const list = document.getElementById("clip-list");
      if (!list)
          return; // halaman clipboard dimatikan di config
      ctx.$("clip-count").textContent = items.length ? `Clipboard · ${items.length}` : "Clipboard";
      list.replaceChildren();
      if (items.length === 0) {
          const empty = document.createElement("div");
          empty.className = "clip-empty";
          empty.textContent = "Belum ada yang disalin";
          list.appendChild(empty);
          return;
      }
      items.forEach((text) => {
          const row = document.createElement("button"); // button: tidak memicu drag geser halaman
          row.className = "clip-item";
          row.title = text.length > 300 ? text.slice(0, 300) + "…" : text;
          const flat = text.replace(/\s+/g, " ").trim();
          row.textContent = flat.length > 120 ? flat.slice(0, 120) + "…" : flat;
          row.addEventListener("click", (e) => {
              e.stopPropagation();
              ctx.invoke("clip_copy", { text }); // Rust menyalin ulang dan memindahkannya ke paling atas
              ctx.flash("happy", 1000);
          });
          list.appendChild(row);
      });
  };
  ctx.refreshClip = function refreshClip() {
      ctx.invoke<string[]>("clip_list").then(ctx.paintClip).catch(() => { });
  };
  ctx.paintPrivacy = function paintPrivacy() {
      const on = ctx.privacyOn && ctx.priv
          ? [ctx.priv.cam ? "cam" : "", ctx.priv.mic ? "mic" : ""].filter(Boolean).join(" ")
          : "";
      if (on)
          ctx.notch.dataset.priv = on;
      else
          ctx.notch.removeAttribute("data-priv");
  };
}

export function init(ctx: Record<string, any>): void {
  let meetingStartedAt = 0;
  let meetingTimer: number | undefined;
  let meetingUsesBubble = false;

  const elapsedLabel = () => {
      const seconds = Math.max(0, Math.floor((Date.now() - meetingStartedAt) / 1000));
      const hours = Math.floor(seconds / 3600);
      const minutes = Math.floor((seconds % 3600) / 60);
      const secs = seconds % 60;
      return hours > 0
          ? `${String(hours).padStart(2, "0")}:${String(minutes).padStart(2, "0")}:${String(secs).padStart(2, "0")}`
          : `${String(minutes).padStart(2, "0")}:${String(secs).padStart(2, "0")}`;
  };

  // Notch menempel di dasar atau sisi layar? (kelas dock dibaca dari notch dan shell)
  const isSideOrBottomDock = () =>
      ["is-bottom", "is-edge-left", "is-edge-right"].some((c) =>
          ctx.notch.classList.contains(c) || ctx.$("shell").classList.contains(c));

  const paintMeetingTimer = () => {
      if (!ctx.meetingMode) return;
      if (isSideOrBottomDock()) {
          // Pengguna sedang memakai hub/kartu lain: jangan ditimpa, awan muncul lagi di tik berikutnya.
          if (ctx.override !== null && !(ctx.override === "dev" && ctx.meetingCard)) return;
          // Kartu rapat lama (mis. dibuat sebelum posisi dock selesai dipulihkan) dilepas
          // supaya notch kembali ke bentuk bubble dan timer tampil sebagai awan.
          if (ctx.override === "dev" && ctx.meetingCard) {
              window.clearTimeout(ctx.hideTimer);
              ctx.override = null;
              ctx.meetingCard = false;
              ctx.render();
          }
      }
      const sub = `${elapsedLabel()} · ${ctx.priv?.cam && ctx.priv?.mic ? "Kamera & mikrofon aktif" : ctx.priv?.cam ? "Kamera aktif" : "Mikrofon aktif"}`;
      meetingUsesBubble = ctx.momentBubble?.show("Mode rapat", sub, 1800) === true;
      if (!meetingUsesBubble) {
          if (ctx.meetingCard) ctx.$("dev-sub").textContent = sub;
          else {
              ctx.showTextCard?.("Mode rapat", sub, 0);
              if (!ctx.showTextCard) {
                  ctx.$("dev-name").textContent = "Mode rapat";
                  ctx.$("dev-sub").textContent = sub;
                  ctx.expand("dev", 0);
              }
              ctx.meetingCard = true;
          }
      }
  };

  const startMeeting = () => {
      if (ctx.meetingMode) return;
      ctx.meetingMode = true;
      ctx.setMomentsMeeting?.(true);
      // Tutup sapaan yang mungkin sedang tampil sebelum timer rapat mengambil alih.
      ctx.notch.classList.remove("greeting-active");
      meetingStartedAt = Date.now();
      ctx.momentBubble?.hide();
      ctx.meetingCard = false;
      paintMeetingTimer();
      meetingTimer = window.setInterval(paintMeetingTimer, 1000);
  };

  const finishMeeting = () => {
      if (!ctx.meetingMode) return;
      const durationSeconds = Math.max(0, Math.floor((Date.now() - meetingStartedAt) / 1000));
      const durationMinutes = Math.floor(durationSeconds / 60);
      const remainingSeconds = durationSeconds % 60;
      const durationLabel = durationMinutes === 0
          ? `${durationSeconds} detik`
          : remainingSeconds === 0
              ? `${durationMinutes} menit`
              : `${durationMinutes} menit ${remainingSeconds} detik`;
      ctx.meetingMode = false;
      ctx.setMomentsMeeting?.(false);
      if (meetingTimer !== undefined) window.clearInterval(meetingTimer);
      meetingTimer = undefined;
      ctx.momentBubble?.hide();
      if (ctx.meetingCard && ctx.override === "dev") {
          ctx.override = null;
          ctx.render();
      }
      ctx.meetingCard = false;
      const text = `Rapat selesai, ${durationLabel}`;
      // Gunakan awan bila tersedia; fallback kartu greeting mengikuti perilaku lama.
      ctx.greetingApi?.say(text, "Terima kasih sudah fokus. 🎉", 6500);
  };

  ctx.listen<string[]>("clipboard", (e) => ctx.paintClip(e.payload));
  document.getElementById("clip-clear")?.addEventListener("click", (e) => {
      e.stopPropagation();
      ctx.invoke<string[]>("clip_clear").then(ctx.paintClip).catch(() => { });
      ctx.flash("surprised", 900);
  });
  ctx.refreshClip();
  ctx.priv = null;
  ctx.listen<Privacy>("privacy", (e) => {
      const p = e.payload;
      const prev = ctx.priv;
      ctx.priv = p; // selalu disimpan, supaya langsung tampil saat fitur dinyalakan dari pengaturan
      ctx.paintPrivacy();

      // Mode rapat otomatis: aktif selama setidaknya satu indikator privasi menyala.
      const inMeeting = !!(p.mic || p.cam);
      if (inMeeting) startMeeting();
      else if (ctx.meetingMode) finishMeeting();

      if (!ctx.privacyOn || !prev || ctx.meetingMode)
          return; // event pertama hanya menandai keadaan awal; kartu perangkat ditahan selama rapat
      if (p.cam && !prev.cam) {
          ctx.showDevice("camera", "Kamera aktif", true, p.cam_app ? `Dipakai ${p.cam_app}` : "Sedang digunakan");
          ctx.flash("suspicious", 2500);
      }
      else if (p.mic && !prev.mic) {
          ctx.showDevice("mic", "Mikrofon aktif", true, p.mic_app ? `Dipakai ${p.mic_app}` : "Sedang digunakan");
          ctx.flash("suspicious", 2500);
      }
  });
}
