//! Read-only Fallout 1 CE MAP loading assembly.
//!
//! Mirrors map_load_file's data order: header, global/local variables,
//! floor/roof squares, script records, and object/inventory tables.
//! Only source file formats and collision are loaded. This is not a running
//! gameplay world: no scripts, party, lighting, time, or actor movement.
use crate::assets::{Bank, GameAssets};
use crate::collision::{CollisionIndex, ObjectKey};
use crate::game_globals::GameGlobalState;
use crate::hex::HexViewport;
use crate::map::{MapHeader, MapSquares, MapVariables};
use crate::object_tables::ObjectTables;
use crate::pathfinding::make_path;
use crate::prototype::PrototypeBody;
use crate::script_binding::{
    bind_script_procedures, bind_scripts, install_map_scripts, BoundScriptRuntime, ScriptBinding,
    ScriptProcedureBinding,
};
use crate::script_data::ScriptTables;
use crate::script_event_state::DetachedScriptEvent;
use crate::script_lifecycle::PlannedMapAction;
use crate::script_locals::{MapScriptChangeSummary, MapScriptLocals};
use crate::script_native_vars::{
    apply_map_variable_opcode, apply_readonly_event_native_opcode,
    apply_readonly_event_native_with_vars, ScriptClockContext, LOCAL_VAR_OPCODE, MAP_VAR_OPCODE,
    SCRIPT_OVERRIDES_OPCODE, SET_LOCAL_VAR_OPCODE, SET_MAP_VAR_OPCODE,
};
use crate::script_object_binding::MapObjectReadBindings;
use crate::script_owner_binding::{MapScriptOwners, ScriptOwnerResolution};
use crate::script_runtime::PlannedLocalActionFrame;
use crate::script_vm::ScriptValue;
use std::collections::BTreeMap;
use std::io;

#[derive(Debug, Clone)]
pub struct StaticScene {
    pub header: MapHeader,
    pub variables: MapVariables,
    pub squares: MapSquares,
    pub scripts: ScriptTables,
    pub objects: ObjectTables,
    pub collision: CollisionIndex,
    /// End of the serialized map object's section, for format inspection.
    pub bytes_consumed: usize,
}

/// Owned, unstarted CE script environment: per-record VMs and MAP-local
/// slots are separate from game-wide GAME_GLOBAL_VARS. A missing installed
/// vault13.gam remains None; no quest default is invented.
#[derive(Debug, Clone)]
pub struct PreparedScriptEnvironment {
    pub scripts: BoundScriptRuntime,
    pub map_locals: MapScriptLocals,
    pub game_globals: Option<GameGlobalState>,
    /// Inert decoded object-query values with snapshot-local handles.
    pub object_queries: MapObjectReadBindings,
    /// Owner diagnostics from source-verified script SID and object SID joins.
    pub script_owners: MapScriptOwners,
    /// Original serialized MAP lastVisitTime in 1/10-second game ticks.
    /// Zero indicates no recorded visit; no gameplay time is advanced.
    pub map_last_visit_ticks: i32,
}

/// Pure sandbox result from a CLONED CE local script preview.
/// All state changes exist only in this returned value, never in the MAP,
/// installed VM, real GAME_GLOBAL_VARS or gameplay scheduler.
#[derive(Debug, Clone)]
pub struct PreviewedSandboxAction {
    pub returned: Option<ScriptValue>,
    pub map_locals: MapScriptLocals,
    pub game_globals: Option<GameGlobalState>,
    pub overrides: bool,
    baseline_map: MapScriptLocals,
    baseline_game_globals: Option<GameGlobalState>,
}

/// An inspection-only description of speculative script changes. No engine
/// state can be modified by constructing, reading or discarding this value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SandboxEffectSummary {
    pub map: MapScriptChangeSummary,
    pub quest_globals_changed: usize,
    pub overrides: bool,
}

