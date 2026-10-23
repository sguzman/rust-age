//! Synthetic CE MAP lifecycle ordering oracle. No game assets are accessed.
use rust_age::script_binding::{BoundScriptRuntime, InstalledScriptBinding, ScriptBinding};
use rust_age::script_lifecycle::MapLifecyclePhase;
use rust_age::script_program::ScriptProgram;
use rust_age::script_runtime::ScriptRuntime;
use rust_age::script_vm::ScriptVm;
use std::error::Error;

fn make_vm(names: &[&[u8]]) -> Result<ScriptVm, Box<dyn Error>> {
    let mut bytes = vec![0_u8; 42];
    bytes.extend_from_slice(&(names.len() as i32).to_be_bytes());
    let mut identifiers = Vec::new();
    for name in names {
        let offset = 4 + identifiers.len() as i32;
        for field in [offset, 0, 0, 0, 0, 0] {
            bytes.extend_from_slice(&field.to_be_bytes());
        }
        identifiers.extend_from_slice(name);
        identifiers.push(0);
    }
    bytes.extend_from_slice(&(identifiers.len() as i32).to_be_bytes());
    bytes.extend_from_slice(&identifiers);
    bytes.extend_from_slice(&0_i32.to_be_bytes());
    Ok(ScriptVm::new(ScriptProgram::decode(&bytes)?))
}
fn binding(cat: usize, ordinal: usize, sid: i32, id: Option<u64>) -> InstalledScriptBinding {
    InstalledScriptBinding {
        binding: ScriptBinding {
            category: cat,
            ordinal,
            script_id: sid,
            script_index: 0,
            owner_id: -1,
            path: Some("scripts/SYNTH.int".to_owned()),
            run_info: None,
            local_variable_offset: -1,
            local_variable_count: 0,
        },
        instance_id: id,
    }
}
fn setup() -> Result<BoundScriptRuntime, Box<dyn Error>> {
    let mut runtime = ScriptRuntime::new();
    runtime.register(1, make_vm(&[
        b"main", b"start", b"map_update_p_proc", b"map_exit_p_proc",
    ])?)?;
    runtime.register(2, make_vm(&[
        b"main", b"start", b"map_enter_p_proc", b"map_update_p_proc",
    ])?)?;
    runtime.register(4, make_vm(&[b"main", b"start", b"MAP_ENTER_P_PROC"])?)?;
    Ok(BoundScriptRuntime {
        runtime,
        entries: vec![
            binding(0, 0, 500, Some(1)),
            binding(0, 1, 501, Some(2)),
            binding(1, 0, 601, None),
            binding(1, 1, 602, Some(4)),
        ],
    })
}
fn probe(phase: &str, sid: i32, flags: i32) -> Result<String, Box<dyn Error>> {
    let stage = match phase {
        "start" => MapLifecyclePhase::Start,
        "enter" => MapLifecyclePhase::Enter,
        "update" => MapLifecyclePhase::Update,
        "exit" => MapLifecyclePhase::Exit,
        _ => return Err("unknown CE MAP lifecycle phase".into()),
    };
    let planned = setup()?.plan_map_lifecycle(stage, sid, flags)?;
    if planned.is_empty() {
        return Ok("NONE".to_owned());
    }
    Ok(planned
        .iter()
        .map(|p| format!("{}:{}:{}:{}", p.script_id, p.action, p.procedure_index,
            p.external_parameter.unwrap_or(-1)))
        .collect::<Vec<_>>()
        .join(","))
}
fn main() {
    for line in include_str!("../../tests/map_lifecycle_cases.txt").lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() != 4 {
            continue;
        }
        let output = (|| -> Result<String, Box<dyn Error>> {
            probe(fields[1], fields[2].parse()?, fields[3].parse()?)
        })()
        .unwrap_or_else(|_| "ERR".to_owned());
        println!("{} {}", fields[0], output);
    }
}
