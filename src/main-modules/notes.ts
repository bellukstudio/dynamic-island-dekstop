// @ts-nocheck
// Catatan dan detail catatan.
// ---------- catatan (notes) — tersimpan permanen, terpisah dari config ----------
type Note = {
    id: string;
    text: string;
    updated_at: number;
};

export function register(ctx: Record<string, any>): void {
  ctx.fmtNoteDate = function fmtNoteDate(ms: number): string {
      const d = new Date(ms);
      const now = new Date();
      const diffMs = now.getTime() - d.getTime();
      const diffMin = Math.floor(diffMs / 60000);
      if (diffMin < 1)
          return "Baru saja";
      if (diffMin < 60)
          return `${diffMin} mnt lalu`;
      const diffH = Math.floor(diffMin / 60);
      if (diffH < 24)
          return `${diffH} jam lalu`;
      const diffD = Math.floor(diffH / 24);
      if (diffD < 7)
          return `${diffD} hari lalu`;
      return d.toLocaleDateString("id-ID", { day: "numeric", month: "short" });
  };
  ctx.openNoteDetail = function openNoteDetail(n: Note) {
      ctx.invoke("open_note_detail", { id: n.id }).catch(console.error);
  };
  ctx.paintNotes = function paintNotes(items: Note[]) {
      const list = document.getElementById("nt-list");
      if (!list)
          return;
      ctx.$("nt-count").textContent = items.length ? String(items.length) : "";
      list.replaceChildren();
      if (items.length === 0) {
          const empty = document.createElement("div");
          empty.className = "nt-empty";
          empty.textContent = "Belum ada catatan";
          list.appendChild(empty);
          return;
      }
      items.forEach((n) => {
          const card = document.createElement("button");
          card.className = "nt-item";
          card.title = "Klik untuk buka detail";
          // Preview teks (2 baris pertama)
          const preview = document.createElement("span");
          preview.className = "nt-item-preview";
          const lines = n.text.split("\n");
          preview.textContent = lines.slice(0, 2).join(" ").slice(0, 120) + (n.text.length > 120 ? "…" : "");
          // Meta baris bawah
          const meta = document.createElement("span");
          meta.className = "nt-item-meta";
          const dateSpan = document.createElement("span");
          dateSpan.className = "nt-item-date";
          dateSpan.textContent = ctx.fmtNoteDate(n.updated_at);
          const lenSpan = document.createElement("span");
          lenSpan.className = "nt-item-len";
          lenSpan.textContent = `${n.text.length} kar`;
          meta.appendChild(dateSpan);
          meta.appendChild(lenSpan);
          // Tombol hapus
          const delBtn = document.createElement("span");
          delBtn.className = "nt-item-del";
          delBtn.textContent = "×";
          delBtn.title = "Hapus";
          card.appendChild(preview);
          card.appendChild(meta);
          card.appendChild(delBtn);
          card.addEventListener("click", (e) => {
              e.stopPropagation();
              if ((e.target as HTMLElement).closest(".nt-item-del")) {
                  ctx.invoke<Note[]>("notes_remove", { id: n.id }).then(ctx.paintNotes).catch(() => { });
              }
              else {
                  ctx.openNoteDetail(n);
              }
          });
          list.appendChild(card);
      });
  };
  ctx.setNotesExpanded = function setNotesExpanded(expanded: boolean) {
      ctx.notch.classList.toggle("notes-expanded", expanded);
      const button = document.getElementById("nt-expand");
      if (button) {
          const label = expanded ? "Sembunyikan daftar catatan" : "Tampilkan daftar catatan";
          button.setAttribute("aria-expanded", String(expanded));
          button.setAttribute("aria-label", label);
          button.title = label;
      }
      ctx.syncHubDynamicHeight();
  };
  ctx.refreshNotes = function refreshNotes() {
      ctx.invoke<Note[]>("notes_list").then(ctx.paintNotes).catch(() => { });
  };
  ctx.addNote = function addNote() {
      const input = ctx.$("nt-input") as HTMLTextAreaElement;
      const text = input.value.trim();
      if (!text)
          return;
      ctx.invoke<Note[]>("notes_add", { text })
          .then((items) => {
          ctx.paintNotes(items);
          input.value = "";
          ctx.flash("love", 900);
      })
          .catch(() => { });
  };
}

export function init(ctx: Record<string, any>): void {
  document.getElementById("nt-expand")?.addEventListener("click", (e) => {
      e.preventDefault();
      e.stopPropagation();
      ctx.setNotesExpanded(!ctx.notch.classList.contains("notes-expanded"));
  });
  // Dengarkan perubahan dari jendela detail
  ctx.listen("notes-changed", () => ctx.refreshNotes()).catch(() => { });
  document.getElementById("nt-clear")?.addEventListener("click", (e) => {
      e.stopPropagation();
      ctx.invoke<Note[]>("notes_clear").then(ctx.paintNotes).catch(() => { });
      ctx.flash("surprised", 900);
  });
  document.getElementById("nt-add-btn")?.addEventListener("click", (e) => { e.stopPropagation(); ctx.addNote(); });
  document.getElementById("nt-input")?.addEventListener("keydown", (e) => {
      e.stopPropagation();
      if ((e as KeyboardEvent).key === "Enter" && !(e as KeyboardEvent).shiftKey) {
          e.preventDefault();
          ctx.addNote();
      }
  });
  ctx.refreshNotes();
}
