//! Independent synthetic CE op_wait/checkWait comparison driver.
//! Source baseline: pinned Fallout 1 CE src/int/intrpret.cc.
//! No original commercial scripts, gameplay callbacks, or external waits.
use rust_age::script_program::ScriptProgram;
use rust_age::script_runtime::ScriptRuntime;
use rust_age::script_vm::ScriptVm;
use std::error::Error;

fn fixture(mode: &str, delay: i32) -> Result<ScriptVm, Box<dyn Error>> {
    let after = match mode {
        "NOOP" => 0x8000_u16,
        "BAD" => 0x80FC_u16,
        _ => return Err("unknown synthetic WAIT mode".into()),
    };
    let mut bytes = vec![0_u8; 42];
    bytes[0..2].copy_from_slice(&0xC001_u16.to_be_bytes());
    bytes[2..6].copy_from_slice(&delay.to_be_bytes());
    bytes[6..8].copy_from_slice(&0x8047_u16.to_be_bytes());
    bytes[8..10].copy_from_slice(&after.to_be_bytes());
    bytes[10..12].copy_from_slice(&0x8011_u16.to_be_bytes());
    bytes.extend_from_slice(&[0_u8; 12]);
    Ok(ScriptVm::new(ScriptProgram::decode(&bytes)?))
}

fn compare(
    mode: &str,
    start: u32,
    duration: i32,
    next_time: u32,
    burst: usize,
) -> Result<String, Box<dyn Error>> {
    let mut runtime = ScriptRuntime::new();
    runtime.register(1, fixture(mode, duration)?)?;
    let first = runtime.advance_core_tick_transactional_bounded(start, 2)?;
    if first.executed_steps != [(1, 2)] || !first.prepared_calls.is_empty() {
        return Err("invalid source-adapted WAIT first tick".into());
    }
    let before = runtime.vm(1).ok_or("synthetic WAIT VM absent")?;
    let (wait_start, wait_end) = before.core_wait_state().ok_or("WAIT not armed")?;
    if before.pc() != 8 || wait_start != start || !before.stack().is_empty() {
        return Err("unexpected synthetic CE WAIT state".into());
    }
    let second = runtime.advance_core_tick_transactional_bounded(next_time, burst);
    let after = runtime.vm(1).ok_or("synthetic WAIT VM missing")?;
    let tail = match second {
        Ok(tick) => {
            if tick.executed_steps.len() != 1
                || tick.executed_steps[0].0 != 1
                || !tick.prepared_calls.is_empty()
            {
                return Err("unexpected CE WAIT event dispatch".into());
            }
            format!(
                "S:{}:{}:{}:{}",
                tick.executed_steps[0].1,
                after.pc(),
                i32::from(after.core_wait_state().is_some()),
                i32::from(after.halted())
            )
        }
        Err(_) => {
            if after.pc() != 8
                || after.core_wait_state() != Some((wait_start, wait_end))
                || after.halted()
                || !after.stack().is_empty()
            {
                return Err("failed WAIT tick partially committed".into());
            }
            "ERR:8:1:0".to_string()
        }
    };
    Ok(format!("S:2:8:{wait_start}:{wait_end}|{tail}"))
}

fn main() {
    for line in include_str!("../../tests/script_wait_cases.txt").lines() {
        let args: Vec<_> = line.split_whitespace().collect();
        if args.len() != 6 {
            continue;
        }
        let result = (|| -> Result<String, Box<dyn Error>> {
            compare(
                args[1],
                args[2].parse()?,
                args[3].parse()?,
                args[4].parse()?,
                args[5].parse()?,
            )
        })()
        .unwrap_or_else(|_| "ERR".to_string());
        println!("{} {}", args[0], result);
    }
}
