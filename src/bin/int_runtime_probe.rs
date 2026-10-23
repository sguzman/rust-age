//! Synthetic C++/Rust CE multi-program event traversal parity probe.
use rust_age::script_program::ScriptProgram;
use rust_age::script_runtime::ScriptRuntime;
use rust_age::script_vm::ScriptVm;
use std::error::Error;

fn make_vm(flags: i32, millis: i32, condition: i32) -> Result<ScriptVm, Box<dyn Error>> {
    let mut bytes = vec![0_u8; 42];
    bytes.extend_from_slice(&1_i32.to_be_bytes());
    for field in [4_i32, flags, millis, condition, 0, 0] {
        bytes.extend_from_slice(&field.to_be_bytes());
    }
    bytes.extend_from_slice(&5_i32.to_be_bytes());
    bytes.extend_from_slice(b"main\0");
    bytes.extend_from_slice(&0_i32.to_be_bytes());
    Ok(ScriptVm::new(ScriptProgram::decode(&bytes)?))
}

fn probe(fields: &[&str]) -> Result<String, Box<dyn Error>> {
    let mut runtime = ScriptRuntime::new();
    for instance in 0..3 {
        let start = 2 + 3 * instance;
        runtime.register(
            (instance + 1) as u64,
            make_vm(
                fields[start].parse()?,
                fields[start + 1].parse()?,
                fields[start + 2].parse()?,
            )?,
        )?;
    }
    let ready = runtime.take_due_with(fields[1].parse()?, |_, _, result| {
        Ok(match result {
            1 => Some(true),
            0 => Some(false),
            _ => None,
        })
    })?;
    if ready.is_empty() {
        return Ok("NONE".to_owned());
    }
    Ok(ready
        .iter()
        .map(|item| item.program_id.to_string())
        .collect::<Vec<_>>()
        .join(","))
}

fn main() {
    for line in include_str!("../../tests/int_runtime_cases.txt").lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() != 11 {
            continue;
        }
        let result = probe(&fields).unwrap_or_else(|_| "ERR".to_owned());
        println!("{} {}", fields[0], result);
    }
}
