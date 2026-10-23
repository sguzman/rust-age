//! Read-only CE script-owner resolution from serialized MAP sources.
//! Pinned src/game/object.cc obj_load_func: each top-level Object::sid is
//! looked up in scr_ptr and then installed as Script::owner. Pinned
//! src/game/scripts.cc scr_read_ScriptSubNode loads scr_oid independently;
//! scr_find_obj_from_program synthesizes an owner for unowned spatial scripts.
//! We never synthesize engine objects or dereference serialized addresses.
use crate::collision::ObjectKey;
use crate::object_tables::ObjectTables;
use crate::script_data::ScriptTables;
use crate::script_object_binding::MapObjectReadBindings;
use std::collections::BTreeMap;
use std::io;

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptOwnerResolution {
    /// The unique serialized top-level object carrying this script SID.
    /// saved_oid_matches is diagnostic: CE binds via Object::sid rather
    /// than requiring Object::id to equal the saved Script::scr_oid.
    Bound {
        key: ObjectKey,
        handle: i32,
        saved_oid_matches: bool,
    },
    /// No object carries this non-spatial script SID; CE returns NULL.
    NullOwner,
    /// CE scr_find_obj_from_program can create a new spatial owner at runtime.
    /// This decoded-only bridge refuses to invent that object.
    SpatialOwnerRequiresEngine,
    /// Refuse records with invalid category encodings or missing SID.
    InvalidSid,
    /// Multiple saved script records share one SID; refuse implicit choice.
    DuplicateScriptSid,
    /// Multiple top-level serialized objects claim this SID; refuse choice.
    DuplicateObjectSid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptOwnerEntry {
    pub category: usize,
    pub ordinal: usize,
    pub script_id: i32,
    pub saved_owner_id: i32,
    pub resolution: ScriptOwnerResolution,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapScriptOwners {
    entries: Vec<ScriptOwnerEntry>,
}

impl MapScriptOwners {
    /// Resolve serialized MAP scripts against top-level MAP objects. The
    /// result contains every serialized script, even if its .INT is absent.
    /// None of these results starts scripts or allocates game objects.
    pub fn from_map(
        scripts: &ScriptTables,
        objects: &ObjectTables,
        snapshots: &MapObjectReadBindings,
    ) -> io::Result<Self> {
        let mut sid_counts = BTreeMap::<i32, usize>::new();
        for category in &scripts.categories {
            for record in category {
                *sid_counts.entry(record.script_id).or_default() += 1;
            }
        }
        let mut owners = BTreeMap::<i32, Vec<(ObjectKey, i32)>>::new();
        for (elevation, row) in objects.elevations.iter().enumerate() {
            for (ordinal, object) in row.iter().enumerate() {
                let sid = object.record.header.script_id;
                if sid == -1 {
                    continue;
                }
                let key = ObjectKey { elevation, ordinal };
                let handle = snapshots
                    .handle_for(key)
                    .ok_or_else(|| invalid("script owner missing top-level object handle"))?;
                owners.entry(sid).or_default().push((key, handle));
            }
        }
        let mut entries = Vec::with_capacity(scripts.script_count());
        for (category, rows) in scripts.categories.iter().enumerate() {
            for (ordinal, record) in rows.iter().enumerate() {
                let sid = record.script_id;
                let encoded_category = ((sid as u32) >> 24) as usize;
                let resolution = if sid == -1
                    || sid == i32::from_be_bytes([0xCC; 4])
                    || encoded_category != category
                {
                    ScriptOwnerResolution::InvalidSid
                } else if sid_counts.get(&sid).copied().unwrap_or(0) != 1 {
                    ScriptOwnerResolution::DuplicateScriptSid
                } else {
                    match owners.get(&sid).map(Vec::as_slice).unwrap_or(&[]) {
                        [] if category == 1 => ScriptOwnerResolution::SpatialOwnerRequiresEngine,
                        [] => ScriptOwnerResolution::NullOwner,
                        [(key, handle)] => {
                            let object = &objects.elevations[key.elevation][key.ordinal];
                            ScriptOwnerResolution::Bound {
                                key: *key,
                                handle: *handle,
                                saved_oid_matches: record.owner_id == object.record.header.id,
                            }
                        }
                        _ => ScriptOwnerResolution::DuplicateObjectSid,
                    }
                };
                entries.push(ScriptOwnerEntry {
                    category,
                    ordinal,
                    script_id: sid,
                    saved_owner_id: record.owner_id,
                    resolution,
                });
            }
        }
        Ok(Self { entries })
    }

    pub fn entries(&self) -> &[ScriptOwnerEntry] {
        &self.entries
    }

    pub fn entry(&self, category: usize, ordinal: usize) -> Option<&ScriptOwnerEntry> {
        self.entries
            .iter()
            .find(|entry| entry.category == category && entry.ordinal == ordinal)
    }

    /// Only source-verifiable owners produce pointer tokens. A non-spatial
    /// missing owner becomes the CE NULL pointer; ambiguous/spatial results
    /// remain unavailable instead of silently becoming NULL.
    pub fn self_handle(&self, category: usize, ordinal: usize) -> Option<i32> {
        match self.entry(category, ordinal)?.resolution {
            ScriptOwnerResolution::Bound { handle, .. } => Some(handle),
            ScriptOwnerResolution::NullOwner => Some(0),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MapScriptOwners, ScriptOwnerResolution};
    use crate::collision::ObjectKey;
    use crate::object_data::{ObjectHeader, ObjectRecord, ObjectUpdate};
    use crate::object_tables::{MapObject, ObjectTables};
    use crate::script_data::{ScriptRecord, ScriptTables};
    use crate::script_object_binding::MapObjectReadBindings;

    fn object(id: i32, sid: i32) -> MapObject {
        MapObject {
            record: ObjectRecord {
                header: ObjectHeader {
                    id,
                    tile: 20099,
                    x: 0,
                    y: 0,
                    screen_x: 0,
                    screen_y: 0,
                    frame: 0,
                    rotation: 0,
                    fid: 0x0300_0001,
                    flags: 0,
                    elevation: 0,
                    pid: 0x0300_0001,
                    cid: -1,
                    light_distance: 0,
                    light_intensity: 0,
                    serialized_outline: 0,
                    script_id: sid,
                    field_80: -1,
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

    fn script(sid: i32, oid: i32) -> ScriptRecord {
        ScriptRecord {
            script_id: sid,
            next: -1,
            subtype: vec![],
            flags: 0,
            script_index: 0,
            program_token: -1,
            owner_id: oid,
            local_variable_offset: -1,
            local_variable_count: 0,
            remaining: [0; 8],
        }
    }

    fn scripts(items: &[(usize, i32, i32)]) -> ScriptTables {
        let mut categories = std::array::from_fn(|_| Vec::new());
        for &(category, sid, oid) in items {
            categories[category].push(script(sid, oid));
        }
        ScriptTables {
            categories,
            bytes_consumed: 0,
        }
    }

    fn tables(rows: [Vec<MapObject>; 3]) -> ObjectTables {
        ObjectTables {
            total_objects: rows.iter().map(Vec::len).sum(),
            elevations: rows,
            bytes_consumed: 0,
        }
    }

    #[test]
    fn binds_by_unique_sid_not_repeated_saved_object_id() {
        let script_rows = scripts(&[(3, 0x0300_0001, 77), (4, 0x0400_0001, 77)]);
        let object_rows = tables([
            vec![object(77, 0x0300_0001), object(77, 0x0400_0001)],
            vec![],
            vec![],
        ]);
        let handles = MapObjectReadBindings::from_tables(&object_rows).unwrap();
        let bound = MapScriptOwners::from_map(&script_rows, &object_rows, &handles).unwrap();
        assert_eq!(bound.self_handle(3, 0), Some(1));
        assert_eq!(bound.self_handle(4, 0), Some(2));
        assert_eq!(
            bound.entry(3, 0).unwrap().resolution,
            ScriptOwnerResolution::Bound {
                key: ObjectKey {
                    elevation: 0,
                    ordinal: 0,
                },
                handle: 1,
                saved_oid_matches: true,
            }
        );
    }

    #[test]
    fn saved_oid_mismatch_is_diagnostic_not_an_ownership_key() {
        let script_rows = scripts(&[(3, 0x0300_0002, 111)]);
        let object_rows = tables([vec![object(222, 0x0300_0002)], vec![], vec![]]);
        let handles = MapObjectReadBindings::from_tables(&object_rows).unwrap();
        let bound = MapScriptOwners::from_map(&script_rows, &object_rows, &handles).unwrap();
        assert_eq!(
            bound.entry(3, 0).unwrap().resolution,
            ScriptOwnerResolution::Bound {
                key: ObjectKey {
                    elevation: 0,
                    ordinal: 0,
                },
                handle: 1,
                saved_oid_matches: false,
            }
        );
    }

    #[test]
    fn missing_nonspatial_yields_null_but_spatial_cannot_be_fabricated() {
        let script_rows = scripts(&[(3, 0x0300_0001, 777), (1, 0x0100_0001, -1)]);
        let objects = tables([vec![object(777, -1)], vec![], vec![]]);
        let handles = MapObjectReadBindings::from_tables(&objects).unwrap();
        let bound = MapScriptOwners::from_map(&script_rows, &objects, &handles).unwrap();
        assert_eq!(bound.self_handle(3, 0), Some(0));
        assert_eq!(bound.self_handle(1, 0), None);
        assert_eq!(
            bound.entry(1, 0).unwrap().resolution,
            ScriptOwnerResolution::SpatialOwnerRequiresEngine
        );
        assert_eq!(bound.self_handle(3, 999), None);
    }

    #[test]
    fn duplicate_script_sid_and_object_claims_fail_closed() {
        let script_rows = scripts(&[
            (3, 0x0300_0001, 1),
            (3, 0x0300_0001, 1),
            (4, 0x0400_0001, 1),
        ]);
        let objects = tables([
            vec![
                object(1, 0x0300_0001),
                object(2, 0x0400_0001),
                object(3, 0x0400_0001),
            ],
            vec![],
            vec![],
        ]);
        let handles = MapObjectReadBindings::from_tables(&objects).unwrap();
        let bound = MapScriptOwners::from_map(&script_rows, &objects, &handles).unwrap();
        assert_eq!(
            bound.entry(3, 0).unwrap().resolution,
            ScriptOwnerResolution::DuplicateScriptSid
        );
        assert_eq!(
            bound.entry(4, 0).unwrap().resolution,
            ScriptOwnerResolution::DuplicateObjectSid
        );
        assert_eq!(bound.self_handle(3, 1), None);
        assert_eq!(bound.self_handle(4, 0), None);
    }

    #[test]
    fn invalid_category_and_sid_do_not_resolve() {
        let script_rows = scripts(&[(3, 0x0100_0001, 2), (3, -1, 2)]);
        let objects = tables([vec![object(2, 0x0100_0001)], vec![], vec![]]);
        let handles = MapObjectReadBindings::from_tables(&objects).unwrap();
        let bound = MapScriptOwners::from_map(&script_rows, &objects, &handles).unwrap();
        assert_eq!(bound.self_handle(3, 0), None);
        assert_eq!(
            bound.entry(3, 1).unwrap().resolution,
            ScriptOwnerResolution::InvalidSid
        );
    }
}
