//! Independent Rust half of CE script_q timer and queue ordering oracle.
use rust_age::collision::ObjectKey;
use rust_age::event_queue::EventQueue;
use rust_age::script_binding::{BoundScriptRuntime, InstalledScriptBinding, ScriptBinding};
use rust_age::script_program::ScriptProgram;
use rust_age::script_runtime::ScriptRuntime;
use rust_age::script_timer::{remove_object_timer_events, ScriptTimerEvent};
use rust_age::script_vm::ScriptVm;
use std::error::Error;

fn vm(names: &[&[u8]]) -> Result<ScriptVm, Box<dyn Error>> {
    let mut bytes = vec![0_u8; 42];
    bytes.extend_from_slice(&(names.len() as i32).to_be_bytes());
    let mut ids = Vec::new();
    for name in names {
        let offset = 4 + ids.len() as i32;
        for field in [offset, 0, 0, 0, 0, 0] {
            bytes.extend_from_slice(&field.to_be_bytes());
        }
        ids.extend_from_slice(name);
        ids.push(0);
    }
    bytes.extend_from_slice(&(ids.len() as i32).to_be_bytes());
    bytes.extend_from_slice(&ids);
    bytes.extend_from_slice(&0_i32.to_be_bytes());
    Ok(ScriptVm::new(ScriptProgram::decode(&bytes)?))
}
fn binding(id: u64, ordinal: usize, sid: i32) -> InstalledScriptBinding {
    InstalledScriptBinding {
        binding: ScriptBinding {
            category: 1,
            ordinal,
            script_id: sid,
            script_index: 0,
            owner_id: -1,
            path: Some("scripts/SYNTH.int".to_owned()),
            run_info: None,
            local_variable_offset: -1,
            local_variable_count: 0,
        },
        instance_id: Some(id),
    }
}
fn prepared() -> Result<BoundScriptRuntime, Box<dyn Error>> {
    let mut runtime = ScriptRuntime::new();
    runtime.register(1, vm(&[b"main", b"timed_event_p_proc"])?)?;
    runtime.register(2, vm(&[b"main"])?)?;
    Ok(BoundScriptRuntime {
        runtime,
        entries: vec![binding(1, 0, 500), binding(2, 1, 600)],
    })
}
fn owner(index: usize) -> ObjectKey {
    ObjectKey { elevation: 0, ordinal: index }
}
fn probe(
    label: &str, now: i32, delay: i32, sid: i32, fixed: i32, who: usize, due: i32,
) -> Result<String, Box<dyn Error>> {
    let bound = prepared()?;
    let mut queue = EventQueue::new();
    bound.queue_script_timer(&mut queue, now, delay, owner(who), sid, fixed)?;
    if label == "tie" {
        bound.queue_script_timer(&mut queue, now, delay, owner(who), sid, fixed + 1)?;
    }
    if label == "remove" {
        bound.queue_script_timer(&mut queue, now, delay, owner(who + 1), sid, fixed + 1)?;
        remove_object_timer_events(&mut queue, owner(who));
    }
    let mut popped = Vec::new();
    while let Some(entry) = queue.pop_due(due) {
        let timer = ScriptTimerEvent::decode(&entry)?;
        let action = match bound.plan_script_timer(&timer)? {
            Some(plan) => format!("{}:{}", plan.action, plan.procedure_index),
            None => "none".to_owned(),
        };
        let hex = entry.payload.iter().map(|n| format!("{n:02x}"))
            .collect::<Vec<_>>().join("");
        popped.push(format!(
            "{}:{}:{}@{}:{}",
            timer.sid, action, timer.fixed_parameter, timer.time, hex
        ));
    }
    if popped.is_empty() {
        return Ok(if queue.is_empty() {
            "NONE:0".to_owned()
        } else {
            format!("WAIT@{}:{}", queue.next_time(), queue.len())
        });
    }
    Ok(format!("{}:{}", popped.join(","), queue.len()))
}
fn main() {
    for line in include_str!("../../tests/script_timer_cases.txt").lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() != 7 {
            continue;
        }
        let result = (|| -> Result<String, Box<dyn Error>> {
            probe(
                fields[0], fields[1].parse()?, fields[2].parse()?,
                fields[3].parse()?, fields[4].parse()?,
                fields[5].parse()?, fields[6].parse()?,
            )
        })().unwrap_or_else(|_| "ERR".to_owned());
        println!("{} {}", fields[0], result);
    }
}
