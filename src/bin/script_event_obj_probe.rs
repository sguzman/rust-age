//! Synthetic CE source_obj / target_obj bounded event host parity.
use rust_age::game_globals::GameGlobalState;
use rust_age::map::MapVariables;
use rust_age::script_binding::BoundScriptRuntime;
use rust_age::script_clock::ScriptGameClock;
use rust_age::script_native_vars::{
    run_invocation_with_event_snapshot_bounded, ScriptClockContext, ScriptInvocationState,
    ScriptNativeEventSnapshot, ScriptNativeOwnerSnapshot, ScriptNativeSnapshot,
};
use rust_age::script_object_read::ScriptObjectSnapshot;
use rust_age::script_program::ScriptProgram;
use rust_age::script_runtime::ScriptRuntime;
use rust_age::script_vm::{ScriptValue, ScriptVm};
use std::error::Error;

fn probe(parts: &[&str]) -> Result<String, Box<dyn Error>> {
    let args = parts[1..]
        .iter()
        .map(|s| s.parse::<i32>())
        .collect::<Result<Vec<_>, _>>()?;
    let [source_handle, target_handle, source_tile, target_pid] = args.as_slice() else {
        return Err("invalid CE event fields".into());
    };
    let objects = [
        ScriptObjectSnapshot {
            handle: 41,
            tile: *source_tile,
            elevation: 0,
            table_elevation: 0,
            pid: 0,
            fid: 0,
        },
        ScriptObjectSnapshot {
            handle: 42,
            tile: 20099,
            elevation: 0,
            table_elevation: 0,
            pid: *target_pid,
            fid: 0,
        },
    ];
    let code = [0x80BD_u16, 0x80D4, 0x80BE, 0x8100, 0x8039, 0x8011];
    let mut bytes = vec![0_u8; 42];
    for (i, op) in code.iter().enumerate() {
        bytes[2 * i..2 * i + 2].copy_from_slice(&op.to_be_bytes());
    }
    bytes.extend_from_slice(&[0_u8; 12]);
    let mut vm = ScriptVm::new(ScriptProgram::decode(&bytes)?);
    let binding = BoundScriptRuntime {
        runtime: ScriptRuntime::new(),
        entries: vec![],
    };
    let mut locals = binding.script_local_state(&MapVariables {
        globals: vec![],
        locals: vec![],
    })?;
    let mut globals = GameGlobalState::from_initial_values(&[])?;
    let mut invocation = ScriptInvocationState::installed();
    let steps = run_invocation_with_event_snapshot_bounded(
        &mut vm,
        1,
        &mut locals,
        &mut globals,
        &mut invocation,
        ScriptNativeEventSnapshot {
            owner: ScriptNativeOwnerSnapshot {
                native: ScriptNativeSnapshot {
                    clock: ScriptClockContext {
                        clock: ScriptGameClock { ticks: 0 },
                        last_visit_ticks: None,
                    },
                    objects: Some(&objects),
                },
                resolution: None,
            },
            source: Some(*source_handle),
            target: Some(*target_handle),
        },
        12,
    )?;
    match vm.stack() {
        [ScriptValue::Int(value)] if vm.halted() => {
            Ok(format!("S:{value} N:{steps} PC:{}", vm.pc()))
        }
        _ => Err("unexpected CE event native result stack".into()),
    }
}

fn main() {
    for line in include_str!("../../tests/script_event_obj_cases.txt").lines() {
        let parts = line.split_whitespace().collect::<Vec<_>>();
        if parts.len() != 5 {
            continue;
        }
        let result = probe(&parts).unwrap_or_else(|_| "ERR".to_owned());
        println!("{} {result}", parts[0]);
    }
}
