//! Isolated, bounded first slice of CE's typed stack interpreter.
//!
//! Reference: pinned Fallout 1 CE src/int/intrpret.cc op_const,
//! op_pop, op_dup, op_swap, op_add, op_sub, op_mul, op_div,
//! op_negate, op_floor, op_jmp, op_stop_prog and op_exit_prog.
//!
//! The default interpreter is disconnected from gameplay, callbacks and player
//! input. A separate opt-in, bounded host path executes a limited native
//! variable/action/game-time subset with caller-supplied synthetic state.
//! Other opcodes error.
//! A narrow numeric, static-string, scheduling and stack/control subset is supported.
use crate::script_program::ScriptProgram;
use std::cmp::Ordering;
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

/// A discarded, candidate-only result of one bounded CE doEvents-style
/// condition evaluation. The actual installed interpreter is never updated.
/// Stack and procedure changes here are speculative and must not be applied
/// to game state without a separately verified commit boundary.
#[derive(Debug, Clone, PartialEq)]
pub struct PreviewedConditionFrame {
    pub satisfied: bool,
    pub data_stack: Vec<ScriptValue>,
    pub return_stack: Vec<ScriptValue>,
    pub frame_pointer: i32,
    pub base_pointer: i32,
    pub procedures: Vec<crate::script_program::ProcedureEntry>,
}

/// Discard-only CE doEvents candidate result. Earlier conditional stack
/// and scheduling effects are visible to later procedures in the clone.
/// No installed VM, MAP globals, or native game callbacks are modified.
#[derive(Debug, Clone, PartialEq)]
pub struct PreviewedDueProcedurePoll {
    pub ready: Vec<usize>,
    pub data_stack: Vec<ScriptValue>,
    pub return_stack: Vec<ScriptValue>,
    pub frame_pointer: i32,
    pub base_pointer: i32,
    pub procedures: Vec<crate::script_program::ProcedureEntry>,
}

