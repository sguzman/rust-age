//! Independent synthetic CE local executeProc commit-state comparison.
//! No commercial Fallout data or external native callbacks are installed.
use rust_age::script_program::ScriptProgram;
use rust_age::script_vm::{ScriptValue, ScriptVm};
use std::error::Error;

fn push_int(code: &mut Vec<u8>, value: i32) {
    code.extend_from_slice(&0xC001_u16.to_be_bytes());
    code.extend_from_slice(&value.to_be_bytes());
}

fn op(code: &mut Vec<u8>, value: u16) {
    code.extend_from_slice(&value.to_be_bytes());
}

fn simulate(
    mode: &str,
    returned: i32,
    delay: i32,
    now: i32,
    initial_flags: i32,
    budget: usize,
) -> Result<String, Box<dyn Error>> {
    let mut code = Vec::new();
    op(&mut code, 0x801A); // remove CE setupCall's zero argument marker
    match mode {
        "VOID" => {}
        "VALUE" | "VALUE_TIMER" => push_int(&mut code, returned),
        "TIMER" | "CANCEL" | "FAIL_NATIVE" | "FAIL_BUDGET" => {}
        _ => return Err("unsupported invented local CE action mode".into()),
    }
    match mode {
        "TIMER" | "VALUE_TIMER" | "FAIL_NATIVE" | "FAIL_BUDGET" => {
            push_int(&mut code, delay);
            push_int(&mut code, 2); // independent scheduled target
            op(&mut code, 0x8006); // CALL_AT
        }
        "CANCEL" => {
            push_int(&mut code, 2);
            op(&mut code, 0x8048); // CANCEL
        }
        _ => {}
    }
    if mode == "FAIL_NATIVE" {
        op(&mut code, 0x80FC); // unsupported original game opcode
    }
    op(
        &mut code,
        if matches!(mode, "VALUE" | "VALUE_TIMER") {
            0x8025
        } else {
            0x8021
        },
    );
    if code.len() > 42 {
        return Err("synthetic CE action crossed the 42-byte header".into());
    }
    let mut bytes = vec![0_u8; 42];
    bytes[..code.len()].copy_from_slice(&code);
    // The CE local return marker PC=20 needs an opcode at that address.
    // VALUE_TIMER places CALL_AT there; all other invented actions have
    // already ended, so supply a STOP_PROGRAM opcode at the return target.
    if code.len() <= 20 {
        bytes[20..22].copy_from_slice(&0x8011_u16.to_be_bytes());
    }
    bytes.extend_from_slice(&3_i32.to_be_bytes());
    for fields in [
        [4_i32, 0, 0, 0, 0, 0],
        [9_i32, 0, 0, 0, 0, 0],
        [15_i32, initial_flags, 77, 28, 0, 0],
    ] {
        for field in fields {
            bytes.extend_from_slice(&field.to_be_bytes());
        }
    }
    bytes.extend_from_slice(&17_i32.to_be_bytes());
    bytes.extend_from_slice(b"main\0start\0timer\0");
    bytes.extend_from_slice(&0_i32.to_be_bytes());
    let mut vm = ScriptVm::new(ScriptProgram::decode(&bytes)?);
    vm.set_clock_millis(now);
    let result = vm.execute_local_action_core_transactional_bounded(1, budget);
    match result {
        Ok(value) => {
            if vm.halted()
                || vm.pc() != 20
                || vm.return_stack() != [ScriptValue::Int(0)]
                || vm.modeled_program_low_flags() != 0
            {
                return Err("inconsistent CE local return commit".into());
            }
            let output = match value {
                Some(ScriptValue::Int(n)) => n.to_string(),
                None => "_".to_string(),
                _ => return Err("unexpected CE return value kind".into()),
            };
            if vm.stack().len() != usize::from(value.is_some()) {
                return Err("unexpected CE local return operand depth".into());
            }
            Ok(format!(
                "S:{output}:{}:{}:{}:{}:{}:{}:{}",
                vm.pc(),
                vm.stack().len(),
                vm.return_stack().len(),
                vm.runtime_procedures()[2].flags,
                vm.runtime_procedures()[2].event_time,
                vm.runtime_procedures()[2].condition_offset,
                i32::from(vm.halted()),
            ))
        }
        Err(_) => {
            if vm.pc() != 0
                || !vm.stack().is_empty()
                || !vm.return_stack().is_empty()
                || vm.halted()
                || vm.runtime_procedures()[2].flags != initial_flags
                || vm.runtime_procedures()[2].event_time != 77
                || vm.runtime_procedures()[2].condition_offset != 28
            {
                return Err("failed core action partially committed VM state".into());
            }
            Ok("ERR".to_owned())
        }
    }
}

fn main() {
    for line in include_str!("../../tests/script_core_transaction_cases.txt").lines() {
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
