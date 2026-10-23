//! Synthetic CE game_time_advance ordering probe; no game events execute.
use rust_age::script_time_advance::{plan_game_time_advance_opcode, GAME_TIME_ADVANCE_OPCODE};
use rust_age::script_vm::ScriptValue;

fn main() {
    for line in include_str!("../../tests/script_time_advance_cases.txt").lines() {
        let parts: Vec<_> = line.split_whitespace().collect();
        if parts.len() != 3 {
            continue;
        }
        let output = (|| {
            let ticks = parts[1].parse::<i32>().ok()?;
            let limit = parts[2].parse::<usize>().ok()?;
            let mut stack = vec![ScriptValue::Int(ticks)];
            let plan =
                plan_game_time_advance_opcode(GAME_TIME_ADVANCE_OPCODE, &mut stack, limit).ok()?;
            if !stack.is_empty() {
                return None;
            }
            let increments = plan
                .increments_before_queue_process()
                .iter()
                .map(i32::to_string)
                .collect::<Vec<_>>()
                .join(",");
            Some(format!("Q:{} D:{increments}", plan.queue_polls()))
        })()
        .unwrap_or_else(|| "ERR".to_owned());
        println!("{} {output}", parts[0]);
    }
}
