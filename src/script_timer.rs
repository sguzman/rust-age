//! CE script_q_add/save/load/process timer metadata, without callbacks.
//! Source: pinned game/scripts.cc script_q_* and game/queue.cc queue_add.
//! Payload bytes are exactly CE's big-endian (sid, fixedParam) pair.
//! The EventQueue only records game-time ticks; it never runs an INT script.
use crate::collision::ObjectKey;
use crate::event_queue::{EventQueue, EventType, QueueEvent};
use crate::script_binding::BoundScriptRuntime;
use crate::script_lifecycle::PlannedMapAction;
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
            event.payload[0..4].try_into().map_err(|_| invalid("invalid SID"))?,
        );
        let fixed_parameter = i32::from_be_bytes(
            event.payload[4..8].try_into().map_err(|_| invalid("invalid fixedParam"))?,
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
        if !self.entries.iter().any(|entry| entry.binding.script_id == sid) {
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
        let Some(entry) = self.entries.iter().find(|x| x.binding.script_id == timer.sid) else {
            return Ok(None);
        };
        let Some(instance) = entry.instance_id else {
            return Ok(None);
        };
        let Some(procedure) = self.runtime.resolve_action(instance, TIMED_PROCEDURE_ACTION)? else {
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

/// Original op_rm_timer_event delegates to queue_remove(object), which
/// removes every event type owned by that object, not just script timers.
pub fn remove_object_timer_events(queue: &mut EventQueue, owner: ObjectKey) -> usize {
    queue.remove_owner(Some(owner))
}

#[cfg(test)]
mod tests {
    use super::{remove_object_timer_events, ScriptTimerEvent, TIMED_PROCEDURE_ACTION};
    use crate::collision::ObjectKey;
    use crate::event_queue::{EventQueue, EventType, QueueEvent};
    use crate::script_binding::{BoundScriptRuntime, InstalledScriptBinding, ScriptBinding};
    use crate::script_program::ScriptProgram;
    use crate::script_runtime::ScriptRuntime;
    use crate::script_vm::ScriptVm;

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
            entries: vec![
                InstalledScriptBinding {
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
                },
            ],
        }
    }

    fn owner(ordinal: usize) -> ObjectKey {
        ObjectKey { elevation: 0, ordinal }
    }

    #[test]
    fn source_ce_timer_payload_fifo_and_fixed_parameter_to_invocation() {
        let bound = prepared();
        let mut queue = EventQueue::new();
        assert!(!bound.queue_script_timer(&mut queue, 100, 20, owner(1), 999, 5).unwrap());
        assert_eq!(queue.len(), 0);
        assert!(bound.queue_script_timer(&mut queue, 100, 20, owner(1), 500, -17).unwrap());
        assert!(bound.queue_script_timer(&mut queue, 100, 20, owner(2), 500, 9).unwrap());
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
        assert_eq!(ScriptTimerEvent::decode(&queue.pop_due(120).unwrap()).unwrap().fixed_parameter, 9);
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
        let invalid = QueueEvent { kind: EventType::Script, payload: vec![0; 7], ..invalid };
        assert!(ScriptTimerEvent::decode(&invalid).is_err());
        let unknown = ScriptTimerEvent {
            time: 20, owner: Some(owner(1)), sid: 1234, fixed_parameter: 5,
        };
        assert!(bound.plan_script_timer(&unknown).unwrap().is_none());
        let mut queue = EventQueue::new();
        bound.queue_script_timer(&mut queue, 0, 1, owner(1), 500, 5).unwrap();
        queue.add(0, 1, EventType::Drug, Some(owner(1)), vec![]).unwrap();
        queue.add(0, 1, EventType::Drug, Some(owner(2)), vec![]).unwrap();
        assert_eq!(remove_object_timer_events(&mut queue, owner(1)), 2);
        assert_eq!(queue.len(), 1);
        assert_eq!(queue.pop_due(1).unwrap().owner, Some(owner(2)));
    }
}
