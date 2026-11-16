// Mulut notch sesuai mood, saat notch berbentuk bubble di SAMPING atau BAWAH layar
// (data-view "edgeIdle" / "bottomIdle" — bentuk yang sama dipakai bola mulut adzan).
// Beda dengan adhan-face.ts: mulut ini bukan animasi melantun, tapi ekspresi diam yang
// berubah bentuk & warna mengikuti data-mood pada .eyes (normal/happy/sleepy/surprised/
// angry/love/suspicious/sad), supaya wajah notch tetap "hidup" walau sedang idle.
// Disembunyikan otomatis saat adzan berbunyi (adhan-mouth yang tampil, lihat adhan-face.ts).
// CSS disuntikkan dari sini, sama seperti adhan-face.ts, supaya tidak bergantung pada
// file CSS mana yang dimuat index.html. Mulut BUKAN anak dari .eyes (alasan sama seperti
// adhan-mouth: CSS mata memakai .eye:last-child di eyes.css/nudge.css).

const CSS = `
/* Diposisikan dari TEPI BAWAH layer (bukan dari tengah ke bawah) supaya selalu jadi
   "dagu" yang jelas di bawah mata, berapa pun tinggi mata yang sedang aktif. */
.mood-mouth {
  position: absolute; left: 50%; bottom: 10px;
  width: 10px; height: 3px; border-radius: 2px;
  background: rgba(120, 220, 255, 0.85);
  box-shadow: 0 0 6px rgba(120, 220, 255, 0.55);
  opacity: 0; transform: translateX(-50%);
  pointer-events: none; z-index: 1;
  transition:
    opacity 0.25s ease,
    width 0.25s cubic-bezier(0.34, 1.56, 0.5, 1),
    height 0.25s cubic-bezier(0.34, 1.56, 0.5, 1),
    border-radius 0.25s ease,
    transform 0.25s cubic-bezier(0.34, 1.56, 0.5, 1),
    bottom 0.25s ease,
    background 0.3s, box-shadow 0.3s;
}
#notch:not(.adhan-reciting)[data-view="edgeIdle"] .mood-mouth,
#notch:not(.adhan-reciting)[data-view="bottomIdle"] .mood-mouth {
  opacity: 1;
}
/* bubble di samping layar (edgeIdle) jauh lebih tinggi (80px) daripada di bawah layar
   (bottomIdle, 44px) -> jarak ke tepi bawah perlu disetel per bentuk supaya tetap pas
   jadi "dagu", tidak mengambang di tengah atau malah kepotong lengkungan bawah pil.
   Nilai dibuat sekecil mungkin (mepet tepi bawah) supaya tidak pernah tertutup mata:
   mata termuncul (mood "love", tinggi 19px) baru turun sampai ~31.5px (bottomIdle) /
   ~49.5px (edgeIdle) dari atas layer — mulut dipastikan mulai di bawah itu. */
#notch[data-view="edgeIdle"] .mood-mouth { bottom: 10px; }
#notch[data-view="bottomIdle"] .mood-mouth { bottom: 3px; }

/* senyum: sudut bawah saja dibulatkan -> melengkung seperti "u" (trik sama dengan mata happy) */
.eyes[data-mood="happy"] ~ .mood-mouth {
  width: 14px; height: 7px; border-radius: 0 0 7px 7px;
  background: rgba(130, 255, 190, 0.9);
  box-shadow: 0 0 7px rgba(130, 255, 190, 0.6);
}

/* mulut kecil nyaris tertutup, agak redup seperti mata sleepy */
.eyes[data-mood="sleepy"] ~ .mood-mouth {
  width: 7px; height: 2px; border-radius: 1px;
  background: rgba(150, 160, 200, 0.5);
  box-shadow: none;
}

/* bentuk "o" kecil seperti terkejut */
.eyes[data-mood="surprised"] ~ .mood-mouth {
  width: 7px; height: 7px; border-radius: 50%;
  background: rgba(255, 240, 140, 0.9);
  box-shadow: 0 0 7px rgba(255, 240, 140, 0.6);
}

/* cemberut: sudut atas saja dibulatkan -> melengkung turun */
.eyes[data-mood="angry"] ~ .mood-mouth {
  width: 12px; height: 6px; border-radius: 6px 6px 0 0;
  background: rgba(255, 90, 90, 0.9);
  box-shadow: 0 0 7px rgba(255, 90, 90, 0.6);
  animation: mood-mouth-shake 0.3s ease-in-out infinite;
}

/* senyum lebar + berdenyut pelan, senada dengan mata love yang berdenyut */
.eyes[data-mood="love"] ~ .mood-mouth {
  width: 15px; height: 7px; border-radius: 0 0 7px 7px;
  background: rgba(255, 105, 180, 0.9);
  box-shadow: 0 0 8px rgba(255, 105, 180, 0.65);
  animation: mood-mouth-pulse 0.8s ease-in-out infinite;
}

/* senyum sebelah: curiga, mulut miring ke satu sisi (sama asimetrisnya dengan mata suspicious) */
.eyes[data-mood="suspicious"] ~ .mood-mouth {
  width: 10px; height: 5px; border-radius: 0 5px 5px 0;
  transform: translateX(calc(-50% + 2px)) rotate(-8deg);
  background: rgba(255, 190, 90, 0.9);
  box-shadow: 0 0 7px rgba(255, 190, 90, 0.55);
}

/* cemberut pelan, agak redup dan sedikit miring, senada dengan mata sad */
.eyes[data-mood="sad"] ~ .mood-mouth {
  width: 11px; height: 6px; border-radius: 6px 6px 0 0;
  transform: translateX(-50%) rotate(-3deg);
  background: rgba(130, 160, 255, 0.85);
  box-shadow: 0 0 6px rgba(130, 160, 255, 0.5);
}

@keyframes mood-mouth-pulse {
  0%, 100% { transform: translateX(-50%) scale(1); }
  50% { transform: translateX(-50%) scale(1.18); }
}
@keyframes mood-mouth-shake {
  0%, 100% { transform: translateX(-50%); }
  50% { transform: translateX(calc(-50% + 1px)); }
}
@media (prefers-reduced-motion: reduce) {
  .mood-mouth { transition: opacity 0.25s ease; }
  .eyes[data-mood="angry"] ~ .mood-mouth,
  .eyes[data-mood="love"] ~ .mood-mouth { animation: none; }
}
`;

/** Pasang mulut mood di layer mata (sibling dari .eyes), lalu biarkan CSS di atas
 *  bereaksi sendiri terhadap data-view (#notch) & data-mood (.eyes) yang sudah
 *  diset oleh kode lain — tidak perlu observer/listener tambahan. */
export function initMoodMouth(eyes: HTMLElement | null): void {
  const style = document.createElement("style");
  style.dataset.moodMouth = "";
  style.textContent = CSS;
  document.head.appendChild(style);

  const mouth = document.createElement("i");
  mouth.className = "mood-mouth";
  mouth.setAttribute("aria-hidden", "true");
  (eyes?.closest(".l-idle") ?? eyes?.parentElement)?.appendChild(mouth);
}
