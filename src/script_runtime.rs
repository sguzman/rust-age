//! CE interpreter program-list traversal, without script/game dispatch.
//!
//! Reference: pinned Fallout 1 CE src/int/intrpret.cc insertProgram,
//! removeProgList and doEvents. Newly registered programs are visited first;
//! procedure slots within one program are visited in their original order.
//! This is NOT a gameplay event loop or program execution system.
use crate::script_procedures::{build_lookup_table, SCRIPT_PROC_NAMES};
use crate::script_vm::{
    PreviewedDueProcedurePoll, PreviewedLocalActionVmFrame, ScriptValue, ScriptVm,
};
use std::collections::VecDeque;
use std::io;

const MAX_PROGRAMS: usize = 4096;

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

/// Bounded clone-only preview of one verified local built-in action.
/// A void (0x8021) procedure has returned=None; 0x8025 supplies a value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PreviewedLocalAction {
    pub frame: PlannedLocalActionFrame,
    pub returned: Option<ScriptValue>,
}

/// Discard-only candidate after an exact built-in action's CE local call.
/// Original registered VMs and native game state remain unchanged.
#[derive(Debug, Clone, PartialEq)]
pub struct PreviewedBoundLocalActionVmFrame {
    pub binding: PlannedLocalActionFrame,
    pub candidate: PreviewedLocalActionVmFrame,
}

/// Source-derived, READ-ONLY local CE executeProc/setupCall frame
/// preflight. It is neither a live stack frame nor executable bytecode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlannedLocalActionFrame {
    pub program_id: u64,
    pub action: usize,
    pub procedure_index: usize,
    pub body_offset: usize,
    /// Current unmodified VM program counter, saved by setupCall.
    pub saved_pc: usize,
    /// CE executeProc calls setupCall(program, address, 20).
    pub return_marker: i32,
    /// setupCall pushes an integer zero on the data stack.
    pub argument_marker: i32,
    pub critical: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DueProcedure {
    pub program_id: u64,
    pub procedure_index: usize,
}

/// One immutable program's speculative CE doEvents selection and frame.
/// Entries follow pinned CE newest-registered-program-first list order.
#[derive(Debug, Clone, PartialEq)]
pub struct PreviewedProgramDuePoll {
    pub program_id: u64,
    pub candidate: PreviewedDueProcedurePoll,
}

/// Bounded, core-only source-order interpreter tick result.
/// CE updatePrograms first interprets each program (newest first), then
/// doEvents prepares due procedure calls. Original engine-native libraries,
/// program cleanup, wait callbacks and game-world effects are unavailable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedCoreTick {
    pub executed_steps: Vec<(u64, usize)>,
    pub prepared_calls: Vec<DueProcedure>,
}

#[derive(Debug, Clone)]
struct ProgramInstance {
    id: u64,
    vm: ScriptVm,
}

/// Stable owned set of isolated .INT VMs. No CE engine opcodes, callbacks,
/// imported procedures, or external scripts execute during polling.
#[derive(Debug, Clone, Default)]
pub struct ScriptRuntime {
    programs: VecDeque<ProgramInstance>,
    suspended: bool,
}

