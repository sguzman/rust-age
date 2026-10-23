//! Original-format synthetic CE MAP object bytes -> bounded native INT host.
//! This is NOT a game script, playable scene, or compiled CE gameplay oracle.
use rust_age::game_globals::GameGlobalState;
use rust_age::map::MapVariables;
use rust_age::object_tables::ObjectTables;
use rust_age::script_binding::BoundScriptRuntime;
use rust_age::script_clock::ScriptGameClock;
use rust_age::script_native_vars::{
    run_invocation_with_clock_and_visit_bounded, run_invocation_with_native_snapshot_bounded,
    ScriptClockContext, ScriptInvocationState, ScriptNativeSnapshot,
};
use rust_age::script_object_binding::MapObjectReadBindings;
use rust_age::script_object_read::{OBJ_PID_OPCODE, TILE_NUM_OPCODE};
use rust_age::script_program::ScriptProgram;
use rust_age::script_runtime::ScriptRuntime;
use rust_age::script_vm::{ScriptValue, ScriptVm};

fn word(buffer: &mut Vec<u8>, value: i32) {
    buffer.extend_from_slice(&value.to_be_bytes());
}

fn object(buffer: &mut Vec<u8>, tile: i32, fid: i32, pid: i32) {
    // Original 18-word obj_read_obj header, with repeated saved ID 77.
    for value in [
        77, tile, 0, 0, 0, 0, 0, 0, fid, 0, 0, pid, -1, 0, 0, 0, -1, 0,
    ] {
        word(buffer, value);
    }
    // Inventory count/capacity, ignored legacy pointer, generic update flags.
    for value in [0, 0, 0, 0] {
        word(buffer, value);
    }
}

fn parsed_objects() -> ObjectTables {
    let mut bytes = Vec::new();
    word(&mut bytes, 2); // total top-level objects
    word(&mut bytes, 2); // elevation 0
    object(&mut bytes, 20_099, 0x0300_0002, 0x0300_0001);
    object(&mut bytes, 20_098, 0x0400_0003, 0x0400_0002);
    word(&mut bytes, 0); // elevation 1
    word(&mut bytes, 0); // elevation 2
    let result = ObjectTables::decode_with(&bytes, |_| Ok(None)).unwrap();
    assert_eq!(result.bytes_consumed, bytes.len());
    result
}

fn vm_for(handle: i32, native: u16) -> ScriptVm {
    let mut code = Vec::new();
    code.extend_from_slice(&0xE001_u16.to_be_bytes()); // CE PUSH pointer
    word(&mut code, handle);
    code.extend_from_slice(&native.to_be_bytes());
    code.extend_from_slice(&0xC001_u16.to_be_bytes()); // CE PUSH integer
    word(&mut code, 0);
    code.extend_from_slice(&0x80C5_u16.to_be_bytes()); // game_global_var(0)
    code.extend_from_slice(&0x8039_u16.to_be_bytes()); // ADD
    code.extend_from_slice(&0x8011_u16.to_be_bytes()); // STOP
    let mut bytes = vec![0_u8; 42];
    bytes[..code.len()].copy_from_slice(&code);
    bytes.extend_from_slice(&[0_u8; 12]);
    ScriptVm::new(ScriptProgram::decode(&bytes).unwrap())
}

#[test]
fn serialized_map_objects_feed_only_explicit_read_only_native_vm() {
    let decoded = parsed_objects();
    let original = decoded.clone();
    let bindings = MapObjectReadBindings::from_tables(&decoded).unwrap();
    let left = bindings
        .handle_for(rust_age::collision::ObjectKey {
            elevation: 0,
            ordinal: 0,
        })
        .unwrap();
    let right = bindings
        .handle_for(rust_age::collision::ObjectKey {
            elevation: 0,
            ordinal: 1,
        })
        .unwrap();
    assert_ne!(left, right); // serialized IDs are both 77

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
    let mut globals = GameGlobalState::from_initial_values(&[7]).unwrap();
    let mut action = ScriptInvocationState::installed();
    let clock = ScriptClockContext {
        clock: ScriptGameClock { ticks: 0 },
        last_visit_ticks: None,
    };
    for (handle, opcode, expected) in [
        (left, TILE_NUM_OPCODE, 20_106),
        (right, TILE_NUM_OPCODE, 20_105),
        (right, OBJ_PID_OPCODE, 0x0400_0002 + 7),
    ] {
        let mut vm = vm_for(handle, opcode);
        assert_eq!(
            run_invocation_with_native_snapshot_bounded(
                &mut vm,
                1,
                &mut locals,
                &mut globals,
                &mut action,
                ScriptNativeSnapshot {
                    clock,
                    objects: Some(bindings.snapshots()),
                },
                12,
            )
            .unwrap(),
            6
        );
        assert!(vm.halted());
        assert_eq!(vm.stack(), [ScriptValue::Int(expected)]);
    }
    assert_eq!(globals.values(), [7]);
    assert_eq!(decoded, original); // the decoded MAP was never mutated

    let mut plain = vm_for(left, TILE_NUM_OPCODE);
    plain.step().unwrap();
    assert!(plain.step().is_err());
    assert_eq!(plain.stack(), [ScriptValue::Pointer(left)]);

    let mut clock_only = vm_for(left, TILE_NUM_OPCODE);
    assert!(run_invocation_with_clock_and_visit_bounded(
        &mut clock_only,
        1,
        &mut locals,
        &mut globals,
        &mut action,
        clock,
        12,
    )
    .is_err());
    assert_eq!(clock_only.stack(), [ScriptValue::Pointer(left)]);
    assert_eq!(clock_only.pc(), 6);
}

