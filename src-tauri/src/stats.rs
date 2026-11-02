use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;
use sysinfo::{CpuRefreshKind, Disks, Networks, ProcessRefreshKind, ProcessesToUpdate, System};
use tauri::{AppHandle, Emitter};

#[cfg(not(target_os = "windows"))]
use std::path::Path;

// Semua teks memakai &str supaya payload per detik tidak mengalokasikan String baru.
// Nilai yang tidak berubah (nama CPU, host, OS) dibuat sekali di awal lalu dipinjam terus.
#[derive(Serialize, Clone)]
struct Stats<'a> {
    // CPU
    cpu: f32,                 // persen 0..100
    cpu_cores: usize,         // jumlah logical processor/thread
    cpu_freq_mhz: u64,        // frekuensi rata-rata yang dilaporkan OS
    cpu_name: &'a str,

    // RAM dan swap
    ram: f32,
    ram_used: f64,
    ram_total: f64,
    swap: f32,
    swap_used: f64,
    swap_total: f64,

    // Partisi sistem utama
    disk: f32,
    disk_used: f64,
    disk_total: f64,
    disk_label: &'a str,

    // Jaringan: byte per detik dari interval refresh terakhir
    down: f64,
    up: f64,

    // Ringkasan sistem
    processes: usize,
    uptime_secs: u64,
    host_name: &'a str,
    os_name: &'a str,
    os_version: &'a str,
    kernel_version: &'a str,
}

fn percent(used: u64, total: u64) -> f32 {
    if total == 0 {
        0.0
    } else {
        ((used as f64 / total as f64) * 100.0).clamp(0.0, 100.0) as f32
    }
}

// String yang hidup selama aplikasi berjalan (dibuat sekali, tidak pernah dibebaskan).
fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

fn primary_disk(disks: &Disks) -> Option<&sysinfo::Disk> {
    #[cfg(target_os = "windows")]
    {
        let system_drive = std::env::var("SystemDrive")
            .unwrap_or_else(|_| "C:".to_string())
            .to_ascii_lowercase();
        disks
            .iter()
            .find(|disk| {
                disk.mount_point()
                    .to_string_lossy()
                    .to_ascii_lowercase()
                    .starts_with(&system_drive)
            })
            .or_else(|| disks.iter().max_by_key(|disk| disk.total_space()))
    }

    #[cfg(target_os = "macos")]
    {
        disks
            .iter()
            .find(|disk| disk.mount_point() == Path::new("/System/Volumes/Data"))
            .or_else(|| disks.iter().find(|disk| disk.mount_point() == Path::new("/")))
            .or_else(|| disks.iter().max_by_key(|disk| disk.total_space()))
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        disks
            .iter()
            .find(|disk| disk.mount_point() == Path::new("/"))
            .or_else(|| disks.iter().max_by_key(|disk| disk.total_space()))
    }
}

// true hanya selama halaman Stats benar-benar terlihat di hub (diatur frontend lewat stats_set_active).
static ACTIVE: AtomicBool = AtomicBool::new(false);

#[tauri::command]
pub fn stats_set_active(active: bool) {
    ACTIVE.store(active, Ordering::Relaxed);
}

