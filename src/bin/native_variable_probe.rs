//! Source-separated Rust half of synthetic CE native variable opcode checks.
use rust_age::map::MapVariables;
use rust_age::script_binding::{BoundScriptRuntime, InstalledScriptBinding, ScriptBinding};
use rust_age::script_list::ScriptRunInfo;
use rust_age::script_native_vars::{
    apply_map_variable_opcode, LOCAL_VAR_OPCODE, MAP_VAR_OPCODE, SET_LOCAL_VAR_OPCODE,
    SET_MAP_VAR_OPCODE,
};
use rust_age::script_program::ScriptProgram;
use rust_age::script_runtime::ScriptRuntime;
use rust_age::script_vm::{ScriptValue, ScriptVm};
use std::error::Error;

fn fixture() -> Result<rust_age::script_locals::MapScriptLocals, Box<dyn Error>> {
    let mut runtime = ScriptRuntime::new();
    let program = ScriptProgram::decode(&[0_u8; 54])?;
    for id in 1..=4 {
        runtime.register(id, ScriptVm::new(program.clone()))?;
    }
    let record = |id: u64, category: usize, offset: i32, count: i32| {
        InstalledScriptBinding {
            binding: ScriptBinding {
                category,
                ordinal: id as usize - 1,
                script_id: id as i32,
                script_index: 0,
                owner_id: -1,
                path: Some("scripts/SYNTH.int".to_owned()),
                run_info: Some(ScriptRunInfo {
                    local_variables: Some(count),
                    ..Default::default()
                }),
                local_variable_offset: offset,
                local_variable_count: count,
            },
            instance_id: Some(id),
        }
    };
    let bound = BoundScriptRuntime {
        runtime,
        entries: vec![
            record(1, 1, 0, 2),
            record(2, 1, -1, 3),
            record(3, 0, -1, 0),
            record(4, 1, -1, 0),
        ],
    };
    Ok(bound.script_local_state(&MapVariables {
        globals: vec![12, -9],
        locals: vec![10, 11],
    })?)
}
fn repr(value: ScriptValue) -> String {
    match value {
        ScriptValue::Int(value) => format!("I:{value}"),
        ScriptValue::Pointer(value) => format!("P:{value}"),
        _ => "ERROR".to_owned(),
    }
}
fn numbers(values: &[i32]) -> String {
    values
        .iter()
        .map(i32::to_string)
        .collect::<Vec<_>>()
        .join(",")
}
fn probe(
    op: &str,
    instance: u64,
    index: i32,
    kind: &str,
    raw: i32,
) -> Result<String, Box<dyn Error>> {
    let mut state = fixture()?;
    let (opcode, setter) = match op {
        "lr" => (LOCAL_VAR_OPCODE, false),
        "lw" => (SET_LOCAL_VAR_OPCODE, true),
        "mr" => (MAP_VAR_OPCODE, false),
        "mw" => (SET_MAP_VAR_OPCODE, true),
        _ => return Err("unknown synthetic opcode".into()),
    };
    let mut stack = vec![ScriptValue::Int(index)];
    if setter {
        let value = match kind {
            "i" => ScriptValue::Int(raw),
            "p" => ScriptValue::Pointer(raw),
            "f" => ScriptValue::Float(f32::from_bits(raw as u32)),
            "s" => ScriptValue::StaticString(raw),
            "d" => ScriptValue::DynamicString(raw),
            _ => return Err("unknown synthetic source value type".into()),
        };
        stack.push(value);
    }
    apply_map_variable_opcode(&mut state, instance, opcode, &mut stack)?;
    let top = stack.last().copied().map(repr).unwrap_or_else(|| "E".to_owned());
    let peek = if setter {
        let looked_up = if op == "lw" {
            state.read_typed(instance, index).ok().flatten()
        } else {
            state.read_global_typed(index).ok()
        };
        looked_up.map(repr).unwrap_or_else(|| "N".to_owned())
    } else {
        "N".to_owned()
    };
    Ok(format!(
        "{top} {peek} G:{} L:{}",
        numbers(state.globals()),
        numbers(state.locals())
    ))
}
fn main() {
    for line in include_str!("../../tests/native_variable_cases.txt").lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() != 6 {
            continue;
        }
        let result = (|| -> Result<String, Box<dyn Error>> {
            probe(
                fields[1],
                fields[2].parse()?,
                fields[3].parse()?,
                fields[4],
                fields[5].parse()?,
            )
        })()
        .unwrap_or_else(|_| "ERR".to_owned());
        println!("{} {}", fields[0], result);
    }
}
