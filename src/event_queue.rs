//! Core chronological Fallout 1 CE event queue, without game callbacks.
//!
//! Source: pinned CE src/game/queue.cc queue_add, queue_remove,
//! queue_remove_this, queue_find, queue_process, queue_clear_type,
//! queue_next_time, and queue.h EventType.
//!
//! Events keep CE's ascending timestamp order and stable FIFO tie order.
//! Payload bytes and owner keys are Rust-owned; event-specific effects,
//! gameplay time, DAT/save serialization and reentrant scheduling are absent.
use crate::collision::ObjectKey;
use std::collections::VecDeque;
use std::io;

const MAX_QUEUED_EVENTS: usize = 100_000;

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum EventType {
    Drug = 0,
    Knockout = 1,
    Withdrawal = 2,
    Script = 3,
    GameTime = 4,
    Poison = 5,
    Radiation = 6,
    Flare = 7,
    Explosion = 8,
    ItemTrickle = 9,
    Sneak = 10,
    ExplosionFailure = 11,
    MapUpdate = 12,
}

impl EventType {
    pub fn from_ce_index(index: i32) -> Option<Self> {
        Some(match index {
            0 => Self::Drug,
            1 => Self::Knockout,
            2 => Self::Withdrawal,
            3 => Self::Script,
            4 => Self::GameTime,
            5 => Self::Poison,
            6 => Self::Radiation,
            7 => Self::Flare,
            8 => Self::Explosion,
            9 => Self::ItemTrickle,
            10 => Self::Sneak,
            11 => Self::ExplosionFailure,
            12 => Self::MapUpdate,
            _ => return None,
        })
    }

