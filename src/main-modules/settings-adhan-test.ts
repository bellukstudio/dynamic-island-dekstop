// Tes audio adzan khusus jendela Settings. Jalur utama dibundle oleh Vite; jalur
// cadangan mendukung file di public/adzan.mp3 dan src/public/adzan.mp3 saat development.
const ADZAN_URLS = [...new Set([
    new URL("../public/adzan.mp3", import.meta.url).href,
    "/adzan.mp3",
    "/src/public/adzan.mp3",
])];

const startButton = document.getElementById("adhan-test-start") as HTMLButtonElement | null;
const pauseButton = document.getElementById("adhan-test-pause") as HTMLButtonElement | null;
const status = document.getElementById("adhan-test-status");

if (startButton && pauseButton && status) {
    type State = "ready" | "loading" | "playing" | "paused" | "ended" | "error";
    let audio: HTMLAudioElement | null = null;
    let sourceIndex = 0;
    let attemptId = 0;

    const setStatus = (message: string, state: State) => {
        status.textContent = message;
        status.setAttribute("data-state", state);
        pauseButton.disabled = !audio || audio.paused || audio.ended;
        startButton.disabled = state === "loading";
        startButton.textContent = state === "paused" ? "▶ Lanjutkan adzan" : "▶ Mulai tes adzan";
    };

    function tryNextSource(reason?: unknown) {
        const id = ++attemptId;
        if (audio) {
            audio.pause();
            audio.removeAttribute("src");
            audio.load();
        }
        if (reason) console.warn("[Settings] Jalur audio adzan gagal:", reason);
        if (sourceIndex >= ADZAN_URLS.length) {
            console.error("[Settings] Semua jalur audio gagal:", ADZAN_URLS);
            setStatus("Gagal memuat adzan.mp3 — pastikan file ada di src/public atau public", "error");
            return;
        }

        const src = ADZAN_URLS[sourceIndex++];
        const candidate = new Audio();
        audio = candidate;
        candidate.preload = "auto";
        candidate.volume = 0.9;
        let failed = false;
        const fail = (why: unknown) => {
            if (failed || id !== attemptId || audio !== candidate) return;
            failed = true;
            tryNextSource(`${src}: ${String(why)}`);
        };
        candidate.addEventListener("playing", () => {
            if (id === attemptId && audio === candidate) setStatus("Adzan sedang diputar", "playing");
        });
        candidate.addEventListener("pause", () => {
            if (id !== attemptId || audio !== candidate) return;
            if (candidate.ended || candidate.currentTime === 0) setStatus("Siap diputar", "ready");
            else setStatus("Adzan dijeda", "paused");
        });
        candidate.addEventListener("ended", () => {
            if (id === attemptId && audio === candidate) setStatus("Pemutaran selesai", "ended");
        }, { once: true });
        candidate.addEventListener("error", () => fail(`audio error ${candidate.error?.code ?? "unknown"}`), { once: true });
        candidate.src = src;
        candidate.load();
        console.info("[Settings] Mencoba audio adzan:", src);
        void candidate.play().then(() => {
            if (id === attemptId && audio === candidate) setStatus("Adzan sedang diputar", "playing");
        }).catch(fail);
        setStatus("Memuat suara adzan…", "loading");
    }

    startButton.addEventListener("click", async () => {
        if (audio && !audio.ended && audio.currentTime > 0 && audio.paused) {
            try {
                await audio.play();
                setStatus("Adzan sedang diputar", "playing");
            } catch (error) {
                setStatus(`Tidak dapat melanjutkan suara: ${String(error)}`, "error");
            }
            return;
        }
        sourceIndex = 0;
        tryNextSource();
    });

    pauseButton.addEventListener("click", () => {
        if (audio && !audio.paused && !audio.ended) audio.pause();
    });

    window.addEventListener("pagehide", () => {
        attemptId++;
        if (audio) {
            audio.pause();
            audio.currentTime = 0;
        }
    });

    setStatus("Siap diputar", "ready");
}
