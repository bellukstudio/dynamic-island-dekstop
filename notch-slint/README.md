# notch-slint (prototipe, Windows dulu)

Prototipe tahap 1 untuk menguji apakah Notch bisa dibuat tanpa webview memakai
[Slint](https://slint.dev). Folder ini terpisah dari versi Tauri, jadi `src-tauri/` dan
`src/` tidak diubah.

## Isi prototipe
- Jendela tanpa bingkai, transparan, selalu di atas. **Bentuk tepi mengikuti posisi di config** (`[position]`),
  sama seperti Tauri:
  - menempel di atas (bawaan, atau `y` dekat tepi atas): sisi atas rata, **telinga cekung 10 px**, bawah membulat 32 px
  - melayang (`custom = true`, `y` > 8 px dari tepi atas): keempat sudut membulat 28 px, tanpa telinga, rim keliling
  - menempel di bawah (`y` >= 50000): versi terbalik
  Ukuran hub 580 x 202 px seperti `--hub-w`/`--hub-h` di Tauri. Posisi `x`/`y` (piksel fisik) dijepit ke area kerja monitor
- Menciut: pil dengan mata yang mengikuti kursor dan berkedip. Hover melebar jadi hub, dengan jeda
  `timing.hover_open_ms` / `hover_close_ms` dari config
- **Config dibaca dari `%APPDATA%\notch\config.toml`** (hanya dibaca, tidak pernah ditulis):
  - `pages`: tab hub mengikuti daftar dan urutan ini. Halaman yang belum dibuat di Slint tampil
    sebagai placeholder "Belum tersedia di versi Slint". Tambah `"uji"` ke `pages` untuk halaman uji render
  - `[appearance]`: `notch_theme` (default/aurora/obsidian/neon/sakura/matcha), `theme`, `dark_bg`,
    `light_bg`, `background`, `text`, `accent`. Default = hitam polos. `theme = "auto"` mengikuti tema aplikasi Windows
  - `[pomodoro]` dan `[timing]`
- Halaman yang sudah ada: Media (tampilan saja), Jam, Cuaca (Open-Meteo, kondisi sekarang, terasa, kelembapan,
  dan perkiraan hujan), Timer (fokus/istirahat/istirahat panjang, ronde, progress, reset/skip), Stats, Baterai,
  Catatan, Reminder (tanggal/jam, pengulangan harian/mingguan/bulanan, snooze), Clipboard (10 teks terbaru, salin ulang, hapus riwayat), Quick (Mic/Wi-Fi/Bluetooth), Tools (Keep awake/Tema/Kunci/Pusatkan)
- Stats, Baterai, Quick/Tools, Catatan, Clipboard, Cuaca, dan Reminder memakai crate bersama `../notch-core`; polling UI
  hanya berjalan saat halamannya terlihat. Cuaca memakai Open-Meteo dan diperbarui tiap lima menit saat halaman aktif
- Reminder memakai file `reminders.json` yang sama dengan versi Tauri, termasuk migrasi pengulangan lama,
  mode harian/mingguan/bulanan, pilihan hari mingguan, snooze lima menit, dan alert saat jatuh tempo
- Clipboard mengikuti batas versi Tauri: 10 item teks di memori, maksimum 10.000 karakter per item,
  duplikasi dipindah ke urutan teratas, salin ulang, dan hapus riwayat tanpa mengosongkan clipboard OS
- **Catatan membaca/menulis `%APPDATA%\notch\notes.json`, file yang sama dengan versi Tauri.**
  Jangan mengedit catatan di kedua aplikasi sekaligus: Tauri menyimpan salinan di memori dan
  penulisan terakhir yang menang
- **Hiasan tema (cahaya, garis scan, rim) digambar Rust ke satu gambar RGBA** berukuran hub (`notch-core/src/decor.rs`),
  bukan lewat clip UI, karena renderer software Slint tidak memotong anak elemen mengikuti radius sudut. Gambar ini
  menambah memori kira-kira 0,5 MB (1x DPI) sampai 2 MB (2x) hanya untuk tema selain default
- **Quick/Tools mengubah pengaturan Windows sungguhan** (mic, Wi-Fi, Bluetooth, tema gelap, kunci layar)

## Belum ada
- Media sungguhan (pemutar, cover album, lirik), Jadwal Shalat/Ramadan, Shelf,
  Lirik, Privasi, Currency (halamannya tampil sebagai placeholder)
- Menggeser notch dengan mouse dan dock samping (kiri/kanan). Posisi dibaca dari config; dock samping
  sementara diperlakukan seperti posisi melayang. Tombol "Pusatkan" mengembalikan ke tengah atas hanya untuk sesi ini
- `scale` baru memengaruhi radius dan telinga, belum ukuran seluruh isi UI
- Tampilan otomatis saat timer selesai

## Pratinjau tanpa jendela
```
cargo run --example preview -- preview-out
```
Merender halaman dan tema ke PNG memakai renderer software (data contoh, bukan data sistem). Berguna
untuk memeriksa tata letak di mesin tanpa Windows/layar.

## Menjalankan (Windows)
```
cd notch-slint
cargo run --release
```
Renderer GPU (tidak disarankan, hanya untuk perbandingan):
```
cargo run --release --no-default-features --features femtovg
```

## Yang perlu dicek di Windows
1. Teks Arab tersambung dan arahnya benar? Emoji tampil berwarna?
2. Area transparan di sekitar pil (terutama saat melebar) menangkap klik atau tembus?
3. Hover tidak berkedip saat kursor di atas tombol/tab; klik tab, tombol timer, dan tombol Quick berfungsi.
4. Posisi di layar DPI tinggi (>100%): `place()` memakai piksel fisik.
5. RAM dan CPU idle (Task Manager, kolom "Memory" dan "CPU") dibanding `npm run tauri dev`/rilis Tauri.

## Hasil ukur di Windows
| Renderer | RAM idle | Teks Arab | Emoji teks |
| --- | --- | --- | --- |
| `software` (default) | ~2,4 MB | benar, tersambung | outline putih (tidak berwarna) |
| `femtovg` (GPU) | ~108 MB | benar, tersambung | berwarna |

Keputusan: pakai `software` dan tampilkan emoji sebagai gambar. Konsekuensi: emoji yang dipilih
bebas oleh pengguna (mis. pesan pointer diam) perlu daftar pilihan atau pemetaan ke gambar.

## Status verifikasi
- `cargo build` (Linux) dan `cargo check --target x86_64-pc-windows-gnu` lolos.
- Belum dijalankan di Windows. Perilaku jendela, animasi, dan angka RAM belum terukur.
- Feature `femtovg` belum dikompilasi.

## Rencana berikutnya
`stats`, `battery`, `notes`, `paths`, `config`, `quick`, `pomodoro`, dan `theme` sudah di `../notch-core`
(dipakai Tauri dan/atau Slint). Lanjutkan modul lain yang masih menempel ke `tauri::*`: clipboard, reminder,
weather, prayer, lalu media dan sisanya;
shelf/WhatsApp/mirror paling akhir.
