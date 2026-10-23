//! Explicit-host, bounded adapter for CE native script-state opcodes.
//! Pinned CE src/int/support/intextra.cc op_{local,map,global}_var,
//! op_set_{local,map,global}_var, op_script_action and initIntExtra.
//!
//! An isolated Rust ScriptVm can run this subset only through the explicit
//! host pathway. Ordinary ScriptVm::step and normal MAP loading remain inert.

use crate::game_globals::GameGlobalState;
use crate::script_locals::MapScriptLocals;
use crate::script_vm::{ScriptValue, ScriptVm};
use std::io;

pub const LOCAL_VAR_OPCODE: u16 = 0x80C1;
pub const SET_LOCAL_VAR_OPCODE: u16 = 0x80C2;
pub const MAP_VAR_OPCODE: u16 = 0x80C3;
pub const SET_MAP_VAR_OPCODE: u16 = 0x80C4;
pub const SCRIPT_ACTION_OPCODE: u16 = 0x80C7;
pub const SCRIPT_OVERRIDES_OPCODE: u16 = 0x80B9;
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

/// Explicitly execute the supported CE variable-opcode subset in an owned
/// diagnostic VM, with caller-supplied mutable MAP and game-global state.
/// This is not part of StaticScene loading, event dispatch or gameplay.
/// Unsupported native/game opcodes still fail before arbitrary host effects.
pub fn run_variable_subset_bounded(
    vm: &mut ScriptVm,
    instance_id: u64,
    locals: &mut MapScriptLocals,
    globals: &mut GameGlobalState,
    max_steps: usize,
) -> io::Result<usize> {
    vm.run_variable_subset_bounded(max_steps, &mut |opcode, stack| match opcode {
        LOCAL_VAR_OPCODE | SET_LOCAL_VAR_OPCODE | MAP_VAR_OPCODE | SET_MAP_VAR_OPCODE => {
            apply_map_variable_opcode(locals, instance_id, opcode, stack)
        }
        game_opcode @ (0x80C5 | 0x80C6) => globals.apply_opcode(game_opcode, stack),
        _ => Err(invalid("unrecognized CE native variable opcode")),
    })
}

/// Detached CE script.action / scriptOverrides for one known script SID.
/// Original exec_script_proc resets overrides at the start of every action.
/// These fields do not dispatch gameplay, execute real scripts, or
/// suppress a game's default behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptInvocationState {
    present: bool,
    action: i32,
    overrides: bool,
}

impl ScriptInvocationState {
    pub fn installed() -> Self {
        Self { present: true, action: 0, overrides: false }
    }

    pub fn missing() -> Self {
        Self { present: false, action: 0, overrides: false }
    }

    /// CE exec_script_proc returns -1 when the SID cannot be found.
    pub fn begin_action(&mut self, action: i32) -> bool {
        if !self.present {
            return false;
        }
        self.action = action;
        self.overrides = false;
        true
    }

    pub fn action(&self) -> i32 {
        self.action
    }

    pub fn overrides(&self) -> bool {
        self.overrides
    }

    fn mark_overrides(&mut self) {
        if self.present {
            self.overrides = true;
        }
    }
}

/// Gated CE op_script_action: original scr_find_sid_from_program lookup
/// produces zero when no matching script can be found. Real script action
/// state has not yet been linked; the caller supplies its current value.
/// No game state or script dispatch occurs.
pub fn push_script_action(
    stack: &mut Vec<ScriptValue>,
    current_action: Option<i32>,
) -> io::Result<()> {
    if stack.len() >= MAX_STACK_VALUES {
        return Err(invalid("CE script action stack exceeds bounded capacity"));
    }
    stack.push(ScriptValue::Int(current_action.unwrap_or(0)));
    Ok(())
}

/// Like run_variable_subset_bounded, but explicitly supplies the current
/// script action for CE opcode 0x80C7. This *does not* determine the action
/// from a running Fallout engine or synthesize its event lifecycle.
pub fn run_with_script_action_bounded(
    vm: &mut ScriptVm,
    instance_id: u64,
    locals: &mut MapScriptLocals,
    globals: &mut GameGlobalState,
    current_action: Option<i32>,
    max_steps: usize,
) -> io::Result<usize> {
    vm.run_variable_subset_bounded(max_steps, &mut |opcode, stack| match opcode {
        LOCAL_VAR_OPCODE | SET_LOCAL_VAR_OPCODE | MAP_VAR_OPCODE | SET_MAP_VAR_OPCODE => {
            apply_map_variable_opcode(locals, instance_id, opcode, stack)
        }
        game_opcode @ (0x80C5 | 0x80C6) => globals.apply_opcode(game_opcode, stack),
        SCRIPT_ACTION_OPCODE => push_script_action(stack, current_action),
        _ => Err(invalid("unrecognized CE native script-state opcode")),
    })
}

