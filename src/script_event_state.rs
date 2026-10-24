//! Detached CE script source/target lifecycle, never live event dispatch.
//!
//! Pinned Fallout 1 CE scripts.cc scr_set_objs and exec_script_proc:
//! scr_set_objs assigns both pointers; exec_script_proc resets overrides,
//! fills a missing target from script->owner, and on normal completion
//! clears source but retains target. fixedParam is assigned by the caller
//! and survives subsequent procedures.
//! All handles below refer only to immutable top-level MAP snapshots.
use crate::collision::ObjectKey;
use crate::script_native_vars::{
    ScriptClockContext, ScriptInvocationState, ScriptNativeEventSnapshot,
    ScriptNativeOwnerSnapshot, ScriptNativeSnapshot,
};
use crate::script_object_binding::MapObjectReadBindings;
use crate::script_owner_binding::ScriptOwnerResolution;
use crate::script_procedures::SCRIPT_PROC_NAMES;
use std::io;

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DetachedScriptEvent {
    owner: ScriptOwnerResolution,
    source: i32,
    target: i32,
    invocation: ScriptInvocationState,
    active: bool,
}

impl DetachedScriptEvent {
    pub fn new(owner: ScriptOwnerResolution) -> Self {
        Self {
            owner,
            source: 0,
            target: 0,
            invocation: ScriptInvocationState::installed(),
            active: false,
        }
    }

    pub fn invocation(&self) -> &ScriptInvocationState {
        &self.invocation
    }

    pub fn owner_resolution(&self) -> ScriptOwnerResolution {
        self.owner
    }

    /// Mutable CE action/overrides state, available only during an explicit
    /// detached invocation. The bounded native host may update overrides.
    pub fn active_invocation_mut(&mut self) -> io::Result<&mut ScriptInvocationState> {
        if !self.active {
            return Err(invalid("CE event has no active script action"));
        }
        Ok(&mut self.invocation)
    }

    pub fn source_handle(&self) -> i32 {
        self.source
    }

    pub fn target_handle(&self) -> i32 {
        self.target
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    /// CE scr_set_objs equivalent restricted to two explicitly selected
    /// top-level objects. None means a real NULL (not a missing host).
    /// Prevalidates both operands, so a bad target leaves source unchanged.
    pub fn set_objs(
        &mut self,
        objects: &MapObjectReadBindings,
        source: Option<ObjectKey>,
        target: Option<ObjectKey>,
    ) -> io::Result<()> {
        if self.active {
            return Err(invalid("cannot replace CE event participants mid-preview"));
        }
        let source = resolve_key(objects, source)?;
        let target = resolve_key(objects, target)?;
        self.source = source;
        self.target = target;
        Ok(())
    }

    /// Enter one synthetic CE script action without executing bytecode.
    /// A null target inherits the verified self owner. If CE would need
    /// runtime-generated spatial or inventory owners, fail closed instead.
    pub fn begin_action(
        &mut self,
        action: usize,
        fixed_param: Option<i32>,
        objects: &MapObjectReadBindings,
    ) -> io::Result<()> {
        if self.active {
            return Err(invalid("CE detached script action already active"));
        }
        if action >= SCRIPT_PROC_NAMES.len() {
            return Err(invalid("CE script action outside 24-slot procedure table"));
        }
        let inherited_target = if self.target == 0 {
            match self.owner {
                ScriptOwnerResolution::Bound { handle, .. }
                | ScriptOwnerResolution::GeneratedSystemBound { handle } => handle,
                ScriptOwnerResolution::NullOwner => 0,
                _ => {
                    return Err(invalid(
                        "CE event target fallback requires unresolved engine owner",
                    ));
                }
            }
        } else {
            self.target
        };
        check_handle(objects, inherited_target)?;
        check_handle(objects, self.source)?;
        let action = i32::try_from(action).map_err(|_| invalid("CE action outside signed i32"))?;
        if !self.invocation.begin_action(action) {
            return Err(invalid("missing CE script invocation"));
        }
        if let Some(value) = fixed_param {
            self.invocation.set_fixed_parameter(value);
        }
        self.target = inherited_target;
        self.active = true;
        Ok(())
    }

    /// Use only while an explicitly begun action is active. NULL is Some(0).
    /// This does not call scripts, allocate objects, or mutate MAP values.
    pub fn snapshot<'a>(
        &self,
        objects: &'a MapObjectReadBindings,
        clock: ScriptClockContext,
    ) -> io::Result<ScriptNativeEventSnapshot<'a>> {
        if !self.active {
            return Err(invalid("CE event snapshot requires active detached action"));
        }
        check_handle(objects, self.source)?;
        check_handle(objects, self.target)?;
        Ok(ScriptNativeEventSnapshot {
            owner: ScriptNativeOwnerSnapshot {
                native: ScriptNativeSnapshot {
                    clock,
                    objects: Some(objects.snapshots()),
                },
                resolution: Some(self.owner),
            },
            source: Some(self.source),
            target: Some(self.target),
        })
    }

    /// Normal CE exec_script_proc completion clears source but not target.
    /// A failed/halted native preview must not call this automatically.
    pub fn finish_action(&mut self) -> io::Result<()> {
        if !self.active {
            return Err(invalid("CE detached action has not begun"));
        }
        self.source = 0;
        self.active = false;
        Ok(())
    }
}

