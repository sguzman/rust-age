//! Differential probe for synthetic CE map-local integer storage.
use rust_age::map::MapVariables;
use rust_age::script_binding::{BoundScriptRuntime, InstalledScriptBinding, ScriptBinding};
use rust_age::script_list::ScriptRunInfo;
use rust_age::script_program::ScriptProgram;
use rust_age::script_runtime::ScriptRuntime;
use rust_age::script_vm::ScriptVm;
use std::error::Error;

fn record(
    cat: usize,
    id: u64,
    offset: i32,
    count: i32,
    listed: Option<i32>,
) -> InstalledScriptBinding {
    InstalledScriptBinding {
        binding: ScriptBinding {
            category: cat,
            ordinal: (id - 1) as usize,
            script_id: id as i32,
            script_index: 0,
            owner_id: -1,
            path: Some("scripts/SYNTH.int".to_owned()),
            run_info: listed.map(|local_variables| ScriptRunInfo {
                local_variables: Some(local_variables),
                ..Default::default()
            }),
            local_variable_offset: offset,
            local_variable_count: count,
        },
        instance_id: Some(id),
    }
}

fn setup() -> Result<BoundScriptRuntime, Box<dyn Error>> {
    let program = ScriptProgram::decode(&[0_u8; 54])?;
    let mut runtime = ScriptRuntime::new();
    for id in 1..=4 {
        runtime.register(id, ScriptVm::new(program.clone()))?;
    }
    Ok(BoundScriptRuntime {
        runtime,
        entries: vec![
            record(1, 1, 0, 2, Some(7)),
            record(1, 2, -1, 0, Some(3)),
            record(0, 3, -1, 0, Some(2)),
            record(1, 4, -1, 0, None),
        ],
    })
}

fn probe(op: &str, instance: u64, index: i32, value: i32) -> Result<String, Box<dyn Error>> {
    let map = MapVariables {
        globals: vec![99],
        locals: vec![10, 11],
    };
    let mut state = setup()?.script_local_state(&map)?;
    let outcome = match op {
        "r" => state
            .read(instance, index)
            .map(|v| v.map(|x| format!("I:{x}")).unwrap_or("NONE".to_owned())),
        "w" => state
            .write(instance, index, value)
            .map(|_| format!("I:{value}")),
        _ => return Err("unknown synthetic local operation".into()),
    };
    let offset = state
        .range(instance)
        .and_then(|r| r.offset)
        .map(|n| n as i32)
        .unwrap_or(-1);
    Ok(format!(
        "{} {} {}",
        outcome.unwrap_or_else(|_| "ERR".to_owned()),
        state.locals().len(),
        offset
    ))
}

fn main() {
    for line in include_str!("../../tests/map_local_cases.txt").lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() != 5 {
            continue;
        }
        let output = (|| -> Result<String, Box<dyn Error>> {
            probe(
                fields[1],
                fields[2].parse()?,
                fields[3].parse()?,
                fields[4].parse()?,
            )
        })()
        .unwrap_or_else(|_| "ERR".to_owned());
        println!("{} {}", fields[0], output);
    }
}
