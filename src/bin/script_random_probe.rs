//! Rust synthetic CE op_random/op_roll_dice comparison probe.
use rust_age::script_random::{ScriptRandomHost, RANDOM_OPCODE, ROLL_DICE_OPCODE};
use rust_age::script_vm::ScriptValue;
use std::error::Error;

fn probe(
    opcode: &str,
    seed: i32,
    low: i32,
    high: i32,
    vcr: bool,
    count: usize,
) -> Result<String, Box<dyn Error>> {
    let mut host = ScriptRandomHost::from_seed(seed, vcr);
    let action = match opcode {
        "R" => RANDOM_OPCODE,
        "D" => ROLL_DICE_OPCODE,
        _ => return Err("unknown CE native random test".into()),
    };
    let mut result = Vec::new();
    for _ in 0..count {
        let mut stack = vec![ScriptValue::Int(low), ScriptValue::Int(high)];
        host.apply_opcode(action, &mut stack)?;
        match stack.as_slice() {
            [ScriptValue::Int(value)] => result.push(value.to_string()),
            _ => return Err("invalid CE random result stack".into()),
        }
    }
    let next = host.rng.between(1, 100)?;
    Ok(format!("{} NEXT:{next}", result.join(",")))
}
fn main() {
    for line in include_str!("../../tests/script_random_cases.txt").lines() {
        let parts: Vec<_> = line.split_whitespace().collect();
        if parts.len() != 7 {
            continue;
        }
        let answer = (|| -> Result<String, Box<dyn Error>> {
            probe(
                parts[1],
                parts[2].parse()?,
                parts[3].parse()?,
                parts[4].parse()?,
                parts[5] == "1",
                parts[6].parse()?,
            )
        })()
        .unwrap_or_else(|_| "ERR".to_owned());
        println!("{} {}", parts[0], answer);
    }
}
