//! Isolated, bounded first slice of CE's typed stack interpreter.
//!
//! Reference: pinned Fallout 1 CE src/int/intrpret.cc op_const,
//! op_pop, op_dup, op_swap, op_add, op_sub, op_mul, op_div,
//! op_negate, op_floor, op_jmp, op_stop_prog and op_exit_prog.
//!
//! The interpreter is intentionally NOT connected to map scripts, callbacks,
//! native opcodes, persistent state, or player input. Unknown opcodes error.
//! Only numeric arithmetic and basic stack/control operations are supported.
use crate::script_program::ScriptProgram;
use std::io;

const MAX_STACK_VALUES: usize = 0x800;

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScriptValue {
    Int(i32),
    Float(f32),
    StaticString(i32),
    DynamicString(i32),
    Pointer(i32),
}

impl ScriptValue {
    fn from_push_opcode(opcode: u16, value: i32) -> io::Result<Self> {
        match opcode {
            0xC001 => Ok(Self::Int(value)),
            0xA001 => Ok(Self::Float(f32::from_bits(value as u32))),
            0x9001 => Ok(Self::StaticString(value)),
            0x9801 => Ok(Self::DynamicString(value)),
            0xE001 => Ok(Self::Pointer(value)),
            _ => Err(invalid("unknown CE PUSH value type")),
        }
    }

