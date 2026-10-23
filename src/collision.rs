//! CE obj_blocking_at collision rules over deserialized MAP objects.
//! Source: pinned CE src/game/object.cc and game/object_types.h.
//!
//! Only top-level MAP objects participate (inventory children are not on
//! objectTable). The loader's per-elevation list position gives an object
//! identity distinct from its possibly duplicated saved numeric ID.
//! Door interaction and runtime changes to flags are separate work.
use crate::hex::HexViewport;
use crate::object_tables::ObjectTables;
use std::collections::BTreeMap;
use std::io;

const OBJECT_HIDDEN: i32 = 0x01;
const OBJECT_NO_BLOCK: i32 = 0x10;
const OBJECT_MULTIHEX: i32 = 0x800;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ObjectKey {
    pub elevation: usize,
    pub ordinal: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CollisionObject {
    pub key: ObjectKey,
    pub tile: i32,
    pub fid: u32,
    pub flags: i32,
}

impl CollisionObject {
    fn eligible(self, excluded: Option<ObjectKey>) -> bool {
        let kind = (self.fid >> 24) & 0x0F;
        self.key
            != excluded.unwrap_or(ObjectKey {
                elevation: usize::MAX,
                ordinal: usize::MAX,
            })
            && self.flags & (OBJECT_HIDDEN | OBJECT_NO_BLOCK) == 0
            && (1..=3).contains(&kind)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CollisionIndex {
    objects: BTreeMap<(usize, i32), Vec<CollisionObject>>,
}

impl CollisionIndex {
    pub fn from_map(tables: &ObjectTables) -> Self {
        let mut index = Self::default();
        for (elevation, objects) in tables.elevations.iter().enumerate() {
            for (ordinal, object) in objects.iter().enumerate() {
                let header = &object.record.header;
                if !(0..40_000).contains(&header.tile) {
                    continue;
                }
                index
                    .objects
                    .entry((elevation, header.tile))
                    .or_default()
                    .push(CollisionObject {
                        key: ObjectKey { elevation, ordinal },
                        tile: header.tile,
                        fid: header.fid as u32,
                        flags: header.flags,
                    });
            }
        }
        index
    }

    /// Return the first CE blocker: exact tile first, then multhex objects
    /// in the six adjacent hexes. The caller may exclude a moving actor.
    pub fn first_blocker(
        &self,
        view: &HexViewport,
        tile: i32,
        elevation: usize,
        excluded: Option<ObjectKey>,
    ) -> Option<CollisionObject> {
        if !(0..40_000).contains(&tile) || elevation >= 3 {
            return None;
        }
        if let Some(objects) = self.objects.get(&(elevation, tile)) {
            if let Some(object) = objects.iter().copied().find(|o| o.eligible(excluded)) {
                return Some(object);
            }
        }
        for direction in 0..6 {
            let next = view.move_in_direction(tile, direction, 1)?;
            if let Some(objects) = self.objects.get(&(elevation, next)) {
                if let Some(object) = objects
                    .iter()
                    .copied()
                    .find(|o| o.flags & OBJECT_MULTIHEX != 0 && o.eligible(excluded))
                {
                    return Some(object);
                }
            }
        }
        None
    }

    /// Return a runtime object's current hex, by its stable serialized key.
    pub fn tile_of(&self, key: ObjectKey) -> Option<i32> {
        self.objects
            .iter()
            .find_map(|(&(elevation, tile), objects)| {
                if elevation == key.elevation && objects.iter().any(|object| object.key == key) {
                    Some(tile)
                } else {
                    None
                }
            })
    }

    /// Move an existing object to a different hex on the same elevation.
    ///
    /// This adjusts only the collision index, not MAP object records,
    /// lighting, triggers, roofing or save state. Movement may be blocked
    /// separately by the caller using first_blocker before committing it.
    pub fn relocate(&mut self, key: ObjectKey, to: i32) -> io::Result<()> {
        if !(0..40_000).contains(&to) || key.elevation >= 3 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid CE collision destination",
            ));
        }
        let Some((from_elevation, from_tile)) =
            self.objects
                .iter()
                .find_map(|(&(elevation, tile), objects)| {
                    if elevation == key.elevation && objects.iter().any(|object| object.key == key)
                    {
                        Some((elevation, tile))
                    } else {
                        None
                    }
                })
        else {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "CE collision object not found",
            ));
        };
        if from_tile == to {
            return Ok(());
        }
        let origin = (from_elevation, from_tile);
        let Some(objects) = self.objects.get_mut(&origin) else {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "missing CE collision origin",
            ));
        };
        let Some(position) = objects.iter().position(|object| object.key == key) else {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "missing CE collision identity",
            ));
        };
        let mut moved = objects.remove(position);
        let remove_empty = objects.is_empty();
        if remove_empty {
            self.objects.remove(&origin);
        }
        moved.tile = to;
        let destination = self.objects.entry((key.elevation, to)).or_default();
        let position = destination.partition_point(|object| object.key.ordinal < key.ordinal);
        destination.insert(position, moved);
        Ok(())
    }

    /// Change runtime visibility/NO_BLOCK/MULTIHEX flags without changing
    /// original serialized MAP data.
    pub fn set_flags(&mut self, key: ObjectKey, flags: i32) -> io::Result<()> {
        for (&(elevation, _), objects) in &mut self.objects {
            if elevation != key.elevation {
                continue;
            }
            if let Some(object) = objects.iter_mut().find(|object| object.key == key) {
                object.flags = flags;
                return Ok(());
            }
        }
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "CE collision object not found",
        ))
    }

    pub fn is_passable(
        &self,
        view: &HexViewport,
        tile: i32,
        elevation: usize,
        excluded: Option<ObjectKey>,
    ) -> bool {
        self.first_blocker(view, tile, elevation, excluded)
            .is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::{CollisionIndex, ObjectKey, OBJECT_HIDDEN, OBJECT_MULTIHEX, OBJECT_NO_BLOCK};
    use crate::hex::HexViewport;
    use crate::object_data::{ObjectHeader, ObjectRecord, ObjectUpdate};
    use crate::object_tables::{MapObject, ObjectTables};

    fn object(id: i32, tile: i32, kind: u32, flags: i32) -> MapObject {
        MapObject {
            record: ObjectRecord {
                header: ObjectHeader {
                    id,
                    tile,
                    x: 0,
                    y: 0,
                    screen_x: 0,
                    screen_y: 0,
                    frame: 0,
                    rotation: 0,
                    fid: (kind << 24) as i32,
                    flags,
                    elevation: 0,
                    pid: 0,
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

    fn index(objects: Vec<MapObject>) -> CollisionIndex {
        CollisionIndex::from_map(&ObjectTables {
            elevations: [objects, vec![], vec![]],
            total_objects: 0,
            bytes_consumed: 0,
        })
    }

    #[test]
    fn direct_blockers_require_ce_fid_kind_and_visible_flags() {
        let view = HexViewport::new(200, 200, 640, 480, 20099).unwrap();
        let blocked = index(vec![
            object(7, 20098, 0, 0),
            object(8, 20098, 2, OBJECT_HIDDEN),
            object(9, 20098, 3, OBJECT_NO_BLOCK),
            object(10, 20098, 1, 0),
        ]);
        assert_eq!(
            blocked
                .first_blocker(&view, 20098, 0, None)
                .unwrap()
                .key
                .ordinal,
            3
        );
        assert!(blocked
            .first_blocker(
                &view,
                20098,
                0,
                Some(ObjectKey {
                    elevation: 0,
                    ordinal: 3,
                })
            )
            .is_none());
        assert!(blocked.first_blocker(&view, 20098, 1, None).is_none());
    }

    #[test]
    fn adjacent_multihex_occupies_target_but_ordinary_object_does_not() {
        let view = HexViewport::new(200, 200, 640, 480, 20099).unwrap();
        let normal = index(vec![object(1, 20098, 2, 0)]);
        assert!(normal.is_passable(&view, 20099, 0, None));
        let huge = index(vec![object(1, 20098, 2, OBJECT_MULTIHEX)]);
        assert_eq!(
            huge.first_blocker(&view, 20099, 0, None).unwrap().tile,
            20098
        );
        assert!(huge.first_blocker(&view, -1, 0, None).is_none());
    }

    #[test]
    fn moving_object_frees_original_hex_and_blocks_new_hex() {
        let view = HexViewport::new(200, 200, 640, 380, 20099).unwrap();
        let key = ObjectKey {
            elevation: 0,
            ordinal: 0,
        };
        let mut index = index(vec![object(10, 20098, 2, 0)]);
        assert!(!index.is_passable(&view, 20098, 0, None));
        assert!(index.is_passable(&view, 20100, 0, None));
        index.relocate(key, 20100).unwrap();
        assert!(index.is_passable(&view, 20098, 0, None));
        assert!(!index.is_passable(&view, 20100, 0, None));
        assert_eq!(
            index.first_blocker(&view, 20100, 0, None).unwrap().tile,
            20100
        );
        index.relocate(key, 20100).unwrap(); // idempotent same-tile move
        assert!(index.relocate(key, -1).is_err());
        assert!(index.relocate(key, 40000).is_err());
        assert!(index
            .relocate(
                ObjectKey {
                    elevation: 1,
                    ordinal: 0
                },
                20098
            )
            .is_err());
        assert!(!index.is_passable(&view, 20100, 0, None));
    }

    #[test]
    fn visibility_and_multihex_updates_change_collision_immediately() {
        let view = HexViewport::new(200, 200, 640, 380, 20099).unwrap();
        let key = ObjectKey {
            elevation: 0,
            ordinal: 0,
        };
        let mut index = index(vec![object(15, 20098, 3, 0)]);
        assert!(!index.is_passable(&view, 20098, 0, None));
        assert!(index.is_passable(&view, 20099, 0, None));
        index.set_flags(key, OBJECT_MULTIHEX).unwrap();
        assert!(!index.is_passable(&view, 20099, 0, None));
        index
            .set_flags(key, OBJECT_HIDDEN | OBJECT_MULTIHEX)
            .unwrap();
        assert!(index.is_passable(&view, 20098, 0, None));
        assert!(index.is_passable(&view, 20099, 0, None));
        index.set_flags(key, OBJECT_NO_BLOCK).unwrap();
        assert!(index.is_passable(&view, 20098, 0, None));
        index.set_flags(key, 0).unwrap();
        assert!(!index.is_passable(&view, 20098, 0, None));
    }

    #[test]
    fn pathfinder_uses_original_blocker_index_without_moving_entities() {
        use crate::pathfinding::make_path;
        let view = HexViewport::new(200, 200, 640, 480, 20099).unwrap();
        let blocked = index(vec![object(2, 20098, 3, 0)]);
        let route = make_path(&view, 20099, 20097, true, true, |tile| {
            blocked.is_passable(&view, tile, 0, None)
        })
        .unwrap();
        let mut tile = 20099;
        for direction in route {
            tile = view
                .move_in_direction(tile, usize::from(direction), 1)
                .unwrap();
            assert_ne!(tile, 20098);
        }
        assert_eq!(tile, 20097);
    }
}
