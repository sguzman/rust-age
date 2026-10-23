//! Synthetic CE MAP lifecycle plan -> installed VM -> inert event metadata.
//! These are invented script and object records; no game procedure executes.
use rust_age::collision::ObjectKey;
use rust_age::map::MapVariables;
use rust_age::object_data::{ObjectHeader, ObjectRecord, ObjectUpdate};
use rust_age::object_tables::{MapObject, ObjectTables};
use rust_age::scene::{DetachedMapEventSessions, PreparedScriptEnvironment};
use rust_age::script_binding::{BoundScriptRuntime, InstalledScriptBinding, ScriptBinding};
use rust_age::script_clock::ScriptGameClock;
use rust_age::script_data::{ScriptRecord, ScriptTables};
use rust_age::script_lifecycle::MapLifecyclePhase;
use rust_age::script_native_vars::ScriptClockContext;
use rust_age::script_object_binding::MapObjectReadBindings;
use rust_age::script_owner_binding::MapScriptOwners;
use rust_age::script_program::ScriptProgram;
use rust_age::script_runtime::ScriptRuntime;
use rust_age::script_vm::{ScriptValue, ScriptVm};

fn synthetic_vm() -> ScriptVm {
    synthetic_vm_with_body(&[0x80, 0x1A, 0x80, 0x21])
}

