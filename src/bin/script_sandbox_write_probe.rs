//! Independent, invented CE local INT sandbox-write parity probe.
use rust_age::game_globals::GameGlobalState;
use rust_age::map::MapVariables;
use rust_age::object_tables::ObjectTables;
use rust_age::scene::PreparedScriptEnvironment;
use rust_age::script_binding::{BoundScriptRuntime, InstalledScriptBinding, ScriptBinding};
use rust_age::script_clock::ScriptGameClock;
use rust_age::script_data::{ScriptRecord, ScriptTables};
use rust_age::script_lifecycle::MapLifecyclePhase;
use rust_age::script_native_vars::ScriptClockContext;
use rust_age::script_object_binding::MapObjectReadBindings;
use rust_age::script_owner_binding::MapScriptOwners;
use rust_age::script_program::ScriptProgram;
use rust_age::script_runtime::ScriptRuntime;
use rust_age::script_vm::ScriptVm;
use std::error::Error;

fn probe(parts: &[&str]) -> Result<String, Box<dyn Error>> {
    let n = parts[1..]
        .iter()
        .map(|value| value.parse::<i32>())
        .collect::<Result<Vec<_>, _>>()?;
    let [target, map_count, game_count, index, value, map0, map1, game0, game1, override_flag] =
        n.as_slice()
    else {
        return Err("unexpected sandbox oracle field count".into());
    };
    let map_count = usize::try_from(*map_count)?;
    let game_count = usize::try_from(*game_count)?;
    let map_values = [*map0, *map1][..map_count].to_vec();
    let game_values = [*game0, *game1][..game_count].to_vec();
    let mut code = vec![0x80, 0x1A, 0xC0, 0x01];
    code.extend_from_slice(&index.to_be_bytes());
    code.extend_from_slice(&0xC001_u16.to_be_bytes());
    code.extend_from_slice(&value.to_be_bytes());
    let write_opcode = if *target == 0 { 0x80C4_u16 } else { 0x80C6_u16 };
    code.extend_from_slice(&write_opcode.to_be_bytes());
    if *override_flag != 0 {
        code.extend_from_slice(&0x80B9_u16.to_be_bytes());
    }
    code.extend_from_slice(&0x8021_u16.to_be_bytes());
    let mut bytes = vec![0_u8; 42];
    bytes[20..20 + code.len()].copy_from_slice(&code);
    bytes.extend_from_slice(&2_i32.to_be_bytes());
    let mut names = Vec::new();
    for name in [b"main".as_slice(), b"map_enter_p_proc".as_slice()] {
        let offset = 4 + names.len() as i32;
        for field in [offset, 0, 0, 0, 20, 0] {
            bytes.extend_from_slice(&field.to_be_bytes());
        }
        names.extend_from_slice(name);
        names.push(0);
    }
    bytes.extend_from_slice(&(names.len() as i32).to_be_bytes());
    bytes.extend_from_slice(&names);
    bytes.extend_from_slice(&0_i32.to_be_bytes());

    let mut runtime = ScriptRuntime::new();
    runtime.register(1, ScriptVm::new(ScriptProgram::decode(&bytes)?))?;
    let binding = BoundScriptRuntime {
        runtime,
        entries: vec![InstalledScriptBinding {
            binding: ScriptBinding {
                category: 0,
                ordinal: 0,
                script_id: 17,
                script_index: 0,
                owner_id: -1,
                path: None,
                run_info: None,
                local_variable_offset: -1,
                local_variable_count: 0,
            },
            instance_id: Some(1),
        }],
    };
    let map_locals = binding.script_local_state(&MapVariables {
        globals: map_values.clone(),
        locals: vec![],
    })?;
    let mut categories = std::array::from_fn(|_| Vec::new());
    categories[0].push(ScriptRecord {
        script_id: 17,
        next: -1,
        subtype: vec![],
        flags: 0,
        script_index: 0,
        program_token: -1,
        owner_id: -1,
        local_variable_offset: -1,
        local_variable_count: 0,
        remaining: [0; 8],
    });
    let scripts = ScriptTables {
        categories,
        bytes_consumed: 0,
    };
    let objects = ObjectTables {
        elevations: [vec![], vec![], vec![]],
        total_objects: 0,
        bytes_consumed: 0,
    };
    let object_queries = MapObjectReadBindings::from_tables(&objects)?;
    let script_owners = MapScriptOwners::from_map(&scripts, &objects, &object_queries)?;
    let environment = PreparedScriptEnvironment {
        scripts: binding,
        map_locals,
        game_globals: Some(GameGlobalState::from_initial_values(&game_values)?),
        object_queries,
        script_owners,
        map_last_visit_ticks: 0,
    };
    let planned = environment
        .scripts
        .plan_map_lifecycle(MapLifecyclePhase::Enter, 999, 0)?[0];
    let sandbox = environment.preview_planned_sandbox_bounded(
        &planned,
        ScriptClockContext {
            clock: ScriptGameClock { ticks: 0 },
            last_visit_ticks: None,
        },
        8,
    )?;
    if sandbox.returned.is_some() {
        return Err("sandbox write unexpectedly returned value".into());
    }
    if environment.map_locals.globals() != map_values.as_slice()
        || environment
            .game_globals
            .as_ref()
            .ok_or("missing baseline globals")?
            .values()
            != game_values.as_slice()
        || environment.scripts.runtime.vm(1).ok_or("missing VM")?.pc() != 0
    {
        return Err("sandbox mutated original installed CE source state".into());
    }
    let map0_out = sandbox
        .map_locals
        .globals()
        .first()
        .copied()
        .unwrap_or(*map0);
    let map1_out = sandbox
        .map_locals
        .globals()
        .get(1)
        .copied()
        .unwrap_or(*map1);
    let game = sandbox
        .game_globals
        .as_ref()
        .ok_or("missing sandbox globals")?;
    let game0_out = game.values().first().copied().unwrap_or(*game0);
    let game1_out = game.values().get(1).copied().unwrap_or(*game1);
    let effects = sandbox.discard()?;
    Ok(format!(
        "M:{map0_out},{map1_out} G:{game0_out},{game1_out} D:{},{} O:{}",
        effects.map.map_globals_changed,
        effects.quest_globals_changed,
        i32::from(effects.overrides)
    ))
}

fn main() {
    for line in include_str!("../../tests/script_sandbox_write_cases.txt").lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() != 11 {
            continue;
        }
        let outcome = probe(&fields).unwrap_or_else(|_| "ERR".to_owned());
        println!("{} {}", fields[0], outcome);
    }
}
