//! Synthetic CE cloned local executeProc/return parity fixture runner.
use rust_age::script_program::ScriptProgram;
use rust_age::script_vm::{ScriptValue, ScriptVm};
use std::error::Error;

fn probe(parts: &[&str]) -> Result<String, Box<dyn Error>> {
    let value = parts[2].parse::<i32>()?;
    let budget = parts[3].parse::<usize>()?;
    let mut code = Vec::new();
    match parts[1] {
        "void" => code.extend_from_slice(&[0x80, 0x1A, 0x80, 0x21]),
        "value" => {
            code.extend_from_slice(&[0x80, 0x1A, 0xC0, 0x01]);
            code.extend_from_slice(&value.to_be_bytes());
            code.extend_from_slice(&0x8025_u16.to_be_bytes());
        }
        "unbalanced" => code.extend_from_slice(&0x8021_u16.to_be_bytes()),
        "stop" => code.extend_from_slice(&[0x80, 0x1A, 0x80, 0x11]),
        "native" => code.extend_from_slice(&[0x80, 0xBD, 0x80, 0x21]),
        _ => return Err("unsupported synthetic local action kind".into()),
    }
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
    let result = vm.preview_local_action_core_bounded(0, budget)?;
    if vm.pc() != 0 || vm.halted() || !vm.stack().is_empty() {
        return Err("CE core preview mutated original installed VM".into());
    }
    match result {
        None => Ok("S:void".to_owned()),
        Some(ScriptValue::Int(number)) => Ok(format!("S:{number}")),
        _ => Err("non-integer synthetic CE local action result".into()),
    }
}

fn main() {
    for line in include_str!("../../tests/script_local_return_cases.txt").lines() {
        let parts: Vec<_> = line.split_whitespace().collect();
        if parts.len() != 4 {
            continue;
        }
        println!(
            "{} {}",
            parts[0],
            probe(&parts).unwrap_or_else(|_| "ERR".to_owned())
        );
    }
}