    fn as_float(self) -> io::Result<f32> {
        match self {
            Self::Int(value) => Ok(value as f32),
            Self::Float(value) => Ok(value),
            _ => Err(invalid("non-numeric operand in CE arithmetic")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arithmetic {
    Add,
    Subtract,
    Multiply,
    Divide,
}

fn numeric_binary(
    left: ScriptValue,
    right: ScriptValue,
    op: Arithmetic,
) -> io::Result<ScriptValue> {
    if let (ScriptValue::Int(lhs), ScriptValue::Int(rhs)) = (left, right) {
        return match op {
            Arithmetic::Add => Ok(match lhs.checked_add(rhs) {
                Some(value) => ScriptValue::Int(value),
                // CE promotes an overflowing integer addition to float.
                None => ScriptValue::Float(lhs as f32 + rhs as f32),
            }),
            Arithmetic::Subtract => Ok(ScriptValue::Int(lhs.wrapping_sub(rhs))),
            Arithmetic::Multiply => Ok(ScriptValue::Int(lhs.wrapping_mul(rhs))),
            Arithmetic::Divide => lhs
                .checked_div(rhs)
                .map(ScriptValue::Int)
                .ok_or_else(|| invalid("CE integer division by zero or overflow")),
        };
    }
    let left = left.as_float()?;
    let right = right.as_float()?;
    if op == Arithmetic::Divide && right == 0.0 {
        return Err(invalid("CE float division by zero"));
    }
    let result = match op {
        Arithmetic::Add => left + right,
        Arithmetic::Subtract => left - right,
        Arithmetic::Multiply => left * right,
        Arithmetic::Divide => left / right,
    };
    Ok(ScriptValue::Float(result))
}

#[derive(Debug, Clone)]
pub struct ScriptVm {
    pub program: ScriptProgram,
    pc: usize,
    stack: Vec<ScriptValue>,
    halted: bool,
}

impl ScriptVm {
    pub fn new(program: ScriptProgram) -> Self {
        Self {
            program,
            pc: 0,
            stack: Vec::new(),
            halted: false,
        }
    }

    pub fn pc(&self) -> usize {
        self.pc
    }

    pub fn stack(&self) -> &[ScriptValue] {
        &self.stack
    }

    pub fn halted(&self) -> bool {
        self.halted
    }

    fn push(&mut self, value: ScriptValue) -> io::Result<()> {
        if self.stack.len() >= MAX_STACK_VALUES {
            return Err(invalid("CE VM operand stack exhausted"));
        }
        self.stack.push(value);
        Ok(())
    }

    fn pop(&mut self) -> io::Result<ScriptValue> {
        self.stack
            .pop()
            .ok_or_else(|| invalid("CE VM operand stack underflow"))
    }

    fn pop_int(&mut self) -> io::Result<i32> {
        match self.pop()? {
            ScriptValue::Int(value) => Ok(value),
            _ => Err(invalid("CE VM integer operand required")),
        }
    }

    pub fn step(&mut self) -> io::Result<()> {
        if self.halted {
            return Ok(());
        }
        let opcode = self.program.word_at(self.pc)?;
        if opcode & 0x8000 == 0 {
            return Err(invalid("invalid CE interpreter opcode marker"));
        }
        self.pc = self
            .pc
            .checked_add(2)
            .ok_or_else(|| invalid("CE program counter overflow"))?;
        match opcode & 0x03FF {
            0x00 => {} // NOOP
            0x01 => {
                let immediate = self.program.long_at(self.pc)?;
                self.pc = self
                    .pc
                    .checked_add(4)
                    .ok_or_else(|| invalid("CE program counter overflow"))?;
                self.push(ScriptValue::from_push_opcode(opcode, immediate)?)?;
            }
            0x04 => {
                let dest = self.pop_int()?;
                let dest =
                    usize::try_from(dest).map_err(|_| invalid("negative CE jump address"))?;
                if dest >= self.program.file_len() {
                    return Err(invalid("CE jump address outside program"));
                }
                self.pc = dest;
            }
            0x0E | 0x10 | 0x11 => {
                // EXIT, EXIT_PROGRAM and STOP_PROGRAM have distinct CE
                // lifecycle effects; here all terminate the isolated run.
                self.halted = true;
            }
            0x18 => {
                let first = self.pop()?;
                let second = self.pop()?;
                self.push(first)?;
                self.push(second)?;
            }
            0x1A => {
                self.pop()?;
            }
            0x1B => {
                let value = self.pop()?;
                self.push(value)?;
                self.push(value)?;
            }
            0x39..=0x3C => {
                let right = self.pop()?;
                let left = self.pop()?;
                let operation = match opcode & 0x03FF {
                    0x39 => Arithmetic::Add,
                    0x3A => Arithmetic::Subtract,
                    0x3B => Arithmetic::Multiply,
                    _ => Arithmetic::Divide,
                };
                self.push(numeric_binary(left, right, operation)?)?;
            }
            0x44 => {
                let value = self.pop()?;
                self.push(match value {
                    ScriptValue::Float(value) => ScriptValue::Int(value as i32),
                    ScriptValue::Int(value) => ScriptValue::Int(value),
                    _ => return Err(invalid("non-numeric CE FLOOR operand")),
                })?;
            }
            0x46 => {
                let value = self.pop()?;
                self.push(match value {
                    ScriptValue::Int(value) => ScriptValue::Int(value.wrapping_neg()),
                    ScriptValue::Float(value) => ScriptValue::Float(-value),
                    _ => return Err(invalid("non-numeric CE NEGATE operand")),
                })?;
            }
            _ => return Err(invalid("unimplemented CE interpreter opcode")),
        }
        Ok(())
    }

    /// Execute only the explicitly supported core opcodes under a caller
    /// supplied instruction budget. Never wait, invoke scripts or hang.
    pub fn run_bounded(&mut self, max_steps: usize) -> io::Result<usize> {
        for count in 0..max_steps {
            if self.halted {
                return Ok(count);
            }
            self.step()?;
        }
        if self.halted {
            return Ok(max_steps);
        }
        Err(invalid("CE interpreter instruction budget exhausted"))
    }
}

#[cfg(test)]
mod tests {
    use super::{ScriptValue, ScriptVm};
    use crate::script_program::ScriptProgram;

    fn fixture(instructions: &[u8]) -> ScriptProgram {
        assert!(instructions.len() <= 42);
        let mut data = vec![0_u8; 42];
        data[..instructions.len()].copy_from_slice(instructions);
        data.extend_from_slice(&[0_u8; 12]); // zero procedures, identifiers, strings
        ScriptProgram::decode(&data).unwrap()
    }

    fn push_int(buffer: &mut Vec<u8>, value: i32) {
        buffer.extend_from_slice(&0xC001_u16.to_be_bytes());
        buffer.extend_from_slice(&value.to_be_bytes());
    }

    fn opcode(buffer: &mut Vec<u8>, value: u16) {
        buffer.extend_from_slice(&value.to_be_bytes());
    }

    #[test]
    fn executes_ce_typed_immediates_addition_and_stop_with_budget() {
        let mut code = Vec::new();
        push_int(&mut code, 6);
        push_int(&mut code, 7);
        opcode(&mut code, 0x8039); // ADD
        opcode(&mut code, 0x8011); // STOP_PROGRAM
        let mut vm = ScriptVm::new(fixture(&code));
        assert_eq!(vm.run_bounded(8).unwrap(), 4);
        assert_eq!(vm.stack(), [ScriptValue::Int(13)]);
        assert!(vm.halted());
        assert_eq!(vm.pc(), 16);
    }

    #[test]
    fn reproduces_ce_int_overflow_promotion_to_float() {
        let mut code = Vec::new();
        push_int(&mut code, i32::MAX);
        push_int(&mut code, 1);
        opcode(&mut code, 0x8039);
        opcode(&mut code, 0x8010);
        let mut vm = ScriptVm::new(fixture(&code));
        vm.run_bounded(8).unwrap();
        assert_eq!(vm.stack(), [ScriptValue::Float(i32::MAX as f32 + 1.0)]);
    }

    #[test]
    fn preserves_float_type_and_stack_duplication() {
        let mut code = Vec::new();
        opcode(&mut code, 0xA001);
        code.extend_from_slice(&1.5_f32.to_bits().to_be_bytes());
        opcode(&mut code, 0x801B); // DUP
        opcode(&mut code, 0x8039); // ADD
        opcode(&mut code, 0x8011); // STOP
        let mut vm = ScriptVm::new(fixture(&code));
        vm.run_bounded(8).unwrap();
        assert_eq!(vm.stack(), [ScriptValue::Float(3.0)]);
    }

    #[test]
    fn rejects_bad_operands_unsupported_opcodes_and_endless_loop() {
        let mut zero_div = Vec::new();
        push_int(&mut zero_div, 9);
        push_int(&mut zero_div, 0);
        opcode(&mut zero_div, 0x803C);
        let mut vm = ScriptVm::new(fixture(&zero_div));
        assert!(vm.run_bounded(5).is_err());

        let mut vm = ScriptVm::new(fixture(&0x804C_u16.to_be_bytes()));
        assert!(vm.step().is_err());

        let mut code = Vec::new();
        push_int(&mut code, 0);
        opcode(&mut code, 0x8004); // JUMP
        let mut vm = ScriptVm::new(fixture(&code));
        assert!(vm.run_bounded(5).is_err());
    }
}
