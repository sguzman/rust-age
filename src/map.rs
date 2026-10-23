//! Read-only Fallout 1 CE map header and per-elevation square tile arrays.
//!
//! Reference: src/game/map.cc (map_read_MapData, map_load_file,
//! square_load) and src/game/map_defs.h.
//! This does NOT load map scripts, dynamic objects or saved map state.

use crate::object_tables::ObjectTables;
use crate::script_data::ScriptTables;
use std::io::{self, Cursor, Read};

pub const MAP_VERSION: i32 = 19;
pub const HEADER_SIZE: usize = 236;
pub const SQUARE_WIDTH: usize = 100;
pub const SQUARE_HEIGHT: usize = 100;
pub const SQUARE_COUNT: usize = SQUARE_WIDTH * SQUARE_HEIGHT;
pub const ELEVATION_COUNT: usize = 3;
const ELEVATION_FLAGS: [i32; ELEVATION_COUNT] = [2, 4, 8];
const MAX_VARIABLE_COUNT: i32 = 1_000_000;

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn read_i32(reader: &mut impl Read) -> io::Result<i32> {
    let mut data = [0_u8; 4];
    reader.read_exact(&mut data)?;
    Ok(i32::from_be_bytes(data))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapHeader {
    pub name: [u8; 16],
    pub entering_tile: i32,
    pub entering_elevation: i32,
    pub entering_rotation: i32,
    pub local_variable_count: i32,
    pub script_index: i32,
    pub flags: i32,
    pub darkness: i32,
    pub global_variable_count: i32,
    pub map_number: i32,
    pub last_visit_time: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapSquares {
    /// Each populated elevation contains exactly 100*100 square tile words.
    /// The upper word holds roof tile and flags; lower word the floor tile.
    pub elevations: [Option<Vec<u32>>; ELEVATION_COUNT],
    /// Offset of the script section; scripts and objects are not decoded yet.
    pub scripts_offset: usize,
}

impl MapSquares {
    /// Original static floor art index (12-bit FID) at one square/elevation.
    /// The upper nibble of the floor word contains CE's overlay flags.
    /// Read original MAP script-list records after the square layers.
    /// No script code is executed. Object records begin at the returned offset.
    pub fn decode_scripts(&self, file: &[u8]) -> io::Result<(ScriptTables, usize)> {
        let data = file
            .get(self.scripts_offset..)
            .ok_or_else(|| invalid("MAP scripts outside file"))?;
        let scripts = ScriptTables::decode(data)?;
        let object_offset = self
            .scripts_offset
            .checked_add(scripts.bytes_consumed)
            .ok_or_else(|| invalid("MAP script offset overflow"))?;
        Ok((scripts, object_offset))
    }

    /// Decode the original script metadata and MAP object tables.
    /// The caller must resolve item/scenery subtype values by their PIDs.
    /// No bytecode execution, object instantiation or game state mutation.
    pub fn decode_objects_with<F>(
        &self,
        file: &[u8],
        resolve_subtype: F,
    ) -> io::Result<(ScriptTables, ObjectTables, usize)>
    where
        F: FnMut(u32) -> io::Result<Option<i32>>,
    {
        let (scripts, object_start) = self.decode_scripts(file)?;
        let data = file
            .get(object_start..)
            .ok_or_else(|| invalid("MAP object data outside file"))?;
        let objects = ObjectTables::decode_with(data, resolve_subtype)?;
        let end = object_start
            .checked_add(objects.bytes_consumed)
            .ok_or_else(|| invalid("MAP object section offset overflow"))?;
        Ok((scripts, objects, end))
    }

    pub fn floor_id(&self, elevation: usize, square: usize) -> Option<u16> {
        let word = *self.elevations.get(elevation)?.as_ref()?.get(square)?;
        Some((word & 0x0FFF) as u16)
    }

    /// Original static roof art index (12-bit FID) at one square/elevation.
    pub fn roof_id(&self, elevation: usize, square: usize) -> Option<u16> {
        let word = *self.elevations.get(elevation)?.as_ref()?.get(square)?;
        Some(((word >> 16) & 0x0FFF) as u16)
    }

    pub fn floor_hidden(&self, elevation: usize, square: usize) -> Option<bool> {
        let word = *self.elevations.get(elevation)?.as_ref()?.get(square)?;
        Some((word & 0x1000) != 0)
    }

    pub fn roof_hidden(&self, elevation: usize, square: usize) -> Option<bool> {
        let word = *self.elevations.get(elevation)?.as_ref()?.get(square)?;
        Some((word & 0x1000_0000) != 0)
    }
}

impl MapHeader {
    pub fn decode(bytes: &[u8]) -> io::Result<Self> {
        if bytes.len() < HEADER_SIZE {
            return Err(invalid("truncated Fallout 1 MAP header"));
        }
        let mut input = Cursor::new(bytes);
        let version = read_i32(&mut input)?;
        if version != MAP_VERSION {
            return Err(invalid("unsupported Fallout 1 MAP version"));
        }
        let mut name = [0_u8; 16];
        input.read_exact(&mut name)?;
        let entering_tile = read_i32(&mut input)?;
        let entering_elevation = read_i32(&mut input)?;
        let entering_rotation = read_i32(&mut input)?;
        let local_variable_count = read_i32(&mut input)?;
        let script_index = read_i32(&mut input)?;
        let flags = read_i32(&mut input)?;
        let darkness = read_i32(&mut input)?;
        let global_variable_count = read_i32(&mut input)?;
        let map_number = read_i32(&mut input)?;
        let last_visit_time = read_i32(&mut input)?;
        for _ in 0..44 {
            read_i32(&mut input)?;
        }
        if input.position() != HEADER_SIZE as u64 {
            return Err(invalid("invalid MAP header boundary"));
        }
        if !(0..40000).contains(&entering_tile)
            || !(0..3).contains(&entering_elevation)
            || !(0..6).contains(&entering_rotation)
            || !(0..=MAX_VARIABLE_COUNT).contains(&global_variable_count)
            || !(0..=MAX_VARIABLE_COUNT).contains(&local_variable_count)
        {
            return Err(invalid(
                "invalid Fallout 1 MAP dimensions or variable counts",
            ));
        }
        Ok(Self {
            name,
            entering_tile,
            entering_elevation,
            entering_rotation,
            local_variable_count,
            script_index,
            flags,
            darkness,
            global_variable_count,
            map_number,
            last_visit_time,
        })
    }

    pub fn name_bytes(&self) -> &[u8] {
        let length = self.name.iter().position(|&ch| ch == 0).unwrap_or(16);
        &self.name[..length]
    }

    /// Parse only static floor/roof tile arrays after the two variable tables.
    /// Keep the remaining script/object section intact for subsequent ports.
    pub fn decode_squares(&self, bytes: &[u8]) -> io::Result<MapSquares> {
        let variables = usize::try_from(self.global_variable_count + self.local_variable_count)
            .map_err(|_| invalid("invalid MAP variable count"))?;
        let variable_bytes = variables
            .checked_mul(4)
            .ok_or_else(|| invalid("MAP variable byte length overflow"))?;
        let mut offset = HEADER_SIZE
            .checked_add(variable_bytes)
            .ok_or_else(|| invalid("MAP variable offset overflow"))?;
        if offset > bytes.len() {
            return Err(invalid("truncated MAP variables"));
        }
        let mut elevations: [Option<Vec<u32>>; ELEVATION_COUNT] = std::array::from_fn(|_| None);
        for (elevation, flag) in ELEVATION_FLAGS.iter().copied().enumerate() {
            if self.flags & flag != 0 {
                continue;
            }
            const LAYER_BYTES: usize = SQUARE_COUNT * 4;
            let end = offset
                .checked_add(LAYER_BYTES)
                .ok_or_else(|| invalid("MAP square layer overflow"))?;
            let data = bytes
                .get(offset..end)
                .ok_or_else(|| invalid("truncated MAP square tiles"))?;
            let mut words = Vec::with_capacity(SQUARE_COUNT);
            for chunk in data.as_chunks::<4>().0 {
                let word = u32::from_be_bytes(*chunk);
                // CE square_load resets the low bit of each roof's flag nibble.
                words.push(word & !0x1000_0000);
            }
            elevations[elevation] = Some(words);
            offset = end;
        }
        Ok(MapSquares {
            elevations,
            scripts_offset: offset,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{MapHeader, HEADER_SIZE, SQUARE_COUNT};
    fn header(flags: i32, global_count: i32, local_count: i32) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&19_i32.to_be_bytes());
        bytes.extend_from_slice(b"TEST.MAP\0\0\0\0\0\0\0\0");
        let fields: [i32; 10] = [
            20099,
            0,
            2,
            local_count,
            -1,
            flags,
            100,
            global_count,
            12,
            1000,
        ];
        for value in fields {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        bytes.extend_from_slice(&[0_u8; 44 * 4]);
        assert_eq!(bytes.len(), HEADER_SIZE);
        bytes
    }

    #[test]
    fn reads_original_map_header_and_skipped_elevations() {
        let mut bytes = header(2 | 8, 1, 2);
        for _ in 0..3 {
            bytes.extend_from_slice(&0_i32.to_be_bytes());
        }
        bytes.extend(std::iter::repeat_n(0_u8, SQUARE_COUNT * 4));
        let map = MapHeader::decode(&bytes).unwrap();
        assert_eq!(map.name_bytes(), b"TEST.MAP");
        assert_eq!(map.entering_tile, 20099);
        assert_eq!(map.script_index, -1);
        let square = map.decode_squares(&bytes).unwrap();
        assert!(square.elevations[0].is_none());
        assert_eq!(square.elevations[1].as_ref().unwrap().len(), SQUARE_COUNT);
        assert!(square.elevations[2].is_none());
        assert_eq!(square.scripts_offset, HEADER_SIZE + 12 + SQUARE_COUNT * 4);
    }

    #[test]
    fn clears_legacy_roof_flag_low_bit() {
        let mut bytes = header(4 | 8, 0, 0);
        bytes.extend_from_slice(&0x1005_0001_u32.to_be_bytes());
        bytes.extend(std::iter::repeat_n(0_u8, (SQUARE_COUNT - 1) * 4));
        let squares = MapHeader::decode(&bytes)
            .unwrap()
            .decode_squares(&bytes)
            .unwrap();
        assert_eq!(squares.elevations[0].as_ref().unwrap()[0], 0x0005_0001);
    }

    #[test]
    fn exposes_floor_and_roof_fids_separately() {
        let mut bytes = header(4 | 8, 0, 0);
        bytes.extend_from_slice(&0x0005_0001_u32.to_be_bytes());
        bytes.extend(std::iter::repeat_n(0_u8, (SQUARE_COUNT - 1) * 4));
        let squares = MapHeader::decode(&bytes)
            .unwrap()
            .decode_squares(&bytes)
            .unwrap();
        assert_eq!(squares.floor_id(0, 0), Some(1));
        assert_eq!(squares.roof_id(0, 0), Some(5));
        assert_eq!(squares.floor_hidden(0, 0), Some(false));
        assert_eq!(squares.floor_id(1, 0), None);
        assert_eq!(squares.roof_id(0, SQUARE_COUNT), None);
    }

    #[test]
    fn maps_object_boundary_after_empty_script_tables() {
        let mut bytes = header(2 | 4 | 8, 0, 0);
        bytes.extend_from_slice(&[0_u8; 20]);
        bytes.extend_from_slice(&88_i32.to_be_bytes());
        let squares = MapHeader::decode(&bytes)
            .unwrap()
            .decode_squares(&bytes)
            .unwrap();
        let (scripts, object_offset) = squares.decode_scripts(&bytes).unwrap();
        assert_eq!(scripts.script_count(), 0);
        assert_eq!(object_offset, HEADER_SIZE + 20);
        assert_eq!(
            &bytes[object_offset..object_offset + 4],
            &88_i32.to_be_bytes()
        );
    }

    #[test]
    fn decodes_empty_map_script_and_object_sections_together() {
        let mut bytes = header(2 | 4 | 8, 0, 0);
        bytes.extend_from_slice(&[0_u8; 20]); // five empty script counts
        bytes.extend_from_slice(&[0_u8; 16]); // total and three empty object counts
        let square = MapHeader::decode(&bytes)
            .unwrap()
            .decode_squares(&bytes)
            .unwrap();
        let (scripts, objects, end) = square.decode_objects_with(&bytes, |_| Ok(None)).unwrap();
        assert_eq!(scripts.script_count(), 0);
        assert_eq!(objects.total_objects, 0);
        assert_eq!(end, bytes.len());
    }

    #[test]
    fn rejects_bad_version_and_truncated_tile_data() {
        let mut bytes = header(4 | 8, 0, 0);
        bytes[3] = 18;
        assert!(MapHeader::decode(&bytes).is_err());
        let bytes = header(4 | 8, 0, 0);
        assert!(MapHeader::decode(&bytes)
            .unwrap()
            .decode_squares(&bytes)
            .is_err());
    }
}