impl ScriptRuntime {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.programs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.programs.is_empty()
    }

    /// CE suspends all doEvents work when suspendEvents is nonzero. Nested
    /// suspension counters and clock correction are not implemented.
    pub fn set_suspended(&mut self, suspended: bool) {
        self.suspended = suspended;
    }

    /// Register with a stable Rust-owned instance ID. CE inserts program
    /// nodes at the list head. Duplicate IDs are rejected by this adapter,
    /// not by the original CE linked list.
    pub fn register(&mut self, id: u64, vm: ScriptVm) -> io::Result<()> {
        if self.programs.len() >= MAX_PROGRAMS {
            return Err(invalid("CE program list exceeds bounded Rust limit"));
        }
        if self.programs.iter().any(|entry| entry.id == id) {
            return Err(invalid("duplicate Rust CE program instance ID"));
        }
        self.programs.push_front(ProgramInstance { id, vm });
        Ok(())
    }

    pub fn vm(&self, id: u64) -> Option<&ScriptVm> {
        self.programs
            .iter()
            .find(|entry| entry.id == id)
            .map(|entry| &entry.vm)
    }

    pub fn vm_mut(&mut self, id: u64) -> Option<&mut ScriptVm> {
        self.programs
            .iter_mut()
            .find(|entry| entry.id == id)
            .map(|entry| &mut entry.vm)
    }

    pub fn remove(&mut self, id: u64) -> Option<ScriptVm> {
        let position = self.programs.iter().position(|entry| entry.id == id)?;
        self.programs.remove(position).map(|entry| entry.vm)
    }

    /// Iterate newest program first, then each procedure in table order.
    /// The host must evaluate conditional offsets and dispatch returned
    /// entries separately. Errors abort the poll; programs visited earlier
    /// may have already consumed events, matching the nontransactional
    /// traversal boundary of the original event scheduler.
    pub fn take_due_with(
        &mut self,
        now_millis: u32,
        mut evaluate_condition: impl FnMut(u64, usize, i32) -> io::Result<Option<bool>>,
    ) -> io::Result<Vec<DueProcedure>> {
        if self.suspended {
            return Ok(Vec::new());
        }
        let mut ready = Vec::new();
        for program in &mut self.programs {
            let id = program.id;
            for procedure_index in program
                .vm
                .take_due_procedures_with(now_millis, |index, offset| {
                    evaluate_condition(id, index, offset)
                })?
            {
                ready.push(DueProcedure {
                    program_id: id,
                    procedure_index,
                });
            }
        }
        Ok(ready)
    }

    /// End-to-end synthetic-only bounded INT condition evaluation across
    /// the CE newest-program-first list. Each program uses isolated scratch
    /// stacks and never invokes engine callbacks; real CE condition stack,
    /// flag and native opcode effects remain unsupported.
    pub fn take_due_preview(
        &mut self,
        now_millis: u32,
        max_steps_per_condition: usize,
    ) -> io::Result<Vec<DueProcedure>> {
        if self.suspended {
            return Ok(Vec::new());
        }
        let mut ready = Vec::new();
        for program in &mut self.programs {
            for procedure_index in program
                .vm
                .take_due_procedures_preview(now_millis, max_steps_per_condition)?
            {
                ready.push(DueProcedure {
                    program_id: program.id,
                    procedure_index,
                });
            }
        }
        Ok(ready)
    }

    /// Read one bounded, sequential CE condition poll from each installed
    /// .INT program in source list order (newest registered first).
    ///
    /// Each program owns its independent data/return stacks and scheduler
    /// entries. Earlier conditions in the SAME program may affect later
    /// entries; no candidate changes cross between programs. The adapter
    /// receives each immutable program ID so the caller can select an exact
    /// read-only native source. Its host effects MUST be pure: this method
    /// cannot roll back external effects. All registered VMs, program order
    /// and game state remain untouched on success or failure.
    ///
    /// No procedure is executed, no CE callbacks run and imported/external
    /// program side effects are unavailable. Suspending doEvents yields an
    /// empty observation and does not invoke the native adapter.
    pub fn preview_program_due_polls_with_host_bounded(
        &self,
        now_millis: u32,
        max_steps_per_condition: usize,
        native: &mut impl FnMut(u64, u16, &mut Vec<ScriptValue>) -> io::Result<()>,
    ) -> io::Result<Vec<PreviewedProgramDuePoll>> {
        if self.suspended {
            return Ok(Vec::new());
        }
        let mut candidates = Vec::with_capacity(self.programs.len());
        for program in &self.programs {
            let id = program.id;
            let candidate = program.vm.preview_due_procedure_poll_with_host_bounded(
                now_millis,
                max_steps_per_condition,
                &mut |opcode, stack| native(id, opcode, stack),
            )?;
            candidates.push(PreviewedProgramDuePoll {
                program_id: id,
                candidate,
            });
        }
        Ok(candidates)
    }

    /// Resolve one of CE's fixed 24 built-in script action slots to the
    /// original local procedure index. The matching rules are reused from
    /// script_procedures; an unimplemented action returns None and never
    /// invents script bytecode. A missing runtime instance is an error.
    pub fn resolve_action(
        &self,
        program_id: u64,
        action: usize,
    ) -> io::Result<Option<DueProcedure>> {
        if action >= SCRIPT_PROC_NAMES.len() {
            return Err(invalid("CE script action index outside 24-slot table"));
        }
        let vm = self
            .vm(program_id)
            .ok_or_else(|| invalid("CE script instance is not registered"))?;
        Ok(
            build_lookup_table(&vm.program)?[action].map(|procedure_index| DueProcedure {
                program_id,
                procedure_index,
            }),
        )
    }

    /// Inspect CE's local executeProc setupCall prerequisites without
    /// modifying the registered program or dispatching an action. Rejects
    /// imported procedures and parameterized calls, unsupported by the
    /// local bounded interpreter. Body byte must start with a CE opcode.
    pub fn inspect_local_action_frame(
        &self,
        program_id: u64,
        action: usize,
    ) -> io::Result<Option<PlannedLocalActionFrame>> {
        let Some(proc) = self.resolve_action(program_id, action)? else {
            return Ok(None);
        };
        let vm = self
            .vm(program_id)
            .ok_or_else(|| invalid("CE local action script instance missing"))?;
        let entry = vm
            .runtime_procedures()
            .get(proc.procedure_index)
            .ok_or_else(|| invalid("CE local action procedure index missing"))?;
        if entry.flags & 0x04 != 0 {
            return Err(invalid("CE imported local action has no Rust host"));
        }
        if entry.argument_count != 0 {
            return Err(invalid("CE local action arguments are not supported"));
        }
        let body_offset = usize::try_from(entry.body_offset)
            .map_err(|_| invalid("CE local action has negative body offset"))?;
        let instruction = vm.program.word_at(body_offset)?;
        if instruction & 0x8000 == 0 {
            return Err(invalid("CE local action body is not a valid opcode"));
        }
        Ok(Some(PlannedLocalActionFrame {
            program_id,
            action,
            procedure_index: proc.procedure_index,
            body_offset,
            saved_pc: vm.pc(),
            return_marker: 20,
            argument_marker: 0,
            critical: entry.flags & 0x10 != 0,
        }))
    }

    /// Compose an exact CE built-in action/frame lookup with its bounded,
    /// cloned local procedure core preview. No gameplay natives or mutable
    /// installed state are supplied. Absence is distinct from void return.
    pub fn preview_local_action_core_bounded(
        &self,
        program_id: u64,
        action: usize,
        max_steps: usize,
    ) -> io::Result<Option<PreviewedLocalAction>> {
        self.preview_local_action_with_host_bounded(program_id, action, max_steps, &mut |_, _| {
            Err(invalid("CE local action native host not provided"))
        })
    }

    /// Same cloned, bounded local call preview with an explicit opcode host.
    /// A caller requesting no external side effects MUST supply a pure,
    /// read-only host. The registered VM is always unchanged.
    pub fn preview_local_action_with_host_bounded(
        &self,
        program_id: u64,
        action: usize,
        max_steps: usize,
        native: &mut impl FnMut(u16, &mut Vec<ScriptValue>) -> io::Result<()>,
    ) -> io::Result<Option<PreviewedLocalAction>> {
        let Some(frame) = self.inspect_local_action_frame(program_id, action)? else {
            return Ok(None);
        };
        let vm = self
            .vm(program_id)
            .ok_or_else(|| invalid("CE local action VM missing after preflight"))?;
        let candidate = vm.preview_local_action_frame_with_host_bounded(
            frame.procedure_index,
            max_steps,
            native,
        )?;
        Ok(Some(PreviewedLocalAction {
            frame,
            returned: candidate.returned,
        }))
    }

    /// Inspect the full speculative frame after one source-verified local
    /// action. The adapter must be pure/read-only; no installed VM or native
    /// world state is updated by returning this candidate.
    pub fn preview_local_action_frame_with_host_bounded(
        &self,
        program_id: u64,
        action: usize,
        max_steps: usize,
        native: &mut impl FnMut(u16, &mut Vec<ScriptValue>) -> io::Result<()>,
    ) -> io::Result<Option<PreviewedBoundLocalActionVmFrame>> {
        let Some(binding) = self.inspect_local_action_frame(program_id, action)? else {
            return Ok(None);
        };
        let vm = self
            .vm(program_id)
            .ok_or_else(|| invalid("CE local action VM missing after preflight"))?;
        let candidate = vm.preview_local_action_frame_with_host_bounded(
            binding.procedure_index,
            max_steps,
            native,
        )?;
        Ok(Some(PreviewedBoundLocalActionVmFrame {
            binding,
            candidate,
        }))
    }

    /// Inspect a source-verified action's complete frame using core opcodes
    /// only. No game natives are supplied by this convenience method.
    pub fn preview_local_action_frame_core_bounded(
        &self,
        program_id: u64,
        action: usize,
        max_steps: usize,
    ) -> io::Result<Option<PreviewedBoundLocalActionVmFrame>> {
        self.preview_local_action_frame_with_host_bounded(
            program_id,
            action,
            max_steps,
            &mut |_, _| Err(invalid("CE local action native host not provided")),
        )
    }

    /// Execute a single, source-verified CE built-in action against its
    /// installed Rust-owned VM with an ATOMIC, core-only transaction.
    /// The candidate is fully checked and run before replacing that one
    /// VM; failures leave all program instances untouched. This does not
    /// call engine natives, mutate MAP/quest state, invoke real game events,
    /// or represent original CE's longjmp error side effects.
    ///
    /// Unlike the speculative preview APIs, successful source-core stack,
    /// PC and procedure schedule effects persist in this ScriptRuntime.
    /// The return marker remains at CE offset 20 until a separately
    /// implemented caller continuation restores the interrupted program.
    pub fn execute_local_action_core_transactional_bounded(
        &mut self,
        program_id: u64,
        action: usize,
        max_steps: usize,
    ) -> io::Result<Option<PreviewedLocalAction>> {
        let Some(frame) = self.inspect_local_action_frame(program_id, action)? else {
            return Ok(None);
        };
        let vm = self
            .vm_mut(program_id)
            .ok_or_else(|| invalid("CE local action VM missing after preflight"))?;
        let returned =
            vm.execute_local_action_core_transactional_bounded(frame.procedure_index, max_steps)?;
        Ok(Some(PreviewedLocalAction { frame, returned }))
    }

    /// Opt-in host dispatch of one resolved CE built-in action, independent
    /// of the timed/conditional event scheduler. A missing script action
    /// returns false; any callback is supplied explicitly by the caller.
    /// This is not an implementation of Fallout's native engine opcodes.
    pub fn dispatch_action_with(
        &mut self,
        program_id: u64,
        action: usize,
        dispatch: impl FnOnce(DueProcedure, &mut ScriptVm) -> io::Result<()>,
    ) -> io::Result<bool> {
        let Some(procedure) = self.resolve_action(program_id, action)? else {
            return Ok(false);
        };
        let vm = self
            .vm_mut(program_id)
            .ok_or_else(|| invalid("CE script instance disappeared during action"))?;
        dispatch(procedure, vm)?;
        Ok(true)
    }

    /// Host callback boundary for ready CE procedures. Unlike the existing
    /// batched diagnostic, this walks each program's procedures in order and
    /// invokes the supplied callback immediately *after* the matching event
    /// flags are cleared. Later decisions can therefore observe host changes
    /// made by earlier callbacks. No native game callback is implemented by
    /// Rust Age; the caller opts in to whatever synthetic host logic runs.
    /// Callback errors leave the due event consumed, as CE clears it before
    /// calling executeProc; earlier callbacks also remain effective.
    pub fn dispatch_due_with(
        &mut self,
        now_millis: u32,
        mut evaluate: impl FnMut(u64, usize, i32) -> io::Result<Option<bool>>,
        mut dispatch: impl FnMut(u64, usize, &mut ScriptVm) -> io::Result<()>,
    ) -> io::Result<Vec<DueProcedure>> {
        if self.suspended {
            return Ok(Vec::new());
        }
        let mut completed = Vec::new();
        for program in &mut self.programs {
            let id = program.id;
            for index in 0..program.vm.runtime_procedures().len() {
                if program
                    .vm
                    .consume_due_procedure_with(index, now_millis, |offset| {
                        evaluate(id, index, offset)
                    })?
                {
                    dispatch(id, index, &mut program.vm)?;
                    completed.push(DueProcedure {
                        program_id: id,
                        procedure_index: index,
                    });
                }
            }
        }
        Ok(completed)
    }

    /// Persist source-ordered CE doEvents timer *call preparation*, not
    /// procedure execution, across installed Rust script VMs. Pinned CE
    /// clears due timer flags before executeProc's local setupCall. Because
    /// the original procedure flags are then zero, it does not immediately
    /// interpret these calls. The resulting PC/stack frames are left ready
    /// for a separately validated future interpreter burst.
    ///
    /// This bounded core-only adapter explicitly rejects conditional and
    /// imported events, which require unavailable evaluator/host behavior.
    /// A full cloned runtime is staged first: failures roll back ALL script
    /// preparations instead of preserving CE's partial error side effects.
    /// No MAP, gameplay-native, quest, or callback state can be touched.
    pub fn prepare_due_timed_local_core_transactional(
        &mut self,
        now_millis: u32,
    ) -> io::Result<Vec<DueProcedure>> {
        if self.suspended {
            return Ok(Vec::new());
        }
        let mut next = self.clone();
        let mut prepared = Vec::new();
        for program in &mut next.programs {
            for procedure_index in 0..program.vm.runtime_procedures().len() {
                if program
                    .vm
                    .prepare_due_timed_local_core_transactional(procedure_index, now_millis)?
                {
                    prepared.push(DueProcedure {
                        program_id: program.id,
                        procedure_index,
                    });
                }
            }
        }
        *self = next;
        Ok(prepared)
    }

    /// One explicit, transactional core-only subset of pinned CE
    /// updatePrograms: interpret each installed program in newest-first
    /// order for at most cpu_burst instructions, THEN call doEvents'
    /// strict timed local call preparation. This ordering matters: a
    /// CALL_AT executed in the interpreter burst can be considered by the
    /// subsequent timed scan, but a newly prepared procedure's body must
    /// not run until a later interpreter burst.
    ///
    /// Conditional events, native engine opcodes, window callbacks,
    /// imported procedures, CE wait handling and program removal are
    /// unsupported. The entire tick is staged on a clone and committed
    /// only after every supported program and timed call succeeds. The
    /// atomic error policy differs deliberately from original CE.
    pub fn advance_core_tick_transactional_bounded(
        &mut self,
        now_millis: u32,
        cpu_burst: usize,
    ) -> io::Result<PreparedCoreTick> {
        if cpu_burst > 4096 {
            return Err(invalid("CE core tick CPU burst exceeds safety bound"));
        }
        let mut next = self.clone();
        let mut executed_steps = Vec::with_capacity(next.programs.len());
        for program in &mut next.programs {
            if program.vm.critical_section() {
                return Err(invalid(
                    "CE unbounded critical-section execution requires full interpreter",
                ));
            }
            program.vm.set_clock_millis(now_millis as i32);
            let mut steps = 0;
            while steps < cpu_burst && !program.vm.halted() {
                program.vm.step()?;
                steps += 1;
            }
            if program.vm.critical_section() {
                return Err(invalid(
                    "CE critical section cannot be suspended by bounded core tick",
                ));
            }
            // CE interpret returns to updatePrograms after clearing
            // transient PROGRAM_FLAG_0x40. Keep real STOP/EXIT terminal.
            program.vm.finish_core_interpreter_burst();
            executed_steps.push((program.id, steps));
        }
        let prepared_calls = next.prepare_due_timed_local_core_transactional(now_millis)?;
        *self = next;
        Ok(PreparedCoreTick {
            executed_steps,
            prepared_calls,
        })
    }

    /// Synthetic-only bounded local interrupt procedure previews for due
    /// programs. Unlike CE's live scheduler, this diagnostic commits the
    /// consumed event flags only when every preview succeeds; engine native
    /// opcodes and real game-world callbacks remain disconnected.
    pub fn preview_due_local_interrupts(
        &mut self,
        now_millis: u32,
        condition_budget: usize,
        procedure_budget: usize,
    ) -> io::Result<Vec<(DueProcedure, ScriptValue)>> {
        let mut scratch = self.clone();
        let due = scratch.take_due_preview(now_millis, condition_budget)?;
        let mut results = Vec::with_capacity(due.len());
        for entry in due {
            let vm = scratch
                .vm(entry.program_id)
                .ok_or_else(|| invalid("CE ready script instance is missing"))?;
            let result =
                vm.preview_local_interrupt_bounded(entry.procedure_index, procedure_budget)?;
            results.push((entry, result));
        }
        *self = scratch;
        Ok(results)
    }

    pub fn take_due_timed(&mut self, now_millis: u32) -> Vec<DueProcedure> {
        if self.suspended {
            return Vec::new();
        }
        let mut ready = Vec::new();
        for program in &mut self.programs {
            for procedure_index in program.vm.take_due_timed_procedures(now_millis) {
                ready.push(DueProcedure {
                    program_id: program.id,
                    procedure_index,
                });
            }
        }
        ready
    }
}

