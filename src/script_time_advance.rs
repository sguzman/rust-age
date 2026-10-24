//! Source-derived, nonexecuting Fallout 1 CE game_time_advance planner.
//! Pinned src/int/support/intextra.cc op_game_time_advance (0x80FC).
//! The original advances one full day then calls queue_process() for each
//! positive full day; it finally advances the signed remainder and calls
//! queue_process() again, including when the remainder is zero.
//! This module only describes that order. It never advances a live clock,
//! processes an event, or invokes a game script.
use crate::event_queue::{EventQueue, EventType};
use crate::script_binding::BoundScriptRuntime;
use crate::script_clock::ScriptGameClock;
use crate::script_timer::ScriptTimerEvent;
use crate::script_vm::ScriptValue;
use std::io;

pub const GAME_TIME_ADVANCE_OPCODE: u16 = 0x80FC;
pub const GAME_TIME_TICKS_PER_DAY: i32 = 864_000;

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

/// Every increment must be followed by exactly one queue_process() call
/// in a future, separately verified game runtime. The last increment may
/// be zero or negative; this is intentional pinned-CE behavior.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeAdvancePlan {
    increments_before_queue_process: Vec<i32>,
}

impl TimeAdvancePlan {
    /// Prepare the original loop's increments with an explicit safety limit
    /// on queue_process() calls. The bound is checked before allocating.
    pub fn from_ticks(delta_ticks: i32, max_queue_polls: usize) -> io::Result<Self> {
        // C++ integer division truncates toward zero. A negative number of
        // days performs no full-day iterations; do not use Euclidean div.
        let full_days = (delta_ticks / GAME_TIME_TICKS_PER_DAY).max(0) as usize;
        let required_polls = full_days
            .checked_add(1)
            .ok_or_else(|| invalid("CE time advance queue poll count overflow"))?;
        if required_polls > max_queue_polls {
            return Err(invalid("CE time advance exceeds queue poll bound"));
        }
        let mut increments_before_queue_process = vec![GAME_TIME_TICKS_PER_DAY; full_days];
        increments_before_queue_process.push(delta_ticks % GAME_TIME_TICKS_PER_DAY);
        Ok(Self {
            increments_before_queue_process,
        })
    }

    pub fn increments_before_queue_process(&self) -> &[i32] {
        &self.increments_before_queue_process
    }

    pub fn queue_polls(&self) -> usize {
        self.increments_before_queue_process.len()
    }
}

/// Decode the pinned CE opcode's single signed integer stack operand and
/// return only a nonexecuting plan. On invalid input or exceeded limits,
/// neither the stack nor any other state is changed.
pub fn plan_game_time_advance_opcode(
    opcode: u16,
    stack: &mut Vec<ScriptValue>,
    max_queue_polls: usize,
) -> io::Result<TimeAdvancePlan> {
    if opcode != GAME_TIME_ADVANCE_OPCODE {
        return Err(invalid("unsupported CE game-time advance opcode"));
    }
    let Some(&ScriptValue::Int(delta_ticks)) = stack.last() else {
        return Err(invalid("CE game_time_advance expects signed integer ticks"));
    };
    let plan = TimeAdvancePlan::from_ticks(delta_ticks, max_queue_polls)?;
    stack.pop();
    Ok(plan)
}

