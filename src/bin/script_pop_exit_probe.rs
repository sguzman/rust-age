//! Source-adapted, synthetic CE POP_EXIT comparison without game files.
use rust_age::script_program::ScriptProgram;
use rust_age::script_vm::{ScriptHaltReason, ScriptVm};
use std::error::Error;

fn push_int(code: &mut Vec<u8>, value: i32) {
    code.extend_from_slice(&0xC001_u16.to_be_bytes());
    code.extend_from_slice(&value.to_be_bytes());
}

fn op(code: &mut Vec<u8>, opcode: u16) {
    code.extend_from_slice(&opcode.to_be_bytes());
}

fn simulate(
    mode: &str,
    destination: i32,
    value: i32,
    critical: bool,
) -> Result<String, Box<dyn Error>> {
    let mut code = Vec::new();
    if critical {
        op(&mut code, 0x804A); // CE START_CRITICAL
    }
    push_int(&mut code, destination);
    op(&mut code, 0x800D); // D_TO_A
    push_int(&mut code, value);
    op(&mut code, 0x801D); // POP_EXIT
    let mut bytes = vec![0_u8; 42];
    bytes[..code.len()].copy_from_slice(&code);
    bytes.extend_from_slice(&[0_u8; 12]);
    let vm = ScriptVm::new(ScriptProgram::decode(&bytes)?);
    let (pc, evaluated_low, restored_low, stack_len, truth) = match mode {
        "RUN" => {
            let mut scratch = vm.clone();
            scratch.run_bounded(4 + usize::from(critical))?;
            if scratch.halt_reason() != Some(ScriptHaltReason::ReturnFromCall) {
                return Err("incorrect CE POP_EXIT reason".into());
            }
            (
                scratch.pc(),
                scratch.modeled_program_low_flags(),
                scratch.modeled_program_low_flags(),
                scratch.stack().len(),
                i32::from(scratch.stack().last().is_some_and(|_| value != 0)),
            )
        }
        "CONDITION" => {
            let candidate = vm.preview_condition_frame_with_host_bounded(
                0,
                4 + usize::from(critical),
                &mut |_, _| Err(std::io::Error::other("unexpected native opcode")),
            )?;
            if candidate.termination != ScriptHaltReason::ReturnFromCall {
                return Err("incorrect CE conditional POP_EXIT reason".into());
            }
            (
                candidate.flag_restore.evaluated_pc,
                candidate.flag_restore.evaluated_low_flags,
                candidate.flag_restore.restored_low_flags,
                candidate.data_stack.len(),
                i32::from(candidate.satisfied),
            )
        }
        _ => return Err("invalid CE POP_EXIT fixture mode".into()),
    };
    if vm.pc() != 0 || vm.halted() || !vm.stack().is_empty() || !vm.return_stack().is_empty() {
        return Err("source VM was mutated by detached POP_EXIT evaluation".into());
    }
    Ok(format!(
        "S:{pc}:{evaluated_low}:{restored_low}:{stack_len}:{truth}"
    ))
}

fn main() {
    for line in include_str!("../../tests/script_pop_exit_cases.txt").lines() {
        let parts: Vec<_> = line.split_whitespace().collect();
        if parts.len() != 5 {
            continue;
        }
        let output = (|| -> Result<String, Box<dyn Error>> {
            simulate(
                parts[1],
                parts[2].parse()?,
                parts[3].parse()?,
                parts[4] == "1",
            )
        })()
        .unwrap_or_else(|_| "ERR".to_owned());
        println!("{} {}", parts[0], output);
    }
}
