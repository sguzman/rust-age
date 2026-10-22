//! CPU-side 8-bit indexed framebuffer compatible with Fallout 1's
//! original transparent/opaque blit semantics.
//!
//! Reference: Fallout 1 CE src/plib/gnw/grbuf.cc and game/art.cc.
//! Frame index 0 is transparent for trans_buf_to_buf; ordinary copies
//! preserve zero. Layer ordering, lighting, roofs and display are not ported.

use crate::frm::Frame;
use crate::palette::Palette;
use std::io::{self, Write};

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedSurface {
    width: usize,
    height: usize,
    pixels: Vec<u8>,
}

impl IndexedSurface {
    pub fn new(width: usize, height: usize) -> io::Result<Self> {
        let size = width
            .checked_mul(height)
            .filter(|&size| size <= 64 * 1024 * 1024)
            .ok_or_else(|| invalid("invalid indexed-surface dimensions"))?;
        Ok(Self {
            width,
            height,
            pixels: vec![0; size],
        })
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    pub fn indices(&self) -> &[u8] {
        &self.pixels
    }

    /// Opaque rectangle fill; clipped to the destination, including negative origins.
    pub fn fill_rect(&mut self, x: i32, y: i32, width: usize, height: usize, color: u8) {
        let left = (x as i64).max(0) as usize;
        let top = (y as i64).max(0) as usize;
        let right = (x as i64 + width as i64).min(self.width as i64).max(0) as usize;
        let bottom = (y as i64 + height as i64).min(self.height as i64).max(0) as usize;
        for row in top..bottom {
            self.pixels[row * self.width + left..row * self.width + right].fill(color);
        }
    }

    /// Copy one original FRM frame into an 8-bit framebuffer. If transparent,
    /// index 0 leaves the background unchanged, as CE's trans_buf_to_buf.
    ///
    /// This does not apply FRM motion offsets: the caller determines the
    /// placement and direction, matching the original art engine's separation.
    pub fn blit_frame(
        &mut self,
        frame: &Frame,
        x: i32,
        y: i32,
        transparent: bool,
    ) -> io::Result<()> {
        let source_width = usize::from(frame.width);
        let source_height = usize::from(frame.height);
        let expected = source_width
            .checked_mul(source_height)
            .ok_or_else(|| invalid("FRM frame dimension overflow"))?;
        if frame.pixels.len() != expected {
            return Err(invalid("FRM pixel count differs from frame dimensions"));
        }
        // All frame dimensions are u16, hence x/y plus widths fit i64.
        let dx0 = (x as i64).max(0);
        let dy0 = (y as i64).max(0);
        let dx1 = (x as i64 + source_width as i64).min(self.width as i64);
        let dy1 = (y as i64 + source_height as i64).min(self.height as i64);
        if dx1 <= dx0 || dy1 <= dy0 {
            return Ok(());
        }
        for dy in dy0..dy1 {
            let sy = (dy - y as i64) as usize;
            let dx = dx0 as usize;
            let sx = (dx0 - x as i64) as usize;
            let count = (dx1 - dx0) as usize;
            let src = &frame.pixels[sy * source_width + sx..sy * source_width + sx + count];
            let dst = &mut self.pixels
                [dy as usize * self.width + dx..dy as usize * self.width + dx + count];
            if transparent {
                for (source, target) in src.iter().zip(dst.iter_mut()) {
                    if *source != 0 {
                        *target = *source;
                    }
                }
            } else {
                dst.copy_from_slice(src);
            }
        }
        Ok(())
    }

    /// Save an RGB PPM diagnostic from game-owned indexed pixels.
    ///
    /// No original art or palettes are shipped; output is generated locally
    /// from the player's installation. This is not CE's gamma/fade pipeline.
    pub fn write_ppm(&self, palette: &Palette, mut output: impl Write) -> io::Result<()> {
        write!(output, "P6\n{} {}\n255\n", self.width, self.height)?;
        let mut row = vec![0_u8; self.width * 3];
        for y in 0..self.height {
            for x in 0..self.width {
                let rgb = palette.rgb8_preview(self.pixels[y * self.width + x]);
                let offset = x * 3;
                row[offset..offset + 3].copy_from_slice(&rgb);
            }
            output.write_all(&row)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::IndexedSurface;
    use crate::frm::Frame;
    use crate::palette::{Palette, LOOKUP_BYTES, VGA_BYTES};

    fn frame(width: u16, height: u16, pixels: &[u8]) -> Frame {
        Frame {
            width,
            height,
            x: 0,
            y: 0,
            pixels: pixels.to_vec(),
        }
    }

    #[test]
    fn transparent_zero_preserves_background_but_opaque_zero_overwrites() {
        let mut screen = IndexedSurface::new(4, 3).unwrap();
        screen.fill_rect(0, 0, 4, 3, 8);
        screen
            .blit_frame(&frame(3, 2, &[0, 1, 0, 2, 0, 3]), 1, 1, true)
            .unwrap();
        assert_eq!(screen.indices(), &[8, 8, 8, 8, 8, 8, 1, 8, 8, 2, 8, 3]);
        screen
            .blit_frame(&frame(2, 1, &[0, 4]), 0, 0, false)
            .unwrap();
        assert_eq!(&screen.indices()[0..4], &[0, 4, 8, 8]);
    }

    #[test]
    fn negative_origin_clips_without_wrapping() {
        let mut screen = IndexedSurface::new(3, 2).unwrap();
        screen
            .blit_frame(&frame(3, 2, &[1, 2, 3, 4, 5, 6]), -1, -1, false)
            .unwrap();
        assert_eq!(screen.indices(), &[5, 6, 0, 0, 0, 0]);
        screen.fill_rect(-2, -1, 3, 2, 7);
        assert_eq!(screen.indices(), &[7, 6, 0, 0, 0, 0]);
        screen.fill_rect(200, 0, 10, 2, 9);
        assert_eq!(screen.indices(), &[7, 6, 0, 0, 0, 0]);
    }

    #[test]
    fn rejects_dimension_mismatch_and_overflow() {
        let mut screen = IndexedSurface::new(1, 1).unwrap();
        assert!(screen.blit_frame(&frame(2, 2, &[1]), 0, 0, true).is_err());
        assert!(IndexedSurface::new(usize::MAX, 2).is_err());
    }

    #[test]
    fn writes_ppm_with_indexed_palette() {
        let mut colors = vec![0; VGA_BYTES + LOOKUP_BYTES];
        colors[3..6].copy_from_slice(&[63, 0, 0]);
        let palette = Palette::decode(&colors).unwrap();
        let mut screen = IndexedSurface::new(2, 1).unwrap();
        screen
            .blit_frame(&frame(2, 1, &[1, 0]), 0, 0, false)
            .unwrap();
        let mut bytes = Vec::new();
        screen.write_ppm(&palette, &mut bytes).unwrap();
        assert_eq!(bytes, b"P6\n2 1\n255\n\xFF\x00\x00\x00\x00\x00");
    }
}
