//! Minimal mutable Fallout 1 CE scene-state bridge.
//!
//! Actor navigation updates both the deserialized MAP object header and
//! the live collision index. This is not the full CE obj_move_to_tile:
//! triggers, scripts, lighting, roof, saves, time, and combat are absent.
use crate::actor_route::{ActorRoute, ActorStep};
use crate::collision::ObjectKey;
use crate::hex::HexViewport;
use crate::scene::{PreparedScriptEnvironment, StaticScene};
use crate::script_lifecycle::PlannedMapAction;
use crate::script_native_vars::ScriptClockContext;
use crate::script_object_binding::MapObjectReadBindings;
use crate::script_owner_binding::MapScriptOwners;
use crate::script_runtime::PreparedCoreTick;
use std::io;

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

/// Finite instruction budgets for one atomic actor movement and CE
/// script-state tick. This is a Rust safety bound, not CE's CPU default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptTickBudget {
    pub now_millis: u32,
    pub cpu_burst: usize,
    pub max_steps_per_condition: usize,
}

/// Atomic actor/spatial dispatch result. Critical local procedures run
/// immediately; noncritical calls remain prepared for the interpreter tick.
#[derive(Debug, Clone)]
pub struct SpatialActorTick {
    pub actor: ActorStep,
    pub spatial: Vec<(PlannedMapAction, Option<usize>)>,
    pub tick: PreparedCoreTick,
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

    /// Refresh caller-owned native object data after route/movement updates.
    /// Previously returned snapshots retain their earlier property values.
    pub fn object_query_snapshot(&self) -> io::Result<MapObjectReadBindings> {
        self.snapshot.object_query_snapshot()
    }

    /// Advance installed Rust-owned MAP scripts against the CURRENT
    /// mutable scene's object positions, rather than stale decoded object
    /// snapshots. CE object identity is tied to an existing table slot and
    /// opaque handle; this bridge rejects changed object counts, recycled
    /// PID slots, altered script bindings, or changed owner resolutions.
    ///
    /// The environment is cloned, re-bound to the live object snapshots,
    /// and committed only after a full bounded native/script tick succeeds.
    /// Source MAP bytes, object positions and collision state do not roll
    /// back here because they were changed by earlier actor operations.
    /// This tests world-property -> CE script-condition integration, NOT
    /// automatic spatial triggers, obj_move_to_tile scripts, or save files.
    /// Verify that installed script owners and MAP object identities
    /// still refer to the same live scene, then refresh read-only native
    /// properties without evaluating or consuming any script event.
    fn refreshed_script_environment(
        &self,
        environment: &PreparedScriptEnvironment,
    ) -> io::Result<PreparedScriptEnvironment> {
        if environment.scripts.entries.len() != self.snapshot.scripts.script_count() {
            return Err(invalid("CE live scene script binding count changed"));
        }
        if environment.map_locals.globals().len() != self.snapshot.variables.globals.len() {
            return Err(invalid("CE live scene MAP global layout changed"));
        }
        for entry in &environment.scripts.entries {
            let record = self
                .snapshot
                .scripts
                .categories
                .get(entry.binding.category)
                .and_then(|category| category.get(entry.binding.ordinal))
                .ok_or_else(|| invalid("CE live scene script binding record unavailable"))?;
            if record.script_id != entry.binding.script_id
                || record.script_index != entry.binding.script_index
                || record.local_variable_count != entry.binding.local_variable_count
                || record.local_variable_offset != entry.binding.local_variable_offset
            {
                return Err(invalid("CE live scene script record identity changed"));
            }
        }
        let queries = self.snapshot.object_query_snapshot()?;
        let previous = environment.object_queries.snapshots();
        let refreshed = queries.snapshots();
        if previous.len() != refreshed.len()
            || previous.iter().zip(refreshed).any(|(old, new)| {
                old.handle != new.handle
                    || old.pid != new.pid
                    || old.table_elevation != new.table_elevation
            })
        {
            return Err(invalid(
                "CE live scene object handle or PID identity changed",
            ));
        }
        let owners =
            MapScriptOwners::from_map(&self.snapshot.scripts, &self.snapshot.objects, &queries)?;
        for entry in &environment.scripts.entries {
            let Some(instance) = entry.instance_id else {
                continue;
            };
            if environment
                .script_owners
                .resolution_for_instance(&environment.scripts, instance)
                != owners.resolution_for_instance(&environment.scripts, instance)
            {
                return Err(invalid("CE live scene script owner identity changed"));
            }
        }
        let mut candidate = environment.clone();
        candidate.object_queries = queries;
        candidate.script_owners = owners;
        Ok(candidate)
    }

