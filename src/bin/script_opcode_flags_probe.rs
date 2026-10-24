//! Independent source-adapted CE Program::flags high-word comparison.
//! These are invented bounded .INT traces, NOT compiled CE game scripts.
use rust_age::script_program::ScriptProgram;
use rust_age::script_runtime::ScriptRuntime;
use rust_age::script_vm::ScriptVm;
use std::error::Error;

fn program(mode: &str, delay: i32) -> Result<ScriptVm, Box<dyn Error>> {
    let mut code = Vec::new();
    match mode {
        "PLAIN" => code.extend_from_slice(&[0x80, 0x00, 0x80, 0x00, 0x80, 0x11]),
        "CRIT" => code.extend_from_slice(&[0x80, 0x02, 0x80, 0x03, 0x80, 0x11]),
        "STOP" => code.extend_from_slice(&[0x80, 0x11]),
        "EXIT" => code.extend_from_slice(&[0x80, 0x0E]),
        "WAIT" => {
            code.extend_from_slice(&0xC001_u16.to_be_bytes());
            code.extend_from_slice(&delay.to_be_bytes());
            code.extend_from_slice(&[0x80, 0x47, 0x80, 0x00, 0x80, 0x11]);
        }
        "BAD" => code.extend_from_slice(&[0x80, 0x00, 0x80, 0xFC]),
        _ => return Err("unrecognized invented CE opcode trace".into()),
    }
    let mut bytes = vec![0_u8; 42];
    bytes[..code.len()].copy_from_slice(&code);
    bytes.extend_from_slice(&[0_u8; 12]);
    Ok(ScriptVm::new(ScriptProgram::decode(&bytes)?))
}

fn observe(
    mode: &str,
    start: u32,
    delay: i32,
    now: u32,
    budget: usize,
) -> Result<String, Box<dyn Error>> {
    let mut runtime = ScriptRuntime::new();
    runtime.register(7, program(mode, delay)?)?;
    let first = runtime.advance_core_tick_transactional_bounded(
        start,
        if mode == "WAIT" { 2 } else { 1 },
    )?;
    if first.executed_steps.len() != 1
        || first.executed_steps[0].0 != 7
        || !first.prepared_calls.is_empty()
    {
        return Err("unexpected synthetic first interpreter burst".into());
    }
    let before = runtime.vm(7).ok_or("missing registered CE VM")?;
    let first_pc = before.pc();
    let first_flags = before.modeled_program_flags();
    let second = runtime.advance_core_tick_transactional_bounded(now, budget);
    let after = runtime.vm(7).ok_or("missing registered CE VM")?;
    let end = match second {
        Ok(tick) => {
            if tick.executed_steps.len() != 1
                || tick.executed_steps[0].0 != 7
                || !tick.prepared_calls.is_empty()
            {
                return Err("unexpected CE opcode trace tick".into());
            }
            format!(
                "S:{}:{}:{}",
                tick.executed_steps[0].1,
                after.pc(),
                after.modeled_program_flags()
            )
        }
        Err(_) => {
            if after.pc() != first_pc || after.modeled_program_flags() != first_flags {
                return Err("CE opcode trace failure changed installed VM".into());
            }
            format!("ERR:{}:{}", after.pc(), after.modeled_program_flags())
        }
    };
    Ok(format!(
        "S:{}:{}:{}|{}",
        first.executed_steps[0].1, first_pc, first_flags, end
    ))
}

fn main() {
    for line in include_str!("../../tests/script_opcode_flags_cases.txt").lines() {
        let args: Vec<_> = line.split_whitespace().collect();
        if args.len() != 6 {
            continue;
        }
        let out = (|| -> Result<String, Box<dyn Error>> {
            observe(
                args[1],
                args[2].parse()?,
                args[3].parse()?,
                args[4].parse()?,
                args[5].parse()?,
            )
        })()
        .unwrap_or_else(|_| "ERR".to_string());
        println!("{} {}", args[0], out);
    }
}
