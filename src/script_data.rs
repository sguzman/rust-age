//! Read-only CE MAP script-list serialization, not a bytecode interpreter.
//!
//! Source: pinned Fallout 1 CE src/game/scripts.cc scr_load,
//! scr_read_ScriptNode and scr_read_ScriptSubNode.
//!
//! Five categories, each with an i32 record count and zero or more 16-slot
//! extents. Serialized dummy slots are consumed but not exposed as scripts.
//! Program/pointer tokens in the records are never followed or executed.

use std::io::{self, Cursor, Read};

pub const SCRIPT_TYPE_COUNT: usize = 5;
pub const SCRIPT_EXTENT_SIZE: usize = 16;
const MAX_SCRIPT_COUNT: usize = 65_536;

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn word(reader: &mut impl Read) -> io::Result<i32> {
    let mut bytes = [0_u8; 4];
    reader.read_exact(&mut bytes)?;
    Ok(i32::from_be_bytes(bytes))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptRecord {
    pub script_id: i32,
    pub next: i32,
    /// Spatial scripts have two subtype fields; timed scripts one.
    pub subtype: Vec<i32>,
    pub flags: i32,
    pub script_index: i32,
    /// Legacy 32-bit program token, not a valid native pointer.
    pub program_token: i32,
    pub owner_id: i32,
    pub local_variable_offset: i32,
    pub local_variable_count: i32,
    /// Remaining serialized fields, preserved in CE read order:
    /// field_28, action, fixedParam, actionBeingUsed, scriptOverrides,
    /// field_48, howMuch, run_info_flags.
    pub remaining: [i32; 8],
}

impl ScriptRecord {
    fn read(input: &mut impl Read) -> io::Result<Self> {
        let script_id = word(input)?;
        let next = word(input)?;
        let subtype_count = match ((script_id as u32) >> 24) as usize {
            1 => 2,
            2 => 1,
            _ => 0,
        };
        let mut subtype = Vec::with_capacity(subtype_count);
        for _ in 0..subtype_count {
            subtype.push(word(input)?);
        }
        let flags = word(input)?;
        let script_index = word(input)?;
        let program_token = word(input)?;
        let owner_id = word(input)?;
        let local_variable_offset = word(input)?;
        let local_variable_count = word(input)?;
        let mut remaining = [0_i32; 8];
        for field in &mut remaining {
            *field = word(input)?;
        }
        Ok(Self {
            script_id,
            next,
            subtype,
            flags,
            script_index,
            program_token,
            owner_id,
            local_variable_offset,
            local_variable_count,
            remaining,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptTables {
    pub categories: [Vec<ScriptRecord>; SCRIPT_TYPE_COUNT],
    /// Byte offset after the serialized scripts, relative to input slice.
    /// For a MAP, this is the start of the original object section.
    pub bytes_consumed: usize,
}

impl ScriptTables {
    pub fn decode(bytes: &[u8]) -> io::Result<Self> {
        let mut input = Cursor::new(bytes);
        let mut categories: [Vec<ScriptRecord>; SCRIPT_TYPE_COUNT] =
            std::array::from_fn(|_| Vec::new());
        for category in &mut categories {
            let count = word(&mut input)?;
            let count = usize::try_from(count).map_err(|_| invalid("negative script count"))?;
            if count > MAX_SCRIPT_COUNT {
                return Err(invalid("script count exceeds safe limit"));
            }
            if count == 0 {
                continue;
            }
            let extents = count.div_ceil(SCRIPT_EXTENT_SIZE);
            category.reserve(count);
            for extent in 0..extents {
                let mut slots = Vec::with_capacity(SCRIPT_EXTENT_SIZE);
                for _ in 0..SCRIPT_EXTENT_SIZE {
                    slots.push(ScriptRecord::read(&mut input)?);
                }
                let actual = word(&mut input)?;
                let _legacy_next_token = word(&mut input)?;
                if !(1..=SCRIPT_EXTENT_SIZE as i32).contains(&actual) {
                    return Err(invalid("invalid script extent length"));
                }
                let expected = (count - extent * SCRIPT_EXTENT_SIZE).min(SCRIPT_EXTENT_SIZE);
                if actual as usize != expected {
                    return Err(invalid("script extent count differs from category total"));
                }
                category.extend(slots.into_iter().take(expected));
            }
        }
        Ok(Self {
            categories,
            bytes_consumed: input.position() as usize,
        })
    }

    pub fn script_count(&self) -> usize {
        self.categories.iter().map(Vec::len).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::{ScriptTables, SCRIPT_EXTENT_SIZE, SCRIPT_TYPE_COUNT};

    fn put(buffer: &mut Vec<u8>, value: i32) {
        buffer.extend_from_slice(&value.to_be_bytes());
    }

    fn slot(buffer: &mut Vec<u8>, kind: u8, script_id: i32) {
        put(buffer, script_id);
        put(buffer, -1);
        if kind == 1 {
            put(buffer, 1234);
            put(buffer, 2);
        } else if kind == 2 {
            put(buffer, 777);
        }
        for field in 0..14 {
            put(buffer, field);
        }
    }

    #[test]
    fn empty_map_script_tables_are_five_zero_counts() {
        let empty = [0_u8; SCRIPT_TYPE_COUNT * 4];
        let table = ScriptTables::decode(&empty).unwrap();
        assert_eq!(table.script_count(), 0);
        assert_eq!(table.bytes_consumed, empty.len());
    }

    #[test]
    fn one_spatial_and_one_timed_record_preserve_extent_padding() {
        let mut data = Vec::new();
        put(&mut data, 0); // system
        put(&mut data, 1); // spatial
        slot(&mut data, 1, 0x0100_0001);
        for _ in 1..SCRIPT_EXTENT_SIZE {
            slot(&mut data, 0, 0);
        }
        put(&mut data, 1); // active extent length
        put(&mut data, 0); // serialized pointer ignored
        put(&mut data, 1); // timed
        slot(&mut data, 2, 0x0200_0001);
        for _ in 1..SCRIPT_EXTENT_SIZE {
            slot(&mut data, 0, 0);
        }
        put(&mut data, 1);
        put(&mut data, 0);
        put(&mut data, 0); // item
        put(&mut data, 0); // critter
        put(&mut data, 0x123456); // sentinel for object-section start
        let table = ScriptTables::decode(&data).unwrap();
        assert_eq!(table.script_count(), 2);
        assert_eq!(table.categories[1][0].script_id, 0x0100_0001);
        assert_eq!(table.categories[1][0].subtype, vec![1234, 2]);
        assert_eq!(table.categories[2][0].subtype, vec![777]);
        assert_eq!(table.categories[2][0].remaining[7], 13);
        assert_eq!(table.bytes_consumed, data.len() - 4);
    }

    #[test]
    fn rejects_missing_or_mismatched_script_extent() {
        let mut data = Vec::new();
        put(&mut data, 1);
        for _ in 0..SCRIPT_EXTENT_SIZE {
            slot(&mut data, 0, 0);
        }
        assert!(ScriptTables::decode(&data).is_err());
        put(&mut data, 2); // wrong count for the only active record
        put(&mut data, 0);
        assert!(ScriptTables::decode(&data).is_err());
    }
}
