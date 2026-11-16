// @ts-nocheck
// Shelf file.
import { startDrag } from "@crabnebula/tauri-plugin-drag";

// Ikon seret: PNG 1x1 transparan (plugin mewajibkan gambar; sistem tetap memakai ikon file bawaan di banyak OS).
const DRAG_ICON = Uint8Array.from(
  atob("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg=="),
  (c) => c.charCodeAt(0),
);
const DRAG_THRESHOLD_PX = 6;
// ---------- shelf file ----------
type ShelfItem = {
    path: string;
    name: string;
    dir: boolean;
};

export function register(ctx: Record<string, any>): void {
  ctx.paintShelf = function paintShelf(items: ShelfItem[]) {
      const list = document.getElementById("sh-list");
      if (!list)
          return; // halaman shelf dimatikan di config
      ctx.$("sh-count").textContent = items.length ? `Shelf · ${items.length}` : "Shelf";
      list.replaceChildren();
      if (items.length === 0) {
          const empty = document.createElement("div");
          empty.className = "sh-empty";
          empty.textContent = "Seret file ke notch";
          list.appendChild(empty);
          return;
      }
      items.forEach((it) => {
          const b = document.createElement("button"); // button: tidak memicu geser halaman
          b.className = "sh-item";
          b.title = `${it.path}\nKlik: buka · Seret: salin keluar · Klik kanan: tampilkan di folder`;
          const ic = document.createElement("span");
          ic.className = "ic";
          ic.textContent = ctx.iconFor(it);
          const nm = document.createElement("span");
          nm.className = "nm";
          nm.textContent = it.name;
          const x = document.createElement("span");
          x.className = "x";
          x.textContent = "×";
          b.append(ic, nm, x);
          // Seret keluar ke File Explorer / aplikasi lain. Klik biasa tetap membuka file, jadi drag baru
          // dimulai setelah pointer bergeser melewati ambang; klik sesudah drag diabaikan.
          let downX = 0, downY = 0, armed = false, dragged = false;
          b.addEventListener("pointerdown", (e) => {
              if (e.button !== 0 || (e.target as HTMLElement).closest(".x"))
                  return;
              downX = e.clientX;
              downY = e.clientY;
              armed = true;
              dragged = false;
          });
          b.addEventListener("pointermove", (e) => {
              if (!armed || dragged)
                  return;
              if (Math.hypot(e.clientX - downX, e.clientY - downY) < DRAG_THRESHOLD_PX)
                  return;
              armed = false;
              dragged = true;
              void ctx.invoke("set_drag_out", { active: true }); // jaga jendela notch tetap menerima event selama drag
              startDrag({ item: [it.path], icon: DRAG_ICON })
                  .catch((err) => console.error("shelf: drag keluar gagal", err))
                  .finally(() => {
                  void ctx.invoke("set_drag_out", { active: false });
                  window.setTimeout(() => { dragged = false; }, 0);
              });
          });
          b.addEventListener("pointerup", () => { armed = false; });
          b.addEventListener("pointercancel", () => { armed = false; });
          b.addEventListener("click", (e) => {
              e.stopPropagation();
              if (dragged)
                  return;
              if ((e.target as HTMLElement).closest(".x")) {
                  ctx.invoke<ShelfItem[]>("shelf_remove", { path: it.path }).then(ctx.paintShelf).catch(() => { });
              }
              else {
                  ctx.invoke("shelf_open", { path: it.path });
                  ctx.flash("happy", 1000);
              }
          });
          b.addEventListener("contextmenu", (e) => {
              e.preventDefault();
              e.stopPropagation();
              ctx.invoke("shelf_reveal", { path: it.path });
          });
          list.appendChild(b);
      });
  };
  ctx.refreshShelf = function refreshShelf() {
      ctx.invoke<ShelfItem[]>("shelf_list").then(ctx.paintShelf).catch(() => { });
  };
}

export function init(ctx: Record<string, any>): void {
  ctx.EXT_ICON = {
      png: "🖼️", jpg: "🖼️", jpeg: "🖼️", gif: "🖼️", webp: "🖼️", svg: "🖼️", bmp: "🖼️",
      mp3: "🎵", flac: "🎵", wav: "🎵", ogg: "🎵", m4a: "🎵",
      mp4: "🎬", mkv: "🎬", mov: "🎬", avi: "🎬", webm: "🎬",
      pdf: "📕", doc: "📝", docx: "📝", txt: "📝", md: "📝", odt: "📝",
      xls: "📊", xlsx: "📊", csv: "📊", ppt: "📽️", pptx: "📽️",
      zip: "🗜️", rar: "🗜️", "7z": "🗜️", gz: "🗜️", tar: "🗜️",
      js: "📜", ts: "📜", py: "📜", rs: "📜", json: "📜", html: "📜", css: "📜",
      exe: "⚙️", msi: "⚙️", deb: "⚙️", appimage: "⚙️",
  };
  ctx.iconFor = (it: ShelfItem) => it.dir ? "📁" : ctx.EXT_ICON[(it.name.split(".").pop() ?? "").toLowerCase()] ?? "📄";
  document.getElementById("sh-clear")?.addEventListener("click", (e) => {
      e.stopPropagation();
      ctx.invoke<ShelfItem[]>("shelf_clear").then(ctx.paintShelf).catch(() => { });
      ctx.flash("surprised", 900);
  });
  // file diseret dari aplikasi lain ke notch: buka halaman shelf saat masuk, simpan saat dilepas
  if (ctx.names.includes("shelf")) {
      ctx.getCurrentWebview()
          .onDragDropEvent((ev) => {
          const p = ev.payload;
          if (p.type === "enter") {
              ctx.notch.classList.add("dropping");
              ctx.hubByHover = false;
              ctx.openHub(ctx.pageOf("shelf"), 8000);
              ctx.flash("surprised", 1200);
          }
          else if (p.type === "leave") {
              ctx.notch.classList.remove("dropping");
              ctx.expand("hub", 1200);
          }
          else if (p.type === "drop") {
              ctx.notch.classList.remove("dropping");
              if (p.paths.length === 0)
                  return;
              ctx.invoke<ShelfItem[]>("shelf_add", { paths: p.paths })
                  .then((items) => {
                  ctx.paintShelf(items);
                  ctx.openHub(ctx.pageOf("shelf"), 3500);
                  ctx.flash("love", 1500);
              })
                  .catch(() => { });
          }
      })
          .catch(() => { });
  }
  ctx.refreshShelf();
}