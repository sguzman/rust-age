//! Bounded CE-style actor movement through original MAP collision.
//!
//! Pathfinding/hex adjacency come from CE anim.cc/tile.cc; index updates
//! approximate the collision portion of obj_move_to_tile in object.cc.
//! This does NOT animate sprites, run object scripts, modify saved object
//! tables, open doors, trigger map exits or update light/roof/seen state.
use crate::collision::{CollisionIndex, ObjectKey};
use crate::hex::HexViewport;
use crate::pathfinding::make_path;
use std::io;

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActorStep {
    Moved { tile: i32, rotation: u8 },
    Blocked { tile: i32 },
    Finished,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActorRoute {
    key: ObjectKey,
    current_tile: i32,
    directions: Vec<u8>,
    cursor: usize,
}

impl ActorRoute {
    /// Plan from an indexed object to another original CE hex. The moving
    /// actor is excluded from blocking itself; all other static/runtime
    /// blockers are considered, including multihex objects.
    pub fn plan(
        view: &HexViewport,
        collision: &CollisionIndex,
        actor: ObjectKey,
        destination: i32,
        not_in_combat: bool,
    ) -> io::Result<Option<Self>> {
        let from = collision
            .tile_of(actor)
            .ok_or_else(|| invalid("actor is missing from CE collision state"))?;
        Ok(
            make_path(view, from, destination, not_in_combat, true, |tile| {
                collision.is_passable(view, tile, actor.elevation, Some(actor))
            })
            .map(|directions| Self {
                key: actor,
                current_tile: from,
                directions,
                cursor: 0,
            }),
        )
    }

    pub fn current_tile(&self) -> i32 {
        self.current_tile
    }

    pub fn remaining_steps(&self) -> usize {
        self.directions.len() - self.cursor
    }

    /// Apply one queued rotation, rechecking a possibly changed collision
    /// index before mutating it. A blocked step does not consume the route,
    /// so a caller can change occupancy and retry or replan explicitly.
    pub fn advance(
        &mut self,
        view: &HexViewport,
        collision: &mut CollisionIndex,
    ) -> io::Result<ActorStep> {
        if collision.tile_of(self.key) != Some(self.current_tile) {
            return Err(invalid("actor moved outside its queued CE route"));
        }
        let Some(&rotation) = self.directions.get(self.cursor) else {
            return Ok(ActorStep::Finished);
        };
        let next = view
            .move_in_direction(self.current_tile, usize::from(rotation), 1)
            .ok_or_else(|| invalid("invalid CE actor rotation"))?;
        if next == self.current_tile
            || !collision.is_passable(view, next, self.key.elevation, Some(self.key))
        {
            return Ok(ActorStep::Blocked { tile: next });
        }
        collision.relocate(self.key, next)?;
        self.current_tile = next;
        self.cursor += 1;
        Ok(ActorStep::Moved {
            tile: next,
            rotation,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{ActorRoute, ActorStep};
    use crate::collision::{CollisionIndex, ObjectKey};
    use crate::hex::HexViewport;
    use crate::object_data::{ObjectHeader, ObjectRecord, ObjectUpdate};
    use crate::object_tables::{MapObject, ObjectTables};

    fn object(id: i32, tile: i32, kind: i32) -> MapObject {
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
                    fid: kind << 24,
                    flags: 0,
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

    #[test]
    fn move_rechecks_collisions_and_retries_when_blocker_becomes_hidden() {
        let view = HexViewport::new(200, 200, 640, 380, 20099).unwrap();
        let tables = ObjectTables {
            elevations: [
                vec![object(1, 20099, 1), object(2, 20100, 3)],
                vec![],
                vec![],
            ],
            total_objects: 2,
            bytes_consumed: 0,
        };
        let mut collision = CollisionIndex::from_map(&tables);
        let actor = ObjectKey {
            elevation: 0,
            ordinal: 0,
        };
        let obstacle = ObjectKey {
            elevation: 0,
            ordinal: 1,
        };
        let mut route = ActorRoute::plan(&view, &collision, actor, 20097, true)
            .unwrap()
            .unwrap();
        assert_eq!(route.remaining_steps(), 2);
        assert_eq!(
            route.advance(&view, &mut collision).unwrap(),
            ActorStep::Moved {
                tile: 20098,
                rotation: 1
            }
        );
        collision.relocate(obstacle, 20097).unwrap();
        assert_eq!(
            route.advance(&view, &mut collision).unwrap(),
            ActorStep::Blocked { tile: 20097 }
        );
        assert_eq!(route.current_tile(), 20098);
        assert_eq!(route.remaining_steps(), 1);
        collision.set_flags(obstacle, 0x01).unwrap(); // original HIDDEN
        assert_eq!(
            route.advance(&view, &mut collision).unwrap(),
            ActorStep::Moved {
                tile: 20097,
                rotation: 1
            }
        );
        assert_eq!(
            route.advance(&view, &mut collision).unwrap(),
            ActorStep::Finished
        );
        assert_eq!(collision.tile_of(actor), Some(20097));
        assert_eq!(collision.tile_of(obstacle), Some(20097));
        assert_eq!(route.remaining_steps(), 0);
    }

    #[test]
    fn missing_actor_or_external_relocation_is_reported() {
        let view = HexViewport::new(200, 200, 640, 380, 20099).unwrap();
        let tables = ObjectTables {
            elevations: [vec![object(1, 20099, 1)], vec![], vec![]],
            total_objects: 1,
            bytes_consumed: 0,
        };
        let mut collision = CollisionIndex::from_map(&tables);
        let actor = ObjectKey {
            elevation: 0,
            ordinal: 0,
        };
        assert!(ActorRoute::plan(
            &view,
            &collision,
            ObjectKey {
                elevation: 0,
                ordinal: 1
            },
            20098,
            true
        )
        .is_err());
        let mut route = ActorRoute::plan(&view, &collision, actor, 20098, true)
            .unwrap()
            .unwrap();
        collision.relocate(actor, 20100).unwrap();
        assert!(route.advance(&view, &mut collision).is_err());
    }
}
