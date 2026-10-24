//! Invented bounded CE interpret critical-burst source comparison.
//! No original Fallout game assets or native gameplay host.
use rust_age::script_program::ScriptProgram;
use rust_age::script_runtime::ScriptRuntime;
use rust_age::script_vm::ScriptVm;
use std::error::Error;

fn program(mode: &str) -> Result<ScriptVm, Box<dyn Error>> {
    let bytes: &[u8] = match mode {
        "PLAIN" => &[0x80, 0x00, 0x80, 0x00, 0x80, 0x11],
        "PAIR" => &[0x80, 0x02, 0x80, 0x03, 0x80, 0x00, 0x80, 0x00, 0x80, 0x11],
        "SPAN" => &[
            0x80, 0x02, 0x80, 0x00, 0x80, 0x00, 0x80, 0x03, 0x80, 0x00, 0x80, 0x11,
        ],
        "DEEP" => &[
            0x80, 0x02, 0x80, 0x00, 0x80, 0x00, 0x80, 0x00, 0x80, 0x00, 0x80, 0x00, 0x80, 0x00,
            0x80, 0x00, 0x80, 0x03, 0x80, 0x00, 0x80, 0x11,
        ],
        "REENTER" => &[
            0x80, 0x02, 0x80, 0x03, 0x80, 0x02, 0x80, 0x03, 0x80, 0x00, 0x80, 0x11,
        ],
        "PRECRIT" => &[
            0x80, 0x02, 0x80, 0x03, 0x80, 0x00, 0x80, 0x00, 0x80, 0x00, 0x80, 0x11,
        ],
        "INFINITE" | "PRECRIT_LOOP" => &[0x80, 0x02, 0xC0, 0x01, 0, 0, 0, 2, 0x80, 0x04],
        "BAD_NATIVE" => &[0x80, 0x02, 0x80, 0xFC],
        "CRIT_STOP" => &[0x80, 0x02, 0x80, 0x11],
        "CRIT_EXIT" => &[0x80, 0x02, 0x80, 0x0E],
        "NOCRIT_BAD" => &[0x80, 0xFC],
        _ => return Err("invented CE critical mode missing".into()),
    };
    let mut file = vec![0_u8; 42];
    file[..bytes.len()].copy_from_slice(bytes);
    file.extend_from_slice(&[0_u8; 12]);
    let mut vm = ScriptVm::new(ScriptProgram::decode(&file)?);
    if mode == "PRECRIT" || mode == "PRECRIT_LOOP" {
        vm.step()?;
    }
    Ok(vm)
}

fn simulate(mode: &str, burst: usize) -> Result<String, Box<dyn Error>> {
    let vm = program(mode)?;
    let old_pc = vm.pc();
    let old_critical = vm.critical_section();
    let mut runtime = ScriptRuntime::new();
    runtime.register(1, vm)?;
    let result = runtime.advance_core_tick_transactional_bounded(10, burst);
    let installed = runtime.vm(1).ok_or("CE script VM missing")?;
    match result {
        Ok(tick) => {
            if !tick.prepared_calls.is_empty()
                || tick.executed_steps.len() != 1
                || tick.executed_steps[0].0 != 1
            {
                return Err("unexpected CE script tick effect".into());
            }
            Ok(format!(
                "S:{}:{}:{}:{}:{}",
                tick.executed_steps[0].1,
                installed.pc(),
                i32::from(installed.critical_section()),
                i32::from(installed.halted()),
                installed.stack().len()
            ))
        }
        Err(_) => {
            if installed.pc() != old_pc
                || installed.critical_section() != old_critical
                || installed.halted()
                || !installed.stack().is_empty()
                || !installed.return_stack().is_empty()
            {
                return Err("critical CE tick failure committed partial VM".into());
            }
            Ok("ERR".to_string())
        }
    }
}

fn main() {
    for line in include_str!("../../tests/script_critical_burst_cases.txt").lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() != 3 {
            continue;
        }
        let result =
            (|| -> Result<String, Box<dyn Error>> { simulate(fields[1], fields[2].parse()?) })()
                .unwrap_or_else(|_| "ERR".to_string());
        println!("{} {}", fields[0], result);
    }
}
