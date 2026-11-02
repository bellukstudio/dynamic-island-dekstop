import { emit, listen } from "@tauri-apps/api/event";

// Gerakan acak saat notch menempel di dasar layar (bentuk kubah "bottomIdle"/"bottomMedia").
//
// Mesin fisika kecil (pegas + gravitasi) yang berjalan tiap frame supaya badan notch terasa seperti
// cairan/jelly: tidak ada perubahan bentuk yang meloncat, semuanya diberi "dorongan" (impuls) lalu
// dibiarkan bergoyang kembali oleh pegas yang redamannya rendah.
//
// Berkeliling: yang bergerak murni translate CSS di dalam jendela (compositor, subpiksel mulus).
// Jendela native TIDAK digeser selama animasi karena memindahkan jendela OS berkali-kali per detik
// tidak pernah sinkron dengan repaint webview dan itulah sumber getaran. Jangkauan jalan dibatasi
// oleh ruang kosong di dalam jendela, dan setiap gerakan berakhir dengan kembali ke posisi asal.
//
// Gaya dasar di bottom-play.css (membaca --bp-x/--bp-y/--bp-r/--bp-sx/--bp-sy).
// Saklar di Pengaturan > Perilaku (localStorage "notch.bottomPlay").

export type BottomPlayHooks = {
    shell: HTMLElement;
    notch: HTMLElement;
    busy: () => boolean;                    // ada hub/kartu/seretan/hover: jangan bergerak
    playing: () => boolean;                 // musik diputar: lebih sering menari
    calm: () => boolean;                    // mengantuk / baterai rendah: gerakan pelan saja
    mood: (m: string, ms: number) => void;
    getCx: () => number;                    // posisi sumbu X ketika menempel di bawah
    setCx: (cx: number) => Promise<unknown>;
    range: () => [number, number];          // rentang X saat menempel di bawah
    dock?: () => "bottom" | "left" | "right" | null;
    getTy?: () => number;                   // koordinat atas jendela saat menempel di samping
    setTy?: (ty: number) => Promise<unknown>;
    verticalRange?: () => [number, number];  // rentang Y saat menempel di samping
    hit: (dx?: number, dy?: number) => void; // perbarui area klik (dx/dy = geseran CSS notch dari posisi asalnya)
};

const KEY = "notch.bottomPlay";
export function bottomPlayEnabled(): boolean {
    try { return localStorage.getItem(KEY) !== "0"; } catch { return true; } // bawaan: aktif
}

const rand = (a: number, b: number) => a + Math.random() * (b - a);
const clamp = (v: number, a: number, b: number) => Math.min(Math.max(v, a), b);

/** Pegas: percepatan = -k(x - t) - c·v. Redaman kecil = bergoyang seperti jelly. */
class Spring {
    v = 0;
    constructor(public k: number, public c: number, public x = 0, public t = 0) { }
    tune(k: number, c: number) { this.k = k; this.c = c; }
    step(dt: number) { this.v += (-this.k * (this.x - this.t) - this.c * this.v) * dt; this.x += this.v * dt; }
    get rest() { return Math.abs(this.x - this.t) < 0.003 && Math.abs(this.v) < 0.03; }
}
/** Dorong pegas (bukan melompatkan nilainya) supaya bentuk berubah mulus. */
const poke = (s: Spring, amount: number) => { s.v += amount * 18; };

const G = 2600;                 // gravitasi (px/s²)
const ACC = 320;                // percepatan/perlambatan rendah agar gerak meluncur pelan (px/s²)
const CANCEL = Symbol("cancel");

// [nama, bobot]
// Pertahankan gerakan acak asli dari versi GitHub; liquid juga boleh terpilih secara acak.
// Selain itu, ada burst liquid terjadwal setiap 3 menit sebanyak 3 rangkaian.
const LIQUID_INTERVAL_MS = 3 * 60 * 1000;
const LIQUID_REPETITIONS = 3;
const NORMAL: [string, number][] = [["liquid", 6], ["ghost", 4], ["roam", 3], ["wiggle", 2], ["squish", 2], ["sway", 2], ["dance", 1], ["yawn", 1]];
const MUSIC: [string, number][] = [["liquid", 7], ["dance", 5], ["ghost", 3], ["roam", 3], ["wiggle", 2], ["squish", 1]];
const CALM: [string, number][] = [["yawn", 3], ["sway", 3], ["squish", 2], ["liquid", 1]];

const MOOD: Record<string, [string, number]> = {
    hop: ["happy", 1400], peek: ["surprised", 1200], wiggle: ["happy", 1200], roam: ["happy", 2600],
    squish: ["surprised", 900], sway: ["suspicious", 2200], dance: ["love", 1800], yawn: ["sleepy", 2400], liquid: ["happy", 1900], ghost: ["surprised", 2600],
};

