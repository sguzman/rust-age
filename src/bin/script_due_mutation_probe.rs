//! Synthetic, source-adapted CE doEvents condition scheduling mutations.
//! No compiled Fallout program or commercial game data is accessed.
use rust_age::script_program::ScriptProgram;
use rust_age::script_vm::ScriptVm;
use std::error::Error;
use std::io;

fn push(code: &mut Vec<u8>, value: i32) {
    code.extend_from_slice(&0xC001_u16.to_be_bytes());
    code.extend_from_slice(&value.to_be_bytes());
}

fn op(code: &mut Vec<u8>, opcode: u16) {
    code.extend_from_slice(&opcode.to_be_bytes());
}

fn simulate(
    mode: &str,
    operand: i32,
    first_truth: i32,
    now: u32,
) -> Result<String, Box<dyn Error>> {
    let call_when = matches!(mode, "WHEN" | "WHEN_TIMED" | "WHEN_BAD");
    let cancel = matches!(mode, "CANCEL" | "CANCEL_BAD");
    let cancel_all = mode == "CANCEL_ALL";
    if !call_when && !cancel && !cancel_all {
        return Err("unsupported synthetic CE scheduling mutation".into());
    }
    let mut first = Vec::new();
    if call_when {
        push(&mut first, if mode == "WHEN_BAD" { operand } else { 28 });
        push(&mut first, 1);
        op(&mut first, 0x8007); // CE CALL_WHEN
        push(&mut first, 0);
    } else {
        if cancel {
            push(&mut first, if mode == "CANCEL_BAD" { operand } else { 1 });
            op(&mut first, 0x8048); // CE CANCEL
        } else {
            op(&mut first, 0x8049); // CE CANCEL_ALL
        }
        push(&mut first, first_truth);
    }
    op(&mut first, 0x8011);
    let mut second = Vec::new();
    push(&mut second, if call_when { operand } else { 1 });
    op(&mut second, 0x8011);
    if first.len() > 24 || second.len() > 14 {
        return Err("synthetic condition body overlaps procedure table".into());
    }
    let initial_flags = if call_when {
        i32::from(mode == "WHEN_TIMED")
    } else {
        0x02
    };
    let initial_condition_offset = if call_when { 0 } else { 28 };
    let mut bytes = vec![0_u8; 42];
    bytes[..first.len()].copy_from_slice(&first);
    bytes[28..28 + second.len()].copy_from_slice(&second);
    bytes.extend_from_slice(&2_i32.to_be_bytes());
    for fields in [
        [4_i32, 0x02, 0, 0, 0, 0],
        [8_i32, initial_flags, 0, initial_condition_offset, 28, 0],
    ] {
        for field in fields {
            bytes.extend_from_slice(&field.to_be_bytes());
        }
    }
    bytes.extend_from_slice(&8_i32.to_be_bytes());
    bytes.extend_from_slice(b"one\0two\0");
    bytes.extend_from_slice(&0_i32.to_be_bytes());
    let vm = ScriptVm::new(ScriptProgram::decode(&bytes)?);
    let poll = vm.preview_due_procedure_poll_with_host_bounded(now, 6, &mut |_, _| {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "original CE game-native operations are not installed",
        ))
    });
    if vm.pc() != 0
        || !vm.stack().is_empty()
        || !vm.return_stack().is_empty()
        || vm.runtime_procedures()[0].flags != 0x02
        || vm.runtime_procedures()[1].flags != initial_flags
        || vm.runtime_procedures()[1].condition_offset != initial_condition_offset
        || vm.runtime_procedures()[1].event_time != 0
    {
        return Err("detached CE event poll modified original interpreter".into());
    }
    let poll = poll?;
    let ready_mask = poll
        .ready
        .iter()
        .fold(0_u32, |mask, index| mask | (1 << index));
    Ok(format!(
        "S:{ready_mask}:{}:{}:{}:{}:{}",
        poll.procedures[0].flags,
        poll.procedures[1].flags,
        poll.procedures[1].event_time,
        poll.procedures[1].condition_offset,
        poll.data_stack.len(),
    ))
}

fn main() {
    for line in include_str!("../../tests/script_due_mutation_cases.txt").lines() {
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
