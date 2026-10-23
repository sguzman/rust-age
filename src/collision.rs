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
        self.key != excluded.unwrap_or(ObjectKey {
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
                index.objects.entry((elevation, header.tile)).or_default().push(
                    CollisionObject {
                        key: ObjectKey { elevation, ordinal },
                        tile: header.tile,
                        fid: header.fid as u32,
                        flags: header.flags,
                    },
                );
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
                if let Some(object) = objects.iter().copied().find(|o| {
                    o.flags & OBJECT_MULTIHEX != 0 && o.eligible(excluded)
                }) {
                    return Some(object);
                }
            }
        }
        None
    }

    pub fn is_passable(
        &self,
        view: &HexViewport,
        tile: i32,
        elevation: usize,
        excluded: Option<ObjectKey>,
    ) -> bool {
        self.first_blocker(view, tile, elevation, excluded).is_none()
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
        assert_eq!(blocked.first_blocker(&view, 20098, 0, None).unwrap().key.ordinal, 3);
        assert!(blocked.first_blocker(&view, 20098, 0, Some(ObjectKey {
            elevation: 0,
            ordinal: 3,
        })).is_none());
        assert!(blocked.first_blocker(&view, 20098, 1, None).is_none());
    }

    #[test]
    fn adjacent_multihex_occupies_target_but_ordinary_object_does_not() {
        let view = HexViewport::new(200, 200, 640, 480, 20099).unwrap();
        let normal = index(vec![object(1, 20098, 2, 0)]);
        assert!(normal.is_passable(&view, 20099, 0, None));
        let huge = index(vec![object(1, 20098, 2, OBJECT_MULTIHEX)]);
        assert_eq!(huge.first_blocker(&view, 20099, 0, None).unwrap().tile, 20098);
        assert!(huge.first_blocker(&view, -1, 0, None).is_none());
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
            tile = view.move_in_direction(tile, usize::from(direction), 1).unwrap();
            assert_ne!(tile, 20098);
        }
        assert_eq!(tile, 20097);
    }
}
