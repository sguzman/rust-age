//! Independent Rust execution of invented CE native-variable opcode traces.
use rust_age::game_globals::GameGlobalState;
use rust_age::map::MapVariables;
use rust_age::script_binding::{BoundScriptRuntime, InstalledScriptBinding, ScriptBinding};
use rust_age::script_native_vars::run_variable_subset_bounded;
use rust_age::script_program::ScriptProgram;
use rust_age::script_runtime::ScriptRuntime;
use rust_age::script_vm::{ScriptValue, ScriptVm};
use std::error::Error;

fn word(code: &mut Vec<u8>, opcode: u16) {
    code.extend_from_slice(&opcode.to_be_bytes());
}
fn push(code: &mut Vec<u8>, n: i32, pointer: bool) {
    word(code, if pointer { 0xE001 } else { 0xC001 });
    code.extend_from_slice(&n.to_be_bytes());
}
fn scenario(name: &str) -> Vec<u8> {
    let mut c = Vec::new();
    match name {
        "map_plus_game" => {
            push(&mut c, 1, false);
            push(&mut c, 39, false);
            word(&mut c, 0x80C4);
            push(&mut c, 1, false);
            word(&mut c, 0x80C3);
            push(&mut c, 0, false);
            word(&mut c, 0x80C5);
            word(&mut c, 0x8039);
        }
        "game_set_read" => {
            push(&mut c, 1, false);
            push(&mut c, 99, false);
            word(&mut c, 0x80C6);
            push(&mut c, 1, false);
            word(&mut c, 0x80C5);
        }
        "local_pointer" => {
            push(&mut c, 0, false);
            push(&mut c, 123, true);
            word(&mut c, 0x80C2);
            push(&mut c, 0, false);
            word(&mut c, 0x80C1);
        }
        "map_invalid" => {
            push(&mut c, 9, false);
            word(&mut c, 0x80C3);
        }
        "local_float" => {
            push(&mut c, 1, false);
            word(&mut c, 0xA001);
            c.extend_from_slice(&1.5_f32.to_bits().to_be_bytes());
            word(&mut c, 0x80C2);
            push(&mut c, 1, false);
            word(&mut c, 0x80C1);
        }
        "mixed_add" => {
            push(&mut c, 0, false);
            word(&mut c, 0x80C3);
            push(&mut c, 1, false);
            word(&mut c, 0x80C5);
            word(&mut c, 0x8039);
        }
        "game_missing" => {
            push(&mut c, 0, false);
            word(&mut c, 0x80C5);
        }
        _ => {}
    }
    word(&mut c, 0x8011);
    c
}
fn test_state() -> Result<rust_age::script_locals::MapScriptLocals, Box<dyn Error>> {
    let program = ScriptProgram::decode(&[0_u8; 54])?;
    let mut runtime = ScriptRuntime::new();
    runtime.register(1, ScriptVm::new(program))?;
    let bound = BoundScriptRuntime {
        runtime,
        entries: vec![InstalledScriptBinding {
            binding: ScriptBinding {
                category: 1,
                ordinal: 0,
                script_id: 1,
                script_index: 0,
                owner_id: -1,
                path: Some("scripts/SYNTH.int".to_owned()),
                run_info: None,
                local_variable_offset: 0,
                local_variable_count: 2,
            },
            instance_id: Some(1),
        }],
    };
    Ok(bound.script_local_state(&MapVariables {
        globals: vec![12, -9],
        locals: vec![10, 11],
    })?)
}
fn repr(value: ScriptValue) -> String {
    match value {
        ScriptValue::Pointer(n) => format!("P:{n}"),
        ScriptValue::Int(n) => format!("I:{n}"),
        _ => "ERR".to_owned(),
    }
}
fn probe(name: &str) -> Result<String, Box<dyn Error>> {
    let code = scenario(name);
    if code.len() > 42 {
        return Err("synthetic CE script too large".into());
    }
    let mut bytes = vec![0_u8; 42];
    bytes[..code.len()].copy_from_slice(&code);
    bytes.extend_from_slice(&[0_u8; 12]);
    let mut vm = ScriptVm::new(ScriptProgram::decode(&bytes)?);
    let mut map = test_state()?;
    let values: &[i32] = if name == "game_missing" { &[] } else { &[7, 8] };
    let mut game = GameGlobalState::from_initial_values(values)?;
    let steps = run_variable_subset_bounded(&mut vm, 1, &mut map, &mut game, 16)?;
    let top = vm.stack().last().copied().map(repr).unwrap_or_else(|| "E".to_owned());
    let game_text = if game.values().is_empty() {
        "none".to_owned()
    } else {
        format!("{},{}", game.values()[0], game.values()[1])
    };
    Ok(format!(
        "{steps} PC:{} S:{top} M:{},{} L:{},{} G:{game_text}",
        vm.pc(),
        map.globals()[0],
        map.globals()[1],
        map.locals()[0],
        map.locals()[1]
    ))
}
fn main() {
    for name in include_str!("../../tests/native_bytecode_cases.txt").lines() {
        let output = probe(name).unwrap_or_else(|_| "ERR".to_owned());
        println!("{name} {output}");
    }
}
