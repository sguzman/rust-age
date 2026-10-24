//! Source-adapted, synthetic CE timed executeProc/setupCall preparation.
//! No original Fallout script content or engine-native host is loaded.
use rust_age::script_program::ScriptProgram;
use rust_age::script_vm::{ScriptValue, ScriptVm};
use std::error::Error;

fn simulate(
    flags: i32,
    event_time: i32,
    now: u32,
    valid_body: bool,
) -> Result<String, Box<dyn Error>> {
    let mut bytes = vec![0_u8; 42];
    if valid_body {
        bytes[20..24].copy_from_slice(&[0x80, 0x1A, 0x80, 0x21]);
    }
    bytes.extend_from_slice(&2_i32.to_be_bytes());
    for fields in [[4_i32, 0, 0, 0, 0, 0], [9_i32, flags, event_time, 0, 20, 0]] {
        for field in fields {
            bytes.extend_from_slice(&field.to_be_bytes());
        }
    }
    bytes.extend_from_slice(&11_i32.to_be_bytes());
    bytes.extend_from_slice(b"main\0start\0");
    bytes.extend_from_slice(&0_i32.to_be_bytes());
    let mut vm = ScriptVm::new(ScriptProgram::decode(&bytes)?);
    let outcome = vm.prepare_due_timed_local_core_transactional(1, now);
    let procedure = vm.runtime_procedures()[1];
    match outcome {
        Ok(prepared) => {
            if prepared {
                if vm.pc() != 20
                    || vm.halted()
                    || vm.critical_section()
                    || vm.stack()
                        != [
                            ScriptValue::Int(0),
                            ScriptValue::Pointer(0),
                            ScriptValue::Int(0),
                            ScriptValue::Int(0),
                        ]
                    || vm.return_stack() != [ScriptValue::Int(0), ScriptValue::Int(20)]
                {
                    return Err("invalid staged CE local call frame".into());
                }
                Ok(format!(
                    "S:{}:{}:{}:{}:{}:{}",
                    vm.pc(),
                    vm.stack().len(),
                    vm.return_stack().len(),
                    procedure.flags,
                    procedure.event_time,
                    i32::from(vm.critical_section()),
                ))
            } else {
                if vm.pc() != 0 || !vm.stack().is_empty() || !vm.return_stack().is_empty() {
                    return Err("not-due CE local call mutated installed VM".into());
                }
                Ok(format!(
                    "N:{}:{}:{}:{}:{}:{}",
                    vm.pc(),
                    vm.stack().len(),
                    vm.return_stack().len(),
                    procedure.flags,
                    procedure.event_time,
                    i32::from(vm.critical_section()),
                ))
            }
        }
        Err(_) => {
            if vm.pc() != 0
                || !vm.stack().is_empty()
                || !vm.return_stack().is_empty()
                || procedure.flags != flags
                || procedure.event_time != event_time
                || vm.halted()
            {
                return Err("failed CE timed frame partially committed VM".into());
            }
            Ok("ERR".to_owned())
        }
    }
}

fn main() {
    for line in include_str!("../../tests/script_due_prepare_cases.txt").lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() != 5 {
            continue;
        }
        let result = (|| -> Result<String, Box<dyn Error>> {
            simulate(
                fields[1].parse()?,
                fields[2].parse()?,
                fields[3].parse()?,
                fields[4] == "1",
            )
        })()
        .unwrap_or_else(|_| "ERR".to_owned());
        println!("{} {}", fields[0], result);
    }
}
