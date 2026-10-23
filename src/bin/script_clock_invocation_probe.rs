//! Synthetic full CE native script-state + clock bytecode trace probe.
use rust_age::game_globals::GameGlobalState;
use rust_age::map::MapVariables;
use rust_age::script_binding::BoundScriptRuntime;
use rust_age::script_clock::ScriptGameClock;
use rust_age::script_native_vars::{run_invocation_with_clock_bounded, ScriptInvocationState};
use rust_age::script_program::ScriptProgram;
use rust_age::script_runtime::ScriptRuntime;
use rust_age::script_vm::{ScriptValue, ScriptVm};
use std::error::Error;

fn op(code: &mut Vec<u8>, value: u16) {
    code.extend_from_slice(&value.to_be_bytes());
}
fn int(code: &mut Vec<u8>, value: i32) {
    op(code, 0xC001);
    code.extend_from_slice(&value.to_be_bytes());
}
fn program() -> Result<ScriptVm, Box<dyn Error>> {
    let mut code = Vec::new();
    for opcode in [0x80F6, 0x80C7, 0x8039, 0x80F7, 0x8039, 0x80EB, 0x8039] {
        op(&mut code, opcode);
    }
    int(&mut code, 0);
    op(&mut code, 0x80C6);
    op(&mut code, 0x80B9);
    int(&mut code, 0);
    op(&mut code, 0x80C5);
    op(&mut code, 0x8011);
    let mut bytes = vec![0_u8; 42];
    bytes[..code.len()].copy_from_slice(&code);
    bytes.extend_from_slice(&[0_u8; 12]);
    Ok(ScriptVm::new(ScriptProgram::decode(&bytes)?))
}
fn run(
    ticks: i32,
    action: i32,
    fixed: i32,
    installed: bool,
    loaded: bool,
) -> Result<String, Box<dyn Error>> {
    let mut vm = program()?;
    let bound = BoundScriptRuntime {
        runtime: ScriptRuntime::new(),
        entries: Vec::new(),
    };
    let mut locals = bound.script_local_state(&MapVariables {
        globals: Vec::new(),
        locals: Vec::new(),
    })?;
    let mut game = GameGlobalState::from_initial_values(if loaded { &[7] } else { &[] })?;
    let mut invocation = if installed {
        ScriptInvocationState::installed()
    } else {
        ScriptInvocationState::missing()
    };
    invocation.set_fixed_parameter(fixed);
    invocation.begin_action(action);
    let steps = run_invocation_with_clock_bounded(
        &mut vm,
        1,
        &mut locals,
        &mut game,
        &mut invocation,
        ScriptGameClock { ticks },
        16,
    )?;
    let top = match vm.stack() {
        [ScriptValue::Int(value)] => *value,
        _ => return Err("unexpected CE native trace result stack".into()),
    };
    let game_value = game
        .values()
        .first()
        .map(i32::to_string)
        .unwrap_or_else(|| "none".to_owned());
    Ok(format!(
        "S:{top} G:{game_value} O:{} PC:{} N:{steps}",
        i32::from(invocation.overrides()),
        vm.pc()
    ))
}
fn main() {
    for line in include_str!("../../tests/script_clock_invocation_cases.txt").lines() {
        let parts: Vec<_> = line.split_whitespace().collect();
        if parts.len() != 6 {
            continue;
        }
        let output = (|| -> Result<String, Box<dyn Error>> {
            run(
                parts[1].parse()?,
                parts[2].parse()?,
                parts[3].parse()?,
                parts[4] == "1",
                parts[5] == "1",
            )
        })()
        .unwrap_or_else(|_| "ERR".to_owned());
        println!("{} {}", parts[0], output);
    }
}
