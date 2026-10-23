//! Adapted CE script SID -> self_obj -> tile_num C++/Rust test probe.
use rust_age::game_globals::GameGlobalState;
use rust_age::map::MapVariables;
use rust_age::object_data::{ObjectHeader, ObjectRecord, ObjectUpdate};
use rust_age::object_tables::{MapObject, ObjectTables};
use rust_age::script_binding::BoundScriptRuntime;
use rust_age::script_clock::ScriptGameClock;
use rust_age::script_data::{ScriptRecord, ScriptTables};
use rust_age::script_native_vars::{
    run_invocation_with_owner_snapshot_bounded, ScriptClockContext, ScriptInvocationState,
    ScriptNativeOwnerSnapshot, ScriptNativeSnapshot,
};
use rust_age::script_object_binding::MapObjectReadBindings;
use rust_age::script_owner_binding::MapScriptOwners;
use rust_age::script_program::ScriptProgram;
use rust_age::script_runtime::ScriptRuntime;
use rust_age::script_vm::{ScriptValue, ScriptVm};
use std::error::Error;

fn probe(fields: &[&str]) -> Result<String, Box<dyn Error>> {
    let numbers = fields[1..]
        .iter()
        .map(|field| field.parse::<i32>())
        .collect::<Result<Vec<_>, _>>()?;
    let [sid, oid, object_sid, object_id, tile] = numbers.as_slice() else {
        return Err("invalid CE owner fixture fields".into());
    };
    let category = ((sid >> 24) & 0xFF) as usize;
    if !matches!(category, 3 | 4) {
        return Err("test requires a non-spatial script".into());
    }
    let mut categories = std::array::from_fn(|_| Vec::new());
    categories[category].push(ScriptRecord {
        script_id: *sid,
        next: -1,
        subtype: vec![],
        flags: 0,
        script_index: 0,
        program_token: -1,
        owner_id: *oid,
        local_variable_offset: -1,
        local_variable_count: 0,
        remaining: [0; 8],
    });
    let scripts = ScriptTables {
        categories,
        bytes_consumed: 0,
    };
    let objects = ObjectTables {
        elevations: [
            vec![MapObject {
                record: ObjectRecord {
                    header: ObjectHeader {
                        id: *object_id,
                        tile: *tile,
                        x: 0,
                        y: 0,
                        screen_x: 0,
                        screen_y: 0,
                        frame: 0,
                        rotation: 0,
                        fid: 0x0300_0001,
                        flags: 0,
                        elevation: 0,
                        pid: 0x0300_0001,
                        cid: -1,
                        light_distance: 0,
                        light_intensity: 0,
                        serialized_outline: 0,
                        script_id: *object_sid,
                        field_80: 0,
                    },
                    inventory_length: 0,
                    inventory_capacity: 0,
                    update: ObjectUpdate::Generic {
                        flags: 0,
                        subtype_fields: vec![],
                    },
                },
                inventory: vec![],
            }],
            vec![],
            vec![],
        ],
        total_objects: 1,
        bytes_consumed: 0,
    };
    let snaps = MapObjectReadBindings::from_tables(&objects)?;
    let owners = MapScriptOwners::from_map(&scripts, &objects, &snaps)?;
    let owner = owners.entry(category, 0).ok_or("missing bound script")?;
    let mut code = vec![];
    code.extend_from_slice(&0x80BC_u16.to_be_bytes());
    code.extend_from_slice(&0x80D4_u16.to_be_bytes());
    code.extend_from_slice(&0x8011_u16.to_be_bytes());
    let mut bytes = vec![0_u8; 42];
    bytes[..code.len()].copy_from_slice(&code);
    bytes.extend_from_slice(&[0_u8; 12]);
    let mut vm = ScriptVm::new(ScriptProgram::decode(&bytes)?);
    let scripts = BoundScriptRuntime {
        runtime: ScriptRuntime::new(),
        entries: vec![],
    };
    let mut locals = scripts.script_local_state(&MapVariables {
        globals: vec![],
        locals: vec![],
    })?;
    let mut globals = GameGlobalState::from_initial_values(&[])?;
    let mut invocation = ScriptInvocationState::installed();
    let steps = run_invocation_with_owner_snapshot_bounded(
        &mut vm,
        1,
        &mut locals,
        &mut globals,
        &mut invocation,
        ScriptNativeOwnerSnapshot {
            native: ScriptNativeSnapshot {
                clock: ScriptClockContext {
                    clock: ScriptGameClock { ticks: 0 },
                    last_visit_ticks: None,
                },
                objects: Some(snaps.snapshots()),
            },
            resolution: Some(owner.resolution),
        },
        6,
    )?;
    match vm.stack() {
        [ScriptValue::Int(value)] if vm.halted() => {
            Ok(format!("S:{value} N:{steps} PC:{}", vm.pc()))
        }
        _ => Err("invalid self_obj result stack".into()),
    }
}

fn main() {
    for line in include_str!("../../tests/script_self_obj_cases.txt").lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() != 6 {
            continue;
        }
        let result = probe(&fields).unwrap_or_else(|_| "ERR".to_owned());
        println!("{} {result}", fields[0]);
    }
}