/// Execute a bounded CE 0x80FC when no source queue_process callback
/// would fire. Both ordinary interpreter and synchronous critical actions
/// use the same source-order whole-day/final-remainder implementation.
///
/// A due event at ANY poll, even the final zero remainder, is an error.
/// Rejecting a due callback is deliberate until reentrant CE dispatch is
/// implemented. Operand stack, clock and cumulative poll budget remain
/// unchanged if the operation is rejected; the enclosing script transaction
/// handles complete MAP/quest/queue rollback.
pub fn apply_game_time_advance_no_due_opcode(
    opcode: u16,
    stack: &mut Vec<ScriptValue>,
    clock: &mut ScriptGameClock,
    queue: &EventQueue,
    remaining_queue_polls: &mut usize,
) -> io::Result<()> {
    let mut pending_stack = stack.clone();
    let plan = plan_game_time_advance_opcode(opcode, &mut pending_stack, *remaining_queue_polls)?;
    let mut pending_clock = *clock;
    for &increment in plan.increments_before_queue_process() {
        pending_clock.ticks = pending_clock
            .ticks
            .checked_add(increment)
            .ok_or_else(|| invalid("CE game_time_advance signed clock overflow"))?;
        if !queue.is_empty() && queue.next_time() <= pending_clock.ticks {
            return Err(invalid(
                "CE game_time_advance due queue event needs reentrant dispatch",
            ));
        }
    }
    *remaining_queue_polls -= plan.queue_polls();
    *stack = pending_stack;
    *clock = pending_clock;
    Ok(())
}