/// Run explicitly supported CE script-state native instructions using
/// a caller-owned invocation context. SCRIPT_OVERRIDES (0x80B9) changes only
/// the local context flag, and SCRIPT_ACTION (0x80C7) reads its action.
/// Normal ScriptVm::step() remains unable to execute these opcodes.
pub fn run_invocation_subset_bounded(
    vm: &mut ScriptVm,
    instance_id: u64,
    locals: &mut MapScriptLocals,
    globals: &mut GameGlobalState,
    invocation: &mut ScriptInvocationState,
    max_steps: usize,
) -> io::Result<usize> {
    vm.run_variable_subset_bounded(max_steps, &mut |opcode, stack| match opcode {
        LOCAL_VAR_OPCODE | SET_LOCAL_VAR_OPCODE | MAP_VAR_OPCODE | SET_MAP_VAR_OPCODE => {
            apply_map_variable_opcode(locals, instance_id, opcode, stack)
        }
        game_opcode @ (0x80C5 | 0x80C6) => globals.apply_opcode(game_opcode, stack),
        SCRIPT_ACTION_OPCODE => push_script_action(stack, Some(invocation.action())),
        SCRIPT_OVERRIDES_OPCODE => {
            invocation.mark_overrides();
            Ok(())
        }
        _ => Err(invalid("unrecognized CE invocation opcode")),
    })
}

#[cfg(test)]
mod tests {
    use super::{
        apply_map_variable_opcode, run_invocation_subset_bounded, run_variable_subset_bounded,
        run_with_script_action_bounded, ScriptInvocationState, LOCAL_VAR_OPCODE, MAP_VAR_OPCODE,
        SCRIPT_ACTION_OPCODE, SCRIPT_OVERRIDES_OPCODE, SET_LOCAL_VAR_OPCODE, SET_MAP_VAR_OPCODE,
    };
    use crate::game_globals::GameGlobalState;
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

    fn program(code: &[u8]) -> ScriptVm {
        assert!(code.len() <= 42);
        let mut bytes = vec![0_u8; 42];
        bytes[..code.len()].copy_from_slice(code);
        bytes.extend_from_slice(&[0_u8; 12]);
        ScriptVm::new(ScriptProgram::decode(&bytes).unwrap())
    }

    fn int(code: &mut Vec<u8>, number: i32) {
        code.extend_from_slice(&0xC001_u16.to_be_bytes());
        code.extend_from_slice(&number.to_be_bytes());
    }

    fn op(code: &mut Vec<u8>, opcode: u16) {
        code.extend_from_slice(&opcode.to_be_bytes());
    }

    #[test]
    fn executes_synthetic_int_variable_bytecode_only_with_explicit_host() {
        let mut code = Vec::new();
        int(&mut code, 1);
        int(&mut code, 39);
        op(&mut code, SET_MAP_VAR_OPCODE);
        int(&mut code, 1);
        op(&mut code, MAP_VAR_OPCODE);
        int(&mut code, 0);
        op(&mut code, 0x80C5);
        op(&mut code, 0x8039); // original CE numeric ADD
        op(&mut code, 0x8011); // STOP
        let mut vm = program(&code);
        let mut map = fixture();
        let mut game = GameGlobalState::from_initial_values(&[7, 8]).unwrap();
        assert_eq!(
            run_variable_subset_bounded(&mut vm, 1, &mut map, &mut game, 12).unwrap(),
            9
        );
        assert!(vm.halted());
        assert_eq!(vm.stack(), [ScriptValue::Int(46)]);
        assert_eq!(map.globals(), [12, 39]);
        assert_eq!(game.values(), [7, 8]);

        let mut default_vm = program(&0x80C3_u16.to_be_bytes());
        assert!(default_vm.step().is_err());
    }

    #[test]
    fn synthetic_game_global_write_and_local_pointer_round_trip() {
        let mut code = Vec::new();
        int(&mut code, 1);
        int(&mut code, 99);
        op(&mut code, 0x80C6);
        int(&mut code, 1);
        op(&mut code, 0x80C5);
        op(&mut code, 0x8011);
        let mut vm = program(&code);
        let mut map = fixture();
        let mut game = GameGlobalState::from_initial_values(&[7, 8]).unwrap();
        assert_eq!(
            run_variable_subset_bounded(&mut vm, 1, &mut map, &mut game, 9).unwrap(),
            6
        );
        assert_eq!(vm.stack(), [ScriptValue::Int(99)]);
        assert_eq!(game.values(), [7, 99]);

        let mut local = Vec::new();
        int(&mut local, 0);
        op(&mut local, 0xE001);
        local.extend_from_slice(&123_i32.to_be_bytes());
        op(&mut local, SET_LOCAL_VAR_OPCODE);
        int(&mut local, 0);
        op(&mut local, LOCAL_VAR_OPCODE);
        op(&mut local, 0x8011);
        let mut vm = program(&local);
        assert_eq!(
            run_variable_subset_bounded(&mut vm, 1, &mut map, &mut game, 9).unwrap(),
            6
        );
        assert_eq!(vm.stack(), [ScriptValue::Pointer(123)]);
        assert_eq!(map.locals(), [0, 11]); // pointer stored out of band.
    }

