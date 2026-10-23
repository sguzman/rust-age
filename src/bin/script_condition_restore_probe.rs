//! Independent invented CE doEvents flag/PC restoration comparison.
//! It loads no commercial game assets and never executes Fallout callbacks.
use rust_age::script_program::ScriptProgram;
use rust_age::script_vm::ScriptVm;
use std::error::Error;

fn op(code: &mut Vec<u8>, opcode: u16) {
    code.extend_from_slice(&opcode.to_be_bytes());
}

fn push_int(code: &mut Vec<u8>, value: i32) {
    op(code, 0xC001);
    code.extend_from_slice(&value.to_be_bytes());
}

fn simulate(
    saved_mode: &str,
    end_mode: &str,
    condition_critical: bool,
    truth: i32,
) -> Result<String, Box<dyn Error>> {
    let mut initial = Vec::new();
    match saved_mode {
        "NONE" => {}
        "CRITICAL" => op(&mut initial, 0x804A),
        "STOP" => op(&mut initial, 0x8011),
        "CRITICAL_STOP" => {
            op(&mut initial, 0x804A);
            op(&mut initial, 0x8011);
        }
        _ => return Err("invalid saved program state".into()),
    }
    let mut condition = Vec::new();
    if condition_critical {
        op(&mut condition, 0x804A);
    }
    push_int(&mut condition, truth);
    op(
        &mut condition,
        match end_mode {
            "EXIT" => 0x800E,
            "EXIT_PROGRAM" => 0x8010,
            "STOP_PROGRAM" => 0x8011,
            _ => return Err("invalid CE condition terminal opcode".into()),
        },
    );
    let mut bytes = vec![0_u8; 42];
    bytes[..initial.len()].copy_from_slice(&initial);
    bytes[20..20 + condition.len()].copy_from_slice(&condition);
    bytes.extend_from_slice(&[0_u8; 12]); // zero procedures and names
    let mut vm = ScriptVm::new(ScriptProgram::decode(&bytes)?);
    for _ in 0..initial.len() / 2 {
        vm.step()?;
    }
    let installed_flags = vm.modeled_program_low_flags();
    let installed_pc = vm.pc();
    let frame = vm.preview_condition_frame_with_host_bounded(20, 3, &mut |_, _| {
        Err("unexpected synthetic CE native".into())
    })?;
    if vm.pc() != installed_pc || vm.modeled_program_low_flags() != installed_flags {
        return Err("condition mutated installed program flags or instruction pointer".into());
    }
    let saved = frame.flag_restore;
    Ok(format!(
        "S:{}:{}:{}:{}:{}:{}:{}",
        i32::from(frame.satisfied),
        saved.saved_pc,
        saved.evaluated_pc,
        saved.restored_pc,
        saved.saved_low_flags,
        saved.evaluated_low_flags,
        saved.restored_low_flags,
    ))
}

fn main() {
    for line in include_str!("../../tests/script_condition_restore_cases.txt").lines() {
        let parts: Vec<_> = line.split_whitespace().collect();
        if parts.len() != 5 {
            continue;
        }
        let result = (|| -> Result<String, Box<dyn Error>> {
            simulate(parts[1], parts[2], parts[3] == "1", parts[4].parse()?)
        })()
        .unwrap_or_else(|_| "ERR".to_owned());
        println!("{} {}", parts[0], result);
    }
}