#[derive(Debug, Clone)]
pub struct ScriptVm {
    pub program: ScriptProgram,
    // CE modifies procedure flag/time/condition fields at runtime.
    // Retain the decoded source table unchanged for read-only inspection.
    runtime_procedures: Vec<crate::script_program::ProcedureEntry>,
    clock_millis: i32,
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
        let runtime_procedures = program.procedures().to_vec();
        Self {
            program,
            runtime_procedures,
            clock_millis: 0,
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

    /// Deterministic CE timer snapshot in milliseconds; no wall clock used.
    pub fn set_clock_millis(&mut self, now: i32) {
        self.clock_millis = now;
    }

    /// Mutable interpreter view of CE procedure flags and scheduling fields.
    /// Reading this does not dispatch or run callbacks.
    pub fn runtime_procedures(&self) -> &[crate::script_program::ProcedureEntry] {
        &self.runtime_procedures
    }

    /// Poll CE doEvents' *timed* branch only, without dispatching callbacks.
    /// A conditional flag takes precedence over a timed flag. CE compares
    /// the signed event field as unsigned milliseconds with strict < time,
    /// and clears all flags before executing a due procedure; neither the
    /// stored event time nor the original INT bytes are cleared.
    pub fn take_due_timed_procedures(&mut self, now_millis: u32) -> Vec<usize> {
        let mut ready = Vec::new();
        for (index, entry) in self.runtime_procedures.iter_mut().enumerate() {
            if entry.flags & 0x02 != 0 {
                continue; // Conditional scripts need a VM condition evaluation.
            }
            if entry.flags & 0x01 != 0 && (entry.event_time as u32) < now_millis {
                entry.flags = 0;
                ready.push(index);
            }
        }
        ready
    }

    /// Poll both branches of pinned CE doEvents without executing a game
    /// procedure. A conditional flag always takes priority, even when the
    /// procedure is also timed and its scheduled time has elapsed.
    ///
    /// The caller evaluates each condition at its original INT byte offset.
    /// Some(true) means a nonzero condition result; Some(false) keeps the
    /// procedure armed; None means condition evaluation did not complete
    /// (CE PROGRAM_FLAG_0x04), so it must remain armed. No callbacks run for
    /// purely timed procedures. All flag updates are applied only after
    /// successful evaluation of the whole poll, preventing partial state
    /// changes if the host reports an error. Actual script execution and
    /// CE's interpreter stack side effects are not implemented here.
    pub fn take_due_procedures_with(
        &mut self,
        now_millis: u32,
        mut evaluate_condition: impl FnMut(usize, i32) -> io::Result<Option<bool>>,
    ) -> io::Result<Vec<usize>> {
        let mut ready = Vec::new();
        for (index, procedure) in self.runtime_procedures.iter().enumerate() {
            if procedure.flags & 0x02 != 0 {
                if evaluate_condition(index, procedure.condition_offset)? == Some(true) {
                    ready.push(index);
                }
            } else if procedure.flags & 0x01 != 0 && (procedure.event_time as u32) < now_millis {
                ready.push(index);
            }
        }
        for index in &ready {
            self.runtime_procedures[*index].flags = 0;
        }
        Ok(ready)
    }

    /// Consume one ready CE procedure, preserving the original engine's
    /// per-entry condition precedence, strict unsigned time comparison and
    /// immediate flag clearing before dispatch. The evaluator is supplied
    /// by the host and no procedure bytecode runs here. A host error leaves
    /// this entry armed (the Rust diagnostic safety choice).
    pub fn consume_due_procedure_with(
        &mut self,
        procedure_index: usize,
        now_millis: u32,
        evaluate_condition: impl FnOnce(i32) -> io::Result<Option<bool>>,
    ) -> io::Result<bool> {
        let entry = self
            .runtime_procedures
            .get(procedure_index)
            .copied()
            .ok_or_else(|| invalid("CE procedure index outside event table"))?;
        let ready = if entry.flags & 0x02 != 0 {
            evaluate_condition(entry.condition_offset)? == Some(true)
        } else {
            entry.flags & 0x01 != 0 && (entry.event_time as u32) < now_millis
        };
        if ready {
            self.runtime_procedures[procedure_index].flags = 0;
        }
        Ok(ready)
    }

    /// CE op_equal/op_not_equal/op_less and related comparisons: for any
    /// original static string, compare zero-terminated 8-bit text with
    /// strcmp ordering, not Unicode conversion or numeric coercion.
    /// Numeric operands are rendered as CE's decimal integer or %.5f text.
    /// Dynamic string heap handles remain explicitly unimplemented.
    fn compare(&self, left: ScriptValue, right: ScriptValue, op: u16) -> io::Result<bool> {
        if matches!(left, ScriptValue::StaticString(_))
            || matches!(right, ScriptValue::StaticString(_))
        {
            let to_bytes = |value| -> io::Result<Vec<u8>> {
                match value {
                    ScriptValue::StaticString(offset) => {
                        let offset = usize::try_from(offset)
                            .map_err(|_| invalid("negative CE static string offset"))?;
                        Ok(self.program.static_string(offset)?.to_vec())
                    }
                    ScriptValue::Int(number) => Ok(number.to_string().into_bytes()),
                    ScriptValue::Float(number) if number.is_finite() => {
                        Ok(format!("{number:.5}").into_bytes())
                    }
                    ScriptValue::DynamicString(_) => {
                        Err(invalid("CE dynamic script strings not implemented"))
                    }
                    _ => Err(invalid("unsupported CE string comparison operand")),
                }
            };
            let comparison = to_bytes(left)?.cmp(&to_bytes(right)?);
            return Ok(match op {
                0x33 => comparison == Ordering::Equal,
                0x34 => comparison != Ordering::Equal,
                0x35 => comparison != Ordering::Greater,
                0x36 => comparison != Ordering::Less,
                0x37 => comparison == Ordering::Less,
                0x38 => comparison == Ordering::Greater,
                _ => return Err(invalid("unsupported CE string comparison")),
            });
        }
        if let (ScriptValue::Pointer(a), ScriptValue::Pointer(b))
        | (ScriptValue::Pointer(a), ScriptValue::Int(b))
        | (ScriptValue::Int(a), ScriptValue::Pointer(b)) = (left, right)
        {
            return match op {
                0x33 => Ok(a == b),
                0x34 => Ok(a != b),
                _ => Err(invalid("unsupported CE pointer ordering")),
            };
        }
        numeric_comparison(left, right, op)
    }

    /// Isolated diagnostic for invented CE condition bytecode that uses
    /// only the currently supported VM opcodes and ends in STOP/EXIT.
    /// The final stack value must be an integer, exactly as doEvents reads
    /// the condition result. No shared stacks or world state are mutated.
    /// This deliberately does NOT model CE interpreter flag restoration,
    /// external opcodes, or the side effects of real game conditions.
    pub fn preview_condition_bounded(
        &self,
        condition_offset: i32,
        max_steps: usize,
    ) -> io::Result<bool> {
        self.preview_condition_with_host_bounded(condition_offset, max_steps, &mut |_, _| {
            Err(invalid("CE condition native host not provided"))
        })
    }

    /// Evaluate an INT condition on a detached VM with a strict instruction
    /// budget and an EXPLICIT native-opcode adapter. Original CE conditional
    /// expressions can contain game-variable and event reads, so the
    /// core-only preview above intentionally rejects them. This method does
    /// not install, consume or dispatch a pending CE procedure.
    ///
    /// The cloned condition retains the installed data/return stacks and
    /// frame/base pointers, as in pinned CE doEvents. Its saved program
    /// counter and flags remain untouched; only the clone enters the
    /// condition. This is not a full CE event lifecycle: a real CE condition
    /// can leave interpreter-side stack effects behind after its result is
    /// popped, whereas this diagnostic intentionally discards those effects.
    ///
    /// Only the VM is cloned. Callers requiring a read-only preview MUST
    /// supply a pure, read-only native adapter: arbitrary host side effects
    /// cannot be rolled back by the interpreter. Unsupported opcodes, bad
    /// offsets and unfinished conditions fail closed.
    pub fn preview_condition_with_host_bounded(
        &self,
        condition_offset: i32,
        max_steps: usize,
        native: &mut impl FnMut(u16, &mut Vec<ScriptValue>) -> io::Result<()>,
    ) -> io::Result<bool> {
        self.preview_condition_frame_with_host_bounded(condition_offset, max_steps, native)
            .map(|frame| frame.satisfied)
    }

    /// Inspect the candidate interpreter frame *after* CE condition
    /// evaluation and its integer result pop. Unlike doEvents, this never
    /// commits the resulting operand/return stacks, frame/base pointers or
    /// procedure scheduling flags. Callers may compare the hypothetical
    /// effects while preserving the installed interpreter unchanged.
    pub fn preview_condition_frame_with_host_bounded(
        &self,
        condition_offset: i32,
        max_steps: usize,
        native: &mut impl FnMut(u16, &mut Vec<ScriptValue>) -> io::Result<()>,
    ) -> io::Result<PreviewedConditionFrame> {
        let offset = usize::try_from(condition_offset)
            .map_err(|_| invalid("negative CE condition bytecode offset"))?;
        let first_opcode = self.program.word_at(offset)?;
        if first_opcode & 0x8000 == 0 {
            return Err(invalid("invalid CE condition opcode marker"));
        }
        // Pinned CE doEvents temporarily changes instructionPointer and
        // clears program flags, but it does NOT reset either stack or the
        // frame/base pointers. Preserve that installed interpreter context
        // inside a CLONE instead of inventing a fresh condition stack.
        let mut probe = self.clone();
        probe.pc = offset;
        probe.critical_section = false;
        probe.halted = false;
        probe.run_variable_subset_bounded(max_steps, native)?;
        let satisfied = probe.pop_int()? != 0;
        Ok(PreviewedConditionFrame {
            satisfied,
            data_stack: probe.stack,
            return_stack: probe.return_stack,
            frame_pointer: probe.frame_pointer,
            base_pointer: probe.base_pointer,
            procedures: probe.runtime_procedures,
        })
    }

    /// Inspect a CE doEvents pass on ONE speculative interpreter clone.
    ///
    /// This follows the pinned interpreter's ascending procedure traversal.
    /// A condition's data/return-stack, frame/base-pointer and scheduling
    /// changes survive inside the scratch VM so later condition evaluations
    /// can see them. The condition result is popped before the next entry.
    /// Procedure flags are cleared for ready entries exactly as CE does
    /// before executeProc, but NO actual procedure is executed or dispatched.
    /// Consequently this models only the pre-dispatch candidate selection,
    /// not later effects of executing a due script procedure.
    ///
    /// The native adapter must be read-only/pure: its external mutations
    /// cannot be rolled back. Errors discard the complete speculative poll;
    /// the installed VM stays unchanged in every case.
    pub fn preview_due_procedure_poll_with_host_bounded(
        &self,
        now_millis: u32,
        max_steps_per_condition: usize,
        native: &mut impl FnMut(u16, &mut Vec<ScriptValue>) -> io::Result<()>,
    ) -> io::Result<PreviewedDueProcedurePoll> {
        let mut scratch = self.clone();
        // CE uses the same timerFunc source in CALL_AT and doEvents. Only
        // the clone's clock is updated, never the installed interpreter.
        scratch.clock_millis = now_millis as i32;
        let mut ready = Vec::new();
        for index in 0..scratch.runtime_procedures.len() {
            let procedure = scratch.runtime_procedures[index];
            if procedure.flags & 0x02 != 0 {
                let frame = scratch.preview_condition_frame_with_host_bounded(
                    procedure.condition_offset,
                    max_steps_per_condition,
                    native,
                )?;
                scratch.stack = frame.data_stack;
                scratch.return_stack = frame.return_stack;
                scratch.frame_pointer = frame.frame_pointer;
                scratch.base_pointer = frame.base_pointer;
                scratch.runtime_procedures = frame.procedures;
                if frame.satisfied {
                    scratch.runtime_procedures[index].flags = 0;
                    ready.push(index);
                }
            } else if procedure.flags & 0x01 != 0
                && (procedure.event_time as u32) < now_millis
            {
                scratch.runtime_procedures[index].flags = 0;
                ready.push(index);
            }
        }
        Ok(PreviewedDueProcedurePoll {
            ready,
            data_stack: scratch.stack,
            return_stack: scratch.return_stack,
            frame_pointer: scratch.frame_pointer,
            base_pointer: scratch.base_pointer,
            procedures: scratch.runtime_procedures,
        })
    }

    /// Convenience candidate-only ready indices from the sequential CE
    /// condition poll above. Does not consume a real procedure or callback.
    pub fn preview_due_procedures_with_host_bounded(
        &self,
        now_millis: u32,
        max_steps_per_condition: usize,
        native: &mut impl FnMut(u16, &mut Vec<ScriptValue>) -> io::Result<()>,
    ) -> io::Result<Vec<usize>> {
        self.preview_due_procedure_poll_with_host_bounded(
            now_millis,
            max_steps_per_condition,
            native,
        )
        .map(|poll| poll.ready)
    }

    /// Synthetic-only end-to-end binding of the bounded condition preview
    /// to the original CE doEvents precedence. Each condition is evaluated
    /// independently from the same pre-poll VM snapshot, so this is not yet
    /// the complete mutable CE interpreter/event callback lifecycle.
    pub fn take_due_procedures_preview(
        &mut self,
        now_millis: u32,
        max_steps_per_condition: usize,
    ) -> io::Result<Vec<usize>> {
        let snapshot = self.clone();
        self.take_due_procedures_with(now_millis, |_, condition_offset| {
            snapshot
                .preview_condition_bounded(condition_offset, max_steps_per_condition)
                .map(Some)
        })
    }

    /// Preserve the core-only CE local interrupt preview, never supplying an
    /// implicit game-native host. The VM is cloned and never mutated.
    /// Bounded, clone-only preview of a local CE executeProc/setupCall
    /// procedure body. The real CE interpreter uses return marker 20 and
    /// ends local calls with POP_FLAGS_EXIT (0x8021) or
    /// POP_FLAGS_RETURN_VAL_EXIT (0x8025). This permits supported core
    /// opcodes only: every game-native operation, external call, wait and
    /// side effect outside the cloned VM fails closed. The installed VM,
    /// MAP, quest globals, event state and script lifecycle are unchanged.
    ///
    /// A returned None means the original local procedure completed without
    /// a return value; Some(value) corresponds to 0x8025. This does NOT
    /// dispatch a game script or model the first-load runProgram path.
    pub fn preview_local_action_core_bounded(
        &self,
        procedure_index: usize,
        max_steps: usize,
    ) -> io::Result<Option<ScriptValue>> {
        self.preview_local_action_with_host_bounded(procedure_index, max_steps, &mut |_, _| {
            Err(invalid("CE local action native host not provided"))
        })
    }

    /// Explicit synthetic local CE action preview with caller-supplied
    /// opcode handling. The program VM itself is cloned, but this callback
    /// may have external side effects: callers must use pure read-only
    /// native hosts when requesting a side-effect-free preview. Imported
    /// procedures and unsupported core opcodes still fail closed.
    pub fn preview_local_action_with_host_bounded(
        &self,
        procedure_index: usize,
        max_steps: usize,
        native: &mut impl FnMut(u16, &mut Vec<ScriptValue>) -> io::Result<()>,
    ) -> io::Result<Option<ScriptValue>> {
        let procedure = self
            .runtime_procedures
            .get(procedure_index)
            .copied()
            .ok_or_else(|| invalid("CE local action procedure index outside table"))?;
        if procedure.flags & 0x04 != 0 {
            return Err(invalid("CE imported local action is not supported"));
        }
        if procedure.argument_count != 0 {
            return Err(invalid("CE local action arguments are not supported"));
        }
        let body = usize::try_from(procedure.body_offset)
            .map_err(|_| invalid("CE local action has negative body offset"))?;
        self.program.word_at(body)?;
        let mut probe = self.clone();
        let original_pc =
            i32::try_from(probe.pc).map_err(|_| invalid("CE saved action PC exceeds i32"))?;
        probe.push_return(ScriptValue::Int(original_pc))?;
        probe.push_return(ScriptValue::Int(20))?;
        let flags = if probe.critical_section { 0x80 } else { 0 };
        probe.push(ScriptValue::Int(flags))?;
        probe.push(ScriptValue::Pointer(0))?;
        probe.push(ScriptValue::Int(0))?; // synthetic window ID
        probe.push(ScriptValue::Int(0))?; // CE setupCall argument marker
        probe.critical_section = procedure.flags & 0x10 != 0;
        probe.halted = false;
        probe.pc = body;

        for _ in 0..max_steps {
            let opcode = probe.program.word_at(probe.pc)?;
            if opcode == 0x8021 || opcode == 0x8025 {
                let returned = if opcode == 0x8025 {
                    Some(probe.pop()?)
                } else {
                    None
                };
                // setupCall's no-argument marker must have been consumed
                // by the script before the return operation.
                if probe.pop()? != ScriptValue::Int(0) || probe.pop()? != ScriptValue::Pointer(0) {
                    return Err(invalid("CE local action host frame is not balanced"));
                }
                let _saved_flags = probe.pop_int()?;
                if probe.pop_return_int()? != 20 {
                    return Err(invalid("CE local action return marker mismatch"));
                }
                if probe.return_stack.last() != Some(&ScriptValue::Int(original_pc)) {
                    return Err(invalid("CE local action saved PC was modified"));
                }
                return Ok(returned);
            }
            probe.step_with_variable_host(native)?;
            if probe.halted {
                return Err(invalid("CE local action stopped without a return"));
            }
        }
        Err(invalid(
            "CE local action preview instruction budget exhausted",
        ))
    }

    pub fn preview_local_interrupt_bounded(
        &self,
        procedure_index: usize,
        max_steps: usize,
    ) -> io::Result<ScriptValue> {
        self.preview_local_interrupt_with_host_bounded(procedure_index, max_steps, &mut |_, _| {
            Err(invalid("CE interrupt native host not provided"))
        })
    }

    /// Explicit-host, synthetic-only local CE interrupt procedure preview. Mirrors the
    /// CE setupCallWithReturnVal/setupCall data and return stack layout for
    /// executeProc (return address 20), then handles 0x8025 on that isolated
    /// invocation. Supported core opcodes and a caller-gated native subset can
    /// execute. Native host side effects are NOT rolled back when the probe
    /// fails; use a read-only host for a genuinely side-effect-free preview.
    /// Imported procedures, external variables, callback pointers,
    /// suspended programs, or a full game script's persistent VM state.
    pub fn preview_local_interrupt_with_host_bounded(
        &self,
        procedure_index: usize,
        max_steps: usize,
        native: &mut impl FnMut(u16, &mut Vec<ScriptValue>) -> io::Result<()>,
    ) -> io::Result<ScriptValue> {
        let procedure = self
            .runtime_procedures
            .get(procedure_index)
            .copied()
            .ok_or_else(|| invalid("CE local interrupt procedure index outside table"))?;
        if procedure.flags & 0x04 != 0 {
            return Err(invalid("CE imported interrupt procedure not supported"));
        }
        if procedure.argument_count != 0 {
            return Err(invalid("CE local interrupt arguments not supported"));
        }
        let mut probe = self.clone();
        let old_pc = i32::try_from(probe.pc).map_err(|_| invalid("CE return PC exceeds i32"))?;
        let body = usize::try_from(procedure.body_offset)
            .map_err(|_| invalid("negative CE interrupt body offset"))?;
        probe.program.word_at(body)?;
        probe.push_return(ScriptValue::Int(old_pc))?;
        probe.push_return(ScriptValue::Int(20))?;
        // The sandbox has no wait callback or window; these CE frame fields
        // remain correctly typed null/zero placeholders, never host pointers.
        let saved_flags = if probe.critical_section { 0x80 } else { 0 };
        probe.push(ScriptValue::Int(saved_flags))?;
        probe.push(ScriptValue::Pointer(0))?;
        probe.push(ScriptValue::Int(0))?;
        probe.push(ScriptValue::Int(0))?; // setupCall's no-argument marker
        probe.critical_section = procedure.flags & 0x10 != 0;
        probe.halted = false;
        probe.pc = body;
        for _ in 0..max_steps {
            let instruction = probe.program.word_at(probe.pc)?;
            if instruction == 0x8025 {
                // CE op_pop_flags_return_val_exit pops the return value,
                // window ID, checkWait pointer, flags, and return PC in order.
                let result = probe.pop()?;
                if probe.pop()? != ScriptValue::Int(0) || probe.pop()? != ScriptValue::Pointer(0) {
                    return Err(invalid("unsupported CE interrupt host frame"));
                }
                let flags = probe.pop_int()?;
                if probe.pop_return_int()? != 20 {
                    return Err(invalid("CE interrupt return address mismatch"));
                }
                probe.critical_section = flags & 0x80 != 0;
                // The saved original PC remains on CE's return stack until
                // the enclosing interpreter context handles it. Since this
                // is an isolated preview, the original VM stays unchanged.
                return Ok(result);
            }
            probe.step_with_variable_host(native)?;
            if probe.halted {
                return Err(invalid("CE interrupt stopped before return"));
            }
        }
        Err(invalid("CE interrupt instruction budget exhausted"))
    }

    fn procedure_at(&self, index: i32) -> io::Result<crate::script_program::ProcedureEntry> {
        let index = usize::try_from(index).map_err(|_| invalid("negative CE procedure index"))?;
        self.runtime_procedures
            .get(index)
            .copied()
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
                    return Err(invalid(
                        "CE imported/external procedure calls not implemented",
                    ));
                }
                self.jump(procedure.body_offset)?;
                if procedure.flags & 0x10 != 0 {
                    self.critical_section = true;
                }
            }
            0x06 => {
                // CALL_AT: top of stack is procedure index, beneath it the
                // delay in seconds. Original CE stores clock + 1000*delay.
                let index = self.pop_int()?;
                let seconds = self.pop_int()?;
                let index = usize::try_from(index)
                    .map_err(|_| invalid("negative CE scheduled procedure index"))?;
                let procedure = self
                    .runtime_procedures
                    .get_mut(index)
                    .ok_or_else(|| invalid("CE scheduled procedure index out of bounds"))?;
                procedure.event_time = self.clock_millis.wrapping_add(seconds.wrapping_mul(1000));
                procedure.flags |= 0x01; // PROCEDURE_FLAG_TIMED
            }
            0x07 => {
                // CALL_WHEN: condition bytecode offset is stored verbatim.
                let index = self.pop_int()?;
                let condition = self.pop_int()?;
                let index = usize::try_from(index)
                    .map_err(|_| invalid("negative CE conditional procedure index"))?;
                let procedure = self
                    .runtime_procedures
                    .get_mut(index)
                    .ok_or_else(|| invalid("CE conditional procedure index out of bounds"))?;
                procedure.flags |= 0x02; // PROCEDURE_FLAG_CONDITIONAL
                procedure.condition_offset = condition;
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
            0x28 => {
                // CE op_lookup_string_proc searches index 1 onward, never
                // the main procedure at zero. Dynamic strings require the
                // interpreter's mutable string heap and remain unsupported.
                let name = match self.pop()? {
                    ScriptValue::StaticString(offset) => {
                        let offset = usize::try_from(offset)
                            .map_err(|_| invalid("negative CE static string offset"))?;
                        self.program.static_string(offset)?
                    }
                    ScriptValue::DynamicString(_) => {
                        return Err(invalid("CE dynamic script strings not implemented"));
                    }
                    _ => return Err(invalid("CE named procedure needs a string operand")),
                };
                let index = self
                    .program
                    .find_callable_procedure(name)?
                    .ok_or_else(|| invalid("CE named procedure not found"))?;
                let index =
                    i32::try_from(index).map_err(|_| invalid("CE procedure index exceeds i32"))?;
                self.push(ScriptValue::Int(index))?;
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
                let result = self.compare(left, right, opcode & 0x03FF)?;
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
            0x48 => {
                let index = self.pop_int()?;
                let index = usize::try_from(index)
                    .map_err(|_| invalid("negative CE cancel procedure index"))?;
                let procedure = self
                    .runtime_procedures
                    .get_mut(index)
                    .ok_or_else(|| invalid("CE cancel procedure index out of bounds"))?;
                procedure.flags = 0;
                procedure.event_time = 0;
                procedure.condition_offset = 0;
            }
            0x49 => {
                // CE CANCEL_ALL resets *all* flags, even import/export flags.
                for procedure in &mut self.runtime_procedures {
                    procedure.flags = 0;
                    procedure.event_time = 0;
                    procedure.condition_offset = 0;
                }
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

    /// Execute a CE core instruction or an explicitly allowlisted native
    /// opcode with a caller-supplied host. The host decides which subset
    /// has real detached state (variables, time, timers or object snapshots).
    ///
    /// The normal step() method NEVER invokes native host functions.
    /// The Rust host receives a detached operand-stack copy; rejected input
    /// preserves this VM's original PC and data stack. External mutable host
    /// state is not transactionally rolled back after callback failure.
    pub fn step_with_variable_host(
        &mut self,
        native: &mut impl FnMut(u16, &mut Vec<ScriptValue>) -> io::Result<()>,
    ) -> io::Result<()> {
        if self.halted {
            return Ok(());
        }
        let opcode = self.program.word_at(self.pc)?;
        if !matches!(
            opcode,
            0x80B4
                | 0x80B5
                | 0x80B9
                | 0x80BB
                | 0x80BC
                | 0x80BD
                | 0x80BE
                | 0x80C0
                | 0x80D4
                | 0x80EA
                | 0x80EB
                | 0x80EC
                | 0x80F0
                | 0x80F1
                | 0x80F2
                | 0x80F6
                | 0x80F7
                | 0x8100
                | 0x8149
        ) && !(0x80C1..=0x80C7).contains(&opcode)
            && !(0x8118..=0x8119).contains(&opcode)
            && opcode != 0x811B
        {
            return self.step();
        }
        let next_pc = self
            .pc
            .checked_add(2)
            .ok_or_else(|| invalid("CE native variable program counter overflow"))?;
        let mut detached = self.stack.clone();
        native(opcode, &mut detached)?;
        if detached.len() > MAX_STACK_VALUES {
            return Err(invalid("CE native variable result exceeds stack bound"));
        }
        self.stack = detached;
        self.pc = next_pc;
        Ok(())
    }

    /// Bounded, opt-in diagnostic execution of supported core and variable
    /// native instructions. Other CE game opcodes remain errors.
    pub fn run_variable_subset_bounded(
        &mut self,
        max_steps: usize,
        native: &mut impl FnMut(u16, &mut Vec<ScriptValue>) -> io::Result<()>,
    ) -> io::Result<usize> {
        for count in 0..max_steps {
            if self.halted {
                return Ok(count);
            }
            self.step_with_variable_host(native)?;
        }
        if self.halted {
            return Ok(max_steps);
        }
        Err(invalid("CE native variable instruction budget exhausted"))
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

    fn local_action_vm(instructions: &[u8], flags: i32, argument_count: i32) -> ScriptVm {
        assert!(instructions.len() <= 22);
        let mut bytes = vec![0_u8; 42];
        bytes[20..20 + instructions.len()].copy_from_slice(instructions);
        bytes.extend_from_slice(&1_i32.to_be_bytes());
        for field in [4_i32, flags, 0, 0, 20, argument_count] {
            bytes.extend_from_slice(&field.to_be_bytes());
        }
        bytes.extend_from_slice(&5_i32.to_be_bytes());
        bytes.extend_from_slice(b"main\0");
        bytes.extend_from_slice(&0_i32.to_be_bytes());
        ScriptVm::new(ScriptProgram::decode(&bytes).unwrap())
    }

    fn synthetic_due_vm(
        first: &[u8],
        second: &[u8],
        second_flags: i32,
    ) -> ScriptVm {
        assert!(first.len() <= 24);
        assert!(second.len() <= 12);
        let mut bytes = vec![0_u8; 42];
        bytes[..first.len()].copy_from_slice(first);
        bytes[28..28 + second.len()].copy_from_slice(second);
        bytes.extend_from_slice(&2_i32.to_be_bytes());
        for fields in [
            [4_i32, 0x02, 0, 0, 0, 0],
            [8_i32, second_flags, 0, 28, 28, 0],
        ] {
            for field in fields {
                bytes.extend_from_slice(&field.to_be_bytes());
            }
        }
        bytes.extend_from_slice(&8_i32.to_be_bytes());
        bytes.extend_from_slice(b"one\0two\0");
        bytes.extend_from_slice(&0_i32.to_be_bytes());
        ScriptVm::new(ScriptProgram::decode(&bytes).unwrap())
    }

    #[test]
    fn sequential_condition_poll_reuses_earlier_condition_stack_effects() {
        let mut first = Vec::new();
        push_int(&mut first, 77);
        push_int(&mut first, 0);
        opcode(&mut first, 0x8011); // false, leaves 77 on CE's operand stack
        let mut second = Vec::new();
        push_int(&mut second, 77);
        opcode(&mut second, 0x8033); // compare with retained 77
        opcode(&mut second, 0x8011);
        let vm = synthetic_due_vm(&first, &second, 0x02);
        let poll = vm
            .preview_due_procedure_poll_with_host_bounded(1000, 4, &mut |_, _| {
                Err(super::invalid("native game opcodes denied"))
            })
            .unwrap();
        assert_eq!(poll.ready, [1]);
        assert!(poll.data_stack.is_empty());
        assert!(poll.return_stack.is_empty());
        assert_eq!(poll.procedures[0].flags, 0x02);
        assert_eq!(poll.procedures[1].flags, 0);
        assert_eq!(vm.runtime_procedures()[0].flags, 0x02);
        assert_eq!(vm.runtime_procedures()[1].flags, 0x02);
        assert_eq!(vm.pc(), 0);
        assert!(vm.stack().is_empty());
        assert!(vm.return_stack().is_empty());
        assert_eq!(
            vm.preview_due_procedures_with_host_bounded(1000, 4, &mut |_, _| {
                Err(super::invalid("native game opcodes denied"))
            })
            .unwrap(),
            vec![1]
        );
    }

    #[test]
    fn sequential_condition_poll_sees_newly_scheduled_later_timed_procedure() {
        let mut first = Vec::new();
        push_int(&mut first, -1); // one second before this synthetic poll
        push_int(&mut first, 1); // procedure index
        opcode(&mut first, 0x8006); // CALL_AT
        push_int(&mut first, 0); // false condition
        opcode(&mut first, 0x8011);
        let vm = synthetic_due_vm(&first, &[0x80, 0x11], 0);
        let poll = vm
            .preview_due_procedure_poll_with_host_bounded(1001, 5, &mut |_, _| {
                Err(super::invalid("native game opcodes denied"))
            })
            .unwrap();
        assert_eq!(poll.ready, [1]);
        assert_eq!(poll.procedures[0].flags, 0x02);
        assert_eq!(poll.procedures[1].flags, 0); // consumed only in clone
        assert_eq!(poll.procedures[1].event_time, 1);
        assert_eq!(vm.runtime_procedures()[1].event_time, 0);
        assert_eq!(vm.runtime_procedures()[1].flags, 0);
    }

    #[test]
    fn sequential_condition_poll_discards_prior_stack_effects_on_error() {
        let mut first = Vec::new();
        push_int(&mut first, 77);
        push_int(&mut first, 0);
        opcode(&mut first, 0x8011);
        let vm = synthetic_due_vm(&first, &[0x80, 0xFC, 0x80, 0x11], 0x02);
        assert!(vm
            .preview_due_procedure_poll_with_host_bounded(1000, 3, &mut |_, _| {
                Err(super::invalid("unsupported CE gameplay mutation"))
            })
            .is_err());
        assert!(vm.stack().is_empty());
        assert!(vm.return_stack().is_empty());
        assert_eq!(vm.runtime_procedures()[0].flags, 0x02);
        assert_eq!(vm.runtime_procedures()[1].flags, 0x02);
    }

    #[test]
    fn bounded_ce_condition_accepts_explicit_pure_native_reads_only() {
        // One invented INT condition: MAP_GLOBAL[0] < 6. The adapter is a
        // caller-controlled read-only value; no real game data is accessed.
        let mut code = Vec::new();
        push_int(&mut code, 0);
        opcode(&mut code, 0x80C3); // CE map_var
        push_int(&mut code, 6);
        opcode(&mut code, 0x8037); // LESS_THAN
        opcode(&mut code, 0x8011); // STOP_PROGRAM
        let vm = ScriptVm::new(fixture(&code));
        for (map_value, expected) in [(5, true), (6, false), (-1, true)] {
            let mut calls = 0;
            let result = vm
                .preview_condition_with_host_bounded(0, 5, &mut |opcode, stack| {
                    assert_eq!(opcode, 0x80C3);
                    assert_eq!(stack.pop(), Some(ScriptValue::Int(0)));
                    calls += 1;
                    stack.push(ScriptValue::Int(map_value));
                    Ok(())
                })
                .unwrap();
            assert_eq!(result, expected);
            assert_eq!(calls, 1);
        }
        assert!(vm.preview_condition_bounded(0, 5).is_err());
        assert!(vm
            .preview_condition_with_host_bounded(0, 4, &mut |_, _| Ok(()))
            .is_err());
        assert!(vm
            .preview_condition_with_host_bounded(-1, 5, &mut |_, _| Ok(()))
            .is_err());
        assert_eq!(vm.pc(), 0);
        assert!(!vm.halted());
        assert!(vm.stack().is_empty());
        assert!(vm.return_stack().is_empty());

        // No gameplay writes can pass through a read-only native host.
        let mut writes = Vec::new();
        push_int(&mut writes, 7);
        push_int(&mut writes, 0);
        opcode(&mut writes, 0x80C4); // CE set_map_var
        opcode(&mut writes, 0x8011);
        let write_vm = ScriptVm::new(fixture(&writes));
        assert!(write_vm
            .preview_condition_with_host_bounded(0, 4, &mut |_, _| {
                Err(super::invalid(
                    "read-only CE condition forbids native writes",
                ))
            })
            .is_err());
        assert_eq!(write_vm.pc(), 0);
        assert!(write_vm.stack().is_empty());
    }

    #[test]
    fn ce_condition_frame_preview_reports_discarded_stack_mutations() {
        let mut bytes = vec![0_u8; 42];
        let mut startup = Vec::new();
        push_int(&mut startup, 9);
        opcode(&mut startup, 0x8011);
        bytes[..startup.len()].copy_from_slice(&startup);

        // CE pops only the result (1). A newly pushed 77 would remain in
        // the running interpreter's stack, but never in the installed Rust
        // VM because this diagnostic is intentionally discard-only.
        let mut condition = Vec::new();
        push_int(&mut condition, 77);
        push_int(&mut condition, 1);
        opcode(&mut condition, 0x8011);
        bytes[20..20 + condition.len()].copy_from_slice(&condition);
        let mut vm = ScriptVm::new(fixture(&bytes));
        vm.run_bounded(2).unwrap();

        let candidate = vm
            .preview_condition_frame_with_host_bounded(20, 3, &mut |_, _| {
                Err(super::invalid("CE game native not supported in condition"))
            })
            .unwrap();
        assert!(candidate.satisfied);
        assert_eq!(
            candidate.data_stack,
            [ScriptValue::Int(9), ScriptValue::Int(77)]
        );
        assert!(candidate.return_stack.is_empty());
        assert_eq!(candidate.frame_pointer, -1);
        assert_eq!(candidate.base_pointer, -1);
        assert!(candidate.procedures.is_empty());
        assert!(vm.preview_condition_bounded(20, 3).unwrap());
        assert_eq!(vm.stack(), [ScriptValue::Int(9)]);
        assert_eq!(vm.pc(), 8);
        assert!(vm.halted());
        assert!(vm.preview_condition_bounded(20, 2).is_err());
        assert_eq!(vm.stack(), [ScriptValue::Int(9)]);
    }

    #[test]
    fn ce_condition_frame_previews_scheduled_procedure_changes_without_commit() {
        // CALL_AT (from original CE interpreter) changes a procedure's
        // scheduled time and flag. The candidate reports the modification;
        // the installed VM and original decoded INT table remain unchanged.
        let mut bytes = vec![0_u8; 42];
        let mut condition = Vec::new();
        push_int(&mut condition, 7); // delay seconds
        push_int(&mut condition, 0); // procedure index
        opcode(&mut condition, 0x8006); // CALL_AT
        push_int(&mut condition, 1); // successful condition result
        opcode(&mut condition, 0x8011); // STOP_PROGRAM
        assert_eq!(condition.len(), 22);
        bytes[20..42].copy_from_slice(&condition);
        bytes.extend_from_slice(&1_i32.to_be_bytes());
        for field in [4_i32, 0, 0, 0, 20, 0] {
            bytes.extend_from_slice(&field.to_be_bytes());
        }
        bytes.extend_from_slice(&5_i32.to_be_bytes());
        bytes.extend_from_slice(b"main\0");
        bytes.extend_from_slice(&0_i32.to_be_bytes());
        let mut vm = ScriptVm::new(ScriptProgram::decode(&bytes).unwrap());
        vm.set_clock_millis(250);
        let candidate = vm
            .preview_condition_frame_with_host_bounded(20, 5, &mut |_, _| {
                Err(super::invalid("CE condition native host forbidden"))
            })
            .unwrap();
        assert!(candidate.satisfied);
        assert_eq!(candidate.procedures[0].flags & 0x01, 0x01);
        assert_eq!(candidate.procedures[0].event_time, 7250);
        assert_eq!(vm.runtime_procedures()[0].flags, 0);
        assert_eq!(vm.runtime_procedures()[0].event_time, 0);
        assert_eq!(vm.program.procedures()[0].flags, 0);
        assert_eq!(vm.pc(), 0);
        assert!(vm.stack().is_empty());
        assert!(vm.return_stack().is_empty());
    }

    #[test]
    fn ce_conditional_preview_preserves_existing_data_and_return_stacks() {
        // A running CE script has already pushed a persistent operand and
        // stopped. doEvents enters its condition without clearing that stack.
        let mut bytes = vec![0_u8; 42];
        let mut startup = Vec::new();
        push_int(&mut startup, 99);
        opcode(&mut startup, 0x8011); // STOP_PROGRAM
        bytes[..startup.len()].copy_from_slice(&startup);
        let mut matches = Vec::new();
        push_int(&mut matches, 99);
        opcode(&mut matches, 0x8033); // EQUAL with saved operand
        opcode(&mut matches, 0x8011);
        bytes[20..20 + matches.len()].copy_from_slice(&matches);
        let mut differs = Vec::new();
        push_int(&mut differs, 88);
        opcode(&mut differs, 0x8033);
        opcode(&mut differs, 0x8011);
        bytes[32..32 + differs.len()].copy_from_slice(&differs);
        let mut vm = ScriptVm::new(fixture(&bytes));
        vm.run_bounded(2).unwrap();
        assert_eq!(vm.stack(), [ScriptValue::Int(99)]);
        assert!(vm.halted());
        assert!(vm.preview_condition_bounded(20, 3).unwrap());
        assert!(!vm.preview_condition_bounded(32, 3).unwrap());
        assert!(vm.preview_condition_bounded(20, 2).is_err());
        assert_eq!(vm.stack(), [ScriptValue::Int(99)]);
        assert_eq!(vm.pc(), 8);
        assert!(vm.halted());

        // CE's condition also inherits the return stack. A_TO_D consumes
        // the return stack ONLY on the cloned preview, never in the VM.
        let mut bytes = vec![0_u8; 42];
        let mut startup = Vec::new();
        push_int(&mut startup, 7);
        opcode(&mut startup, 0x800D); // D_TO_A
        opcode(&mut startup, 0x8011);
        bytes[..startup.len()].copy_from_slice(&startup);
        let mut condition = Vec::new();
        opcode(&mut condition, 0x800C); // A_TO_D
        push_int(&mut condition, 7);
        opcode(&mut condition, 0x8033); // EQUAL
        opcode(&mut condition, 0x8011);
        bytes[20..20 + condition.len()].copy_from_slice(&condition);
        let mut vm = ScriptVm::new(fixture(&bytes));
        vm.run_bounded(3).unwrap();
        assert_eq!(vm.return_stack(), [ScriptValue::Int(7)]);
        assert!(vm.preview_condition_bounded(20, 4).unwrap());
        assert_eq!(vm.return_stack(), [ScriptValue::Int(7)]);
        assert!(vm.stack().is_empty());
        assert_eq!(vm.pc(), 10);
    }

    #[test]
    fn ce_conditional_preview_reuses_original_global_base_pointer() {
        // CE doEvents preserves the persistent interpreter base pointer.
        // FETCH_GLOBAL at relative index -1 reads the retained first slot.
        let mut bytes = vec![0_u8; 42];
        let mut startup = Vec::new();
        push_int(&mut startup, 41);
        opcode(&mut startup, 0x802C); // SET_GLOBAL: base pointer = 1
        push_int(&mut startup, 5);
        opcode(&mut startup, 0x8011);
        bytes[..startup.len()].copy_from_slice(&startup);
        let mut condition = Vec::new();
        push_int(&mut condition, -1);
        opcode(&mut condition, 0x8012); // FETCH_GLOBAL base - 1
        push_int(&mut condition, 41);
        opcode(&mut condition, 0x8033);
        opcode(&mut condition, 0x8011);
        bytes[20..20 + condition.len()].copy_from_slice(&condition);
        let mut vm = ScriptVm::new(fixture(&bytes));
        vm.run_bounded(4).unwrap();
        assert_eq!(vm.base_pointer(), 1);
        assert_eq!(vm.stack(), [ScriptValue::Int(41), ScriptValue::Int(5)]);
        assert!(vm.preview_condition_bounded(20, 5).unwrap());
        assert_eq!(vm.base_pointer(), 1);
        assert_eq!(vm.stack(), [ScriptValue::Int(41), ScriptValue::Int(5)]);
        assert_eq!(vm.pc(), 16);
        assert!(vm.halted());
    }

    #[test]
    fn local_action_core_preview_exits_with_source_accurate_return_frame() {
        let instructions = [0x80_u8, 0x1A, 0x80, 0x21]; // POP; POP_FLAGS_EXIT
        let vm = local_action_vm(&instructions, 0x10, 0);
        assert_eq!(vm.preview_local_action_core_bounded(0, 4).unwrap(), None);
        assert_eq!(vm.pc(), 0);
        assert!(!vm.halted());
        assert!(vm.stack().is_empty());
        assert!(vm.return_stack().is_empty());
    }

    #[test]
    fn local_action_value_return_and_failure_are_fully_detached() {
        let mut code = vec![0x80, 0x1A, 0xC0, 0x01];
        code.extend_from_slice(&42_i32.to_be_bytes());
        code.extend_from_slice(&0x8025_u16.to_be_bytes());
        let vm = local_action_vm(&code, 0, 0);
        assert_eq!(
            vm.preview_local_action_core_bounded(0, 3).unwrap(),
            Some(ScriptValue::Int(42))
        );
        assert!(vm.preview_local_action_core_bounded(0, 2).is_err());
        assert!(vm.preview_local_action_core_bounded(9, 5).is_err());
        assert_eq!(vm.pc(), 0);
        assert!(vm.stack().is_empty());
        assert!(vm.return_stack().is_empty());

        let unbalanced = local_action_vm(&[0x80, 0x21], 0, 0);
        assert!(unbalanced.preview_local_action_core_bounded(0, 2).is_err());
        let halted = local_action_vm(&[0x80, 0x1A, 0x80, 0x11], 0, 0);
        assert!(halted.preview_local_action_core_bounded(0, 3).is_err());
        let native = local_action_vm(&[0x80, 0xBD, 0x80, 0x21], 0, 0);
        assert!(native.preview_local_action_core_bounded(0, 3).is_err());
        let imported = local_action_vm(&[0x80, 0x1A, 0x80, 0x21], 0x04, 0);
        assert!(imported.preview_local_action_core_bounded(0, 3).is_err());
        let args = local_action_vm(&[0x80, 0x1A, 0x80, 0x21], 0, 1);
        assert!(args.preview_local_action_core_bounded(0, 3).is_err());
    }

    #[test]
    fn local_action_explicit_native_host_runs_only_on_cloned_vm() {
        let code = [0x80, 0x1A, 0x80, 0xBD, 0x80, 0x25];
        let vm = local_action_vm(&code, 0, 0);
        let mut calls = 0;
        let result = vm
            .preview_local_action_with_host_bounded(0, 3, &mut |opcode, stack| {
                assert_eq!(opcode, 0x80BD);
                calls += 1;
                stack.push(ScriptValue::Pointer(17));
                Ok(())
            })
            .unwrap();
        assert_eq!(result, Some(ScriptValue::Pointer(17)));
        assert_eq!(calls, 1);
        assert!(vm.preview_local_action_core_bounded(0, 3).is_err());
        assert_eq!(vm.pc(), 0);
        assert!(vm.stack().is_empty());
        assert!(vm.return_stack().is_empty());
        assert!(!vm.halted());
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

    fn fixture_with_static_strings(instructions: &[u8], strings: &[u8]) -> ScriptProgram {
        assert!(instructions.len() <= 42);
        let mut bytes = vec![0_u8; 42];
        bytes[..instructions.len()].copy_from_slice(instructions);
        bytes.extend_from_slice(&0_i32.to_be_bytes()); // no procedures
        bytes.extend_from_slice(&0_i32.to_be_bytes()); // no identifiers
        bytes.extend_from_slice(&(strings.len() as i32).to_be_bytes());
        bytes.extend_from_slice(strings);
        ScriptProgram::decode(&bytes).unwrap()
    }

    fn push_static(code: &mut Vec<u8>, offset: i32) {
        opcode(code, 0x9001);
        code.extend_from_slice(&offset.to_be_bytes());
    }

    #[test]
    fn ce_static_string_comparisons_preserve_byte_order_and_case() {
        let mut code = Vec::new();
        push_static(&mut code, 0);
        push_static(&mut code, 6);
        opcode(&mut code, 0x8037); // alpha < beta
        opcode(&mut code, 0x8011);
        let mut vm = ScriptVm::new(fixture_with_static_strings(&code, b"alpha\0beta\0"));
        vm.run_bounded(8).unwrap();
        assert_eq!(vm.stack(), [ScriptValue::Int(1)]);

        let mut code = Vec::new();
        push_static(&mut code, 0);
        push_static(&mut code, 4);
        opcode(&mut code, 0x8033); // case-sensitive equality
        opcode(&mut code, 0x8011);
        let mut vm = ScriptVm::new(fixture_with_static_strings(&code, b"ABC\0abc\0"));
        vm.run_bounded(8).unwrap();
        assert_eq!(vm.stack(), [ScriptValue::Int(0)]);
    }

    #[test]
    fn ce_static_strings_compare_against_formatted_ints_and_floats() {
        let mut code = Vec::new();
        push_int(&mut code, 7);
        push_static(&mut code, 0);
        opcode(&mut code, 0x8033); // 7 == "7"
        opcode(&mut code, 0x8011);
        let mut vm = ScriptVm::new(fixture_with_static_strings(&code, b"7\0"));
        vm.run_bounded(8).unwrap();
        assert_eq!(vm.stack(), [ScriptValue::Int(1)]);

        let mut code = Vec::new();
        opcode(&mut code, 0xA001);
        code.extend_from_slice(&1.5_f32.to_bits().to_be_bytes());
        push_static(&mut code, 0);
        opcode(&mut code, 0x8033);
        opcode(&mut code, 0x8011);
        let mut vm = ScriptVm::new(fixture_with_static_strings(&code, b"1.50000\0"));
        vm.run_bounded(8).unwrap();
        assert_eq!(vm.stack(), [ScriptValue::Int(1)]);
    }

    #[test]
    fn ce_comparisons_reject_invalid_string_offsets_and_heap_handles() {
        let mut code = Vec::new();
        push_static(&mut code, -1);
        push_static(&mut code, 0);
        opcode(&mut code, 0x8033);
        let mut vm = ScriptVm::new(fixture_with_static_strings(&code, b"ok\0"));
        assert!(vm.run_bounded(8).is_err());

        let mut code = Vec::new();
        opcode(&mut code, 0x9801);
        code.extend_from_slice(&0_i32.to_be_bytes());
        push_static(&mut code, 0);
        opcode(&mut code, 0x8034);
        let mut vm = ScriptVm::new(fixture_with_static_strings(&code, b"ok\0"));
        assert!(vm.run_bounded(8).is_err());
    }

    #[test]
    fn ce_pointer_equals_null_integer_without_dereferencing() {
        let mut code = Vec::new();
        opcode(&mut code, 0xE001);
        code.extend_from_slice(&0_i32.to_be_bytes());
        push_int(&mut code, 0);
        opcode(&mut code, 0x8033);
        opcode(&mut code, 0x8011);
        let mut vm = ScriptVm::new(fixture(&code));
        vm.run_bounded(8).unwrap();
        assert_eq!(vm.stack(), [ScriptValue::Int(1)]);
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

    fn fixture_with_procedure(
        instructions: &[u8],
        body_offset: i32,
        flags: i32,
        args: i32,
    ) -> ScriptProgram {
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

    /// Invented INT layout with a main procedure, a second procedure, and
    /// one original static-string-table entry. No game assets are required.
    fn named_procedure_fixture(instructions: &[u8], query: &[u8]) -> ScriptProgram {
        assert!(instructions.len() <= 42);
        let mut bytes = vec![0_u8; 42];
        bytes[..instructions.len()].copy_from_slice(instructions);
        bytes.extend_from_slice(&2_i32.to_be_bytes());
        let first = b"main\0";
        let second = b"start\0";
        for (offset, body) in [(4_i32, 18_i32), (4 + first.len() as i32, 26)] {
            for field in [offset, 0, 0, 0, body, 0] {
                bytes.extend_from_slice(&field.to_be_bytes());
            }
        }
        bytes.extend_from_slice(&((first.len() + second.len()) as i32).to_be_bytes());
        bytes.extend_from_slice(first);
        bytes.extend_from_slice(second);
        bytes.extend_from_slice(&((query.len() + 1) as i32).to_be_bytes());
        bytes.extend_from_slice(query);
        bytes.push(0);
        ScriptProgram::decode(&bytes).unwrap()
    }

    #[test]
    fn ce_lookup_procedure_opcode_uses_static_strings_and_skips_main() {
        let mut code = Vec::new();
        opcode(&mut code, 0x9001); // PUSH static string pool offset
        code.extend_from_slice(&0_i32.to_be_bytes());
        opcode(&mut code, 0x8028); // LOOKUP_PROCEDURE_BY_NAME
        opcode(&mut code, 0x8011); // STOP
        let mut vm = ScriptVm::new(named_procedure_fixture(&code, b"StArT"));
        assert_eq!(vm.run_bounded(8).unwrap(), 3);
        assert_eq!(vm.stack(), [ScriptValue::Int(1)]);
        let mut vm = ScriptVm::new(named_procedure_fixture(&code, b"main"));
        assert!(vm.run_bounded(8).is_err());
    }

    #[test]
    fn ce_lookup_procedure_opcode_rejects_unsupported_or_bad_strings() {
        let mut code = Vec::new();
        opcode(&mut code, 0x9801); // PUSH dynamic string handle
        code.extend_from_slice(&0_i32.to_be_bytes());
        opcode(&mut code, 0x8028);
        let mut vm = ScriptVm::new(named_procedure_fixture(&code, b"start"));
        assert!(vm.run_bounded(4).is_err());

        let mut code = Vec::new();
        opcode(&mut code, 0x9001);
        code.extend_from_slice(&(-1_i32).to_be_bytes());
        opcode(&mut code, 0x8028);
        let mut vm = ScriptVm::new(named_procedure_fixture(&code, b"start"));
        assert!(vm.run_bounded(4).is_err());

        let mut code = Vec::new();
        push_int(&mut code, 0);
        opcode(&mut code, 0x8028);
        let mut vm = ScriptVm::new(named_procedure_fixture(&code, b"start"));
        assert!(vm.run_bounded(4).is_err());
    }

    #[test]
    fn ce_call_at_and_call_when_mutate_only_runtime_procedure_metadata() {
        let mut timed = Vec::new();
        push_int(&mut timed, 2); // CE seconds of delay
        push_int(&mut timed, 0); // CE procedure index is popped first
        opcode(&mut timed, 0x8006);
        opcode(&mut timed, 0x8011);
        let mut vm = ScriptVm::new(fixture_with_procedure(&timed, 18, 0x08, 0));
        vm.set_clock_millis(750);
        vm.run_bounded(8).unwrap();
        let original = vm.program.procedures()[0];
        assert_eq!(original.flags, 0x08);
        assert_eq!(original.event_time, 0);
        let pending = vm.runtime_procedures()[0];
        assert_eq!(pending.flags, 0x09);
        assert_eq!(pending.event_time, 2750);
        assert_eq!(pending.condition_offset, 0);

        let mut conditional = Vec::new();
        push_int(&mut conditional, 123); // condition bytecode offset
        push_int(&mut conditional, 0);
        opcode(&mut conditional, 0x8007);
        opcode(&mut conditional, 0x8011);
        let mut vm = ScriptVm::new(fixture_with_procedure(&conditional, 18, 0x10, 0));
        vm.run_bounded(8).unwrap();
        assert_eq!(vm.runtime_procedures()[0].flags, 0x12);
        assert_eq!(vm.runtime_procedures()[0].condition_offset, 123);
        assert_eq!(vm.program.procedures()[0].flags, 0x10);
    }

    #[test]
    fn ce_cancel_and_cancel_all_clear_flags_time_and_condition() {
        let mut code = Vec::new();
        push_int(&mut code, 10);
        push_int(&mut code, 0);
        opcode(&mut code, 0x8006);
        push_int(&mut code, 0);
        opcode(&mut code, 0x8048);
        opcode(&mut code, 0x8011);
        let mut vm = ScriptVm::new(fixture_with_procedure(&code, 18, 0x08, 0));
        vm.run_bounded(12).unwrap();
        assert_eq!(vm.runtime_procedures()[0].flags, 0);
        assert_eq!(vm.runtime_procedures()[0].event_time, 0);
        assert_eq!(vm.runtime_procedures()[0].condition_offset, 0);
        assert_eq!(vm.program.procedures()[0].flags, 0x08);

        let mut vm = ScriptVm::new(fixture_with_procedure(
            &[0x80, 0x49, 0x80, 0x11],
            18,
            0x04,
            0,
        ));
        vm.run_bounded(4).unwrap();
        assert_eq!(vm.runtime_procedures()[0].flags, 0);
        assert_eq!(vm.program.procedures()[0].flags, 0x04);
    }

    #[test]
    fn ce_timed_event_poll_requires_strictly_later_unsigned_time() {
        let mut code = Vec::new();
        push_int(&mut code, 2);
        push_int(&mut code, 0);
        opcode(&mut code, 0x8006);
        opcode(&mut code, 0x8011);
        let mut vm = ScriptVm::new(fixture_with_procedure(&code, 18, 0x08, 0));
        vm.set_clock_millis(750);
        vm.run_bounded(8).unwrap();
        assert!(vm.take_due_timed_procedures(2750).is_empty());
        assert_eq!(vm.runtime_procedures()[0].flags, 0x09);
        assert_eq!(vm.take_due_timed_procedures(2751), [0]);
        assert_eq!(vm.runtime_procedures()[0].flags, 0);
        assert_eq!(vm.runtime_procedures()[0].event_time, 2750);
        assert!(vm.take_due_timed_procedures(5000).is_empty());
        assert_eq!(vm.program.procedures()[0].flags, 0x08);
    }

    #[test]
    fn ce_timed_event_poll_defers_conditional_flags_and_uses_unsigned_compare() {
        let mut code = Vec::new();
        push_int(&mut code, -1);
        push_int(&mut code, 0);
        opcode(&mut code, 0x8006);
        push_int(&mut code, 12);
        push_int(&mut code, 0);
        opcode(&mut code, 0x8007);
        opcode(&mut code, 0x8011);
        let mut vm = ScriptVm::new(fixture_with_procedure(&code, 18, 0, 0));
        vm.run_bounded(8).unwrap();
        assert_eq!(vm.runtime_procedures()[0].flags, 0x03);
        assert!(vm.take_due_timed_procedures(u32::MAX).is_empty());
        assert_eq!(vm.runtime_procedures()[0].condition_offset, 12);

        let mut code = Vec::new();
        push_int(&mut code, -1);
        push_int(&mut code, 0);
        opcode(&mut code, 0x8006);
        opcode(&mut code, 0x8011);
        let mut vm = ScriptVm::new(fixture_with_procedure(&code, 18, 0, 0));
        vm.run_bounded(8).unwrap();
        assert_eq!(vm.runtime_procedures()[0].event_time, -1000);
        assert!(vm.take_due_timed_procedures(10_000).is_empty());
        assert_eq!(vm.take_due_timed_procedures(u32::MAX), [0]);
    }

    #[test]
    fn ce_conditional_poll_has_priority_and_preserves_false_conditions() {
        let mut vm = ScriptVm::new(named_procedure_fixture(&[], b"condition"));
        vm.runtime_procedures[0].flags = 0x03; // conditional and timed
        vm.runtime_procedures[0].event_time = 10;
        vm.runtime_procedures[0].condition_offset = 77;
        vm.runtime_procedures[1].flags = 0x09; // exported and timed
        vm.runtime_procedures[1].event_time = 99;
        let original = vm.program.procedures().to_vec();
        let mut called = Vec::new();
        let ready = vm
            .take_due_procedures_with(100, |index, offset| {
                called.push((index, offset));
                Ok(Some(false))
            })
            .unwrap();
        assert_eq!(called, [(0, 77)]);
        assert_eq!(ready, [1]);
        assert_eq!(vm.runtime_procedures[0].flags, 0x03);
        assert_eq!(vm.runtime_procedures[1].flags, 0);
        assert_eq!(vm.runtime_procedures[1].event_time, 99);
        assert_eq!(vm.program.procedures(), original);

        assert!(vm
            .take_due_procedures_with(u32::MAX, |_, _| Ok(None))
            .unwrap()
            .is_empty());
        assert_eq!(vm.runtime_procedures[0].flags, 0x03);
        assert_eq!(
            vm.take_due_procedures_with(0, |_, offset| {
                assert_eq!(offset, 77);
                Ok(Some(true))
            })
            .unwrap(),
            [0]
        );
        assert_eq!(vm.runtime_procedures[0].flags, 0);
        assert_eq!(vm.runtime_procedures[0].condition_offset, 77);
        assert!(vm
            .take_due_procedures_with(u32::MAX, |_, _| Ok(Some(true)))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn ce_conditional_host_error_does_not_partially_consume_events() {
        let mut vm = ScriptVm::new(named_procedure_fixture(&[], b"unused"));
        vm.runtime_procedures[0].flags = 0x01;
        vm.runtime_procedures[0].event_time = 1;
        vm.runtime_procedures[1].flags = 0x02;
        vm.runtime_procedures[1].condition_offset = 24;
        assert!(vm
            .take_due_procedures_with(100, |_, _| {
                Err(std::io::Error::other(
                    "synthetic condition evaluator failed",
                ))
            })
            .is_err());
        assert_eq!(vm.runtime_procedures[0].flags, 0x01);
        assert_eq!(vm.runtime_procedures[1].flags, 0x02);
        assert_eq!(
            vm.take_due_procedures_with(100, |index, offset| {
                assert_eq!((index, offset), (1, 24));
                Ok(Some(true))
            })
            .unwrap(),
            [0, 1]
        );
        assert!(vm.runtime_procedures.iter().all(|entry| entry.flags == 0));
    }

    fn condition_fixture(result: i32) -> ScriptProgram {
        let mut code = vec![0x80, 0x11]; // STOP in unrelated main code
        code.resize(20, 0); // bytecode condition entry at absolute offset 20
        push_int(&mut code, result);
        opcode(&mut code, 0x8011);
        fixture_with_procedure(&code, 0, 0, 0)
    }

    #[test]
    fn ce_bounded_condition_preview_polls_true_and_false_without_mutating_vm() {
        for (value, expected) in [(0, false), (7, true), (-3, true)] {
            let mut vm = ScriptVm::new(condition_fixture(value));
            vm.runtime_procedures[0].flags = 0x03;
            vm.runtime_procedures[0].condition_offset = 20;
            vm.runtime_procedures[0].event_time = 5;
            assert_eq!(vm.preview_condition_bounded(20, 3).unwrap(), expected);
            assert_eq!(vm.pc(), 0);
            assert!(!vm.halted());
            assert!(vm.stack().is_empty());
            let result = vm.take_due_procedures_preview(100, 3).unwrap();
            assert_eq!(result, if expected { vec![0] } else { vec![] });
            assert_eq!(vm.runtime_procedures[0].flags, if expected { 0 } else { 3 });
            assert_eq!(vm.program.procedures()[0].flags, 0);
        }
    }

    #[test]
    fn ce_condition_preview_rejects_bad_offset_unknown_op_and_exhausted_budget() {
        let mut vm = ScriptVm::new(condition_fixture(1));
        vm.runtime_procedures[0].flags = 0x02;
        vm.runtime_procedures[0].condition_offset = -1;
        assert!(vm.take_due_procedures_preview(100, 8).is_err());
        assert_eq!(vm.runtime_procedures[0].flags, 0x02);
        vm.runtime_procedures[0].condition_offset = 20;
        assert!(vm.take_due_procedures_preview(100, 1).is_err());
        assert_eq!(vm.runtime_procedures[0].flags, 0x02);

        let mut code = vec![0x80, 0x11];
        code.resize(20, 0);
        opcode(&mut code, 0x804C); // unsupported opcode, not an engine callback
        let mut vm = ScriptVm::new(fixture_with_procedure(&code, 0, 0, 0));
        vm.runtime_procedures[0].flags = 0x02;
        vm.runtime_procedures[0].condition_offset = 20;
        assert!(vm.take_due_procedures_preview(100, 4).is_err());
        assert_eq!(vm.runtime_procedures[0].flags, 0x02);
    }

    fn interrupt_fixture(body: &[u8], flags: i32, argument_count: i32) -> ScriptProgram {
        let mut bytes = vec![0_u8; 42];
        // Keep main entry separate from procedure body, which starts at 20.
        assert!(body.len() <= 20);
        bytes[20..20 + body.len()].copy_from_slice(body);
        bytes.extend_from_slice(&1_i32.to_be_bytes());
        for field in [4_i32, flags, 0, 0, 20, argument_count] {
            bytes.extend_from_slice(&field.to_be_bytes());
        }
        bytes.extend_from_slice(&5_i32.to_be_bytes());
        bytes.extend_from_slice(b"main\0");
        bytes.extend_from_slice(&0_i32.to_be_bytes());
        ScriptProgram::decode(&bytes).unwrap()
    }

    #[test]
    fn ce_local_interrupt_preview_restores_isolated_stack_on_value_return() {
        let mut body = Vec::new();
        opcode(&mut body, 0x801A); // discard CE setupCall zero argument marker
        push_int(&mut body, 42);
        opcode(&mut body, 0x8025); // CE POP_FLAGS_RETURN_VAL_EXIT
        let mut vm = ScriptVm::new(interrupt_fixture(&body, 0x10, 0));
        vm.set_clock_millis(123);
        let previous = vm.clone();
        assert_eq!(
            vm.preview_local_interrupt_bounded(0, 4).unwrap(),
            ScriptValue::Int(42)
        );
        assert_eq!(vm.pc(), previous.pc());
        assert_eq!(vm.stack(), previous.stack());
        assert_eq!(vm.return_stack(), previous.return_stack());
        assert_eq!(vm.critical_section(), previous.critical_section());
        assert_eq!(vm.runtime_procedures(), previous.runtime_procedures());
        assert!(!vm.halted());
    }

    #[test]
    fn ce_local_interrupt_preview_rejects_imports_arguments_errors_and_budget() {
        let mut body = Vec::new();
        opcode(&mut body, 0x801A);
        push_int(&mut body, 42);
        opcode(&mut body, 0x8025);
        let vm = ScriptVm::new(interrupt_fixture(&body, 0, 0));
        assert!(vm.preview_local_interrupt_bounded(1, 4).is_err());
        assert!(vm.preview_local_interrupt_bounded(0, 2).is_err());
        let vm = ScriptVm::new(interrupt_fixture(&body, 0x04, 0));
        assert!(vm.preview_local_interrupt_bounded(0, 4).is_err());
        let vm = ScriptVm::new(interrupt_fixture(&body, 0, 1));
        assert!(vm.preview_local_interrupt_bounded(0, 4).is_err());
        let vm = ScriptVm::new(interrupt_fixture(&[0x80, 0x11], 0, 0));
        assert!(vm.preview_local_interrupt_bounded(0, 4).is_err());
        let vm = ScriptVm::new(interrupt_fixture(&[0x80, 0x4C], 0, 0));
        assert!(vm.preview_local_interrupt_bounded(0, 4).is_err());
    }

    #[test]
    fn ce_procedure_scheduling_rejects_invalid_indices_and_operand_types() {
        for opcode_id in [0x8006, 0x8007, 0x8048] {
            let mut code = Vec::new();
            if opcode_id != 0x8048 {
                push_int(&mut code, 3);
            }
            push_int(&mut code, -1);
            opcode(&mut code, opcode_id);
            let mut vm = ScriptVm::new(fixture_with_procedure(&code, 18, 0, 0));
            assert!(vm.run_bounded(8).is_err());
            assert_eq!(vm.runtime_procedures()[0].flags, 0);
        }
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
