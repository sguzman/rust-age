//! Pinned CE local executeProc frame adapted C++/Rust fixture probe.
use rust_age::script_program::ScriptProgram;
use rust_age::script_runtime::ScriptRuntime;
use rust_age::script_vm::ScriptVm;
use std::error::Error;

fn probe(parts: &[&str]) -> Result<String, Box<dyn Error>> {
    let numbers = parts[1..]
        .iter()
        .map(|s| s.parse::<i32>())
        .collect::<Result<Vec<_>, _>>()?;
    let [flags, argc, body, opcode] = numbers.as_slice() else {
        return Err("wrong local CE frame field count".into());
    };
    let opcode = u16::try_from(*opcode)?;
    let mut bytes = vec![0_u8; 42];
    bytes[20..22].copy_from_slice(&opcode.to_be_bytes());
    bytes.extend_from_slice(&2_i32.to_be_bytes());
    for record in [[4_i32, 0, 0, 0, 0, 0], [9_i32, *flags, 0, 0, *body, *argc]] {
        for field in record {
            bytes.extend_from_slice(&field.to_be_bytes());
        }
    }
    bytes.extend_from_slice(&11_i32.to_be_bytes());
    bytes.extend_from_slice(b"main\0start\0");
    bytes.extend_from_slice(&0_i32.to_be_bytes());
    let mut runtime = ScriptRuntime::new();
    runtime.register(1, ScriptVm::new(ScriptProgram::decode(&bytes)?))?;
    let frame = runtime
        .inspect_local_action_frame(1, 1)?
        .ok_or("missing CE start action")?;
    if frame.saved_pc != 0 || frame.procedure_index != 1 {
        return Err("unexpected original VM location".into());
    }
    Ok(format!(
        "S:{},{},{},{}",
        frame.body_offset,
        frame.return_marker,
        frame.argument_marker,
        i32::from(frame.critical)
    ))
}

fn main() {
    for line in include_str!("../../tests/script_local_frame_cases.txt").lines() {
        let parts: Vec<_> = line.split_whitespace().collect();
        if parts.len() != 5 {
            continue;
        }
        println!(
            "{} {}",
            parts[0],
            probe(&parts).unwrap_or_else(|_| "ERR".to_owned())
        );
    }
}
