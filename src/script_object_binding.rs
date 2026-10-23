//! Detached CE MAP object-table to script-property snapshot bridge.
//! Each nonzero handle identifies a top-level ObjectKey (elevation/ordinal),
//! NOT a serialized object ID, script SID, C++ address or reusable save token.
//! Inventory children are not members of the top-level object table.
use crate::collision::ObjectKey;
use crate::object_tables::ObjectTables;
use crate::script_object_read::ScriptObjectSnapshot;
use std::collections::BTreeMap;
use std::io;

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

/// Immutable caller-owned native reads. Refresh from tables after movement;
/// the old snapshot intentionally preserves its old object values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapObjectReadBindings {
    snapshots: Vec<ScriptObjectSnapshot>,
    keys: Vec<ObjectKey>,
    handles: BTreeMap<ObjectKey, i32>,
}

impl MapObjectReadBindings {
    pub fn from_tables(tables: &ObjectTables) -> io::Result<Self> {
        let mut snapshots = Vec::new();
        let mut keys = Vec::new();
        let mut handles = BTreeMap::new();
        for (elevation, objects) in tables.elevations.iter().enumerate() {
            for (ordinal, object) in objects.iter().enumerate() {
                let ordinal_handle = snapshots
                    .len()
                    .checked_add(1)
                    .ok_or_else(|| invalid("CE object handle count overflow"))?;
                let handle = i32::try_from(ordinal_handle)
                    .map_err(|_| invalid("CE object handle exceeds signed i32"))?;
                let key = ObjectKey { elevation, ordinal };
                let header = &object.record.header;
                snapshots.push(ScriptObjectSnapshot {
                    handle,
                    tile: header.tile,
                    elevation: header.elevation,
                    table_elevation: elevation,
                    pid: header.pid as i32,
                    fid: header.fid,
                });
                keys.push(key);
                handles.insert(key, handle);
            }
        }
        Ok(Self {
            snapshots,
            keys,
            handles,
        })
    }

    pub fn snapshots(&self) -> &[ScriptObjectSnapshot] {
        &self.snapshots
    }

    /// Nonzero handle valid only in this detached scene snapshot.
    pub fn handle_for(&self, key: ObjectKey) -> Option<i32> {
        self.handles.get(&key).copied()
    }

    pub fn key_for(&self, handle: i32) -> Option<ObjectKey> {
        let offset = usize::try_from(handle.checked_sub(1)?).ok()?;
        self.keys.get(offset).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::MapObjectReadBindings;
    use crate::collision::ObjectKey;
    use crate::object_data::{ObjectHeader, ObjectRecord, ObjectUpdate};
    use crate::object_tables::{InventoryEntry, MapObject, ObjectTables};
    use crate::script_object_read::{
        apply_object_read_opcode, ELEVATION_OPCODE, OBJ_ART_FID_OPCODE, OBJ_PID_OPCODE,
        TILE_NUM_OPCODE,
    };
    use crate::script_vm::ScriptValue;

    fn object(tile: i32, elevation: i32, pid: u32, fid: i32) -> MapObject {
        MapObject {
            record: ObjectRecord {
                header: ObjectHeader {
                    id: 77,
                    tile,
                    x: 0,
                    y: 0,
                    screen_x: 0,
                    screen_y: 0,
                    frame: 0,
                    rotation: 0,
                    fid,
                    flags: 0,
                    elevation,
                    pid,
                    cid: -1,
                    light_distance: 0,
                    light_intensity: 0,
                    serialized_outline: 0,
                    script_id: -1,
                    field_80: 0,
                },
                inventory_length: 0,
                inventory_capacity: 0,
                update: ObjectUpdate::Generic {
                    flags: 0,
                    subtype_fields: vec![],
                },
            },
            inventory: vec![],
        }
    }

    fn key(elevation: usize, ordinal: usize) -> ObjectKey {
        ObjectKey { elevation, ordinal }
    }

    #[test]
    fn duplicate_ids_distinct_handles_nested_inventory_excluded() {
        let mut first = object(20099, 0, 0xFFFF_FF87, 0x3400_0102);
        first.inventory.push(InventoryEntry {
            quantity: 2,
            object: Box::new(object(-1, 0, 7, 0x77)),
        });
        let tables = ObjectTables {
            elevations: [
                vec![first, object(20100, 0, 0x1000_0003, -20)],
                vec![],
                vec![object(21222, 2, 0x2000_0011, 0x1234)],
            ],
            total_objects: 3,
            bytes_consumed: 0,
        };
        let bindings = MapObjectReadBindings::from_tables(&tables).unwrap();
        assert_eq!(bindings.snapshots().len(), 3);
        assert_eq!(bindings.handle_for(key(0, 0)), Some(1));
        assert_eq!(bindings.handle_for(key(0, 1)), Some(2));
        assert_eq!(bindings.handle_for(key(2, 0)), Some(3));
        assert_eq!(bindings.key_for(1), Some(key(0, 0)));
        assert_eq!(bindings.key_for(3), Some(key(2, 0)));
        assert_eq!(bindings.handle_for(key(1, 0)), None);
        for bad in [0, -1, 4, i32::MIN, i32::MAX] {
            assert_eq!(bindings.key_for(bad), None);
        }
        for (handle, opcode, expected) in [
            (1, TILE_NUM_OPCODE, 20099),
            (2, TILE_NUM_OPCODE, 20100),
            (3, ELEVATION_OPCODE, 2),
            (1, OBJ_PID_OPCODE, 0xFFFF_FF87_u32 as i32),
            (2, OBJ_ART_FID_OPCODE, -20),
        ] {
            let mut stack = vec![ScriptValue::Pointer(handle)];
            apply_object_read_opcode(bindings.snapshots(), opcode, &mut stack).unwrap();
            assert_eq!(stack, [ScriptValue::Int(expected)]);
        }
        assert_eq!(tables.elevations[0][0].record.header.tile, 20099);
    }

    #[test]
    fn empty_tables_fail_closed_except_explicit_null_pointer() {
        let tables = ObjectTables {
            elevations: std::array::from_fn(|_| vec![]),
            total_objects: 0,
            bytes_consumed: 0,
        };
        let bindings = MapObjectReadBindings::from_tables(&tables).unwrap();
        assert!(bindings.snapshots().is_empty());
        assert_eq!(bindings.handle_for(key(0, 0)), None);
        let mut unknown = vec![ScriptValue::Pointer(1)];
        assert!(
            apply_object_read_opcode(bindings.snapshots(), TILE_NUM_OPCODE, &mut unknown).is_err()
        );
        assert_eq!(unknown, [ScriptValue::Pointer(1)]);
        let mut null = vec![ScriptValue::Pointer(0)];
        apply_object_read_opcode(bindings.snapshots(), TILE_NUM_OPCODE, &mut null).unwrap();
        assert_eq!(null, [ScriptValue::Int(-1)]);
    }
}