fn resolve_key(objects: &MapObjectReadBindings, key: Option<ObjectKey>) -> io::Result<i32> {
    match key {
        Some(key) => objects
            .handle_for(key)
            .ok_or_else(|| invalid("CE detached event ObjectKey not in top-level MAP")),
        None => Ok(0),
    }
}

fn check_handle(objects: &MapObjectReadBindings, handle: i32) -> io::Result<()> {
    // Top-level MAP objects have ObjectKeys, while CE's source-created
    // header system Object is nonserialized and intentionally has none.
    // Both are valid only when registered under one unique opaque handle.
    if handle != 0
        && objects
            .snapshots()
            .iter()
            .filter(|snapshot| snapshot.handle == handle)
            .count()
            != 1
    {
        return Err(invalid("CE event pointer not in detached MAP snapshot"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::DetachedScriptEvent;
    use crate::collision::ObjectKey;
    use crate::game_globals::GameGlobalState;
    use crate::map::MapVariables;
    use crate::object_data::{ObjectHeader, ObjectRecord, ObjectUpdate};
    use crate::object_tables::{MapObject, ObjectTables};
    use crate::script_binding::BoundScriptRuntime;
    use crate::script_clock::ScriptGameClock;
    use crate::script_native_vars::{
        run_invocation_with_event_snapshot_bounded, ScriptClockContext,
    };
    use crate::script_object_binding::MapObjectReadBindings;
    use crate::script_owner_binding::ScriptOwnerResolution;
    use crate::script_program::ScriptProgram;
    use crate::script_runtime::ScriptRuntime;
    use crate::script_vm::{ScriptValue, ScriptVm};

    fn keys() -> MapObjectReadBindings {
        let object = |id, sid, tile| MapObject {
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
                    fid: 1,
                    flags: 0,
                    elevation: 0,
                    pid: 0x0300_0001,
                    cid: -1,
                    light_distance: 0,
                    light_intensity: 0,
                    serialized_outline: 0,
                    script_id: sid,
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
        };
        let objects = ObjectTables {
            elevations: [
                vec![object(77, 0x0300_0001, 20099), object(77, -1, 20100)],
                vec![],
                vec![],
            ],
            total_objects: 2,
            bytes_consumed: 0,
        };
        MapObjectReadBindings::from_tables(&objects).unwrap()
    }

    fn owner() -> ScriptOwnerResolution {
        ScriptOwnerResolution::Bound {
            key: ObjectKey {
                elevation: 0,
                ordinal: 0,
            },
            handle: 1,
            saved_oid_matches: true,
        }
    }

    fn key(ordinal: usize) -> ObjectKey {
        ObjectKey {
            elevation: 0,
            ordinal,
        }
    }

    fn clock() -> ScriptClockContext {
        ScriptClockContext {
            clock: ScriptGameClock { ticks: 0 },
            last_visit_ticks: None,
        }
    }

    #[test]
    fn ce_sets_both_pointers_target_persists_and_source_clears() {
        let objects = keys();
        let mut event = DetachedScriptEvent::new(owner());
        assert!(event.snapshot(&objects, clock()).is_err());
        assert!(event.active_invocation_mut().is_err());
        event.set_objs(&objects, Some(key(1)), None).unwrap();
        event.begin_action(6, Some(19), &objects).unwrap();
        let snapshot = event.snapshot(&objects, clock()).unwrap();
        assert_eq!(snapshot.source, Some(2));
        assert_eq!(snapshot.target, Some(1)); // target inherited from self
        assert_eq!(event.invocation().action(), 6);
        assert_eq!(event.invocation().fixed_parameter(), 19);
        assert!(!event.active_invocation_mut().unwrap().overrides());
        assert!(event.begin_action(8, None, &objects).is_err());
        event.finish_action().unwrap();
        assert_eq!(event.source_handle(), 0);
        assert_eq!(event.target_handle(), 1);
        assert!(event.finish_action().is_err());
        event.begin_action(23, None, &objects).unwrap();
        assert_eq!(event.snapshot(&objects, clock()).unwrap().source, Some(0));
        assert_eq!(event.snapshot(&objects, clock()).unwrap().target, Some(1));
        assert_eq!(event.invocation().fixed_parameter(), 19);
    }

    #[test]
    fn detached_event_metadata_feeds_bounded_int_and_preserves_cleanup_rules() {
        let objects = keys();
        let mut event = DetachedScriptEvent::new(owner());
        event.set_objs(&objects, Some(key(1)), None).unwrap();
        event.begin_action(6, Some(19), &objects).unwrap();
        let snapshot = event.snapshot(&objects, clock()).unwrap();

        // Native self_obj, source_obj, game action and overrides share one
        // explicitly bounded INT trace; the caller controls the lifecycle.
        let ops = [
            0x80BD_u16, // source_obj
            0x80D4,     // tile_num
            0x80BC,     // self_obj
            0x8100,     // obj_pid
            0x8039,     // add
            0x80C7,     // script_action
            0x8039,     // add
            0x80B9,     // script_overrides
            0x8011,     // stop
        ];
        let mut program = vec![0_u8; 42];
        for (index, opcode) in ops.iter().enumerate() {
            program[index * 2..index * 2 + 2].copy_from_slice(&opcode.to_be_bytes());
        }
        program.extend_from_slice(&[0_u8; 12]);
        let mut vm = ScriptVm::new(ScriptProgram::decode(&program).unwrap());
        let state = BoundScriptRuntime {
            runtime: ScriptRuntime::new(),
            entries: vec![],
        };
        let mut locals = state
            .script_local_state(&MapVariables {
                globals: vec![],
                locals: vec![],
            })
            .unwrap();
        let mut globals = GameGlobalState::from_initial_values(&[]).unwrap();
        assert_eq!(
            run_invocation_with_event_snapshot_bounded(
                &mut vm,
                1,
                &mut locals,
                &mut globals,
                event.active_invocation_mut().unwrap(),
                snapshot,
                12,
            )
            .unwrap(),
            9
        );
        assert!(vm.halted());
        assert_eq!(vm.stack(), [ScriptValue::Int(20100 + 0x0300_0001 + 6)]);
        assert!(event.invocation().overrides());
        event.finish_action().unwrap();
        assert_eq!(event.source_handle(), 0);
        assert_eq!(event.target_handle(), 1);
        event.begin_action(23, None, &objects).unwrap();
        assert!(!event.invocation().overrides()); // CE resets for next action
        assert_eq!(event.invocation().fixed_parameter(), 19);
        let second = event.snapshot(&objects, clock()).unwrap();
        assert_eq!(second.source, Some(0));
        assert_eq!(second.target, Some(1));
    }

    #[test]
    fn explicit_target_survives_missing_owner_with_target_not_guessed() {
        let objects = keys();
        let mut event = DetachedScriptEvent::new(ScriptOwnerResolution::SpatialOwnerRequiresEngine);
        event.set_objs(&objects, None, Some(key(1))).unwrap();
        event.begin_action(15, None, &objects).unwrap();
        assert_eq!(event.snapshot(&objects, clock()).unwrap().target, Some(2));
        event.finish_action().unwrap();
        event.set_objs(&objects, None, None).unwrap();
        assert!(event.begin_action(15, None, &objects).is_err());
        assert!(!event.is_active());
        assert_eq!(event.target_handle(), 0);
    }

    #[test]
    fn source_created_header_object_supports_native_self_and_inherited_target() {
        let empty = ObjectTables {
            elevations: std::array::from_fn(|_| vec![]),
            total_objects: 0,
            bytes_consumed: 0,
        };
        let mut objects = MapObjectReadBindings::from_tables(&empty).unwrap();
        let owner_handle = objects.add_generated_header_system_object().unwrap();
        assert_eq!(owner_handle, 1);
        assert_eq!(objects.key_for(owner_handle), None);
        assert!(objects.add_generated_header_system_object().is_err());

        let mut event =
            DetachedScriptEvent::new(ScriptOwnerResolution::GeneratedSystemBound {
                handle: owner_handle,
            });
        event.begin_action(15, Some(1), &objects).unwrap();
        let snapshot = event.snapshot(&objects, clock()).unwrap();
        assert_eq!(snapshot.target, Some(owner_handle));
        assert_eq!(snapshot.source, Some(0));
        for (which, property, expected) in [
            (
                crate::script_native_vars::SELF_OBJ_OPCODE,
                crate::script_object_read::OBJ_PID_OPCODE,
                -1,
            ),
            (
                crate::script_native_vars::TARGET_OBJ_OPCODE,
                crate::script_object_read::TILE_NUM_OPCODE,
                1,
            ),
            (
                crate::script_native_vars::SELF_OBJ_OPCODE,
                crate::script_object_read::OBJ_ART_FID_OPCODE,
                0x0500_000C,
            ),
        ] {
            let mut stack = Vec::new();
            crate::script_native_vars::apply_readonly_event_native_opcode(
                snapshot,
                event.invocation(),
                which,
                &mut stack,
            )
            .unwrap();
            assert_eq!(stack, [ScriptValue::Pointer(owner_handle)]);
            crate::script_native_vars::apply_readonly_event_native_opcode(
                snapshot,
                event.invocation(),
                property,
                &mut stack,
            )
            .unwrap();
            assert_eq!(stack, [ScriptValue::Int(expected)]);
        }
        event.finish_action().unwrap();

        // No ObjectKey or forged native pointer can alias the nonserialized
        // header object. Explicitly reject unregistered owner handles.
        let mut forged =
            DetachedScriptEvent::new(ScriptOwnerResolution::GeneratedSystemBound {
                handle: owner_handle + 1,
            });
        assert!(forged.begin_action(15, None, &objects).is_err());
        assert!(!forged.is_active());
    }

    #[test]
    fn invalid_updates_are_atomic_and_unowned_nonspatial_is_null() {
        let objects = keys();
        let mut event = DetachedScriptEvent::new(ScriptOwnerResolution::NullOwner);
        event.set_objs(&objects, Some(key(0)), None).unwrap();
        assert!(event.set_objs(&objects, None, Some(key(99))).is_err());
        assert_eq!(event.source_handle(), 1);
        assert_eq!(event.target_handle(), 0);
        assert!(event.begin_action(99, None, &objects).is_err());
        event.begin_action(1, None, &objects).unwrap();
        assert_eq!(event.snapshot(&objects, clock()).unwrap().target, Some(0));
        assert!(event.set_objs(&objects, None, None).is_err());
    }
}
