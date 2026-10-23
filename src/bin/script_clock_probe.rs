//! Independent Rust synthetic CE game-time native opcode differential probe.
use rust_age::script_clock::{
    ScriptGameClock, GAME_TICKS_OPCODE, GAME_TIME_HOUR_OPCODE, GAME_TIME_IN_SECONDS_OPCODE,
    GAME_TIME_OPCODE,
};
use rust_age::script_program::ScriptProgram;
use rust_age::script_vm::{ScriptValue, ScriptVm};
use std::error::Error;

fn push(code: &mut Vec<u8>, value: i32) {
    code.extend_from_slice(&0xC001_u16.to_be_bytes());
    code.extend_from_slice(&value.to_be_bytes());
}
fn word(code: &mut Vec<u8>, opcode: u16) {
    code.extend_from_slice(&opcode.to_be_bytes());
}
fn probe(kind: &str, ticks: i32, seconds: i32) -> Result<String, Box<dyn Error>> {
    let mut code = Vec::new();
    match kind {
        "t" => word(&mut code, GAME_TIME_OPCODE),
        "s" => word(&mut code, GAME_TIME_IN_SECONDS_OPCODE),
        "h" => word(&mut code, GAME_TIME_HOUR_OPCODE),
        "k" => {
            push(&mut code, seconds);
            word(&mut code, GAME_TICKS_OPCODE);
        }
        "c" => {
            word(&mut code, GAME_TIME_IN_SECONDS_OPCODE);
            push(&mut code, seconds);
            word(&mut code, GAME_TICKS_OPCODE);
            word(&mut code, 0x8039);
        }
        _ => return Err("unknown source-clock test".into()),
    };
    word(&mut code, 0x8011);
    let mut bytes = vec![0_u8; 42];
    bytes[..code.len()].copy_from_slice(&code);
    bytes.extend_from_slice(&[0_u8; 12]);
    let mut vm = ScriptVm::new(ScriptProgram::decode(&bytes)?);
    let steps = ScriptGameClock { ticks }.run_bounded(&mut vm, 9)?;
    match vm.stack() {
        [ScriptValue::Int(value)] => Ok(format!("S:{value} N:{steps} PC:{}", vm.pc())),
        _ => Err("unexpected CE time result stack".into()),
    }
}
fn main() {
    for line in include_str!("../../tests/script_clock_cases.txt").lines() {
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
