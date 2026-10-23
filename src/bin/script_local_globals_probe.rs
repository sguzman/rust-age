//! Synthetic CE MAP-global and quest-global immutable procedure probe.
use rust_age::game_globals::GameGlobalState;
use rust_age::map::MapVariables;
use rust_age::script_binding::BoundScriptRuntime;
use rust_age::script_clock::ScriptGameClock;
use rust_age::script_native_vars::{
    apply_readonly_event_native_with_vars, ScriptClockContext,
    ScriptInvocationState, ScriptNativeEventSnapshot, ScriptNativeOwnerSnapshot,
    ScriptNativeSnapshot,
};
use rust_age::script_owner_binding::ScriptOwnerResolution;
use rust_age::script_program::ScriptProgram;
use rust_age::script_runtime::ScriptRuntime;
use rust_age::script_vm::{ScriptValue, ScriptVm};
use std::error::Error;

fn probe(parts: &[&str]) -> Result<i32, Box<dyn Error>> {
    let values = parts[1..].iter().map(|v| v.parse::<i32>())
        .collect::<Result<Vec<_>, _>>()?;
    let [map_count, game_count, map_index, game_index, map0, map1, game0, game1] =
        values.as_slice()
    else {
        return Err("wrong synthetic CE variable case size".into());
    };
    let map_count = usize::try_from(*map_count)?;
    let game_count = usize::try_from(*game_count)?;
    let map_values = [*map0, *map1][..map_count].to_vec();
    let game_values = [*game0, *game1][..game_count].to_vec();
    let binding = BoundScriptRuntime {
        runtime: ScriptRuntime::new(),
        entries: vec![],
    };
    let map = binding.script_local_state(&MapVariables {
        globals: map_values,
        locals: vec![],
    })?;
    let game = GameGlobalState::from_initial_values(&game_values)?;
    let snapshot = ScriptNativeEventSnapshot {
        owner: ScriptNativeOwnerSnapshot {
            native: ScriptNativeSnapshot {
                clock: ScriptClockContext {
                    clock: ScriptGameClock { ticks: 0 },
                    last_visit_ticks: None,
                },
                objects: None,
            },
            resolution: Some(ScriptOwnerResolution::NullOwner),
        },
        source: Some(0),
        target: Some(0),
    };
    let mut code = vec![0x80, 0x1A, 0xC0, 0x01];
    code.extend_from_slice(&map_index.to_be_bytes());
    code.extend_from_slice(&[0x80, 0xC3, 0xC0, 0x01]);
    code.extend_from_slice(&game_index.to_be_bytes());
    code.extend_from_slice(&[0x80, 0xC5, 0x80, 0x39, 0x80, 0x25]);
    let mut bytes = vec![0_u8; 42];
    bytes[20..20 + code.len()].copy_from_slice(&code);
    bytes.extend_from_slice(&1_i32.to_be_bytes());
    for field in [4_i32, 0, 0, 0, 20, 0] {
        bytes.extend_from_slice(&field.to_be_bytes());
    }
    bytes.extend_from_slice(&5_i32.to_be_bytes());
    bytes.extend_from_slice(b"main\0");
    bytes.extend_from_slice(&0_i32.to_be_bytes());
    let vm = ScriptVm::new(ScriptProgram::decode(&bytes)?);
    let invocation = ScriptInvocationState::installed();
    let result = vm.preview_local_action_with_host_bounded(0, 8, &mut |op, stack| {
        apply_readonly_event_native_with_vars(
            snapshot, &invocation, &map, Some(&game), op, stack,
        )
    })?;
    if vm.pc() != 0 || !vm.stack().is_empty() || !vm.return_stack().is_empty() {
        return Err("original CE VM changed during cloned variable query".into());
    }
    match result {
        Some(ScriptValue::Int(result)) => Ok(result),
        _ => Err("unexpected CE variable result".into()),
    }
}

fn main() {
    for line in include_str!("../../tests/script_local_globals_cases.txt").lines() {
        let parts: Vec<_> = line.split_whitespace().collect();
        if parts.len() != 9 {
            continue;
        }
        let result = probe(&parts)
            .map(|value| format!("S:{value}"))
            .unwrap_or_else(|_| "ERR".to_owned());
        println!("{} {}", parts[0], result);
    }
}