    /// Original q_func[].field_10, not permission to remove an event without
    /// invoking its original map-exit cleanup callback.
    pub fn participates_in_map_cleanup(self) -> bool {
        !matches!(self, Self::Poison | Self::Radiation)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueEvent {
    pub time: i32,
    pub kind: EventType,
    pub owner: Option<ObjectKey>,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EventQueue {
    events: VecDeque<QueueEvent>,
}

impl EventQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// CE queue_next_time returns 0 for an empty queue (also a valid time).
    pub fn next_time(&self) -> i32 {
        self.events.front().map_or(0, |event| event.time)
    }

    /// Match queue_add's ascending time and insertion after existing equal
    /// timestamps. Delay can be negative just as in the original CE.
    pub fn add(
        &mut self,
        now: i32,
        delay: i32,
        kind: EventType,
        owner: Option<ObjectKey>,
        payload: Vec<u8>,
    ) -> io::Result<()> {
        let time = now
            .checked_add(delay)
            .ok_or_else(|| invalid("CE event timestamp overflows i32"))?;
        if self.events.len() >= MAX_QUEUED_EVENTS {
            return Err(invalid("CE event queue exceeds safe size"));
        }
        let index = self
            .events
            .iter()
            .position(|event| event.time > time)
            .unwrap_or(self.events.len());
        self.events.insert(
            index,
            QueueEvent {
                time,
                kind,
                owner,
                payload,
            },
        );
        Ok(())
    }

    pub fn find(&self, owner: Option<ObjectKey>, kind: EventType) -> bool {
        self.events
            .iter()
            .any(|event| event.owner == owner && event.kind == kind)
    }

    pub fn remove_owner(&mut self, owner: Option<ObjectKey>) -> usize {
        let before = self.events.len();
        self.events.retain(|event| event.owner != owner);
        before - self.events.len()
    }

    pub fn remove_owner_type(&mut self, owner: Option<ObjectKey>, kind: EventType) -> usize {
        let before = self.events.len();
        self.events
            .retain(|event| !(event.owner == owner && event.kind == kind));
        before - self.events.len()
    }

    /// Simplified queue_clear_type: predicate returning true mirrors the CE
    /// cleanup callback's return value 1 (free/remove), false keeps event.
    pub fn clear_type_where(
        &mut self,
        kind: EventType,
        mut should_remove: impl FnMut(&QueueEvent) -> bool,
    ) -> usize {
        let before = self.events.len();
        self.events
            .retain(|event| event.kind != kind || !should_remove(event));
        before - self.events.len()
    }

    pub fn clear(&mut self) {
        self.events.clear();
    }

    /// Pop exactly one event due at the supplied game-time snapshot. The
    /// caller may execute its handler and add new events before the next pop.
    pub fn pop_due(&mut self, now: i32) -> Option<QueueEvent> {
        if self.events.front().is_some_and(|event| event.time <= now) {
            self.events.pop_front()
        } else {
            None
        }
    }

    /// CE queue_process processes overdue events in stable order, stopping
    /// when a handler's integer return value is nonzero. The supplied time
    /// is sampled only once; a handler must not advance time implicitly.
    /// To enqueue from a handler, use pop_due and dispatch externally.
    pub fn process_due(&mut self, now: i32, mut handler: impl FnMut(&QueueEvent) -> i32) -> i32 {
        while let Some(event) = self.pop_due(now) {
            let result = handler(&event);
            if result != 0 {
                return result;
            }
        }
        0
    }
}

#[cfg(test)]
mod tests {
    use super::{EventQueue, EventType};
    use crate::collision::ObjectKey;

    fn actor(ordinal: usize) -> Option<ObjectKey> {
        Some(ObjectKey {
            elevation: 0,
            ordinal,
        })
    }

    #[test]
    fn ce_timestamps_are_ascending_and_equal_time_events_stay_fifo() {
        let mut queue = EventQueue::new();
        assert_eq!(queue.next_time(), 0);
        queue
            .add(100, 10, EventType::Script, actor(1), vec![1])
            .unwrap();
        queue
            .add(100, 5, EventType::Poison, actor(2), vec![2])
            .unwrap();
        queue
            .add(100, 10, EventType::Drug, actor(3), vec![3])
            .unwrap();
        queue
            .add(100, -5, EventType::Sneak, actor(4), vec![4])
            .unwrap();
        assert_eq!(queue.len(), 4);
        assert_eq!(queue.next_time(), 95);
        assert!(queue.pop_due(94).is_none());
        let mut seen = vec![];
        queue.process_due(110, |event| {
            seen.push((event.time, event.payload[0]));
            0
        });
        assert_eq!(seen, [(95, 4), (105, 2), (110, 1), (110, 3)]);
        assert!(queue.is_empty());
    }

    #[test]
    fn ce_handler_nonzero_stops_due_processing_and_preserves_rest() {
        let mut queue = EventQueue::new();
        for number in 0..3 {
            queue
                .add(0, 0, EventType::Script, actor(number), vec![number as u8])
                .unwrap();
        }
        let mut processed = Vec::new();
        assert_eq!(
            queue.process_due(0, |event| {
                processed.push(event.payload[0]);
                if processed.len() == 2 {
                    1
                } else {
                    0
                }
            }),
            1
        );
        assert_eq!(processed, [0, 1]);
        assert_eq!(queue.len(), 1);
        assert_eq!(queue.pop_due(0).unwrap().payload, vec![2]);
    }

    #[test]
    fn removal_by_owner_type_and_ce_map_leave_flags() {
        let mut queue = EventQueue::new();
        queue
            .add(20, 1, EventType::Poison, actor(1), vec![])
            .unwrap();
        queue.add(20, 1, EventType::Drug, actor(1), vec![]).unwrap();
        queue.add(20, 2, EventType::Drug, actor(2), vec![]).unwrap();
        queue.add(20, 3, EventType::Drug, None, vec![]).unwrap();
        assert!(queue.find(actor(1), EventType::Drug));
        assert_eq!(queue.remove_owner_type(actor(1), EventType::Drug), 1);
        assert!(!queue.find(actor(1), EventType::Drug));
        assert_eq!(queue.remove_owner(actor(1)), 1);
        assert_eq!(
            queue.clear_type_where(EventType::Drug, |event| event.owner == actor(2)),
            1
        );
        assert_eq!(queue.len(), 1);
        assert_eq!(queue.remove_owner(None), 1);
        assert!(!EventType::Poison.participates_in_map_cleanup());
        assert!(!EventType::Radiation.participates_in_map_cleanup());
        assert!(EventType::Flare.participates_in_map_cleanup());
    }

    #[test]
    fn validates_type_indices_and_rejects_clock_overflow() {
        assert_eq!(EventType::from_ce_index(0), Some(EventType::Drug));
        assert_eq!(EventType::from_ce_index(12), Some(EventType::MapUpdate));
        assert_eq!(EventType::from_ce_index(13), None);
        assert_eq!(EventType::from_ce_index(-1), None);
        assert!(EventQueue::new()
            .add(i32::MAX, 1, EventType::Script, None, vec![])
            .is_err());
    }
}
