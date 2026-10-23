//! CE typed-stack numeric differential probe from synthetic bytecode.
use rust_age::script_program::ScriptProgram;
use rust_age::script_vm::{ScriptValue, ScriptVm};

fn main() {
    for line in include_str!("../../tests/int_numeric_cases.txt").lines() {
        let (name, hex) = line.split_once(' ').unwrap();
        let parsed = (|| -> Result<String, Box<dyn std::error::Error>> {
            if hex.len() % 2 != 0 {
                return Err("invalid test hex".into());
            }
            let mut bytes = Vec::new();
            for part in hex.as_bytes().chunks_exact(2) {
                bytes.push(u8::from_str_radix(std::str::from_utf8(part)?, 16)?);
            }
            if bytes.len() > 42 {
                return Err("oversized test program".into());
            }
            let mut file = vec![0_u8; 42];
            file[..bytes.len()].copy_from_slice(&bytes);
            file.extend_from_slice(&[0_u8; 12]);
            let program = ScriptProgram::decode(&file)?;
            let mut vm = ScriptVm::new(program);
            vm.run_bounded(16)?;
            if vm.stack().is_empty() {
                return Ok("EMPTY".to_owned());
            }
            Ok(vm
                .stack()
                .iter()
                .map(|value| match value {
                    ScriptValue::Int(n) => format!("I:{n}"),
                    ScriptValue::Float(n) => format!("F:{n:.6}"),
                    ScriptValue::StaticString(_)
                    | ScriptValue::DynamicString(_)
                    | ScriptValue::Pointer(_) => "UNSUPPORTED".to_owned(),
                })
                .collect::<Vec<_>>()
                .join(","))
        })();
        println!("{name} {}", parsed.unwrap_or_else(|_| "ERR".to_owned()));
    }
}