impl PreviewedSandboxAction {
    /// Compare the hypothetical post-action state against its own exact
    /// pre-action snapshot; never infer changes from other MAP instances.
    pub fn effects(&self) -> io::Result<SandboxEffectSummary> {
        let map = self.map_locals.change_summary_since(&self.baseline_map)?;
        let quest_globals_changed = match (&self.baseline_game_globals, &self.game_globals) {
            (None, None) => 0,
            (Some(before), Some(after)) if before.values().len() == after.values().len() => before
                .values()
                .iter()
                .zip(after.values())
                .filter(|(left, right)| left != right)
                .count(),
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "CE sandbox quest-global layout changed",
                ));
            }
        };
        Ok(SandboxEffectSummary {
            map,
            quest_globals_changed,
            overrides: self.overrides,
        })
    }

    /// Explicitly CONSUME and discard this preview. Only the read-only
    /// summary survives. There is deliberately no commit/apply method:
    /// gameplay script dispatch and persistence remain unimplemented.
    pub fn discard(self) -> io::Result<SandboxEffectSummary> {
        self.effects()
    }
}

impl PreparedScriptEnvironment {
    /// Find the source-derived CE owner of an installed, unstarted Rust VM.
    /// Returns None when no verified instance/record mapping is available.
    pub fn owner_for_instance(&self, instance_id: u64) -> Option<ScriptOwnerResolution> {
        self.script_owners
            .resolution_for_instance(&self.scripts, instance_id)
    }

    /// Prepare event metadata for one actually installed INT instance.
    /// No event callbacks or script procedures execute here.
    pub fn detached_event_for_instance(&self, instance_id: u64) -> Option<DetachedScriptEvent> {
        self.owner_for_instance(instance_id)
            .map(DetachedScriptEvent::new)
    }