#[cfg(test)]
mod tests {
    use super::{DueProcedure, PlannedLocalActionFrame, ScriptRuntime};
    use crate::script_program::ScriptProgram;
    use crate::script_vm::{ScriptValue, ScriptVm};

    fn vm(flags: i32, time: i32, condition: i32) -> ScriptVm {
        let mut bytes = vec![0_u8; 42];
        bytes.extend_from_slice(&1_i32.to_be_bytes());
        for field in [4_i32, flags, time, condition, 0, 0] {
            bytes.extend_from_slice(&field.to_be_bytes());
        }
        bytes.extend_from_slice(&5_i32.to_be_bytes());
        bytes.extend_from_slice(b"main\0");
        bytes.extend_from_slice(&0_i32.to_be_bytes());
        ScriptVm::new(ScriptProgram::decode(&bytes).unwrap())
    }

    fn local_action_vm(flags: i32, argument_count: i32, body_offset: i32) -> ScriptVm {
        let mut bytes = vec![0_u8; 42];
        bytes[20..22].copy_from_slice(&0x8011_u16.to_be_bytes());
        bytes.extend_from_slice(&2_i32.to_be_bytes());
        for fields in [
            [4_i32, 0, 0, 0, 0, 0],
            [9_i32, flags, 0, 0, body_offset, argument_count],
        ] {
            for field in fields {
                bytes.extend_from_slice(&field.to_be_bytes());
            }
        }
        bytes.extend_from_slice(&11_i32.to_be_bytes());
        bytes.extend_from_slice(b"main\0start\0");
        bytes.extend_from_slice(&0_i32.to_be_bytes());
        ScriptVm::new(ScriptProgram::decode(&bytes).unwrap())
    }

