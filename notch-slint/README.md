# notch-slint (prototipe, Windows dulu)

Prototipe tahap 1 untuk menguji apakah Notch bisa dibuat tanpa webview memakai
[Slint](https://slint.dev). Folder ini terpisah dari versi Tauri, jadi `src-tauri/` dan
`src/` tidak diubah.

## Isi prototipe
- Jendela tanpa bingkai, transparan, selalu di atas, di tengah atas layar
- Pil yang melebar saat hover (animasi di dalam, jendela di-resize dari Rust)
- Mata yang mengikuti kursor (`GetCursorPos`, ~20 fps, hanya update jika kursor bergerak) dan berkedip
- Kartu jam/tanggal + timer fokus 25 menit (Mulai/Jeda/Reset)
- Baris CPU/RAM dari crate bersama `../notch-core` (hanya disampel saat notch melebar)
- Uji render: teks Arab (teks asli) dan emoji (sebagai gambar PNG 48 px, lihat `assets/emoji/NOTICE.md`)

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
2. Area transparan di sekitar pil menangkap klik/hover atau tembus?
3. Posisi di layar DPI tinggi (>100%): `place()` memakai piksel fisik.
4. RAM dan CPU idle (Task Manager, kolom "Memory" dan "CPU") dibanding `npm run tauri dev`/rilis Tauri.

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
`stats` sudah dipindah ke `../notch-core` (dipakai Tauri dan Slint). Lanjutkan modul lain yang
sekarang menempel ke `tauri::*`: battery, clipboard, notes, reminder, lalu media dan sisanya;
shelf/WhatsApp/mirror paling akhir.
