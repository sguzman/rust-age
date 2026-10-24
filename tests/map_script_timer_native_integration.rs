//! Source-adapted installed MAP object -> CE native timer -> due timed_event
//! -> bounded quest-global mutation. No original commercial game resources.
use rust_age::collision::ObjectKey;
use rust_age::event_queue::{EventQueue, EventType};
use rust_age::game_globals::GameGlobalState;
use rust_age::map::MapVariables;
use rust_age::object_data::{ObjectHeader, ObjectRecord, ObjectUpdate};
use rust_age::object_tables::{MapObject, ObjectTables};
use rust_age::scene::PreparedScriptEnvironment;
use rust_age::script_binding::{BoundScriptRuntime, InstalledScriptBinding, ScriptBinding};
use rust_age::script_clock::ScriptGameClock;
use rust_age::script_data::{ScriptRecord, ScriptTables};
use rust_age::script_native_vars::ScriptClockContext;
use rust_age::script_object_binding::MapObjectReadBindings;
use rust_age::script_owner_binding::MapScriptOwners;
use rust_age::script_program::ScriptProgram;
use rust_age::script_runtime::ScriptRuntime;
use rust_age::script_vm::{ScriptHaltReason, ScriptVm};
use std::collections::BTreeMap;

const SID: i32 = 0x0300_0001;
const OWNER: ObjectKey = ObjectKey {
    elevation: 0,
    ordinal: 0,
};

