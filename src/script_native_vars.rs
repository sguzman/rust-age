//! Pure, bounded adapter for the four CE native MAP variable opcodes.
//! Pinned CE src/int/support/intextra.cc op_local_var, op_set_local_var,
//! op_map_var, op_set_map_var and initIntExtra registrations.
//!
//! These routines operate on a supplied synthetic operand stack and a
//! Rust-owned MAP state copy. They are NOT installed in ScriptVm's bytecode
//! dispatcher: gameplay scripts, quest logic and host opcodes cannot run.

use crate::script_locals::MapScriptLocals;
use crate::script_vm::ScriptValue;
use std::io;

pub const LOCAL_VAR_OPCODE: u16 = 0x80C1;
pub const SET_LOCAL_VAR_OPCODE: u16 = 0x80C2;
pub const MAP_VAR_OPCODE: u16 = 0x80C3;
pub const SET_MAP_VAR_OPCODE: u16 = 0x80C4;
const MAX_STACK_VALUES: usize = 0x800;

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

/// CE uses ProgramValue's raw integer union field for non-pointer MAP writes.
/// The returned value is an integer, even when the source was a float or
/// static/dynamic string offset; no string content or memory is dereferenced.
fn stored_value(value: ScriptValue) -> ScriptValue {
    match value {
        ScriptValue::Pointer(handle) => ScriptValue::Pointer(handle),
        ScriptValue::Float(number) => ScriptValue::Int(number.to_bits() as i32),
        ScriptValue::Int(number)
        | ScriptValue::StaticString(number)
        | ScriptValue::DynamicString(number) => ScriptValue::Int(number),
    }
}

