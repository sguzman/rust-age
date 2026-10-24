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

/// CE source opcode termination is not just a boolean: EXIT and
/// EXIT_PROGRAM set EXITED (0x01), while STOP_PROGRAM sets STOPPED (0x08).
/// This reports only these modeled bits, not all original program flags
/// or actual parent/child lifecycle effects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptHaltReason {
    Exit,
    ExitProgram,
    StopProgram,
    /// CE POP_EXIT: return address from rstack, transient 0x40 flag.
    /// interpret clears 0x40 before returning; this does not purge
    /// the .INT program or set EXITED/STOPPED.
    ReturnFromCall,
}

impl ScriptHaltReason {
    pub fn ce_terminal_flag(self) -> u16 {
        match self {
            Self::Exit | Self::ExitProgram => 0x01,
            Self::StopProgram => 0x08,
            Self::ReturnFromCall => 0x40,
        }
    }
}

/// The modeled low CE flags and saved/restored program counters around one
/// successful doEvents conditional evaluation. Pinned CE saves flags/PC,
/// zeros flags and enters the condition, then restores saved flags/PC after
/// popping the condition result (unless PROGRAM_FLAG_0x04 interrupted it).
///
/// Only EXITED, STOPPED and CRITICAL_SECTION are currently represented.
/// Waiting, parent/child, interpreter error and upper opcode bits remain
/// outside this bounded model. A failed condition returns an error rather
/// than a misleading restoration trace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreviewedConditionFlagRestore {
    pub saved_pc: usize,
    pub evaluated_pc: usize,
    pub restored_pc: usize,
    pub saved_low_flags: u16,
    pub evaluated_low_flags: u16,
    pub restored_low_flags: u16,
}

/// A discarded, candidate-only result of one bounded CE doEvents-style
/// condition evaluation. The actual installed interpreter is never updated.
/// Stack and procedure changes here are speculative and must not be applied
/// to game state without a separately verified commit boundary.
#[derive(Debug, Clone, PartialEq)]
pub struct PreviewedConditionFrame {
    pub satisfied: bool,
    pub termination: ScriptHaltReason,
    pub flag_restore: PreviewedConditionFlagRestore,
    pub data_stack: Vec<ScriptValue>,
    pub return_stack: Vec<ScriptValue>,
    pub frame_pointer: i32,
    pub base_pointer: i32,
    pub procedures: Vec<crate::script_program::ProcedureEntry>,
}

/// A returned-only, discard-only post-call frame from CE's local
/// executeProc/setupCall protocol. This is the scratch VM AFTER the local
/// call finished by POP_FLAGS_EXIT/POP_FLAGS_RETURN_VAL_EXIT, including its
/// retained saved-PC return marker and hypothetical procedure scheduling.
///
/// Nothing in this structure may be applied to the installed VM: native
/// adapters MUST be pure/read-only and game dispatch is still disabled.
#[derive(Debug, Clone, PartialEq)]
pub struct PreviewedLocalActionVmFrame {
    pub returned: Option<ScriptValue>,
    pub program_counter: usize,
    pub saved_low_flags: u16,
    pub restored_low_flags: u16,
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
    pub condition_terminations: Vec<(usize, ScriptHaltReason)>,
    pub condition_flag_restorations: Vec<(usize, PreviewedConditionFlagRestore)>,
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
    // Pinned CE checkWait callback timing, gated to core tick only.
    core_wait: Option<(u32, u32)>,
    // Upper Program::flags word from CE interpret's last fetched opcode.
    // This is a trace, not an implementation of all lower lifecycle flags.
    last_opcode: Option<u16>,
    halted: bool,
    halt_reason: Option<ScriptHaltReason>,
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
            core_wait: None,
            last_opcode: None,
            halted: false,
            halt_reason: None,
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

    /// Termination kind from the last bounded CE core run, if any.
    /// Does not implement the original program's full flags lifecycle.
    pub fn halt_reason(&self) -> Option<ScriptHaltReason> {
        self.halt_reason
    }

    /// Complete one bounded interpret() burst after a POP_EXIT/POP_FLAGS_EXIT
    /// return. Pinned CE interpret clears transient PROGRAM_FLAG_0x40
    /// before restoring currentProgram and returning. Preserve EXITED and
    /// STOPPED halts; only ReturnFromCall is a resumable transient marker.
    pub(crate) fn finish_core_interpreter_burst(&mut self) {
        if self.halt_reason == Some(ScriptHaltReason::ReturnFromCall) {
            self.halted = false;
            self.halt_reason = None;
        }
    }

    /// Only the CE program flag bits modeled by this detached interpreter.
    /// This is NOT the full original Program::flags register.
    pub fn modeled_program_low_flags(&self) -> u16 {
        let mut flags = if self.critical_section { 0x80 } else { 0 };
        if self.core_wait.is_some() {
            flags |= 0x10; // PROGRAM_IS_WAITING, pinned intrpret.h
        }
        if let Some(reason) = self.halt_reason {
            // Original interpret clears PROGRAM_FLAG_0x40 after a
            // POP_EXIT return before the caller observes program flags.
            if reason != ScriptHaltReason::ReturnFromCall {
                flags |= reason.ce_terminal_flag();
            }
        }
        flags
    }

    /// The last successfully interpreted CE opcode, not a live host callback.
    /// Pinned interpret writes opcode << 16 on every bytecode dispatch.
    pub fn last_opcode(&self) -> Option<u16> {
        self.last_opcode
    }