function pick(list: [string, number][], avoid: string): string {
    const pool = list.filter(([n]) => n !== avoid);
    const total = pool.reduce((s, [, w]) => s + w, 0);
    let r = Math.random() * total;
    for (const [n, w] of pool) { if ((r -= w) <= 0) return n; }
    return pool[0][0];
}

export function initBottomPlay(h: BottomPlayHooks) {
    // Checklist Bottom Play adalah persetujuan eksplisit untuk animasi ini.
    // Karena itu prefers-reduced-motion OS tidak boleh membatalkan fitur yang diaktifkan pengguna.

    // ---------- keadaan fisika ----------
    const y = new Spring(0, 0);          // vertikal (negatif = ke atas)
    let grav = true;                     // true: gravitasi + lantai; false: mode pegas (dipakai "peek")
    const sq = new Spring(220, 9);       // penyok badan: + gepeng, - memanjang
    const wall = new Spring(220, 9);     // penyok samping saat menabrak tepi layar
    const rot = new Spring(100, 9);      // kemiringan utama (derajat)
    const px = new Spring(38, 11);       // posisi x visual notch di layar (mengikuti "wortel")
    const flow = new Spring(18, 8);      // rotasi organik yang berubah perlahan
    const float = new Spring(24, 9);     // gerak naik-turun halus seperti cairan
    const tall = new Spring(150, 13);    // memanjang ala hantu: + meninggi (sy naik, sx menyempit), - gepeng saat mendarat
    let rotT = 0;                        // kemiringan yang diminta gerakan (di luar kemiringan inersia)
    let nextRippleAt = 0;                // waktu impuls cairan berikutnya
    const resetTune = () => {
        sq.tune(220, 9); wall.tune(220, 9); rot.tune(100, 9); px.tune(38, 11);
        flow.tune(18, 8); float.tune(24, 9); flow.t = 0; float.t = 0; y.tune(0, 0);
        tall.tune(150, 13); tall.t = 0;
    };
    resetTune();

    let travel: null | { dest: number; carrot: number; cv: number; speed: number; edge: boolean; bumped: boolean; acc?: number } = null;
    let prevV = 0, axS = 0;              // percepatan halus (untuk memiringkan badan karena inersia)

    // Posisi asal notch (sumbu aktif) dan ruang geser CSS di dalam jendela pada kedua arah.
    let anchor0 = 0, slackLo = 0, slackHi = 0;
    let lastHit = 0;

    let active = "";
    let last = "";
    let timer = 0;
    let raf = 0;
    let lastTs = 0;
    let waits: { ok: () => boolean; res: () => void; rej: (e: unknown) => void }[] = [];

    let enabledAt = 0, enabledVal = true; // saklar dibaca tiap 300ms, bukan tiap frame
    let runtimeEnabledOverride: boolean | null = null;
    const enabled = () => {
        const n = performance.now();
        if (runtimeEnabledOverride !== null) return runtimeEnabledOverride;
        if (n - enabledAt > 300) { enabledAt = n; enabledVal = bottomPlayEnabled(); }
        return enabledVal;
    };
    type Dock = "bottom" | "left" | "right" | null;
    const currentDock = (): Dock => {
        if (h.dock) return h.dock();
        return h.notch.classList.contains("is-bottom") ? "bottom" : null;
    };
    // Kunci orientasi sumbu selama satu rangkaian gerak. Saat mouse masuk, view berubah
    // dari edgeIdle/edgeMedia menjadi hub dan currentDock() bisa menjadi null. Tanpa kunci ini,
    // koordinat Y (gerak vertikal di sisi) bisa salah dikirim ke setCx(), yang memang menempelkan
    // notch ke bawah layar.
    let motionDock: Dock = null;
    const axisDock = (): Dock => motionDock ?? currentDock();
    const isSideDock = (dock = currentDock()) => dock === "left" || dock === "right";
    const supportedDock = () => {
        const dock = currentDock();
        const view = h.notch.dataset.view;
        if (dock === "bottom") return view === "bottomIdle" || view === "bottomMedia";
        if (dock === "left" || dock === "right") return view === "edgeIdle" || view === "edgeMedia";
        return false;
    };
    const blockedReason = (): string | null => {
        if (!enabled()) return "Checklist animasi terbaca nonaktif.";
        if (!supportedDock()) return `Notch tidak berada di posisi bawah/samping yang didukung (dock=${currentDock() ?? "lain"}, view=${h.notch.dataset.view ?? "?"}).`;
        if (h.busy()) return "Notch sedang dipakai, panel terbuka, atau drag masih aktif.";
        return null;
    };
    const allowed = () => blockedReason() === null;

    let lastBlockedLog = "";
    let manualLiquidRequested = false;
    const reportTest = (payload: { requestId?: string; ok: boolean; message: string }) => {
        void emit("bottom-play-test-result", payload).catch((error) => {
            console.warn("[BottomPlay] Gagal mengirim status uji:", error);
        });
    };

    // Settings memberi sinyal langsung ke jendela Notch, tidak hanya mengandalkan localStorage.
    void listen<{ enabled?: boolean }>("bottom-play-setting-changed", ({ payload }) => {
        if (typeof payload?.enabled !== "boolean") return;
        runtimeEnabledOverride = payload.enabled;
        enabledVal = payload.enabled;
        enabledAt = performance.now();
        try { localStorage.setItem(KEY, payload.enabled ? "1" : "0"); } catch { /* storage opsional */ }
        if (!payload.enabled && active) abort();
        if (payload.enabled) schedule(0);
        console.info(`[BottomPlay] Saklar dari Settings: ${payload.enabled ? "aktif" : "nonaktif"}.`);
    }).catch((error) => console.warn("[BottomPlay] Listener saklar gagal:", error));

    // Tes manual langsung menjalankan liquid sehingga tidak perlu menunggu interval 3 menit.
    void listen<{ requestId?: string }>("bottom-play-test", ({ payload }) => {
        const requestId = payload?.requestId;
        const reason = blockedReason();
        if (reason) {
            reportTest({ requestId, ok: false, message: `Animasi belum dapat berjalan: ${reason}` });
            console.warn("[BottomPlay] Tes ditolak:", reason);
            return;
        }
        manualLiquidRequested = true;
        if (!active) schedule(0);
        reportTest({ requestId, ok: true, message: active ? "Tes liquid masuk antrean; gerakan aktif saat ini selesai dahulu." : "Tes liquid dimulai." });
        console.info("[BottomPlay] Tes liquid diminta dari Settings.");
    }).catch((error) => console.warn("[BottomPlay] Listener tes gagal:", error));
    const MAX_WALK = 380; // batas jalan dari posisi asal (px)
    const axisRange = (): [number, number] =>
        [anchor0 - Math.min(slackLo, MAX_WALK), anchor0 + Math.min(slackHi, MAX_WALK)];
    const axisPosition = (): number => {
        const dock = axisDock();
        if (isSideDock(dock)) return h.getTy?.() ?? -1;
        return h.getCx();
    };
    // ---------- menunggu (digerakkan frame, bisa dibatalkan) ----------
    const until = (ok: () => boolean) => new Promise<void>((res, rej) => { waits.push({ ok, res, rej }); });
    const wait = (ms: number) => { const t = performance.now() + ms; return until(() => performance.now() >= t); };

    // ---------- fisika ----------

    function physics(dt: number) {
        if (grav) {
            if (y.x < 0 || y.v < 0) {
                y.v += G * dt; y.x += y.v * dt;
                if (y.x >= 0) {
                    if (y.v > 140) poke(sq, Math.min(0.26, 0.04 + y.v * 0.0003));
                    y.x = 0; y.v = y.v > 700 ? -y.v * 0.1 : 0;
                }
            }
        } else {
            y.step(dt);
        }
        sq.step(dt); wall.step(dt); rot.step(dt); px.step(dt); flow.step(dt); float.step(dt); tall.step(dt);
    }

    function stepTravel(dt: number) {
        if (!travel) return;
        const T = travel;
        const [lo, hi] = axisRange();
        T.dest = clamp(T.dest, lo, hi);
        const d = T.dest - T.carrot;
        const A = T.acc ?? ACC;
        const vd = Math.sign(d) * Math.min(T.speed, Math.sqrt(2 * A * Math.abs(d)));
        T.cv += clamp(vd - T.cv, -A * dt, A * dt);
        T.carrot += T.cv * dt;
        if (Math.abs(d) < 0.5 && Math.abs(T.cv) < 8) { T.carrot = T.dest; T.cv = 0; }
        px.t = T.carrot;

        // menabrak tepi layar: badan menyepit ke samping lalu membal
        if (T.edge && !T.bumped && Math.abs(px.x - T.dest) < 10 && Math.abs(px.v) > 50) {
            T.bumped = true; poke(wall, Math.min(0.24, Math.abs(px.v) * 0.0016));
        }
        if (T.carrot === T.dest && Math.abs(px.x - T.dest) < 1.5 && Math.abs(px.v) < 20) {
            px.x = T.dest; px.v = 0; travel = null;
            poke(sq, 0.1); // tiba: badan bergoyang sebentar
        }
    }

    function paint() {
        // Pertahankan sumbu gerak sebelumnya selama efek pegas mereda setelah abort().
        const dock = axisDock();
        const side = isSideDock(dock);
        // Seluruh perpindahan = translate CSS dari posisi asal (jendela native tidak bergerak).
        const resid = clamp(px.x - anchor0, -slackLo - 4, slackHi + 4);
        const speedStretch = Math.min(0.13, Math.abs(px.v) * 0.00022);
        const ripple = flow.x;
        const pulse = clamp(float.x * 0.012, -0.025, 0.025);
        let sx: number;
        let sy: number;
        if (side) {
            // Di sisi, peregangan mengikuti gerak vertikal. Sisi luar tetap menjadi titik tumpu.
            sx = clamp(1 + sq.x * 0.60 - wall.x * 0.70 - pulse * 0.20 - speedStretch * 0.35 - ripple * 0.0015, 0.90, 1.10);
            sy = clamp(1 - sq.x * 0.72 + wall.x * 0.25 + pulse + speedStretch + ripple * 0.003 + rot.x * 0.005, 0.90, 1.12);
        } else {
            // Di bawah, peregangan mengikuti gerak horizontal dan titik bawah tetap terkunci.
            sx = clamp(1 + sq.x * 0.72 - wall.x * 0.72 - pulse * 0.25 + speedStretch - ripple * 0.0015 + rot.x * 0.005, 0.92, 1.12);
            sy = clamp(1 - sq.x + wall.x * 0.5 + pulse - speedStretch * 0.3 + ripple * 0.003, 0.90, 1.10);
        }
        // Efek hantu: hanya memakai scale (compositor), jadi tetap ringan dan tidak bergetar.
        // Saat meninggi badan sedikit menyempit; saat menyusut ada sedikit gepeng lalu kembali 1.
        const lift = clamp(tall.x, -0.25, 1.1);
        sx = clamp(sx - lift * 0.30, 0.68, 1.12);
        sy = clamp(sy + lift, 0.75, 2.1);
        const style = h.shell.style;
        if (side) style.setProperty("--bp-y", `${resid.toFixed(2)}px`);
        else style.setProperty("--bp-x", `${resid.toFixed(2)}px`);
        // Rotasi perlu ditulis setiap frame; sebelumnya --bp-r di-reset ke 0 saat mulai
        // tetapi tidak pernah diperbarui, sehingga wiggle/sway/dance nyaris tidak terlihat.
        // TIDAK ada rotasi nyata: memutar badan di atas tepi layar mengangkat salah satu sudutnya
        // sehingga notch terlepas dari dasar/sisi. Kemiringan diterjemahkan jadi goyangan lebar/tinggi
        // saja, sedangkan sisi yang menempel tidak pernah bergeser.
        style.setProperty("--bp-r", "0deg");
        style.setProperty("--bp-sx", sx.toFixed(4));
        style.setProperty("--bp-sy", sy.toFixed(4));
    }

    const settled = () =>
        !active && !travel &&
        rot.rest && sq.rest && wall.rest && flow.rest && float.rest && tall.rest && grav &&
        y.x === 0 && y.v === 0 && Math.abs(px.v) < 0.5 && Math.abs(rotT) < 0.02 && Math.abs(rot.x) < 0.02;

    function frame(ts: number) {
        const dt = Math.min(0.033, (ts - lastTs) / 1000 || 0.016);
        lastTs = ts;

        if (active && !allowed()) abort();

        const now = performance.now();
        // Impuls kecil acak memberi gerakan mikro terus-menerus selama animasi aktif.
        // Nilainya berubah target, bukan langsung mengubah transform, sehingga tetap lembut.
        if (active && now >= nextRippleAt) {
            const calm = h.calm();
            const strength = calm ? 0.34 : h.playing() ? 1.12 : 0.82;
            // Riak lebih jarang dan lebih halus supaya tubuh tidak terlihat bergetar terus-menerus.
            nextRippleAt = now + (calm ? rand(1000, 1700) : h.playing() ? rand(380, 760) : rand(520, 1150));
            flow.t = rand(-2.5, 2.5) * strength;
            float.t = rand(-1.25, 0.55) * strength;
            poke(sq, rand(-0.018, 0.026) * strength);
            if (Math.random() < 0.08) poke(wall, rand(-0.012, 0.015) * strength);
        } else if (!active) {
            flow.t = 0;
            float.t = 0;
        }

        stepTravel(dt);
        // inersia: badan condong ke belakang saat melaju, ke depan saat mengerem
        const ax = (px.v - prevV) / dt; prevV = px.v;
        axS += (ax - axS) * 0.25;
        rot.t = rotT; // tanpa kemiringan inersia: sumber goyangan acak yang terlihat seperti getar

        // Substep 12 ms cukup stabil untuk pegas yang dipakai di sini dan mengurangi kerja CPU.
        const n = Math.ceil(dt / 0.012), sub = dt / n;
        for (let i = 0; i < n; i++) physics(sub);
        if (!grav && !active && Math.abs(y.x) < 0.5 && Math.abs(y.v) < 20) { grav = true; y.x = 0; y.v = 0; }

        // Area klik native mengikuti notch yang bergeser (10x/detik cukup, tanpa menggeser jendela).
        if ((travel || Math.abs(px.v) > 0.5) && now - lastHit > 100) {
            lastHit = now;
            const d = px.x - anchor0;
            if (isSideDock(axisDock())) h.hit(0, d); else h.hit(d, 0);
        }
        paint();

        const ready = waits.filter((w) => w.ok());
        if (ready.length) { waits = waits.filter((w) => !ready.includes(w)); ready.forEach((w) => w.res()); }

        if (settled()) { finish(); return; }
        raf = requestAnimationFrame(frame);
    }

    function startLoop() {
        if (raf) return;
        lastTs = performance.now(); prevV = px.v; axS = 0;
        raf = requestAnimationFrame(frame);
    }

    function finish() {
        raf = 0;
        h.shell.removeAttribute("data-play");
        h.shell.removeAttribute("data-play-axis");
        for (const v of ["--bp-x", "--bp-y", "--bp-r", "--bp-sx", "--bp-sy"]) h.shell.style.removeProperty(v);
        resetTune(); rotT = 0;
        motionDock = null;
        h.hit();
    }

    // dibatalkan (hover, kartu, seretan, saklar mati): berhenti mengerem halus di tempat
    function abort() {
        const ws = waits; waits = [];
        ws.forEach((w) => w.rej(CANCEL));
        active = "";
        // Bila notch sedang jauh dari asalnya, luncurkan cepat dan halus kembali supaya hub/kartu
        // yang terbuka karena interaksi muncul di posisi yang benar.
        travel = Math.abs(px.x - anchor0) > 1.5
            ? { dest: anchor0, carrot: px.x, cv: px.v, speed: 700, edge: false, bumped: false, acc: 2600 }
            : null;
        px.t = px.x; px.tune(90, 22);
        sq.t = 0; wall.t = 0; rotT = 0;
        sq.tune(220, 12); rot.tune(160, 16);
        tall.t = 0; tall.tune(220, 18); // hantu yang terputus menyusut cepat tapi halus
        if (!grav) { y.tune(160, 20); y.t = 0; }
        flow.t = 0; float.t = 0;
        schedule(1500);
    }

    // ---------- gerakan ----------
    const travelTo = (dest: number, edge: boolean, speed: [number, number] = [75, 135]) => {
        travel = { dest, carrot: px.x, cv: 0, speed: rand(speed[0], speed[1]), edge, bumped: false };
        return until(() => !travel);
    };

    const TRICKS: Record<string, () => Promise<void>> = {
        // menekuk perlahan, melenting dengan gravitasi, mendarat gepeng lalu bergoyang
        async hop() {
            // Rebound bentuk tanpa mengangkat badan dari tepi yang ditempeli.
            sq.t = 0.16; await wait(140);
            sq.t = -0.08; poke(sq, -0.07); await wait(180);
            sq.t = 0; poke(sq, 0.025); await wait(220);
        },
        // meluncur ke bawah layar, sembunyi sebentar, lalu muncul kaget
        async peek() {
            // Peek digambarkan dengan penyusutan singkat, bukan gerakan menjauh dari tepi.
            sq.t = 0.12; await wait(260);
            sq.t = -0.10; poke(sq, -0.06); await wait(320);
            sq.t = 0; await wait(220);
        },
        async wiggle() {
            rot.tune(200, 11);
            for (const a of [-7, 7, -6, 5, -3.5, 2, 0]) {
                rotT = a;
                flow.t = a * 0.32;
                poke(sq, Math.sign(a) * 0.012);
                await wait(110);
            }
            flow.t = 0;
        },
        // mengembang-mengempis seperti agar-agar
        async squish() {
            for (const a of [0.15, -0.13, 0.1, -0.07, 0.04]) { poke(sq, a); await wait(300); }
            await wait(200);
        },
        // mengayun pelan
        async sway() {
            rot.tune(36, 7);
            for (const a of [-4.5, 4.5, -2, 0]) {
                rotT = a;
                flow.t = a * 0.22;
                await wait(560);
            }
            flow.t = 0;
            await wait(300);
        },
        // ikut ketukan: miring kiri-kanan sambil memantul kecil
        async dance() {
            rot.tune(180, 12);
            for (let i = 0; i < 8; i++) {
                rotT = i % 2 ? 3.2 : -3.2; poke(sq, 0.06); flow.t = i % 2 ? 1.2 : -1.2;
                await wait(210);
            }
            rotT = 0; await wait(250);
        },
        // meregang ke atas pelan-pelan, lalu lemas
        async yawn() {
            sq.tune(50, 9); sq.t = -0.15; await wait(950);
            sq.tune(220, 9); sq.t = 0; await wait(600);
        },
        // Mengalir ke titik-titik acak, kadang berubah arah dan sedikit mengembang saat berbelok.
        // Tidak wajib pulang ke posisi awal agar pola terasa hidup, bukan animasi bolak-balik.
        async liquid() {
            const [lo, hi] = axisRange();
            const span = hi - lo;
            if (axisPosition() < 0 || span < 140) {
                await TRICKS.squish();
                await TRICKS.sway();
                return;
            }

            const calm = h.calm();
            const strength = calm ? 0.42 : h.playing() ? 0.78 : 0.68;
            const steps = Math.floor(rand(calm ? 2 : 3, calm ? 4 : 6));
            let direction = Math.random() < 0.5 ? -1 : 1;

            for (let i = 0; i < steps; i++) {
                const from = clamp(px.x, lo, hi);
                if (Math.random() < (calm ? 0.22 : 0.38)) direction *= -1;

                const distance = rand(span * (calm ? 0.035 : 0.055), span * (calm ? 0.10 : 0.15));
                let dest = clamp(from + direction * distance + rand(-span * 0.035, span * 0.035), lo, hi);
                if (Math.abs(dest - from) < Math.min(42, span * 0.06)) {
                    direction = from < (lo + hi) / 2 ? 1 : -1;
                    dest = clamp(from + direction * rand(span * 0.08, span * 0.16), lo, hi);
                }

                let edge = dest <= lo + 1 || dest >= hi - 1;
                // Sebagian besar gerak berbelok sebelum tepi, sesekali menyentuh tepi untuk efek pantul.
                if (edge && Math.random() < 0.58) {
                    dest = clamp(dest + (dest <= lo + 1 ? 1 : -1) * rand(18, 46), lo, hi);
                    edge = false;
                }

                const speed: [number, number] = calm ? [28, 48] : h.playing() ? [58, 98] : [42, 78];
                await travelTo(dest, edge, speed);
                poke(sq, rand(0.03, 0.085) * strength);
                flow.t = rand(-2.8, 2.8) * strength;
                float.t = rand(-1.1, 0.45) * strength;
                if (Math.random() < (calm ? 0.12 : 0.34)) rotT = rand(-4.2, 4.2) * strength;
                await wait(rand(calm ? 520 : 360, calm ? 950 : 720));

                // Riak bentuk kecil ketika berhenti sejenak. Saat mode tenang tidak ada lompatan.
                if (!calm && Math.random() < 0.34) {
                    sq.t = rand(0.035, 0.095);
                    await wait(rand(180, 320));
                    sq.t = 0;
                }
                if (!calm && Math.random() < 0.22) {
                    // Riak kecil berupa gepeng-melebar, bukan lompatan vertikal.
                    sq.t = rand(0.045, 0.095);
                    flow.t = rand(-2.2, 2.2) * strength;
                    poke(sq, rand(0.008, 0.025));
                    await wait(rand(220, 380));
                    sq.t = 0;
                }
            }

            rotT = 0;
            sq.t = 0;
        },
        // Berjalan pelan lalu TIBA-TIBA meninggi seperti hantu, tetap berjalan sambil menjulang,
        // kemudian menyusut kembali ke ukuran semula. Murni scale CSS, tanpa mengubah ukuran jendela.
        async ghost() {
            const [lo, hi] = axisRange();
            const span = hi - lo;
            if (axisPosition() < 0 || span < 140) { await TRICKS.sway(); return; }

            const side = isSideDock(axisDock());
            const from = clamp(px.x, lo, hi);
            const mid = (lo + hi) / 2;
            // Selalu berjalan menuju sisi yang lebih lapang (di samping: badan memanjang ke bawah).
            let dir = side ? (from > mid ? -1 : 1) : (Math.random() < 0.5 ? -1 : 1);
            const dist = clamp(rand(span * 0.2, span * 0.34), 120, span * 0.45);
            if (from + dir * dist < lo || from + dir * dist > hi) dir = -dir;
            const dest = clamp(from + dir * dist, lo, hi);

            const moving = travelTo(dest, false, [40, 66]);
            moving.catch(() => { }); // pembatalan ditangani oleh wait() di bawah
            const vh = h.shell.offsetHeight || 60;

            const bursts = Math.random() < 0.35 ? 2 : 1;
            for (let i = 0; i < bursts; i++) {
                await wait(rand(650, 1500));
                // Tinggi dibatasi ruang yang tersisa di sisi layar supaya tidak terpotong.
                let height = rand(0.7, 1.0) * (i ? 0.75 : 1);
                if (side) height = Math.min(height, Math.max(0, (hi - Math.max(px.x, dest) + 12) / vh));
                if (height < 0.3) break;

                // MENINGGI mendadak: target naik + dorongan kecepatan awal agar terasa tiba-tiba.
                tall.tune(150, 12); tall.t = height; poke(tall, 0.35);
                h.mood("surprised", 1500);
                rotT = rand(-2.5, 2.5);               // sedikit meliuk saat menjulang
                flow.t = rand(-2, 2);
                await wait(rand(1000, 1700));         // tetap berjalan sambil tinggi

                // MENYUSUT ke ukuran semula, sedikit gepeng saat mendarat lalu pulih
                tall.tune(130, 11); tall.t = 0; rotT = 0; flow.t = 0;
                await wait(rand(450, 650));
            }
            await moving;
            poke(sq, 0.05);
            await wait(350);
        },
        // meluncur ke posisi acak (kadang sampai mentok di tepi layar), lalu pulang
        async roam() {
            const [lo, hi] = axisRange();
            const home = axisPosition();
            if (home < 0 || hi - lo < 160) { await TRICKS.wiggle(); return; }
            const legs = Math.random() < 0.4 ? 2 : 1;
            for (let i = 0; i < legs; i++) {
                const r = Math.random();
                let dest = r < 0.3 ? lo : r < 0.6 ? hi : rand(lo, hi);
                if (Math.abs(dest - px.x) < 120) dest = px.x < (lo + hi) / 2 ? hi : lo;
                await travelTo(dest, dest === lo || dest === hi);
                h.mood("happy", 1400);
                await wait(rand(500, 1100));
                await (Math.random() < 0.5 ? TRICKS.squish() : TRICKS.wiggle());
            }
            await travelTo(clamp(home, lo, hi), false);
            await wait(250);
        },
    };

    function schedule(ms?: number) {
        window.clearTimeout(timer);
        // Gerakan biasa tidak terlalu rapat: beri jeda agar karakter terasa santai.
        // Jadwal liquid 3 menit tetap dikelola timer terpisah di bawah.
        const delay = h.calm() ? rand(10000, 18000) : h.playing() ? rand(5000, 9500) : rand(7000, 14000);
        timer = window.setTimeout(tick, ms ?? delay);
    }

    // Jadwal liquid berjalan terpisah dari timer gerakan acak. Dengan begitu, timer gerakan
    // biasa tidak bisa terus menggeser waktu liquid. Jika Notch sedang dipakai/di-hover,
    // tiga gerakan tetap antre dan dijalankan ketika kondisi aman kembali.
    let nextLiquidAt = performance.now() + LIQUID_INTERVAL_MS;
    let liquidRepetitionsLeft = 0;
    // Waktu acak untuk gerakan ke-2 dan ke-3 dalam satu siklus. Ini mencegah ketiga
    // rangkaian bertumpuk rapat dan membuat pola tampak lebih alami.
    let nextLiquidRunAt = 0;

    function tick() {
        if (active) { schedule(500); return; }
        if (!allowed()) { schedule(1500); return; }

        // Tes manual dari Settings didahulukan, tanpa menunggu jadwal otomatis.
        if (manualLiquidRequested) {
            manualLiquidRequested = false;
            void run("liquid", "manual");
            return;
        }

        // Satu siklus berisi 3 rangkaian liquid. Antar-rangkaian diberi jeda acak,
        // tetapi tetap harus selesai sebelum siklus 3 menit berikutnya dimulai.
        if (liquidRepetitionsLeft > 0) {
            const remaining = nextLiquidRunAt - performance.now();
            if (remaining > 0) {
                schedule(remaining);
                return;
            }
            liquidRepetitionsLeft--;
            void run("liquid", "scheduled");
            return;
        }

        void run(pick(h.calm() ? CALM : h.playing() ? MUSIC : NORMAL, last), "random");
    }

    function pollLiquidSchedule() {
        const now = performance.now();
        const reason = blockedReason();
        const state = reason ?? "siap bergerak";
        if (state !== lastBlockedLog) {
            lastBlockedLog = state;
            console.info(reason ? `[BottomPlay] Menunggu: ${reason}` : "[BottomPlay] Kondisi memenuhi syarat; animasi acak aktif.");
        }
        if (now >= nextLiquidAt) {
            if (liquidRepetitionsLeft === 0) {
                liquidRepetitionsLeft = LIQUID_REPETITIONS;
                nextLiquidRunAt = now;
                console.info("[BottomPlay] Jadwal liquid tiba: 3 rangkaian akan dijalankan dengan jeda acak.");
            }
            // Jangan membuat backlog besar ketika aplikasi ditangguhkan lama.
            nextLiquidAt = now + LIQUID_INTERVAL_MS;
        }

        // Setelah waktunya tiba, dahulukan liquid dari timer gerakan acak. Jika masih
        // ada interaksi/hover, tick() akan mencoba kembali tanpa membuang antrean.
        const liquidRunDue = liquidRepetitionsLeft > 0 && now >= nextLiquidRunAt;
        if ((liquidRunDue || manualLiquidRequested) && !active && allowed()) {
            window.clearTimeout(timer);
            timer = window.setTimeout(tick, 0);
        }
        window.setTimeout(pollLiquidSchedule, 500);
    }

    async function run(name: string, source: "random" | "scheduled" | "manual" = "random") {
        last = active = name;
        grav = true; y.x = 0; y.v = 0; rotT = 0;
        const dock = currentDock();
        motionDock = dock; // kunci bawah/kiri/kanan selama gerakan dan fase mereda
        const side = isSideDock(dock);
        const position = axisPosition();
        // Ukur ruang kosong di dalam jendela sebelum transform animasi dipasang.
        const rect = h.shell.getBoundingClientRect();
        const margin = 8;
        anchor0 = position < 0 ? 0 : position;
        if (side) { slackLo = Math.max(0, rect.top - margin); slackHi = Math.max(0, window.innerHeight - rect.bottom - margin); }
        else { slackLo = Math.max(0, rect.left - margin); slackHi = Math.max(0, window.innerWidth - rect.right - margin); }
        if (position < 0) { slackLo = 0; slackHi = 0; }
        px.x = px.t = anchor0;
        px.v = 0;
        h.shell.dataset.play = name;
        h.shell.dataset.playAxis = side ? "side" : "bottom";
        // Titik tumpu disetel untuk docking bawah atau samping saja.
        h.shell.style.setProperty("--bp-x", "0px");
        h.shell.style.setProperty("--bp-y", "0px");
        h.shell.style.setProperty("--bp-r", "0deg");
        nextRippleAt = performance.now() + rand(100, 320);
        flow.t = 0; float.t = 0;
        const [m, ms] = MOOD[name];
        h.mood(m, ms);
        startLoop();
        try {
            await TRICKS[name]();
            // Selalu pulang ke posisi asal dengan luncuran halus (jendela tidak dipindah).
            if (Math.abs(px.x - anchor0) > 1.5) { await travelTo(anchor0, false, [55, 95]); await wait(250); }
        } catch (e) {
            if (e === CANCEL) {
                // Gerakan liquid yang terputus oleh hover/interaksi tetap harus melengkapi 3 putaran.
                if (name === "liquid" && source === "scheduled") {
                    liquidRepetitionsLeft = Math.min(LIQUID_REPETITIONS, liquidRepetitionsLeft + 1);
                    nextLiquidRunAt = performance.now() + rand(2500, 6000);
                } else if (name === "liquid" && source === "manual") {
                    manualLiquidRequested = true;
                }
                return; // abort() sudah merapikan gerakan dan menjadwalkan ulang
            }

            // Kegagalan tak terduga tidak boleh meninggalkan `active` terisi selamanya,
            // karena itu akan membuat seluruh gerakan berikutnya terblokir.
            console.error("[BottomPlay] Animasi gagal:", name, e);
            active = "";
            travel = null;
            sq.t = 0; wall.t = 0; rotT = 0; flow.t = 0; float.t = 0; tall.t = 0;
            finish();
            schedule(1000);
        }
        active = "";
        sq.t = 0; wall.t = 0; rotT = 0;
        if (name === "liquid" && source === "scheduled" && liquidRepetitionsLeft > 0) {
            nextLiquidRunAt = performance.now() + rand(7000, 22000);
            console.info(`[BottomPlay] Liquid selesai; rangkaian berikutnya dijadwalkan dengan jeda acak ${Math.round((nextLiquidRunAt - performance.now()) / 1000)} detik.`);
        }
        schedule();
    }

    schedule(4000);
    window.setTimeout(pollLiquidSchedule, 500);
}
