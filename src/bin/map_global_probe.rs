//! Independent CE MAP-global read/write differential probe, no game data.
use rust_age::map::MapVariables;
use rust_age::script_binding::BoundScriptRuntime;
use rust_age::script_runtime::ScriptRuntime;
use rust_age::script_vm::ScriptValue;
use std::error::Error;

fn probe(action: &str, index: i32, value: i32) -> Result<String, Box<dyn Error>> {
    let bound = BoundScriptRuntime {
        runtime: ScriptRuntime::new(),
        entries: Vec::new(),
    };
    let map = MapVariables {
        globals: vec![12, -9],
        locals: Vec::new(),
    };
    let mut state = bound.script_local_state(&map)?;
    let result = (|| -> Result<ScriptValue, Box<dyn Error>> {
        if action == "w" {
            state.write_global(index, value)?;
        } else if action == "p" {
            state.write_global_typed(index, ScriptValue::Pointer(value))?;
        }
        Ok(state.read_global_typed(index)?)
    })();
    let answer = match result {
        Ok(ScriptValue::Int(number)) => format!("I:{number}"),
        Ok(ScriptValue::Pointer(number)) => format!("P:{number}"),
        _ => "ERR".to_owned(),
    };
    Ok(format!(
        "{answer} {} {}",
        state.globals()[0],
        state.globals()[1]
    ))
}

fn main() {
    for line in include_str!("../../tests/map_global_cases.txt").lines() {
        let parts: Vec<_> = line.split_whitespace().collect();
        if parts.len() != 4 {
            continue;
        }
        let result = (|| -> Result<String, Box<dyn Error>> {
            probe(parts[1], parts[2].parse()?, parts[3].parse()?)
        })()
        .unwrap_or_else(|_| "ERR".to_owned());
        println!("{} {}", parts[0], result);
    }
}
