//! Read-only differential probe for CE's named-procedure lookup opcode.
//! Uses fabricated .INT bytes and never runs an installed game script.
use rust_age::script_program::ScriptProgram;
use rust_age::script_vm::{ScriptValue, ScriptVm};
use std::error::Error;

fn make_program(names: &str, query: &str) -> Result<ScriptProgram, Box<dyn Error>> {
    let procedures: Vec<&str> = if names == "-" {
        Vec::new()
    } else {
        names.split(',').collect()
    };
    let mut data = vec![0_u8; 42];
    data[0..2].copy_from_slice(&0x9001_u16.to_be_bytes());
    data[2..6].copy_from_slice(&0_i32.to_be_bytes());
    data[6..8].copy_from_slice(&0x8028_u16.to_be_bytes());
    data[8..10].copy_from_slice(&0x8011_u16.to_be_bytes());

    data.extend_from_slice(&i32::try_from(procedures.len())?.to_be_bytes());
    let mut identifiers = Vec::new();
    for name in procedures {
        let offset = i32::try_from(4 + identifiers.len())?;
        for value in [offset, 0, 0, 0, 18, 0] {
            data.extend_from_slice(&value.to_be_bytes());
        }
        identifiers.extend_from_slice(name.as_bytes());
        identifiers.push(0);
    }
    data.extend_from_slice(&i32::try_from(identifiers.len())?.to_be_bytes());
    data.extend_from_slice(&identifiers);
    data.extend_from_slice(&i32::try_from(query.len() + 1)?.to_be_bytes());
    data.extend_from_slice(query.as_bytes());
    data.push(0);
    Ok(ScriptProgram::decode(&data)?)
}

fn probe(names: &str, query: &str) -> Result<String, Box<dyn Error>> {
    let program = make_program(names, query)?;
    let mut vm = ScriptVm::new(program);
    vm.run_bounded(4)?;
    match vm.stack() {
        [ScriptValue::Int(index)] => Ok(format!("I:{index}")),
        _ => Err("CE named-procedure lookup returned non-index".into()),
    }
}

fn main() {
    for line in include_str!("../../tests/int_named_cases.txt").lines() {
        let mut parts = line.split_whitespace();
        let Some(label) = parts.next() else { continue };
        let Some(names) = parts.next() else { continue };
        let Some(query) = parts.next() else { continue };
        let result = probe(names, query).unwrap_or_else(|_| "ERR".to_owned());
        println!("{label} {result}");
    }
}