pub fn start(app: AppHandle) {
    // halaman "stats" dihapus dari config -> pemantauan tidak dijalankan sama sekali
    if !crate::config::get().pages.iter().any(|p| p == "stats") {
        return;
    }

    thread::spawn(move || {
        // Dulu System::new_all() memuat SEMUA hal (seluruh proses beserta path/cmd/env-nya, dll.).
        // Sekarang hanya CPU + memori; daftar proses diambil seminimal mungkin (lihat bawah).
        let mut sys = System::new();
        sys.refresh_cpu_specifics(CpuRefreshKind::everything()); // sekali: nama, jumlah core, frekuensi
        sys.refresh_memory();
        let mut nets = Networks::new_with_refreshed_list();
        let mut disks = Disks::new_with_refreshed_list();

        // data statis: dibuat sekali
        let host_name = leak(System::host_name().unwrap_or_else(|| "Perangkat".to_string()));
        let os_name = leak(System::name().unwrap_or_else(|| std::env::consts::OS.to_string()));
        let os_version = leak(System::os_version().unwrap_or_default());
        let kernel_version = leak(System::kernel_version().unwrap_or_default());

        let cpu_list = sys.cpus();
        let cpu_cores = cpu_list.len();
        let cpu_freq_mhz = if cpu_cores > 0 {
            cpu_list.iter().map(|cpu| cpu.frequency()).sum::<u64>() / cpu_cores as u64
        } else {
            0
        };
        let cpu_name = leak(
            cpu_list
                .first()
                .map(|cpu| cpu.brand().trim().to_string())
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| "CPU".to_string()),
        );

        // nilai yang berubah lambat: disegarkan tiap beberapa detik, bukan tiap detik
        let mut tick: u32 = 0;
        let mut processes = 0usize;
        let mut disk_used = 0u64;
        let mut disk_total = 0u64;
        let mut disk_label = String::from("Penyimpanan N/A");

        let mut was_active = false;
        loop {
            // Halaman Stats tertutup: tidak ada refresh sysinfo dan tidak ada event ke webview.
            if !ACTIVE.load(Ordering::Relaxed) {
                was_active = false;
                thread::sleep(Duration::from_millis(300));
                continue;
            }
            if !was_active {
                // baru dibuka: buang selisih selama jeda (kalau tidak, kecepatan jaringan melonjak palsu)
                was_active = true;
                sys.refresh_cpu_usage();
                nets.refresh(true);
                tick = 0;
            }
            // refresh CPU/jaringan memakai selisih antar-panggilan, jadi instance dipertahankan
            thread::sleep(Duration::from_millis(1000));

            sys.refresh_cpu_usage();
            sys.refresh_memory();
            nets.refresh(true);

            // Jumlah proses cukup dihitung tiap 5 dtk. ProcessRefreshKind::nothing() = hanya
            // daftar PID/nama, tanpa cmd, exe, environ, dsb. (yang paling banyak makan memori).
            if tick % 5 == 0 {
                sys.refresh_processes_specifics(
                    ProcessesToUpdate::All,
                    true,
                    ProcessRefreshKind::nothing(),
                );
                processes = sys.processes().len();
            }

            // Disk jarang berubah, dan menyegarkannya menyentuh semua volume. Cukup tiap 10 dtk.
            if tick % 10 == 0 {
                disks.refresh(true);
                if let Some(disk) = primary_disk(&disks) {
                    disk_total = disk.total_space();
                    disk_used = disk_total.saturating_sub(disk.available_space());
                    disk_label.clear();
                    disk_label.push_str(&disk.mount_point().to_string_lossy());
                } else {
                    disk_used = 0;
                    disk_total = 0;
                    disk_label.clear();
                    disk_label.push_str("Penyimpanan N/A");
                }
            }
            tick = (tick + 1) % 10;

            // received()/transmitted() menunjukkan byte sejak refresh jaringan sebelumnya.
            let (mut rx, mut tx) = (0u64, 0u64);
            for (_, network) in nets.iter() {
                rx = rx.saturating_add(network.received());
                tx = tx.saturating_add(network.transmitted());
            }

            let ram_total = sys.total_memory();
            let ram_used = sys.used_memory();
            let swap_total = sys.total_swap();
            let swap_used = sys.used_swap();

            // dikirim lewat referensi: tidak ada salinan Stats / String per detik
            let _ = app.emit(
                "stats",
                &Stats {
                    cpu: sys.global_cpu_usage().clamp(0.0, 100.0),
                    cpu_cores,
                    cpu_freq_mhz,
                    cpu_name,
                    ram: percent(ram_used, ram_total),
                    ram_used: ram_used as f64,
                    ram_total: ram_total as f64,
                    swap: percent(swap_used, swap_total),
                    swap_used: swap_used as f64,
                    swap_total: swap_total as f64,
                    disk: percent(disk_used, disk_total),
                    disk_used: disk_used as f64,
                    disk_total: disk_total as f64,
                    disk_label: &disk_label,
                    down: rx as f64,
                    up: tx as f64,
                    processes,
                    uptime_secs: System::uptime(),
                    host_name,
                    os_name,
                    os_version,
                    kernel_version,
                },
            );
        }
    });
}
