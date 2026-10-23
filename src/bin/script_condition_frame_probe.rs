//! Adapted synthetic CE doEvents condition-state comparison probe.
//! No compiled original engine or commercial Fallout assets are loaded.
use rust_age::script_program::ScriptProgram;
use rust_age::script_vm::{ScriptValue, ScriptVm};
use std::error::Error;

fn push_int(code: &mut Vec<u8>, value: i32) {
    code.extend_from_slice(&0xC001_u16.to_be_bytes());
    code.extend_from_slice(&value.to_be_bytes());
}

fn op(code: &mut Vec<u8>, opcode: u16) {
    code.extend_from_slice(&opcode.to_be_bytes());
}

fn probe(mode: &str, saved: i32, expected: i32) -> Result<bool, Box<dyn Error>> {
    let mut startup = Vec::new();
    push_int(&mut startup, saved);
    match mode {
        "DATA" => {}
        "RETURN" => op(&mut startup, 0x800D), // D_TO_A
        "GLOBAL" => {
            op(&mut startup, 0x802C); // SET_GLOBAL with base 1
            push_int(&mut startup, 5); // unrelated persistent slot
        }
        "FRAME" => {
            push_int(&mut startup, 0);
            op(&mut startup, 0x802B); // PUSH_BASE with zero arguments
        }
        _ => return Err("unknown CE condition context".into()),
    }
    op(&mut startup, 0x8011); // STOP_PROGRAM
    if startup.len() > 20 {
        return Err("startup overlapped condition bytecode".into());
    }
    let mut condition = Vec::new();
    match mode {
        "DATA" => {}
        "RETURN" => op(&mut condition, 0x800C), // A_TO_D
        "GLOBAL" => {
            push_int(&mut condition, -1);
            op(&mut condition, 0x8012); // FETCH_GLOBAL at base-1
        }
        "FRAME" => {
            push_int(&mut condition, -1);
            op(&mut condition, 0x8032); // FETCH from frame-1
        }
        _ => unreachable!(),
    }
    push_int(&mut condition, expected);
    op(&mut condition, 0x8033); // CE integer EQUAL
    op(&mut condition, 0x8011); // STOP_PROGRAM
    if condition.len() > 22 {
        return Err("condition exceeded synthetic bytecode region".into());
    }
    let mut bytes = vec![0_u8; 42];
    bytes[..startup.len()].copy_from_slice(&startup);
    bytes[20..20 + condition.len()].copy_from_slice(&condition);
    bytes.extend_from_slice(&[0_u8; 12]); // no procedures/IDs/strings
    let mut vm = ScriptVm::new(ScriptProgram::decode(&bytes)?);
    let step_count = match mode {
        "DATA" => 2,
        "RETURN" => 3,
        "GLOBAL" | "FRAME" => 4,
        _ => unreachable!(),
    };
    vm.run_bounded(step_count)?;
    let pc = vm.pc();
    let stack = vm.stack().to_vec();
    let ret = vm.return_stack().to_vec();
    let frame = vm.frame_pointer();
    let base = vm.base_pointer();
    let halted = vm.halted();
    let result = vm.preview_condition_bounded(20, 5)?;
    if vm.pc() != pc
        || vm.stack() != stack
        || vm.return_stack() != ret
        || vm.frame_pointer() != frame
        || vm.base_pointer() != base
        || vm.halted() != halted
    {
        return Err("CE cloned condition mutated installed interpreter".into());
    }
    if !vm.halted() || !vm.stack().iter().all(|x| matches!(x, ScriptValue::Int(_))) {
        return Err("CE condition fixture lost its original stopped state".into());
    }
    Ok(result)
}

fn main() {
    for line in include_str!("../../tests/script_condition_frame_cases.txt").lines() {
        let parts: Vec<_> = line.split_whitespace().collect();
        if parts.len() != 4 {
            continue;
        }
        let result = (|| -> Result<bool, Box<dyn Error>> {
            probe(parts[1], parts[2].parse()?, parts[3].parse()?)
        })();
        let output = result
            .map(|value| format!("S:{}", i32::from(value)))
            .unwrap_or_else(|_| "ERR".to_owned());
        println!("{} {}", parts[0], output);
    }
}