    #[test]
    fn local_action_frame_matches_ce_setup_call_without_mutating_vm() {
        let mut runtime = ScriptRuntime::new();
        runtime.register(7, local_action_vm(0x10, 0, 20)).unwrap();
        let original = runtime.vm(7).unwrap().clone();
        let frame = runtime.inspect_local_action_frame(7, 1).unwrap();
        assert_eq!(
            frame,
            Some(PlannedLocalActionFrame {
                program_id: 7,
                action: 1,
                procedure_index: 1,
                body_offset: 20,
                saved_pc: 0,
                return_marker: 20,
                argument_marker: 0,
                critical: true,
            })
        );
        assert_eq!(runtime.inspect_local_action_frame(7, 23).unwrap(), None);
        assert!(runtime.inspect_local_action_frame(88, 1).is_err());
        assert!(runtime.inspect_local_action_frame(7, 24).is_err());
        assert_eq!(runtime.vm(7).unwrap().pc(), original.pc());
        assert_eq!(runtime.vm(7).unwrap().stack(), original.stack());
        assert_eq!(
            runtime.vm(7).unwrap().return_stack(),
            original.return_stack()
        );
        assert!(!runtime.vm(7).unwrap().halted());
    }

    #[test]
    fn unsupported_ce_local_action_entries_fail_before_frame_setup() {
        for (flags, args, body) in [
            (0x04, 0, 20),  // Imported/external CE procedure.
            (0x00, 1, 20),  // Nonzero argument count.
            (0x00, 0, -1),  // Negative body pointer.
            (0x00, 0, 300), // Outside .INT file.
            (0x00, 0, 22),  // Uninitialized byte, no opcode marker.
        ] {
            let mut runtime = ScriptRuntime::new();
            runtime
                .register(1, local_action_vm(flags, args, body))
                .unwrap();
            assert!(runtime.inspect_local_action_frame(1, 1).is_err());
            assert_eq!(runtime.vm(1).unwrap().pc(), 0);
            assert!(runtime.vm(1).unwrap().stack().is_empty());
        }
    }

    #[test]
    fn composed_local_action_core_preview_keeps_source_vm_inert() {
        let mut bytes = vec![0_u8; 42];
        bytes[20..24].copy_from_slice(&[0x80, 0x1A, 0x80, 0x21]);
        bytes.extend_from_slice(&2_i32.to_be_bytes());
        for fields in [[4_i32, 0, 0, 0, 20, 0], [9_i32, 0, 0, 0, 20, 0]] {
            for field in fields {
                bytes.extend_from_slice(&field.to_be_bytes());
            }
        }
        bytes.extend_from_slice(&11_i32.to_be_bytes());
        bytes.extend_from_slice(b"main\0start\0");
        bytes.extend_from_slice(&0_i32.to_be_bytes());

        let mut runtime = ScriptRuntime::new();
        runtime
            .register(3, ScriptVm::new(ScriptProgram::decode(&bytes).unwrap()))
            .unwrap();
        let preview = runtime
            .preview_local_action_core_bounded(3, 1, 3)
            .unwrap()
            .unwrap();
        assert_eq!(preview.frame.procedure_index, 1);
        assert_eq!(preview.frame.return_marker, 20);
        assert_eq!(preview.returned, None);
        assert_eq!(runtime.vm(3).unwrap().pc(), 0);
        assert!(runtime.vm(3).unwrap().stack().is_empty());
        assert!(runtime.vm(3).unwrap().return_stack().is_empty());
        assert!(!runtime.vm(3).unwrap().halted());
        assert!(runtime.preview_local_action_core_bounded(3, 1, 1).is_err());
        assert!(runtime
            .preview_local_action_core_bounded(3, 23, 3)
            .unwrap()
            .is_none());
    }

    fn core_transaction_action_vm(body: &[u8]) -> ScriptVm {
        assert!(body.len() <= 22);
        let mut bytes = vec![0_u8; 42];
        bytes[20..20 + body.len()].copy_from_slice(body);
        bytes.extend_from_slice(&2_i32.to_be_bytes());
        for fields in [[4_i32, 0, 0, 0, 20, 0], [9_i32, 0, 0, 0, 20, 0]] {
            for field in fields {
                bytes.extend_from_slice(&field.to_be_bytes());
            }
        }
        bytes.extend_from_slice(&11_i32.to_be_bytes());
        bytes.extend_from_slice(b"main\0start\0");
        bytes.extend_from_slice(&0_i32.to_be_bytes());
        ScriptVm::new(ScriptProgram::decode(&bytes).unwrap())
    }

    #[test]
    fn persistent_core_action_schedules_timed_event_on_exact_registered_vm() {
        let mut body = vec![0x80, 0x1A]; // discard setupCall argument marker
        body.extend_from_slice(&0xC001_u16.to_be_bytes());
        body.extend_from_slice(&2_i32.to_be_bytes());
        body.extend_from_slice(&0xC001_u16.to_be_bytes());
        body.extend_from_slice(&1_i32.to_be_bytes());
        body.extend_from_slice(&[0x80, 0x06, 0x80, 0x21]); // CALL_AT, POP_FLAGS_EXIT
        let mut runtime = ScriptRuntime::new();
        runtime
            .register(10, core_transaction_action_vm(&body))
            .unwrap();
        runtime
            .register(20, core_transaction_action_vm(&body))
            .unwrap();
        let expected = runtime
            .preview_local_action_frame_core_bounded(10, 1, 5)
            .unwrap()
            .unwrap();
        assert_eq!(expected.candidate.procedures[1].event_time, 2000);
        assert_eq!(runtime.vm(10).unwrap().runtime_procedures()[1].flags, 0);
        let executed = runtime
            .execute_local_action_core_transactional_bounded(10, 1, 5)
            .unwrap()
            .unwrap();
        assert_eq!(executed.frame, expected.binding);
        assert_eq!(executed.returned, None);
        assert_eq!(runtime.vm(10).unwrap().pc(), 20);
        assert_eq!(
            runtime.vm(10).unwrap().return_stack(),
            [ScriptValue::Int(0)]
        );
        assert_eq!(
            runtime.vm(10).unwrap().runtime_procedures(),
            expected.candidate.procedures
        );
        assert_eq!(runtime.vm(20).unwrap().pc(), 0);
        assert_eq!(runtime.vm(20).unwrap().runtime_procedures()[1].flags, 0);
        assert!(runtime.take_due_timed(2000).is_empty());
        assert_eq!(
            runtime.take_due_timed(2001),
            [DueProcedure {
                program_id: 10,
                procedure_index: 1
            }]
        );
        assert!(runtime.take_due_timed(3000).is_empty());
        assert_eq!(runtime.vm(20).unwrap().runtime_procedures()[1].flags, 0);
    }

