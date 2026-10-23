//! Bounded synthetic INT parity against source-derived CE tile/PID lookup.
use rust_age::script_object_read::{
    run_object_read_subset_bounded, ScriptObjectSnapshot, TILE_CONTAINS_OBJ_PID_OPCODE,
};
use rust_age::script_program::ScriptProgram;
use rust_age::script_vm::{ScriptValue, ScriptVm};
use std::error::Error;

fn push(code: &mut Vec<u8>, value: i32) {
    code.extend_from_slice(&0xC001_u16.to_be_bytes());
    code.extend_from_slice(&value.to_be_bytes());
}

fn probe(fields: &[&str]) -> Result<String, Box<dyn Error>> {
    let numbers = fields[1..]
        .iter()
        .map(|value| value.parse::<i32>())
        .collect::<Result<Vec<_>, _>>()?;
    let mut code = Vec::new();
    for value in [numbers[1], numbers[0], numbers[2]] {
        push(&mut code, value);
    }
    code.extend_from_slice(&TILE_CONTAINS_OBJ_PID_OPCODE.to_be_bytes());
    code.extend_from_slice(&0x8011_u16.to_be_bytes());
    let mut bytes = vec![0_u8; 42];
    bytes[..code.len()].copy_from_slice(&code);
    bytes.extend_from_slice(&[0_u8; 12]);
    let mut vm = ScriptVm::new(ScriptProgram::decode(&bytes)?);
    let objects = [0, 1].map(|index| {
        let offset = 3 + index * 3;
        ScriptObjectSnapshot {
            handle: (index + 1) as i32,
            table_elevation: numbers[offset] as usize,
            elevation: numbers[offset] + 10, // deliberately NOT table elevation
            tile: numbers[offset + 1],
            pid: numbers[offset + 2],
            fid: 0,
        }
    });
    let steps = run_object_read_subset_bounded(&objects, &mut vm, 8)?;
    match vm.stack() {
        [ScriptValue::Int(value)] if vm.halted() => {
            Ok(format!("S:{value} N:{steps} PC:{}", vm.pc()))
        }
        _ => Err("unexpected CE tile-contains result stack".into()),
    }
}

fn main() {
    for line in include_str!("../../tests/script_tile_contains_cases.txt").lines() {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.len() != 10 {
            continue;
        }
        let result = probe(&fields).unwrap_or_else(|_| "ERR".to_owned());
        println!("{} {result}", fields[0]);
    }
}
