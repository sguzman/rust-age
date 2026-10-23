//! Invented CE doEvents two-procedure test bench. No original game files.
use rust_age::script_program::ScriptProgram;
use rust_age::script_vm::ScriptVm;
use std::error::Error;

fn push(code: &mut Vec<u8>, value: i32) {
    code.extend_from_slice(&0xC001_u16.to_be_bytes());
    code.extend_from_slice(&value.to_be_bytes());
}

fn op(code: &mut Vec<u8>, opcode: u16) {
    code.extend_from_slice(&opcode.to_be_bytes());
}

fn simulate(mode: &str, operand: i32, expected: i32, now: u32) -> Result<String, Box<dyn Error>> {
    let carry = mode == "CARRY_FALSE" || mode == "CARRY_TRUE" || mode == "ERROR";
    let condition_on_second = mode == "TIMER_COND";
    if !carry && mode != "TIMER" && !condition_on_second {
        return Err("unknown CE sequential poll mode".into());
    }
    let mut first = Vec::new();
    if carry {
        push(&mut first, operand);
        push(&mut first, i32::from(mode == "CARRY_TRUE"));
        op(&mut first, 0x8011);
    } else {
        push(&mut first, operand); // signed seconds
        push(&mut first, 1); // later procedure index
        op(&mut first, 0x8006); // CALL_AT
        push(&mut first, 0); // condition is false, but timer write survives
        op(&mut first, 0x8011);
    }
    let mut second = Vec::new();
    if mode == "ERROR" {
        op(&mut second, 0x80FC); // native gameplay mutation denied
        op(&mut second, 0x8011);
    } else if carry {
        push(&mut second, expected);
        op(&mut second, 0x8033); // EQ consumes retained stack operand
        op(&mut second, 0x8011);
    } else if condition_on_second {
        push(&mut second, 0); // false branch outranks scheduled timer
        op(&mut second, 0x8011);
    } else {
        op(&mut second, 0x8011);
    }
    if first.len() > 24 || second.len() > 14 {
        return Err("CE two-procedure synthetic fixture overflow".into());
    }
    let mut bytes = vec![0_u8; 42];
    bytes[..first.len()].copy_from_slice(&first);
    bytes[28..28 + second.len()].copy_from_slice(&second);
    bytes.extend_from_slice(&2_i32.to_be_bytes());
    let second_flags = i32::from(carry || condition_on_second) * 0x02;
    for fields in [
        [4_i32, 0x02, 0, 0, 0, 0],
        [8_i32, second_flags, 0, 28, 28, 0],
    ] {
        for field in fields {
            bytes.extend_from_slice(&field.to_be_bytes());
        }
    }
    bytes.extend_from_slice(&8_i32.to_be_bytes());
    bytes.extend_from_slice(b"one\0two\0");
    bytes.extend_from_slice(&0_i32.to_be_bytes());

    let vm = ScriptVm::new(ScriptProgram::decode(&bytes)?);
    let result = vm.preview_due_procedure_poll_with_host_bounded(now, 6, &mut |_, _| {
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "CE gameplay native not permitted by condition probe",
        ))
    });
    if vm.pc() != 0
        || !vm.stack().is_empty()
        || !vm.return_stack().is_empty()
        || vm.runtime_procedures()[0].flags != 0x02
        || vm.runtime_procedures()[1].flags != second_flags
        || vm.runtime_procedures()[1].event_time != 0
    {
        return Err("original CE interpreter changed during detached poll".into());
    }
    let poll = result?;
    let mask = poll.ready.iter().fold(0_u32, |mask, index| mask | 1 << index);
    Ok(format!(
        "S:{mask}:{}:{}:{}:{}",
        poll.procedures[0].flags,
        poll.procedures[1].flags,
        poll.procedures[1].event_time,
        poll.data_stack.len(),
    ))
}

fn main() {
    for line in include_str!("../../tests/script_due_sequence_cases.txt").lines() {
        let parts: Vec<_> = line.split_whitespace().collect();
        if parts.len() != 5 {
            continue;
        }
        let result = (|| -> Result<String, Box<dyn Error>> {
            simulate(
                parts[1],
                parts[2].parse()?,
                parts[3].parse()?,
                parts[4].parse()?,
            )
        })()
        .unwrap_or_else(|_| "ERR".to_owned());
        println!("{} {}", parts[0], result);
    }
}