    #[test]
    fn persistent_core_action_error_is_atomic_and_does_not_dispatch_natives() {
        let mut runtime = ScriptRuntime::new();
        // It first mutates a staged schedule, then tries forbidden opcode.
        let mut body = vec![0x80, 0x1A];
        body.extend_from_slice(&0xC001_u16.to_be_bytes());
        body.extend_from_slice(&1_i32.to_be_bytes());
        body.extend_from_slice(&0xC001_u16.to_be_bytes());
        body.extend_from_slice(&1_i32.to_be_bytes());
        body.extend_from_slice(&[0x80, 0x06, 0x80, 0xFC, 0x80, 0x21]);
        runtime
            .register(1, core_transaction_action_vm(&body))
            .unwrap();
        let original_vm = runtime.vm(1).unwrap();
        let original_stack = original_vm.stack().to_vec();
        let original_pc = original_vm.pc();
        let original_procedures = original_vm.runtime_procedures().to_vec();
        for budget in [4, 7] {
            assert!(runtime
                .execute_local_action_core_transactional_bounded(1, 1, budget)
                .is_err());
            let installed = runtime.vm(1).unwrap();
            assert_eq!(installed.pc(), original_pc);
            assert_eq!(installed.stack(), original_stack);
            assert!(installed.return_stack().is_empty());
            assert_eq!(installed.runtime_procedures(), original_procedures);
        }
        assert!(runtime.take_due_timed(9000).is_empty());
        assert!(runtime
            .execute_local_action_core_transactional_bounded(1, 23, 3)
            .unwrap()
            .is_none());
    }

    fn due_timed_preparation_vm(flags: i32, event_time: i32, valid_body: bool) -> ScriptVm {
        let mut bytes = vec![0_u8; 42];
        if valid_body {
            bytes[20..24].copy_from_slice(&[0x80, 0x1A, 0x80, 0x21]);
        }
        bytes.extend_from_slice(&2_i32.to_be_bytes());
        for fields in [[4_i32, 0, 0, 0, 0, 0], [9_i32, flags, event_time, 0, 20, 0]] {
            for field in fields {
                bytes.extend_from_slice(&field.to_be_bytes());
            }
        }
        bytes.extend_from_slice(&11_i32.to_be_bytes());
        bytes.extend_from_slice(b"main\0start\0");
        bytes.extend_from_slice(&0_i32.to_be_bytes());
        ScriptVm::new(ScriptProgram::decode(&bytes).unwrap())
    }

    #[test]
    fn timed_event_preparation_persists_ce_call_frames_in_program_list_order() {
        let mut runtime = ScriptRuntime::new();
        runtime
            .register(1, due_timed_preparation_vm(0x01, 9, true))
            .unwrap();
        runtime
            .register(2, due_timed_preparation_vm(0x11, 9, true))
            .unwrap();
        assert!(runtime
            .prepare_due_timed_local_core_transactional(9)
            .unwrap()
            .is_empty());
        for id in [1, 2] {
            assert_eq!(runtime.vm(id).unwrap().pc(), 0);
            assert!(runtime.vm(id).unwrap().stack().is_empty());
        }
        let prepared = runtime
            .prepare_due_timed_local_core_transactional(10)
            .unwrap();
        assert_eq!(
            prepared,
            [
                DueProcedure {
                    program_id: 2,
                    procedure_index: 1
                },
                DueProcedure {
                    program_id: 1,
                    procedure_index: 1
                }
            ]
        );
        for id in [1, 2] {
            let vm = runtime.vm(id).unwrap();
            assert_eq!(vm.pc(), 20);
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
            assert_eq!(vm.runtime_procedures()[1].flags, 0);
            assert_eq!(vm.runtime_procedures()[1].event_time, 9);
            assert!(!vm.halted());
            assert!(!vm.critical_section());
        }
        assert!(runtime
            .prepare_due_timed_local_core_transactional(1000)
            .unwrap()
            .is_empty());
        assert_eq!(runtime.vm(2).unwrap().stack().len(), 4);
    }

    #[test]
    fn timed_event_preparation_rejects_later_invalid_call_atomically() {
        let mut runtime = ScriptRuntime::new();
        // Newest (ID 2) prepares successfully; old (ID 1) then fails.
        runtime
            .register(1, due_timed_preparation_vm(0x01, 4, false))
            .unwrap();
        runtime
            .register(2, due_timed_preparation_vm(0x01, 4, true))
            .unwrap();
        assert!(runtime
            .prepare_due_timed_local_core_transactional(5)
            .is_err());
        for id in [1, 2] {
            let vm = runtime.vm(id).unwrap();
            assert_eq!(vm.pc(), 0);
            assert!(vm.stack().is_empty());
            assert!(vm.return_stack().is_empty());
            assert_eq!(vm.runtime_procedures()[1].flags, 0x01);
        }
        runtime.set_suspended(true);
        assert!(runtime
            .prepare_due_timed_local_core_transactional(5)
            .unwrap()
            .is_empty());
        assert_eq!(runtime.vm(2).unwrap().runtime_procedures()[1].flags, 0x01);
        runtime.set_suspended(false);
        let mut unsupported = ScriptRuntime::new();
        unsupported
            .register(3, due_timed_preparation_vm(0x03, 4, true))
            .unwrap();
        assert!(unsupported
            .prepare_due_timed_local_core_transactional(5)
            .is_err());
        assert_eq!(
            unsupported.vm(3).unwrap().runtime_procedures()[1].flags,
            0x03
        );
    }

    fn tick_local_vm(flags: i32, time: i32, invalid_start: bool) -> ScriptVm {
        let mut bytes = vec![0_u8; 42];
        // Built-in NOPs at PC=0 and PC=2, then CE 0x8011 STOP_PROGRAM.
        bytes[..6].copy_from_slice(&[0x80, 0x00, 0x80, 0x00, 0x80, 0x11]);
        if invalid_start {
            bytes[..2].copy_from_slice(&0x80FC_u16.to_be_bytes());
        }
        bytes[20..24].copy_from_slice(&[0x80, 0x1A, 0x80, 0x21]);
        bytes.extend_from_slice(&2_i32.to_be_bytes());
        for fields in [[4_i32, 0, 0, 0, 0, 0], [9_i32, flags, time, 0, 20, 0]] {
            for field in fields {
                bytes.extend_from_slice(&field.to_be_bytes());
            }
        }
        bytes.extend_from_slice(&11_i32.to_be_bytes());
        bytes.extend_from_slice(b"main\0start\0");
        bytes.extend_from_slice(&0_i32.to_be_bytes());
        ScriptVm::new(ScriptProgram::decode(&bytes).unwrap())
    }

