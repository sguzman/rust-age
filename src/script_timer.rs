//! CE script_q_add/save/load/process timer metadata, without callbacks.
//! Source: pinned game/scripts.cc script_q_* and game/queue.cc queue_add.
//! Payload bytes are exactly CE's big-endian (sid, fixedParam) pair.
//! The EventQueue only records game-time ticks; it never runs an INT script.
use crate::collision::ObjectKey;
use crate::event_queue::{EventQueue, EventType, QueueEvent};
use crate::script_binding::BoundScriptRuntime;
use crate::script_lifecycle::PlannedMapAction;
use crate::script_vm::{ScriptValue, ScriptVm};
use std::io;

pub const TIMED_PROCEDURE_ACTION: usize = 22;

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptTimerEvent {
    pub time: i32,
    pub owner: Option<ObjectKey>,
    pub sid: i32,
    pub fixed_parameter: i32,
}

impl ScriptTimerEvent {
    /// Decode a popped CE event queue entry of type SCRIPT. The caller is
    /// responsible for choosing when to pop; no event dispatch occurs here.
    pub fn decode(event: &QueueEvent) -> io::Result<Self> {
        if event.kind != EventType::Script || event.payload.len() != 8 {
            return Err(invalid("not a CE script timer payload"));
        }
        let sid = i32::from_be_bytes(
            event.payload[0..4]
                .try_into()
                .map_err(|_| invalid("invalid SID"))?,
        );
        let fixed_parameter = i32::from_be_bytes(
            event.payload[4..8]
                .try_into()
                .map_err(|_| invalid("invalid fixedParam"))?,
        );
        Ok(Self {
            time: event.time,
            owner: event.owner,
            sid,
            fixed_parameter,
        })
    }
}

impl BoundScriptRuntime {
    /// CE script_q_add succeeds only when the source script record exists.
    /// The caller supplies a trusted ObjectKey instead of dereferencing a
    /// C++ Object*; delay is measured in game-time ticks and can be negative.
    /// Unknown SIDs do not alter the queue. No .INT execution is attempted.
    pub fn queue_script_timer(
        &self,
        queue: &mut EventQueue,
        now_ticks: i32,
        delay_ticks: i32,
        owner: ObjectKey,
        sid: i32,
        fixed_parameter: i32,
    ) -> io::Result<bool> {
        if !self
            .entries
            .iter()
            .any(|entry| entry.binding.script_id == sid)
        {
            return Ok(false);
        }
        let mut payload = Vec::with_capacity(8);
        payload.extend_from_slice(&sid.to_be_bytes());
        payload.extend_from_slice(&fixed_parameter.to_be_bytes());
        queue.add(
            now_ticks,
            delay_ticks,
            EventType::Script,
            Some(owner),
            payload,
        )?;
        Ok(true)
    }

    /// CE script_q_process sets script->fixedParam and requests
    /// SCRIPT_PROC_TIMED (22). Here the result is only a planned action:
    /// missing records/programs or unimplemented timed procedures return
    /// None. The caller must opt into any future execution explicitly.
    pub fn plan_script_timer(
        &self,
        timer: &ScriptTimerEvent,
    ) -> io::Result<Option<PlannedMapAction>> {
        let Some(entry) = self
            .entries
            .iter()
            .find(|x| x.binding.script_id == timer.sid)
        else {
            return Ok(None);
        };
        let Some(instance) = entry.instance_id else {
            return Ok(None);
        };
        let Some(procedure) = self
            .runtime
            .resolve_action(instance, TIMED_PROCEDURE_ACTION)?
        else {
            return Ok(None);
        };
        Ok(Some(PlannedMapAction {
            instance_id: instance,
            script_id: timer.sid,
            category: entry.binding.category,
            ordinal: entry.binding.ordinal,
            action: TIMED_PROCEDURE_ACTION,
            procedure_index: procedure.procedure_index,
            external_parameter: Some(timer.fixed_parameter),
        }))
    }
}