fn synthetic_vm_with_body(body: &[u8]) -> ScriptVm {
    assert!(body.len() <= 22);
    let names: [&[u8]; 2] = [b"main", b"map_enter_p_proc"];
    let mut bytes = vec![0_u8; 42];
    bytes[20..20 + body.len()].copy_from_slice(body);
    bytes.extend_from_slice(&(names.len() as i32).to_be_bytes());
    let mut strings = Vec::new();
    for name in names {
        let offset = 4 + strings.len() as i32;
        for value in [offset, 0, 0, 0, 20, 0] {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        strings.extend_from_slice(name);
        strings.push(0);
    }
    bytes.extend_from_slice(&(strings.len() as i32).to_be_bytes());
    bytes.extend_from_slice(&strings);
    bytes.extend_from_slice(&0_i32.to_be_bytes());
    ScriptVm::new(ScriptProgram::decode(&bytes).unwrap())
}

fn environment() -> PreparedScriptEnvironment {
    let sid = 17;
    let record = ScriptRecord {
        script_id: sid,
        next: -1,
        subtype: vec![],
        flags: 0,
        script_index: 0,
        program_token: -1,
        owner_id: -1,
        local_variable_offset: -1,
        local_variable_count: 0,
        remaining: [0; 8],
    };
    let tables = ScriptTables {
        categories: [vec![record], vec![], vec![], vec![], vec![]],
        bytes_consumed: 0,
    };
    let mut runtime = ScriptRuntime::new();
    runtime.register(1, synthetic_vm()).unwrap();
    let bound = BoundScriptRuntime {
        runtime,
        entries: vec![InstalledScriptBinding {
            binding: ScriptBinding {
                category: 0,
                ordinal: 0,
                script_id: sid,
                script_index: 0,
                owner_id: -1,
                path: Some("scripts/TEST.int".to_owned()),
                run_info: None,
                local_variable_offset: -1,
                local_variable_count: 0,
            },
            instance_id: Some(1),
        }],
    };
    let map_locals = bound
        .script_local_state(&MapVariables {
            globals: vec![],
            locals: vec![],
        })
        .unwrap();
    let objects = ObjectTables {
        elevations: [vec![], vec![], vec![]],
        total_objects: 0,
        bytes_consumed: 0,
    };
    let queries = MapObjectReadBindings::from_tables(&objects).unwrap();
    let owners = MapScriptOwners::from_map(&tables, &objects, &queries).unwrap();
    PreparedScriptEnvironment {
        scripts: bound,
        map_locals,
        game_globals: None,
        object_queries: queries,
        script_owners: owners,
        map_last_visit_ticks: 0,
    }
}

#[test]
fn planner_checks_owner_sid_instance_procedure_and_preserves_inert_vm() {
    let env = environment();
    let actions = env
        .scripts
        .plan_map_lifecycle(MapLifecyclePhase::Enter, 999, 0)
        .unwrap();
    assert_eq!(actions.len(), 1);
    let action = actions[0];
    assert_eq!(action.script_id, 17);
    assert_eq!(action.procedure_index, 1);
    let frame = env.local_frame_for_planned(&action).unwrap();
    assert_eq!(frame.program_id, 1);
    assert_eq!(frame.action, 15);
    assert_eq!(frame.procedure_index, 1);
    assert_eq!(frame.body_offset, 20);
    assert_eq!(frame.saved_pc, 0);
    assert_eq!(frame.return_marker, 20);
    assert_eq!(frame.argument_marker, 0);
    assert!(!frame.critical);
    assert_eq!(
        env.preview_planned_local_core_bounded(&action, 4).unwrap(),
        None
    );
    assert!(env.preview_planned_local_core_bounded(&action, 1).is_err());
    let mut event = env.detached_event_for_planned(&action).unwrap();
    assert!(event.is_active());
    assert_eq!(event.invocation().action(), 15);
    assert_eq!(event.invocation().fixed_parameter(), 1);
    event.finish_action().unwrap();
    assert_eq!(event.target_handle(), 0);
    assert_eq!(env.scripts.runtime.vm(1).unwrap().pc(), 0);
    assert!(env.scripts.runtime.vm(1).unwrap().stack().is_empty());

    for wrong in [
        rust_age::script_lifecycle::PlannedMapAction {
            instance_id: 999,
            ..action
        },
        rust_age::script_lifecycle::PlannedMapAction {
            script_id: 18,
            ..action
        },
        rust_age::script_lifecycle::PlannedMapAction {
            ordinal: 1,
            ..action
        },
        rust_age::script_lifecycle::PlannedMapAction {
            procedure_index: 0,
            ..action
        },
        rust_age::script_lifecycle::PlannedMapAction {
            action: 23,
            ..action
        },
    ] {
        assert!(env.detached_event_for_planned(&wrong).is_err());
        assert!(env.local_frame_for_planned(&wrong).is_err());
        assert!(env.preview_planned_local_core_bounded(&wrong, 4).is_err());
    }
}

#[test]
fn isolated_map_event_sessions_retain_fixed_parameter_and_reject_stale_actions() {
    let environment = environment();
    let plan = environment
        .scripts
        .plan_map_lifecycle(MapLifecyclePhase::Enter, 999, 0)
        .unwrap()[0];
    let mut sessions = DetachedMapEventSessions::new();
    assert!(sessions.get(1).is_none());
    assert!(sessions.finish(1).is_err());
    assert!(sessions.set_objs(&environment, 999, None, None).is_err());
    assert!(sessions.get(999).is_none());
    assert!(sessions
        .set_objs(
            &environment,
            1,
            Some(rust_age::collision::ObjectKey {
                elevation: 0,
                ordinal: 99,
            }),
            None,
        )
        .is_err());
    assert!(sessions.get(1).is_none()); // rejected participant is atomic

    sessions.set_objs(&environment, 1, None, None).unwrap();
    let started = sessions.begin_planned(&environment, &plan).unwrap();
    assert_eq!(started.invocation().action(), 15);
    assert_eq!(started.invocation().fixed_parameter(), 1);
    assert_eq!(started.source_handle(), 0);
    assert_eq!(started.target_handle(), 0);
    assert!(sessions.begin_planned(&environment, &plan).is_err());
    sessions.finish(1).unwrap();
    assert!(!sessions.get(1).unwrap().is_active());

    // A later action with no fixed-param assignment preserves previous state.
    let again = rust_age::script_lifecycle::PlannedMapAction {
        external_parameter: None,
        ..plan
    };
    let state = sessions.begin_planned(&environment, &again).unwrap();
    assert_eq!(state.invocation().fixed_parameter(), 1);
    assert_eq!(state.source_handle(), 0);
    assert_eq!(state.target_handle(), 0);
    sessions.finish(1).unwrap();

    let stale = rust_age::script_lifecycle::PlannedMapAction {
        script_id: 42,
        ..plan
    };
    assert!(sessions.begin_planned(&environment, &stale).is_err());
    assert!(!sessions.get(1).unwrap().is_active());
    assert_eq!(sessions.get(1).unwrap().invocation().fixed_parameter(), 1);
    assert!(sessions.get(999).is_none());
    assert_eq!(environment.scripts.runtime.vm(1).unwrap().pc(), 0);
}

#[test]
fn repeated_planned_actions_keep_inherited_top_level_target_but_clear_source() {
    let mut environment = environment();
    let mut vm = ScriptRuntime::new();
    // POP argument marker; source_obj; tile_num; value-returning CE exit.
    vm.register(
        1,
        synthetic_vm_with_body(&[0x80, 0x1A, 0x80, 0xBD, 0x80, 0xD4, 0x80, 0x25]),
    )
    .unwrap();
    environment.scripts.runtime = vm;
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
            vec![object(77, 17, 20099), object(77, -1, 20100)],
            vec![],
            vec![],
        ],
        total_objects: 2,
        bytes_consumed: 0,
    };
    let scripts = ScriptTables {
        categories: [
            vec![ScriptRecord {
                script_id: 17,
                next: -1,
                subtype: vec![],
                flags: 0,
                script_index: 0,
                program_token: -1,
                owner_id: 77,
                local_variable_offset: -1,
                local_variable_count: 0,
                remaining: [0; 8],
            }],
            vec![],
            vec![],
            vec![],
            vec![],
        ],
        bytes_consumed: 0,
    };
    let queries = MapObjectReadBindings::from_tables(&objects).unwrap();
    environment.script_owners = MapScriptOwners::from_map(&scripts, &objects, &queries).unwrap();
    environment.object_queries = queries;
    let plan = environment
        .scripts
        .plan_map_lifecycle(MapLifecyclePhase::Enter, 999, 0)
        .unwrap()[0];
    let mut sessions = DetachedMapEventSessions::new();
    sessions
        .set_objs(
            &environment,
            1,
            Some(ObjectKey {
                elevation: 0,
                ordinal: 1,
            }),
            None,
        )
        .unwrap();
    let started = sessions.begin_planned(&environment, &plan).unwrap();
    assert_eq!(started.source_handle(), 2);
    assert_eq!(started.target_handle(), 1);
    let clock = ScriptClockContext {
        clock: ScriptGameClock { ticks: 100 },
        last_visit_ticks: None,
    };
    assert_eq!(
        environment
            .preview_active_event_reads_bounded(&sessions, &plan, clock, 5)
            .unwrap(),
        Some(ScriptValue::Int(20100))
    );
    assert_eq!(sessions.get(1).unwrap().source_handle(), 2);
    assert!(environment
        .preview_active_event_reads_bounded(&DetachedMapEventSessions::new(), &plan, clock, 5)
        .is_err());
    sessions.finish(1).unwrap();
    assert!(environment
        .preview_active_event_reads_bounded(&sessions, &plan, clock, 5)
        .is_err());
    let resumed = sessions
        .begin_planned(
            &environment,
            &rust_age::script_lifecycle::PlannedMapAction {
                external_parameter: None,
                ..plan
            },
        )
        .unwrap();
    assert_eq!(resumed.source_handle(), 0);
    assert_eq!(resumed.target_handle(), 1);
    assert_eq!(resumed.invocation().fixed_parameter(), 1);
    assert_eq!(
        environment
            .preview_active_event_reads_bounded(
                &sessions,
                &rust_age::script_lifecycle::PlannedMapAction {
                    external_parameter: None,
                    ..plan
                },
                clock,
                5,
            )
            .unwrap(),
        Some(ScriptValue::Int(-1))
    );
    let stale = rust_age::script_lifecycle::PlannedMapAction {
        script_id: 99,
        ..plan
    };
    assert!(environment
        .preview_active_event_reads_bounded(&sessions, &stale, clock, 5)
        .is_err());
    assert_eq!(sessions.get(1).unwrap().target_handle(), 1);
    assert_eq!(environment.scripts.runtime.vm(1).unwrap().pc(), 0);
}

