//! Invented source-adapted CALL_AT/WHEN/CANCEL within the same CE core tick.
//! Does not load game assets or call native Fallout engine services.
use rust_age::script_program::ScriptProgram;
use rust_age::script_runtime::ScriptRuntime;
use rust_age::script_vm::ScriptVm;
use std::error::Error;

fn fixture(mode: &str, seconds: i32) -> Result<ScriptVm, Box<dyn Error>> {
    let mut bytes = vec![0_u8; 42];
    if mode == "CALLAT" || mode == "WHEN" {
        bytes[0..2].copy_from_slice(&0xC001_u16.to_be_bytes());
        bytes[2..6].copy_from_slice(&if mode == "WHEN" { 20 } else { seconds }.to_be_bytes());
        bytes[6..8].copy_from_slice(&0xC001_u16.to_be_bytes());
        bytes[8..12].copy_from_slice(&1_i32.to_be_bytes());
        bytes[12..14].copy_from_slice(
            &if mode == "WHEN" {
                0x8007_u16
            } else {
                0x8006_u16
            }
            .to_be_bytes(),
        );
        bytes[14..16].copy_from_slice(&0x8011_u16.to_be_bytes());
    } else if mode == "CANCEL" {
        bytes[0..2].copy_from_slice(&0xC001_u16.to_be_bytes());
        bytes[2..6].copy_from_slice(&1_i32.to_be_bytes());
        bytes[6..8].copy_from_slice(&0x8048_u16.to_be_bytes());
        bytes[8..10].copy_from_slice(&0x8011_u16.to_be_bytes());
    } else {
        return Err("unsupported synthetic tick opcode mode".into());
    }
    bytes[20..24].copy_from_slice(&[0x80, 0x1A, 0x80, 0x21]);
    bytes.extend_from_slice(&2_i32.to_be_bytes());
    for fields in [
        [4_i32, 0, 0, 0, 0, 0],
        [9_i32, i32::from(mode == "CANCEL"), 0, 0, 20, 0],
    ] {
        for field in fields {
            bytes.extend_from_slice(&field.to_be_bytes());
        }
    }
    bytes.extend_from_slice(&11_i32.to_be_bytes());
    bytes.extend_from_slice(b"main\0start\0");
    bytes.extend_from_slice(&0_i32.to_be_bytes());
    Ok(ScriptVm::new(ScriptProgram::decode(&bytes)?))
}

fn simulate(mode: &str, seconds: i32, now: u32) -> Result<String, Box<dyn Error>> {
    let mut runtime = ScriptRuntime::new();
    runtime.register(7, fixture(mode, seconds)?)?;
    let burst = if mode == "CANCEL" { 2 } else { 3 };
    let result = runtime.advance_core_tick_transactional_bounded(now, burst);
    let vm = runtime
        .vm(7)
        .ok_or("installed CE synthetic script missing")?;
    match result {
        Ok(tick) => {
            if tick.executed_steps != [(7, burst)]
                || tick.prepared_calls.len() > 1
                || (tick.prepared_calls.len() == 1
                    && (tick.prepared_calls[0].program_id != 7
                        || tick.prepared_calls[0].procedure_index != 1))
            {
                return Err("unexpected CE timer scan order".into());
            }
            Ok(format!(
                "S:{}:{}:{}:{}:{}:{}:{}",
                burst,
                tick.prepared_calls.len(),
                vm.pc(),
                vm.runtime_procedures()[1].flags,
                vm.runtime_procedures()[1].event_time,
                vm.return_stack().len(),
                vm.stack().len(),
            ))
        }
        Err(_) => {
            let initial_flags = i32::from(mode == "CANCEL");
            if vm.pc() != 0
                || !vm.stack().is_empty()
                || !vm.return_stack().is_empty()
                || vm.runtime_procedures()[1].flags != initial_flags
                || vm.runtime_procedures()[1].event_time != 0
            {
                return Err("CE conditional fallback partially committed script VM".into());
            }
            Ok("ERR".to_owned())
        }
    }
}

fn main() {
    for line in include_str!("../../tests/script_same_tick_schedule_cases.txt").lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() != 4 {
            continue;
        }
        let result = (|| -> Result<String, Box<dyn Error>> {
            simulate(fields[1], fields[2].parse()?, fields[3].parse()?)
        })()
        .unwrap_or_else(|_| "ERR".to_owned());
        println!("{} {}", fields[0], result);
    }
}
