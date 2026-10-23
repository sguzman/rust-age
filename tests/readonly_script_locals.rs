//! Synthetic saved script-local reads through cloned CE INT, never engine.
use rust_age::map::MapVariables;
use rust_age::script_binding::{BoundScriptRuntime, InstalledScriptBinding, ScriptBinding};
use rust_age::script_clock::ScriptGameClock;
use rust_age::script_native_vars::{
    apply_readonly_event_native_with_vars, ScriptClockContext, ScriptInvocationState,
    ScriptNativeEventSnapshot, ScriptNativeOwnerSnapshot, ScriptNativeSnapshot,
};
use rust_age::script_owner_binding::ScriptOwnerResolution;
use rust_age::script_program::ScriptProgram;
use rust_age::script_runtime::ScriptRuntime;
use rust_age::script_vm::{ScriptValue, ScriptVm};

fn bound(offset: i32) -> BoundScriptRuntime {
    let mut bytes = vec![0_u8; 42];
    // POP CE argument marker; PUSH local index 0; local_var; return value.
    let mut code = vec![0x80, 0x1A, 0xC0, 0x01];
    code.extend_from_slice(&0_i32.to_be_bytes());
    code.extend_from_slice(&[0x80, 0xC1, 0x80, 0x25]);
    bytes[20..20 + code.len()].copy_from_slice(&code);
    bytes.extend_from_slice(&1_i32.to_be_bytes());
    for field in [4_i32, 0, 0, 0, 20, 0] {
        bytes.extend_from_slice(&field.to_be_bytes());
    }
    bytes.extend_from_slice(&5_i32.to_be_bytes());
    bytes.extend_from_slice(b"main\0");
    bytes.extend_from_slice(&0_i32.to_be_bytes());
    let mut runtime = ScriptRuntime::new();
    runtime
        .register(1, ScriptVm::new(ScriptProgram::decode(&bytes).unwrap()))
        .unwrap();
    BoundScriptRuntime {
        runtime,
        entries: vec![InstalledScriptBinding {
            binding: ScriptBinding {
                category: 1,
                ordinal: 0,
                script_id: 0x0100_0001,
                script_index: 0,
                owner_id: -1,
                path: None,
                run_info: None,
                local_variable_offset: offset,
                local_variable_count: 1,
            },
            instance_id: Some(1),
        }],
    }
}

fn preview(
    bound: &BoundScriptRuntime,
    map: &rust_age::script_locals::MapScriptLocals,
) -> std::io::Result<Option<ScriptValue>> {
    let snapshot = ScriptNativeEventSnapshot {
        owner: ScriptNativeOwnerSnapshot {
            native: ScriptNativeSnapshot {
                clock: ScriptClockContext {
                    clock: ScriptGameClock { ticks: 0 },
                    last_visit_ticks: None,
                },
                objects: None,
            },
            resolution: Some(ScriptOwnerResolution::NullOwner),
        },
        source: Some(0),
        target: Some(0),
    };
    let invocation = ScriptInvocationState::installed();
    bound
        .runtime
        .vm(1)
        .unwrap()
        .preview_local_action_with_host_bounded(0, 4, &mut |opcode, stack| {
            apply_readonly_event_native_with_vars(
                snapshot,
                &invocation,
                map,
                None,
                1,
                opcode,
                stack,
            )
        })
}

#[test]
fn saved_locals_are_read_without_allocation_and_preserve_pointer_type() {
    let bound = bound(0);
    let mut map = bound
        .script_local_state(&MapVariables {
            globals: vec![],
            locals: vec![27],
        })
        .unwrap();
    assert_eq!(preview(&bound, &map).unwrap(), Some(ScriptValue::Int(27)));
    map.write_typed(1, 0, ScriptValue::Pointer(-20)).unwrap();
    assert_eq!(
        preview(&bound, &map).unwrap(),
        Some(ScriptValue::Pointer(-20))
    );
    assert_eq!(map.locals(), [0]);
    assert_eq!(map.range(1).unwrap().offset, Some(0));
    assert_eq!(bound.runtime.vm(1).unwrap().pc(), 0);
    assert!(bound.runtime.vm(1).unwrap().stack().is_empty());
    assert!(bound.runtime.vm(1).unwrap().return_stack().is_empty());
}

#[test]
fn unallocated_local_reads_fail_without_modifying_rust_map() {
    let bound = bound(-1);
    let map = bound
        .script_local_state(&MapVariables {
            globals: vec![],
            locals: vec![],
        })
        .unwrap();
    assert!(preview(&bound, &map).is_err());
    assert!(map.locals().is_empty());
    assert_eq!(map.range(1).unwrap().offset, None);
    assert_eq!(bound.runtime.vm(1).unwrap().pc(), 0);
}
