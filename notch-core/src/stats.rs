//! Pemantauan CPU, RAM, swap, disk, jaringan, dan info sistem.
//!
//! Pemanggil memegang loop dan jedanya (mis. 1 detik): panggil [`StatsMonitor::resume`]
//! saat halaman Stats baru terlihat, lalu [`StatsMonitor::sample`] sekali per interval.

use serde::Serialize;
use sysinfo::{CpuRefreshKind, Disks, Networks, ProcessRefreshKind, ProcessesToUpdate, System};

#[cfg(not(target_os = "windows"))]
use std::path::Path;

/// Satu sampel. Teks dipinjam dari [`StatsMonitor`] supaya tidak ada alokasi String per detik.
#[derive(Serialize, Clone, Debug)]
pub struct Stats<'a> {
    // CPU
    pub cpu: f32,          // persen 0..100
    pub cpu_cores: usize,  // jumlah logical processor/thread
    pub cpu_freq_mhz: u64, // frekuensi rata-rata yang dilaporkan OS
    pub cpu_name: &'a str,

    // RAM dan swap
    pub ram: f32,
    pub ram_used: f64,
    pub ram_total: f64,
    pub swap: f32,
    pub swap_used: f64,
    pub swap_total: f64,

    // Partisi sistem utama
    pub disk: f32,
    pub disk_used: f64,
    pub disk_total: f64,
    pub disk_label: &'a str,

    // Jaringan: byte per detik dari interval refresh terakhir
    pub down: f64,
    pub up: f64,

    // Ringkasan sistem
    pub processes: usize,
    pub uptime_secs: u64,
    pub host_name: &'a str,
    pub os_name: &'a str,
    pub os_version: &'a str,
    pub kernel_version: &'a str,
}

