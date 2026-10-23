//! CE interpreter program-list traversal, without script/game dispatch.
//!
//! Reference: pinned Fallout 1 CE src/int/intrpret.cc insertProgram,
//! removeProgList and doEvents. Newly registered programs are visited first;
//! procedure slots within one program are visited in their original order.
//! This is NOT a gameplay event loop or program execution system.
use crate::script_vm::ScriptVm;
use std::collections::VecDeque;
use std::io;

const MAX_PROGRAMS: usize = 4096;

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DueProcedure {
    pub program_id: u64,
    pub procedure_index: usize,
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
    use super::{DueProcedure, ScriptRuntime};
    use crate::script_program::ScriptProgram;
    use crate::script_vm::ScriptVm;

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