pub const ADD_TIMER_EVENT_OPCODE: u16 = 0x80F0;
pub const REMOVE_TIMER_EVENT_OPCODE: u16 = 0x80F1;

/// Explicit, synthetic mapping from opaque CE pointer values to stable Rust
/// object keys and script IDs. Handles are NOT addresses or dereferenceable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimerObjectBinding {
    pub handle: i32,
    pub owner: ObjectKey,
    pub sid: i32,
}

/// Source-faithful operand order for pinned intextra.cc op_add_timer_event
/// (pop param, delay, Object*) and op_rm_timer_event (pop Object*).
/// A null object is a no-op after consuming arguments. Unknown/ambiguous
/// nonzero opaque handles error safely before modifying the stack or queue.
/// The supplied queue and bindings belong solely to this synthetic host.
pub fn apply_timer_native_opcode(
    scripts: &BoundScriptRuntime,
    queue: &mut EventQueue,
    now_ticks: i32,
    bindings: &[TimerObjectBinding],
    opcode: u16,
    stack: &mut Vec<ScriptValue>,
) -> io::Result<()> {
    let needed = match opcode {
        ADD_TIMER_EVENT_OPCODE => 3,
        REMOVE_TIMER_EVENT_OPCODE => 1,
        _ => return Err(invalid("unknown CE timer native opcode")),
    };
    if stack.len() < needed {
        return Err(invalid("CE timer native stack underflow"));
    }
    let at = stack.len() - needed;
    let ScriptValue::Pointer(handle) = stack[at] else {
        return Err(invalid("CE timer event object must be an opaque pointer"));
    };
    let (delay, parameter) = if opcode == ADD_TIMER_EVENT_OPCODE {
        let (ScriptValue::Int(delay), ScriptValue::Int(parameter)) =
            (stack[at + 1], stack[at + 2])
        else {
            return Err(invalid("CE timer delay and parameter must be integers"));
        };
        (delay, parameter)
    } else {
        (0, 0)
    };
    if handle != 0 {
        let mut matching = bindings.iter().filter(|object| object.handle == handle);
        let binding = matching
            .next()
            .ok_or_else(|| invalid("unknown CE opaque timer object handle"))?;
        if matching.next().is_some() {
            return Err(invalid("ambiguous CE opaque timer object handle"));
        }
        if opcode == ADD_TIMER_EVENT_OPCODE {
            // Original native ignores the return from script_q_add.
            let _ = scripts.queue_script_timer(
                queue, now_ticks, delay, binding.owner, binding.sid, parameter,
            )?;
        } else {
            // Original queue_remove removes ALL event types owned by object.
            remove_object_timer_events(queue, binding.owner);
        }
    }
    stack.truncate(at);
    Ok(())
}

impl BoundScriptRuntime {
    /// Explicitly gate timer opcodes to an isolated VM + caller-owned queue.
    /// No real engine callback or game-time progression occurs.
    pub fn run_timer_subset_bounded(
        &self,
        vm: &mut ScriptVm,
        queue: &mut EventQueue,
        now_ticks: i32,
        bindings: &[TimerObjectBinding],
        max_steps: usize,
    ) -> io::Result<usize> {
        vm.run_variable_subset_bounded(max_steps, &mut |opcode, stack| {
            apply_timer_native_opcode(self, queue, now_ticks, bindings, opcode, stack)
        })
    }
}

/// Original op_rm_timer_event delegates to queue_remove(object), which
/// removes every event type owned by that object, not just script timers.
pub fn remove_object_timer_events(queue: &mut EventQueue, owner: ObjectKey) -> usize {
    queue.remove_owner(Some(owner))
}