    /// Source-shaped flags projection: last dispatched opcode in the high
    /// word and ONLY modeled EXIT/STOP/WAIT/CRITICAL bits in the low word.
    /// External, child/parent, error and full callback flags remain gated.
    pub fn modeled_program_flags(&self) -> u32 {
        (u32::from(self.last_opcode.unwrap_or(0)) << 16)
            | u32::from(self.modeled_program_low_flags())
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

    /// The saved source WAIT start/end in unsigned scaled clock milliseconds.
    /// This does not represent arbitrary CE wait callback pointers.
    pub fn core_wait_state(&self) -> Option<(u32, u32)> {
        self.core_wait
    }

    /// CE interpret polls checkWait before fetching an opcode. The pinned
    /// callback returns true on now <= waitEnd; preserve its exact predicate
    /// (even though it is counterintuitive) rather than inventing sleep semantics.
    /// A false result consumes interpreter budget but executes no instruction.
    pub(crate) fn poll_core_wait(&mut self) -> bool {
        if let Some((_, end)) = self.core_wait {
            if (self.clock_millis as u32) > end {
                return false;
            }
            self.core_wait = None;
        }
        true
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
        let saved_pc = self.pc;
        let saved_low_flags = self.modeled_program_low_flags();
        let mut probe = self.clone();
        probe.pc = offset;
        probe.critical_section = false;
        probe.last_opcode = None; // CE doEvents zeroes condition flags.
        probe.halted = false;
        probe.halt_reason = None;
        probe.run_variable_subset_bounded(max_steps, native)?;
        let satisfied = probe.pop_int()? != 0;
        let termination = probe
            .halt_reason
            .ok_or_else(|| invalid("CE condition terminated without modeled halt reason"))?;
        let flag_restore = PreviewedConditionFlagRestore {
            saved_pc,
            evaluated_pc: probe.pc,
            restored_pc: saved_pc,
            saved_low_flags,
            evaluated_low_flags: probe.modeled_program_low_flags(),
            restored_low_flags: saved_low_flags,
        };
        Ok(PreviewedConditionFrame {
            satisfied,
            termination,
            flag_restore,
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
        let mut condition_terminations = Vec::new();
        let mut condition_flag_restorations = Vec::new();
        for index in 0..scratch.runtime_procedures.len() {
            let procedure = scratch.runtime_procedures[index];
            if procedure.flags & 0x02 != 0 {
                let frame = scratch.preview_condition_frame_with_host_bounded(
                    procedure.condition_offset,
                    max_steps_per_condition,
                    native,
                )?;
                condition_terminations.push((index, frame.termination));
                condition_flag_restorations.push((index, frame.flag_restore));
                scratch.stack = frame.data_stack;
                scratch.return_stack = frame.return_stack;
                scratch.frame_pointer = frame.frame_pointer;
                scratch.base_pointer = frame.base_pointer;
                scratch.runtime_procedures = frame.procedures;
                if frame.satisfied {
                    scratch.runtime_procedures[index].flags = 0;
                    ready.push(index);
                }
            } else if procedure.flags & 0x01 != 0 && (procedure.event_time as u32) < now_millis {
                scratch.runtime_procedures[index].flags = 0;
                ready.push(index);
            }
        }
        Ok(PreviewedDueProcedurePoll {
            ready,
            condition_terminations,
            condition_flag_restorations,
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

    /// Backwards-compatible returned-value projection of the full
    /// scratch local-action frame below. The original interpreter is never
    /// committed or mutated.
    pub fn preview_local_action_with_host_bounded(
        &self,
        procedure_index: usize,
        max_steps: usize,
        native: &mut impl FnMut(u16, &mut Vec<ScriptValue>) -> io::Result<()>,
    ) -> io::Result<Option<ScriptValue>> {
        self.preview_local_action_frame_with_host_bounded(procedure_index, max_steps, native)
            .map(|frame| frame.returned)
    }

    /// Stage pinned CE setupCall(program, body, 20) on a scratch VM.
    /// The original procedure metadata must be supported before touching
    /// stacks. A direct local call may inherit PROCEDURE_FLAG_CRITICAL;
    /// doEvents timed procedures have their flags ZEROED before executeProc
    /// and therefore stage a noncritical call, without interpreting it.
    fn stage_local_core_call_frame(
        &mut self,
        procedure_index: usize,
        enter_critical: bool,
    ) -> io::Result<()> {
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
        if self.program.word_at(body)? & 0x8000 == 0 {
            return Err(invalid("CE local action body is not an opcode"));
        }
        let original_pc =
            i32::try_from(self.pc).map_err(|_| invalid("CE saved action PC exceeds i32"))?;
        let saved_flags = self.modeled_program_low_flags();
        if saved_flags & !0x80 != 0 {
            return Err(invalid(
                "CE local action has unsupported saved program flags",
            ));
        }
        self.push_return(ScriptValue::Int(original_pc))?;
        self.push_return(ScriptValue::Int(20))?;
        self.push(ScriptValue::Int(i32::from(saved_flags)))?;
        self.push(ScriptValue::Pointer(0))?;
        self.push(ScriptValue::Int(0))?; // bounded null-host window ID
        self.push(ScriptValue::Int(0))?; // CE setupCall argument marker
        self.critical_section = enter_critical;
        self.halted = false;
        self.halt_reason = None;
        self.pc = body;
        Ok(())
    }

    /// Persist only the CE doEvents **timed local call setup** in this VM.
    /// Due timers clear their flags before executeProc calls setupCall. In
    /// the pinned source, executeProc does not immediately interpret that
    /// noncritical call: PC and the two CE stacks are prepared for a
    /// subsequent interpreter burst. This method never runs body bytecode
    /// or invokes a native engine callback.
    ///
    /// This safe subset refuses conditional or imported procedures instead
    /// of incorrectly ignoring them or resolving an external program.
    /// Strict unsigned CE event-time comparison is preserved. On failure,
    /// the original VM remains unchanged, unlike CE's error longjmp path.
    pub fn prepare_due_timed_local_core_transactional(
        &mut self,
        procedure_index: usize,
        now_millis: u32,
    ) -> io::Result<bool> {
        let procedure = self
            .runtime_procedures
            .get(procedure_index)
            .copied()
            .ok_or_else(|| invalid("CE timed procedure index outside table"))?;
        if procedure.flags & 0x02 != 0 {
            return Err(invalid(
                "CE conditional event needs full evaluator before timed-only preparation",
            ));
        }
        if procedure.flags & 0x01 == 0 || (procedure.event_time as u32) >= now_millis {
            return Ok(false);
        }
        if procedure.flags & 0x04 != 0 {
            return Err(invalid(
                "CE imported scheduled procedure host not installed",
            ));
        }
        let mut next = self.clone();
        next.runtime_procedures[procedure_index].flags = 0;
        next.clock_millis = now_millis as i32;
        // With the schedule flags cleared, pinned executeProc sees no
        // PROCEDURE_FLAG_CRITICAL. It only prepares the local call frame.
        next.stage_local_core_call_frame(procedure_index, false)?;
        *self = next;
        Ok(true)
    }

    /// Inspect the full candidate state after a source-derived local
    /// executeProc/setupCall frame returns, including procedure writes.
    /// All effects remain on a CLONE. Callers must only supply read-only
    /// native adapters; they are not rolled back if the adapter mutates its
    /// own external state. Imported calls and unsupported opcodes fail.
    pub fn preview_local_action_frame_with_host_bounded(
        &self,
        procedure_index: usize,
        max_steps: usize,
        native: &mut impl FnMut(u16, &mut Vec<ScriptValue>) -> io::Result<()>,
    ) -> io::Result<PreviewedLocalActionVmFrame> {
        let procedure = self
            .runtime_procedures
            .get(procedure_index)
            .copied()
            .ok_or_else(|| invalid("CE local action procedure index outside table"))?;
        let mut probe = self.clone();
        let original_pc =
            i32::try_from(probe.pc).map_err(|_| invalid("CE saved action PC exceeds i32"))?;
        let flags = probe.modeled_program_low_flags();
        probe.stage_local_core_call_frame(procedure_index, procedure.flags & 0x10 != 0)?;

        for _ in 0..max_steps {
            let opcode = probe.program.word_at(probe.pc)?;
            if opcode == 0x8021 || opcode == 0x8025 {
                // Execute the actual bounded CE core return opcode. It
                // preflights the null-host frame, restores modeled flags,
                // consumes the source return marker and exits via 0x40.
                // The previous preview duplicated that unwinding by hand.
                probe.step_with_variable_host(native)?;
                if probe.pc != 20
                    || probe.halt_reason != Some(ScriptHaltReason::ReturnFromCall)
                    || !probe.halted
                    || probe.modeled_program_low_flags() != flags
                {
                    return Err(invalid("CE local action restored an invalid caller frame"));
                }
                if probe.return_stack.last() != Some(&ScriptValue::Int(original_pc)) {
                    return Err(invalid("CE local action saved PC was modified"));
                }
                // Keep the CE value on the candidate operand stack.
                // The older value-only API projects a COPY of this field.
                let returned = if opcode == 0x8025 {
                    Some(*probe.stack.last().ok_or_else(|| {
                        invalid("CE local action return value missing after POP_FLAGS")
                    })?)
                } else {
                    None
                };
                return Ok(PreviewedLocalActionVmFrame {
                    returned,
                    program_counter: probe.pc,
                    saved_low_flags: flags,
                    restored_low_flags: probe.modeled_program_low_flags(),
                    data_stack: probe.stack,
                    return_stack: probe.return_stack,
                    frame_pointer: probe.frame_pointer,
                    base_pointer: probe.base_pointer,
                    procedures: probe.runtime_procedures,
                });
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

    /// Explicit, CORE-ONLY, transactional local executeProc transition.
    /// Unlike detached preview, a successful supported call replaces this
    /// VM's owned stacks, schedule and PC with the candidate post-call state.
    /// No native host, MAP/quest writes, imported calls or gameplay callbacks
    /// can run. Any interpreter error or budget exhaustion leaves the entire
    /// installed VM intact. This atomic failure rule is a Rust adapter safety
    /// property, NOT original CE's longjmp/error-side-effect behavior.
    ///
    /// CE setupCall's original PC remains on the return stack, and the
    /// local POP_FLAGS_EXIT return marker sets PC to 20 before interpret
    /// clears transient 0x40. A subsequent real CE engine integration must
    /// handle caller continuation and full program flags explicitly.
    pub fn execute_local_action_core_transactional_bounded(
        &mut self,
        procedure_index: usize,
        max_steps: usize,
    ) -> io::Result<Option<ScriptValue>> {
        let frame = self.preview_local_action_frame_with_host_bounded(
            procedure_index,
            max_steps,
            &mut |_, _| Err(invalid("CE local core transaction has no native host")),
        )?;
        let original_pc =
            i32::try_from(self.pc).map_err(|_| invalid("CE saved action PC exceeds i32"))?;
        if frame.program_counter != 20
            || frame.return_stack.last() != Some(&ScriptValue::Int(original_pc))
            || frame.saved_low_flags != frame.restored_low_flags
            || frame.restored_low_flags & !0x80 != 0
            || frame.procedures.len() != self.runtime_procedures.len()
        {
            return Err(invalid(
                "CE local core transaction has invalid return frame",
            ));
        }

        // All preflight and execution occurs before modifying the installed
        // VM. Replacing only this VM after successful completion prevents
        // partial stack/schedule commits on malformed synthetic scripts.
        let mut next = self.clone();
        next.pc = frame.program_counter;
        next.stack = frame.data_stack;
        next.return_stack = frame.return_stack;
        next.frame_pointer = frame.frame_pointer;
        next.base_pointer = frame.base_pointer;
        next.runtime_procedures = frame.procedures;
        next.critical_section = frame.restored_low_flags & 0x80 != 0;
        next.halted = false; // interpret cleared its transient 0x40 return.
        next.halt_reason = None;
        *self = next;
        Ok(frame.returned)
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
                // Run the same bounded POP_FLAGS_RETURN_VAL_EXIT handler
                // used by the CE core stepper. Only null-host call frames
                // are representable; malformed frames fail closed.
                probe.step_with_variable_host(native)?;
                if probe.pc != 20
                    || probe.halt_reason != Some(ScriptHaltReason::ReturnFromCall)
                    || !probe.halted
                    || probe.modeled_program_low_flags() != saved_flags as u16
                {
                    return Err(invalid("CE interrupt returned with invalid modeled flags"));
                }
                if probe.return_stack.last() != Some(&ScriptValue::Int(old_pc)) {
                    return Err(invalid("CE interrupt changed the saved original PC"));
                }
                return probe.pop();
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

    /// A safe, local, null-host slice of pinned CE op_pop_flags and its
    /// return variants. Only a zero window ID and null wait pointer are
    /// accepted: nonzero host state is outside this isolated interpreter.
    /// Only the modeled critical (0x80) flag is restorable. Other CE bits
    /// require the full program lifecycle and fail closed.
    ///
    /// Preflight every operand and the return target before changing stacks,
    /// so malformed synthetic call frames do not partially unwind the VM.
    fn pop_local_flags_frame(&mut self, opcode: u16) -> io::Result<()> {
        let preserve_value = opcode == 0x8025;
        let return_frame = matches!(opcode, 0x8020 | 0x8021 | 0x8025);
        let exit_interpreter = matches!(opcode, 0x8021 | 0x8025);
        let value_count = usize::from(preserve_value);
        let n = self.stack.len();
        if n < 3 + value_count {
            return Err(invalid("CE local POP_FLAGS frame underflow"));
        }
        let window = self.stack[n - 1 - value_count];
        let wait = self.stack[n - 2 - value_count];
        let flags = self.stack[n - 3 - value_count];
        if window != ScriptValue::Int(0) || wait != ScriptValue::Pointer(0) {
            return Err(invalid("CE local POP_FLAGS requires a null-host frame"));
        }
        let ScriptValue::Int(flags) = flags else {
            return Err(invalid("CE local POP_FLAGS requires integer flags"));
        };
        if flags & !0x80 != 0 {
            return Err(invalid(
                "CE local POP_FLAGS contains unsupported program flags",
            ));
        }
        let target = if return_frame {
            let address = match self.return_stack.last() {
                Some(ScriptValue::Int(address)) => *address,
                _ => return Err(invalid("CE local POP_FLAGS return address is missing")),
            };
            let target = usize::try_from(address)
                .map_err(|_| invalid("negative CE local POP_FLAGS return address"))?;
            let first_word = self.program.word_at(target)?;
            if first_word & 0x8000 == 0 {
                return Err(invalid("CE local POP_FLAGS return target is not an opcode"));
            }
            Some(target)
        } else {
            None
        };
        let value = if preserve_value {
            self.stack.pop()
        } else {
            None
        };
        self.stack.truncate(self.stack.len() - 3);
        self.critical_section = flags & 0x80 != 0;
        self.halt_reason = None;
        if let Some(target) = target {
            self.return_stack.pop();
            self.pc = target;
            if exit_interpreter {
                // CE POP_EXIT sets transient 0x40; interpret clears it
                // before returning to the enclosing caller.
                self.halt_reason = Some(ScriptHaltReason::ReturnFromCall);
                self.halted = true;
            }
        }
        if let Some(value) = value {
            self.push(value)?;
        }
        Ok(())
    }

    pub fn step(&mut self) -> io::Result<()> {
        self.step_core(false)
    }

    /// Explicit clocked WAIT subset for the transactional interpreter tick.
    /// Generic bounded core/native previews continue to reject WAIT.
    pub(crate) fn step_core_with_wait(&mut self) -> io::Result<()> {
        self.step_core(true)
    }

    fn step_core(&mut self, allow_clock_wait: bool) -> io::Result<()> {
        if self.core_wait.is_some() && !allow_clock_wait {
            return Err(invalid("CE WAIT state requires clocked core tick"));
        }
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
                // The pinned CE handlers set EXITED for EXIT/EXIT_PROGRAM,
                // and STOPPED for STOP_PROGRAM. Preserve the distinction
                // without changing the bounded interpreter's halt policy.
                self.halt_reason = Some(match opcode & 0x03FF {
                    0x0E => ScriptHaltReason::Exit,
                    0x10 => ScriptHaltReason::ExitProgram,
                    _ => ScriptHaltReason::StopProgram,
                });
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
            0x1D => {
                // Pinned CE op_pop_exit pops the return address, sets PC,
                // and raises transient PROGRAM_FLAG_0x40. interpret breaks
                // and clears the bit before returning to its caller. The
                // bounded runner models this as a distinct halt reason.
                let address = self.pop_return_int()?;
                self.jump(address)?;
                self.halt_reason = Some(ScriptHaltReason::ReturnFromCall);
                self.halted = true;
            }
            0x1E => {
                self.pop_return()?;
            }
            0x1F | 0x20 | 0x21 | 0x25 => {
                // CE op_pop_flags, op_pop_flags_return,
                // op_pop_flags_exit and op_pop_flags_return_val_exit.
                // Only safe local, nil-host frames are supported.
                self.pop_local_flags_frame(opcode)?;
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
            0x47 if allow_clock_wait => {
                // Pinned CE op_wait stores scaled timer milliseconds in two
                // unsigned fields and installs the built-in checkWait.
                // The core tick supplies already-scaled deterministic time.
                let duration = self.pop_int()?;
                let start = self.clock_millis as u32;
                self.core_wait = Some((start, start.wrapping_add(duration as u32)));
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
        // CE interpret dispatch first sets flags = (flags & 0xFFFF) |
        // (opcode << 16). But op_pop_flags itself restores saved flags
        // masked to 0xFFFF, erasing the high opcode word on return.
        self.last_opcode = if matches!(opcode, 0x801F | 0x8020 | 0x8021 | 0x8025) {
            None
        } else {
            Some(opcode)
        };
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
        // A caller-owned native callback cannot bypass an installed clocked
        // WAIT. Only advance_core_tick's source checkWait branch may resume it.
        if self.core_wait.is_some() {
            return Err(invalid("CE WAIT cannot run through native preview host"));
        }
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
        self.last_opcode = Some(opcode);
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

    fn synthetic_due_vm(first: &[u8], second: &[u8], second_flags: i32) -> ScriptVm {
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
    fn sequential_condition_poll_observes_new_call_when_on_later_entry() {
        // In pinned CE doEvents, procedure zero may arm procedure one while
        // its own condition is being evaluated. The later entry is visited
        // with its newly written conditional flag and bytecode offset.
        let mut first = Vec::new();
        push_int(&mut first, 28); // procedure one's condition entry
        push_int(&mut first, 1); // procedure one
        opcode(&mut first, 0x8007); // CE CALL_WHEN
        push_int(&mut first, 0); // first condition is false
        opcode(&mut first, 0x8011);
        let mut second = Vec::new();
        push_int(&mut second, 1); // later condition is true
        opcode(&mut second, 0x8011);
        let vm = synthetic_due_vm(&first, &second, 0);
        let poll = vm
            .preview_due_procedure_poll_with_host_bounded(300, 5, &mut |_, _| {
                Err(super::invalid("gameplay natives disabled"))
            })
            .unwrap();
        assert_eq!(poll.ready, [1]);
        assert_eq!(poll.condition_terminations.len(), 2);
        assert_eq!(poll.procedures[0].flags, 0x02);
        assert_eq!(poll.procedures[1].flags, 0);
        assert_eq!(poll.procedures[1].condition_offset, 28);
        assert_eq!(vm.runtime_procedures()[1].flags, 0);
        assert_eq!(vm.runtime_procedures()[1].condition_offset, 28);
        assert!(vm.stack().is_empty());
    }

    #[test]
    fn sequential_condition_poll_cancel_blocks_later_condition() {
        let mut first = Vec::new();
        push_int(&mut first, 1);
        opcode(&mut first, 0x8048); // CE CANCEL later conditional event
        push_int(&mut first, 0); // first condition false
        opcode(&mut first, 0x8011);
        let mut second = Vec::new();
        push_int(&mut second, 1);
        opcode(&mut second, 0x8011);
        let vm = synthetic_due_vm(&first, &second, 0x02);
        let poll = vm
            .preview_due_procedure_poll_with_host_bounded(300, 4, &mut |_, _| {
                Err(super::invalid("gameplay natives disabled"))
            })
            .unwrap();
        assert!(poll.ready.is_empty());
        assert_eq!(poll.condition_terminations.len(), 1);
        assert_eq!(poll.procedures[0].flags, 0x02);
        assert_eq!(poll.procedures[1].flags, 0);
        assert_eq!(poll.procedures[1].condition_offset, 0);
        assert_eq!(vm.runtime_procedures()[1].flags, 0x02);
        assert_eq!(vm.runtime_procedures()[1].condition_offset, 28);
    }

    #[test]
    fn sequential_condition_poll_cancel_all_disarms_earlier_and_later_entries() {
        let mut first = Vec::new();
        opcode(&mut first, 0x8049); // CE CANCEL_ALL
        push_int(&mut first, 0);
        opcode(&mut first, 0x8011);
        let mut second = Vec::new();
        push_int(&mut second, 1);
        opcode(&mut second, 0x8011);
        let vm = synthetic_due_vm(&first, &second, 0x02);
        let poll = vm
            .preview_due_procedure_poll_with_host_bounded(300, 3, &mut |_, _| {
                Err(super::invalid("gameplay natives disabled"))
            })
            .unwrap();
        assert!(poll.ready.is_empty());
        assert_eq!(poll.condition_terminations.len(), 1);
        assert_eq!(poll.procedures[0].flags, 0);
        assert_eq!(poll.procedures[1].flags, 0);
        assert_eq!(poll.procedures[0].condition_offset, 0);
        assert_eq!(poll.procedures[1].condition_offset, 0);
        assert_eq!(vm.runtime_procedures()[0].flags, 0x02);
        assert_eq!(vm.runtime_procedures()[1].flags, 0x02);
    }

    #[test]
    fn sequential_condition_poll_rejects_bad_new_call_when_offset_atomically() {
        let mut first = Vec::new();
        push_int(&mut first, -1);
        push_int(&mut first, 1);
        opcode(&mut first, 0x8007);
        push_int(&mut first, 0);
        opcode(&mut first, 0x8011);
        let vm = synthetic_due_vm(&first, &[0x80, 0x11], 0);
        assert!(vm
            .preview_due_procedure_poll_with_host_bounded(300, 5, &mut |_, _| {
                Err(super::invalid("gameplay natives disabled"))
            })
            .is_err());
        assert_eq!(vm.runtime_procedures()[0].flags, 0x02);
        assert_eq!(vm.runtime_procedures()[1].flags, 0);
        assert_eq!(vm.runtime_procedures()[1].condition_offset, 28);
        assert!(vm.stack().is_empty());
        assert_eq!(vm.pc(), 0);
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
    fn ce_conditional_flag_trace_restores_saved_program_state() {
        use super::ScriptHaltReason;

        for (condition_end, expected_during) in [
            (0x800E_u16, 0x01_u16),
            (0x8010_u16, 0x01_u16),
            (0x8011_u16, 0x08_u16),
        ] {
            let mut bytes = vec![0_u8; 42];
            let mut startup = Vec::new();
            opcode(&mut startup, 0x804A); // START_CRITICAL
            push_int(&mut startup, 51); // retained operand
            opcode(&mut startup, 0x8011); // STOP_PROGRAM
            bytes[..startup.len()].copy_from_slice(&startup);
            let mut condition = Vec::new();
            push_int(&mut condition, 1);
            opcode(&mut condition, condition_end);
            bytes[20..20 + condition.len()].copy_from_slice(&condition);
            let mut vm = ScriptVm::new(fixture(&bytes));
            vm.run_bounded(3).unwrap();
            assert_eq!(vm.modeled_program_low_flags(), 0x88);
            assert_eq!(vm.halt_reason(), Some(ScriptHaltReason::StopProgram));
            let flags = vm
                .preview_condition_frame_with_host_bounded(20, 2, &mut |_, _| {
                    Err(super::invalid("CE natives disabled"))
                })
                .unwrap();
            assert!(flags.satisfied);
            assert_eq!(flags.flag_restore.saved_pc, 10);
            assert_eq!(flags.flag_restore.evaluated_pc, 28);
            assert_eq!(flags.flag_restore.restored_pc, 10);
            assert_eq!(flags.flag_restore.saved_low_flags, 0x88);
            assert_eq!(flags.flag_restore.evaluated_low_flags, expected_during);
            assert_eq!(flags.flag_restore.restored_low_flags, 0x88);
            assert_eq!(flags.data_stack, [ScriptValue::Int(51)]);
            assert_eq!(vm.modeled_program_low_flags(), 0x88);
            assert!(vm.halted());
            assert_eq!(vm.pc(), 10);
            assert_eq!(vm.stack(), [ScriptValue::Int(51)]);
        }
    }

    #[test]
    fn ce_conditional_flag_trace_reports_critical_entered_only_during_evaluation() {
        let mut bytes = vec![0_u8; 42];
        let mut condition = Vec::new();
        opcode(&mut condition, 0x804A); // START_CRITICAL
        push_int(&mut condition, 0);
        opcode(&mut condition, 0x8011); // STOP_PROGRAM
        bytes[20..20 + condition.len()].copy_from_slice(&condition);
        let vm = ScriptVm::new(fixture(&bytes));
        let frame = vm
            .preview_condition_frame_with_host_bounded(20, 3, &mut |_, _| {
                Err(super::invalid("CE game native forbidden"))
            })
            .unwrap();
        assert!(!frame.satisfied);
        assert_eq!(frame.flag_restore.saved_low_flags, 0);
        assert_eq!(frame.flag_restore.evaluated_low_flags, 0x88);
        assert_eq!(frame.flag_restore.restored_low_flags, 0);
        assert_eq!(frame.flag_restore.saved_pc, 0);
        assert_eq!(frame.flag_restore.evaluated_pc, 30);
        assert!(!vm.critical_section());
        assert!(!vm.halted());
        assert_eq!(vm.pc(), 0);
        assert!(vm
            .preview_condition_frame_with_host_bounded(20, 2, &mut |_, _| {
                Err(super::invalid("CE native forbidden"))
            })
            .is_err());
        assert_eq!(vm.modeled_program_low_flags(), 0);
    }

    #[test]
    fn sequential_condition_flags_are_restored_between_two_cloned_evaluations() {
        let mut first = Vec::new();
        opcode(&mut first, 0x804A); // condition enters critical section
        push_int(&mut first, 0);
        opcode(&mut first, 0x8011);
        let mut second = Vec::new();
        push_int(&mut second, 1);
        opcode(&mut second, 0x800E); // EXIT, not STOP
        let vm = synthetic_due_vm(&first, &second, 0x02);
        let poll = vm
            .preview_due_procedure_poll_with_host_bounded(100, 3, &mut |_, _| {
                Err(super::invalid("CE natives forbidden"))
            })
            .unwrap();
        assert_eq!(poll.ready, [1]);
        assert_eq!(poll.condition_flag_restorations.len(), 2);
        assert_eq!(poll.condition_flag_restorations[0].1.saved_low_flags, 0);
        assert_eq!(
            poll.condition_flag_restorations[0].1.evaluated_low_flags,
            0x88
        );
        assert_eq!(poll.condition_flag_restorations[0].1.restored_low_flags, 0);
        assert_eq!(poll.condition_flag_restorations[1].1.saved_low_flags, 0);
        assert_eq!(
            poll.condition_flag_restorations[1].1.evaluated_low_flags,
            0x01
        );
        assert_eq!(poll.condition_flag_restorations[1].1.restored_low_flags, 0);
        assert_eq!(vm.modeled_program_low_flags(), 0);
        assert!(vm.stack().is_empty());
        assert!(vm.return_stack().is_empty());
    }

    fn push_pointer(buffer: &mut Vec<u8>, value: i32) {
        buffer.extend_from_slice(&0xE001_u16.to_be_bytes());
        buffer.extend_from_slice(&value.to_be_bytes());
    }

    fn local_return_fixture(
        restored_flags: i32,
        opcode_value: u16,
        result: i32,
        window: i32,
        wait: i32,
        address: i32,
    ) -> ScriptVm {
        let mut code = Vec::new();
        if opcode_value != 0x801F {
            push_int(&mut code, address);
            opcode(&mut code, 0x800D); // D_TO_A
        }
        push_int(&mut code, restored_flags);
        push_pointer(&mut code, wait);
        push_int(&mut code, window);
        if opcode_value == 0x8025 {
            push_int(&mut code, result);
        }
        opcode(&mut code, opcode_value);
        assert!(code.len() <= 42);
        let mut bytes = vec![0_u8; 42];
        bytes[..code.len()].copy_from_slice(&code);
        // Synthetic CE return-to-opcode destinations for the supported
        // no-host examples. Remaining data-only offsets must fail closed.
        if opcode_value != 0x801F && (address == 36 || address == 40) {
            let target = address as usize;
            bytes[target..target + 2].copy_from_slice(&0x8011_u16.to_be_bytes());
        }
        bytes.extend_from_slice(&[0_u8; 12]);
        ScriptVm::new(ScriptProgram::decode(&bytes).unwrap())
    }

    #[test]
    fn detached_ce_local_action_frame_preserves_scheduled_mutations_only_on_clone() {
        // Pinned CE source: executeProc pushes a zero argument marker,
        // executes the body, and its return-value exit leaves a value on
        // the data stack. CALL_AT writes the procedure table in that VM.
        let mut first = Vec::new();
        opcode(&mut first, 0x801A); // consume setupCall argument marker
        push_int(&mut first, 37); // return value retained beneath CALL_AT operands
        push_int(&mut first, 1); // delay in seconds
        push_int(&mut first, 1); // second procedure
        opcode(&mut first, 0x8006); // CALL_AT at CE's synthetic return marker 20
        opcode(&mut first, 0x8025); // POP_FLAGS_RETURN_VAL_EXIT
        assert_eq!(first.len(), 24);
        let mut vm = synthetic_due_vm(&first, &[0x80, 0x11], 0);
        vm.set_clock_millis(2000);
        let before = vm.runtime_procedures().to_vec();
        let frame = vm
            .preview_local_action_frame_with_host_bounded(0, 6, &mut |_, _| {
                Err(super::invalid("CE gameplay native unavailable"))
            })
            .unwrap();
        assert_eq!(frame.returned, Some(ScriptValue::Int(37)));
        assert_eq!(frame.data_stack, [ScriptValue::Int(37)]);
        // setupCall saves the pre-invocation PC below the interrupt marker.
        assert_eq!(frame.return_stack, [ScriptValue::Int(0)]);
        assert_eq!(frame.program_counter, 20);
        assert_eq!(frame.saved_low_flags, 0);
        assert_eq!(frame.restored_low_flags, 0);
        assert_eq!(frame.procedures[1].flags & 0x01, 0x01);
        assert_eq!(frame.procedures[1].event_time, 3000);
        assert_eq!(before[1].flags, 0);
        assert_eq!(vm.runtime_procedures(), before);
        assert!(vm.stack().is_empty());
        assert!(vm.return_stack().is_empty());
        assert_eq!(vm.pc(), 0);
        assert_eq!(
            vm.preview_local_action_core_bounded(0, 6).unwrap(),
            Some(ScriptValue::Int(37))
        );
    }

    #[test]
    fn ce_bounded_interpret_burst_clears_only_transient_return_flag() {
        let mut returned = local_return_fixture(0, 0x8021, 0, 0, 0, 36);
        assert_eq!(returned.run_bounded(6).unwrap(), 6);
        assert!(returned.halted());
        assert_eq!(
            returned.halt_reason(),
            Some(super::ScriptHaltReason::ReturnFromCall)
        );
        returned.finish_core_interpreter_burst();
        assert!(!returned.halted());
        assert_eq!(returned.halt_reason(), None);
        assert_eq!(returned.pc(), 36);
        assert!(returned.return_stack().is_empty());

        let mut stopped = ScriptVm::new(fixture(&[0x80, 0x11]));
        stopped.step().unwrap();
        stopped.finish_core_interpreter_burst();
        assert!(stopped.halted());
        assert_eq!(
            stopped.halt_reason(),
            Some(super::ScriptHaltReason::StopProgram)
        );
    }

    #[test]
    fn ce_timed_local_call_prepares_persistent_stack_without_interpreting_body() {
        let body = [0x80, 0x1A, 0x80, 0x21]; // POP arg; POP_FLAGS_EXIT
        let mut prefix = vec![0_u8; 22];
        prefix[..2].copy_from_slice(&0x8011_u16.to_be_bytes());
        prefix[20..22].copy_from_slice(&0x8011_u16.to_be_bytes());
        let mut vm = synthetic_due_vm(&prefix, &body, 0x11);
        vm.runtime_procedures[1].event_time = 100;
        assert!(!vm
            .prepare_due_timed_local_core_transactional(1, 100)
            .unwrap());
        assert_eq!(vm.pc(), 0);
        assert!(vm.stack().is_empty());
        assert!(vm.return_stack().is_empty());
        assert_eq!(vm.runtime_procedures()[1].flags, 0x11);
        assert!(vm
            .prepare_due_timed_local_core_transactional(1, 101)
            .unwrap());
        assert_eq!(vm.runtime_procedures()[1].flags, 0);
        assert_eq!(vm.runtime_procedures()[1].event_time, 100);
        assert_eq!(vm.pc(), 28);
        assert_eq!(
            vm.stack(),
            [
                ScriptValue::Int(0),
                ScriptValue::Pointer(0),
                ScriptValue::Int(0),
                ScriptValue::Int(0)
            ]
        );
        assert_eq!(
            vm.return_stack(),
            [ScriptValue::Int(0), ScriptValue::Int(20)]
        );
        assert!(!vm.halted());
        assert!(!vm.critical_section());
        // No CE opcodes were run by the scheduler. The next interpreter
        // burst can consume the argument marker and return via its frame.
        vm.step().unwrap();
        assert_eq!(vm.pc(), 30);
        assert_eq!(vm.stack().len(), 3);
        vm.step().unwrap();
        assert_eq!(vm.pc(), 20);
        assert!(vm.halted());
        assert_eq!(
            vm.halt_reason(),
            Some(super::ScriptHaltReason::ReturnFromCall)
        );
    }

    #[test]
    fn ce_timed_local_setup_rejects_missing_host_or_bad_body_without_consumption() {
        for (flags, body) in [
            (0x03, [0x80, 0x1A]),
            (0x05, [0x80, 0x1A]),
            (0x01, [0x00, 0x00]),
        ] {
            let mut vm = synthetic_due_vm(&[0x80, 0x11], &body, flags);
            vm.runtime_procedures[1].event_time = 5;
            let original = vm.runtime_procedures().to_vec();
            assert!(vm.prepare_due_timed_local_core_transactional(1, 6).is_err());
            assert_eq!(vm.runtime_procedures(), original);
            assert_eq!(vm.pc(), 0);
            assert!(vm.stack().is_empty());
            assert!(vm.return_stack().is_empty());
        }
    }

    #[test]
    fn core_only_transactional_local_call_commits_ce_schedule_and_return_frame() {
        let mut first = Vec::new();
        opcode(&mut first, 0x801A); // setupCall argument marker
        push_int(&mut first, 91); // return value retained beneath CALL_AT
        push_int(&mut first, 3); // timer delay in seconds
        push_int(&mut first, 1); // later procedure index
        opcode(&mut first, 0x8006); // CE CALL_AT at return address 20
        opcode(&mut first, 0x8025); // POP_FLAGS_RETURN_VAL_EXIT
        let mut vm = synthetic_due_vm(&first, &[0x80, 0x11], 0);
        vm.set_clock_millis(4000);
        let preview = vm
            .preview_local_action_frame_with_host_bounded(0, 6, &mut |_, _| {
                Err(super::invalid("CE transaction has no native host"))
            })
            .unwrap();
        assert_eq!(vm.pc(), 0);
        assert_eq!(vm.runtime_procedures()[1].flags, 0);
        assert_eq!(
            vm.execute_local_action_core_transactional_bounded(0, 6)
                .unwrap(),
            Some(ScriptValue::Int(91))
        );
        assert_eq!(vm.pc(), preview.program_counter);
        assert_eq!(vm.pc(), 20);
        assert_eq!(vm.stack(), preview.data_stack);
        assert_eq!(vm.return_stack(), preview.return_stack);
        assert_eq!(vm.return_stack(), [ScriptValue::Int(0)]);
        assert_eq!(vm.runtime_procedures(), preview.procedures);
        assert_eq!(vm.runtime_procedures()[1].event_time, 7000);
        assert_eq!(vm.runtime_procedures()[1].flags & 0x01, 0x01);
        assert_eq!(vm.modeled_program_low_flags(), 0);
        assert!(!vm.halted());
        assert_eq!(vm.halt_reason(), None);
    }

    #[test]
    fn core_only_transactional_local_call_failure_preserves_entire_installed_vm() {
        let mut first = Vec::new();
        opcode(&mut first, 0x801A); // setupCall argument marker
        push_int(&mut first, 2);
        push_int(&mut first, 1);
        opcode(&mut first, 0x8006); // candidate CALL_AT update
        opcode(&mut first, 0x80FC); // unsupported gameplay native opcode
        opcode(&mut first, 0x8021); // unreachable POP_FLAGS_EXIT
        let mut vm = synthetic_due_vm(&first, &[0x80, 0x11], 0);
        vm.set_clock_millis(2000);
        let old_pc = vm.pc();
        let old_stack = vm.stack().to_vec();
        let old_return = vm.return_stack().to_vec();
        let old_procedures = vm.runtime_procedures().to_vec();
        let old_flags = vm.modeled_program_low_flags();
        assert!(vm
            .execute_local_action_core_transactional_bounded(0, 8)
            .is_err());
        assert!(vm
            .execute_local_action_core_transactional_bounded(0, 3)
            .is_err());
        assert_eq!(vm.pc(), old_pc);
        assert_eq!(vm.stack(), old_stack);
        assert_eq!(vm.return_stack(), old_return);
        assert_eq!(vm.runtime_procedures(), old_procedures);
        assert_eq!(vm.modeled_program_low_flags(), old_flags);
    }

    #[test]
    fn core_only_transactional_local_call_rejects_imported_procedures_and_host_changes() {
        let mut first = Vec::new();
        opcode(&mut first, 0x801A);
        opcode(&mut first, 0x8021);
        let mut imported = synthetic_due_vm(&first, &[0x80, 0x11], 0);
        imported.runtime_procedures[0].flags |= 0x04;
        assert!(imported
            .execute_local_action_core_transactional_bounded(0, 3)
            .is_err());
        assert_eq!(imported.runtime_procedures()[0].flags & 0x04, 0x04);
        assert_eq!(imported.pc(), 0);
        let mut no_return = synthetic_due_vm(&[0x80, 0x11], &[0x80, 0x11], 0);
        assert!(no_return
            .execute_local_action_core_transactional_bounded(0, 3)
            .is_err());
        assert_eq!(no_return.pc(), 0);
        assert!(no_return.stack().is_empty());
    }

    #[test]
    fn detached_ce_local_action_frame_aborts_unhandled_native_without_writes() {
        let mut first = Vec::new();
        opcode(&mut first, 0x801A); // POP setupCall marker
        opcode(&mut first, 0x80FC); // forbidden game-native opcode
        opcode(&mut first, 0x8021); // unreachable successful return
        let vm = synthetic_due_vm(&first, &[0x80, 0x11], 0);
        let before = vm.runtime_procedures().to_vec();
        assert!(vm
            .preview_local_action_frame_with_host_bounded(0, 3, &mut |_, _| {
                Err(super::invalid("CE native host not installed"))
            })
            .is_err());
        assert_eq!(vm.runtime_procedures(), before);
        assert!(vm.stack().is_empty());
        assert!(vm.return_stack().is_empty());
        assert_eq!(vm.pc(), 0);
    }

    #[test]
    fn ce_local_pop_flags_variants_unwind_null_host_frames_and_resume() {
        use super::ScriptHaltReason;
        for flags in [0, 0x80] {
            // Standalone POP_FLAGS restores only the modeled critical bit.
            let mut vm = local_return_fixture(flags, 0x801F, 0, 0, 0, 0);
            vm.run_bounded(4).unwrap_err(); // no stop after POP_FLAGS
            assert!(vm.stack().is_empty());
            assert_eq!(vm.return_stack().len(), 0);
            assert_eq!(vm.modeled_program_low_flags(), flags as u16);

            // POP_FLAGS_RETURN jumps and continues the interpreter.
            let mut vm = local_return_fixture(flags, 0x8020, 0, 0, 0, 36);
            for _ in 0..6 {
                vm.step().unwrap();
            }
            assert_eq!(vm.pc(), 36);
            assert!(!vm.halted());
            assert!(vm.stack().is_empty());
            assert!(vm.return_stack().is_empty());
            assert_eq!(vm.modeled_program_low_flags(), flags as u16);
            vm.step().unwrap(); // resumed CE STOP_PROGRAM at PC 36
            assert_eq!(vm.pc(), 38);
            assert!(vm.halted());
            assert_eq!(vm.halt_reason(), Some(ScriptHaltReason::StopProgram));
            assert_eq!(vm.modeled_program_low_flags(), 0x08 | flags as u16);

            // POP_FLAGS_EXIT returns to the C++ interpret caller.
            let mut vm = local_return_fixture(flags, 0x8021, 0, 0, 0, 36);
            assert_eq!(vm.run_bounded(6).unwrap(), 6);
            assert_eq!(vm.pc(), 36);
            assert_eq!(vm.halt_reason(), Some(ScriptHaltReason::ReturnFromCall));
            assert_eq!(vm.modeled_program_low_flags(), flags as u16);
            assert!(vm.stack().is_empty());
            assert!(vm.return_stack().is_empty());

            // POP_FLAGS_RETURN_VAL_EXIT saves the value across frame pop.
            for value in [0, 1, -81, i32::MIN, i32::MAX] {
                let mut vm = local_return_fixture(flags, 0x8025, value, 0, 0, 40);
                assert_eq!(vm.run_bounded(7).unwrap(), 7);
                assert_eq!(vm.pc(), 40);
                assert_eq!(vm.stack(), [ScriptValue::Int(value)]);
                assert!(vm.return_stack().is_empty());
                assert_eq!(vm.halt_reason(), Some(ScriptHaltReason::ReturnFromCall));
                assert_eq!(vm.modeled_program_low_flags(), flags as u16);
            }
        }
    }

    #[test]
    fn ce_local_pop_flags_rejects_unrepresentable_host_state_without_partial_unwind() {
        for opcode_value in [0x801F_u16, 0x8020, 0x8021, 0x8025] {
            for (flags, window, wait, address) in [
                (0x01, 0, 0, 36),
                (0x08, 0, 0, 36),
                (0, 1, 0, 36),
                (0, 0, 8, 36),
                (0, 0, 0, -1),
                (0, 0, 0, 34), // in file, after the frame opcode, not executable
                (0, 0, 0, 600),
            ] {
                if opcode_value == 0x801F && address != 36 {
                    continue;
                }
                let mut vm = local_return_fixture(flags, opcode_value, 5, window, wait, address);
                let count = match opcode_value {
                    0x801F => 3,
                    0x8025 => 6,
                    _ => 5,
                };
                for _ in 0..count {
                    vm.step().unwrap();
                }
                let before_stack = vm.stack().to_vec();
                let before_return = vm.return_stack().to_vec();
                let before_flags = vm.modeled_program_low_flags();
                assert!(vm.step().is_err());
                assert_eq!(vm.stack(), before_stack);
                assert_eq!(vm.return_stack(), before_return);
                assert_eq!(vm.modeled_program_low_flags(), before_flags);
                assert!(!vm.halted());
            }
        }
    }

    #[test]
    fn ce_pop_exit_restores_saved_pc_and_clears_transient_interpret_flag() {
        use super::ScriptHaltReason;
        let mut code = Vec::new();
        push_int(&mut code, 24); // address on the return stack
        opcode(&mut code, 0x800D); // D_TO_A
        push_int(&mut code, 73); // must survive the POP_EXIT
        opcode(&mut code, 0x801D); // source CE POP_EXIT
        let mut vm = ScriptVm::new(fixture(&code));
        assert_eq!(vm.run_bounded(4).unwrap(), 4);
        assert!(vm.halted());
        assert_eq!(vm.pc(), 24);
        assert_eq!(vm.stack(), [ScriptValue::Int(73)]);
        assert!(vm.return_stack().is_empty());
        assert_eq!(vm.halt_reason(), Some(ScriptHaltReason::ReturnFromCall));
        assert_eq!(ScriptHaltReason::ReturnFromCall.ce_terminal_flag(), 0x40);
        // 0x40 is set by the opcode, then cleared at interpret return.
        assert_eq!(vm.modeled_program_low_flags(), 0);
    }

    #[test]
    fn ce_pop_exit_condition_restore_is_detached_and_bad_return_address_fails() {
        use super::ScriptHaltReason;
        for (destination, valid) in [(30_i32, true), (-1, false), (99, false)] {
            let mut code = Vec::new();
            push_int(&mut code, destination);
            opcode(&mut code, 0x800D); // D_TO_A
            push_int(&mut code, 1); // the CE condition result
            opcode(&mut code, 0x801D); // POP_EXIT
            let vm = ScriptVm::new(fixture(&code));
            let result = vm.preview_condition_frame_with_host_bounded(0, 4, &mut |_, _| {
                Err(super::invalid("CE native host not configured"))
            });
            if valid {
                let candidate = result.unwrap();
                assert!(candidate.satisfied);
                assert_eq!(candidate.termination, ScriptHaltReason::ReturnFromCall);
                assert_eq!(candidate.flag_restore.saved_pc, 0);
                assert_eq!(candidate.flag_restore.evaluated_pc, 30);
                assert_eq!(candidate.flag_restore.restored_pc, 0);
                assert_eq!(candidate.flag_restore.evaluated_low_flags, 0);
                assert_eq!(candidate.flag_restore.restored_low_flags, 0);
                assert!(candidate.data_stack.is_empty());
                assert!(candidate.return_stack.is_empty());
            } else {
                assert!(result.is_err());
            }
            assert_eq!(vm.pc(), 0);
            assert_eq!(vm.modeled_program_low_flags(), 0);
            assert!(!vm.halted());
            assert!(vm.stack().is_empty());
            assert!(vm.return_stack().is_empty());
        }
    }

    #[test]
    fn ce_exit_and_stop_opcode_flags_remain_distinct_in_detached_condition_frames() {
        use super::ScriptHaltReason;

        for (opcode_value, reason, terminal_flag) in [
            (0x800E, ScriptHaltReason::Exit, 0x01),
            (0x8010, ScriptHaltReason::ExitProgram, 0x01),
            (0x8011, ScriptHaltReason::StopProgram, 0x08),
        ] {
            let mut code = Vec::new();
            push_int(&mut code, 1);
            opcode(&mut code, opcode_value);
            let vm = ScriptVm::new(fixture(&code));
            let frame = vm
                .preview_condition_frame_with_host_bounded(0, 2, &mut |_, _| {
                    Err(super::invalid("CE native host not provided"))
                })
                .unwrap();
            assert!(frame.satisfied);
            assert_eq!(frame.termination, reason);
            assert_eq!(frame.termination.ce_terminal_flag(), terminal_flag);
            assert!(!vm.halted());
            assert_eq!(vm.halt_reason(), None);
            assert_eq!(vm.pc(), 0);

            let mut standalone = vm;
            standalone.run_bounded(2).unwrap();
            assert_eq!(standalone.halt_reason(), Some(reason));
            assert!(standalone.halted());
        }

        let mut first = Vec::new();
        push_int(&mut first, 0);
        opcode(&mut first, 0x8010);
        let mut second = Vec::new();
        push_int(&mut second, 1);
        opcode(&mut second, 0x8011);
        let vm = synthetic_due_vm(&first, &second, 0x02);
        let candidate = vm
            .preview_due_procedure_poll_with_host_bounded(100, 2, &mut |_, _| {
                Err(super::invalid("gameplay opcodes forbidden"))
            })
            .unwrap();
        assert_eq!(candidate.ready, [1]);
        assert_eq!(
            candidate.condition_terminations,
            [
                (0, ScriptHaltReason::ExitProgram),
                (1, ScriptHaltReason::StopProgram),
            ]
        );
        assert!(!vm.halted());
        assert_eq!(vm.halt_reason(), None);
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