    /// Verify the source-derived local executeProc/setupCall frame of one
    /// already-selected MAP action. It does not create a live CE call frame,
    /// move a VM program counter, dispatch callbacks or invoke a procedure.
    pub fn local_frame_for_planned(
        &self,
        planned: &PlannedMapAction,
    ) -> io::Result<PlannedLocalActionFrame> {
        let _ = self.detached_event_for_planned(planned)?;
        let frame = self
            .scripts
            .runtime
            .inspect_local_action_frame(planned.instance_id, planned.action)?
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "CE planned local action procedure absent",
                )
            })?;
        if frame.procedure_index != planned.procedure_index {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "CE planned local action frame does not match procedure",
            ));
        }
        Ok(frame)
    }

    /// Run only the supported local core opcodes on a CLONED installed VM,
    /// after the MAP plan and original CE local frame have been checked.
    /// A no-value local return is None; unsupported original opcodes error.
    /// This diagnostic does not enter the game event loop, mutate script
    /// locals/quest state or use the native/engine callback host.
    pub fn preview_planned_local_core_bounded(
        &self,
        planned: &PlannedMapAction,
        max_steps: usize,
    ) -> io::Result<Option<ScriptValue>> {
        let frame = self.local_frame_for_planned(planned)?;
        let preview = self
            .scripts
            .runtime
            .preview_local_action_core_bounded(planned.instance_id, planned.action, max_steps)?
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "CE local action vanished between preflight and preview",
                )
            })?;
        if preview.frame != frame {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "CE local action frame changed during read-only preview",
            ));
        }
        Ok(preview.returned)
    }

    /// Run a validated local MAP script action on CLONED VM, MAP-variable,
    /// GAME_GLOBAL_VARS and invocation state. Only source-derived variable
    /// reads/writes, script_overrides, and pure object/clock/event queries
    /// can run. Every other native/gameplay opcode fails closed. Returned
    /// variable stores are speculative; caller must not confuse them with
    /// committed original CE state or actual quest effects.
    pub fn preview_planned_sandbox_bounded(
        &self,
        planned: &PlannedMapAction,
        clock: ScriptClockContext,
        max_steps: usize,
    ) -> io::Result<PreviewedSandboxAction> {
        let frame = self.local_frame_for_planned(planned)?;
        let event = self.detached_event_for_planned(planned)?;
        let snapshot = event.snapshot(&self.object_queries, clock)?;
        let mut map_locals = self.map_locals.clone();
        let mut game_globals = self.game_globals.clone();
        let baseline_map = map_locals.clone();
        let baseline_game_globals = game_globals.clone();
        let mut invocation = *event.invocation();
        let preview = self
            .scripts
            .runtime
            .preview_local_action_with_host_bounded(
                planned.instance_id,
                planned.action,
                max_steps,
                &mut |opcode, stack| match opcode {
                    LOCAL_VAR_OPCODE | SET_LOCAL_VAR_OPCODE | MAP_VAR_OPCODE
                    | SET_MAP_VAR_OPCODE => apply_map_variable_opcode(
                        &mut map_locals,
                        planned.instance_id,
                        opcode,
                        stack,
                    ),
                    crate::game_globals::GLOBAL_VAR_OPCODE
                    | crate::game_globals::SET_GLOBAL_VAR_OPCODE => game_globals
                        .as_mut()
                        .ok_or_else(|| {
                            io::Error::new(
                                io::ErrorKind::InvalidData,
                                "CE sandbox game globals absent",
                            )
                        })?
                        .apply_opcode(opcode, stack),
                    SCRIPT_OVERRIDES_OPCODE => {
                        invocation.mark_overrides();
                        Ok(())
                    }
                    _ => apply_readonly_event_native_opcode(snapshot, &invocation, opcode, stack),
                },
            )?
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "CE sandbox local action missing after preflight",
                )
            })?;
        if frame != preview.frame {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "CE sandbox action frame changed during preview",
            ));
        }
        Ok(PreviewedSandboxAction {
            returned: preview.returned,
            map_locals,
            game_globals,
            overrides: invocation.overrides(),
            baseline_map,
            baseline_game_globals,
        })
    }

    /// Preview only supported CE object, event and clock READ opcodes
    /// inside the local procedure, using a cloned installed VM and a pure
    /// native adapter. No global/local state writes, overrides, timers,
    /// gameplay callbacks, or live CE event dispatch can occur.
    pub fn preview_planned_event_reads_bounded(
        &self,
        planned: &PlannedMapAction,
        clock: ScriptClockContext,
        max_steps: usize,
    ) -> io::Result<Option<ScriptValue>> {
        let frame = self.local_frame_for_planned(planned)?;
        let event = self.detached_event_for_planned(planned)?;
        let snapshot = event.snapshot(&self.object_queries, clock)?;
        let preview = self
            .scripts
            .runtime
            .preview_local_action_with_host_bounded(
                planned.instance_id,
                planned.action,
                max_steps,
                &mut |opcode, stack| {
                    apply_readonly_event_native_with_vars(
                        snapshot,
                        event.invocation(),
                        &self.map_locals,
                        self.game_globals.as_ref(),
                        planned.instance_id,
                        opcode,
                        stack,
                    )
                },
            )?
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "CE read-only local action missing after preflight",
                )
            })?;
        if preview.frame != frame {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "CE read-only local action frame changed during preview",
            ));
        }
        Ok(preview.returned)
    }

    /// Preview an already-active, explicitly begun MAP script event
    /// against a CLONED installed INT. Unlike the initial-plan-only preview,
    /// this reads the retained per-script source/target/fixedParam context.
    /// Validation and execution are read-only: failure neither consumes the
    /// active event nor updates the registered VM, MAP or game globals.
    pub fn preview_active_event_reads_bounded(
        &self,
        sessions: &DetachedMapEventSessions,
        planned: &PlannedMapAction,
        clock: ScriptClockContext,
        max_steps: usize,
    ) -> io::Result<Option<ScriptValue>> {
        let frame = self.local_frame_for_planned(planned)?;
        let event = sessions.get(planned.instance_id).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "CE active event session missing",
            )
        })?;
        let action = i32::try_from(planned.action).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidData, "CE planned action exceeds i32")
        })?;
        if !event.is_active()
            || event.invocation().action() != action
            || Some(event.owner_resolution()) != self.owner_for_instance(planned.instance_id)
            || planned
                .external_parameter
                .is_some_and(|fixed| event.invocation().fixed_parameter() != fixed)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "CE active event session does not match planned action",
            ));
        }
        let snapshot = event.snapshot(&self.object_queries, clock)?;
        let preview = self
            .scripts
            .runtime
            .preview_local_action_with_host_bounded(
                planned.instance_id,
                planned.action,
                max_steps,
                &mut |opcode, stack| {
                    apply_readonly_event_native_with_vars(
                        snapshot,
                        event.invocation(),
                        &self.map_locals,
                        self.game_globals.as_ref(),
                        planned.instance_id,
                        opcode,
                        stack,
                    )
                },
            )?
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "CE active event local action missing after preflight",
                )
            })?;
        if preview.frame != frame {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "CE active event local frame changed during preview",
            ));
        }
        Ok(preview.returned)
    }

    /// Compose an already-planned CE MAP lifecycle action with its exact
    /// installed Rust VM record and source-derived owner. A stale or altered
    /// plan is rejected before allocating detached action metadata.
    /// No INT bytecode or native game callback runs.
    pub fn detached_event_for_planned(
        &self,
        planned: &PlannedMapAction,
    ) -> io::Result<DetachedScriptEvent> {
        let mut candidates = self
            .scripts
            .entries
            .iter()
            .filter(|entry| entry.instance_id == Some(planned.instance_id));
        let entry = candidates.next().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "CE planned script instance missing",
            )
        })?;
        if candidates.next().is_some()
            || entry.binding.script_id != planned.script_id
            || entry.binding.category != planned.category
            || entry.binding.ordinal != planned.ordinal
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "CE planned script record identity mismatch",
            ));
        }
        let procedure = self
            .scripts
            .runtime
            .resolve_action(planned.instance_id, planned.action)?;
        if procedure.is_none_or(|procedure| procedure.procedure_index != planned.procedure_index) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "CE planned script procedure mismatch",
            ));
        }
        let owner = self
            .owner_for_instance(planned.instance_id)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "CE planned script owner unresolved",
                )
            })?;
        let mut event = DetachedScriptEvent::new(owner);
        event.begin_action(
            planned.action,
            planned.external_parameter,
            &self.object_queries,
        )?;
        Ok(event)
    }
}

