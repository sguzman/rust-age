//! Read-only CE MAP object-table and recursive inventory deserialization.
//!
//! Reference: src/game/object.cc obj_load_func and obj_load_obj.
//! The caller resolves item/scenery subtype from its original .PRO.
//! This is NOT entity creation, art rendering, AI or save support.

use crate::object_data::ObjectRecord;
use crate::prototype::ObjectKind;
use std::io::{self, Cursor, Read};

pub const ELEVATION_COUNT: usize = 3;
const MAX_MAP_OBJECTS: usize = 100_000;
const MAX_INVENTORY_DEPTH: usize = 32;

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn word(reader: &mut impl Read) -> io::Result<i32> {
    let mut bytes = [0_u8; 4];
    reader.read_exact(&mut bytes)?;
    Ok(i32::from_be_bytes(bytes))
}

fn count(reader: &mut impl Read) -> io::Result<usize> {
    let number = word(reader)?;
    let parsed = usize::try_from(number).map_err(|_| invalid("negative MAP object count"))?;
    if parsed > MAX_MAP_OBJECTS {
        return Err(invalid("MAP object count exceeds safe limit"));
    }
    Ok(parsed)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InventoryEntry {
    pub quantity: i32,
    pub object: Box<MapObject>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapObject {
    pub record: ObjectRecord,
    pub inventory: Vec<InventoryEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectTables {
    pub elevations: [Vec<MapObject>; ELEVATION_COUNT],
    pub total_objects: usize,
    /// Offset after all serialized objects, relative to provided slice.
    pub bytes_consumed: usize,
}

impl ObjectTables {
    /// Read original obj_load stream; type lookup must return an item or
    /// scenery subtype from that PID's user-owned .PRO file.
    pub fn decode_with<F>(bytes: &[u8], mut subtype_for: F) -> io::Result<Self>
    where
        F: FnMut(u32) -> io::Result<Option<i32>>,
    {
        let mut input = Cursor::new(bytes);
        let expected = count(&mut input)?;
        let mut visited = 0_usize;
        let mut elevations: [Vec<MapObject>; ELEVATION_COUNT] = std::array::from_fn(|_| Vec::new());
        let mut total = 0_usize;
        for elevation in &mut elevations {
            let current = count(&mut input)?;
            total = total
                .checked_add(current)
                .filter(|&n| n <= expected)
                .ok_or_else(|| invalid("MAP elevation counts exceed total"))?;
            elevation.reserve(current);
            for _ in 0..current {
                elevation.push(read_object(&mut input, &mut subtype_for, 0, &mut visited)?);
            }
        }
        if total != expected {
            return Err(invalid("MAP object counts disagree"));
        }
        Ok(Self {
            elevations,
            total_objects: total,
            bytes_consumed: input.position() as usize,
        })
    }
}

fn read_object<F>(
    input: &mut Cursor<&[u8]>,
    subtype_for: &mut F,
    depth: usize,
    visited: &mut usize,
) -> io::Result<MapObject>
where
    F: FnMut(u32) -> io::Result<Option<i32>>,
{
    if depth >= MAX_INVENTORY_DEPTH || *visited >= MAX_MAP_OBJECTS {
        return Err(invalid("MAP inventory recursion/size limit exceeded"));
    }
    *visited += 1;

    // obj_read_obj stores PID as its twelfth i32, i.e. at byte 44.
    let offset = (input.position() as usize)
        .checked_add(44)
        .ok_or_else(|| invalid("MAP object offset overflow"))?;
    let raw_pid = input
        .get_ref()
        .get(offset..offset + 4)
        .ok_or_else(|| invalid("truncated MAP object PID"))?;
    let pid = u32::from_be_bytes(
        raw_pid
            .try_into()
            .map_err(|_| invalid("short object PID"))?,
    );
    let subtype = match ObjectKind::from_pid(pid)? {
        ObjectKind::Item | ObjectKind::Scenery => subtype_for(pid)?,
        _ => None,
    };

    let record = ObjectRecord::read(input, subtype)?;
    let inventory_len =
        usize::try_from(record.inventory_length).map_err(|_| invalid("invalid inventory count"))?;
    let mut inventory = Vec::with_capacity(inventory_len);
    for _ in 0..inventory_len {
        let quantity = word(input)?;
        if quantity < 0 {
            return Err(invalid("negative inventory quantity"));
        }
        let object = read_object(input, subtype_for, depth + 1, visited)?;
        inventory.push(InventoryEntry {
            quantity,
            object: Box::new(object),
        });
    }
    Ok(MapObject { record, inventory })
}

#[cfg(test)]
mod tests {
    use super::ObjectTables;

    fn put(bytes: &mut Vec<u8>, number: i32) {
        bytes.extend_from_slice(&number.to_be_bytes());
    }

    fn object(bytes: &mut Vec<u8>, pid: i32, count: i32, capacity: i32, update: &[i32]) {
        // The 18 CE obj_read_obj header words, including PID at field 11.
        for field in [
            45,
            20099,
            0,
            0,
            0,
            0,
            0,
            2,
            0x0400_0001,
            0,
            0,
            pid,
            -1,
            0,
            0,
            0,
            -1,
            -1,
        ] {
            put(bytes, field);
        }
        put(bytes, count);
        put(bytes, capacity);
        put(bytes, 0); // ignored 32-bit inventory pointer
        for value in update {
            put(bytes, *value);
        }
    }

    #[test]
    fn empty_object_tables_are_four_count_words() {
        let empty = [0_u8; 16];
        let tables = ObjectTables::decode_with(&empty, |_| Ok(None)).unwrap();
        assert_eq!(tables.total_objects, 0);
        assert_eq!(tables.bytes_consumed, 16);
    }

    #[test]
    fn decodes_map_object_with_nested_prototype_dependent_weapon() {
        let mut data = Vec::new();
        put(&mut data, 1); // total top-level objects
        put(&mut data, 1); // elevation 0
        object(&mut data, 0x0400_0002, 1, 1, &[0]); // tile with inventory
        put(&mut data, 2); // child quantity
        object(&mut data, 0x0000_0001, 0, 0, &[0, 16, 0x0000_0010]);
        put(&mut data, 0); // elevation 1
        put(&mut data, 0); // elevation 2
        put(&mut data, 55); // subsequent map-data sentinel
        let tables = ObjectTables::decode_with(&data, |pid| {
            if pid == 0x0000_0001 {
                Ok(Some(3))
            } else {
                Ok(None)
            }
        })
        .unwrap();
        assert_eq!(tables.total_objects, 1);
        assert_eq!(tables.elevations[0].len(), 1);
        assert_eq!(tables.elevations[0][0].inventory.len(), 1);
        assert_eq!(tables.elevations[0][0].inventory[0].quantity, 2);
        assert_eq!(
            tables.elevations[0][0].inventory[0]
                .object
                .record
                .header
                .pid,
            0x0000_0001
        );
        assert_eq!(tables.bytes_consumed, data.len() - 4);
    }

    #[test]
    fn rejects_truncation_and_mismatched_count() {
        assert!(ObjectTables::decode_with(&[0_u8; 8], |_| Ok(None)).is_err());
        let mut bytes = Vec::new();
        put(&mut bytes, 0);
        put(&mut bytes, 1);
        assert!(ObjectTables::decode_with(&bytes, |_| Ok(None)).is_err());
    }
}