    #[test]
    fn explicitly_gated_host_failures_preserve_vm_stack_and_pc() {
        let mut code = Vec::new();
        int(&mut code, 1);
        op(&mut code, 0x80C5);
        let mut vm = program(&code);
        vm.step().unwrap();
        let saved_pc = vm.pc();
        let saved_stack = vm.stack().to_vec();
        let mut rejected = |_op: u16, stack: &mut Vec<ScriptValue>| {
            stack.push(ScriptValue::Int(999));
            Err(std::io::Error::other("synthetic rejected native opcode"))
        };
        assert!(vm.step_with_variable_host(&mut rejected).is_err());
        assert_eq!(vm.pc(), saved_pc);
        assert_eq!(vm.stack(), saved_stack);
    }

    #[test]
    fn ce_script_overrides_reset_at_each_new_invocation() {
        let mut code = Vec::new();
        op(&mut code, SCRIPT_OVERRIDES_OPCODE);
        op(&mut code, SCRIPT_ACTION_OPCODE);
        op(&mut code, 0x8011);
        let mut map = fixture();
        let mut globals = GameGlobalState::from_initial_values(&[8]).unwrap();
        let mut invocation = ScriptInvocationState::installed();
        assert!(invocation.begin_action(15));
        let mut first = program(&code);
        assert_eq!(run_invocation_subset_bounded(
            &mut first, 1, &mut map, &mut globals, &mut invocation, 5,
        ).unwrap(), 3);
        assert_eq!(first.stack(), [ScriptValue::Int(15)]);
        assert!(invocation.overrides());
        assert!(invocation.begin_action(23));
        assert!(!invocation.overrides());
        let mut next = program(&code);
        run_invocation_subset_bounded(
            &mut next, 1, &mut map, &mut globals, &mut invocation, 5,
        ).unwrap();
        assert_eq!(next.stack(), [ScriptValue::Int(23)]);
        assert!(invocation.overrides());
    }

    #[test]
    fn missing_sid_leaves_override_unset_and_returns_zero() {
        let mut code = Vec::new();
        op(&mut code, SCRIPT_OVERRIDES_OPCODE);
        op(&mut code, SCRIPT_ACTION_OPCODE);
        op(&mut code, 0x8011);
        let mut state = ScriptInvocationState::missing();
        assert!(!state.begin_action(15));
        let mut map = fixture();
        let mut globals = GameGlobalState::from_initial_values(&[8]).unwrap();
        let mut vm = program(&code);
        run_invocation_subset_bounded(
            &mut vm, 1, &mut map, &mut globals, &mut state, 5,
        ).unwrap();
        assert_eq!(vm.stack(), [ScriptValue::Int(0)]);
        assert!(!state.overrides());
        let mut ungated = program(&code);
        assert!(ungated.step().is_err());
    }

    #[test]
    fn ce_script_action_is_bounded_opt_in_and_returns_zero_for_unknown_sid() {
        let mut code = Vec::new();
        op(&mut code, SCRIPT_ACTION_OPCODE);
        int(&mut code, 4);
        op(&mut code, 0x8039);
        op(&mut code, 0x8011);
        let mut vm = program(&code);
        let mut map = fixture();
        let mut game = GameGlobalState::from_initial_values(&[8]).unwrap();
        assert_eq!(
            run_with_script_action_bounded(&mut vm, 1, &mut map, &mut game, Some(15), 7).unwrap(),
            4
        );
        assert_eq!(vm.stack(), [ScriptValue::Int(19)]);
        assert!(vm.halted());
        let mut unknown = program(&code);
        assert_eq!(
            run_with_script_action_bounded(&mut unknown, 1, &mut map, &mut game, None, 7).unwrap(),
            4
        );
        assert_eq!(unknown.stack(), [ScriptValue::Int(4)]);
        let mut not_authorized = program(&code);
        assert!(
            run_variable_subset_bounded(&mut not_authorized, 1, &mut map, &mut game, 7).is_err()
        );
        assert_eq!(not_authorized.pc(), 0); // explicit host failure is rollback-safe.
        assert!(not_authorized.step().is_err());
        assert_eq!(not_authorized.pc(), 2); // ordinary core step already decoded opcode.
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