/// Original script_q_process is a no-op when the timer's script SID
/// is absent from the complete installed script registry. It is NOT a
/// no-op for an existing script with a missing timed procedure, because
/// CE still updates that script's fixedParam and event action. Absent-SID
/// records can be consumed during the pinned 0x80FC queue_process poll
/// without executing a callback. Other due types and any executable script
/// timer still require reentrant dispatch, which this boundary rejects.
///
/// Even when earlier inert timers are consumed, later unsupported events,
/// integer overflow, or exhausted queue-poll budget revert the original
/// queue, clock and operand stack. The installed MAP runtime owns the
/// remaining VM/global/quest transaction.
pub fn apply_game_time_advance_with_inert_timers_opcode(
    opcode: u16,
    stack: &mut Vec<ScriptValue>,
    clock: &mut ScriptGameClock,
    queue: &mut EventQueue,
    bound: &BoundScriptRuntime,
    remaining_queue_polls: &mut usize,
) -> io::Result<()> {
    let mut pending_stack = stack.clone();
    let plan = plan_game_time_advance_opcode(opcode, &mut pending_stack, *remaining_queue_polls)?;
    let mut pending_clock = *clock;
    let mut pending_queue = queue.clone();
    for &increment in plan.increments_before_queue_process() {
        pending_clock.ticks = pending_clock
            .ticks
            .checked_add(increment)
            .ok_or_else(|| invalid("CE game_time_advance signed clock overflow"))?;
        while let Some(event) = pending_queue.pop_due(pending_clock.ticks) {
            if event.kind != EventType::Script {
                return Err(invalid(
                    "CE game_time_advance due non-script queue handler unavailable",
                ));
            }
            let timer = ScriptTimerEvent::decode(&event)?;
            if bound
                .entries
                .iter()
                .any(|entry| entry.binding.script_id == timer.sid)
            {
                return Err(invalid(
                    "CE game_time_advance installed script timer needs reentrant dispatch",
                ));
            }
        }
    }
    *remaining_queue_polls -= plan.queue_polls();
    *stack = pending_stack;
    *clock = pending_clock;
    *queue = pending_queue;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        apply_game_time_advance_no_due_opcode, plan_game_time_advance_opcode, TimeAdvancePlan,
        GAME_TIME_ADVANCE_OPCODE, GAME_TIME_TICKS_PER_DAY,
    };
    use crate::event_queue::{EventQueue, EventType};
    use crate::script_clock::ScriptGameClock;
    use crate::script_vm::ScriptValue;

    #[test]
    fn all_no_due_native_polls_share_an_atomic_clock_operand_and_budget() {
        let mut clock = ScriptGameClock { ticks: 100 };
        let mut stack = vec![ScriptValue::Int(864_000)];
        let queue = EventQueue::new();
        let mut limit = 2;
        apply_game_time_advance_no_due_opcode(
            GAME_TIME_ADVANCE_OPCODE,
            &mut stack,
            &mut clock,
            &queue,
            &mut limit,
        )
        .unwrap();
        assert_eq!(clock.ticks, 864_100);
        assert!(stack.is_empty());
        assert_eq!(limit, 0);

        let mut due = EventQueue::new();
        due.add(0, 864_100, EventType::Script, None, vec![])
            .unwrap();
        for (start, increment, polls, expected_queue) in [
            (100_i32, 864_000_i32, 2_usize, &due),
            (100, 864_000, 1, &queue),
            (i32::MAX, 1, 1, &queue),
        ] {
            let mut pending = ScriptGameClock { ticks: start };
            let mut stack = vec![ScriptValue::Int(increment)];
            let mut remaining = polls;
            assert!(apply_game_time_advance_no_due_opcode(
                GAME_TIME_ADVANCE_OPCODE,
                &mut stack,
                &mut pending,
                expected_queue,
                &mut remaining,
            )
            .is_err());
            assert_eq!(pending.ticks, start);
            assert_eq!(stack, [ScriptValue::Int(increment)]);
            assert_eq!(remaining, polls);
        }
    }

    #[test]
    fn ce_daily_chunks_and_final_zero_poll_are_preserved() {
        for (delta, expected) in [
            (0, vec![0]),
            (1, vec![1]),
            (
                GAME_TIME_TICKS_PER_DAY - 1,
                vec![GAME_TIME_TICKS_PER_DAY - 1],
            ),
            (GAME_TIME_TICKS_PER_DAY, vec![GAME_TIME_TICKS_PER_DAY, 0]),
            (
                GAME_TIME_TICKS_PER_DAY * 2 + 7,
                vec![GAME_TIME_TICKS_PER_DAY, GAME_TIME_TICKS_PER_DAY, 7],
            ),
            (-1, vec![-1]),
            (-GAME_TIME_TICKS_PER_DAY, vec![0]),
            (-GAME_TIME_TICKS_PER_DAY - 1, vec![-1]),
        ] {
            let plan = TimeAdvancePlan::from_ticks(delta, 10).unwrap();
            assert_eq!(plan.increments_before_queue_process(), expected);
            assert_eq!(plan.queue_polls(), expected.len());
        }
    }

    #[test]
    fn zero_queue_poll_budget_and_too_many_days_are_rejected() {
        assert!(TimeAdvancePlan::from_ticks(0, 0).is_err());
        assert!(TimeAdvancePlan::from_ticks(GAME_TIME_TICKS_PER_DAY, 1).is_err());
        assert_eq!(
            TimeAdvancePlan::from_ticks(GAME_TIME_TICKS_PER_DAY, 2)
                .unwrap()
                .queue_polls(),
            2
        );
        assert!(TimeAdvancePlan::from_ticks(i32::MAX, 2).is_err());
        assert_eq!(
            TimeAdvancePlan::from_ticks(i32::MIN, 1)
                .unwrap()
                .queue_polls(),
            1
        );
    }

    #[test]
    fn native_stack_operand_is_consumed_only_on_success() {
        let mut stack = vec![ScriptValue::Pointer(7), ScriptValue::Int(864_001)];
        let plan = plan_game_time_advance_opcode(GAME_TIME_ADVANCE_OPCODE, &mut stack, 2).unwrap();
        assert_eq!(plan.increments_before_queue_process(), [864_000, 1]);
        assert_eq!(stack, [ScriptValue::Pointer(7)]);

        for (opcode, values, max_polls) in [
            (GAME_TIME_ADVANCE_OPCODE, vec![], 10),
            (GAME_TIME_ADVANCE_OPCODE, vec![ScriptValue::Pointer(1)], 10),
            (GAME_TIME_ADVANCE_OPCODE, vec![ScriptValue::Int(864_000)], 1),
            (0x80FD, vec![ScriptValue::Int(1)], 10),
        ] {
            let mut stack = values.clone();
            assert!(plan_game_time_advance_opcode(opcode, &mut stack, max_polls).is_err());
            assert_eq!(stack, values);
        }
    }
}