#[test]
fn planned_script_readonly_native_preview_can_read_null_owner_but_not_mutate() {
    let mut environment = environment();
    let mut runtime = ScriptRuntime::new();
    // POP arg marker; self_obj -> NULL; tile_num(NULL) -> -1; return value.
    runtime
        .register(
            1,
            synthetic_vm_with_body(&[0x80, 0x1A, 0x80, 0xBC, 0x80, 0xD4, 0x80, 0x25]),
        )
        .unwrap();
    environment.scripts.runtime = runtime;
    let action = environment
        .scripts
        .plan_map_lifecycle(MapLifecyclePhase::Enter, 999, 0)
        .unwrap()[0];
    let clock = ScriptClockContext {
        clock: ScriptGameClock { ticks: 100 },
        last_visit_ticks: None,
    };
    assert_eq!(
        environment
            .preview_planned_event_reads_bounded(&action, clock, 5)
            .unwrap(),
        Some(ScriptValue::Int(-1))
    );
    // Without the explicit pure native adapter, all game-native reads fail.
    assert!(environment
        .preview_planned_local_core_bounded(&action, 5)
        .is_err());
    assert_eq!(environment.scripts.runtime.vm(1).unwrap().pc(), 0);
    assert!(environment
        .scripts
        .runtime
        .vm(1)
        .unwrap()
        .stack()
        .is_empty());

    let mut denying = ScriptRuntime::new();
    // POP; script_overrides (a write); POP_FLAGS_EXIT
    denying
        .register(
            1,
            synthetic_vm_with_body(&[0x80, 0x1A, 0x80, 0xB9, 0x80, 0x21]),
        )
        .unwrap();
    environment.scripts.runtime = denying;
    assert!(environment
        .preview_planned_event_reads_bounded(&action, clock, 5)
        .is_err());
    assert_eq!(environment.scripts.runtime.vm(1).unwrap().pc(), 0);
}
