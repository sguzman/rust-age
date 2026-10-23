//! Invented CE conditional expression: map_var(index) == wanted.
//! The host permits an immutable MAP read only; no original game runs.
use rust_age::map::MapVariables;
use rust_age::script_binding::BoundScriptRuntime;
use rust_age::script_clock::ScriptGameClock;
use rust_age::script_native_vars::{
    apply_readonly_event_native_with_vars, ScriptClockContext, ScriptInvocationState,
    ScriptNativeEventSnapshot, ScriptNativeOwnerSnapshot, ScriptNativeSnapshot,
};
use rust_age::script_owner_binding::ScriptOwnerResolution;
use rust_age::script_program::ScriptProgram;
use rust_age::script_runtime::ScriptRuntime;
use rust_age::script_vm::ScriptVm;
use std::error::Error;

fn probe(parts: &[&str]) -> Result<bool, Box<dyn Error>> {
    let values = parts[2..]
        .iter()
        .map(|value| value.parse::<i32>())
        .collect::<Result<Vec<_>, _>>()?;
    let [count, index, wanted, first, second] = values.as_slice() else {
        return Err("wrong synthetic CE condition case length".into());
    };
    let (control, false_arm, step_budget) = match parts[1] {
        "IF" => (0x802F_u16, 32_i32, 8_usize),
        "WHILE" => (0x8030_u16, 34_i32, 9_usize),
        _ => return Err("invalid CE condition branch selector".into()),
    };
    let count = usize::try_from(*count)?;
    if count > 2 {
        return Err("synthetic MAP globals exceed fixture capacity".into());
    }
    let bound = BoundScriptRuntime {
        runtime: ScriptRuntime::new(),
        entries: vec![],
    };
    let locals = bound.script_local_state(&MapVariables {
        globals: [*first, *second][..count].to_vec(),
        locals: vec![],
    })?;
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
    // CE IF pops the condition; when false it jumps to byte 32,
    // otherwise it drops the destination and executes the true arm.
    let mut code = Vec::new();
    code.extend_from_slice(&0xC001_u16.to_be_bytes()); // PUSH false-arm address
    code.extend_from_slice(&false_arm.to_be_bytes());
    code.extend_from_slice(&0xC001_u16.to_be_bytes()); // PUSH MAP index
    code.extend_from_slice(&index.to_be_bytes());
    code.extend_from_slice(&0x80C3_u16.to_be_bytes()); // map_var
    code.extend_from_slice(&0xC001_u16.to_be_bytes()); // PUSH expected value
    code.extend_from_slice(&wanted.to_be_bytes());
    code.extend_from_slice(&0x8033_u16.to_be_bytes()); // EQUAL
    code.extend_from_slice(&control.to_be_bytes()); // IF or WHILE
    if control == 0x8030 {
        code.extend_from_slice(&0x801A_u16.to_be_bytes()); // WHILE keeps the true branch target
    }
    code.extend_from_slice(&0xC001_u16.to_be_bytes()); // true
    code.extend_from_slice(&1_i32.to_be_bytes());
    code.extend_from_slice(&0x8011_u16.to_be_bytes()); // STOP
    assert_eq!(code.len(), usize::try_from(false_arm)?);
    code.extend_from_slice(&0xC001_u16.to_be_bytes()); // false
    code.extend_from_slice(&0_i32.to_be_bytes());
    code.extend_from_slice(&0x8011_u16.to_be_bytes());
    let mut bytes = vec![0_u8; 42];
    bytes[..code.len()].copy_from_slice(&code);
    bytes.extend_from_slice(&1_i32.to_be_bytes());
    for value in [4_i32, 0x02, 0, 0, 20, 0] {
        bytes.extend_from_slice(&value.to_be_bytes());
    }
    bytes.extend_from_slice(&5_i32.to_be_bytes());
    bytes.extend_from_slice(b"main\0");
    bytes.extend_from_slice(&0_i32.to_be_bytes());
    let vm = ScriptVm::new(ScriptProgram::decode(&bytes)?);
    let invocation = ScriptInvocationState::installed();
    let result = vm.preview_condition_with_host_bounded(0, step_budget, &mut |opcode, stack| {
        apply_readonly_event_native_with_vars(
            snapshot,
            &invocation,
            &locals,
            None,
            0,
            opcode,
            stack,
        )
    })?;
    if vm.pc() != 0
        || vm.halted()
        || !vm.stack().is_empty()
        || !vm.return_stack().is_empty()
        || vm.runtime_procedures()[0].flags != 0x02
    {
        return Err("conditional preview changed the original VM".into());
    }
    Ok(result)
}

fn main() {
    for line in include_str!("../../tests/script_condition_read_cases.txt").lines() {
        let parts: Vec<_> = line.split_whitespace().collect();
        if parts.len() != 7 {
            continue;
        }
        let result = probe(&parts)
            .map(|value| format!("S:{}", i32::from(value)))
            .unwrap_or_else(|_| "ERR".to_owned());
        println!("{} {}", parts[0], result);
    }
}