#[test]
fn decoded_map_tile_contains_pid_uses_explicit_combined_host() {
    let decoded = parsed_objects();
    let bindings = MapObjectReadBindings::from_tables(&decoded).unwrap();
    let prepared = BoundScriptRuntime {
        runtime: ScriptRuntime::new(),
        entries: vec![],
    };
    let mut locals = prepared
        .script_local_state(&MapVariables {
            globals: vec![],
            locals: vec![],
        })
        .unwrap();
    let mut globals = GameGlobalState::from_initial_values(&[7]).unwrap();
    let mut invocation = ScriptInvocationState::installed();

    for (tile, elevation, pid, expected) in [
        (20_099, 0, 0x0300_0001, 8),
        (20_098, 0, 0x0400_0002, 8),
        (20_099, 1, 0x0300_0001, 7),
        (20_099, 0, 0x0300_0002, 7),
    ] {
        let mut code = Vec::new();
        for value in [tile, elevation, pid] {
            code.extend_from_slice(&0xC001_u16.to_be_bytes());
            word(&mut code, value);
        }
        code.extend_from_slice(&0x80BB_u16.to_be_bytes());
        code.extend_from_slice(&0xC001_u16.to_be_bytes());
        word(&mut code, 0);
        code.extend_from_slice(&0x80C5_u16.to_be_bytes());
        code.extend_from_slice(&0x8039_u16.to_be_bytes());
        code.extend_from_slice(&0x8011_u16.to_be_bytes());
        let mut bytes = vec![0_u8; 42];
        bytes[..code.len()].copy_from_slice(&code);
        bytes.extend_from_slice(&[0_u8; 12]);
        let mut vm = ScriptVm::new(ScriptProgram::decode(&bytes).unwrap());
        assert_eq!(
            run_invocation_with_native_snapshot_bounded(
                &mut vm,
                1,
                &mut locals,
                &mut globals,
                &mut invocation,
                ScriptNativeSnapshot {
                    clock: ScriptClockContext {
                        clock: ScriptGameClock { ticks: 100 },
                        last_visit_ticks: None,
                    },
                    objects: Some(bindings.snapshots()),
                },
                12,
            )
            .unwrap(),
            8
        );
        assert!(vm.halted());
        assert_eq!(vm.stack(), [ScriptValue::Int(expected)]);
    }
    assert_eq!(globals.values(), [7]); // detached queries did not write globals

    let mut code = Vec::new();
    for value in [20_099, 0, 0x0300_0001] {
        code.extend_from_slice(&0xC001_u16.to_be_bytes());
        word(&mut code, value);
    }
    code.extend_from_slice(&0x80BB_u16.to_be_bytes());
    code.extend_from_slice(&0x8011_u16.to_be_bytes());
    let mut bytes = vec![0_u8; 42];
    bytes[..code.len()].copy_from_slice(&code);
    bytes.extend_from_slice(&[0_u8; 12]);
    let program = ScriptProgram::decode(&bytes).unwrap();
    let mut unhosted = ScriptVm::new(program);
    for _ in 0..3 {
        unhosted.step().unwrap();
    }
    assert!(unhosted.step().is_err()); // native denied in ordinary VM
}

#[test]
fn unknown_object_token_fails_without_mutating_quest_globals() {
    let objects = parsed_objects();
    let snapshots = MapObjectReadBindings::from_tables(&objects).unwrap();
    let mut vm = vm_for(999, TILE_NUM_OPCODE);
    let mut globals = GameGlobalState::from_initial_values(&[7]).unwrap();
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
    let mut invocation = ScriptInvocationState::installed();
    assert!(run_invocation_with_native_snapshot_bounded(
        &mut vm,
        1,
        &mut locals,
        &mut globals,
        &mut invocation,
        ScriptNativeSnapshot {
            clock: ScriptClockContext {
                clock: ScriptGameClock { ticks: 0 },
                last_visit_ticks: None,
            },
            objects: Some(snapshots.snapshots()),
        },
        12,
    )
    .is_err());
    assert_eq!(vm.pc(), 6);
    assert_eq!(vm.stack(), [ScriptValue::Pointer(999)]);
    assert_eq!(globals.values(), [7]);
}
