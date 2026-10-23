//! Synthetic installed MAP -> scripts.lst -> INT -> self_obj integration.
//! No original game data or playable event callbacks.
use rust_age::assets::GameAssets;
use rust_age::collision::CollisionIndex;
use rust_age::map::{MapHeader, MapSquares, MapVariables};
use rust_age::object_data::{ObjectHeader, ObjectRecord, ObjectUpdate};
use rust_age::object_tables::{MapObject, ObjectTables};
use rust_age::scene::StaticScene;
use rust_age::script_data::{ScriptRecord, ScriptTables};
use rust_age::script_native_vars::{
    run_invocation_with_owner_snapshot_bounded, ScriptClockContext, ScriptInvocationState,
    ScriptNativeOwnerSnapshot, ScriptNativeSnapshot,
};
use rust_age::script_owner_binding::ScriptOwnerResolution;
use rust_age::script_vm::ScriptValue;
use rust_age::script_clock::ScriptGameClock;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

fn sample_scene() -> StaticScene {
    let mut categories = std::array::from_fn(|_| Vec::new());
    categories[3].push(ScriptRecord {
        script_id: 0x0300_0001,
        next: -1,
        subtype: vec![],
        flags: 0,
        script_index: 0,
        program_token: -1,
        owner_id: 44,
        local_variable_offset: -1,
        local_variable_count: 0,
        remaining: [0; 8],
    });
    let objects = ObjectTables {
        elevations: [
            vec![MapObject {
                record: ObjectRecord {
                    header: ObjectHeader {
                        id: 44,
                        tile: 20123,
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
                        script_id: 0x0300_0001,
                        field_80: -1,
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
    let collision = CollisionIndex::from_map(&objects);
    StaticScene {
        header: MapHeader {
            name: [0; 16],
            entering_tile: 20123,
            entering_elevation: 0,
            entering_rotation: 0,
            local_variable_count: 0,
            script_index: -1,
            flags: 0,
            darkness: 0,
            global_variable_count: 0,
            map_number: 0,
            last_visit_time: 0,
        },
        variables: MapVariables {
            globals: vec![],
            locals: vec![],
        },
        squares: MapSquares {
            elevations: [None, None, None],
            scripts_offset: 0,
        },
        scripts: ScriptTables {
            categories,
            bytes_consumed: 0,
        },
        objects,
        collision,
        bytes_consumed: 0,
    }
}

#[test]
fn installed_int_stays_unstarted_then_reads_serialized_owner_safely() {
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let root = std::env::temp_dir().join(format!(
        "rust-age-owner-integration-{}-{stamp}",
        std::process::id()
    ));
    let scripts_dir = root.join("data").join("scripts");
    fs::create_dir_all(&scripts_dir).unwrap();
    fs::write(root.join("master.dat"), [0_u8; 16]).unwrap();
    fs::write(root.join("critter.dat"), [0_u8; 16]).unwrap();
    fs::write(scripts_dir.join("scripts.lst"), b"OWNER.ssl\n").unwrap();
    // Original-format invented INT: self_obj; tile_num; STOP.
    let mut program = vec![0_u8; 54];
    program[0..2].copy_from_slice(&0x80BC_u16.to_be_bytes());
    program[2..4].copy_from_slice(&0x80D4_u16.to_be_bytes());
    program[4..6].copy_from_slice(&0x8011_u16.to_be_bytes());
    fs::write(scripts_dir.join("OWNER.int"), program).unwrap();

    let mut assets = GameAssets::open(&root).unwrap();
    let original = sample_scene();
    let initial_objects = original.objects.clone();
    let mut environment = original.unstarted_script_environment(&mut assets).unwrap();
    assert_eq!(environment.scripts.runtime.len(), 1);
    let owner = environment.owner_for_instance(1).unwrap();
    assert!(matches!(
        owner,
        ScriptOwnerResolution::Bound {
            handle: 1,
            saved_oid_matches: true,
            ..
        }
    ));
    assert_eq!(environment.owner_for_instance(999), None);
    assert_eq!(environment.scripts.runtime.vm(1).unwrap().pc(), 0);
    assert!(environment.scripts.runtime.vm(1).unwrap().stack().is_empty());

    let mut globals = environment
        .game_globals
        .take()
        .unwrap_or_else(|| rust_age::game_globals::GameGlobalState::from_initial_values(&[]).unwrap());
    let mut invocation = ScriptInvocationState::installed();
    let vm = environment.scripts.runtime.vm_mut(1).unwrap();
    assert_eq!(
        run_invocation_with_owner_snapshot_bounded(
            vm,
            1,
            &mut environment.map_locals,
            &mut globals,
            &mut invocation,
            ScriptNativeOwnerSnapshot {
                native: ScriptNativeSnapshot {
                    clock: ScriptClockContext {
                        clock: ScriptGameClock { ticks: 0 },
                        last_visit_ticks: Some(environment.map_last_visit_ticks),
                    },
                    objects: Some(environment.object_queries.snapshots()),
                },
                resolution: Some(owner),
            },
            8,
        )
        .unwrap(),
        3
    );
    assert_eq!(vm.stack(), [ScriptValue::Int(20123)]);
    assert!(vm.halted());
    assert_eq!(original.objects, initial_objects);
    fs::remove_dir_all(root).unwrap();
}
