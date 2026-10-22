//! Fallout 1 CE palette data from plib/color/color.cc.
//!
//! The native engine uses original 6-bit RGB channels and an embedded
//! 5:5:5 RGB-to-index lookup table. Rendering/gamma parity is not yet done.

use std::io;

pub const COLORS: usize = 256;
pub const CHANNELS: usize = 3;
pub const VGA_BYTES: usize = COLORS * CHANNELS;
pub const LOOKUP_BYTES: usize = 32768;

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

/// Original VGA palette and 15-bit RGB nearest-palette-index table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Palette {
    pub colors: [[u8; CHANNELS]; COLORS],
    pub mapped: [bool; COLORS],
    rgb15_to_index: Vec<u8>,
}

impl Palette {
    pub fn decode(bytes: &[u8]) -> io::Result<Self> {
        if bytes.len() < VGA_BYTES + LOOKUP_BYTES {
            return Err(invalid("truncated Fallout color.pal"));
        }
        let mut colors = [[0_u8; CHANNELS]; COLORS];
        let mut mapped = [false; COLORS];
        for (index, color) in colors.iter_mut().enumerate() {
            let start = index * CHANNELS;
            color.copy_from_slice(&bytes[start..start + CHANNELS]);
            if color.iter().all(|v| *v <= 0x3F) {
                mapped[index] = true;
            } else {
                *color = [0; CHANNELS];
            }
        }
        Ok(Self {
            colors,
            mapped,
            rgb15_to_index: bytes[VGA_BYTES..VGA_BYTES + LOOKUP_BYTES].to_vec(),
        })
    }

    /// Exact preserved six-bit channel values used by the original palette.
    pub fn rgb6(&self, index: u8) -> [u8; 3] {
        self.colors[usize::from(index)]
    }

    /// Presentation-only conversion; CE gamma and lighting are not applied.
    pub fn rgb8_preview(&self, index: u8) -> [u8; 3] {
        self.rgb6(index).map(|value| {
            let scaled = (u16::from(value) * 255 + 31) / 63;
            scaled as u8
        })
    }

    pub fn rgb555_to_index(&self, rgb555: u16) -> u8 {
        self.rgb15_to_index[usize::from(rgb555 & 0x7FFF)]
    }

    /// CE colorGamma: apply pow(component, gamma), clamp to the original
    /// 0..63 VGA range, and truncate to an integer. The SDL display stage
    /// and gamma-adjusted animated transitions remain separate.
    pub fn gamma_adjusted(&self, gamma: f64) -> io::Result<[[u8; 3]; COLORS]> {
        if !gamma.is_finite() || gamma < 0.0 {
            return Err(invalid("gamma must be finite and nonnegative"));
        }
        let mut lut = [0_u8; 64];
        for (i, entry) in lut.iter_mut().enumerate() {
            *entry = (i as f64).powf(gamma).clamp(0.0, 63.0) as u8;
        }
        Ok(std::array::from_fn(|index| {
            self.colors[index].map(|channel| lut[channel as usize])
        }))
    }
}

/// CE fadeSystemPalette interpolates all 768 original 6-bit channels with
/// integer division (truncating toward zero). This is one transition step;
/// the game's actual frame throttling and sound updates are not yet ported.
pub fn fade_step(
    start: &[[u8; 3]; COLORS],
    target: &[[u8; 3]; COLORS],
    step: i32,
    steps: i32,
) -> io::Result<[[u8; 3]; COLORS]> {
    if steps <= 0 || !(0..=steps).contains(&step) {
        return Err(invalid("invalid palette fade step"));
    }
    Ok(std::array::from_fn(|i| {
        std::array::from_fn(|c| {
            let initial = i32::from(start[i][c]);
            let final_value = i32::from(target[i][c]);
            (initial - (initial - final_value) * step / steps) as u8
        })
    }))
}

#[cfg(test)]
mod tests {
    use super::{Palette, LOOKUP_BYTES, VGA_BYTES};

    fn fixture() -> Vec<u8> {
        let mut bytes = vec![0; VGA_BYTES + LOOKUP_BYTES];
        bytes[0..3].copy_from_slice(&[63, 31, 0]);
        bytes[3..6].copy_from_slice(&[255, 3, 3]);
        bytes[VGA_BYTES + 0x7FFF] = 31;
        bytes
    }

    #[test]
    fn preserves_six_bit_values_and_rgb555_lookup() {
        let palette = Palette::decode(&fixture()).unwrap();
        assert_eq!(palette.rgb6(0), [63, 31, 0]);
        assert_eq!(palette.rgb8_preview(0), [255, 125, 0]);
        assert!(palette.mapped[0]);
        assert_eq!(palette.rgb555_to_index(0x7FFF), 31);
    }

    #[test]
    fn marks_out_of_range_color_unmapped_and_black() {
        let palette = Palette::decode(&fixture()).unwrap();
        assert!(!palette.mapped[1]);
        assert_eq!(palette.rgb6(1), [0, 0, 0]);
    }

    #[test]
    fn rejects_truncated_palettes() {
        assert!(Palette::decode(&fixture()[..VGA_BYTES]).is_err());
    }

    #[test]
    fn gamma_lut_matches_original_six_bit_power_behavior() {
        let source = fixture();
        let palette = Palette::decode(&source).unwrap();
        let identity = palette.gamma_adjusted(1.0).unwrap();
        assert_eq!(identity[0], [63, 31, 0]);
        let square = palette.gamma_adjusted(2.0).unwrap();
        assert_eq!(square[0], [63, 63, 0]);
        assert!(palette.gamma_adjusted(f64::NAN).is_err());
        assert!(palette.gamma_adjusted(-1.0).is_err());
    }

    #[test]
    fn integer_fade_steps_follow_ce_truncation_rules() {
        let mut start = [[0; 3]; super::COLORS];
        let mut target = [[0; 3]; super::COLORS];
        start[1] = [63, 0, 15];
        target[1] = [0, 63, 45];
        assert_eq!(super::fade_step(&start, &target, 0, 3).unwrap(), start);
        assert_eq!(
            super::fade_step(&start, &target, 1, 3).unwrap()[1],
            [42, 21, 25]
        );
        assert_eq!(super::fade_step(&start, &target, 3, 3).unwrap(), target);
        assert!(super::fade_step(&start, &target, 1, 0).is_err());
    }
}
