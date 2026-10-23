//! Synthetic CE stop/exit flag distinctions, no game program/assets.
use rust_age::script_program::ScriptProgram;
use rust_age::script_vm::ScriptVm;
use std::error::Error;
use std::io;

fn probe(mode: &str, value: i32) -> Result<String, Box<dyn Error>> {
    let opcode = match mode {
        "EXIT" => 0x800E_u16,
        "EXIT_PROGRAM" => 0x8010,
        "STOP_PROGRAM" => 0x8011,
        _ => return Err("unsupported synthetic CE termination".into()),
    };
    let mut bytes = vec![0_u8; 42];
    bytes[..2].copy_from_slice(&0xC001_u16.to_be_bytes());
    bytes[2..6].copy_from_slice(&value.to_be_bytes());
    bytes[6..8].copy_from_slice(&opcode.to_be_bytes());
    bytes.extend_from_slice(&[0_u8; 12]);
    let vm = ScriptVm::new(ScriptProgram::decode(&bytes)?);
    let condition = vm.preview_condition_frame_with_host_bounded(0, 2, &mut |_, _| {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "CE native opcode unsupported",
        ))
    })?;
    if vm.halted() || vm.halt_reason().is_some() || vm.pc() != 0 || !vm.stack().is_empty() {
        return Err("detached CE condition mutated original".into());
    }
    Ok(format!(
        "S:{}:{}:{:?}",
        i32::from(condition.satisfied),
        condition.termination.ce_terminal_flag(),
        condition.termination,
    ))
}

fn main() {
    for line in include_str!("../../tests/script_termination_cases.txt").lines() {
        let parts: Vec<_> = line.split_whitespace().collect();
        if parts.len() != 3 {
            continue;
        }
        let result =
            (|| -> Result<String, Box<dyn Error>> { probe(parts[1], parts[2].parse()?) })()
                .unwrap_or_else(|_| "ERR".to_owned());
        println!("{} {}", parts[0], result);
    }
}