    #[test]
    fn ce_core_tick_interprets_programs_before_preparing_due_timed_calls() {
        let mut runtime = ScriptRuntime::new();
        runtime.register(1, tick_local_vm(0x01, 9, false)).unwrap();
        runtime.register(2, tick_local_vm(0, 9, false)).unwrap();
        let tick = runtime
            .advance_core_tick_transactional_bounded(10, 1)
            .unwrap();
        assert_eq!(tick.executed_steps, [(2, 1), (1, 1)]);
        assert_eq!(
            tick.prepared_calls,
            [DueProcedure {
                program_id: 1,
                procedure_index: 1
            }]
        );
        // Both source programs ran their NOP at PC 0 before doEvents.
        assert_eq!(runtime.vm(2).unwrap().pc(), 2);
        // The due local call then saved PC=2 and installed its body at 20.
        assert_eq!(runtime.vm(1).unwrap().pc(), 20);
        assert_eq!(
            runtime.vm(1).unwrap().return_stack(),
            [ScriptValue::Int(2), ScriptValue::Int(20)]
        );
        assert_eq!(runtime.vm(1).unwrap().stack().len(), 4);
        assert_eq!(runtime.vm(1).unwrap().runtime_procedures()[1].flags, 0);
        // On a later explicit tick, prepared bytecode may finally run.
        let later = runtime
            .advance_core_tick_transactional_bounded(11, 2)
            .unwrap();
        assert_eq!(later.prepared_calls, []);
        assert_eq!(later.executed_steps, [(2, 2), (1, 2)]);
        assert_eq!(runtime.vm(1).unwrap().pc(), 20);
        assert_eq!(runtime.vm(1).unwrap().return_stack(), [ScriptValue::Int(2)]);
        assert!(!runtime.vm(1).unwrap().halted());
        assert_eq!(runtime.vm(1).unwrap().halt_reason(), None);
    }

    #[test]
    fn ce_core_tick_rejects_interrupted_critical_section_but_allows_completion() {
        let mut bytes = vec![0_u8; 42];
        // Enter and then leave the CE critical section before this script
        // can yield a CPU burst. The bounded adapter rejects the partial
        // first instruction rather than committing a fictitious CE yield.
        bytes[..4].copy_from_slice(&[0x80, 0x02, 0x80, 0x03]);
        bytes.extend_from_slice(&[0_u8; 12]);
        let mut runtime = ScriptRuntime::new();
        runtime
            .register(1, ScriptVm::new(ScriptProgram::decode(&bytes).unwrap()))
            .unwrap();
        assert!(runtime
            .advance_core_tick_transactional_bounded(1, 1)
            .is_err());
        assert_eq!(runtime.vm(1).unwrap().pc(), 0);
        assert!(!runtime.vm(1).unwrap().critical_section());
        let complete = runtime
            .advance_core_tick_transactional_bounded(1, 2)
            .unwrap();
        assert_eq!(complete.executed_steps, [(1, 2)]);
        assert!(complete.prepared_calls.is_empty());
        assert_eq!(runtime.vm(1).unwrap().pc(), 4);
        assert!(!runtime.vm(1).unwrap().critical_section());
    }

    #[test]
    fn ce_core_tick_fails_atomically_if_later_program_requires_gameplay_native() {
        let mut runtime = ScriptRuntime::new();
        // Most recent script (ID 2) runs its NOP first, then older script
        // (ID 1) fails; no program step or schedule is committed.
        runtime.register(1, tick_local_vm(0x01, 9, true)).unwrap();
        runtime.register(2, tick_local_vm(0x01, 9, false)).unwrap();
        assert!(runtime
            .advance_core_tick_transactional_bounded(10, 1)
            .is_err());
        for id in [1, 2] {
            assert_eq!(runtime.vm(id).unwrap().pc(), 0);
            assert!(runtime.vm(id).unwrap().stack().is_empty());
            assert_eq!(runtime.vm(id).unwrap().runtime_procedures()[1].flags, 1);
        }
        assert!(runtime
            .advance_core_tick_transactional_bounded(10, 4097)
            .is_err());
        runtime.set_suspended(true);
        let tick = runtime
            .advance_core_tick_transactional_bounded(10, 0)
            .unwrap();
        assert!(tick.prepared_calls.is_empty());
        assert_eq!(tick.executed_steps, [(2, 0), (1, 0)]);
        assert_eq!(runtime.vm(1).unwrap().runtime_procedures()[1].flags, 1);
    }

    #[test]
    fn newest_ce_program_is_polled_first_with_strict_due_time() {
        let mut runtime = ScriptRuntime::new();
        runtime.register(1, vm(0x01, 10, 0)).unwrap();
        runtime.register(2, vm(0x01, 10, 0)).unwrap();
        runtime.register(3, vm(0x01, 10, 0)).unwrap();
        assert_eq!(runtime.len(), 3);
        assert!(runtime.take_due_timed(10).is_empty());
        assert_eq!(
            runtime.take_due_timed(11),
            [
                DueProcedure {
                    program_id: 3,
                    procedure_index: 0
                },
                DueProcedure {
                    program_id: 2,
                    procedure_index: 0
                },
                DueProcedure {
                    program_id: 1,
                    procedure_index: 0
                },
            ]
        );
        assert!(runtime.take_due_timed(100).is_empty());
    }

    #[test]
    fn conditional_checks_take_priority_and_suspension_preserves_work() {
        let mut runtime = ScriptRuntime::new();
        runtime.register(1, vm(0x03, 1, 77)).unwrap();
        runtime.register(2, vm(0x01, 1, 0)).unwrap();
        runtime.set_suspended(true);
        assert!(runtime
            .take_due_with(100, |_, _, _| panic!("no callback when suspended"))
            .unwrap()
            .is_empty());
        runtime.set_suspended(false);
        let mut called = Vec::new();
        assert_eq!(
            runtime
                .take_due_with(100, |id, idx, offset| {
                    called.push((id, idx, offset));
                    Ok(Some(false))
                })
                .unwrap(),
            [DueProcedure {
                program_id: 2,
                procedure_index: 0
            }]
        );
        assert_eq!(called, [(1, 0, 77)]);
        assert_eq!(runtime.vm(1).unwrap().runtime_procedures()[0].flags, 0x03);
        assert_eq!(
            runtime
                .take_due_with(100, |_, _, _| Ok(Some(true)))
                .unwrap(),
            [DueProcedure {
                program_id: 1,
                procedure_index: 0
            }]
        );
    }

    fn condition_vm(value: i32) -> ScriptVm {
        let mut bytes = vec![0_u8; 42];
        bytes[20..22].copy_from_slice(&0xC001_u16.to_be_bytes());
        bytes[22..26].copy_from_slice(&value.to_be_bytes());
        bytes[26..28].copy_from_slice(&0x8011_u16.to_be_bytes());
        bytes.extend_from_slice(&1_i32.to_be_bytes());
        for field in [4_i32, 0x02, 0, 20, 0, 0] {
            bytes.extend_from_slice(&field.to_be_bytes());
        }
        bytes.extend_from_slice(&5_i32.to_be_bytes());
        bytes.extend_from_slice(b"main\0");
        bytes.extend_from_slice(&0_i32.to_be_bytes());
        ScriptVm::new(ScriptProgram::decode(&bytes).unwrap())
    }