#[cfg(test)]
mod tests {
    use super::{
        apply_timer_native_opcode, remove_object_timer_events, ScriptTimerEvent,
        TimerObjectBinding, ADD_TIMER_EVENT_OPCODE, REMOVE_TIMER_EVENT_OPCODE,
        TIMED_PROCEDURE_ACTION,
    };
    use crate::collision::ObjectKey;
    use crate::event_queue::{EventQueue, EventType, QueueEvent};
    use crate::script_binding::{BoundScriptRuntime, InstalledScriptBinding, ScriptBinding};
    use crate::script_program::ScriptProgram;
    use crate::script_runtime::ScriptRuntime;
    use crate::script_vm::{ScriptValue, ScriptVm};

    fn prepared() -> BoundScriptRuntime {
        let names: [&[u8]; 2] = [b"main", b"timed_event_p_proc"];
        let mut bytes = vec![0_u8; 42];
        bytes.extend_from_slice(&2_i32.to_be_bytes());
        let mut identifiers = Vec::new();
        for name in names {
            let offset = (4 + identifiers.len()) as i32;
            for field in [offset, 0, 0, 0, 0, 0] {
                bytes.extend_from_slice(&field.to_be_bytes());
            }
            identifiers.extend_from_slice(name);
            identifiers.push(0);
        }
        bytes.extend_from_slice(&(identifiers.len() as i32).to_be_bytes());
        bytes.extend_from_slice(&identifiers);
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
                    script_id: 500,
                    script_index: 0,
                    owner_id: 77,
                    path: Some("scripts/SYNTH.int".to_owned()),
                    run_info: None,
                    local_variable_offset: -1,
                    local_variable_count: 0,
                },
                instance_id: Some(1),
            }],
        }
    }

    fn owner(ordinal: usize) -> ObjectKey {
        ObjectKey {
            elevation: 0,
            ordinal,
        }
    }

    fn bytecode(code: &[u8]) -> ScriptVm {
        assert!(code.len() <= 42);
        let mut bytes = vec![0_u8; 42];
        bytes[..code.len()].copy_from_slice(code);
        bytes.extend_from_slice(&[0_u8; 12]);
        ScriptVm::new(ScriptProgram::decode(&bytes).unwrap())
    }

    fn push(code: &mut Vec<u8>, opcode: u16, value: i32) {
        code.extend_from_slice(&opcode.to_be_bytes());
        code.extend_from_slice(&value.to_be_bytes());
    }

    #[test]
    fn ce_native_timer_add_and_remove_do_not_dispatch_scripts() {
        let bound = prepared();
        let object = TimerObjectBinding {
            handle: 77,
            owner: owner(1),
            sid: 500,
        };
        let mut queue = EventQueue::new();
        let mut add = Vec::new();
        push(&mut add, 0xE001, 77);
        push(&mut add, 0xC001, 20);
        push(&mut add, 0xC001, -17);
        add.extend_from_slice(&ADD_TIMER_EVENT_OPCODE.to_be_bytes());
        add.extend_from_slice(&0x8011_u16.to_be_bytes());
        let mut vm = bytecode(&add);
        assert_eq!(
            bound
                .run_timer_subset_bounded(&mut vm, &mut queue, 100, &[object], 8)
                .unwrap(),
            5
        );
        assert!(vm.halted());
        assert!(vm.stack().is_empty());
        assert_eq!(queue.len(), 1);
        assert_eq!(queue.next_time(), 120);
        assert!(!bound.runtime.vm(1).unwrap().halted());

        let mut remove = Vec::new();
        push(&mut remove, 0xE001, 77);
        remove.extend_from_slice(&REMOVE_TIMER_EVENT_OPCODE.to_be_bytes());
        remove.extend_from_slice(&0x8011_u16.to_be_bytes());
        let mut vm = bytecode(&remove);
        assert_eq!(
            bound
                .run_timer_subset_bounded(&mut vm, &mut queue, 100, &[object], 8)
                .unwrap(),
            3
        );
        assert!(queue.is_empty());
        assert!(vm.stack().is_empty());
    }

    #[test]
    fn ce_native_timer_null_object_and_bad_handles_are_bounded() {
        let bound = prepared();
        let mut queue = EventQueue::new();
        let mut null = vec![
            ScriptValue::Pointer(0),
            ScriptValue::Int(5),
            ScriptValue::Int(-3),
        ];
        apply_timer_native_opcode(
            &bound, &mut queue, 100, &[], ADD_TIMER_EVENT_OPCODE, &mut null,
        )
        .unwrap();
        assert!(null.is_empty());
        assert!(queue.is_empty());
        let original = vec![ScriptValue::Pointer(99)];
        let mut unknown = original.clone();
        assert!(apply_timer_native_opcode(
            &bound, &mut queue, 100, &[], REMOVE_TIMER_EVENT_OPCODE, &mut unknown,
        ).is_err());
        assert_eq!(unknown, original);
        let mut wrong = vec![ScriptValue::Int(1), ScriptValue::Int(2), ScriptValue::Int(3)];
        assert!(apply_timer_native_opcode(
            &bound, &mut queue, 100, &[], ADD_TIMER_EVENT_OPCODE, &mut wrong,
        ).is_err());
        assert_eq!(wrong.len(), 3);
    }

    #[test]
    fn source_ce_timer_payload_fifo_and_fixed_parameter_to_invocation() {
        let bound = prepared();
        let mut queue = EventQueue::new();
        assert!(!bound
            .queue_script_timer(&mut queue, 100, 20, owner(1), 999, 5)
            .unwrap());
        assert_eq!(queue.len(), 0);
        assert!(bound
            .queue_script_timer(&mut queue, 100, 20, owner(1), 500, -17)
            .unwrap());
        assert!(bound
            .queue_script_timer(&mut queue, 100, 20, owner(2), 500, 9)
            .unwrap());
        assert!(queue.pop_due(119).is_none());
        let first = queue.pop_due(120).unwrap();
        assert_eq!(first.payload, [0, 0, 1, 244, 255, 255, 255, 239]);
        let timer = ScriptTimerEvent::decode(&first).unwrap();
        assert_eq!(timer.sid, 500);
        assert_eq!(timer.fixed_parameter, -17);
        let planned = bound.plan_script_timer(&timer).unwrap().unwrap();
        assert_eq!(planned.action, TIMED_PROCEDURE_ACTION);
        assert_eq!(planned.procedure_index, 1);
        let context = planned.detached_invocation().unwrap();
        assert_eq!(context.action(), 22);
        assert_eq!(context.fixed_parameter(), -17);
        assert!(!context.overrides());
        assert_eq!(
            ScriptTimerEvent::decode(&queue.pop_due(120).unwrap())
                .unwrap()
                .fixed_parameter,
            9
        );
        assert!(queue.is_empty());
    }

    #[test]
    fn missing_int_or_malformed_timers_fail_without_game_side_effects() {
        let bound = prepared();
        let invalid = QueueEvent {
            time: 7,
            kind: EventType::Drug,
            owner: Some(owner(2)),
            payload: vec![0; 8],
        };
        assert!(ScriptTimerEvent::decode(&invalid).is_err());
        let invalid = QueueEvent {
            kind: EventType::Script,
            payload: vec![0; 7],
            ..invalid
        };
        assert!(ScriptTimerEvent::decode(&invalid).is_err());
        let unknown = ScriptTimerEvent {
            time: 20,
            owner: Some(owner(1)),
            sid: 1234,
            fixed_parameter: 5,
        };
        assert!(bound.plan_script_timer(&unknown).unwrap().is_none());
        let mut queue = EventQueue::new();
        bound
            .queue_script_timer(&mut queue, 0, 1, owner(1), 500, 5)
            .unwrap();
        queue
            .add(0, 1, EventType::Drug, Some(owner(1)), vec![])
            .unwrap();
        queue
            .add(0, 1, EventType::Drug, Some(owner(2)), vec![])
            .unwrap();
        assert_eq!(remove_object_timer_events(&mut queue, owner(1)), 2);
        assert_eq!(queue.len(), 1);
        assert_eq!(queue.pop_due(1).unwrap().owner, Some(owner(2)));
    }
}
