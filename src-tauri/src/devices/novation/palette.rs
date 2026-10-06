//! The 128 palette colours of the MK3 Launchpads, which the native pulse and
//! flash take (MINI-PRM p12-14), and the entry nearest to a colour.
//!
//! Novation publishes the palette only as a swatch image without numbers
//! (MINI-PRM p11, byte-identical to X-PRM p12), and no MK3 command redefines
//! it. The table is therefore APPROXIMATE: each entry is its swatch's fill
//! colour `s` with the image's grey floor removed,
//! `c = round((s - 0x61) * 255 / 0x9E)` clamped to 0-255. It agrees with the
//! colours the manual names (5 red, 13 yellow, 21 bright and 23 dim green,
//! 37 turquoise, 45 blue, 0 off) but was not measured on LEDs: NOT HARDWARE
//! VERIFIED.

use crate::core::model::Rgb;

/// Entry `i` is palette index `i`; see the module documentation for how the
/// colours were derived.
#[rustfmt::skip]
pub(crate) const PALETTE: [(u8, u8, u8); 128] = [
    (0, 0, 0), (132, 132, 132), (200, 200, 200), (255, 255, 255), (255, 132, 132), (255, 0, 0), (200, 0, 0), (132, 0, 0), // 0-7
    (255, 236, 187), (255, 132, 0), (200, 69, 0), (132, 34, 0), (255, 228, 103), (255, 255, 0), (200, 200, 0), (132, 132, 0), // 8-15
    (200, 255, 103), (157, 255, 0), (103, 200, 0), (52, 132, 0), (157, 255, 132), (0, 255, 0), (0, 200, 0), (0, 132, 0), // 16-23
    (157, 255, 157), (0, 255, 69), (0, 200, 34), (0, 132, 16), (157, 255, 173), (0, 255, 173), (0, 200, 103), (0, 132, 52), // 24-31
    (157, 255, 236), (0, 255, 219), (0, 200, 157), (0, 132, 86), (157, 236, 255), (0, 228, 255), (0, 165, 200), (0, 103, 132), // 32-39
    (157, 200, 255), (0, 165, 255), (0, 103, 200), (0, 52, 132), (103, 69, 255), (0, 0, 255), (0, 0, 200), (0, 0, 132), // 40-47
    (173, 132, 255), (103, 0, 255), (52, 0, 200), (34, 0, 132), (255, 132, 255), (255, 0, 255), (200, 0, 200), (132, 0, 132), // 48-55
    (255, 132, 187), (255, 0, 157), (200, 0, 103), (132, 0, 69), (255, 34, 0), (219, 132, 0), (200, 157, 0), (103, 103, 0), // 56-63
    (0, 132, 0), (0, 132, 69), (0, 69, 187), (0, 0, 255), (0, 132, 132), (69, 0, 236), (173, 132, 157), (69, 34, 52), // 64-71
    (255, 0, 0), (236, 255, 103), (228, 250, 0), (173, 255, 0), (34, 200, 0), (0, 255, 173), (0, 219, 255), (0, 103, 255), // 72-79
    (69, 0, 255), (173, 0, 250), (228, 69, 200), (103, 34, 0), (255, 103, 0), (200, 245, 0), (187, 255, 69), (0, 255, 0), // 80-87
    (132, 255, 103), (173, 250, 187), (132, 255, 240), (173, 211, 255), (103, 157, 240), (187, 157, 245), (245, 69, 255), (255, 0, 173), // 88-95
    (255, 157, 0), (236, 228, 0), (211, 255, 0), (200, 173, 0), (132, 103, 0), (0, 144, 34), (34, 157, 69), (52, 52, 103), // 96-103
    (52, 69, 173), (173, 118, 52), (200, 0, 0), (245, 132, 103), (245, 144, 34), (255, 236, 69), (219, 245, 103), (187, 228, 34), // 104-111
    (52, 52, 103), (245, 245, 187), (200, 250, 211), (219, 219, 255), (211, 187, 255), (132, 132, 132), (187, 187, 187), (245, 255, 255), // 112-119
    (219, 0, 0), (118, 0, 0), (52, 240, 0), (0, 132, 0), (236, 228, 0), (132, 103, 0), (228, 157, 0), (157, 34, 0), // 120-127
];

/// The palette entry that shows `color` best: off only for `Rgb::OFF`,
/// otherwise the entry of 1-127 at the least weighted squared distance
/// `2Δr² + 4Δg² + 3Δb²`, the first one on a tie.
pub(crate) fn nearest(color: Rgb) -> u8 {
    if color == Rgb::OFF {
        return 0;
    }
    (1..=LAST)
        .min_by_key(|&index| distance(color, palette_rgb(index)))
        .unwrap_or(0)
}

/// The last palette index.
const LAST: u8 = 127;

/// `2Δr² + 4Δg² + 3Δb²`: the eye tells green apart best and red least.
fn distance(a: Rgb, b: Rgb) -> u32 {
    let d = |x: u8, y: u8| u32::from(x.abs_diff(y)).pow(2);
    2 * d(a.r, b.r) + 4 * d(a.g, b.g) + 3 * d(a.b, b.b)
}

/// The colour of palette entry `index`; off past the last entry.
pub(crate) fn palette_rgb(index: u8) -> Rgb {
    PALETTE
        .get(usize::from(index))
        .map_or(Rgb::OFF, |&(r, g, b)| Rgb::new(r, g, b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_colours_the_manual_names_are_their_entries() {
        assert_eq!(nearest(Rgb::new(255, 0, 0)), 5, "red");
        assert_eq!(nearest(Rgb::new(255, 255, 0)), 13, "yellow");
        assert_eq!(nearest(Rgb::new(0, 255, 0)), 21, "bright green");
        assert_eq!(nearest(Rgb::new(0, 132, 0)), 23, "dim green");
        assert_eq!(nearest(Rgb::new(0, 228, 255)), 37, "turquoise");
        assert_eq!(nearest(Rgb::new(0, 0, 255)), 45, "blue");
        assert_eq!(nearest(Rgb::OFF), 0);
    }

    #[test]
    fn every_entry_is_nearest_to_its_first_occurrence() {
        for index in 1..=127u8 {
            let colour = palette_rgb(index);
            let first = (1..=127u8).find(|&i| palette_rgb(i) == colour).unwrap();
            assert_eq!(nearest(colour), first, "entry {index}, {colour:?}");
        }
    }

    #[test]
    fn only_off_is_off() {
        for colour in [Rgb::new(1, 1, 1), Rgb::new(0, 0, 1), Rgb::new(3, 0, 0)] {
            assert_ne!(nearest(colour), 0, "{colour:?}");
        }
        assert_eq!(palette_rgb(0), Rgb::OFF);
    }

    #[test]
    fn a_green_error_weighs_more_than_a_red_one() {
        // Plain distance picks 31 (0, 132, 52): 28 off in green, 8 in blue.
        // Weighted, 102 (34, 157, 69) is nearer: 34 off in red, 3 in green.
        assert_eq!(nearest(Rgb::new(0, 160, 60)), 102);
    }

    #[test]
    fn colours_are_read_from_the_table() {
        assert_eq!(palette_rgb(5), Rgb::new(255, 0, 0));
        assert_eq!(palette_rgb(37), Rgb::new(0, 228, 255));
        assert_eq!(palette_rgb(127), Rgb::new(157, 34, 0));
        assert_eq!(palette_rgb(128), Rgb::OFF);
        assert_eq!(palette_rgb(255), Rgb::OFF);
    }
}
