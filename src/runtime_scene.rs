//! Minimal mutable Fallout 1 CE scene-state bridge.
//!
//! Actor navigation updates both the deserialized MAP object header and
//! the live collision index. This is not the full CE obj_move_to_tile:
//! triggers, scripts, lighting, roof, saves, time, and combat are absent.
use crate::actor_route::{ActorRoute, ActorStep};
use crate::collision::ObjectKey;
use crate::hex::HexViewport;
use crate::scene::StaticScene;
use std::io;

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[derive(Debug, Clone)]
pub struct RuntimeScene {
    snapshot: StaticScene,
}

impl RuntimeScene {
    pub fn new(snapshot: StaticScene) -> Self {
        Self { snapshot }
    }

    /// The current per-elevation object tables can be rendered by the
    /// existing read-only object preview without a separate sprite model.
    pub fn snapshot(&self) -> &StaticScene {
        &self.snapshot
    }

    pub fn plan_actor(
        &self,
        view: &HexViewport,
        actor: ObjectKey,
        to: i32,
        not_in_combat: bool,
    ) -> io::Result<Option<ActorRoute>> {
        ActorRoute::plan(view, &self.snapshot.collision, actor, to, not_in_combat)
    }

    pub fn advance_actor(
        &mut self,
        view: &HexViewport,
        route: &mut ActorRoute,
    ) -> io::Result<ActorStep> {
        let key = route.actor_key();
        let Some(object) = self
            .snapshot
            .objects
            .elevations
            .get_mut(key.elevation)
            .and_then(|objects| objects.get_mut(key.ordinal))
        else {
            return Err(invalid("CE actor is not present in scene tables"));
        };
        if object.record.header.tile != route.current_tile() {
            return Err(invalid("CE scene actor and route positions diverged"));
        }
        let step = route.advance(view, &mut self.snapshot.collision)?;
        if let ActorStep::Moved { tile, rotation } = step {
            object.record.header.tile = tile;
            object.record.header.rotation = i32::from(rotation);
        }
        Ok(step)
    }

    /// Mutate visibility/collision bits together with the original MAP
    /// object header so subsequent object preview passes see the new flags.
    pub fn set_object_flags(&mut self, key: ObjectKey, flags: i32) -> io::Result<()> {
        let Some(object) = self
            .snapshot
            .objects
            .elevations
            .get_mut(key.elevation)
            .and_then(|objects| objects.get_mut(key.ordinal))
        else {
            return Err(invalid("CE object is not present in scene tables"));
        };
        self.snapshot.collision.set_flags(key, flags)?;
        object.record.header.flags = flags;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::RuntimeScene;
    use crate::actor_route::ActorStep;
    use crate::collision::{CollisionIndex, ObjectKey};
    use crate::hex::HexViewport;
    use crate::map::{MapHeader, MapSquares, MapVariables};
    use crate::object_data::{ObjectHeader, ObjectRecord, ObjectUpdate};
    use crate::object_tables::{MapObject, ObjectTables};
    use crate::scene::StaticScene;
    use crate::script_data::ScriptTables;

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
                    pid: if kind == 1 { 0x0100_0001 } else { 0x0300_0001 },
                    cid: -1,
                    light_distance: 0,
                    light_intensity: 0,
                    serialized_outline: 0,
                    script_id: -1,
                    field_80: 0,
                },
                inventory_length: 0,
                inventory_capacity: 0,
                update: if kind == 1 {
                    ObjectUpdate::Critter {
                        field_0: 0,
                        combat: [0; 7],
                        hit_points: 20,
                        radiation: 0,
                        poison: 0,
                    }
                } else {
                    ObjectUpdate::Generic {
                        flags: 0,
                        subtype_fields: vec![],
                    }
                },
            },
            inventory: vec![],
        }
    }

    fn scene() -> RuntimeScene {
        let objects = ObjectTables {
            elevations: [
                vec![object(1, 20099, 1), object(2, 20100, 3)],
                vec![],
                vec![],
            ],
            total_objects: 2,
            bytes_consumed: 0,
        };
        let collision = CollisionIndex::from_map(&objects);
        RuntimeScene::new(StaticScene {
            header: MapHeader {
                name: [0; 16],
                entering_tile: 20099,
                entering_elevation: 0,
                entering_rotation: 0,
                local_variable_count: 0,
                script_index: -1,
                flags: 0,
                darkness: 0,
                global_variable_count: 0,
                map_number: 0,
                last_visit_time: 0,
            },
            variables: MapVariables {
                globals: vec![],
                locals: vec![],
            },
            squares: MapSquares {
                elevations: [None, None, None],
                scripts_offset: 0,
            },
            scripts: ScriptTables {
                categories: std::array::from_fn(|_| Vec::new()),
                bytes_consumed: 0,
            },
            objects,
            collision,
            bytes_consumed: 0,
        })
    }

    #[test]
    fn steps_original_ce_actor_route_and_keeps_map_and_collision_in_sync() {
        let view = HexViewport::new(200, 200, 640, 380, 20099).unwrap();
        let mut world = scene();
        let actor = ObjectKey {
            elevation: 0,
            ordinal: 0,
        };
        let mut route = world.plan_actor(&view, actor, 20097, true).unwrap().unwrap();
        assert_eq!(world.advance_actor(&view, &mut route).unwrap(), ActorStep::Moved {
            tile: 20098,
            rotation: 1,
        });
        assert_eq!(world.snapshot().objects.elevations[0][0].record.header.tile, 20098);
        assert_eq!(world.snapshot().objects.elevations[0][0].record.header.rotation, 1);
        assert_eq!(world.snapshot().collision.tile_of(actor), Some(20098));
        assert_eq!(world.advance_actor(&view, &mut route).unwrap(), ActorStep::Moved {
            tile: 20097,
            rotation: 1,
        });
        assert_eq!(world.snapshot().objects.elevations[0][0].record.header.tile, 20097);
        assert_eq!(world.snapshot().collision.tile_of(actor), Some(20097));
        assert_eq!(world.advance_actor(&view, &mut route).unwrap(), ActorStep::Finished);
    }

    #[test]
    fn toggling_visibility_updates_original_object_record_and_collision() {
        let view = HexViewport::new(200, 200, 640, 380, 20099).unwrap();
        let mut world = scene();
        let wall = ObjectKey {
            elevation: 0,
            ordinal: 1,
        };
        assert!(!world.snapshot().collision.is_passable(&view, 20100, 0, None));
        world.set_object_flags(wall, 0x01).unwrap(); // CE HIDDEN
        assert_eq!(world.snapshot().objects.elevations[0][1].record.header.flags, 0x01);
        assert!(world.snapshot().collision.is_passable(&view, 20100, 0, None));
        assert!(world.set_object_flags(ObjectKey { elevation: 0, ordinal: 99 }, 0).is_err());
    }
}
