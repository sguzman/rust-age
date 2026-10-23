//! Synthetic CE MAP lifecycle plan -> installed VM -> inert event metadata.
//! These are invented script and object records; no game procedure executes.
use rust_age::map::MapVariables;
use rust_age::object_tables::ObjectTables;
use rust_age::scene::{DetachedMapEventSessions, PreparedScriptEnvironment};
use rust_age::script_binding::{BoundScriptRuntime, InstalledScriptBinding, ScriptBinding};
use rust_age::script_data::{ScriptRecord, ScriptTables};
use rust_age::script_lifecycle::MapLifecyclePhase;
use rust_age::script_object_binding::MapObjectReadBindings;
use rust_age::script_owner_binding::MapScriptOwners;
use rust_age::script_program::ScriptProgram;
use rust_age::script_runtime::ScriptRuntime;
use rust_age::script_vm::ScriptVm;

fn synthetic_vm() -> ScriptVm {
    let names: [&[u8]; 2] = [b"main", b"map_enter_p_proc"];
    let mut bytes = vec![0_u8; 42];
    bytes.extend_from_slice(&(names.len() as i32).to_be_bytes());
    let mut strings = Vec::new();
    for name in names {
        let offset = 4 + strings.len() as i32;
        for value in [offset, 0, 0, 0, 0, 0] {
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
    assert_eq!(environment.scripts.runtime.vm(1).unwrap().pc(), 0);
}
