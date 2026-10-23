//! Synthetic CE string/numeric/pointer comparison differential probe.
//! Does not load or execute Fallout game scripts.
use rust_age::script_program::ScriptProgram;
use rust_age::script_vm::{ScriptValue, ScriptVm};
use std::error::Error;

fn push_operand(
    code: &mut Vec<u8>,
    strings: &mut Vec<u8>,
    kind: &str,
    value: &str,
) -> Result<(), Box<dyn Error>> {
    let (op, raw) = match kind {
        "S" => {
            let offset = i32::try_from(strings.len())?;
            strings.extend_from_slice(value.as_bytes());
            strings.push(0);
            (0x9001_u16, offset)
        }
        "I" => (0xC001_u16, value.parse::<i32>()?),
        "F" => (0xA001_u16, value.parse::<f32>()?.to_bits() as i32),
        "P" => (0xE001_u16, value.parse::<i32>()?),
        _ => return Err("unknown CE operand type".into()),
    };
    code.extend_from_slice(&op.to_be_bytes());
    code.extend_from_slice(&raw.to_be_bytes());
    Ok(())
}

fn probe(fields: &[&str]) -> Result<String, Box<dyn Error>> {
    let mut code = Vec::new();
    let mut strings = Vec::new();
    push_operand(&mut code, &mut strings, fields[1], fields[2])?;
    push_operand(&mut code, &mut strings, fields[3], fields[4])?;
    let opcode = match fields[5] {
        "eq" => 0x8033_u16,
        "ne" => 0x8034,
        "le" => 0x8035,
        "ge" => 0x8036,
        "lt" => 0x8037,
        "gt" => 0x8038,
        _ => return Err("invalid CE comparison".into()),
    };
    code.extend_from_slice(&opcode.to_be_bytes());
    code.extend_from_slice(&0x8011_u16.to_be_bytes());
    let mut bytes = vec![0_u8; 42];
    bytes[..code.len()].copy_from_slice(&code);
    bytes.extend_from_slice(&0_i32.to_be_bytes()); // zero procedures
    bytes.extend_from_slice(&0_i32.to_be_bytes()); // zero identifiers
    bytes.extend_from_slice(&i32::try_from(strings.len())?.to_be_bytes());
    bytes.extend_from_slice(&strings);
    let mut vm = ScriptVm::new(ScriptProgram::decode(&bytes)?);
    vm.run_bounded(8)?;
    match vm.stack() {
        [ScriptValue::Int(n)] => Ok(format!("I:{n}")),
        _ => Err("comparison returned unexpected type".into()),
    }
}

fn main() {
    for line in include_str!("../../tests/int_string_cases.txt").lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() != 6 {
            continue;
        }
        let result = probe(&fields).unwrap_or_else(|_| "ERR".to_owned());
        println!("{} {}", fields[0], result);
    }
}
