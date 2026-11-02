//! Satu proses PowerShell bersama untuk semua pemantau Windows
//! (perangkat USB/jack, kecerahan, Bluetooth).
//!
//! Sebelumnya tiap pemantau menjalankan PowerShell sendiri-sendiri; satu proses
//! PowerShell memakai puluhan MB, jadi digabung menjadi satu. Skrip mencetak baris
//! berawalan tag ("H:", "D:", "T:") dan dibagikan ke pelanggan yang sesuai.
use std::io::{BufRead, BufReader};
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

const NO_WINDOW: u32 = 0x0800_0000;
// Prioritas di bawah normal: pemindaian WMI/PnP tidak boleh bersaing dengan aplikasi di depan.
const BELOW_NORMAL: u32 = 0x0000_4000;

// PowerShell yang berjalan lama (apalagi dengan Get-CimInstance / Get-PnpDevice tiap putaran)
// perlahan menggemuk. Proses dimatikan lalu dijalankan ulang berkala supaya memorinya kembali.
const RECYCLE: Duration = Duration::from_secs(30 * 60);

static SUBS: Mutex<Vec<(&'static str, mpsc::Sender<String>)>> = Mutex::new(Vec::new());

// Daftarkan diri SEBELUM start() dipanggil. Baris yang diterima sudah tanpa tag.
pub fn subscribe(tag: &'static str) -> mpsc::Receiver<String> {
    let (tx, rx) = mpsc::channel();
    SUBS.lock().unwrap_or_else(|e| e.into_inner()).push((tag, tx));
    rx
}

// Putaran = 1 dtk (sebelumnya 0,7 dtk). Pemindaian PnP adalah bagian paling mahal, jadi dijarangkan.
// H: kecerahan layar          tiap putaran, hanya dicetak bila berubah (CimSession dipakai ulang)
// D: perangkat USB + jack     tiap 4 putaran (~4 dtk), diakhiri "D:---"
// T: Bluetooth terhubung      tiap 6 putaran (~6 dtk), diakhiri "T:---"
//   {83DA6326-...} 15 = DEVPKEY_Device_IsConnected
//   {104EA319-...} 2  = tingkat baterai perangkat Bluetooth (jika dilaporkan)
const SCRIPT: &str = r##"
$ErrorActionPreference = 'SilentlyContinue'
$ProgressPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$mm = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio\Render'
$conn = '{83DA6326-97A6-4088-9453-A1923F573B29} 15'
$batt = '{104EA319-6EE2-4701-BD47-8DDBF425BBE5} 2'
$cs = $null
try { $cs = New-CimSession } catch { }
$lastBr = -1
$brFail = 0
$brSkip = 0
$i = 0
while ($true) {
  # PC desktop / monitor eksternal tanpa WMI kecerahan: berhenti menanyakan tiap putaran
  if ($brSkip -gt 0) { $brSkip-- } else {
    if ($cs) { $br = (Get-CimInstance -CimSession $cs -Namespace root/WMI -ClassName WmiMonitorBrightness | Select-Object -First 1).CurrentBrightness }
    else { $br = (Get-CimInstance -Namespace root/WMI -ClassName WmiMonitorBrightness | Select-Object -First 1).CurrentBrightness }
    if ($br -ne $null) {
      $brFail = 0
      if ($br -ne $lastBr) { $lastBr = $br; 'H:B|' + $br }
    } else {
      $brFail++
      if ($brFail -ge 3) { $brSkip = 30 }
    }
  }

  if ($i % 4 -eq 0) {
    Get-PnpDevice -PresentOnly | Where-Object { $_.InstanceId -like 'USB\VID_*' } |
      ForEach-Object { 'D:' + $_.InstanceId + '|' + $_.Class + '|' + $_.FriendlyName }

    # jack 3.5mm: endpoint render aktif (DeviceState = 1) bernama headphone/headset
    Get-ChildItem $mm | ForEach-Object {
      $st = (Get-ItemProperty $_.PSPath).DeviceState
      if ($st -eq 1) {
        $p = Get-ItemProperty ($_.PSPath + '\Properties')
        $desc = $p.'{a45c254e-df1c-4efd-8020-67d146a850e0},2'
        $adapter = $p.'{b3f8fa53-0004-438e-9003-51a46e139bfc},6'
        if (($desc -match 'Headphone|Headset|Earphone|Earbud|Handset') -and ($adapter -notmatch 'USB|Bluetooth|Hands-Free')) {
          'D:JACK|' + $desc + ' (' + $adapter + ')'
        }
      }
    }
    'D:---'
  }

  if ($i % 6 -eq 0) {
    Get-PnpDevice -Class Bluetooth -PresentOnly | Where-Object {
      $_.InstanceId -like 'BTHENUM\DEV_*' -or $_.InstanceId -like 'BTHLE\DEV_*'
    } | ForEach-Object {
      $id = $_.InstanceId
      $c = (Get-PnpDeviceProperty -InstanceId $id -KeyName $conn).Data
      if ($c -eq $true) {
        $bl = (Get-PnpDeviceProperty -InstanceId $id -KeyName $batt).Data
        'T:BT|' + $id + '|' + $bl + '|' + $_.FriendlyName
      }
    }
    'T:---'
  }

  $i = ($i + 1) % 12
  if ($i -eq 0) { [GC]::Collect() }
  Start-Sleep -Milliseconds 1000
}
"##;

// true = berhenti karena daur ulang terencana (bukan crash)
fn run() -> bool {
    let subs = SUBS.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let child = Command::new("powershell")
        .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", SCRIPT])
        .creation_flags(NO_WINDOW | BELOW_NORMAL)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let Ok(mut child) = child else {
        eprintln!("winps: PowerShell gagal dijalankan");
        return false;
    };
    let Some(out) = child.stdout.take() else {
        let _ = child.kill();
        return false;
    };

    let child = Arc::new(Mutex::new(child));
    let recycled = Arc::new(AtomicBool::new(false));
    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    {
        let child = Arc::clone(&child);
        let recycled = Arc::clone(&recycled);
        std::thread::spawn(move || {
            // Timeout = masih jalan setelah RECYCLE -> matikan. Disconnected = run() sudah selesai, thread keluar.
            if let Err(mpsc::RecvTimeoutError::Timeout) = stop_rx.recv_timeout(RECYCLE) {
                recycled.store(true, Ordering::SeqCst);
                if let Ok(mut c) = child.lock() {
                    let _ = c.kill();
                }
            }
        });
    }

    for line in BufReader::new(out).lines().map_while(Result::ok) {
        if let Some((tag, rest)) = line.split_once(':') {
            for (t, tx) in &subs {
                if *t == tag {
                    let _ = tx.send(rest.to_string());
                }
            }
        }
    }
    drop(stop_tx);
    if let Ok(mut c) = child.lock() {
        let _ = c.kill();
        let _ = c.wait();
    }
    recycled.load(Ordering::SeqCst)
}

// Panggil SETELAH semua modul mendaftar lewat subscribe().
pub fn start() {
    std::thread::spawn(|| loop {
        let planned = run();
        // daur ulang terencana: segera jalan lagi; kalau berhenti sendiri: coba lagi nanti
        std::thread::sleep(Duration::from_secs(if planned { 1 } else { 10 }));
    });
}