/// Apply one original CE native variable operation to a detached stack.
///
/// In CE, reads pop the index and push a typed value. Writes pop the value
/// first, then the index. Missing/out-of-range reads yield integer -1;
/// failing writes still consume operands but do not modify MAP state.
/// The adapter validates type, underflow and capacity *before* mutation,
/// preserving its stack on invalid inputs, which is a Rust safety behavior
/// rather than a claim about original CE interpreter error unwinding.
pub fn apply_map_variable_opcode(
    locals: &mut MapScriptLocals,
    instance_id: u64,
    opcode: u16,
    stack: &mut Vec<ScriptValue>,
) -> io::Result<()> {
    let write = match opcode {
        LOCAL_VAR_OPCODE | MAP_VAR_OPCODE => false,
        SET_LOCAL_VAR_OPCODE | SET_MAP_VAR_OPCODE => true,
        _ => return Err(invalid("not a supported CE MAP variable opcode")),
    };
    let needed = if write { 2 } else { 1 };
    if stack.len() < needed {
        return Err(invalid("CE MAP variable opcode operand stack underflow"));
    }
    let index_position = stack.len() - needed;
    let ScriptValue::Int(index) = stack[index_position] else {
        return Err(invalid("CE MAP variable opcode expected integer index"));
    };
    if !write && stack.len() > MAX_STACK_VALUES {
        return Err(invalid("CE MAP variable opcode stack exceeds Rust bound"));
    }
    if write {
        let value = stored_value(stack[stack.len() - 1]);
        match opcode {
            SET_LOCAL_VAR_OPCODE => {
                // CE scr_set_local_var result is ignored by op_set_local_var.
                let _ = locals.write_typed(instance_id, index, value);
            }
            SET_MAP_VAR_OPCODE => {
                // CE map_set_global_var result is ignored by op_set_map_var.
                let _ = locals.write_global_typed(index, value);
            }
            _ => unreachable!(),
        }
        stack.truncate(index_position);
    } else {
        let result = match opcode {
            LOCAL_VAR_OPCODE => locals
                .read_typed(instance_id, index)
                .ok()
                .flatten()
                .unwrap_or(ScriptValue::Int(-1)),
            MAP_VAR_OPCODE => locals
                .read_global_typed(index)
                .unwrap_or(ScriptValue::Int(-1)),
            _ => unreachable!(),
        };
        stack.pop();
        stack.push(result);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        apply_map_variable_opcode, LOCAL_VAR_OPCODE, MAP_VAR_OPCODE, SET_LOCAL_VAR_OPCODE,
        SET_MAP_VAR_OPCODE,
    };
    use crate::map::MapVariables;
    use crate::script_binding::{BoundScriptRuntime, InstalledScriptBinding, ScriptBinding};
    use crate::script_list::ScriptRunInfo;
    use crate::script_program::ScriptProgram;
    use crate::script_runtime::ScriptRuntime;
    use crate::script_vm::{ScriptValue, ScriptVm};

    fn fixture() -> crate::script_locals::MapScriptLocals {
        let mut runtime = ScriptRuntime::new();
        let program = ScriptProgram::decode(&[0_u8; 54]).unwrap();
        for id in 1..=3 {
            runtime
                .register(id, ScriptVm::new(program.clone()))
                .unwrap();
        }
        let record =
            |id: u64, category: usize, saved_offset: i32, count: i32| InstalledScriptBinding {
                binding: ScriptBinding {
                    category,
                    ordinal: id as usize - 1,
                    script_id: id as i32,
                    script_index: 0,
                    owner_id: -1,
                    path: Some("scripts/SYNTH.int".to_owned()),
                    run_info: Some(ScriptRunInfo {
                        local_variables: Some(count),
                        ..Default::default()
                    }),
                    local_variable_offset: saved_offset,
                    local_variable_count: count,
                },
                instance_id: Some(id),
            };
        BoundScriptRuntime {
            runtime,
            entries: vec![record(1, 1, 0, 2), record(2, 1, -1, 3), record(3, 0, -1, 0)],
        }
        .script_local_state(&MapVariables {
            globals: vec![12, -9],
            locals: vec![10, 11],
        })
        .unwrap()
    }

    #[test]
    fn ce_variable_opcodes_preserve_operand_order_and_pointer_tags() {
        let mut state = fixture();
        let mut stack = vec![ScriptValue::Int(1)];
        apply_map_variable_opcode(&mut state, 1, MAP_VAR_OPCODE, &mut stack).unwrap();
        assert_eq!(stack, [ScriptValue::Int(-9)]);

        stack = vec![ScriptValue::Int(1), ScriptValue::Int(44)];
        apply_map_variable_opcode(&mut state, 1, SET_MAP_VAR_OPCODE, &mut stack).unwrap();
        assert!(stack.is_empty());
        assert_eq!(state.globals(), [12, 44]);

        stack = vec![ScriptValue::Int(0), ScriptValue::Pointer(1234)];
        apply_map_variable_opcode(&mut state, 1, SET_MAP_VAR_OPCODE, &mut stack).unwrap();
        stack.push(ScriptValue::Int(0));
        apply_map_variable_opcode(&mut state, 1, MAP_VAR_OPCODE, &mut stack).unwrap();
        assert_eq!(stack, [ScriptValue::Pointer(1234)]);

        stack = vec![ScriptValue::Int(1)];
        apply_map_variable_opcode(&mut state, 1, LOCAL_VAR_OPCODE, &mut stack).unwrap();
        assert_eq!(stack, [ScriptValue::Int(11)]);

        stack = vec![ScriptValue::Int(2), ScriptValue::Pointer(-20)];
        apply_map_variable_opcode(&mut state, 2, SET_LOCAL_VAR_OPCODE, &mut stack).unwrap();
        assert!(stack.is_empty());
        stack.push(ScriptValue::Int(2));
        apply_map_variable_opcode(&mut state, 2, LOCAL_VAR_OPCODE, &mut stack).unwrap();
        assert_eq!(stack, [ScriptValue::Pointer(-20)]);
        assert_eq!(state.range(2).unwrap().offset, Some(2));
        assert_eq!(state.locals(), [10, 11, 0, 0, 0]);
    }

    #[test]
    fn ce_read_fallbacks_and_discarded_invalid_writes() {
        let mut state = fixture();
        for (opcode, instance, index) in [
            (LOCAL_VAR_OPCODE, 3, 0),
            (LOCAL_VAR_OPCODE, 99, 0),
            (LOCAL_VAR_OPCODE, 1, -1),
            (MAP_VAR_OPCODE, 1, -1),
            (MAP_VAR_OPCODE, 1, 20),
        ] {
            let mut stack = vec![ScriptValue::Int(index)];
            apply_map_variable_opcode(&mut state, instance, opcode, &mut stack).unwrap();
            assert_eq!(stack, [ScriptValue::Int(-1)]);
        }
        let mut stack = vec![ScriptValue::Int(40), ScriptValue::Int(8)];
        apply_map_variable_opcode(&mut state, 1, SET_MAP_VAR_OPCODE, &mut stack).unwrap();
        assert!(stack.is_empty());
        assert_eq!(state.globals(), [12, -9]);
        stack = vec![ScriptValue::Int(-1), ScriptValue::Int(8)];
        apply_map_variable_opcode(&mut state, 1, SET_LOCAL_VAR_OPCODE, &mut stack).unwrap();
        assert!(stack.is_empty());
        assert_eq!(state.locals(), [10, 11]);
    }

    #[test]
    fn ce_union_bits_and_zero_pointer_are_preserved() {
        let mut state = fixture();
        let mut stack = vec![ScriptValue::Int(1), ScriptValue::Float(1.0)];
        apply_map_variable_opcode(&mut state, 1, SET_MAP_VAR_OPCODE, &mut stack).unwrap();
        stack.push(ScriptValue::Int(1));
        apply_map_variable_opcode(&mut state, 1, MAP_VAR_OPCODE, &mut stack).unwrap();
        assert_eq!(stack, [ScriptValue::Int(0x3F80_0000)]);
        stack = vec![ScriptValue::Int(0), ScriptValue::Pointer(0)];
        apply_map_variable_opcode(&mut state, 1, SET_MAP_VAR_OPCODE, &mut stack).unwrap();
        stack.push(ScriptValue::Int(0));
        apply_map_variable_opcode(&mut state, 1, MAP_VAR_OPCODE, &mut stack).unwrap();
        assert_eq!(stack, [ScriptValue::Int(0)]);
    }

    #[test]
    fn rejects_bad_stack_inputs_without_consuming_them() {
        let mut state = fixture();
        for (opcode, operands) in [
            (0x80C5, vec![ScriptValue::Int(0)]),
            (SET_MAP_VAR_OPCODE, vec![ScriptValue::Int(0)]),
            (MAP_VAR_OPCODE, vec![ScriptValue::Float(1.0)]),
        ] {
            let mut stack = operands.clone();
            assert!(apply_map_variable_opcode(&mut state, 1, opcode, &mut stack).is_err());
            assert_eq!(stack, operands);
        }
    }
}
