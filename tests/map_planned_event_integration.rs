//! Synthetic CE MAP lifecycle plan -> installed VM -> inert event metadata.
//! These are invented script and object records; no game procedure executes.
use rust_age::collision::ObjectKey;
use rust_age::game_globals::GameGlobalState;
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
    synthetic_vm_with_condition(body, &[])
}

fn synthetic_vm_with_condition(body: &[u8], condition: &[u8]) -> ScriptVm {
    let flags = if condition.is_empty() { 0 } else { 0x02 };
    synthetic_vm_with_scheduled_condition(body, condition, flags, 0)
}

fn synthetic_vm_with_scheduled_condition(
    body: &[u8],
    condition: &[u8],
    first_flags: i32,
    first_event_time: i32,
) -> ScriptVm {
    assert!(body.len() <= 22);
    assert!(condition.len() <= 20);
    let names: [&[u8]; 2] = [b"main", b"map_enter_p_proc"];
    let mut bytes = vec![0_u8; 42];
    bytes[..condition.len()].copy_from_slice(condition);
    bytes[20..20 + body.len()].copy_from_slice(body);
    bytes.extend_from_slice(&(names.len() as i32).to_be_bytes());
    let mut strings = Vec::new();
    for (index, name) in names.into_iter().enumerate() {
        let offset = 4 + strings.len() as i32;
        let (flags, event_time) = if index == 0 {
            (first_flags, first_event_time)
        } else {
            (0, 0)
        };
        for value in [offset, flags, event_time, 0, 20, 0] {
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

fn synthetic_vm_with_followup_condition(expected_fixed_parameter: i32) -> ScriptVm {
    // Procedure zero schedules procedure one while doEvents walks the same
    // installed VM. The second procedure reads the verified fixed_param
    // event context, rather than a fabricated or mutable host callback.
    let mut first = Vec::new();
    first.extend_from_slice(&0xC001_u16.to_be_bytes());
    first.extend_from_slice(&28_i32.to_be_bytes());
    first.extend_from_slice(&0xC001_u16.to_be_bytes());
    first.extend_from_slice(&1_i32.to_be_bytes());
    first.extend_from_slice(&[0x80, 0x07]); // CALL_WHEN
    first.extend_from_slice(&0xC001_u16.to_be_bytes());
    first.extend_from_slice(&0_i32.to_be_bytes());
    first.extend_from_slice(&[0x80, 0x11]); // false condition
    assert_eq!(first.len(), 22);
    let mut second = vec![0x80, 0xF7]; // CE fixed_param native read
    second.extend_from_slice(&0xC001_u16.to_be_bytes());
    second.extend_from_slice(&expected_fixed_parameter.to_be_bytes());
    // CE EQUAL; STOP_PROGRAM.
    second.extend_from_slice(&[0x80, 0x33, 0x80, 0x11]);
    // CE stores the procedure table immediately after the 42-byte
    // bytecode header. Do not insert an invented 22-byte padding section.
    let mut bytes = vec![0_u8; 42];
    bytes[..first.len()].copy_from_slice(&first);
    bytes[28..28 + second.len()].copy_from_slice(&second);
    bytes.extend_from_slice(&2_i32.to_be_bytes());
    let mut names = Vec::new();
    for (name, flags, condition_offset) in [
        (b"main".as_slice(), 0x02_i32, 0_i32),
        (b"map_enter_p_proc".as_slice(), 0_i32, 0_i32),
    ] {
        let identifier_offset = 4 + names.len() as i32;
        // This preview never dispatches the body. Point its synthetic
        // entry to a valid opcode without overlapping the CE table.
        for field in [identifier_offset, flags, 0, condition_offset, 28, 0] {
            bytes.extend_from_slice(&field.to_be_bytes());
        }
        names.extend_from_slice(name);
        names.push(0);
    }
    bytes.extend_from_slice(&(names.len() as i32).to_be_bytes());
    bytes.extend_from_slice(&names);
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
        invocations: std::collections::BTreeMap::new(),
    }
}

fn synthetic_map_enter_with_flags(flags: i32) -> ScriptVm {
    let mut bytes = vec![0_u8; 42];
    bytes[20..24].copy_from_slice(&[0x80, 0x1A, 0x80, 0x21]);
    bytes.extend_from_slice(&2_i32.to_be_bytes());
    let mut strings = Vec::new();
    for (name, flag, body) in [
        (b"main".as_slice(), 0_i32, 0_i32),
        (b"map_enter_p_proc".as_slice(), flags, 20_i32),
    ] {
        let offset = 4 + strings.len() as i32;
        for field in [offset, flag, 0, 0, body, 0] {
            bytes.extend_from_slice(&field.to_be_bytes());
        }
        strings.extend_from_slice(name);
        strings.push(0);
    }
    bytes.extend_from_slice(&(strings.len() as i32).to_be_bytes());
    bytes.extend_from_slice(&strings);
    bytes.extend_from_slice(&0_i32.to_be_bytes());
    ScriptVm::new(ScriptProgram::decode(&bytes).unwrap())
}

fn two_enter_scripts(second_flags: i32) -> PreparedScriptEnvironment {
    let mut env = environment();
    env.scripts
        .runtime
        .register(2, synthetic_map_enter_with_flags(second_flags))
        .unwrap();
    let second = InstalledScriptBinding {
        binding: ScriptBinding {
            category: 0,
            ordinal: 1,
            script_id: 18,
            script_index: 1,
            owner_id: -1,
            path: Some("scripts/SECOND.int".to_owned()),
            run_info: None,
            local_variable_offset: -1,
            local_variable_count: 0,
        },
        instance_id: Some(2),
    };
    env.scripts.entries.push(second);
    let records = [17, 18]
        .into_iter()
        .map(|script_id| ScriptRecord {
            script_id,
            next: -1,
            subtype: vec![],
            flags: 0,
            script_index: 0,
            program_token: -1,
            owner_id: -1,
            local_variable_offset: -1,
            local_variable_count: 0,
            remaining: [0; 8],
        })
        .collect();
    let tables = ScriptTables {
        categories: [records, vec![], vec![], vec![], vec![]],
        bytes_consumed: 0,
    };
    let empty_objects = ObjectTables {
        elevations: [vec![], vec![], vec![]],
        total_objects: 0,
        bytes_consumed: 0,
    };
    env.script_owners =
        MapScriptOwners::from_map(&tables, &empty_objects, &env.object_queries).unwrap();
    env.map_locals = env
        .scripts
        .script_local_state(&MapVariables {
            globals: vec![],
            locals: vec![],
        })
        .unwrap();
    env
}

#[test]
fn entire_map_enter_phase_stages_source_order_and_fixed_param_atomically() {
    let mut env = two_enter_scripts(0);
    let staged = env
        .prepare_noncritical_map_lifecycle_transactional(MapLifecyclePhase::Enter, 999, 0)
        .unwrap();
    assert_eq!(
        staged
            .iter()
            .map(|frame| frame.program_id)
            .collect::<Vec<_>>(),
        [1, 2]
    );
    for id in [1, 2] {
        let vm = env.scripts.runtime.vm(id).unwrap();
        assert_eq!(vm.pc(), 20);
        assert_eq!(
            vm.return_stack(),
            [ScriptValue::Int(0), ScriptValue::Int(20)]
        );
        assert_eq!(env.invocations.get(&id).unwrap().action(), 15);
        assert_eq!(env.invocations.get(&id).unwrap().fixed_parameter(), 1);
    }
    assert!(env.map_locals.globals().is_empty());
    assert!(env.invocations.len() == 2);
}

#[test]
fn failed_later_critical_lifecycle_action_rolls_back_all_prepared_scripts() {
    let mut env = two_enter_scripts(0x10);
    assert!(env
        .prepare_noncritical_map_lifecycle_transactional(MapLifecyclePhase::Enter, 999, 1)
        .is_err());
    assert!(env.invocations.is_empty());
    for id in [1, 2] {
        let vm = env.scripts.runtime.vm(id).unwrap();
        assert_eq!(vm.pc(), 0);
        assert!(vm.return_stack().is_empty());
        assert!(vm.stack().is_empty());
    }
    // Duplicate aliases must not stage nested calls or persist partial
    // action metadata even when both aliases refer to valid procedures.
    let mut duplicate = two_enter_scripts(0);
    let mut alias = duplicate.scripts.entries[1].clone();
    alias.binding.ordinal = 2;
    alias.binding.script_id = 19;
    duplicate.scripts.entries.push(alias);
    assert!(duplicate
        .prepare_noncritical_map_lifecycle_transactional(MapLifecyclePhase::Enter, 999, 0)
        .is_err());
    assert!(duplicate.invocations.is_empty());
    assert_eq!(duplicate.scripts.runtime.vm(1).unwrap().pc(), 0);
    assert_eq!(duplicate.scripts.runtime.vm(2).unwrap().pc(), 0);
}

fn synthetic_enter_update_vm() -> ScriptVm {
    let mut bytes = vec![0_u8; 42];
    bytes[..2].copy_from_slice(&0x8000_u16.to_be_bytes());
    bytes[20..24].copy_from_slice(&[0x80, 0x1A, 0x80, 0x21]);
    bytes[24..26].copy_from_slice(&0x801A_u16.to_be_bytes());
    bytes[26..28].copy_from_slice(&0xC001_u16.to_be_bytes());
    bytes[28..32].copy_from_slice(&0_i32.to_be_bytes());
    bytes[32..34].copy_from_slice(&0x80F7_u16.to_be_bytes()); // FIXED_PARAM
    bytes[34..36].copy_from_slice(&0x80C6_u16.to_be_bytes()); // SET_GLOBAL_VAR
    bytes[36..38].copy_from_slice(&0x8021_u16.to_be_bytes()); // POP_FLAGS_EXIT
    bytes.extend_from_slice(&3_i32.to_be_bytes());
    let mut strings = Vec::new();
    for (name, body) in [
        (b"main".as_slice(), 0_i32),
        (b"map_enter_p_proc".as_slice(), 20_i32),
        (b"map_update_p_proc".as_slice(), 24_i32),
    ] {
        let offset = 4 + strings.len() as i32;
        for field in [offset, 0, 0, 0, body, 0] {
            bytes.extend_from_slice(&field.to_be_bytes());
        }
        strings.extend_from_slice(name);
        strings.push(0);
    }
    bytes.extend_from_slice(&(strings.len() as i32).to_be_bytes());
    bytes.extend_from_slice(&strings);
    bytes.extend_from_slice(&0_i32.to_be_bytes());
    ScriptVm::new(ScriptProgram::decode(&bytes).unwrap())
}

#[test]
fn map_update_retains_prior_enter_fixed_parameter_during_native_execution() {
    let mut env = environment();
    *env.scripts.runtime.vm_mut(1).unwrap() = synthetic_enter_update_vm();
    env.game_globals = Some(GameGlobalState::from_initial_values(&[0]).unwrap());
    let enter = env
        .prepare_noncritical_map_lifecycle_transactional(MapLifecyclePhase::Enter, 999, 0)
        .unwrap();
    assert_eq!(enter.len(), 1);
    assert_eq!(env.invocations.get(&1).unwrap().fixed_parameter(), 1);
    let clock = ScriptClockContext {
        clock: ScriptGameClock { ticks: 0 },
        last_visit_ticks: Some(0),
    };
    let first = env.advance_script_state_tick_bounded(clock, 100, 2, 0).unwrap();
    assert_eq!(first.executed_steps, [(1, 2)]);
    assert_eq!(env.game_globals.as_ref().unwrap().values(), [0]);

    // CE does not clear fixedParam when scr_exec_map_update_scripts
    // selects a subsequent built-in procedure. Its native opcode must
    // observe the previous MAP_ENTER flag-derived value.
    let update = env
        .prepare_noncritical_map_lifecycle_transactional(MapLifecyclePhase::Update, 999, 0)
        .unwrap();
    assert_eq!(update.len(), 1);
    assert_eq!(update[0].procedure_index, 2);
    assert_eq!(env.invocations.get(&1).unwrap().action(), 23);
    assert_eq!(env.invocations.get(&1).unwrap().fixed_parameter(), 1);
    let tick = env.advance_script_state_tick_bounded(clock, 101, 5, 0).unwrap();
    assert_eq!(tick.executed_steps, [(1, 5)]);
    assert_eq!(env.game_globals.as_ref().unwrap().values(), [1]);
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
    let candidate = env
        .preview_planned_local_frame_core_bounded(&action, 4)
        .unwrap();
    assert_eq!(candidate.binding, frame);
    assert_eq!(candidate.candidate.returned, None);
    assert!(candidate.candidate.data_stack.is_empty());
    assert_eq!(candidate.candidate.return_stack, [ScriptValue::Int(0)]);
    assert_eq!(candidate.candidate.program_counter, 20);
    assert_eq!(candidate.candidate.saved_low_flags, 0);
    assert_eq!(candidate.candidate.restored_low_flags, 0);
    assert!(env
        .preview_planned_local_frame_core_bounded(&action, 1)
        .is_err());
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
    let sandbox = environment
        .preview_active_sandbox_bounded(&sessions, &plan, clock, 5)
        .unwrap();
    assert_eq!(sandbox.returned, Some(ScriptValue::Int(20100)));
    assert_eq!(sandbox.discard().unwrap().map.map_globals_changed, 0);
    assert_eq!(sessions.get(1).unwrap().source_handle(), 2);
    assert_eq!(sessions.get(1).unwrap().target_handle(), 1);
    assert!(environment
        .preview_active_event_reads_bounded(&DetachedMapEventSessions::new(), &plan, clock, 5)
        .is_err());
    sessions.finish(1).unwrap();
    assert!(environment
        .preview_active_event_reads_bounded(&sessions, &plan, clock, 5)
        .is_err());
    assert!(environment
        .preview_active_sandbox_bounded(&sessions, &plan, clock, 5)
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
fn planned_map_sandbox_reports_cloned_call_at_schedule_alongside_world_effects() {
    let mut environment = environment();
    // Consume the CE setupCall argument marker before CALL_AT.
    // CALL_AT takes delay in seconds, then the procedure index.
    let mut body = vec![0x80, 0x1A];
    body.extend_from_slice(&0xC001_u16.to_be_bytes());
    body.extend_from_slice(&1_i32.to_be_bytes());
    body.extend_from_slice(&0xC001_u16.to_be_bytes());
    body.extend_from_slice(&1_i32.to_be_bytes());
    // Schedule the MAP procedure and set a speculative script override;
    // neither writes the installed MAP or the active script session.
    body.extend_from_slice(&[0x80, 0x06, 0x80, 0xB9, 0x80, 0x21]);
    assert_eq!(body.len(), 20);
    environment.scripts.runtime = {
        let mut runtime = ScriptRuntime::new();
        runtime.register(1, synthetic_vm_with_body(&body)).unwrap();
        runtime
    };
    let plan = environment
        .scripts
        .plan_map_lifecycle(MapLifecyclePhase::Enter, 999, 0)
        .unwrap()[0];
    let clock = ScriptClockContext {
        clock: ScriptGameClock { ticks: 200 },
        last_visit_ticks: None,
    };
    let sandbox = environment
        .preview_planned_sandbox_bounded(&plan, clock, 6)
        .unwrap();
    assert_eq!(sandbox.vm_frame.binding.procedure_index, 1);
    assert_eq!(sandbox.vm_frame.candidate.program_counter, 20);
    assert_eq!(sandbox.vm_frame.candidate.returned, None);
    assert_eq!(
        sandbox.vm_frame.candidate.return_stack,
        [ScriptValue::Int(0)]
    );
    assert_eq!(sandbox.vm_frame.candidate.procedures[1].flags & 0x01, 0x01);
    assert_eq!(sandbox.vm_frame.candidate.procedures[1].event_time, 1000);
    let effects = sandbox.effects().unwrap();
    assert_eq!(effects.procedure_entries_changed, 1);
    assert_eq!(effects.quest_globals_changed, 0);
    assert_eq!(effects.map.map_globals_changed, 0);
    assert_eq!(effects.map.local_slots_changed, 0);
    assert!(effects.overrides);
    assert!(sandbox.overrides);
    assert_eq!(sandbox.discard().unwrap(), effects);
    assert!(!environment
        .detached_event_for_planned(&plan)
        .unwrap()
        .invocation()
        .overrides());
    let installed = environment.scripts.runtime.vm(1).unwrap();
    assert_eq!(installed.runtime_procedures()[1].flags, 0);
    assert_eq!(installed.runtime_procedures()[1].event_time, 0);
    assert!(installed.stack().is_empty());
    assert!(installed.return_stack().is_empty());
    assert_eq!(installed.pc(), 0);
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

#[test]
fn cloned_map_and_game_global_reads_use_immutable_values_and_deny_writes() {
    let mut environment = environment();
    environment.map_locals = environment
        .scripts
        .script_local_state(&MapVariables {
            globals: vec![14],
            locals: vec![],
        })
        .unwrap();
    environment.game_globals = Some(GameGlobalState::from_initial_values(&[27]).unwrap());
    let mut runtime = ScriptRuntime::new();
    // POP argument marker; PUSH index zero; map_var; PUSH index zero;
    // global_var; ADD; POP_FLAGS_RETURN_VAL_EXIT.
    let mut body = vec![0x80, 0x1A, 0xC0, 0x01];
    body.extend_from_slice(&0_i32.to_be_bytes());
    body.extend_from_slice(&[0x80, 0xC3, 0xC0, 0x01]);
    body.extend_from_slice(&0_i32.to_be_bytes());
    body.extend_from_slice(&[0x80, 0xC5, 0x80, 0x39, 0x80, 0x25]);
    runtime.register(1, synthetic_vm_with_body(&body)).unwrap();
    environment.scripts.runtime = runtime;
    let plan = environment
        .scripts
        .plan_map_lifecycle(MapLifecyclePhase::Enter, 999, 0)
        .unwrap()[0];
    let clock = ScriptClockContext {
        clock: ScriptGameClock { ticks: 0 },
        last_visit_ticks: None,
    };
    assert_eq!(
        environment
            .preview_planned_event_reads_bounded(&plan, clock, 8)
            .unwrap(),
        Some(ScriptValue::Int(41))
    );
    assert_eq!(environment.map_locals.globals(), [14]);
    assert_eq!(environment.game_globals.as_ref().unwrap().values(), [27]);
    assert_eq!(environment.scripts.runtime.vm(1).unwrap().pc(), 0);

    environment.game_globals = None;
    assert!(environment
        .preview_planned_event_reads_bounded(&plan, clock, 8)
        .is_err());
    // The live script stays untouched even after the host refuses access.
    assert_eq!(environment.scripts.runtime.vm(1).unwrap().pc(), 0);

    // A MAP global write is not a read-only CE opcode.
    let mut write_runtime = ScriptRuntime::new();
    write_runtime
        .register(
            1,
            synthetic_vm_with_body(&[0x80, 0x1A, 0x80, 0xC4, 0x80, 0x21]),
        )
        .unwrap();
    environment.scripts.runtime = write_runtime;
    assert!(environment
        .preview_planned_event_reads_bounded(&plan, clock, 5)
        .is_err());
    assert_eq!(environment.map_locals.globals(), [14]);
}

#[test]
fn planned_conditional_read_uses_ce_procedure_offset_without_dispatch() {
    let mut environment = environment();
    environment.map_locals = environment
        .scripts
        .script_local_state(&MapVariables {
            globals: vec![12],
            locals: vec![],
        })
        .unwrap();

    // Synthetic CE condition at program offset zero: MAP_GLOBAL[0] == 12.
    let mut condition = Vec::new();
    condition.extend_from_slice(&0xC001_u16.to_be_bytes()); // PUSH index
    condition.extend_from_slice(&0_i32.to_be_bytes());
    condition.extend_from_slice(&0x80C3_u16.to_be_bytes()); // map_var
    condition.extend_from_slice(&0xC001_u16.to_be_bytes()); // PUSH expected
    condition.extend_from_slice(&12_i32.to_be_bytes());
    condition.extend_from_slice(&[0x80, 0x33, 0x80, 0x11]); // EQ; STOP
    let body = [0x80, 0x1A, 0x80, 0x21];
    *environment.scripts.runtime.vm_mut(1).unwrap() =
        synthetic_vm_with_condition(&body, &condition);
    let plan = environment
        .scripts
        .plan_map_lifecycle(MapLifecyclePhase::Enter, 999, 0)
        .unwrap()[0];
    let clock = ScriptClockContext {
        clock: ScriptGameClock { ticks: 100 },
        last_visit_ticks: None,
    };
    assert!(environment
        .preview_planned_condition_reads_bounded(&plan, 0, clock, 5)
        .unwrap());
    let frame = environment
        .preview_planned_condition_frame_reads_bounded(&plan, 0, clock, 5)
        .unwrap();
    assert!(frame.satisfied);
    assert!(frame.data_stack.is_empty());
    assert!(frame.return_stack.is_empty());
    assert_eq!(
        environment
            .preview_planned_due_reads_bounded(&plan, clock, 100, 5)
            .unwrap(),
        vec![0]
    );
    assert!(environment
        .preview_planned_condition_reads_bounded(&plan, 0, clock, 4)
        .is_err());
    for index in [1, 10] {
        assert!(environment
            .preview_planned_condition_reads_bounded(&plan, index, clock, 5)
            .is_err());
    }
    assert_eq!(
        environment
            .scripts
            .runtime
            .vm(1)
            .unwrap()
            .runtime_procedures()[0]
            .flags,
        0x02
    );
    assert_eq!(environment.scripts.runtime.vm(1).unwrap().pc(), 0);
    assert_eq!(environment.map_locals.globals(), [12]);

    // Read new immutable MAP data on another preview; neither poll
    // consumes the scheduled event nor changes the installed VM.
    environment.map_locals = environment
        .scripts
        .script_local_state(&MapVariables {
            globals: vec![13],
            locals: vec![],
        })
        .unwrap();
    assert!(!environment
        .preview_planned_condition_reads_bounded(&plan, 0, clock, 5)
        .unwrap());
    assert!(environment
        .preview_planned_due_reads_bounded(&plan, clock, 100, 5)
        .unwrap()
        .is_empty());

    // Script overrides remain a write and must be refused by this adapter.
    *environment.scripts.runtime.vm_mut(1).unwrap() =
        synthetic_vm_with_condition(&body, &[0x80, 0xB9, 0x80, 0x11]);
    assert!(environment
        .preview_planned_condition_reads_bounded(&plan, 0, clock, 2)
        .is_err());
    assert!(environment
        .preview_planned_due_reads_bounded(&plan, clock, 100, 5)
        .is_err());
    assert_eq!(environment.map_locals.globals(), [13]);
    assert_eq!(
        environment
            .scripts
            .runtime
            .vm(1)
            .unwrap()
            .runtime_procedures()[0]
            .flags,
        0x02
    );
    let stale = rust_age::script_lifecycle::PlannedMapAction {
        instance_id: 999,
        ..plan
    };
    assert!(environment
        .preview_planned_condition_reads_bounded(&stale, 0, clock, 5)
        .is_err());
}

#[test]
fn verified_map_condition_frame_reports_detached_scheduling_side_effects() {
    let mut environment = environment();
    let body = [0x80, 0x1A, 0x80, 0x21];
    let mut condition = Vec::new();
    // CALL_AT procedure index 1 in 7 seconds, then return fixed_param
    // from the verified MAP event (1). Only the cloned procedure table
    // may observe the new timed flag and future event timestamp.
    condition.extend_from_slice(&0xC001_u16.to_be_bytes());
    condition.extend_from_slice(&7_i32.to_be_bytes());
    condition.extend_from_slice(&0xC001_u16.to_be_bytes());
    condition.extend_from_slice(&1_i32.to_be_bytes());
    condition.extend_from_slice(&[0x80, 0x06, 0x80, 0xF7, 0x80, 0x11]);
    *environment.scripts.runtime.vm_mut(1).unwrap() =
        synthetic_vm_with_condition(&body, &condition);
    let plan = environment
        .scripts
        .plan_map_lifecycle(MapLifecyclePhase::Enter, 999, 0)
        .unwrap()[0];
    let clock = ScriptClockContext {
        clock: ScriptGameClock { ticks: 0 },
        last_visit_ticks: None,
    };
    let candidate = environment
        .preview_planned_condition_frame_reads_bounded(&plan, 0, clock, 5)
        .unwrap();
    assert!(candidate.satisfied);
    assert_eq!(candidate.flag_restore.saved_pc, 0);
    assert_eq!(candidate.flag_restore.evaluated_pc, 18);
    assert_eq!(candidate.flag_restore.restored_pc, 0);
    assert_eq!(candidate.flag_restore.saved_low_flags, 0);
    assert_eq!(candidate.flag_restore.evaluated_low_flags, 0x08);
    assert_eq!(candidate.flag_restore.restored_low_flags, 0);
    assert_eq!(candidate.procedures[1].flags & 0x01, 0x01);
    assert_eq!(candidate.procedures[1].event_time, 7000);
    let due = environment
        .preview_planned_due_frame_reads_bounded(&plan, clock, 0, 5)
        .unwrap();
    assert_eq!(due.ready, [0]);
    assert_eq!(due.procedures[0].flags, 0);
    assert_eq!(due.procedures[1].flags & 0x01, 0x01);
    assert_eq!(due.procedures[1].event_time, 7000);
    assert_eq!(due.condition_flag_restorations.len(), 1);
    assert_eq!(due.condition_flag_restorations[0].0, 0);
    assert_eq!(due.condition_flag_restorations[0].1.saved_low_flags, 0);
    assert_eq!(
        due.condition_flag_restorations[0].1.evaluated_low_flags,
        0x08
    );
    assert_eq!(due.condition_flag_restorations[0].1.restored_low_flags, 0);
    assert!(due.data_stack.is_empty());
    assert_eq!(
        environment
            .scripts
            .runtime
            .vm(1)
            .unwrap()
            .runtime_procedures()[1]
            .flags,
        0
    );
    assert_eq!(
        environment
            .scripts
            .runtime
            .vm(1)
            .unwrap()
            .runtime_procedures()[1]
            .event_time,
        0
    );
    assert_eq!(environment.scripts.runtime.vm(1).unwrap().pc(), 0);
}

#[test]
fn active_map_condition_can_arm_and_read_later_procedure_without_committing() {
    let mut environment = environment();
    let clock = ScriptClockContext {
        clock: ScriptGameClock { ticks: 0 },
        last_visit_ticks: None,
    };
    for (expected, ready, retained_flags) in [(1, vec![1_usize], 0_i32), (2, vec![], 0x02_i32)] {
        *environment.scripts.runtime.vm_mut(1).unwrap() =
            synthetic_vm_with_followup_condition(expected);
        let plan = environment
            .scripts
            .plan_map_lifecycle(MapLifecyclePhase::Enter, 999, 0)
            .unwrap()[0];
        let mut sessions = DetachedMapEventSessions::new();
        sessions.begin_planned(&environment, &plan).unwrap();
        let candidate = environment
            .preview_active_due_frame_reads_bounded(&sessions, &plan, clock, 0, 5)
            .unwrap();
        assert_eq!(candidate.ready, ready);
        assert_eq!(candidate.procedures[0].flags, 0x02);
        assert_eq!(candidate.procedures[1].flags, retained_flags);
        assert_eq!(candidate.procedures[1].condition_offset, 28);
        assert_eq!(candidate.condition_terminations.len(), 2);
        assert!(candidate.data_stack.is_empty());
        assert!(sessions.get(1).unwrap().is_active());
        let installed = environment.scripts.runtime.vm(1).unwrap();
        assert_eq!(installed.runtime_procedures()[0].flags, 0x02);
        assert_eq!(installed.runtime_procedures()[1].flags, 0);
        assert_eq!(installed.runtime_procedures()[1].condition_offset, 0);
        assert!(installed.stack().is_empty());
        assert_eq!(installed.pc(), 0);
    }
}

#[test]
fn read_only_due_preview_obeys_ce_condition_priority_and_strict_due_time() {
    let mut environment = environment();
    let body = [0x80, 0x1A, 0x80, 0x21];
    let condition_false = [0xC0, 0x01, 0, 0, 0, 0, 0x80, 0x11];
    *environment.scripts.runtime.vm_mut(1).unwrap() =
        synthetic_vm_with_scheduled_condition(&body, &condition_false, 0x03, 10);
    let plan = environment
        .scripts
        .plan_map_lifecycle(MapLifecyclePhase::Enter, 999, 0)
        .unwrap()[0];
    let clock = ScriptClockContext {
        clock: ScriptGameClock { ticks: 100 },
        last_visit_ticks: None,
    };
    // Although the time has elapsed, CE prioritizes the conditional flag.
    assert!(environment
        .preview_planned_due_reads_bounded(&plan, clock, 11, 2)
        .unwrap()
        .is_empty());
    assert_eq!(
        environment
            .scripts
            .runtime
            .vm(1)
            .unwrap()
            .runtime_procedures()[0]
            .flags,
        0x03
    );

    *environment.scripts.runtime.vm_mut(1).unwrap() =
        synthetic_vm_with_scheduled_condition(&body, &condition_false, 0x01, 10);
    assert!(environment
        .preview_planned_due_reads_bounded(&plan, clock, 10, 2)
        .unwrap()
        .is_empty());
    assert_eq!(
        environment
            .preview_planned_due_reads_bounded(&plan, clock, 11, 2)
            .unwrap(),
        vec![0]
    );
    assert_eq!(
        environment
            .scripts
            .runtime
            .vm(1)
            .unwrap()
            .runtime_procedures()[0]
            .flags,
        0x01
    );

    let condition_true = [0xC0, 0x01, 0, 0, 0, 1, 0x80, 0x11];
    *environment.scripts.runtime.vm_mut(1).unwrap() =
        synthetic_vm_with_scheduled_condition(&body, &condition_true, 0x03, 10);
    assert_eq!(
        environment
            .preview_planned_due_reads_bounded(&plan, clock, 10, 2)
            .unwrap(),
        vec![0]
    );
    assert_eq!(
        environment
            .scripts
            .runtime
            .vm(1)
            .unwrap()
            .runtime_procedures()[0]
            .flags,
        0x03
    );
}

#[test]
fn active_due_condition_uses_retained_script_fixed_parameter_and_fails_closed() {
    let mut environment = environment();
    // CE fixed_param read; PUSH 1; EQUAL; STOP_PROGRAM.
    let condition = [0x80, 0xF7, 0xC0, 0x01, 0, 0, 0, 1, 0x80, 0x33, 0x80, 0x11];
    let body = [0x80, 0x1A, 0x80, 0x21];
    *environment.scripts.runtime.vm_mut(1).unwrap() =
        synthetic_vm_with_condition(&body, &condition);
    let plan = environment
        .scripts
        .plan_map_lifecycle(MapLifecyclePhase::Enter, 999, 0)
        .unwrap()[0];
    let mut sessions = DetachedMapEventSessions::new();
    sessions.begin_planned(&environment, &plan).unwrap();
    let clock = ScriptClockContext {
        clock: ScriptGameClock { ticks: 0 },
        last_visit_ticks: None,
    };
    assert_eq!(
        environment
            .preview_active_due_reads_bounded(&sessions, &plan, clock, 0, 4)
            .unwrap(),
        vec![0]
    );
    let candidate = environment
        .preview_active_due_frame_reads_bounded(&sessions, &plan, clock, 0, 4)
        .unwrap();
    assert_eq!(candidate.ready, [0]);
    assert!(candidate.data_stack.is_empty());
    assert_eq!(candidate.procedures[0].flags, 0);
    assert_eq!(
        environment
            .scripts
            .runtime
            .vm(1)
            .unwrap()
            .runtime_procedures()[0]
            .flags,
        0x02
    );
    assert!(sessions.get(1).unwrap().is_active());
    assert_eq!(sessions.get(1).unwrap().invocation().fixed_parameter(), 1);
    assert_eq!(
        environment
            .scripts
            .runtime
            .vm(1)
            .unwrap()
            .runtime_procedures()[0]
            .flags,
        0x02
    );
    assert_eq!(environment.scripts.runtime.vm(1).unwrap().pc(), 0);

    let mismatch = rust_age::script_lifecycle::PlannedMapAction {
        external_parameter: Some(123),
        ..plan
    };
    assert!(environment
        .preview_active_due_reads_bounded(&sessions, &mismatch, clock, 0, 4)
        .is_err());
    assert!(sessions.get(1).unwrap().is_active());
    sessions.finish(1).unwrap();
    assert!(environment
        .preview_active_due_reads_bounded(&sessions, &plan, clock, 0, 4)
        .is_err());
}

#[test]
fn planned_mutating_script_state_is_cloned_and_never_committed_to_map_or_quests() {
    let mut environment = environment();
    environment.map_locals = environment
        .scripts
        .script_local_state(&MapVariables {
            globals: vec![12],
            locals: vec![],
        })
        .unwrap();
    environment.game_globals = Some(GameGlobalState::from_initial_values(&[3]).unwrap());

    // POP setupCall's argument marker; PUSH map index 0; PUSH value 19;
    // set_map_var; script_overrides; POP_FLAGS_EXIT.
    let mut code = vec![0x80, 0x1A, 0xC0, 0x01];
    code.extend_from_slice(&0_i32.to_be_bytes());
    code.extend_from_slice(&0xC001_u16.to_be_bytes());
    code.extend_from_slice(&19_i32.to_be_bytes());
    code.extend_from_slice(&[0x80, 0xC4, 0x80, 0xB9, 0x80, 0x21]);
    assert_eq!(code.len(), 20);
    let mut runtime = ScriptRuntime::new();
    runtime.register(1, synthetic_vm_with_body(&code)).unwrap();
    environment.scripts.runtime = runtime;
    let plan = environment
        .scripts
        .plan_map_lifecycle(MapLifecyclePhase::Enter, 999, 0)
        .unwrap()[0];
    let clock = ScriptClockContext {
        clock: ScriptGameClock { ticks: 0 },
        last_visit_ticks: None,
    };
    let sandbox = environment
        .preview_planned_sandbox_bounded(&plan, clock, 8)
        .unwrap();
    assert_eq!(sandbox.returned, None);
    assert!(sandbox.overrides);
    assert_eq!(sandbox.map_locals.globals(), [19]);
    assert_eq!(sandbox.game_globals.as_ref().unwrap().values(), [3]);
    let effects = sandbox.effects().unwrap();
    assert_eq!(effects.map.map_globals_changed, 1);
    assert_eq!(effects.map.local_slots_changed, 0);
    assert_eq!(effects.map.newly_allocated_slots, 0);
    assert_eq!(effects.quest_globals_changed, 0);
    assert!(effects.overrides);
    assert_eq!(sandbox.discard().unwrap(), effects);
    assert_eq!(environment.map_locals.globals(), [12]);
    assert_eq!(environment.game_globals.as_ref().unwrap().values(), [3]);
    assert_eq!(environment.scripts.runtime.vm(1).unwrap().pc(), 0);
    assert!(environment
        .scripts
        .runtime
        .vm(1)
        .unwrap()
        .stack()
        .is_empty());

    // The identical CE setup frame can also mutate a *cloned* quest flag.
    code[15] = 0xC6; // SET_GLOBAL_VAR (0x80C6) in place of MAP-var write
    let mut runtime = ScriptRuntime::new();
    runtime.register(1, synthetic_vm_with_body(&code)).unwrap();
    environment.scripts.runtime = runtime;
    let sandbox = environment
        .preview_planned_sandbox_bounded(&plan, clock, 8)
        .unwrap();
    assert_eq!(sandbox.map_locals.globals(), [12]);
    assert_eq!(sandbox.game_globals.as_ref().unwrap().values(), [19]);
    let effects = sandbox.discard().unwrap();
    assert_eq!(effects.map.map_globals_changed, 0);
    assert_eq!(effects.quest_globals_changed, 1);
    assert!(effects.overrides);
    assert_eq!(environment.game_globals.as_ref().unwrap().values(), [3]);

    // Missing originals are never fabricated by the opt-in sandbox.
    environment.game_globals = None;
    assert!(environment
        .preview_planned_sandbox_bounded(&plan, clock, 8)
        .is_err());
    assert_eq!(environment.map_locals.globals(), [12]);

    // A gameplay time-advance native remains forbidden even in this sandbox.
    let mut runtime = ScriptRuntime::new();
    runtime
        .register(
            1,
            synthetic_vm_with_body(&[0x80, 0x1A, 0x80, 0xFC, 0x80, 0x21]),
        )
        .unwrap();
    environment.scripts.runtime = runtime;
    assert!(environment
        .preview_planned_sandbox_bounded(&plan, clock, 8)
        .is_err());
    assert_eq!(environment.scripts.runtime.vm(1).unwrap().pc(), 0);
}

#[test]
fn failed_sandbox_after_speculative_writes_and_overrides_rolls_back_entire_action() {
    let mut environment = environment();
    environment.map_locals = environment
        .scripts
        .script_local_state(&MapVariables {
            globals: vec![12],
            locals: vec![],
        })
        .unwrap();
    environment.game_globals = Some(GameGlobalState::from_initial_values(&[3]).unwrap());
    // POP frame marker; write MAP global slot zero as 19; script_overrides;
    // unsupported gameplay clock advancement. All earlier writes remain
    // detached when the final CE opcode fails.
    let mut code = vec![0x80, 0x1A, 0xC0, 0x01];
    code.extend_from_slice(&0_i32.to_be_bytes());
    code.extend_from_slice(&0xC001_u16.to_be_bytes());
    code.extend_from_slice(&19_i32.to_be_bytes());
    code.extend_from_slice(&[0x80, 0xC4, 0x80, 0xB9, 0x80, 0xFC, 0x80, 0x21]);
    assert_eq!(code.len(), 22);
    let mut runtime = ScriptRuntime::new();
    runtime.register(1, synthetic_vm_with_body(&code)).unwrap();
    environment.scripts.runtime = runtime;
    let plan = environment
        .scripts
        .plan_map_lifecycle(MapLifecyclePhase::Enter, 999, 0)
        .unwrap()[0];
    let clock = ScriptClockContext {
        clock: ScriptGameClock { ticks: 100 },
        last_visit_ticks: None,
    };
    for budget in [4, 6, 12] {
        assert!(environment
            .preview_planned_sandbox_bounded(&plan, clock, budget)
            .is_err());
        assert_eq!(environment.map_locals.globals(), [12]);
        assert_eq!(environment.game_globals.as_ref().unwrap().values(), [3]);
        assert_eq!(environment.scripts.runtime.vm(1).unwrap().pc(), 0);
        assert!(environment
            .scripts
            .runtime
            .vm(1)
            .unwrap()
            .stack()
            .is_empty());
        let event = environment.detached_event_for_planned(&plan).unwrap();
        assert!(!event.invocation().overrides());
        assert!(event.is_active());
    }
}

#[test]
fn active_sandbox_overrides_and_variable_writes_never_mutate_real_session() {
    let mut environment = environment();
    environment.map_locals = environment
        .scripts
        .script_local_state(&MapVariables {
            globals: vec![12],
            locals: vec![],
        })
        .unwrap();
    environment.game_globals = Some(GameGlobalState::from_initial_values(&[3]).unwrap());
    let mut code = vec![0x80, 0x1A, 0xC0, 0x01];
    code.extend_from_slice(&0_i32.to_be_bytes());
    code.extend_from_slice(&0xC001_u16.to_be_bytes());
    code.extend_from_slice(&19_i32.to_be_bytes());
    code.extend_from_slice(&[0x80, 0xC4, 0x80, 0xB9, 0x80, 0x21]);
    let mut runtime = ScriptRuntime::new();
    runtime.register(1, synthetic_vm_with_body(&code)).unwrap();
    environment.scripts.runtime = runtime;
    let planned = environment
        .scripts
        .plan_map_lifecycle(MapLifecyclePhase::Enter, 999, 0)
        .unwrap()[0];
    let clock = ScriptClockContext {
        clock: ScriptGameClock { ticks: 0 },
        last_visit_ticks: None,
    };
    let mut sessions = DetachedMapEventSessions::new();
    sessions.begin_planned(&environment, &planned).unwrap();
    assert!(!sessions.get(1).unwrap().invocation().overrides());
    let result = environment
        .preview_active_sandbox_bounded(&sessions, &planned, clock, 8)
        .unwrap();
    assert_eq!(result.map_locals.globals(), [19]);
    let summary = result.discard().unwrap();
    assert_eq!(summary.map.map_globals_changed, 1);
    assert!(summary.overrides);
    assert_eq!(environment.map_locals.globals(), [12]);
    assert_eq!(environment.game_globals.as_ref().unwrap().values(), [3]);
    assert!(!sessions.get(1).unwrap().invocation().overrides());
    assert!(sessions.get(1).unwrap().is_active());
    assert_eq!(environment.scripts.runtime.vm(1).unwrap().pc(), 0);
    let mismatch = rust_age::script_lifecycle::PlannedMapAction {
        external_parameter: Some(123),
        ..planned
    };
    assert!(environment
        .preview_active_sandbox_bounded(&sessions, &mismatch, clock, 8)
        .is_err());
    assert_eq!(environment.map_locals.globals(), [12]);
    sessions.finish(1).unwrap();
    assert!(environment
        .preview_active_sandbox_bounded(&sessions, &planned, clock, 8)
        .is_err());
}
