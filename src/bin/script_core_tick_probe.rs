//! Synthetic, core-only CE updatePrograms tick ordering probe.
//! It stages installed Rust VMs; it never loads or invokes Fallout assets.
use rust_age::script_program::ScriptProgram;
use rust_age::script_runtime::ScriptRuntime;
use rust_age::script_vm::ScriptVm;
use std::error::Error;

fn vm(flags: i32, time: i32, bad_start: bool) -> Result<ScriptVm, Box<dyn Error>> {
    let mut bytes = vec![0_u8; 42];
    bytes[..6].copy_from_slice(&[0x80, 0x00, 0x80, 0x00, 0x80, 0x11]);
    if bad_start {
        bytes[..2].copy_from_slice(&0x80FC_u16.to_be_bytes());
    }
    bytes[20..24].copy_from_slice(&[0x80, 0x1A, 0x80, 0x21]);
    bytes.extend_from_slice(&2_i32.to_be_bytes());
    for fields in [[4_i32, 0, 0, 0, 0, 0], [9_i32, flags, time, 0, 20, 0]] {
        for field in fields {
            bytes.extend_from_slice(&field.to_be_bytes());
        }
    }
    bytes.extend_from_slice(&11_i32.to_be_bytes());
    bytes.extend_from_slice(b"main\0start\0");
    bytes.extend_from_slice(&0_i32.to_be_bytes());
    Ok(ScriptVm::new(ScriptProgram::decode(&bytes)?))
}

fn simulate(
    flags: i32,
    time: i32,
    now: u32,
    burst: usize,
    bad_start: bool,
) -> Result<String, Box<dyn Error>> {
    let mut rt = ScriptRuntime::new();
    rt.register(1, vm(flags, time, bad_start)?)?;
    rt.register(2, vm(0, 9, false)?)?;
    let result = rt.advance_core_tick_transactional_bounded(now, burst);
    match result {
        Ok(tick) => {
            if tick.executed_steps != [(2, burst), (1, burst)] {
                return Err("CE tick interpreter order is incorrect".into());
            }
            let prepared = tick.prepared_calls.len();
            if prepared > 1
                || (prepared == 1
                    && (tick.prepared_calls[0].program_id != 1
                        || tick.prepared_calls[0].procedure_index != 1))
            {
                return Err("CE tick due event order is incorrect".into());
            }
            let vm1 = rt.vm(1).ok_or("installed CE VM1 missing")?;
            let vm2 = rt.vm(2).ok_or("installed CE VM2 missing")?;
            Ok(format!(
                "S:{}:{}:{}:{}:{}:{}:{}:{}",
                prepared,
                tick.executed_steps[0].1,
                tick.executed_steps[1].1,
                vm2.pc(),
                vm1.pc(),
                vm1.runtime_procedures()[1].flags,
                vm1.return_stack().len(),
                i32::from(vm1.halted()),
            ))
        }
        Err(_) => {
            for id in [1, 2] {
                let vm = rt.vm(id).ok_or("CE atomic tick VM missing")?;
                if vm.pc() != 0
                    || !vm.stack().is_empty()
                    || !vm.return_stack().is_empty()
                    || vm.halted()
                    || vm.runtime_procedures()[1].flags != if id == 1 { flags } else { 0 }
                {
                    return Err("failed CE core tick mutated installed VM".into());
                }
            }
            Ok("ERR".to_owned())
        }
    }
}

fn main() {
    for line in include_str!("../../tests/script_core_tick_cases.txt").lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() != 6 {
            continue;
        }
        let result = (|| -> Result<String, Box<dyn Error>> {
            simulate(
                fields[1].parse()?,
                fields[2].parse()?,
                fields[3].parse()?,
                fields[4].parse()?,
                fields[5] == "1",
            )
        })()
        .unwrap_or_else(|_| "ERR".to_owned());
        println!("{} {}", fields[0], result);
    }
}
