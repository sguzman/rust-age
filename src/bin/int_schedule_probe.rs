//! Synthetic differential probe for CE's procedure scheduling opcodes.
//! No game files, external scripts or callbacks are ever executed.
use rust_age::script_program::ScriptProgram;
use rust_age::script_vm::ScriptVm;
use std::error::Error;

fn push_int(code: &mut Vec<u8>, value: i32) {
    code.extend_from_slice(&0xC001_u16.to_be_bytes());
    code.extend_from_slice(&value.to_be_bytes());
}

fn opcode(code: &mut Vec<u8>, op: u16) {
    code.extend_from_slice(&op.to_be_bytes());
}

fn make_program(code: &[u8], flags: i32) -> Result<ScriptProgram, Box<dyn Error>> {
    if code.len() > 42 {
        return Err("synthetic procedure exceeds CE preamble".into());
    }
    let mut data = vec![0_u8; 42];
    data[..code.len()].copy_from_slice(code);
    data.extend_from_slice(&1_i32.to_be_bytes());
    for value in [4_i32, flags, 0, 0, 20, 0] {
        data.extend_from_slice(&value.to_be_bytes());
    }
    data.extend_from_slice(&5_i32.to_be_bytes());
    data.extend_from_slice(b"main\0");
    data.extend_from_slice(&0_i32.to_be_bytes());
    Ok(ScriptProgram::decode(&data)?)
}

fn run(operation: &str, flags: i32, first: i32, second: i32, now: i32)
    -> Result<String, Box<dyn Error>>
{
    let mut code = Vec::new();
    if matches!(operation, "at" | "both" | "cancel") {
        push_int(&mut code, first); // seconds before procedure index
        push_int(&mut code, 0);
        opcode(&mut code, 0x8006);
    }
    if matches!(operation, "when" | "both") {
        push_int(&mut code, if operation == "both" { second } else { first });
        push_int(&mut code, 0);
        opcode(&mut code, 0x8007);
    }
    if operation == "cancel" {
        push_int(&mut code, 0);
        opcode(&mut code, 0x8048);
    }
    if operation == "all" {
        opcode(&mut code, 0x8049);
    }
    opcode(&mut code, 0x8011);
    let mut vm = ScriptVm::new(make_program(&code, flags)?);
    vm.set_clock_millis(now);
    vm.run_bounded(16)?;
    let proc = vm.runtime_procedures()[0];
    Ok(format!("{},{},{}", proc.flags, proc.event_time, proc.condition_offset))
}

fn main() {
    for line in include_str!("../../tests/int_schedule_cases.txt").lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() != 6 {
            continue;
        }
        let result = (|| -> Result<String, Box<dyn Error>> {
            run(
                fields[1],
                fields[2].parse()?,
                fields[3].parse()?,
                fields[4].parse()?,
                fields[5].parse()?,
            )
        })();
        println!(
            "{} {}",
            fields[0],
            result.unwrap_or_else(|_| "ERR".to_owned())
        );
    }
}