    pub fn advance_script_state_with_current_scene_bounded(
        &self,
        environment: &mut PreparedScriptEnvironment,
        clock: ScriptClockContext,
        now_millis: u32,
        cpu_burst: usize,
        max_steps_per_condition: usize,
    ) -> io::Result<PreparedCoreTick> {
        let mut candidate = self.refreshed_script_environment(environment)?;
        let tick = candidate.advance_script_state_tick_bounded(
            clock,
            now_millis,
            cpu_burst,
            max_steps_per_condition,
        )?;
        *environment = candidate;
        Ok(tick)
    }

    /// Apply one CE actor-route step and run the installed bounded
    /// script/native-state tick against the resulting CURRENT MAP object
    /// properties, all inside one caller-owned Rust transaction.
    ///
    /// Unlike the separate advance_actor + script-tick entry points, a
    /// failing native opcode, stale object owner binding or invalid route
    /// rolls back the actor position, rotation, collision, route cursor,
    /// script program frames, MAP variables and GAME_GLOBAL_VARS together.
    /// Blocked/Finished steps still perform the explicitly requested
    /// script tick, as this is an opt-in world-update operation.
    ///
    /// This is a safety boundary for Rust-owned scene state; it does not
    /// claim CE's live obj_move_to_tile scripting, spatial trigger dispatch,
    /// animation, combat side effects or original game save behavior.
    pub fn advance_actor_and_script_state_transactional_bounded(
        &mut self,
        view: &HexViewport,
        route: &mut ActorRoute,
        environment: &mut PreparedScriptEnvironment,
        clock: ScriptClockContext,
        budget: ScriptTickBudget,
    ) -> io::Result<(ActorStep, PreparedCoreTick)> {
        let mut candidate_world = self.clone();
        let mut candidate_route = route.clone();
        let mut candidate_environment = environment.clone();
        let step = candidate_world.advance_actor(view, &mut candidate_route)?;
        let tick = candidate_world.advance_script_state_with_current_scene_bounded(
            &mut candidate_environment,
            clock,
            budget.now_millis,
            budget.cpu_burst,
            budget.max_steps_per_condition,
        )?;
        *self = candidate_world;
        *route = candidate_route;
        *environment = candidate_environment;
        Ok((step, tick))
    }

