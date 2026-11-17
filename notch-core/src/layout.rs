//! Penempatan jendela notch di layar dan mode tepinya. Aturannya meniru
//! `apply_notch_position_for` (src-tauri/src/lib.rs) dan `updatePositionUI` (window-drag-orb.ts).
//! Fungsi murni: area kerja monitor diberikan pemanggil.

/// `Position.y` sebesar ini (>= setengahnya) berarti "menempel di dasar layar".
pub const BOTTOM_TY: i32 = 100_000;
/// Jarak ke tepi atas (px) di bawah ini masih dianggap menempel di atas.
pub const TOP_SNAP: i32 = 8;
/// Jarak aman antara Notch dock bawah dan batas atas taskbar Windows.
pub const BOTTOM_GAP: i32 = 0;
/// Sentinel posisi kustom untuk docking vertikal. `x` menyimpan sisi, `y` menyimpan pusat vertikal.
pub const LEFT_TX: i32 = -100_000;
pub const RIGHT_TX: i32 = 100_000;

/// Area kerja monitor dalam piksel fisik: horizontal = lebar monitor penuh, vertikal = di luar taskbar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkArea {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// Bentuk notch: menentukan sudut mana yang membulat dan ke mana "telinga" cekung mengarah.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dock {
    /// Menempel di tepi atas: bawah membulat, telinga di atas.
    Top,
    /// Digeser menjauh dari tepi: keempat sudut membulat, tanpa telinga.
    Floating,
    /// Menempel di dasar layar: atas membulat, telinga di bawah.
    Bottom,
    /// Menempel di sisi kiri: sisi kiri rata, mata disusun vertikal.
    Left,
    /// Menempel di sisi kanan: sisi kanan rata, mata disusun vertikal.
    Right,
}

/// Hitung pojok kiri-atas jendela dan modenya.
/// `pos` = posisi kustom `(center_x, top_y)` dari config, atau `None` untuk bawaan (tengah atas).
pub fn place(work: &WorkArea, width: i32, height: i32, pos: Option<(i32, i32)>) -> ((i32, i32), Dock) {
    match pos {
        None => ((work.x + (work.w - width) / 2, work.y), Dock::Top),
        Some((LEFT_TX, cy)) => {
            let x = work.x;
            let min_y = work.y + height / 2;
            let max_y = (work.y + work.h - height / 2).max(min_y);
            ( (x, cy.clamp(min_y, max_y) - height / 2), Dock::Left )
        }
        Some((RIGHT_TX, cy)) => {
            let x = work.x + (work.w - width).max(0);
            let min_y = work.y + height / 2;
            let max_y = (work.y + work.h - height / 2).max(min_y);
            ( (x, cy.clamp(min_y, max_y) - height / 2), Dock::Right )
        }
        Some((cx, ty)) => {
            let half_w = width / 2;
            let min_x = work.x + half_w;
            let max_x = (work.x + work.w - half_w).max(min_x);
            let min_y = work.y;
            let bottom_gap = if ty >= BOTTOM_TY / 2 { BOTTOM_GAP } else { 0 };
            let max_y = (work.y + work.h - height - bottom_gap).max(min_y);
            let x = cx.clamp(min_x, max_x) - half_w;
            let y = ty.clamp(min_y, max_y);
            let dock = if ty >= BOTTOM_TY / 2 {
                Dock::Bottom
            } else if y - work.y > TOP_SNAP {
                Dock::Floating
            } else {
                Dock::Top
            };
            ((x, y), dock)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: WorkArea = WorkArea { x: 0, y: 0, w: 1920, h: 1040 }; // 1080 dikurangi taskbar 40

    #[test]
    fn default_is_top_center() {
        assert_eq!(place(&W, 600, 300, None), ((660, 0), Dock::Top));
    }

    #[test]
    fn default_respects_work_area_origin() {
        let w = WorkArea { x: 1920, y: 48, w: 2560, h: 1392 }; // monitor kedua, taskbar di atas
        assert_eq!(place(&w, 600, 300, None), ((1920 + 980, 48), Dock::Top));
    }

    #[test]
    fn custom_position_is_floating_when_far_from_top() {
        let ((x, y), d) = place(&W, 600, 300, Some((960, 120)));
        assert_eq!((x, y), (660, 120));
        assert_eq!(d, Dock::Floating);
    }

    #[test]
    fn near_top_edge_stays_docked() {
        assert_eq!(place(&W, 600, 300, Some((960, 8))).1, Dock::Top);
        assert_eq!(place(&W, 600, 300, Some((960, 9))).1, Dock::Floating);
        assert_eq!(place(&W, 600, 300, Some((960, 0))).1, Dock::Top);
    }

    #[test]
    fn bottom_sentinel_sticks_to_bottom_of_work_area() {
        let ((_, y), d) = place(&W, 600, 300, Some((960, BOTTOM_TY)));
        assert_eq!(d, Dock::Bottom);
        assert_eq!(y, 1040 - 300 - BOTTOM_GAP);
    }

    #[test]
    fn side_docking_uses_vertical_center_and_screen_edge() {
        assert_eq!(place(&W, 300, 262, Some((LEFT_TX, 520))), ((0, 389), Dock::Left));
        assert_eq!(place(&W, 300, 262, Some((RIGHT_TX, 520))), ((1620, 389), Dock::Right));
        assert_eq!(place(&W, 300, 262, Some((LEFT_TX, -500))), ((0, 0), Dock::Left));
    }

    #[test]
    fn side_docking_respects_negative_monitor_origin() {
        let left_monitor = WorkArea { x: -1600, y: 0, w: 1600, h: 900 };
        assert_eq!(
            place(&left_monitor, 300, 262, Some((LEFT_TX, 450))),
            ((-1600, 319), Dock::Left)
        );
        assert_eq!(
            place(&left_monitor, 300, 262, Some((RIGHT_TX, 450))),
            ((-300, 319), Dock::Right)
        );
    }

    #[test]
    fn clamps_inside_work_area() {
        // terlalu kiri/kanan
        assert_eq!(place(&W, 600, 300, Some((0, 50))).0 .0, 0);
        assert_eq!(place(&W, 600, 300, Some((5000, 50))).0 .0, 1920 - 600);
        // terlalu atas (di atas area kerja) -> menempel ke atas
        let ((_, y), d) = place(&W, 600, 300, Some((960, -50)));
        assert_eq!((y, d), (0, Dock::Top));
    }

    #[test]
    fn window_larger_than_work_area_does_not_panic() {
        let tiny = WorkArea { x: 0, y: 0, w: 400, h: 200 };
        // min_x (= 300) melebihi max_x (= 100), jadi dijepit ke min_x -> jendela mulai di (0, 0)
        let ((x, y), _) = place(&tiny, 600, 300, Some((200, 10)));
        assert_eq!((x, y), (0, 0));
    }
}
