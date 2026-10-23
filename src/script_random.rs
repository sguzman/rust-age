//! Explicit-host CE op_random and op_roll_dice source-compatible subset.
//! Pinned src/int/support/intextra.cc op_random (0x80B4),
//! op_roll_dice (0x80B5), and src/game/roll.cc roll_random.
//! The game uses a separate VCR playback branch in op_random; both paths
//! are modeled using explicit caller-owned state with no system entropy.
use crate::random_roll::FalloutRng;
use crate::script_vm::{ScriptValue, ScriptVm};
use std::io;

pub const RANDOM_OPCODE: u16 = 0x80B4;
pub const ROLL_DICE_OPCODE: u16 = 0x80B5;

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptRandomHost {
    pub rng: FalloutRng,
    /// Pinned vcr_status != VCR_STATE_TURNED_OFF uses midpoint arithmetic
    /// instead of consuming a random generator draw.
    pub vcr_playback: bool,
}

impl ScriptRandomHost {
    pub fn from_seed(seed: i32, vcr_playback: bool) -> Self {
        Self {
            rng: FalloutRng::from_seed(seed),
            vcr_playback,
        }
    }

    /// Original op_random/roll_dice pops high first then low. The source
    /// roll_dice native logs unimplemented and pushes integer zero.
    /// Invalid Rust operand types and arithmetic overflow return an error
    /// before consuming any stack values; no native game objects are used.
    pub fn apply_opcode(&mut self, opcode: u16, stack: &mut Vec<ScriptValue>) -> io::Result<()> {
        if opcode != RANDOM_OPCODE && opcode != ROLL_DICE_OPCODE {
            return Err(invalid("unsupported CE random native opcode"));
        }
        if stack.len() < 2 {
            return Err(invalid("CE random native operand stack underflow"));
        }
        let index = stack.len() - 2;
        let (ScriptValue::Int(low), ScriptValue::Int(high)) = (stack[index], stack[index + 1])
        else {
            return Err(invalid("CE random native arguments must be integer"));
        };
        let value = if opcode == ROLL_DICE_OPCODE {
            0
        } else if self.vcr_playback {
            high.checked_sub(low)
                .ok_or_else(|| invalid("CE VCR random midpoint overflows signed i32"))?
                / 2
        } else {
            self.rng.between(low, high)?
        };
        stack.truncate(index);
        stack.push(ScriptValue::Int(value));
        Ok(())
    }

    /// Opt-in, instruction-bounded execution. The default VM cannot call
    /// this generator or any game native; the random seed is never implicit.
    pub fn run_bounded(&mut self, vm: &mut ScriptVm, max_steps: usize) -> io::Result<usize> {
        vm.run_variable_subset_bounded(max_steps, &mut |opcode, stack| {
            self.apply_opcode(opcode, stack)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{ScriptRandomHost, RANDOM_OPCODE, ROLL_DICE_OPCODE};
    use crate::script_program::ScriptProgram;
    use crate::script_vm::{ScriptValue, ScriptVm};

    #[test]
    fn ce_random_uses_original_min_max_pop_order_and_roll_seed() {
        let mut host = ScriptRandomHost::from_seed(1, false);
        let mut stack = vec![ScriptValue::Int(1), ScriptValue::Int(100)];
        host.apply_opcode(RANDOM_OPCODE, &mut stack).unwrap();
        assert_eq!(stack, [ScriptValue::Int(1)]);
        stack = vec![ScriptValue::Int(10), ScriptValue::Int(1)];
        host.apply_opcode(RANDOM_OPCODE, &mut stack).unwrap();
        assert_eq!(stack, [ScriptValue::Int(10)]);
    }

    #[test]
    fn dice_not_implemented_and_vcr_mode_do_not_consume_rng() {
        let mut host = ScriptRandomHost::from_seed(47, true);
        let rng_before = host.rng.clone();
        let mut stack = vec![ScriptValue::Int(1), ScriptValue::Int(100)];
        host.apply_opcode(RANDOM_OPCODE, &mut stack).unwrap();
        assert_eq!(stack, [ScriptValue::Int(49)]);
        let mut dice = vec![ScriptValue::Int(4), ScriptValue::Int(6)];
        host.apply_opcode(ROLL_DICE_OPCODE, &mut dice).unwrap();
        assert_eq!(dice, [ScriptValue::Int(0)]);
        assert_eq!(host.rng, rng_before);
    }

    #[test]
    fn invalid_arguments_do_not_mutate_stack_or_rng() {
        let mut host = ScriptRandomHost::from_seed(47, false);
        for (opcode, original) in [
            (RANDOM_OPCODE, vec![ScriptValue::Int(1)]),
            (
                RANDOM_OPCODE,
                vec![ScriptValue::Pointer(9), ScriptValue::Int(5)],
            ),
            (0x80B6, vec![ScriptValue::Int(1), ScriptValue::Int(4)]),
            (
                RANDOM_OPCODE,
                vec![ScriptValue::Int(i32::MIN), ScriptValue::Int(i32::MAX)],
            ),
        ] {
            let mut stack = original.clone();
            let saved = host.rng.clone();
            assert!(host.apply_opcode(opcode, &mut stack).is_err());
            assert_eq!(stack, original);
            assert_eq!(host.rng, saved);
        }
    }

    #[test]
    fn synthetic_random_int_program_is_strictly_opt_in() {
        let mut code = Vec::new();
        for value in [1_i32, 100] {
            code.extend_from_slice(&0xC001_u16.to_be_bytes());
            code.extend_from_slice(&value.to_be_bytes());
        }
        code.extend_from_slice(&RANDOM_OPCODE.to_be_bytes());
        code.extend_from_slice(&0x8011_u16.to_be_bytes());
        let mut bytes = vec![0_u8; 42];
        bytes[..code.len()].copy_from_slice(&code);
        bytes.extend_from_slice(&[0_u8; 12]);
        let program = ScriptProgram::decode(&bytes).unwrap();
        let mut vm = ScriptVm::new(program.clone());
        let mut host = ScriptRandomHost::from_seed(1, false);
        assert_eq!(host.run_bounded(&mut vm, 8).unwrap(), 4);
        assert_eq!(vm.stack(), [ScriptValue::Int(1)]);
        assert!(vm.halted());
        let mut ungated = ScriptVm::new(program);
        ungated.step().unwrap();
        ungated.step().unwrap();
        assert!(ungated.step().is_err());
    }
}