/// Opt-in detached CE script-event state keyed by installed Rust VM ID.
/// This is separate from MAP loading and the script scheduler. It never
/// executes an INT or invokes original game callbacks.
#[derive(Debug, Clone, Default)]
pub struct DetachedMapEventSessions {
    sessions: BTreeMap<u64, DetachedScriptEvent>,
}

impl DetachedMapEventSessions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, instance_id: u64) -> Option<&DetachedScriptEvent> {
        self.sessions.get(&instance_id)
    }

    /// Mimic an explicit scr_set_objs call for an installed script; absent
    /// participants are true NULL, and unsafe ObjectKeys are rejected.
    pub fn set_objs(
        &mut self,
        environment: &PreparedScriptEnvironment,
        instance_id: u64,
        source: Option<ObjectKey>,
        target: Option<ObjectKey>,
    ) -> io::Result<()> {
        let owner = environment.owner_for_instance(instance_id).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "CE script event instance unresolved",
            )
        })?;
        // Stage changes before inserting a new session or mutating an
        // existing one. Invalid ObjectKeys must not leave empty entries.
        let mut candidate = self
            .sessions
            .get(&instance_id)
            .copied()
            .unwrap_or_else(|| DetachedScriptEvent::new(owner));
        if candidate.owner_resolution() != owner {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "CE script event session owner changed",
            ));
        }
        candidate.set_objs(&environment.object_queries, source, target)?;
        self.sessions.insert(instance_id, candidate);
        Ok(())
    }

    /// Revalidate the exact planned VM/SID/procedure before accessing any
    /// per-script state. Existing target and fixed parameter persist.
    /// An unsuccessful entry leaves the previous event metadata untouched.
    pub fn begin_planned<'a>(
        &'a mut self,
        environment: &PreparedScriptEnvironment,
        planned: &PlannedMapAction,
    ) -> io::Result<&'a mut DetachedScriptEvent> {
        // A planning record alone is insufficient to authorize a detached
        // action: inspect its actual local CE executeProc body as well.
        let _ = environment.local_frame_for_planned(planned)?;
        let owner = environment
            .owner_for_instance(planned.instance_id)
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "CE script event owner missing")
            })?;
        let mut candidate = self
            .sessions
            .get(&planned.instance_id)
            .copied()
            .unwrap_or_else(|| DetachedScriptEvent::new(owner));
        if candidate.owner_resolution() != owner {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "CE script event session owner changed",
            ));
        }
        candidate.begin_action(
            planned.action,
            planned.external_parameter,
            &environment.object_queries,
        )?;
        self.sessions.insert(planned.instance_id, candidate);
        self.sessions.get_mut(&planned.instance_id).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "CE script event session missing",
            )
        })
    }

    /// Explicit normal-completion boundary. The source is cleared and the
    /// target retained, just as in pinned CE exec_script_proc.
    pub fn finish(&mut self, instance_id: u64) -> io::Result<()> {
        self.sessions
            .get_mut(&instance_id)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "CE script event session missing",
                )
            })?
            .finish_action()
    }
}

