//! Synthetic CE interrupt stack-return differential probe, no original assets.
use rust_age::script_clock::ScriptGameClock;
use rust_age::script_program::ScriptProgram;
use rust_age::script_vm::ScriptVm;
use std::error::Error;

fn make_vm(value: i32, flags: i32, argc: i32, clock: bool) -> Result<ScriptVm, Box<dyn Error>> {
    let mut bytes = vec![0_u8; 42];
    bytes[20..22].copy_from_slice(&0x801A_u16.to_be_bytes());
    if clock {
        bytes[22..24].copy_from_slice(&0x80F6_u16.to_be_bytes());
        bytes[24..26].copy_from_slice(&0x8025_u16.to_be_bytes());
    } else {
        bytes[22..24].copy_from_slice(&0xC001_u16.to_be_bytes());
        bytes[24..28].copy_from_slice(&value.to_be_bytes());
        bytes[28..30].copy_from_slice(&0x8025_u16.to_be_bytes());
    }
    bytes.extend_from_slice(&1_i32.to_be_bytes());
    for field in [4_i32, flags, 0, 0, 20, argc] {
        bytes.extend_from_slice(&field.to_be_bytes());
    }
    bytes.extend_from_slice(&5_i32.to_be_bytes());
    bytes.extend_from_slice(b"main\0");
    bytes.extend_from_slice(&0_i32.to_be_bytes());
    Ok(ScriptVm::new(ScriptProgram::decode(&bytes)?))
}

fn probe(label: &str, flags: i32, argc: i32, value: i32, budget: usize) -> String {
    let clock = label.starts_with("clock_");
    match make_vm(value, flags, argc, clock).and_then(|vm| {
        if clock {
            let clock = ScriptGameClock { ticks: value };
            vm.preview_local_interrupt_with_host_bounded(
                0,
                budget,
                &mut |opcode, stack| clock.apply_opcode(opcode, stack),
            )
            .map_err(Into::into)
        } else {
            vm.preview_local_interrupt_bounded(0, budget)
                .map_err(Into::into)
        }
    }) {
        Ok(rust_age::script_vm::ScriptValue::Int(result)) => format!("I:{result}"),
        _ => "ERR".to_owned(),
    }
}

fn main() {
    for line in include_str!("../../tests/int_interrupt_cases.txt").lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() != 5 {
            continue;
        }
        let result = (|| -> Result<String, Box<dyn Error>> {
            Ok(probe(
                fields[0],
                fields[1].parse()?,
                fields[2].parse()?,
                fields[3].parse()?,
                fields[4].parse()?,
            ))
        })()
        .unwrap_or_else(|_| "ERR".to_owned());
        println!("{} {}", fields[0], result);
    }
}
