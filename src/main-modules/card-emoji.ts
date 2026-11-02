// Mengganti ikon kartu teks (#dev-icon) dengan emoji yang bergoyang bergantian.
// Pola yang sama dengan nudge.ts (gaya `data-kind="nudge"` ada di nudge.css), dipakai bersama oleh
// fitur istirahat mata dan alert harga supaya tidak menyalin kode DOM yang sama dua kali.

/** Pasang emoji (maks. 5) pada kartu teks yang sedang/akan tampil, lalu mulai ulang animasi masuknya. */
export function setCardEmoji(emoji: string[]): void {
  const icon = document.getElementById("dev-icon");
  if (icon) {
    const parts = emoji.filter((e) => e.trim()).slice(0, 5);
    icon.dataset.kind = "nudge";
    icon.dataset.count = String(parts.length);
    icon.replaceChildren(
      ...parts.map((ch, i) => {
        const span = document.createElement("span");
        span.className = "emo";
        span.style.setProperty("--i", String(i)); // jeda animasi per emoji
        span.textContent = ch;
        return span;
      })
    );
  }
  const layer = document.querySelector<HTMLElement>(".l-dev");
  if (layer) {
    layer.classList.remove("nudge-in");
    void layer.offsetWidth;
    layer.classList.add("nudge-in");
  }
}