impl StaticScene {
    /// Open an original MAP in the user's master game-data bank.
    pub fn load(assets: &mut GameAssets, path: &str) -> io::Result<Option<Self>> {
        match assets.read(Bank::Master, path)? {
            Some(bytes) => Self::decode(assets, &bytes).map(Some),
            None => Ok(None),
        }
    }

    /// Construct a static scene without executing any script.
    pub fn decode(assets: &mut GameAssets, bytes: &[u8]) -> io::Result<Self> {
        let header = MapHeader::decode(bytes)?;
        let variables = header.decode_variables(bytes)?;
        let squares = header.decode_squares(bytes)?;
        let (scripts, objects, bytes_consumed) = squares.decode_objects_with(bytes, |pid| {
            let proto = assets.prototype_by_pid(pid)?.ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("missing original prototype {pid:#010X}"),
                )
            })?;
            Ok(match proto.body {
                PrototypeBody::Item { item_type, .. } => Some(item_type),
                PrototypeBody::Scenery { scenery_type, .. } => Some(scenery_type),
                _ => None,
            })
        })?;
        let collision = CollisionIndex::from_map(&objects);
        Ok(Self {
            header,
            variables,
            squares,
            scripts,
            objects,
            collision,
            bytes_consumed,
        })
    }

    /// Construct immutable native object reads from top-level MAP objects.
    /// Serialized object IDs may repeat; handles use ObjectKey identity.
    pub fn object_query_snapshot(&self) -> io::Result<MapObjectReadBindings> {
        MapObjectReadBindings::from_tables(&self.objects)
    }

    /// Read-only association between each serialized MAP script and its
    /// installed .INT path/flags. Does not execute script bytecode.
    pub fn script_bindings(&self, assets: &mut GameAssets) -> io::Result<Vec<ScriptBinding>> {
        bind_scripts(assets, &self.scripts)
    }

    /// Resolve installed INT procedure actions for a MAP without running
    /// the VM or invoking engine callbacks. Missing assets stay visible.
    pub fn script_procedure_bindings(
        &self,
        assets: &mut GameAssets,
    ) -> io::Result<Vec<ScriptProcedureBinding>> {
        bind_script_procedures(assets, &self.scripts)
    }

    /// Decode installed .INT programs into independent, unstarted Rust VMs.
    /// This is a read-only preparation path, not map script lifecycle entry.
    pub fn unstarted_script_runtime(
        &self,
        assets: &mut GameAssets,
    ) -> io::Result<BoundScriptRuntime> {
        install_map_scripts(assets, &self.scripts)
    }

    /// Prepare the decoded MAP's attached Rust script instances together
    /// with independent mutable MAP-local storage. This does not execute
    /// START, MAP_ENTER, game native opcodes or any background event.
    pub fn unstarted_script_state(
        &self,
        assets: &mut GameAssets,
    ) -> io::Result<(BoundScriptRuntime, MapScriptLocals)> {
        let scripts = self.unstarted_script_runtime(assets)?;
        let locals = scripts.script_local_state(&self.variables)?;
        Ok((scripts, locals))
    }

    /// Prepare installed MAP scripts, their mutable MAP slots and separate
    /// game-wide quest defaults as owned Rust state. This is a read/decode
    /// boundary only: no procedure, native opcode, timer or world update
    /// is executed by this function.
    pub fn unstarted_script_environment(
        &self,
        assets: &mut GameAssets,
    ) -> io::Result<PreparedScriptEnvironment> {
        let (scripts, map_locals) = self.unstarted_script_state(assets)?;
        let game_globals = assets.game_global_defaults()?;
        let object_queries = self.object_query_snapshot()?;
        let script_owners =
            MapScriptOwners::from_map(&self.scripts, &self.objects, &object_queries)?;
        Ok(PreparedScriptEnvironment {
            scripts,
            map_locals,
            game_globals,
            object_queries,
            script_owners,
            map_last_visit_ticks: self.header.last_visit_time,
        })
    }

    /// CE-style numeric route through the initial static object collision.
    /// The caller supplies a viewport and the actor's current identity.
    /// Door scripts and moving blockers are not handled here.
    pub fn route(
        &self,
        view: &HexViewport,
        from: i32,
        to: i32,
        elevation: usize,
        excluded_actor: Option<ObjectKey>,
        not_in_combat: bool,
    ) -> Option<Vec<u8>> {
        make_path(view, from, to, not_in_combat, true, |tile| {
            self.collision
                .is_passable(view, tile, elevation, excluded_actor)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::StaticScene;
    use crate::assets::GameAssets;
    use crate::hex::HexViewport;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn composes_all_empty_map_sections_and_routes_without_runtime() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("rust-age-scene-{}-{stamp}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("master.dat"), [0_u8; 16]).unwrap();
        fs::write(root.join("critter.dat"), [0_u8; 16]).unwrap();
        fs::create_dir_all(root.join("data")).unwrap();
        fs::write(
            root.join("data").join("VAULT13.GAM"),
            b"GAME_GLOBAL_VARS:\nA=7\nB=-13\n",
        )
        .unwrap();
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&19_i32.to_be_bytes());
        bytes.extend_from_slice(b"TEST.MAP\0\0\0\0\0\0\0\0");
        for value in [20099_i32, 0, 0, 1, -1, 2 | 4 | 8, 0, 2, 0, 0] {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        bytes.extend_from_slice(&[0_u8; 44 * 4]);
        for value in [12_i32, -8, 99] {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        bytes.extend_from_slice(&[0_u8; 20 + 16]);
        let mut assets = GameAssets::open(&root).unwrap();
        let scene = StaticScene::decode(&mut assets, &bytes).unwrap();
        assert_eq!(scene.variables.globals, [12, -8]);
        assert_eq!(scene.variables.locals, [99]);
        assert_eq!(scene.scripts.script_count(), 0);
        assert_eq!(scene.objects.total_objects, 0);
        assert_eq!(scene.bytes_consumed, bytes.len());
        let (prepared, locals) = scene.unstarted_script_state(&mut assets).unwrap();
        assert!(prepared.runtime.is_empty());
        assert!(prepared.entries.is_empty());
        assert_eq!(locals.globals(), [12, -8]);
        assert_eq!(locals.locals(), [99]);
        assert_eq!(scene.variables.locals, [99]); // no mutation of MAP
        let environment = scene.unstarted_script_environment(&mut assets).unwrap();
        assert!(environment.scripts.runtime.is_empty());
        assert!(environment.object_queries.snapshots().is_empty());
        assert!(environment.script_owners.entries().is_empty());
        assert_eq!(environment.owner_for_instance(1), None);
        assert_eq!(environment.map_locals.globals(), [12, -8]);
        assert_eq!(environment.map_locals.locals(), [99]);
        assert_eq!(
            environment.map_last_visit_ticks,
            scene.header.last_visit_time
        );
        assert_eq!(environment.game_globals.unwrap().values(), [7, -13]);
        assert_eq!(scene.variables.globals, [12, -8]); // game globals stay separate.
        fs::remove_file(root.join("data").join("VAULT13.GAM")).unwrap();
        assert!(scene
            .unstarted_script_environment(&mut assets)
            .unwrap()
            .game_globals
            .is_none());
        let view = HexViewport::new(200, 200, 640, 380, 20099).unwrap();
        assert_eq!(
            scene.route(&view, 20099, 20098, 0, None, true),
            Some(vec![1])
        );
        fs::remove_dir_all(root).unwrap();
    }
}
