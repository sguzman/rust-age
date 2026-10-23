//! Source-derived CE object-property numeric/bytecode parity probe.
use rust_age::script_object_read::{
    run_object_read_subset_bounded, ScriptObjectSnapshot, ELEVATION_OPCODE, OBJ_ART_FID_OPCODE,
    OBJ_PID_OPCODE, TILE_NUM_OPCODE,
};
use rust_age::script_program::ScriptProgram;
use rust_age::script_vm::{ScriptValue, ScriptVm};
use std::error::Error;

fn probe(parts: &[&str]) -> Result<String, Box<dyn Error>> {
    let opcode = match parts[1] {
        "t" => TILE_NUM_OPCODE,
        "e" => ELEVATION_OPCODE,
        "p" => OBJ_PID_OPCODE,
        "f" => OBJ_ART_FID_OPCODE,
        _ => return Err("unknown object opcode fixture".into()),
    };
    let object = ScriptObjectSnapshot {
        handle: parts[2].parse()?,
        elevation: parts[3].parse()?,
        tile: parts[4].parse()?,
        pid: parts[5].parse()?,
        fid: parts[6].parse()?,
    };
    let mut code = Vec::new();
    code.extend_from_slice(&0xE001_u16.to_be_bytes());
    code.extend_from_slice(&object.handle.to_be_bytes());
    code.extend_from_slice(&opcode.to_be_bytes());
    code.extend_from_slice(&0x8011_u16.to_be_bytes());
    let mut bytes = vec![0_u8; 42];
    bytes[..code.len()].copy_from_slice(&code);
    bytes.extend_from_slice(&[0_u8; 12]);
    let mut vm = ScriptVm::new(ScriptProgram::decode(&bytes)?);
    let objects = if object.handle == 0 {
        Vec::new()
    } else {
        vec![object]
    };
    let steps = run_object_read_subset_bounded(&objects, &mut vm, 8)?;
    match vm.stack() {
        [ScriptValue::Int(value)] if vm.halted() => {
            Ok(format!("S:{value} N:{steps} PC:{}", vm.pc()))
        }
        _ => Err("unexpected object native execution result".into()),
    }
}

fn main() {
    for line in include_str!("../../tests/script_object_read_cases.txt").lines() {
        let parts: Vec<_> = line.split_whitespace().collect();
        if parts.len() != 7 {
            continue;
        }
        let result = probe(&parts).unwrap_or_else(|_| "ERR".to_owned());
        println!("{} {result}", parts[0]);
    }
}
