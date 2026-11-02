// Bar navigasi ikon di atas hub (pengganti titik-titik di kanan).
let clockTimer: number | undefined;
const NAV: Record<string, [string, string]> = {
    media: ["Media", `<path d="M9 18V5l12-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="16" r="3"/>`],
    clock: ["Jam", `<circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/>`],
    weather: ["Cuaca", `<path d="M17.5 19a4.5 4.5 0 1 0-1-8.9A6 6 0 0 0 5 12a4 4 0 0 0 1 7z"/>`],
    battery: ["Baterai", `<rect x="2" y="7" width="18" height="10" rx="2"/><path d="M22 11v2"/>`],
    timer: ["Timer", `<circle cx="12" cy="13" r="8"/><path d="M12 9v4l2 2M9 2h6"/>`],
    quick: ["Quick", `<rect x="3" y="3" width="7" height="7" rx="1.5"/><rect x="14" y="3" width="7" height="7" rx="1.5"/><rect x="3" y="14" width="7" height="7" rx="1.5"/><rect x="14" y="14" width="7" height="7" rx="1.5"/>`],
    tools: ["Alat", `<path d="M14.7 6.3a4 4 0 0 0-5.4 5.4L3 18l3 3 6.3-6.3a4 4 0 0 0 5.4-5.4l-2.5 2.5-2.4-.6-.6-2.4z"/>`],
    stats: ["Sistem", `<path d="M4 18a8 8 0 1 1 16 0M12 18l4-5"/>`],
    reminder: ["Pengingat", `<path d="M6 8a6 6 0 0 1 12 0c0 7 3 9 3 9H3s3-2 3-9M10.3 21a1.94 1.94 0 0 0 3.4 0"/>`],
    notes: ["Catatan", `<path d="M5 3h14v18H5zM9 8h6M9 12h6M9 16h3"/>`],
    clipboard: ["Clipboard", `<rect x="6" y="4" width="12" height="17" rx="2"/><path d="M9 4h6v3H9z"/>`],
    shelf: ["Shelf", `<path d="M3 8l2-4h14l2 4M3 8v12h18V8M3 8h18M9 12h6"/>`],
    currency: ["Currency", `<path d="M12 3v18M17 7.5c0-1.4-2.2-2.5-5-2.5S7 6.1 7 7.5 9.2 10 12 10s5 1.1 5 2.5S14.8 15 12 15s-5-1.1-5-2.5"/>`],
    prayer: [
        "Jadwal Shalat",
        `<path d="M3 21h18M5 21V11l7-6 7 6v10M9 21v-6h6v6"/> 
        <path d="M12 2v3M10 4l2-2 2 2"/>`,
    ],
};

/** Membangun nav. Mengembalikan fungsi untuk menandai halaman aktif. */
export function initHubNav(names: string[], onPick: (i: number) => void) {
    const list = document.getElementById("hub-nav");
    const dateEl = document.getElementById("nv-date");
    const timeEl = document.getElementById("nv-time");
    if (!list) return (_page: number) => { };

    // Daftar dapat dibangun ulang saat checkbox fitur di Settings berubah.
    list.setAttribute("role", "group");
    list.setAttribute("aria-label", "Navigasi halaman Notch");
    list.replaceChildren();
    if (clockTimer !== undefined) window.clearInterval(clockTimer);

    const btns: HTMLButtonElement[] = names.map((n, i) => {
        const [label, svg] = NAV[n] ?? [n, `<circle cx="12" cy="12" r="3"/>`];
        const b = document.createElement("button");
        b.className = "nv";
        b.title = label;
        b.setAttribute("type", "button");
        b.setAttribute("aria-label", label);
        b.setAttribute("aria-current", "false");
        // Ikon berasal dari daftar statis NAV; label dirender sebagai teks agar aman dari HTML.
        const icon = document.createElementNS("http://www.w3.org/2000/svg", "svg");
        icon.setAttribute("viewBox", "0 0 24 24");
        icon.setAttribute("aria-hidden", "true");
        icon.innerHTML = svg;
        const caption = document.createElement("span");
        caption.textContent = label;
        b.append(icon, caption);
        b.addEventListener("click", (e) => {
            e.preventDefault();
            e.stopPropagation();
            onPick(i);
        });
        list.appendChild(b);
        return b;
    });

    const tick = () => {
        const d = new Date();
        if (dateEl) dateEl.textContent = d.toLocaleDateString("id-ID", { weekday: "short", day: "numeric", month: "short" });
        if (timeEl) timeEl.textContent =
            `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
    };
    tick();
    clockTimer = window.setInterval(tick, 1000);

    return (page: number) => {
        btns.forEach((b, n) => {
            const active = n === page;
            b.classList.toggle("active", active);
            b.setAttribute("aria-current", active ? "page" : "false");
        });
        // Gulir HANYA daftar ikon. scrollIntoView ikut menggulir #notch (overflow:hidden tetap bisa
        // digulir lewat kode), sehingga seluruh isi pil naik ke atas dan wajah jam/mata tak terlihat.
        const b = btns[page];
        if (!b) return;
        const lr = list.getBoundingClientRect(), br = b.getBoundingClientRect();
        // Jangan gunakan scrollIntoView: pada WebView itu dapat menggulir parent #notch.
        // Scroll hanya nv-list secara horizontal agar konten halaman tidak bergeser vertikal.
        if (br.left < lr.left) list.scrollLeft -= (lr.left - br.left) + 6;
        else if (br.right > lr.right) list.scrollLeft += (br.right - lr.right) + 6;
    };
}