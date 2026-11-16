# Notch

Notch adalah aplikasi pendamping desktop untuk Windows dan Linux. Sebuah pil kecil di tepi layar membuka hub berisi media, lirik, cuaca, timer, pengingat, catatan, riwayat clipboard, shelf file, dan berbagai kontrol harian.

- **Desktop shell:** Tauri 2
- **Lapisan sistem:** Rust
- **Antarmuka:** TypeScript, Vite, dan CSS
- **Platform:** Windows dan Linux (X11 / Wayland, bergantung pada lingkungan)

Situs proyek: [GitHub Pages](https://bellukchips.github.io/dynamic-island-dekstop/)  
Repositori: [bellukchips/dynamic-island-dekstop](https://github.com/bellukchips/dynamic-island-dekstop)  
Rilis: [Unduh versi aplikasi](https://github.com/bellukchips/dynamic-island-dekstop/releases)

## Fitur

Halaman hub dapat diaktifkan, dinonaktifkan, dan diurutkan melalui Pengaturan atau `config.toml`.

| Halaman / fitur | Keterangan |
| --- | --- |
| `media` | Menampilkan judul, artis, cover album, kontrol pemutaran, dan progress bar. Warna aksen dapat mengikuti cover album. |
| `lyrics` | Lirik tersinkron dengan lagu; baris aktif juga tampil pada notch ringkas. |
| `clock` | Jam dan tanggal. Klik tanggal untuk membuka kalender bulanan dengan hari libur Indonesia. |
| `weather` | Menampilkan suhu, suhu terasa, kelembapan, dan peringatan hujan. |
| `battery` | Menampilkan persentase baterai dan status pengisian, termasuk penanda saat baterai lemah. |
| `timer` | Timer Pomodoro untuk fokus, istirahat singkat, dan istirahat panjang. Durasi dan jumlah ronde dapat diatur. |
| `quick`, `tools` | Kontrol cepat untuk mikrofon, Wi-Fi, Bluetooth, keep-awake, serta tampilan terang/gelap jika didukung platform. |
| `stats` | Pemantauan CPU, RAM, serta kecepatan unduh dan unggah jaringan. |
| `reminder` | Pengingat dengan tanggal, jam, pengulangan harian/bulanan, suara, tunda, dan aksi selesai. |
| `notes` | Catatan cepat permanen dengan jendela detail terpisah. |
| `clipboard` | Riwayat clipboard dengan aksi salin ulang dan hapus. |
| `shelf` | Shelf file sementara. Seret file ke notch, lalu seret kembali ke File Explorer atau aplikasi lain. |
| `prayer` | Jadwal shalat harian, preferensi, kartu pengingat, dan pemutaran audio adzan saat pengingat diaktifkan. |
| `privacy` | Indikator dan kartu ketika mikrofon atau kamera sedang digunakan. |
| `currency` | Konversi Rupiah ke mata uang pilihan serta ringkasan performa bulanan saham pilihan di Bursa Efek Indonesia. Membutuhkan akses jaringan dan sumber data pasar yang tersedia. |

### Perilaku desktop tambahan

- Kartu perangkat ketika Bluetooth, USB, charger, keyboard, atau mouse tersambung maupun dilepas.
- HUD volume dan kecerahan di dalam notch, termasuk status mute.
- Mata mengikuti kursor, berkedip, memiliki delapan ekspresi, dan sepuluh tema: `aurora`, `neon`, `pastel`, `pixel`, `holo`, `ember`, `ocean`, `sakura`, `cyber`, dan `monochrome`.
- Notch dapat diseret ke posisi mana pun. Saat digeser, tampilannya dapat berubah menjadi bola dan menempel pada tepi atas, bawah, kiri, atau kanan layar.
- Gerakan acak yang halus tersedia sebagai opsi ketika fitur gerak diaktifkan.
- Menu tray berisi Pengaturan, Pusatkan Notch, dan Keluar.
- Dukungan mulai otomatis saat login.
- Banner pemberitahuan ketika versi baru tersedia.
- Tema otomatis, terang, gelap, atau kustom; skala antarmuka dari 0,5× sampai 1,5×.
- Navigasi hub dapat digulir secara horizontal ketika banyak halaman diaktifkan.

Integrasi sistem seperti Bluetooth, deteksi perangkat, indikator mikrofon/kamera, dan kecerahan bergantung pada sistem operasi serta izin yang tersedia.

## Prasyarat

| Alat | Kebutuhan |
| --- | --- |
| Rust | Toolchain stable |
| Node.js | Versi 18 atau lebih baru, termasuk npm |
| Git | Untuk clone repositori |
| Dependensi Tauri | Pustaka native yang diperlukan sistem operasi tujuan |

### Windows

1. Pasang [Microsoft C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) dan pilih **Desktop development with C++**.
2. Pastikan **Microsoft Edge WebView2 Runtime** sudah tersedia. Instalasi Windows 10 dan Windows 11 terkini umumnya sudah menyertakannya.
3. Pasang Rust dengan `winget install --id Rustlang.Rustup`, atau gunakan [rustup.rs](https://rustup.rs).
4. Buka terminal baru, lalu cek `rustc --version` dan `cargo --version`.
5. Pilih toolchain MSVC:

   ```powershell
   rustup default stable-msvc
   ```

### Ubuntu / Debian

```bash
sudo apt update
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file \
  libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev \
  libgtk-layer-shell-dev
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
. "$HOME/.cargo/env"
```

### Arch Linux

```bash
sudo pacman -Syu
sudo pacman -S --needed webkit2gtk-4.1 base-devel curl wget file openssl \
  appmenu-gtk-module libappindicator-gtk3 librsvg xdotool gtk-layer-shell
sudo pacman -S rustup
rustup default stable
```

### macOS

Rust dapat dipasang, tetapi integrasi sistem saat ini difokuskan pada Windows dan Linux.

```bash
xcode-select --install
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
. "$HOME/.cargo/env"
```

Untuk distribusi lain dan prasyarat Tauri terbaru, lihat [panduan prasyarat Tauri](https://tauri.app/start/prerequisites/).

## Instalasi dan menjalankan aplikasi

```bash
git clone https://github.com/bellukchips/dynamic-island-dekstop.git
cd dynamic-island-dekstop
npm install
npm install --save-dev @types/node
```

Cargo akan mengunduh dependensi Rust secara otomatis pada build pertama.

Sapaan neural menggunakan klip Edge TTS yang disimpan di `public/greeting/`. Untuk membuat atau
memperbarui klip setelah mengubah teks atau preset suara, siapkan Python dan jalankan:

```bash
python -m pip install edge-tts
npm run greeting:audio
```

Jalankan generator sebelum build rilis agar klip ikut dibundel. Generator mendukung empat preset
neural; teks kustom dan preset suara sistem tetap menggunakan voice Windows.

```bash
npm run tauri dev       # jalankan dengan Vite hot reload
npm run tauri build     # buat paket rilis
```

Hasil build berada di `src-tauri/target/release/`. Installer biasanya tersedia di `src-tauri/target/release/bundle/`.

## Struktur proyek

```text
.
├── index.html                 # jendela notch utama
├── settings.html              # jendela pengaturan
├── note-detail.html           # jendela detail catatan
├── docs/
│   └── index.html             # situs GitHub Pages; tidak masuk build aplikasi
├── vite.config.ts
├── package.json
├── src/                       # antarmuka TypeScript dan modul fitur
└── src-tauri/
    ├── Cargo.toml
    ├── tauri.conf.json
    ├── capabilities/
    └── src/                   # command Rust dan integrasi sistem
```

Vite hanya membundel halaman yang terdaftar di `build.rollupOptions.input` dalam `vite.config.ts`. Situs di `docs/` dipublikasikan terpisah melalui GitHub Pages dan tidak ikut dalam build aplikasi desktop.

## Konfigurasi

Rust membaca `config.toml` lalu menyediakan pengaturan ke antarmuka melalui `window.__CFG__`. Nilai kosong menggunakan nilai bawaan di `main.ts`.

```toml
scale = 1.0
pages = ["media", "clock", "weather", "battery", "timer", "quick", "tools",
         "stats", "reminder", "notes", "clipboard", "shelf", "lyrics",
         "privacy", "prayer", "currency"]

[appearance]
theme = "auto"             # auto, light, dark, custom
background = "#000000"     # digunakan saat theme = "custom"
text = ""                  # kosong = warna teks otomatis
dark_bg = "#000000"
light_bg = "#f5f5f7"
accent = ""                # kosong = mengikuti cover album
default_mood = "normal"    # normal, happy, sleepy, surprised, angry, love, suspicious, sad
eye_theme = "aurora"       # aurora, neon, pastel, pixel, holo, ember, ocean, sakura, cyber, monochrome

[timing]
hub_auto_ms = 4000
card_ms = 3500
hud_ms = 1500
hover_open_ms = 150
hover_close_ms = 450
sleep_after_s = 60

[pomodoro]
focus_min = 25
break_min = 5
long_break_min = 15
rounds = 4
```

Urutan `pages` menentukan urutan halaman di hub. Hapus ID halaman untuk menyembunyikannya atau tambahkan kembali untuk menampilkannya. Perubahan dari Pengaturan diteruskan melalui event `config-changed`. Posisi notch yang diseret disimpan di bagian `[position]` melalui `custom`, `x`, dan `y`.

## Kontrak Rust dan antarmuka

### Command

Command dipanggil dari TypeScript melalui Tauri `invoke`.

| Kelompok | Command |
| --- | --- |
| Jendela | `set_size`, `set_hit_area`, `set_drag_out`, `notch_monitor`, `open_note_detail` |
| Posisi | `notch_start_move`, `notch_move_to`, `notch_end_move`, `notch_reset_position`, `notch_get_position` |
| Media | `media_control`, `media_seek`, `lyrics_fetch` |
| Kontrol cepat | `quick_state`, `quick_toggle` |
| Clipboard | `clip_list`, `clip_copy`, `clip_clear` |
| Shelf | `shelf_list`, `shelf_add`, `shelf_remove`, `shelf_clear`, `shelf_open`, `shelf_reveal`, `shelf_log` |
| Catatan | `notes_list`, `notes_add`, `notes_update`, `notes_remove`, `notes_clear` |
| Pengingat | `reminders_list`, `reminders_add`, `reminders_toggle`, `reminders_remove`, `reminders_clear_done`, `reminders_stop`, `reminders_snooze` |
| Shalat | `prayer_month`, `prayer_info`, `prayer_set_prefs` |
| Konfigurasi | `cfg_get`, `cfg_preview`, `cfg_save`, `cfg_restart`, `nudge_test`, `autostart_get`, `autostart_set`, `bt_list` |
| Currency / data saham | `fetch_yahoo_chart` |

Command harus didaftarkan di `generate_handler![]`, dan jendela pemanggil harus memiliki izin yang sesuai di `src-tauri/capabilities/`.

### Event

Antarmuka mendengarkan event seperti:

`cursor`, `media`, `media-art`, `bluetooth`, `battery`, `device`, `weather`, `hud`, `stats`, `clipboard`, `privacy`, `reminder-due`, `notes-changed`, `note-open`, `position-changed`, dan `config-changed`.

## Shortcut keyboard untuk pengujian manual

| Tombol | Aksi |
| --- | --- |
| `1`–`7` | Ganti ekspresi mata |
| `T` | Ganti tampilan notch antara mata dan jam |
| `K` | Buka kalender |
| `M` | Demo indikator mikrofon/kamera |
| `B` | Tampilkan kartu Bluetooth |
| `C` / `V` / `L` | Demo status baterai normal / mengisi / lemah |
| `D` | Tampilkan kartu perangkat contoh |
| `W` | Tampilkan peringatan cuaca |
| `H` | Buka hub |
| `U` / `I` | Demo HUD volume / kecerahan |
| `P` | Jalankan timer tiga detik |
| `←` / `→` | Ganti halaman atau bulan |

## Pemecahan masalah

### `Cannot find name 'path'` atau `__dirname` di `vite.config.ts`

Pasang tipe Node.js:

```bash
npm install --save-dev @types/node
```

### `Port 1420 is already in use`

Hentikan proses yang sedang menggunakan port 1420, lalu jalankan kembali perintah dev.

### Build Rust pertama gagal di Linux

Pasang paket sistem yang disebutkan dalam pesan error build. Tauri memerlukan WebKitGTK native dan pustaka pengembangan terkait.

### Jendela tidak merespons klik di Windows

Jendela compact dapat memakai perilaku click-through. Deteksi hover menggunakan event posisi kursor global, jadi pastikan event `cursor` dikirim dan diterima.

### Posisi atau klik berbeda di Wayland

Secara bawaan aplikasi berjalan melalui XWayland. Untuk mencoba jalur native layer-shell, jalankan:

```bash
NOTCH_NATIVE=1 npm run tauri dev
```

### OSD volume atau kecerahan Windows masih muncul

OSD Windows dimiliki Explorer dan tidak dapat disembunyikan oleh aplikasi ini. Utilitas terpisah seperti HideVolumeOSD atau ModernFlyouts dapat membantu.

## Skrip yang berguna

```bash
npm run tauri dev
npm run tauri build
cargo check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml
```

## Lisensi

MIT License.
