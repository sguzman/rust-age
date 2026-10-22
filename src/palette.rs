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
}
