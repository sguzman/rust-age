//! Invented source-derived CE local POP_FLAGS return-frame comparison.
//! No game program, callback pointer, saved game, or commercial asset is loaded.
use rust_age::script_program::ScriptProgram;
use rust_age::script_vm::{ScriptHaltReason, ScriptValue, ScriptVm};
use std::error::Error;

fn push(code: &mut Vec<u8>, opcode: u16, value: i32) {
    code.extend_from_slice(&opcode.to_be_bytes());
    code.extend_from_slice(&value.to_be_bytes());
}

fn op(code: &mut Vec<u8>, opcode: u16) {
    code.extend_from_slice(&opcode.to_be_bytes());
}

fn simulate(
    mode: &str,
    flags: i32,
    window: i32,
    wait: i32,
    address: i32,
    returned: i32,
) -> Result<String, Box<dyn Error>> {
    let opcode = match mode {
        "FLAGS" => 0x801F,
        "RETURN" => 0x8020,
        "EXIT" => 0x8021,
        "VALUE_EXIT" => 0x8025,
        _ => return Err("unsupported CE local return opcode".into()),
    };
    let mut code = Vec::new();
    if mode != "FLAGS" {
        push(&mut code, 0xC001, address);
        op(&mut code, 0x800D); // D_TO_A
    }
    push(&mut code, 0xC001, flags);
    push(&mut code, 0xE001, wait);
    push(&mut code, 0xC001, window);
    if mode == "VALUE_EXIT" {
        push(&mut code, 0xC001, returned);
    }
    op(&mut code, opcode);
    if code.len() > 42 {
        return Err("synthetic CE return frame overflow".into());
    }
    let mut bytes = vec![0_u8; 42];
    bytes[..code.len()].copy_from_slice(&code);
    bytes.extend_from_slice(&[0_u8; 12]);
    let mut vm = ScriptVm::new(ScriptProgram::decode(&bytes)?);
    let steps = if mode == "FLAGS" {
        4
    } else if mode == "VALUE_EXIT" {
        7
    } else {
        6
    };
    for _ in 0..steps {
        vm.step()?;
    }
    if !vm.return_stack().is_empty() {
        return Err("synthetic CE local return stack was not consumed".into());
    }
    let halted = i32::from(vm.halted());
    let reason = match vm.halt_reason() {
        None => "NONE",
        Some(ScriptHaltReason::ReturnFromCall) => "RETURN",
        _ => return Err("wrong synthetic CE return flag reason".into()),
    };
    let observed_value = match vm.stack() {
        [] => "_".to_owned(),
        [ScriptValue::Int(value)] => value.to_string(),
        _ => return Err("unexpected CE local frame operand remains".into()),
    };
    Ok(format!(
        "S:{}:{}:{}:{}:{}",
        vm.pc(),
        vm.modeled_program_low_flags(),
        halted,
        reason,
        observed_value
    ))
}

fn main() {
    for line in include_str!("../../tests/script_pop_flags_cases.txt").lines() {
        let parts: Vec<_> = line.split_whitespace().collect();
        if parts.len() != 7 {
            continue;
        }
        let result = (|| -> Result<String, Box<dyn Error>> {
            simulate(
                parts[1],
                parts[2].parse()?,
                parts[3].parse()?,
                parts[4].parse()?,
                parts[5].parse()?,
                parts[6].parse()?,
            )
        })()
        .unwrap_or_else(|_| "ERR".to_owned());
        println!("{} {}", parts[0], result);
    }
}
