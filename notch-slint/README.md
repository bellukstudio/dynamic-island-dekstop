# notch-slint (prototipe, Windows dulu)

Prototipe tahap 1 untuk menguji apakah Notch bisa dibuat tanpa webview memakai
[Slint](https://slint.dev). Folder ini terpisah dari versi Tauri, jadi `src-tauri/` dan
`src/` tidak diubah.

## Isi prototipe
- Jendela tanpa bingkai, transparan, selalu di atas, di tengah atas layar, dengan **bahu cekung**
  di kiri/kanan atas yang menyatu dengan tepi layar
- Menciut: pil dengan mata yang mengikuti kursor dan berkedip. Hover melebar jadi hub, dengan jeda
  `timing.hover_open_ms` / `hover_close_ms` dari config
- **Config dibaca dari `%APPDATA%\notch\config.toml`** (hanya dibaca, tidak pernah ditulis):
  - `pages`: tab hub mengikuti daftar dan urutan ini. Halaman yang belum dibuat di Slint tampil
    sebagai placeholder "Belum tersedia di versi Slint". Tambah `"uji"` ke `pages` untuk halaman uji render
  - `[appearance]`: `notch_theme` (default/aurora/obsidian/neon/sakura/matcha), `theme`, `dark_bg`,
    `light_bg`, `background`, `text`, `accent`. Default = hitam polos. `theme = "auto"` mengikuti tema aplikasi Windows
  - `[pomodoro]` dan `[timing]`
- Halaman yang sudah ada: Media (tampilan saja), Jam, Timer (fokus/istirahat/istirahat panjang, ronde,
  progress, reset/skip), Stats, Baterai, Catatan, Quick (Mic/Wi-Fi/Bluetooth), Tools (Keep awake/Tema/Kunci/Pusatkan)
- Stats, Baterai, Quick/Tools, dan Catatan memakai crate bersama `../notch-core`; tiap halaman hanya
  bekerja saat halamannya terlihat
- **Catatan membaca/menulis `%APPDATA%\notch\notes.json`, file yang sama dengan versi Tauri.**
  Jangan mengedit catatan di kedua aplikasi sekaligus: Tauri menyimpan salinan di memori dan
  penulisan terakhir yang menang
- **Quick/Tools mengubah pengaturan Windows sungguhan** (mic, Wi-Fi, Bluetooth, tema gelap, kunci layar)

## Belum ada
Media sungguhan (pemutar, cover, lirik), Cuaca, Jadwal Shalat/Ramadan, Pengingat, Clipboard, Shelf,
Lirik, Privasi, Currency, posisi kustom/dock tepi (tombol "Pusatkan" belum menggeser apa pun),
`scale`, tampilan otomatis saat timer selesai, dan tekstur garis-garis tema neon. Cahaya tema
berbentuk lingkaran, sedangkan versi Tauri lebih lebar (elips).

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
