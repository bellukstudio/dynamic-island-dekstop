//! Persegi yang tetap terlihat saat notch menciut di dalam jendela yang TIDAK diperkecil.
//!
//! Mengubah ukuran jendela native membuat Windows menampilkan frame lama di koordinat baru ("notch ganda").
//! Bila jendela dibiarkan seukuran hub dan hanya persegi notch yang ditampilkan (SetWindowRgn), tidak ada
//! perubahan ukuran, jadi tidak ada frame basi dan notch tidak perlu disembunyikan dulu.

/// Persegi `(x, y, lebar, tinggi)` dalam koordinat jendela besar yang persis menutupi jendela menciut.
///
/// `origin`/`size`: posisi layar dan ukuran jendela menciut (piksel fisik); `big_origin`/`big_size`: jendela saat ini.
/// Hanya untuk notch yang menempel di tepi ATAS jendela (dock atas/melayang). `None` bila posisi notch di jendela
/// besar tidak sama dengan di jendela menciut (jendela besar terjepit tepi layar, atau ukuran tidak masuk);
/// pemanggil lalu mengecilkan jendela seperti biasa.
pub fn collapsed_rect(
    origin: (i32, i32),
    size: (i32, i32),
    big_origin: (i32, i32),
    big_size: (i32, i32),
) -> Option<(i32, i32, i32, i32)> {
    let (dx, dy) = (origin.0 - big_origin.0, origin.1 - big_origin.1);
    let (w, h) = size;
    let (bw, bh) = big_size;
    // Slint menggambar notch rata tengah horizontal dan menempel di tepi atas jendela.
    let centered = (2 * dx + w - bw).abs() <= 2;
    let aligned = dy.abs() <= 1;
    let fits = w > 0 && h > 0 && dx >= 0 && dy >= 0 && dx + w <= bw && dy + h <= bh;
    (centered && aligned && fits).then_some((dx, dy, w, h))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn top_centered_notch_is_clipped_to_its_own_rect() {
        // Jendela hub 780 lebar di x=570, jendela menciut 300 lebar di x=810 (keduanya berpusat di 960).
        assert_eq!(collapsed_rect((810, 0), (300, 140), (570, 0), (780, 430)), Some((240, 0, 300, 140)));
    }

    #[test]
    fn same_size_window_clips_nothing_extra() {
        assert_eq!(collapsed_rect((100, 0), (300, 140), (100, 0), (300, 140)), Some((0, 0, 300, 140)));
    }

    #[test]
    fn big_window_pushed_by_screen_edge_is_rejected() {
        // Hub besar terjepit tepi kiri layar: pusatnya tidak lagi sama dengan pusat notch menciut.
        assert_eq!(collapsed_rect((10, 0), (300, 140), (0, 0), (780, 430)), None);
    }

    #[test]
    fn floating_window_pushed_down_by_screen_bottom_is_rejected() {
        // Jendela besar digeser ke atas agar muat, sehingga notch di bagian atasnya tidak sejajar lagi.
        assert_eq!(collapsed_rect((810, 600), (300, 140), (570, 400), (780, 430)), None);
    }



    #[test]
    fn collapsed_larger_than_window_is_rejected() {
        assert_eq!(collapsed_rect((0, 0), (300, 140), (0, 0), (200, 100)), None);
    }
}