fn percent(used: u64, total: u64) -> f32 {
    if total == 0 {
        0.0
    } else {
        ((used as f64 / total as f64) * 100.0).clamp(0.0, 100.0) as f32
    }
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

const NO_DISK: &str = "Penyimpanan N/A";

pub struct StatsMonitor {
    sys: System,
    nets: Networks,
    disks: Disks,

    // data statis: dibuat sekali
    host_name: String,
    os_name: String,
    os_version: String,
    kernel_version: String,
    cpu_name: String,
    cpu_cores: usize,
    cpu_freq_mhz: u64,

    // nilai yang berubah lambat: disegarkan tiap beberapa detik, bukan tiap detik
    tick: u32,
    processes: usize,
    disk_used: u64,
    disk_total: u64,
    disk_label: String,
}

impl StatsMonitor {
    /// Hanya memuat CPU + memori; daftar proses diambil seminimal mungkin (lihat `sample`).
    pub fn new() -> Self {
        let mut sys = System::new();
        sys.refresh_cpu_specifics(CpuRefreshKind::everything()); // sekali: nama, jumlah core, frekuensi
        sys.refresh_memory();
        let nets = Networks::new_with_refreshed_list();
        let disks = Disks::new_with_refreshed_list();

        let cpu_list = sys.cpus();
        let cpu_cores = cpu_list.len();
        let cpu_freq_mhz = if cpu_cores > 0 {
            cpu_list.iter().map(|cpu| cpu.frequency()).sum::<u64>() / cpu_cores as u64
        } else {
            0
        };
        let cpu_name = cpu_list
            .first()
            .map(|cpu| cpu.brand().trim().to_string())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "CPU".to_string());

        Self {
            host_name: System::host_name().unwrap_or_else(|| "Perangkat".to_string()),
            os_name: System::name().unwrap_or_else(|| std::env::consts::OS.to_string()),
            os_version: System::os_version().unwrap_or_default(),
            kernel_version: System::kernel_version().unwrap_or_default(),
            cpu_name,
            cpu_cores,
            cpu_freq_mhz,
            sys,
            nets,
            disks,
            tick: 0,
            processes: 0,
            disk_used: 0,
            disk_total: 0,
            disk_label: NO_DISK.to_string(),
        }
    }

    /// Panggil saat pemantauan baru dilanjutkan setelah jeda: membuang selisih selama jeda
    /// (kalau tidak, kecepatan jaringan melonjak palsu) dan mengulang siklus refresh lambat.
    pub fn resume(&mut self) {
        self.sys.refresh_cpu_usage();
        self.nets.refresh(true);
        self.tick = 0;
    }

    /// Ambil satu sampel. CPU dan jaringan memakai selisih antar-panggilan, jadi panggil
    /// dengan jeda tetap (mis. 1 detik) dan pertahankan instance ini.
    pub fn sample(&mut self) -> Stats<'_> {
        self.sys.refresh_cpu_usage();
        self.sys.refresh_memory();
        self.nets.refresh(true);

        // Jumlah proses cukup dihitung tiap 5 dtk. ProcessRefreshKind::nothing() = hanya
        // daftar PID/nama, tanpa cmd, exe, environ, dsb. (yang paling banyak makan memori).
        if self.tick % 5 == 0 {
            self.sys.refresh_processes_specifics(
                ProcessesToUpdate::All,
                true,
                ProcessRefreshKind::nothing(),
            );
            self.processes = self.sys.processes().len();
        }

        // Disk jarang berubah, dan menyegarkannya menyentuh semua volume. Cukup tiap 10 dtk.
        if self.tick % 10 == 0 {
            self.disks.refresh(true);
            self.disk_label.clear();
            if let Some(disk) = primary_disk(&self.disks) {
                self.disk_total = disk.total_space();
                self.disk_used = self.disk_total.saturating_sub(disk.available_space());
                self.disk_label.push_str(&disk.mount_point().to_string_lossy());
            } else {
                self.disk_used = 0;
                self.disk_total = 0;
                self.disk_label.push_str(NO_DISK);
            }
        }
        self.tick = (self.tick + 1) % 10;

        // received()/transmitted() menunjukkan byte sejak refresh jaringan sebelumnya.
        let (mut rx, mut tx) = (0u64, 0u64);
        for (_, network) in self.nets.iter() {
            rx = rx.saturating_add(network.received());
            tx = tx.saturating_add(network.transmitted());
        }

        let ram_total = self.sys.total_memory();
        let ram_used = self.sys.used_memory();
        let swap_total = self.sys.total_swap();
        let swap_used = self.sys.used_swap();

        Stats {
            cpu: self.sys.global_cpu_usage().clamp(0.0, 100.0),
            cpu_cores: self.cpu_cores,
            cpu_freq_mhz: self.cpu_freq_mhz,
            cpu_name: &self.cpu_name,
            ram: percent(ram_used, ram_total),
            ram_used: ram_used as f64,
            ram_total: ram_total as f64,
            swap: percent(swap_used, swap_total),
            swap_used: swap_used as f64,
            swap_total: swap_total as f64,
            disk: percent(self.disk_used, self.disk_total),
            disk_used: self.disk_used as f64,
            disk_total: self.disk_total as f64,
            disk_label: &self.disk_label,
            down: rx as f64,
            up: tx as f64,
            processes: self.processes,
            uptime_secs: System::uptime(),
            host_name: &self.host_name,
            os_name: &self.os_name,
            os_version: &self.os_version,
            kernel_version: &self.kernel_version,
        }
    }
}

impl Default for StatsMonitor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_handles_zero_and_clamps() {
        assert_eq!(percent(5, 0), 0.0);
        assert_eq!(percent(50, 100), 50.0);
        assert_eq!(percent(200, 100), 100.0);
    }

    #[test]
    fn sample_returns_sane_values() {
        let mut m = StatsMonitor::new();
        m.resume();
        std::thread::sleep(std::time::Duration::from_millis(300));
        let s = m.sample();
        assert!((0.0..=100.0).contains(&s.cpu));
        assert!((0.0..=100.0).contains(&s.ram));
        assert!(s.ram_total > 0.0);
        assert!(s.cpu_cores > 0);
        assert!(!s.host_name.is_empty());
    }

    #[test]
    fn serializes_to_same_field_names_as_before() {
        // Frontend (src/main-modules/quick-stats.ts) membaca nama field ini.
        let mut m = StatsMonitor::new();
        let json = serde_json::to_value(m.sample()).unwrap();
        for k in [
            "cpu", "cpu_cores", "cpu_freq_mhz", "cpu_name", "ram", "ram_used", "ram_total", "swap",
            "swap_used", "swap_total", "disk", "disk_used", "disk_total", "disk_label", "down", "up",
            "processes", "uptime_secs", "host_name", "os_name", "os_version", "kernel_version",
        ] {
            assert!(json.get(k).is_some(), "field {k} hilang");
        }
    }
}
