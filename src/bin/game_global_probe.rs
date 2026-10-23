//! Independent Rust half of CE game-global integer opcode decision checks.
use rust_age::game_globals::{GameGlobalState, GLOBAL_VAR_OPCODE, SET_GLOBAL_VAR_OPCODE};
use rust_age::script_vm::ScriptValue;
use std::error::Error;

fn display(values: &[i32]) -> String {
    if values.is_empty() {
        "(empty)".to_owned()
    } else {
        values
            .iter()
            .map(i32::to_string)
            .collect::<Vec<_>>()
            .join(",")
    }
}
fn probe(operation: &str, initial: &str, index: i32, value: i32) -> Result<String, Box<dyn Error>> {
    let values: &[i32] = if initial == "empty" {
        &[]
    } else {
        &[12, -17, 0]
    };
    let mut globals = GameGlobalState::from_initial_values(values)?;
    let mut stack = vec![ScriptValue::Int(index)];
    let opcode = if operation == "r" {
        GLOBAL_VAR_OPCODE
    } else {
        stack.push(ScriptValue::Int(value));
        SET_GLOBAL_VAR_OPCODE
    };
    globals.apply_opcode(opcode, &mut stack)?;
    let output = match stack.as_slice() {
        [] => "E".to_owned(),
        [ScriptValue::Int(value)] => format!("I:{value}"),
        _ => return Err("unexpected CE game global opcode stack".into()),
    };
    Ok(format!("{output} {}", display(globals.values())))
}
fn main() {
    for line in include_str!("../../tests/game_global_cases.txt").lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() != 5 {
            continue;
        }
        let result = (|| -> Result<String, Box<dyn Error>> {
            probe(fields[1], fields[2], fields[3].parse()?, fields[4].parse()?)
        })()
        .unwrap_or_else(|_| "ERR".to_owned());
        println!("{} {}", fields[0], result);
    }
}
