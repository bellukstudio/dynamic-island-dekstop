// Animasi mulut saat adzan berbunyi.
//  - Notch di kiri/bawah (awan adzan): mulut kecil muncul di bawah mata (layer .l-idle).
//  - Notch di atas/melayang/kanan (kartu prayerAlert): ikon kartu berubah menjadi wajah kecil yang melantun.
// CSS disuntikkan dari sini supaya tidak bergantung pada file CSS mana yang dimuat index.html.
// Mulut sengaja BUKAN anak dari .eyes: CSS mata memakai .eye:last-child (eyes.css, nudge.css).

const CSS = `
/* --- mulut di layer mata (notch kiri/bawah) --- */
.adhan-mouth {
  position: absolute; left: 50%; top: 50%;
  width: 11px; height: 8px; margin: 7px 0 0 -5.5px;
  border-radius: 50%;
  background: rgba(var(--fg-rgb, 255, 255, 255), 0.9);
  box-shadow: 0 0 7px rgba(var(--accent, 120, 220, 255), 0.6);
  opacity: 0; transform: scale(0.72, 0.22); transform-origin: 50% 40%;
  pointer-events: none; transition: opacity 0.2s ease; z-index: 2;
}
#notch.adhan-reciting[data-view="edgeIdle"] .eyes,
#notch.adhan-reciting[data-view="bottomIdle"] .eyes {
  margin-top: -12px;
  transition: transform 0.12s ease-out, margin-top 0.25s ease;
}
#notch.adhan-reciting[data-view="edgeIdle"] .adhan-mouth,
#notch.adhan-reciting[data-view="bottomIdle"] .adhan-mouth {
  opacity: 1; animation: adhan-mouth-talk 1.15s ease-in-out infinite;
}

/* --- wajah di ikon kartu adzan (notch atas/melayang/kanan) --- */
.adhan-card-face { display: none; flex-direction: column; align-items: center; gap: 5px; }
.adhan-card-face .af-eyes { display: flex; gap: 8px; }
.adhan-card-face .af-eye {
  display: block; width: 4px; height: 8px; border-radius: 3px;
  background: rgb(var(--accent, 120, 220, 255));
  box-shadow: 0 0 6px rgba(var(--accent, 120, 220, 255), 0.7);
  animation: adhan-eye-blink 3.6s ease-in-out infinite;
}
.adhan-card-face .af-mouth {
  display: block; width: 12px; height: 8px; border-radius: 50%;
  background: rgb(var(--accent, 120, 220, 255));
  box-shadow: 0 0 6px rgba(var(--accent, 120, 220, 255), 0.55);
  transform: scale(0.72, 0.22); transform-origin: 50% 40%;
}
#notch.adhan-reciting[data-view="prayerAlert"] .pr-alert-icon > svg { display: none; }
#notch.adhan-reciting[data-view="prayerAlert"] .adhan-card-face { display: flex; }
#notch.adhan-reciting[data-view="prayerAlert"] .adhan-card-face .af-mouth {
  animation: adhan-mouth-talk 1.15s ease-in-out infinite;
}

/* ritme tidak rata (seperti suku kata) supaya terasa melantun, bukan berkedip */
@keyframes adhan-mouth-talk {
  0%, 100% { transform: scale(0.72, 0.22); }
  14% { transform: scale(1, 0.95); }
  27% { transform: scale(0.82, 0.38); }
  41% { transform: scale(1.08, 1); }
  54% { transform: scale(0.74, 0.2); }
  68% { transform: scale(0.96, 0.72); }
  82% { transform: scale(0.84, 0.44); }
}
@keyframes adhan-eye-blink {
  0%, 92%, 100% { transform: scaleY(1); }
  96% { transform: scaleY(0.1); }
}
@media (prefers-reduced-motion: reduce) {
  #notch.adhan-reciting[data-view="edgeIdle"] .adhan-mouth,
  #notch.adhan-reciting[data-view="bottomIdle"] .adhan-mouth { animation: none; transform: scale(0.9, 0.6); }
  #notch.adhan-reciting[data-view="prayerAlert"] .adhan-card-face .af-mouth { animation: none; transform: scale(0.9, 0.6); }
  .adhan-card-face .af-eye { animation: none; }
}
`;

export type AdhanFace = { start: () => void; stop: () => void };

function el<K extends keyof HTMLElementTagNameMap>(tag: K, cls: string): HTMLElementTagNameMap[K] {
  const e = document.createElement(tag);
  e.className = cls;
  return e;
}

export function initAdhanFace(notch: HTMLElement, eyes: HTMLElement | null): AdhanFace {
  const style = document.createElement("style");
  style.dataset.adhanFace = "";
  style.textContent = CSS;
  document.head.appendChild(style);

  // mulut untuk layer mata
  const mouth = el("i", "adhan-mouth");
  mouth.setAttribute("aria-hidden", "true");
  (eyes?.closest(".l-idle") ?? eyes?.parentElement)?.appendChild(mouth);

  // wajah untuk ikon kartu adzan (elemen kartu sudah ada di HTML; wajah dibuat lewat JS)
  const icon = document.querySelector<HTMLElement>(".pr-alert-icon");
  if (icon) {
    const face = el("span", "adhan-card-face");
    face.setAttribute("aria-hidden", "true");
    const row = el("span", "af-eyes");
    row.append(el("i", "af-eye"), el("i", "af-eye"));
    face.append(row, el("i", "af-mouth"));
    icon.appendChild(face);
  } else {
    console.warn("[AdhanFace] .pr-alert-icon tidak ditemukan; wajah di kartu adzan dilewati.");
  }

  return {
    start: () => notch.classList.add("adhan-reciting"),
    stop: () => notch.classList.remove("adhan-reciting"),
  };
}