    #[test]
    fn detached_sequential_program_polls_obey_ce_list_order_and_preserve_all_vms() {
        let mut runtime = ScriptRuntime::new();
        runtime.register(1, condition_vm(1)).unwrap();
        runtime.register(2, vm(0x01, 10, 0)).unwrap();
        runtime.register(3, condition_vm(0)).unwrap();
        let polls = runtime
            .preview_program_due_polls_with_host_bounded(11, 2, &mut |_, _, _| {
                panic!("these synthetic core-only programs require no native host")
            })
            .unwrap();
        assert_eq!(
            polls.iter().map(|p| p.program_id).collect::<Vec<_>>(),
            [3, 2, 1]
        );
        assert!(polls[0].candidate.ready.is_empty());
        assert_eq!(polls[1].candidate.ready, [0]);
        assert_eq!(polls[2].candidate.ready, [0]);
        assert_eq!(polls[0].candidate.procedures[0].flags, 0x02);
        assert_eq!(polls[1].candidate.procedures[0].flags, 0);
        assert_eq!(polls[2].candidate.procedures[0].flags, 0);
        assert_eq!(polls[0].candidate.condition_flag_restorations.len(), 1);
        assert!(polls[1].candidate.condition_flag_restorations.is_empty());
        assert_eq!(polls[2].candidate.condition_flag_restorations.len(), 1);
        for index in [0, 2] {
            let trace = polls[index].candidate.condition_flag_restorations[0].1;
            assert_eq!(trace.saved_low_flags, 0);
            assert_eq!(trace.evaluated_low_flags, 0x08);
            assert_eq!(trace.restored_low_flags, 0);
            assert_eq!(trace.saved_pc, 0);
            assert_eq!(trace.restored_pc, 0);
        }
        assert_eq!(runtime.vm(1).unwrap().runtime_procedures()[0].flags, 0x02);
        assert_eq!(runtime.vm(2).unwrap().runtime_procedures()[0].flags, 0x01);
        assert_eq!(runtime.vm(3).unwrap().runtime_procedures()[0].flags, 0x02);

        // A late condition error rejects the whole observation without
        // consuming even an earlier ready candidate.
        assert!(runtime
            .preview_program_due_polls_with_host_bounded(11, 1, &mut |_, _, _| {
                panic!("no game-native invocation in core-only conditions")
            })
            .is_err());
        assert_eq!(runtime.vm(2).unwrap().runtime_procedures()[0].flags, 0x01);
        runtime.set_suspended(true);
        assert!(runtime
            .preview_program_due_polls_with_host_bounded(11, 2, &mut |_, _, _| {
                panic!("CE suspended program should never call native host")
            })
            .unwrap()
            .is_empty());
    }

    #[test]
    fn detached_program_polls_route_native_reads_by_program_id() {
        // The native adapter must be selected by exact script instance ID;
        // no MAP/game-global writes and no live callback is permitted.
        let mut bytes = vec![0_u8; 42];
        bytes[20..22].copy_from_slice(&0xC001_u16.to_be_bytes());
        bytes[22..26].copy_from_slice(&0_i32.to_be_bytes());
        bytes[26..28].copy_from_slice(&0x80C3_u16.to_be_bytes()); // map_var
        bytes[28..30].copy_from_slice(&0x8011_u16.to_be_bytes());
        bytes.extend_from_slice(&1_i32.to_be_bytes());
        for field in [4_i32, 0x02, 0, 20, 0, 0] {
            bytes.extend_from_slice(&field.to_be_bytes());
        }
        bytes.extend_from_slice(&5_i32.to_be_bytes());
        bytes.extend_from_slice(b"main\0");
        bytes.extend_from_slice(&0_i32.to_be_bytes());
        let program = ScriptProgram::decode(&bytes).unwrap();
        let mut runtime = ScriptRuntime::new();
        runtime.register(4, ScriptVm::new(program.clone())).unwrap();
        runtime.register(9, ScriptVm::new(program)).unwrap();
        let mut seen = Vec::new();
        let polls = runtime
            .preview_program_due_polls_with_host_bounded(1, 3, &mut |id, opcode, stack| {
                assert_eq!(opcode, 0x80C3);
                assert_eq!(stack.pop(), Some(ScriptValue::Int(0)));
                seen.push(id);
                stack.push(ScriptValue::Int(if id == 9 { 1 } else { 0 }));
                Ok(())
            })
            .unwrap();
        assert_eq!(seen, [9, 4]);
        assert_eq!(polls[0].candidate.ready, [0]);
        assert!(polls[1].candidate.ready.is_empty());
        for id in [4, 9] {
            let vm = runtime.vm(id).unwrap();
            assert_eq!(vm.runtime_procedures()[0].flags, 0x02);
            assert!(vm.stack().is_empty());
            assert_eq!(vm.pc(), 0);
        }

        assert!(runtime
            .preview_program_due_polls_with_host_bounded(1, 3, &mut |id, _, stack| {
                assert_eq!(stack.pop(), Some(ScriptValue::Int(0)));
                if id == 4 {
                    return Err(super::invalid("read-only script-native missing"));
                }
                stack.push(ScriptValue::Int(1));
                Ok(())
            })
            .is_err());
        assert_eq!(runtime.vm(9).unwrap().runtime_procedures()[0].flags, 0x02);
    }

    #[test]
    fn synthetic_bounded_script_conditions_obey_program_order_without_gameplay() {
        let mut runtime = ScriptRuntime::new();
        runtime.register(1, condition_vm(1)).unwrap();
        runtime.register(2, vm(0x01, 10, 0)).unwrap();
        runtime.register(3, condition_vm(0)).unwrap();
        let ready = runtime.take_due_preview(11, 3).unwrap();
        assert_eq!(ready.len(), 2);
        assert_eq!(ready[0].program_id, 2);
        assert_eq!(ready[1].program_id, 1);
        assert_eq!(runtime.vm(3).unwrap().runtime_procedures()[0].flags, 0x02);
        assert_eq!(runtime.vm(2).unwrap().runtime_procedures()[0].flags, 0);
        assert_eq!(runtime.vm(1).unwrap().runtime_procedures()[0].flags, 0);
        assert!(runtime.take_due_preview(100, 3).unwrap().is_empty());
    }

    #[test]
    fn synthetic_condition_budget_error_preserves_pending_program_state() {
        let mut runtime = ScriptRuntime::new();
        runtime.register(9, condition_vm(1)).unwrap();
        assert!(runtime.take_due_preview(100, 1).is_err());
        assert_eq!(runtime.vm(9).unwrap().runtime_procedures()[0].flags, 0x02);
        assert_eq!(runtime.take_due_preview(100, 3).unwrap()[0].program_id, 9);
    }

    fn interrupt_vm(condition: i32, value: i32) -> ScriptVm {
        let mut bytes = vec![0_u8; 42];
        bytes[20..22].copy_from_slice(&0xC001_u16.to_be_bytes());
        bytes[22..26].copy_from_slice(&condition.to_be_bytes());
        bytes[26..28].copy_from_slice(&0x8011_u16.to_be_bytes());
        bytes[30..32].copy_from_slice(&0x801A_u16.to_be_bytes());
        bytes[32..34].copy_from_slice(&0xC001_u16.to_be_bytes());
        bytes[34..38].copy_from_slice(&value.to_be_bytes());
        bytes[38..40].copy_from_slice(&0x8025_u16.to_be_bytes());
        bytes.extend_from_slice(&1_i32.to_be_bytes());
        for field in [4_i32, 0x02, 0, 20, 30, 0] {
            bytes.extend_from_slice(&field.to_be_bytes());
        }
        bytes.extend_from_slice(&5_i32.to_be_bytes());
        bytes.extend_from_slice(b"main\0");
        bytes.extend_from_slice(&0_i32.to_be_bytes());
        ScriptVm::new(ScriptProgram::decode(&bytes).unwrap())
    }

