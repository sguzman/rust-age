//! Fallout 1 CE FRM sprite data. Original game assets remain external.
//!
//! Source reference: src/game/art.cc (art_readFrameData,
//! art_readSubFrameData) at the pinned CE revision. This is a pixel-data
//! decoder, NOT a renderer or proof of visual parity.

use std::collections::BTreeMap;
use std::io::{self, Cursor, Read};

const HEADER_BYTES: usize = 62;
const DIRECTIONS: usize = 6;
const MAX_FRAME_BYTES: usize = 128 * 1024 * 1024;

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn i16be(input: &mut impl Read) -> io::Result<i16> {
    let mut bytes = [0_u8; 2];
    input.read_exact(&mut bytes)?;
    Ok(i16::from_be_bytes(bytes))
}

fn i32be(input: &mut impl Read) -> io::Result<i32> {
    let mut bytes = [0_u8; 4];
    input.read_exact(&mut bytes)?;
    Ok(i32::from_be_bytes(bytes))
}

/// One animation frame's palette indices (not RGB).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub width: u16,
    pub height: u16,
    pub x: i16,
    pub y: i16,
    pub pixels: Vec<u8>,
}

/// One FRM file with six directional animation sequences.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frm {
    pub version: i32,
    pub frames_per_second: i16,
    pub action_frame: i16,
    pub frame_count: usize,
    pub x_offsets: [i16; DIRECTIONS],
    pub y_offsets: [i16; DIRECTIONS],
    pub frames: [Vec<Frame>; DIRECTIONS],
}

impl Frm {
    pub fn decode(bytes: &[u8]) -> io::Result<Self> {
        if bytes.len() < HEADER_BYTES {
            return Err(invalid("truncated FRM header"));
        }
        let mut stream = Cursor::new(bytes);
        let version = i32be(&mut stream)?;
        let frames_per_second = i16be(&mut stream)?;
        let action_frame = i16be(&mut stream)?;
        let frame_count = i16be(&mut stream)?;
        if !(0..=4096).contains(&frame_count) {
            return Err(invalid("invalid FRM frame count"));
        }
        let frame_count = frame_count as usize;

        let mut x_offsets = [0; DIRECTIONS];
        let mut y_offsets = [0; DIRECTIONS];
        let mut direction_offsets = [0_i32; DIRECTIONS];
        for offset in &mut x_offsets {
            *offset = i16be(&mut stream)?;
        }
        for offset in &mut y_offsets {
            *offset = i16be(&mut stream)?;
        }
        for offset in &mut direction_offsets {
            *offset = i32be(&mut stream)?;
        }
        let data_size = i32be(&mut stream)?;
        if data_size < 0 || bytes.len() - HEADER_BYTES < data_size as usize {
            return Err(invalid("invalid FRM declared data size"));
        }
        let data_size = data_size as usize;
        let mut unique_rotations: BTreeMap<i32, Vec<Frame>> = BTreeMap::new();
        let mut frames: [Vec<Frame>; DIRECTIONS] = std::array::from_fn(|_| Vec::new());

        for (index, offset) in direction_offsets.iter().copied().enumerate() {
            if offset < 0 || offset as usize > data_size {
                return Err(invalid("FRM rotation offset out of bounds"));
            }
            if let Some(existing) = unique_rotations.get(&offset) {
                frames[index] = existing.clone();
                continue;
            }
            let mut current = Vec::with_capacity(frame_count);
            stream.set_position((HEADER_BYTES + offset as usize) as u64);
            for _ in 0..frame_count {
                if stream.position() as usize + 12 > HEADER_BYTES + data_size {
                    return Err(invalid("truncated FRM subframe header"));
                }
                let width = i16be(&mut stream)?;
                let height = i16be(&mut stream)?;
                let length = i32be(&mut stream)?;
                let x = i16be(&mut stream)?;
                let y = i16be(&mut stream)?;
                if width < 0 || height < 0 || length < 0 || length as usize > MAX_FRAME_BYTES {
                    return Err(invalid("invalid FRM subframe dimensions"));
                }
                let length = length as usize;
                if length > HEADER_BYTES + data_size - stream.position() as usize {
                    return Err(invalid("truncated FRM pixel data"));
                }
                let mut pixels = vec![0; length];
                stream.read_exact(&mut pixels)?;
                current.push(Frame {
                    width: width as u16,
                    height: height as u16,
                    x,
                    y,
                    pixels,
                });
            }
            unique_rotations.insert(offset, current.clone());
            frames[index] = current;
        }
        Ok(Self {
            version,
            frames_per_second,
            action_frame,
            frame_count,
            x_offsets,
            y_offsets,
            frames,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::Frm;

    fn fixture() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&4_i32.to_be_bytes());
        bytes.extend_from_slice(&10_i16.to_be_bytes());
        bytes.extend_from_slice(&0_i16.to_be_bytes());
        bytes.extend_from_slice(&1_i16.to_be_bytes());
        bytes.extend_from_slice(&[0; 24]);
        bytes.extend_from_slice(&[0; 24]);
        bytes.extend_from_slice(&15_i32.to_be_bytes());
        bytes.extend_from_slice(&3_i16.to_be_bytes());
        bytes.extend_from_slice(&1_i16.to_be_bytes());
        bytes.extend_from_slice(&3_i32.to_be_bytes());
        bytes.extend_from_slice(&(-2_i16).to_be_bytes());
        bytes.extend_from_slice(&1_i16.to_be_bytes());
        bytes.extend_from_slice(&[7, 8, 9]);
        bytes
    }

    #[test]
    fn reads_frames_and_six_shared_rotations() {
        let sprite = Frm::decode(&fixture()).unwrap();
        assert_eq!(sprite.version, 4);
        assert_eq!(sprite.frames_per_second, 10);
        assert_eq!(sprite.frame_count, 1);
        assert_eq!(sprite.frames.len(), 6);
        for direction in &sprite.frames {
            assert_eq!(direction.len(), 1);
            assert_eq!(direction[0].width, 3);
            assert_eq!(direction[0].height, 1);
            assert_eq!(direction[0].x, -2);
            assert_eq!(direction[0].pixels, [7, 8, 9]);
        }
    }

    #[test]
    fn rejects_truncated_header_pixels_and_bad_offsets() {
        assert!(Frm::decode(&fixture()[..30]).is_err());
        let mut truncated = fixture();
        truncated.pop();
        assert!(Frm::decode(&truncated).is_err());
        let mut offset = fixture();
        offset[34..38].copy_from_slice(&500_i32.to_be_bytes());
        assert!(Frm::decode(&offset).is_err());
    }
}
