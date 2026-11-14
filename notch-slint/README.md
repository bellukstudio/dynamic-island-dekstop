# notch-slint (prototipe, Windows dulu)

Prototipe tahap 1 untuk menguji apakah Notch bisa dibuat tanpa webview memakai
[Slint](https://slint.dev). Folder ini terpisah dari versi Tauri, jadi `src-tauri/` dan
`src/` tidak diubah.

## Isi prototipe
- Jendela tanpa bingkai, transparan, selalu di atas, di tengah atas layar
- Bentuk notch dengan **bahu cekung** di kiri/kanan atas (menyatu dengan tepi layar)
- Menciut: pil dengan mata yang mengikuti kursor dan berkedip. Hover: melebar jadi hub
- Hub dengan baris ikon tab (ikon digambar dengan `Path`, tanpa decoder gambar/SVG):
  Media (tampilan saja, belum terhubung ke pemutar), Jam, Timer fokus, Stats (CPU/RAM/disk/jaringan
  dari `../notch-core`, hanya disampel saat halaman ini terlihat), dan halaman uji render sementara
  (teks Arab asli + emoji sebagai gambar PNG, lihat `assets/emoji/NOTICE.md`)
- Tema lewat `Theme` global di `ui/notch.slint`. Default hitam polos; tema bergradasi seperti di
  pengaturan versi Tauri belum dibuat, tinggal menambah set nilai `Theme`

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
3. Hover tidak berkedip saat kursor di atas tombol/tab; klik tab dan tombol Mulai/Reset berfungsi.
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
`stats` sudah dipindah ke `../notch-core` (dipakai Tauri dan Slint). Lanjutkan modul lain yang
sekarang menempel ke `tauri::*`: battery, clipboard, notes, reminder, lalu media dan sisanya;
shelf/WhatsApp/mirror paling akhir.