    /// Apply scr_chk_spatials_in's source-ordered spatial script
    /// matching when an actual visible/non-flat MAP actor moves, then
    /// run the installed bounded CE script tick on its new properties.
    ///
    /// The original script record must identify a valid installed VM.
    /// scr_set_objs(script->sid, actor, NULL) uses a verified ObjectKey
    /// and NULL target; the opaque source handle is refreshed for reads.
    /// Critical spatial procedures run immediately, while noncritical
    /// procedures create deferred local CE call frames.
    ///
    /// This runs on cloned scene, route, collision and script/quest
    /// state. Any failure rolls back all of them, unlike original CE
    /// error side effects. Mouse cursors, explicit engine spatial-disable
    /// state, nested movement natives, visuals and saves remain unsupported.
    pub fn advance_actor_with_spatial_events_transactional_bounded(
        &mut self,
        view: &HexViewport,
        route: &mut ActorRoute,
        environment: &mut PreparedScriptEnvironment,
        clock: ScriptClockContext,
        budget: ScriptTickBudget,
    ) -> io::Result<SpatialActorTick> {
        let mut candidate_world = self.clone();
        let mut candidate_route = route.clone();
        let step = candidate_world.advance_actor(view, &mut candidate_route)?;
        let mut state = candidate_world.refreshed_script_environment(environment)?;
        let mut spatial = Vec::new();
        if let ActorStep::Moved { tile, .. } = step {
            let key = candidate_route.actor_key();
            let header = &candidate_world
                .snapshot
                .objects
                .elevations
                .get(key.elevation)
                .and_then(|objects| objects.get(key.ordinal))
                .ok_or_else(|| invalid("CE moving spatial source actor disappeared"))?
                .record
                .header;
            // CE skips OBJECT_HIDDEN and OBJECT_FLAT spatial sources.
            // Mouse cursors cannot appear in the decoded MAP object table.
            if header.flags & (0x01 | 0x08) == 0 {
                let selected = state.scripts.plan_spatial_at(
                    &candidate_world.snapshot.scripts,
                    view,
                    tile,
                    key.elevation,
                )?;
                let mut seen = std::collections::BTreeSet::new();
                for action in selected {
                    if !seen.insert(action.instance_id) {
                        return Err(invalid(
                            "CE spatial dispatch repeated one installed interpreter",
                        ));
                    }
                    state.spatial_sources.insert(action.instance_id, key);
                    let critical = state.local_frame_for_planned(&action)?.critical;
                    let executed = if critical {
                        Some(state.execute_critical_map_action_transactional_bounded(
                            &action,
                            clock,
                            budget.now_millis,
                            4096,
                        )?)
                    } else {
                        state.prepare_noncritical_map_action_transactional(&action)?;
                        None
                    };
                    spatial.push((action, executed));
                }
            }
        }
        let tick = state.advance_script_state_tick_bounded(
            clock,
            budget.now_millis,
            budget.cpu_burst,
            budget.max_steps_per_condition,
        )?;
        *self = candidate_world;
        *route = candidate_route;
        *environment = state;
        Ok(SpatialActorTick {
            actor: step,
            spatial,
            tick,
        })
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
    use super::{RuntimeScene, ScriptTickBudget};
    use crate::actor_route::ActorStep;
    use crate::collision::{CollisionIndex, ObjectKey};
    use crate::game_globals::GameGlobalState;
    use crate::hex::HexViewport;
    use crate::map::{MapHeader, MapSquares, MapVariables};
    use crate::object_data::{ObjectHeader, ObjectRecord, ObjectUpdate};
    use crate::object_tables::{MapObject, ObjectTables};
    use crate::scene::{PreparedScriptEnvironment, StaticScene};
    use crate::script_binding::{BoundScriptRuntime, InstalledScriptBinding, ScriptBinding};
    use crate::script_clock::ScriptGameClock;
    use crate::script_data::{ScriptRecord, ScriptTables};
    use crate::script_native_vars::ScriptClockContext;
    use crate::script_object_binding::MapObjectReadBindings;
    use crate::script_owner_binding::MapScriptOwners;
    use crate::script_program::ScriptProgram;
    use crate::script_runtime::{DueProcedure, ScriptRuntime};
    use crate::script_vm::ScriptVm;

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
    fn moved_map_actor_tile_becomes_a_live_ce_script_condition_without_stale_snapshots() {
        let mut world = scene();
        let actor = ObjectKey {
            elevation: 0,
            ordinal: 0,
        };
        world.snapshot.objects.elevations[0][0]
            .record
            .header
            .script_id = 17;
        world.snapshot.scripts.categories[0].push(ScriptRecord {
            script_id: 17,
            next: -1,
            subtype: vec![],
            flags: 0,
            script_index: 0,
            program_token: -1,
            owner_id: -1,
            local_variable_offset: -1,
            local_variable_count: 0,
            remaining: [0; 8],
        });

        // One due condition checks self_obj -> tile_num == destination.
        // An actual RuntimeScene actor step updates the MAP object header,
        // and the next CE doEvents condition reads the new property.
        // When true, a later interpreter burst writes the quest-global flag.
        let mut bytes = vec![0_u8; 42];
        bytes[0..2].copy_from_slice(&0x80BC_u16.to_be_bytes()); // SELF_OBJ
        bytes[2..4].copy_from_slice(&0x80D4_u16.to_be_bytes()); // TILE_NUM
        bytes[4..6].copy_from_slice(&0xC001_u16.to_be_bytes());
        bytes[6..10].copy_from_slice(&20098_i32.to_be_bytes());
        bytes[10..12].copy_from_slice(&0x8033_u16.to_be_bytes()); // EQUAL
        bytes[12..14].copy_from_slice(&0x8011_u16.to_be_bytes()); // STOP
        bytes[20..22].copy_from_slice(&0x801A_u16.to_be_bytes()); // POP arg
        bytes[22..24].copy_from_slice(&0xC001_u16.to_be_bytes());
        bytes[24..28].copy_from_slice(&0_i32.to_be_bytes()); // game-global index
        bytes[28..30].copy_from_slice(&0xC001_u16.to_be_bytes());
        bytes[30..34].copy_from_slice(&1_i32.to_be_bytes()); // quest state
        bytes[34..36].copy_from_slice(&0x80C6_u16.to_be_bytes()); // SET_GLOBAL_VAR
        bytes[36..38].copy_from_slice(&0x8021_u16.to_be_bytes()); // POP_FLAGS_EXIT
        bytes.extend_from_slice(&2_i32.to_be_bytes());
        let mut names = Vec::new();
        for (name, flags, condition_offset, body_offset) in [
            (b"main".as_slice(), 0_i32, 0_i32, 0_i32),
            (b"map_enter_p_proc".as_slice(), 0x02_i32, 0_i32, 20_i32),
        ] {
            let offset = 4 + names.len() as i32;
            for field in [offset, flags, 0, condition_offset, body_offset, 0] {
                bytes.extend_from_slice(&field.to_be_bytes());
            }
            names.extend_from_slice(name);
            names.push(0);
        }
        bytes.extend_from_slice(&(names.len() as i32).to_be_bytes());
        bytes.extend_from_slice(&names);
        bytes.extend_from_slice(&0_i32.to_be_bytes());
        let mut runtime = ScriptRuntime::new();
        runtime
            .register(1, ScriptVm::new(ScriptProgram::decode(&bytes).unwrap()))
            .unwrap();
        let scripts = BoundScriptRuntime {
            runtime,
            entries: vec![InstalledScriptBinding {
                binding: ScriptBinding {
                    category: 0,
                    ordinal: 0,
                    script_id: 17,
                    script_index: 0,
                    owner_id: -1,
                    path: None,
                    run_info: None,
                    local_variable_offset: -1,
                    local_variable_count: 0,
                },
                instance_id: Some(1),
            }],
        };
        let map_locals = scripts
            .script_local_state(&world.snapshot.variables)
            .unwrap();
        let queries = MapObjectReadBindings::from_tables(&world.snapshot.objects).unwrap();
        let owners =
            MapScriptOwners::from_map(&world.snapshot.scripts, &world.snapshot.objects, &queries)
                .unwrap();
        let mut environment = PreparedScriptEnvironment {
            scripts,
            map_locals,
            game_globals: Some(GameGlobalState::from_initial_values(&[0]).unwrap()),
            object_queries: queries,
            script_owners: owners,
            map_last_visit_ticks: 0,
            invocations: std::collections::BTreeMap::new(),
            spatial_sources: std::collections::BTreeMap::new(),
        };
        let clock = ScriptClockContext {
            clock: ScriptGameClock { ticks: 0 },
            last_visit_ticks: Some(0),
        };
        let before = world
            .advance_script_state_with_current_scene_bounded(&mut environment, clock, 100, 0, 5)
            .unwrap();
        assert!(before.prepared_calls.is_empty());
        assert_eq!(
            environment
                .scripts
                .runtime
                .vm(1)
                .unwrap()
                .runtime_procedures()[1]
                .flags,
            0x02
        );

        let view = HexViewport::new(200, 200, 640, 380, 20099).unwrap();
        let mut route = world
            .plan_actor(&view, actor, 20098, true)
            .unwrap()
            .unwrap();
        let (step, after) = world
            .advance_actor_and_script_state_transactional_bounded(
                &view,
                &mut route,
                &mut environment,
                clock,
                ScriptTickBudget {
                    now_millis: 101,
                    cpu_burst: 0,
                    max_steps_per_condition: 5,
                },
            )
            .unwrap();
        assert_eq!(
            step,
            ActorStep::Moved {
                tile: 20098,
                rotation: 1,
            }
        );
        assert_eq!(
            after.prepared_calls,
            [DueProcedure {
                program_id: 1,
                procedure_index: 1,
            }]
        );
        assert_eq!(environment.object_queries.snapshots()[0].tile, 20098);
        assert_eq!(environment.scripts.runtime.vm(1).unwrap().pc(), 20);
        assert_eq!(environment.game_globals.as_ref().unwrap().values(), [0]);

        let executed = world
            .advance_script_state_with_current_scene_bounded(&mut environment, clock, 102, 5, 5)
            .unwrap();
        assert_eq!(executed.executed_steps, [(1, 5)]);
        assert_eq!(environment.game_globals.as_ref().unwrap().values(), [1]);
        assert_eq!(environment.object_queries.snapshots()[0].tile, 20098);

        // Object identity recycling cannot silently rebind the installed
        // script's self_obj handle to another PID after a live MAP change.
        world.snapshot.objects.elevations[0][0].record.header.pid = 0x0100_0002;
        assert!(world
            .advance_script_state_with_current_scene_bounded(&mut environment, clock, 103, 0, 5,)
            .is_err());
        assert_eq!(environment.object_queries.snapshots()[0].pid, 0x0100_0001);
        assert_eq!(environment.scripts.runtime.vm(1).unwrap().pc(), 20);
        assert_eq!(environment.game_globals.as_ref().unwrap().values(), [1]);
    }

    #[test]
    fn actor_and_native_tick_failure_reverts_world_collision_route_and_vm() {
        let view = HexViewport::new(200, 200, 640, 380, 20099).unwrap();
        let mut world = scene();
        let actor = ObjectKey {
            elevation: 0,
            ordinal: 0,
        };
        let mut route = world
            .plan_actor(&view, actor, 20098, true)
            .unwrap()
            .unwrap();
        let bound = BoundScriptRuntime {
            runtime: ScriptRuntime::new(),
            entries: vec![],
        };
        let mut environment = PreparedScriptEnvironment {
            scripts: bound,
            map_locals: BoundScriptRuntime {
                runtime: ScriptRuntime::new(),
                entries: vec![],
            }
            .script_local_state(&world.snapshot().variables)
            .unwrap(),
            game_globals: None, // fail closed: no invented GAME_GLOBAL_VARS
            object_queries: world.object_query_snapshot().unwrap(),
            script_owners: MapScriptOwners::from_map(
                &world.snapshot().scripts,
                &world.snapshot().objects,
                &world.object_query_snapshot().unwrap(),
            )
            .unwrap(),
            map_last_visit_ticks: 0,
            invocations: std::collections::BTreeMap::new(),
            spatial_sources: std::collections::BTreeMap::new(),
        };
        let before_queries = environment.object_queries.clone();
        let clock = ScriptClockContext {
            clock: ScriptGameClock { ticks: 0 },
            last_visit_ticks: Some(0),
        };
        assert!(world
            .advance_actor_and_script_state_transactional_bounded(
                &view,
                &mut route,
                &mut environment,
                clock,
                ScriptTickBudget {
                    now_millis: 100,
                    cpu_burst: 1,
                    max_steps_per_condition: 0,
                },
            )
            .is_err());
        assert_eq!(route.current_tile(), 20099);
        assert_eq!(route.remaining_steps(), 1);
        assert_eq!(world.snapshot().collision.tile_of(actor), Some(20099));
        assert_eq!(
            world.snapshot().objects.elevations[0][0].record.header.tile,
            20099
        );
        assert_eq!(
            world.snapshot().objects.elevations[0][0]
                .record
                .header
                .rotation,
            0
        );
        assert_eq!(environment.object_queries, before_queries);
        assert!(environment.scripts.entries.is_empty());
        assert!(environment.game_globals.is_none());

        // Supplying the real caller-owned game-global bank permits a
        // successful movement/tick commit; do not repeat failed route steps.
        environment.game_globals = Some(GameGlobalState::from_initial_values(&[0]).unwrap());
        let (step, tick) = world
            .advance_actor_and_script_state_transactional_bounded(
                &view,
                &mut route,
                &mut environment,
                clock,
                ScriptTickBudget {
                    now_millis: 101,
                    cpu_burst: 1,
                    max_steps_per_condition: 0,
                },
            )
            .unwrap();
        assert!(matches!(step, ActorStep::Moved { tile: 20098, .. }));
        assert!(tick.executed_steps.is_empty());
        assert_eq!(route.current_tile(), 20098);
        assert_eq!(route.remaining_steps(), 0);
        assert_eq!(world.snapshot().collision.tile_of(actor), Some(20098));
        assert_eq!(environment.object_queries.snapshots()[0].tile, 20098);
    }

    #[test]
    fn steps_original_ce_actor_route_and_keeps_map_and_collision_in_sync() {
        let view = HexViewport::new(200, 200, 640, 380, 20099).unwrap();
        let mut world = scene();
        let actor = ObjectKey {
            elevation: 0,
            ordinal: 0,
        };
        let mut route = world
            .plan_actor(&view, actor, 20098, true)
            .unwrap()
            .unwrap();
        assert_eq!(
            world.advance_actor(&view, &mut route).unwrap(),
            ActorStep::Moved {
                tile: 20098,
                rotation: 1,
            }
        );
        assert_eq!(
            world.snapshot().objects.elevations[0][0].record.header.tile,
            20098
        );
        assert_eq!(
            world.snapshot().objects.elevations[0][0]
                .record
                .header
                .rotation,
            1
        );
        assert_eq!(world.snapshot().collision.tile_of(actor), Some(20098));
        assert_eq!(
            world.advance_actor(&view, &mut route).unwrap(),
            ActorStep::Finished
        );
    }

    #[test]
    fn native_object_snapshots_refresh_after_actor_route_movement() {
        let view = HexViewport::new(200, 200, 640, 380, 20099).unwrap();
        let mut world = scene();
        let actor = ObjectKey {
            elevation: 0,
            ordinal: 0,
        };
        let before = world.object_query_snapshot().unwrap();
        let handle = before.handle_for(actor).unwrap();
        assert_eq!(before.snapshots()[0].tile, 20099);
        let mut route = world
            .plan_actor(&view, actor, 20098, true)
            .unwrap()
            .unwrap();
        assert!(matches!(
            world.advance_actor(&view, &mut route).unwrap(),
            ActorStep::Moved { tile: 20098, .. }
        ));
        let after = world.object_query_snapshot().unwrap();
        assert_eq!(after.handle_for(actor), Some(handle));
        assert_eq!(after.key_for(handle), Some(actor));
        assert_eq!(after.snapshots()[0].tile, 20098);
        assert_eq!(before.snapshots()[0].tile, 20099);
        assert_eq!(after.snapshots()[1].tile, 20100);
    }

    #[test]
    fn toggling_visibility_updates_original_object_record_and_collision() {
        let view = HexViewport::new(200, 200, 640, 380, 20099).unwrap();
        let mut world = scene();
        let wall = ObjectKey {
            elevation: 0,
            ordinal: 1,
        };
        assert!(!world
            .snapshot()
            .collision
            .is_passable(&view, 20100, 0, None));
        world.set_object_flags(wall, 0x01).unwrap(); // CE HIDDEN
        assert_eq!(
            world.snapshot().objects.elevations[0][1]
                .record
                .header
                .flags,
            0x01
        );
        assert!(world
            .snapshot()
            .collision
            .is_passable(&view, 20100, 0, None));
        assert!(world
            .set_object_flags(
                ObjectKey {
                    elevation: 0,
                    ordinal: 99
                },
                0
            )
            .is_err());
    }
}
