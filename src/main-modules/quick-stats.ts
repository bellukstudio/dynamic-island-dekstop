// @ts-nocheck
// Kontrol cepat dan statistik sistem.
// ---------- kontrol cepat ----------
type Quick = {
    mic: boolean | null;
    wifi: boolean | null;
    bt: boolean | null;
    awake: boolean | null;
    dark: boolean | null;
};
type QKey = keyof Quick;
// ---------- status sistem (CPU / RAM / swap / disk / jaringan / info sistem) ----------
type Stats = {
    cpu: number;
    cpu_cores: number;
    cpu_freq_mhz: number;
    cpu_name: string;
    ram: number;
    ram_used: number;
    ram_total: number;
    swap: number;
    swap_used: number;
    swap_total: number;
    disk: number;
    disk_used: number;
    disk_total: number;
    disk_label: string;
    down: number;
    up: number;
    processes: number;
    uptime_secs: number;
    host_name: string;
    os_name: string;
    os_version: string;
    kernel_version: string;
};

export function register(ctx: Record<string, any>): void {
    ctx.paintQuick = function paintQuick(q: Quick) {
        (Object.keys(ctx.qIds) as QKey[]).forEach((k) => {
            const el = document.getElementById(ctx.qIds[k]);
            if (!el)
                return;
            const v = q[k];
            el.classList.toggle("na", v === null);
            el.classList.toggle("on", v === true);
            const [na, on, off] = ctx.qLabel[k];
            el.querySelector("span")!.textContent = v === null ? na : v ? on : off;
        });
        const wasAwake = ctx.keepAwake;
        ctx.keepAwake = q.awake === true;
        if (ctx.keepAwake !== wasAwake)
            ctx.updateBase();
    };
    ctx.refreshQuick = function refreshQuick() {
        ctx.invoke<Quick>("quick_state")
            .then((q) => { ctx.paintQuick(q); })
            .catch((e) => console.error("quick_state gagal:", e));
    };
}

export function init(ctx: Record<string, any>): void {
    ctx.qIds = {
        mic: "q-mic", wifi: "q-wifi", bt: "q-bt", awake: "q-awake", dark: "q-dark",
    };
    ctx.qLabel = {
        mic: ["Mic", "Mic mati", "Mic aktif"],
        wifi: ["Wi-Fi", "Wi-Fi on", "Wi-Fi off"],
        bt: ["Bluetooth", "Bluetooth on", "Bluetooth off"],
        awake: ["Keep awake", "Awake: ON", "Awake: OFF"],
        dark: ["Tema", "Gelap", "Terang"],
    };
    // klik tombol: Rust mengubah pengaturannya lalu mengembalikan keadaan terbaru
    ([...Object.keys(ctx.qIds), "lock"] as (QKey | "lock")[]).forEach((k) => {
        const id = k === "lock" ? "q-lock" : ctx.qIds[k];
        document.getElementById(id)?.addEventListener("click", (e) => {
            e.stopPropagation();
            if (k === "lock")
                ctx.flash("sleepy", 1500);
            ctx.invoke<Quick>("quick_toggle", { kind: k })
                .then(ctx.paintQuick)
                .catch((err) => console.error("quick_toggle gagal:", err));
        });
    });
    ctx.fmtSpeed = (b: number) => b >= 1048576 ? `${(b / 1048576).toFixed(1)} MB/s` : `${Math.round(b / 1024)} KB/s`;
    ctx.GB = 1024 ** 3;
    ctx.gib = (b: number) => (b / ctx.GB).toFixed(1);
    ctx.setText = (id: string, v: string) => {
        const el = document.getElementById(id);
        if (el && el.textContent !== v)
            el.textContent = v;
    };
    ctx.setBar = (id: string, pct: number, hotAt: number) => {
        const el = document.getElementById(id);
        if (!el)
            return;
        el.style.width = `${pct}%`;
        el.classList.toggle("hot", pct >= hotAt);
    };
    ctx.fmtUptime = (s: number) => {
        const d = Math.floor(s / 86400);
        const h = Math.floor((s % 86400) / 3600);
        const m = Math.floor((s % 3600) / 60);
        return d ? `${d}h ${h}j` : h ? `${h}j ${m}m` : `${m}m`;
    };
    // Thread Rust hanya memantau + mengirim event "stats" saat halaman Stats terbuka di hub.
    let statsOn = false;
    const syncStats = () => {
        const want = ctx.notch.dataset.view === "hub" && ctx.names?.[ctx.page] === "stats";
        if (want === statsOn)
            return;
        statsOn = want;
        void ctx.invoke("stats_set_active", { active: want }).catch(() => { statsOn = !want; });
    };
    new MutationObserver(syncStats).observe(ctx.notch, { attributes: true, attributeFilter: ["data-view"] });
    const prevSetPage = ctx.setPage;
    ctx.setPage = function (i: number) {
        const r = prevSetPage.call(this, i);
        syncStats();
        return r;
    };
    ctx.listen<Stats>("stats", (e) => {
        const s = e.payload;
        if (!document.getElementById("st-cpu"))
            return; // halaman stats dimatikan di config
        const cpu = Math.round(s.cpu);
        const ram = Math.round(s.ram);
        const swap = Math.round(s.swap);
        const disk = Math.round(s.disk);
        // bar + persen
        ctx.setBar("st-cpu", cpu, 85);
        ctx.setText("st-cpu-v", `${cpu}%`);
        ctx.setBar("st-ram", ram, 90);
        ctx.setText("st-ram-v", `${ram}%`);
        ctx.setBar("st-swap", swap, 90);
        ctx.setText("st-swap-v", `${swap}%`);
        ctx.setBar("st-disk", disk, 90);
        ctx.setText("st-disk-v", `${disk}%`);
        // detail (id harus sama dengan yang ada di index.html: st-*-meta)
        ctx.setText("st-cpu-meta", `${s.cpu_name} · ${s.cpu_cores} thread` +
            (s.cpu_freq_mhz ? ` · ${(s.cpu_freq_mhz / 1000).toFixed(1)} GHz` : ""));
        ctx.setText("st-ram-meta", `${ctx.gib(s.ram_used)} / ${ctx.gib(s.ram_total)} GiB digunakan`);
        ctx.setText("st-swap-meta", `${ctx.gib(s.swap_used)} / ${ctx.gib(s.swap_total)} GiB digunakan`);
        ctx.setText("st-disk-meta", s.disk_total > 0
            ? `${ctx.gib(s.disk_used)} / ${ctx.gib(s.disk_total)} GiB · ${s.disk_label}`
            : "Penyimpanan N/A");
        // jaringan + ringkasan sistem
        ctx.setText("st-down", `↓ ${ctx.fmtSpeed(s.down)}`);
        ctx.setText("st-up", `↑ ${ctx.fmtSpeed(s.up)}`);
        ctx.setText("st-processes", `${s.processes} proses`);
        ctx.setText("st-uptime", `Aktif ${ctx.fmtUptime(s.uptime_secs)}`);
        ctx.setText("st-os", `${s.os_name} ${s.os_version}`.trim());
        ctx.setText("st-kernel", s.kernel_version);
        ctx.setText("st-host", s.host_name);
    });
}