    #[test]
    fn interrupt_previews_emit_ordered_values_without_mutating_program_bytes() {
        let mut runtime = ScriptRuntime::new();
        runtime.register(1, interrupt_vm(1, 51)).unwrap();
        runtime.register(2, interrupt_vm(1, 72)).unwrap();
        runtime.register(3, interrupt_vm(0, 10)).unwrap();
        let original = runtime.vm(1).unwrap().program.clone();
        let previews = runtime.preview_due_local_interrupts(10, 3, 4).unwrap();
        assert_eq!(previews.len(), 2);
        assert_eq!(previews[0].0.program_id, 2);
        assert_eq!(previews[0].1, crate::script_vm::ScriptValue::Int(72));
        assert_eq!(previews[1].0.program_id, 1);
        assert_eq!(previews[1].1, crate::script_vm::ScriptValue::Int(51));
        assert_eq!(runtime.vm(1).unwrap().program, original);
        assert_eq!(runtime.vm(3).unwrap().runtime_procedures()[0].flags, 0x02);
        assert!(runtime
            .preview_due_local_interrupts(10, 3, 4)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn failed_interrupt_preview_rolls_back_earlier_event_consumption() {
        let mut runtime = ScriptRuntime::new();
        runtime.register(1, interrupt_vm(1, 5)).unwrap();
        runtime.register(2, interrupt_vm(1, 6)).unwrap();
        assert!(runtime.preview_due_local_interrupts(10, 3, 2).is_err());
        assert_eq!(runtime.vm(1).unwrap().runtime_procedures()[0].flags, 0x02);
        assert_eq!(runtime.vm(2).unwrap().runtime_procedures()[0].flags, 0x02);
        assert_eq!(
            runtime
                .preview_due_local_interrupts(10, 3, 4)
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn opt_in_host_dispatch_observes_ce_order_and_consumed_flags() {
        let mut runtime = ScriptRuntime::new();
        runtime.register(1, vm(0x03, 1, 77)).unwrap();
        runtime.register(2, vm(0x01, 1, 0)).unwrap();
        runtime.set_suspended(true);
        assert!(runtime
            .dispatch_due_with(
                100,
                |_, _, _| panic!("suspended condition"),
                |_, _, _| { panic!("suspended dispatch") }
            )
            .unwrap()
            .is_empty());
        runtime.set_suspended(false);
        let mut called = Vec::new();
        let result = runtime
            .dispatch_due_with(
                100,
                |id, _, offset| {
                    assert_eq!((id, offset), (1, 77));
                    Ok(Some(true))
                },
                |id, index, vm| {
                    assert_eq!(vm.runtime_procedures()[index].flags, 0);
                    called.push(id);
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(called, [2, 1]);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].program_id, 2);
        assert_eq!(result[1].program_id, 1);
        assert!(runtime
            .dispatch_due_with(100, |_, _, _| Ok(Some(true)), |_, _, _| Ok(()))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn live_style_dispatch_callbacks_can_influence_later_condition_checks() {
        use std::cell::Cell;
        use std::rc::Rc;

        let mut runtime = ScriptRuntime::new();
        runtime.register(1, vm(0x02, 0, 19)).unwrap();
        runtime.register(2, vm(0x02, 0, 19)).unwrap();
        let gate = Rc::new(Cell::new(false));
        let evaluate_gate = Rc::clone(&gate);
        let dispatch_gate = Rc::clone(&gate);
        let result = runtime
            .dispatch_due_with(
                100,
                move |id, _, offset| {
                    assert_eq!(offset, 19);
                    Ok(Some(id == 2 || evaluate_gate.get()))
                },
                move |id, _, vm| {
                    assert_eq!(vm.runtime_procedures()[0].flags, 0);
                    if id == 2 {
                        dispatch_gate.set(true);
                    }
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].program_id, 2);
        assert_eq!(result[1].program_id, 1);
        assert!(gate.get());
    }

    #[test]
    fn host_dispatch_failure_preserves_consumed_event_and_skips_later_program() {
        let mut runtime = ScriptRuntime::new();
        runtime.register(1, vm(0x01, 1, 0)).unwrap();
        runtime.register(2, vm(0x01, 1, 0)).unwrap();
        let mut visited = Vec::new();
        assert!(runtime
            .dispatch_due_with(
                100,
                |_, _, _| Ok(None),
                |id, _, _| {
                    visited.push(id);
                    Err(std::io::Error::other("synthetic host dispatch failed"))
                },
            )
            .is_err());
        assert_eq!(visited, [2]);
        assert_eq!(runtime.vm(2).unwrap().runtime_procedures()[0].flags, 0);
        assert_eq!(runtime.vm(1).unwrap().runtime_procedures()[0].flags, 1);
        let mut visited_after = Vec::new();
        runtime
            .dispatch_due_with(
                100,
                |_, _, _| Ok(None),
                |id, _, _| {
                    visited_after.push(id);
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(visited_after, [1]);
    }

    fn named_action_vm() -> ScriptVm {
        let mut bytes = vec![0_u8; 42];
        bytes.extend_from_slice(&3_i32.to_be_bytes());
        let names: [&[u8]; 3] = [b"main", b"START", b"Map_Exit_P_Proc"];
        let mut identifiers = Vec::new();
        for name in names {
            let offset = 4 + identifiers.len() as i32;
            for field in [offset, 0, 0, 0, 0, 0] {
                bytes.extend_from_slice(&field.to_be_bytes());
            }
            identifiers.extend_from_slice(name);
            identifiers.push(0);
        }
        bytes.extend_from_slice(&(identifiers.len() as i32).to_be_bytes());
        bytes.extend_from_slice(&identifiers);
        bytes.extend_from_slice(&0_i32.to_be_bytes());
        ScriptVm::new(ScriptProgram::decode(&bytes).unwrap())
    }

    #[test]
    fn original_ce_builtin_actions_resolve_case_insensitively_without_execution() {
        let mut runtime = ScriptRuntime::new();
        runtime.register(19, named_action_vm()).unwrap();
        assert_eq!(
            runtime.resolve_action(19, 1).unwrap(),
            Some(DueProcedure {
                program_id: 19,
                procedure_index: 1,
            })
        );
        assert_eq!(
            runtime.resolve_action(19, 16).unwrap(),
            Some(DueProcedure {
                program_id: 19,
                procedure_index: 2,
            })
        );
        assert_eq!(runtime.resolve_action(19, 11).unwrap(), None);
        assert!(runtime.resolve_action(19, 24).is_err());
        assert!(runtime.resolve_action(18, 1).is_err());
        let mut called = Vec::new();
        assert!(runtime
            .dispatch_action_with(19, 1, |proc, vm| {
                called.push(proc);
                assert!(vm.stack().is_empty());
                Ok(())
            })
            .unwrap());
        assert_eq!(called[0].procedure_index, 1);
        assert!(!runtime
            .dispatch_action_with(19, 11, |_, _| {
                panic!("a missing original CE action must not dispatch")
            })
            .unwrap());
        assert_eq!(runtime.vm(19).unwrap().pc(), 0);
    }

    #[test]
    fn bounded_registration_and_removal_keep_program_identity() {
        let mut runtime = ScriptRuntime::new();
        runtime.register(5, vm(0x01, 0, 0)).unwrap();
        assert!(runtime.register(5, vm(0x01, 0, 0)).is_err());
        assert_eq!(runtime.len(), 1);
        assert!(runtime.vm_mut(5).is_some());
        assert!(runtime.remove(99).is_none());
        assert!(runtime.remove(5).is_some());
        assert!(runtime.is_empty());
    }
}
