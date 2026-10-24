//! Source-derived CE field_78 first interpreter clock comparison.
//! The pinned CE initializes this signed sentinel BEFORE CPU budgeting.
//! Invented bytecode only; no installed Fallout assets or gameplay callbacks.
use rust_age::script_program::ScriptProgram;
use rust_age::script_runtime::ScriptRuntime;
use rust_age::script_vm::ScriptVm;
use std::error::Error;

fn scenario(mode: &str) -> Result<ScriptVm, Box<dyn Error>> {
    let code: &[u8] = match mode {
        "NOOP" => &[0x80, 0x00, 0x80, 0x11],
        "BAD" => &[0x80, 0xFC],
        _ => return Err("unrecognized CE entry clock scenario".into()),
    };
    let mut bytes = vec![0_u8; 42];
    bytes[..code.len()].copy_from_slice(code);
    bytes.extend_from_slice(&[0_u8; 12]);
    Ok(ScriptVm::new(ScriptProgram::decode(&bytes)?))
}

fn run(
    mode: &str,
    first_time: u32,
    first_burst: usize,
    next_time: u32,
    next_burst: usize,
) -> Result<String, Box<dyn Error>> {
    let mut runtime = ScriptRuntime::new();
    runtime.register(3, scenario(mode)?)?;
    let mut trace = Vec::new();
    for (now, budget) in [(first_time, first_burst), (next_time, next_burst)] {
        let outcome = runtime.advance_core_tick_transactional_bounded(now, budget);
        let vm = runtime.vm(3).ok_or("missing CE timestamp test VM")?;
        let state = match outcome {
            Ok(tick) => {
                if tick.executed_steps.len() != 1
                    || tick.executed_steps[0].0 != 3
                    || !tick.prepared_calls.is_empty()
                {
                    return Err("invalid synthetic updatePrograms boundary".into());
                }
                format!("S:{}:{}", vm.pc(), vm.first_interpret_clock_millis())
            }
            Err(_) => format!("ERR:{}:{}", vm.pc(), vm.first_interpret_clock_millis()),
        };
        trace.push(state);
    }
    Ok(trace.join("|"))
}

fn main() {
    for line in include_str!("../../tests/script_first_entry_cases.txt").lines() {
        let args: Vec<_> = line.split_whitespace().collect();
        if args.len() != 6 {
            continue;
        }
        let outcome = (|| -> Result<String, Box<dyn Error>> {
            run(
                args[1],
                args[2].parse()?,
                args[3].parse()?,
                args[4].parse()?,
                args[5].parse()?,
            )
        })()
        .unwrap_or_else(|_| "ERR".to_string());
        println!("{} {}", args[0], outcome);
    }
}
