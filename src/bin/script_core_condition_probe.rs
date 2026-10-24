//! Invented, source-adapted C++/Rust CE conditional tick comparisons.
//! No compiled upstream Fallout CE or commercial game assets.
use rust_age::script_program::ScriptProgram;
use rust_age::script_runtime::ScriptRuntime;
use rust_age::script_vm::ScriptVm;
use std::error::Error;

fn build(flags: i32, result: i32, bad: bool) -> Result<ScriptVm, Box<dyn Error>> {
    let mut bytes = vec![0_u8; 42];
    bytes[..4].copy_from_slice(&[0x80, 0x00, 0x80, 0x00]);
    bytes[20..22].copy_from_slice(
        &(if bad { 0x80FC_u16 } else { 0xC001_u16 }).to_be_bytes(),
    );
    bytes[22..26].copy_from_slice(&result.to_be_bytes());
    bytes[26..28].copy_from_slice(&0x8011_u16.to_be_bytes());
    bytes[36..40].copy_from_slice(&[0x80, 0x1A, 0x80, 0x21]);
    bytes.extend_from_slice(&2_i32.to_be_bytes());
    for fields in [[4_i32, 0, 0, 0, 0, 0], [9_i32, flags, 10, 20, 36, 0]] {
        for field in fields {
            bytes.extend_from_slice(&field.to_be_bytes());
        }
    }
    bytes.extend_from_slice(&11_i32.to_be_bytes());
    bytes.extend_from_slice(b"main\0start\0");
    bytes.extend_from_slice(&0_i32.to_be_bytes());
    Ok(ScriptVm::new(ScriptProgram::decode(&bytes)?))
}

fn inspect(
    mode: &str,
    flags: i32,
    result: i32,
    now: u32,
    burst: usize,
    condition_budget: usize,
) -> Result<String, Box<dyn Error>> {
    let mut runtime = ScriptRuntime::new();
    runtime.register(7, build(flags, result, mode == "BAD")?)?;
    let tick = runtime.advance_core_tick_with_conditions_transactional_bounded(
        now,
        burst,
        condition_budget,
    );
    let vm = runtime.vm(7).ok_or("CE candidate instance missing")?;
    match tick {
        Ok(t) => {
            if t.executed_steps.len() != 1 || t.executed_steps[0].0 != 7 {
                return Err("unexpected CE per-program core burst".into());
            }
            Ok(format!(
                "S:{}:{}:{}:{}:{}:{}:{}",
                t.executed_steps[0].1,
                vm.pc(),
                vm.runtime_procedures()[1].flags,
                vm.stack().len(),
                vm.return_stack().len(),
                vm.first_interpret_clock_millis(),
                t.prepared_calls.len()
            ))
        }
        Err(_) => {
            if vm.pc() != 0
                || vm.runtime_procedures()[1].flags != flags
                || !vm.stack().is_empty()
                || !vm.return_stack().is_empty()
                || vm.first_interpret_clock_millis() != -1
            {
                return Err("bounded CE condition failure changed original VM".into());
            }
            Ok(format!("ERR:0:{flags}:0:0:-1:0"))
        }
    }
}

fn main() {
    for line in include_str!("../../tests/script_core_condition_cases.txt").lines() {
        let a: Vec<_> = line.split_whitespace().collect();
        if a.len() != 7 {
            continue;
        }
        let out = (|| -> Result<String, Box<dyn Error>> {
            inspect(
                a[1],
                a[2].parse()?,
                a[3].parse()?,
                a[4].parse()?,
                a[5].parse()?,
                a[6].parse()?,
            )
        })()
        .unwrap_or_else(|_| "ERR".to_string());
        println!("{} {}", a[0], out);
    }
}
