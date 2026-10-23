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

    /// CE op_not tests the raw 32-bit union field, not floating point equality.
    fn raw_zero(self) -> bool {
        match self {
            Self::Int(value)
            | Self::StaticString(value)
            | Self::DynamicString(value)
            | Self::Pointer(value) => value == 0,
            Self::Float(value) => value.to_bits() == 0,
        }
    }

    /// CE ProgramValue::isEmpty powers IF and WHILE and tests numeric zero.
    fn is_empty(self) -> bool {
        match self {
            Self::Float(value) => value == 0.0,
            Self::Int(value) | Self::StaticString(value) | Self::Pointer(value) => value == 0,
            Self::DynamicString(_) => true, // CE isEmpty has no dynamic string case.
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

fn numeric_comparison(left: ScriptValue, right: ScriptValue, operation: u16) -> io::Result<bool> {
    if let (ScriptValue::Int(a), ScriptValue::Int(b)) = (left, right) {
        return Ok(match operation {
            0x33 => a == b,
            0x34 => a != b,
            0x35 => a <= b,
            0x36 => a >= b,
            0x37 => a < b,
            0x38 => a > b,
            _ => return Err(invalid("unsupported CE comparison")),
        });
    }
    let a = left.as_float()?;
    let b = right.as_float()?;
    Ok(match operation {
        0x33 => a == b,
        0x34 => a != b,
        0x35 => a <= b,
        0x36 => a >= b,
        0x37 => a < b,
        0x38 => a > b,
        _ => return Err(invalid("unsupported CE comparison")),
    })
}

#[derive(Debug, Clone)]
pub struct ScriptVm {
    pub program: ScriptProgram,
    pc: usize,
    stack: Vec<ScriptValue>,
    return_stack: Vec<ScriptValue>,
    frame_pointer: i32,
    base_pointer: i32,
    critical_section: bool,
    halted: bool,
}

impl ScriptVm {
    pub fn new(program: ScriptProgram) -> Self {
        Self {
            program,
            pc: 0,
            stack: Vec::new(),
            return_stack: Vec::new(),
            frame_pointer: -1,
            base_pointer: -1,
            critical_section: false,
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

    pub fn frame_pointer(&self) -> i32 {
        self.frame_pointer
    }

    pub fn base_pointer(&self) -> i32 {
        self.base_pointer
    }

    pub fn return_stack(&self) -> &[ScriptValue] {
        &self.return_stack
    }

    pub fn critical_section(&self) -> bool {
        self.critical_section
    }

    fn procedure_at(&self, index: i32) -> io::Result<crate::script_program::ProcedureEntry> {
        let index = usize::try_from(index)
            .map_err(|_| invalid("negative CE procedure index"))?;
        self.program.procedures().get(index).copied()
            .ok_or_else(|| invalid("CE procedure index outside table"))
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

    fn push_return(&mut self, value: ScriptValue) -> io::Result<()> {
        if self.return_stack.len() >= MAX_STACK_VALUES {
            return Err(invalid("CE VM return stack exhausted"));
        }
        self.return_stack.push(value);
        Ok(())
    }

    fn pop_return(&mut self) -> io::Result<ScriptValue> {
        self.return_stack
            .pop()
            .ok_or_else(|| invalid("CE VM return stack underflow"))
    }

    fn pop_return_int(&mut self) -> io::Result<i32> {
        match self.pop_return()? {
            ScriptValue::Int(value) => Ok(value),
            _ => Err(invalid("CE VM integer return value required")),
        }
    }

    /// Frame/global offsets index values remaining on the data stack. This
    /// does not follow pointers outside the bounded Rust-owned stack.
    fn at_base(&self, base: i32, offset: i32) -> io::Result<usize> {
        let location = base
            .checked_add(offset)
            .ok_or_else(|| invalid("CE interpreter stack index overflow"))?;
        let index = usize::try_from(location)
            .map_err(|_| invalid("negative CE interpreter stack index"))?;
        if index >= self.stack.len() {
            return Err(invalid("CE interpreter stack slot out of bounds"));
        }
        Ok(index)
    }

    fn pop_int(&mut self) -> io::Result<i32> {
        match self.pop()? {
            ScriptValue::Int(value) => Ok(value),
            _ => Err(invalid("CE VM integer operand required")),
        }
    }

    fn jump(&mut self, destination: i32) -> io::Result<()> {
        let target =
            usize::try_from(destination).map_err(|_| invalid("negative CE jump address"))?;
        if target >= self.program.file_len() {
            return Err(invalid("CE jump address outside program"));
        }
        self.pc = target;
        Ok(())
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
            0x02 | 0x4A => {
                self.critical_section = true;
            }
            0x03 | 0x4B => {
                self.critical_section = false;
            }
            0x04 => {
                let dest = self.pop_int()?;
                self.jump(dest)?;
            }
            0x05 => {
                let index = self.pop_int()?;
                let procedure = self.procedure_at(index)?;
                if procedure.flags & 0x04 != 0 {
                    return Err(invalid("CE imported/external procedure calls not implemented"));
                }
                self.jump(procedure.body_offset)?;
                if procedure.flags & 0x10 != 0 {
                    self.critical_section = true;
                }
            }
            0x0C => {
                let value = self.pop_return()?;
                self.push(value)?;
            }
            0x0D => {
                let value = self.pop()?;
                self.push_return(value)?;
            }
            0x0E | 0x10 | 0x11 => {
                // EXIT, EXIT_PROGRAM and STOP_PROGRAM have distinct CE
                // lifecycle effects; here all terminate the isolated run.
                self.halted = true;
            }
            0x12 => {
                let offset = self.pop_int()?;
                let index = self.at_base(self.base_pointer, offset)?;
                self.push(self.stack[index])?;
            }
            0x13 => {
                let offset = self.pop_int()?;
                let value = self.pop()?;
                let index = self.at_base(self.base_pointer, offset)?;
                self.stack[index] = value;
            }
            0x18 => {
                let first = self.pop()?;
                let second = self.pop()?;
                self.push(first)?;
                self.push(second)?;
            }
            0x19 => {
                let first = self.pop_return()?;
                let second = self.pop_return()?;
                self.push_return(first)?;
                self.push_return(second)?;
            }
            0x1A => {
                self.pop()?;
            }
            0x1B => {
                let value = self.pop()?;
                self.push(value)?;
                self.push(value)?;
            }
            0x1C => {
                let address = self.pop_return_int()?;
                self.jump(address)?;
            }
            0x1E => {
                self.pop_return()?;
            }
            0x27 => {
                let expected = self.pop_int()?;
                let index = self.pop_int()?;
                if self.procedure_at(index)?.argument_count != expected {
                    return Err(invalid("CE procedure argument count mismatch"));
                }
            }
            0x29 => {
                self.frame_pointer = self.pop_return_int()?;
            }
            0x2A => {
                let target = usize::try_from(self.frame_pointer)
                    .map_err(|_| invalid("CE POP_TO_BASE without an active frame"))?;
                if target > self.stack.len() {
                    return Err(invalid("CE frame extends beyond data stack"));
                }
                self.stack.truncate(target);
            }
            0x2B => {
                let argument_count = self.pop_int()?;
                let argument_count = usize::try_from(argument_count)
                    .map_err(|_| invalid("negative CE frame argument count"))?;
                if argument_count > self.stack.len() {
                    return Err(invalid("CE frame argument count exceeds data stack"));
                }
                self.push_return(ScriptValue::Int(self.frame_pointer))?;
                self.frame_pointer = i32::try_from(self.stack.len() - argument_count)
                    .map_err(|_| invalid("CE frame pointer exceeds i32"))?;
            }
            0x2C => {
                self.base_pointer = i32::try_from(self.stack.len())
                    .map_err(|_| invalid("CE global base pointer exceeds i32"))?;
            }
            0x2D => {
                let index = self.pop_int()?;
                let entry = self.procedure_at(index)?;
                self.push(ScriptValue::Int(entry.body_offset))?;
            }
            0x2E => {
                let count = self.pop_int()?;
                let count =
                    usize::try_from(count).map_err(|_| invalid("negative CE DUMP count"))?;
                let remaining = self
                    .stack
                    .len()
                    .checked_sub(count)
                    .ok_or_else(|| invalid("CE DUMP stack underflow"))?;
                self.stack.truncate(remaining);
            }
            0x31 => {
                let offset = self.pop_int()?;
                let value = self.pop()?;
                let index = self.at_base(self.frame_pointer, offset)?;
                self.stack[index] = value;
            }
            0x32 => {
                let offset = self.pop_int()?;
                let index = self.at_base(self.frame_pointer, offset)?;
                self.push(self.stack[index])?;
            }

            0x2F => {
                // IF discards condition, then consumes either branch target
                // (false case) or the remaining unneeded value (true case).
                if self.pop()?.is_empty() {
                    let dest = self.pop_int()?;
                    self.jump(dest)?;
                } else {
                    self.pop()?;
                }
            }
            0x30 => {
                // WHILE retains the branch target when true (CE behavior).
                if self.pop()?.is_empty() {
                    let dest = self.pop_int()?;
                    self.jump(dest)?;
                }
            }
            0x33..=0x38 => {
                let right = self.pop()?;
                let left = self.pop()?;
                let result = numeric_comparison(left, right, opcode & 0x03FF)?;
                self.push(ScriptValue::Int(i32::from(result)))?;
            }
            0x3D => {
                let divisor = self.pop_int()?;
                let dividend = self.pop_int()?;
                let result = dividend
                    .checked_rem(divisor)
                    .ok_or_else(|| invalid("CE integer MOD by zero or overflow"))?;
                self.push(ScriptValue::Int(result))?;
            }
            0x3E | 0x3F => {
                let right = self.pop()?;
                let left = self.pop()?;
                let truth = if opcode & 0x03FF == 0x3E {
                    !left.raw_zero() && !right.raw_zero()
                } else {
                    !left.raw_zero() || !right.raw_zero()
                };
                self.push(ScriptValue::Int(i32::from(truth)))?;
            }
            0x40..=0x42 => {
                let right = self.pop_int()?;
                let left = self.pop_int()?;
                let value = match opcode & 0x03FF {
                    0x40 => left & right,
                    0x41 => left | right,
                    _ => left ^ right,
                };
                self.push(ScriptValue::Int(value))?;
            }
            0x43 => {
                let value = self.pop_int()?;
                self.push(ScriptValue::Int(!value))?;
            }
            0x45 => {
                let value = self.pop()?;
                self.push(ScriptValue::Int(i32::from(value.raw_zero())))?;
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
    fn comparisons_and_boolean_arithmetic_follow_original_operand_order() {
        let mut code = Vec::new();
        push_int(&mut code, 3);
        push_int(&mut code, 7);
        opcode(&mut code, 0x8037); // LESS THAN
        push_int(&mut code, 0);
        opcode(&mut code, 0x803F); // OR
        push_int(&mut code, 1);
        opcode(&mut code, 0x8033); // EQUAL
        opcode(&mut code, 0x8011);
        let mut vm = ScriptVm::new(fixture(&code));
        vm.run_bounded(16).unwrap();
        assert_eq!(vm.stack(), [ScriptValue::Int(1)]);
    }

    #[test]
    fn integer_modulo_bitops_and_not_follow_ce() {
        let mut code = Vec::new();
        push_int(&mut code, -11);
        push_int(&mut code, 4);
        opcode(&mut code, 0x803D); // MOD: -11 remainder 4
        push_int(&mut code, -1);
        opcode(&mut code, 0x8042); // XOR
        opcode(&mut code, 0x8043); // BNOT
        opcode(&mut code, 0x8011);
        let mut vm = ScriptVm::new(fixture(&code));
        vm.run_bounded(16).unwrap();
        assert_eq!(vm.stack(), [ScriptValue::Int(-3)]);

        let mut vm = ScriptVm::new(fixture(&0x8043_u16.to_be_bytes()));
        assert!(vm.step().is_err());
    }

    #[test]
    fn conditional_branch_uses_ce_popped_target_and_while_stack_rule() {
        let mut code = Vec::new();
        push_int(&mut code, 32);
        push_int(&mut code, 0);
        opcode(&mut code, 0x802F); // IF: empty -> jump to offset 32
        opcode(&mut code, 0x804C); // deliberately unsupported, skipped
        while code.len() < 32 {
            opcode(&mut code, 0x8000);
        }
        push_int(&mut code, 123);
        opcode(&mut code, 0x8011);
        let mut vm = ScriptVm::new(fixture(&code));
        vm.run_bounded(12).unwrap();
        assert_eq!(vm.stack(), [ScriptValue::Int(123)]);
    }

    #[test]
    fn ce_return_stack_transfers_and_swapa_preserve_typed_values() {
        let mut code = Vec::new();
        push_int(&mut code, 1);
        opcode(&mut code, 0x800D); // D_TO_A
        push_int(&mut code, 2);
        opcode(&mut code, 0x800D);
        opcode(&mut code, 0x8019); // SWAPA
        opcode(&mut code, 0x800C); // A_TO_D
        opcode(&mut code, 0x800C);
        opcode(&mut code, 0x8011);
        let mut vm = ScriptVm::new(fixture(&code));
        vm.run_bounded(12).unwrap();
        assert_eq!(vm.stack(), [ScriptValue::Int(1), ScriptValue::Int(2)]);
        assert!(vm.return_stack().is_empty());
    }

    #[test]
    fn ce_frame_base_fetch_and_restore_return_stack() {
        let mut code = Vec::new();
        push_int(&mut code, 42);
        push_int(&mut code, 1);
        opcode(&mut code, 0x802B); // PUSH_BASE
        push_int(&mut code, 0);
        opcode(&mut code, 0x8032); // FETCH local
        opcode(&mut code, 0x8029); // POP_BASE
        opcode(&mut code, 0x8011);
        let mut vm = ScriptVm::new(fixture(&code));
        vm.run_bounded(12).unwrap();
        assert_eq!(vm.stack(), [ScriptValue::Int(42), ScriptValue::Int(42)]);
        assert_eq!(vm.frame_pointer(), -1);
        assert!(vm.return_stack().is_empty());
    }

    #[test]
    fn ce_local_and_global_store_preserve_stack_slot_addresses() {
        let mut local = Vec::new();
        push_int(&mut local, 42);
        push_int(&mut local, 1);
        opcode(&mut local, 0x802B);
        push_int(&mut local, 99);
        push_int(&mut local, 0);
        opcode(&mut local, 0x8031); // STORE
        push_int(&mut local, 0);
        opcode(&mut local, 0x8032); // FETCH
        opcode(&mut local, 0x8011);
        let mut vm = ScriptVm::new(fixture(&local));
        vm.run_bounded(16).unwrap();
        assert_eq!(vm.stack(), [ScriptValue::Int(99), ScriptValue::Int(99)]);
        assert_eq!(vm.frame_pointer(), 0);

        let mut global = Vec::new();
        opcode(&mut global, 0x802C); // SET_GLOBAL
        push_int(&mut global, 77);
        push_int(&mut global, 42);
        push_int(&mut global, 0);
        opcode(&mut global, 0x8013); // STORE_GLOBAL
        push_int(&mut global, 0);
        opcode(&mut global, 0x8012); // FETCH_GLOBAL
        opcode(&mut global, 0x8011);
        let mut vm = ScriptVm::new(fixture(&global));
        vm.run_bounded(16).unwrap();
        assert_eq!(vm.stack(), [ScriptValue::Int(42), ScriptValue::Int(42)]);
        assert_eq!(vm.base_pointer(), 0);
    }

    #[test]
    fn ce_frame_primitives_reject_uninitialized_and_invalid_stack_slots() {
        let mut vm = ScriptVm::new(fixture(&0x8032_u16.to_be_bytes()));
        assert!(vm.step().is_err());
        let mut vm = ScriptVm::new(fixture(&0x8029_u16.to_be_bytes()));
        assert!(vm.step().is_err());
        let mut vm = ScriptVm::new(fixture(&0x802A_u16.to_be_bytes()));
        assert!(vm.step().is_err());
        let mut code = Vec::new();
        push_int(&mut code, 2);
        opcode(&mut code, 0x802B);
        let mut vm = ScriptVm::new(fixture(&code));
        assert!(vm.run_bounded(4).is_err());
    }


    fn fixture_with_procedure(instructions: &[u8], body_offset: i32, flags: i32, args: i32) -> ScriptProgram {
        assert!(instructions.len() <= 42);
        let mut bytes = vec![0_u8; 42];
        bytes[..instructions.len()].copy_from_slice(instructions);
        bytes.extend_from_slice(&1_i32.to_be_bytes()); // one Procedure
        for value in [4_i32, flags, 0, 0, body_offset, args] {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        bytes.extend_from_slice(&5_i32.to_be_bytes());
        bytes.extend_from_slice(b"main\0");
        bytes.extend_from_slice(&0_i32.to_be_bytes());
        ScriptProgram::decode(&bytes).unwrap()
    }

    #[test]
    fn ce_local_call_jumps_to_procedure_body_and_uses_return_stack() {
        let mut code = Vec::new();
        push_int(&mut code, 16); // continuation after CALL
        opcode(&mut code, 0x800D); // D_TO_A: save explicit return target
        push_int(&mut code, 0); // procedure index
        opcode(&mut code, 0x8005); // CALL
        opcode(&mut code, 0x8011); // STOP when return reaches byte 16
        push_int(&mut code, 9); // procedure body begins at byte 18
        opcode(&mut code, 0x801C); // POP_RETURN -> jumps to 16
        let mut vm = ScriptVm::new(fixture_with_procedure(&code, 18, 0, 0));
        vm.run_bounded(12).unwrap();
        assert_eq!(vm.stack(), [ScriptValue::Int(9)]);
        assert!(vm.return_stack().is_empty());
        assert!(vm.halted());
    }

    #[test]
    fn ce_procedure_address_and_argument_count_use_original_table() {
        let mut code = Vec::new();
        push_int(&mut code, 0);
        opcode(&mut code, 0x802D); // FETCH_PROCEDURE_ADDRESS
        push_int(&mut code, 0);
        push_int(&mut code, 2);
        opcode(&mut code, 0x8027); // CHECK_PROCEDURE_ARGUMENT_COUNT
        opcode(&mut code, 0x8011);
        let mut vm = ScriptVm::new(fixture_with_procedure(&code, 38, 0, 2));
        vm.run_bounded(12).unwrap();
        assert_eq!(vm.stack(), [ScriptValue::Int(38)]);

        let mut bad = Vec::new();
        push_int(&mut bad, 0);
        push_int(&mut bad, 99);
        opcode(&mut bad, 0x8027);
        let mut vm = ScriptVm::new(fixture_with_procedure(&bad, 18, 0, 2));
        assert!(vm.run_bounded(5).is_err());
    }

    #[test]
    fn critical_flags_are_recorded_and_imported_calls_are_rejected() {
        let mut code = Vec::new();
        opcode(&mut code, 0x8002);
        opcode(&mut code, 0x8003);
        opcode(&mut code, 0x8011);
        let mut vm = ScriptVm::new(fixture(&code));
        vm.step().unwrap();
        assert!(vm.critical_section());
        vm.step().unwrap();
        assert!(!vm.critical_section());
        vm.step().unwrap();
        assert!(vm.halted());

        let mut code = Vec::new();
        push_int(&mut code, 0);
        opcode(&mut code, 0x8005);
        let mut vm = ScriptVm::new(fixture_with_procedure(&code, 20, 0x04, 0));
        assert!(vm.run_bounded(5).is_err());
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