fn procedure(main: &[u8]) -> ScriptVm {
    assert!(main.len() <= 24);
    let mut bytes = vec![0_u8; 42];
    bytes[..main.len()].copy_from_slice(main);
    // CE executeProc uses literal return address 20 in the saved frame.
    // Keep that address an actual opcode in invented short main scripts,
    // even when STOP/EXIT at byte 16 prevents normal execution reaching it.
    if main.len() <= 20 {
        bytes[20..22].copy_from_slice(&0x8000_u16.to_be_bytes());
    }
    // timed_event_p_proc: drop saved argument; game_global[0] = fixed_param.
    bytes[24..26].copy_from_slice(&0x801A_u16.to_be_bytes());
    bytes[26..28].copy_from_slice(&0xC001_u16.to_be_bytes());
    bytes[28..32].copy_from_slice(&0_i32.to_be_bytes());
    bytes[32..34].copy_from_slice(&0x80F7_u16.to_be_bytes());
    bytes[34..36].copy_from_slice(&0x80C6_u16.to_be_bytes());
    bytes[36..38].copy_from_slice(&0x8021_u16.to_be_bytes());
    bytes.extend_from_slice(&2_i32.to_be_bytes());
    let mut names = Vec::new();
    for (name, body) in [
        (b"main".as_slice(), 0_i32),
        (b"timed_event_p_proc".as_slice(), 24_i32),
    ] {
        let offset = 4 + names.len() as i32;
        for field in [offset, 0, 0, 0, body, 0] {
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

fn main_add(handle: Option<i32>, terminal: u16) -> ScriptVm {
    let mut code = Vec::new();
    if let Some(handle) = handle {
        code.extend_from_slice(&0xE001_u16.to_be_bytes());
        code.extend_from_slice(&handle.to_be_bytes());
    } else {
        code.extend_from_slice(&0x80BC_u16.to_be_bytes()); // SELF_OBJ
    }
    for value in [5_i32, 42] {
        code.extend_from_slice(&0xC001_u16.to_be_bytes());
        code.extend_from_slice(&value.to_be_bytes());
    }
    code.extend_from_slice(&0x80F0_u16.to_be_bytes()); // ADD_TIMER_EVENT
    code.extend_from_slice(&terminal.to_be_bytes()); // STOP or unsupported
    procedure(&code)
}

fn main_remove() -> ScriptVm {
    procedure(&[
        0x80, 0xBC, // SELF_OBJ
        0x80, 0xF1, // RM_TIMER_EVENT
        0x80, 0x11, // STOP_PROGRAM
    ])
}

fn environment(vm: ScriptVm) -> PreparedScriptEnvironment {
    let tables = ScriptTables {
        categories: [
            vec![],
            vec![],
            vec![],
            vec![ScriptRecord {
                script_id: SID,
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
        ],
        bytes_consumed: 0,
    };
    let objects = ObjectTables {
        elevations: [
            vec![MapObject {
                record: ObjectRecord {
                    header: ObjectHeader {
                        id: 77,
                        tile: 20123,
                        x: 0,
                        y: 0,
                        screen_x: 0,
                        screen_y: 0,
                        frame: 0,
                        rotation: 0,
                        fid: SID,
                        flags: 0,
                        elevation: 0,
                        pid: SID as u32,
                        cid: -1,
                        light_distance: 0,
                        light_intensity: 0,
                        serialized_outline: 0,
                        script_id: SID,
                        field_80: -1,
                    },
                    inventory_length: 0,
                    inventory_capacity: 0,
                    update: ObjectUpdate::Generic {
                        flags: 0,
                        subtype_fields: vec![],
                    },
                },
                inventory: vec![],
            }],
            vec![],
            vec![],
        ],
        total_objects: 1,
        bytes_consumed: 0,
    };
    let query = MapObjectReadBindings::from_tables(&objects).unwrap();
    let owners = MapScriptOwners::from_map(&tables, &objects, &query).unwrap();
    assert_eq!(query.handle_for(OWNER), Some(1));
    let mut runtime = ScriptRuntime::new();
    runtime.register(1, vm).unwrap();
    let bound = BoundScriptRuntime {
        runtime,
        entries: vec![InstalledScriptBinding {
            binding: ScriptBinding {
                category: 3,
                ordinal: 0,
                script_id: SID,
                script_index: 0,
                owner_id: 77,
                path: Some("scripts/TIMED.int".to_owned()),
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
    PreparedScriptEnvironment {
        scripts: bound,
        map_locals,
        game_globals: Some(GameGlobalState::from_initial_values(&[0]).unwrap()),
        object_queries: query,
        script_owners: owners,
        map_last_visit_ticks: 0,
        invocations: BTreeMap::new(),
        spatial_sources: BTreeMap::new(),
    }
}

fn clock(ticks: i32) -> ScriptClockContext {
    ScriptClockContext {
        clock: ScriptGameClock { ticks },
        last_visit_ticks: Some(0),
    }
}

#[test]
fn installed_ce_self_obj_schedules_queue_and_due_timer_writes_quest_flag() {
    let mut env = environment(main_add(None, 0x8011));
    let mut queue = EventQueue::new();
    let started = env
        .advance_script_state_with_timer_queue_bounded(&mut queue, clock(100), 1000, 5, 0)
        .unwrap();
    assert_eq!(started.executed_steps, [(1, 5)]);
    assert_eq!(queue.len(), 1);
    assert_eq!(queue.next_time(), 105);
    assert!(queue.find(Some(OWNER), EventType::Script));
    assert_eq!(env.game_globals.as_ref().unwrap().values(), [0]);

    let before = env
        .process_due_script_timers_transactional_bounded(&mut queue, clock(104), 104, 1004, 1, 0)
        .unwrap();
    assert!(before.is_empty());
    let due = env
        .process_due_script_timers_transactional_bounded(&mut queue, clock(105), 105, 1005, 1, 0)
        .unwrap();
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].0.sid, SID);
    assert_eq!(due[0].0.fixed_parameter, 42);
    assert_eq!(due[0].0.owner, Some(OWNER));
    assert_eq!(due[0].1, None);
    assert!(queue.is_empty());
    assert_eq!(env.game_globals.as_ref().unwrap().values(), [0]);

    let tick = env
        .advance_script_state_tick_bounded(clock(105), 1006, 5, 0)
        .unwrap();
    assert_eq!(tick.executed_steps, [(1, 5)]);
    assert_eq!(env.game_globals.as_ref().unwrap().values(), [42]);
    assert_eq!(
        env.scripts.runtime.vm(1).unwrap().halt_reason(),
        Some(ScriptHaltReason::StopProgram)
    );
    assert_eq!(
        env.scripts
            .runtime
            .vm(1)
            .unwrap()
            .modeled_program_low_flags(),
        8
    );
    assert_eq!(env.invocations.get(&1).unwrap().action(), 22);
    assert_eq!(env.invocations.get(&1).unwrap().fixed_parameter(), 42);
}

#[test]
fn terminal_exit_program_still_receives_ce_timed_event_and_restores_exit_flags() {
    let mut env = environment(main_add(None, 0x8010)); // EXIT_PROGRAM
    let mut queue = EventQueue::new();
    env.advance_script_state_with_timer_queue_bounded(&mut queue, clock(100), 1000, 5, 0)
        .unwrap();
    assert_eq!(
        env.scripts.runtime.vm(1).unwrap().halt_reason(),
        Some(ScriptHaltReason::ExitProgram)
    );
    env.process_due_script_timers_transactional_bounded(&mut queue, clock(105), 105, 1005, 1, 0)
        .unwrap();
    assert_eq!(env.scripts.runtime.vm(1).unwrap().pc(), 24);
    assert!(!env.scripts.runtime.vm(1).unwrap().halted());
    env.advance_script_state_tick_bounded(clock(105), 1006, 5, 0)
        .unwrap();
    assert_eq!(env.game_globals.as_ref().unwrap().values(), [42]);
    assert_eq!(
        env.scripts.runtime.vm(1).unwrap().halt_reason(),
        Some(ScriptHaltReason::ExitProgram)
    );
    assert_eq!(
        env.scripts
            .runtime
            .vm(1)
            .unwrap()
            .modeled_program_low_flags(),
        1
    );
    assert!(queue.is_empty());
}

#[test]
fn timer_remove_native_clears_all_event_types_owned_by_same_ce_object() {
    let mut env = environment(main_remove());
    let mut queue = EventQueue::new();
    queue
        .add(100, 5, EventType::Script, Some(OWNER), vec![0; 8])
        .unwrap();
    queue
        .add(100, 8, EventType::Drug, Some(OWNER), vec![])
        .unwrap();
    queue
        .add(100, 9, EventType::Script, None, vec![0; 8])
        .unwrap();
    let result = env
        .advance_script_state_with_timer_queue_bounded(&mut queue, clock(100), 1000, 3, 0)
        .unwrap();
    assert_eq!(result.executed_steps, [(1, 3)]);
    assert_eq!(queue.len(), 1);
    assert!(!queue.find(Some(OWNER), EventType::Script));
    assert_eq!(queue.next_time(), 109);
    assert_eq!(env.game_globals.as_ref().unwrap().values(), [0]);
}

#[test]
fn invalid_pointer_or_late_opcode_rolls_back_vm_and_queued_timer_writes() {
    for vm in [main_add(Some(900), 0x8011), main_add(None, 0x80FF)] {
        let mut env = environment(vm);
        let mut queue = EventQueue::new();
        let failed =
            env.advance_script_state_with_timer_queue_bounded(&mut queue, clock(100), 1000, 5, 0);
        assert!(failed.is_err());
        assert!(queue.is_empty());
        assert_eq!(env.scripts.runtime.vm(1).unwrap().pc(), 0);
        assert!(env.scripts.runtime.vm(1).unwrap().stack().is_empty());
        assert_eq!(env.game_globals.as_ref().unwrap().values(), [0]);
    }
}
