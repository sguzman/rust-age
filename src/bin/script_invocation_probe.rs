//! Rust side of the CE action/override synthetic bytecode comparison.
use rust_age::game_globals::GameGlobalState;
use rust_age::map::MapVariables;
use rust_age::script_binding::BoundScriptRuntime;
use rust_age::script_native_vars::{
    run_invocation_subset_bounded, ScriptInvocationState, FIXED_PARAM_OPCODE, SCRIPT_ACTION_OPCODE,
    SCRIPT_OVERRIDES_OPCODE,
};
use rust_age::script_program::ScriptProgram;
use rust_age::script_runtime::ScriptRuntime;
use rust_age::script_vm::{ScriptValue, ScriptVm};
use std::error::Error;

fn probe(present: bool, action: i32, kind: &str) -> Result<String, Box<dyn Error>> {
    let mut code = Vec::new();
    if kind.starts_with("fixed_") {
        code.extend_from_slice(&FIXED_PARAM_OPCODE.to_be_bytes());
    }
    if kind == "override" || kind == "override_read" {
        code.extend_from_slice(&SCRIPT_OVERRIDES_OPCODE.to_be_bytes());
    }
    if kind == "read" || kind == "override_read" {
        code.extend_from_slice(&SCRIPT_ACTION_OPCODE.to_be_bytes());
    }
    if kind == "fixed_sum" {
        code.extend_from_slice(&SCRIPT_ACTION_OPCODE.to_be_bytes());
        code.extend_from_slice(&0x8039_u16.to_be_bytes());
    }
    code.extend_from_slice(&0x8011_u16.to_be_bytes());

    let mut data = vec![0_u8; 42];
    data[..code.len()].copy_from_slice(&code);
    data.extend_from_slice(&[0_u8; 12]);
    let mut vm = ScriptVm::new(ScriptProgram::decode(&data)?);
    let mut invocation = if present {
        ScriptInvocationState::installed()
    } else {
        ScriptInvocationState::missing()
    };
    invocation.begin_action(action);
    if kind == "fixed_first" || kind == "fixed_sum" || kind == "fixed_missing" {
        invocation.set_fixed_parameter(1);
    }
    let bound = BoundScriptRuntime {
        runtime: ScriptRuntime::new(),
        entries: vec![],
    };
    let mut map = bound.script_local_state(&MapVariables {
        globals: vec![],
        locals: vec![],
    })?;
    let mut game = GameGlobalState::from_initial_values(&[])?;
    let steps = run_invocation_subset_bounded(&mut vm, 1, &mut map, &mut game, &mut invocation, 6)?;
    let stack = match vm.stack() {
        [] => "E".to_owned(),
        [ScriptValue::Int(value)] => value.to_string(),
        _ => return Err("unexpected synthetic CE result stack".into()),
    };
    Ok(format!(
        "A:{} O:{} S:{stack} PC:{} STEPS:{steps}",
        invocation.action(),
        i32::from(invocation.overrides()),
        vm.pc()
    ))
}
fn main() {
    for line in include_str!("../../tests/script_invocation_cases.txt").lines() {
        let parts: Vec<_> = line.split_whitespace().collect();
        if parts.len() != 4 {
            continue;
        }
        let result = (|| -> Result<String, Box<dyn Error>> {
            probe(parts[1] == "1", parts[2].parse()?, parts[3])
        })()
        .unwrap_or_else(|_| "ERR".to_owned());
        println!("{} {}", parts[0], result);
    }
}
