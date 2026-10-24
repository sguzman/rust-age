//! Fallout 1 CE MAP loading and explicit, bounded Rust-owned script-state assembly.
//!
//! Mirrors map_load_file's data order: header, global/local variables,
//! floor/roof squares, script records, and object/inventory tables.
//! Only source file formats and collision are loaded. This is not a running
//! gameplay world: no scripts, party, lighting, time, or actor movement.
use crate::assets::{Bank, GameAssets};
use crate::collision::{CollisionIndex, ObjectKey};
use crate::event_queue::{EventQueue, EventType};
use crate::game_globals::GameGlobalState;
use crate::hex::HexViewport;
use crate::map::{MapHeader, MapSquares, MapVariables};
use crate::object_tables::ObjectTables;
use crate::pathfinding::make_path;
use crate::prototype::PrototypeBody;
use crate::script_binding::{
    bind_script_procedures, bind_scripts, install_map_scripts, BoundScriptRuntime, ScriptBinding,
    ScriptProcedureBinding,
};
use crate::script_clock::ScriptGameClock;
use crate::script_data::ScriptTables;
use crate::script_event_state::DetachedScriptEvent;
use crate::script_lifecycle::MapLifecyclePhase;
use crate::script_lifecycle::PlannedMapAction;
use crate::script_locals::{MapScriptChangeSummary, MapScriptLocals};
use crate::script_native_vars::{
    apply_map_variable_opcode, apply_mutable_condition_native_with_vars,
    apply_readonly_event_native_opcode, apply_readonly_event_native_with_vars, ScriptClockContext,
    ScriptInvocationState, ScriptNativeEventSnapshot, ScriptNativeOwnerSnapshot,
    ScriptNativeSnapshot, LOCAL_VAR_OPCODE, MAP_VAR_OPCODE, SCRIPT_OVERRIDES_OPCODE,
    SET_LOCAL_VAR_OPCODE, SET_MAP_VAR_OPCODE,
};
use crate::script_object_binding::MapObjectReadBindings;
use crate::script_owner_binding::{MapScriptOwners, ScriptOwnerResolution};
use crate::script_runtime::{
    PlannedLocalActionFrame, PreparedCoreTick, PreviewedBoundLocalActionVmFrame,
    ReadOnlyConditionEvent, TimerNativeTickContext,
};
use crate::script_time_advance::TimeAdvancePlan;
use crate::script_timer::{
    apply_timer_native_opcode, ScriptTimerEvent, TimerObjectBinding, ADD_TIMER_EVENT_OPCODE,
    REMOVE_TIMER_EVENT_OPCODE,
};
use crate::script_vm::{PreviewedConditionFrame, PreviewedDueProcedurePoll, ScriptValue};
use std::collections::BTreeMap;
use std::io;

#[derive(Debug, Clone)]
pub struct StaticScene {
    pub header: MapHeader,
    pub variables: MapVariables,
    pub squares: MapSquares,
    pub scripts: ScriptTables,
    pub objects: ObjectTables,
    pub collision: CollisionIndex,
    /// End of the serialized map object's section, for format inspection.
    pub bytes_consumed: usize,
}

/// Initially unstarted CE script environment: per-record VMs and MAP-local
/// slots are separate from game-wide GAME_GLOBAL_VARS. A missing installed
/// vault13.gam remains None; no quest default is invented.
#[derive(Debug, Clone)]
pub struct PreparedScriptEnvironment {
    pub scripts: BoundScriptRuntime,
    pub map_locals: MapScriptLocals,
    pub game_globals: Option<GameGlobalState>,
    /// Inert decoded object-query values with snapshot-local handles.
    pub object_queries: MapObjectReadBindings,
    /// Owner diagnostics from source-verified script SID and object SID joins.
    pub script_owners: MapScriptOwners,
    /// Original serialized MAP lastVisitTime in 1/10-second game ticks.
    /// Zero indicates no recorded visit; no gameplay time is advanced.
    pub map_last_visit_ticks: i32,
    /// Rust-owned source action/fixedParam state for actions explicitly
    /// staged through prepare_noncritical_map_action_transactional.
    /// Absent IDs retain their original initial action value zero.
    pub invocations: BTreeMap<u64, ScriptInvocationState>,
    /// Source MAP actor key from a source-verified spatial scr_set_objs
    /// event, paired with an explicit NULL target. Only that event
    /// authorizes source_obj; unresolved handles fail closed on use.
    pub spatial_sources: BTreeMap<u64, ObjectKey>,
}

/// Finite queue-process limits for source-shaped game_time_advance.
/// These do not change CE's whole-day/remainder callback order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptTimeAdvanceBudget {
    pub max_queue_polls: usize,
    pub max_events_per_poll: usize,
    pub max_critical_steps: usize,
}

/// Verified chunk sequence: every signed game-time increment is followed
/// by precisely one queue-process attempt, including a final zero chunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutedScriptTimeAdvance {
    pub increments: Vec<i32>,
    pub due_events_per_poll: Vec<Vec<(ScriptTimerEvent, Option<usize>)>>,
}

/// Pure sandbox result from a CLONED CE local script preview.
/// All state changes exist only in this returned value, never in the MAP,
/// installed VM, real GAME_GLOBAL_VARS or gameplay scheduler.
#[derive(Debug, Clone)]
pub struct PreviewedSandboxAction {
    pub returned: Option<ScriptValue>,
    /// Full candidate local CE interpreter state, never an installed VM.
    pub vm_frame: PreviewedBoundLocalActionVmFrame,
    pub map_locals: MapScriptLocals,
    pub game_globals: Option<GameGlobalState>,
    pub overrides: bool,
    baseline_map: MapScriptLocals,
    baseline_game_globals: Option<GameGlobalState>,
    baseline_procedures: Vec<crate::script_program::ProcedureEntry>,
}

/// An inspection-only description of speculative script changes. No engine
/// state can be modified by constructing, reading or discarding this value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SandboxEffectSummary {
    pub map: MapScriptChangeSummary,
    pub quest_globals_changed: usize,
    /// Count of hypothetical CE procedure-table entries whose stored
    /// schedule/flags differ; not a count of dispatched game callbacks.
    pub procedure_entries_changed: usize,
    pub overrides: bool,
}

impl PreviewedSandboxAction {
    /// Compare the hypothetical post-action state against its own exact
    /// pre-action snapshot; never infer changes from other MAP instances.
    pub fn effects(&self) -> io::Result<SandboxEffectSummary> {
        let map = self.map_locals.change_summary_since(&self.baseline_map)?;
        let quest_globals_changed = match (&self.baseline_game_globals, &self.game_globals) {
            (None, None) => 0,
            (Some(before), Some(after)) if before.values().len() == after.values().len() => before
                .values()
                .iter()
                .zip(after.values())
                .filter(|(left, right)| left != right)
                .count(),
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "CE sandbox quest-global layout changed",
                ));
            }
        };
        if self.baseline_procedures.len() != self.vm_frame.candidate.procedures.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "CE sandbox procedure table length changed",
            ));
        }
        let procedure_entries_changed = self
            .baseline_procedures
            .iter()
            .zip(&self.vm_frame.candidate.procedures)
            .filter(|(before, after)| before != after)
            .count();
        Ok(SandboxEffectSummary {
            map,
            quest_globals_changed,
            procedure_entries_changed,
            overrides: self.overrides,
        })
    }

    /// Explicitly CONSUME and discard this preview. Only the read-only
    /// summary survives. There is deliberately no commit/apply method:
    /// gameplay script dispatch and persistence remain unimplemented.
    pub fn discard(self) -> io::Result<SandboxEffectSummary> {
        self.effects()
    }
}

impl PreparedScriptEnvironment {
    /// Run a bounded Rust-owned source-ordered CE script tick on the MAP
    /// environment assembled by StaticScene::unstarted_script_environment.
    /// All instances inherit their verified original MAP owner joins and
    /// object-query handles. Source/target event pointers remain UNSET (not
    /// fictional NULL objects), and a caller supplies the explicit game
    /// clock. The event invocation starts at the initial CE action value
    /// zero; full gameplay event lifecycle and overrides remain unavailable.
    ///
    /// Mutations to attached script VMs, MAP/local variables and loaded
    /// GAME_GLOBAL_VARS commit only after the entire tick succeeds. Failure
    /// preserves all owned state; source MAP bytes and objects are not
    /// mutated, nor are native game callbacks, spatial engine owners,
    /// dialogue or savegame effects synthesized. A missing game-global
    /// bank rejects this opt-in mutable subset rather than inventing one.
    pub fn advance_script_state_tick_bounded(
        &mut self,
        clock: ScriptClockContext,
        now_millis: u32,
        cpu_burst: usize,
        max_steps_per_condition: usize,
    ) -> io::Result<PreparedCoreTick> {
        let mut events = BTreeMap::new();
        for entry in &self.scripts.entries {
            let Some(id) = entry.instance_id else {
                continue;
            };
            let owner = self
                .script_owners
                .resolution_for_instance(&self.scripts, id);

            let source = self
                .spatial_sources
                .get(&id)
                .map(|key| {
                    self.object_queries.handle_for(*key).ok_or_else(|| {
                        io::Error::new(
                            io::ErrorKind::InvalidData,
                            "CE spatial source object handle unavailable",
                        )
                    })
                })
                .transpose()?;
            let event = ReadOnlyConditionEvent {
                event: ScriptNativeEventSnapshot {
                    owner: ScriptNativeOwnerSnapshot {
                        native: ScriptNativeSnapshot {
                            clock,
                            objects: Some(self.object_queries.snapshots()),
                        },
                        resolution: owner,
                    },
                    source,
                    target: source.map(|_| 0),
                },
                invocation: self
                    .invocations
                    .get(&id)
                    .copied()
                    .unwrap_or_else(ScriptInvocationState::installed),
            };
            if events.insert(id, event).is_some() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "duplicate installed CE MAP interpreter instance",
                ));
            }
        }
        let game = self.game_globals.as_mut().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "CE mutable script tick requires loaded GAME_GLOBAL_VARS",
            )
        })?;
        self.scripts
            .runtime
            .advance_tick_with_mutable_native_state_transactional_bounded(
                now_millis,
                cpu_burst,
                max_steps_per_condition,
                &mut self.map_locals,
                game,
                &events,
            )
    }

    /// Run one installed MAP script tick with CE script queue add/remove
    /// natives connected to a caller-owned, timestamped EventQueue.
    /// Timer object pointers are resolved only from source-verified MAP
    /// object-owner bindings; unknown, ambiguous or synthetic pointers are
    /// rejected. The queue is cloned together with the VMs and quest/MAP
    /// banks, then committed only after successful interpreter bursts and
    /// conditional event dispatch. Actual queue_process is a separate,
    /// explicit game-time phase handled by process_due_script_timers.
    pub fn advance_script_state_with_timer_queue_bounded(
        &mut self,
        queue: &mut EventQueue,
        clock: ScriptClockContext,
        now_millis: u32,
        cpu_burst: usize,
        max_steps_per_condition: usize,
    ) -> io::Result<PreparedCoreTick> {
        let mut next = self.clone();
        let mut queued = queue.clone();
        let mut events = BTreeMap::new();
        let mut timer_bindings = Vec::new();
        for entry in &next.scripts.entries {
            let Some(id) = entry.instance_id else {
                continue;
            };
            let owner = next
                .script_owners
                .resolution_for_instance(&next.scripts, id);
            if let Some(ScriptOwnerResolution::Bound { key, handle, .. }) = owner {
                timer_bindings.push(TimerObjectBinding {
                    handle,
                    owner: key,
                    sid: entry.binding.script_id,
                });
            }
            let source = next
                .spatial_sources
                .get(&id)
                .map(|key| {
                    next.object_queries.handle_for(*key).ok_or_else(|| {
                        io::Error::new(
                            io::ErrorKind::InvalidData,
                            "CE timer tick spatial source handle unavailable",
                        )
                    })
                })
                .transpose()?;
            let event = ReadOnlyConditionEvent {
                event: ScriptNativeEventSnapshot {
                    owner: ScriptNativeOwnerSnapshot {
                        native: ScriptNativeSnapshot {
                            clock,
                            objects: Some(next.object_queries.snapshots()),
                        },
                        resolution: owner,
                    },
                    source,
                    target: source.map(|_| 0),
                },
                invocation: next
                    .invocations
                    .get(&id)
                    .copied()
                    .unwrap_or_else(ScriptInvocationState::installed),
            };
            if events.insert(id, event).is_some() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "CE timer tick repeated one installed script identity",
                ));
            }
        }
        let bound = next.scripts.clone();
        let game = next.game_globals.as_mut().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "CE timer native tick requires loaded GAME_GLOBAL_VARS",
            )
        })?;
        let executed = next
            .scripts
            .runtime
            .advance_tick_with_mutable_native_timer_queue_transactional_bounded(
                now_millis,
                cpu_burst,
                max_steps_per_condition,
                TimerNativeTickContext {
                    map: &mut next.map_locals,
                    game,
                    queue: &mut queued,
                    now_game_ticks: clock.clock.ticks,
                    bound: &bound,
                    timer_bindings: &timer_bindings,
                    events: &events,
                },
            )?;
        *self = next;
        *queue = queued;
        Ok(executed)
    }

    /// Persist a source-verified, planned NONCRITICAL MAP lifecycle
    /// executeProc frame and its action/fixedParam invocation context.
    /// Source script selection and owner identity must match the original
    /// MAP bindings; no invented action names, procedure bytecode or object
    /// identities are permitted. The original CE noncritical setupCall
    /// defers bytecode until a later interpreter burst. Critical actions
    /// must execute immediately in CE and are intentionally refused here.
    ///
    /// This is source-faithful CALL PREPARATION, not automatic START,
    /// MAP_ENTER, spatial events, gameplay-native effects, or any actor
    /// interaction. A failure leaves VM, MAP and invocation state intact.
    pub fn prepare_noncritical_map_action_transactional(
        &mut self,
        planned: &PlannedMapAction,
    ) -> io::Result<PlannedLocalActionFrame> {
        let verified = self.local_frame_for_planned(planned)?;
        if verified.critical {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "CE critical MAP action requires immediate interpretation",
            ));
        }
        // CE exec_script_proc updates the action and resets overrides,
        // but leaves script->fixedParam unchanged unless the surrounding
        // MAP_ENTER event explicitly sets it.
        let mut invocation = self
            .invocations
            .get(&planned.instance_id)
            .copied()
            .unwrap_or_else(ScriptInvocationState::installed);
        if !invocation.begin_action(
            i32::try_from(planned.action).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "CE MAP action outside i32")
            })?,
        ) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "CE MAP lifecycle invocation is absent",
            ));
        }
        if let Some(parameter) = planned.external_parameter {
            invocation.set_fixed_parameter(parameter);
        }
        let mut next = self.clone();
        let installed = next
            .scripts
            .runtime
            .prepare_noncritical_local_action_transactional(planned.instance_id, planned.action)?
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "CE planned MAP action no longer exists",
                )
            })?;
        if installed != verified {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "CE planned MAP action frame changed before commit",
            ));
        }
        next.invocations.insert(planned.instance_id, invocation);
        *self = next;
        Ok(verified)
    }

    /// Execute an original CE CRITICAL local MAP lifecycle procedure
    /// immediately (executeProc -> setupCall -> op_critical_start ->
    /// interpret(program, 0)), rather than deferring its call frame.
    /// The selected procedure, installed MAP identity and owner must
    /// be verified before any effects. Only existing source-derived MAP,
    /// game-global, clock, invocation, and immutable object native
    /// adapters are permitted; arbitrary engine gameplay is NOT emulated.
    ///
    /// The entire prepared MAP environment is cloned first. Unsupported
    /// native effects, nested critical/WAIT frames and an exhausted
    /// 4096-instruction maximum roll back all VM, quest and MAP writes.
    /// This deliberately differs from CE's nontransactional error path.
    pub fn execute_critical_map_action_transactional_bounded(
        &mut self,
        planned: &PlannedMapAction,
        clock: ScriptClockContext,
        now_millis: u32,
        max_steps: usize,
    ) -> io::Result<usize> {
        self.execute_critical_map_action_with_timer_queue_transactional_bounded(
            planned, clock, now_millis, max_steps, None,
        )
    }

    /// Internal critical native host that can also route original timer
    /// queue add/remove callbacks. The queue and script/quest state are
    /// separately cloned before any interpreter bytecode executes.
    /// Critical callbacks may schedule another due timer at the same
    /// game-time tick; the enclosing queue_process loop will visit it.
    fn execute_critical_map_action_with_timer_queue_transactional_bounded(
        &mut self,
        planned: &PlannedMapAction,
        clock: ScriptClockContext,
        now_millis: u32,
        max_steps: usize,
        timer_queue: Option<&mut EventQueue>,
    ) -> io::Result<usize> {
        let verified = self.local_frame_for_planned(planned)?;
        if !verified.critical {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "CE noncritical MAP action must be deferred to later interpreter tick",
            ));
        }
        let owner = self
            .owner_for_instance(planned.instance_id)
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "CE critical MAP owner missing")
            })?;
        let mut invocation = self
            .invocations
            .get(&planned.instance_id)
            .copied()
            .unwrap_or_else(ScriptInvocationState::installed);
        let action = i32::try_from(planned.action).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "CE critical MAP action outside i32",
            )
        })?;
        if !invocation.begin_action(action) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "CE critical MAP invocation absent",
            ));
        }
        if let Some(parameter) = planned.external_parameter {
            invocation.set_fixed_parameter(parameter);
        }
        let mut next = self.clone();
        let source = next
            .spatial_sources
            .get(&planned.instance_id)
            .map(|key| {
                next.object_queries.handle_for(*key).ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "CE spatial critical event source handle unavailable",
                    )
                })
            })
            .transpose()?;
        let snapshot = ScriptNativeEventSnapshot {
            owner: ScriptNativeOwnerSnapshot {
                native: ScriptNativeSnapshot {
                    clock,
                    objects: Some(next.object_queries.snapshots()),
                },
                resolution: Some(owner),
            },
            source,
            target: source.map(|_| 0),
        };
        // A clone of the immutable MAP instance registry is needed for
        // timer SID resolution while the interpreter is mutably borrowed.
        // Each native object pointer is validated against its original
        // source-owned ObjectKey, not synthesized from a numeric address.
        let timer_bindings: Vec<_> = next
            .scripts
            .entries
            .iter()
            .filter_map(|entry| {
                let id = entry.instance_id?;
                match next
                    .script_owners
                    .resolution_for_instance(&next.scripts, id)
                {
                    Some(ScriptOwnerResolution::Bound { key, handle, .. }) => {
                        Some(TimerObjectBinding {
                            handle,
                            owner: key,
                            sid: entry.binding.script_id,
                        })
                    }
                    _ => None,
                }
            })
            .collect();
        let bound = next.scripts.clone();
        let mut queued = timer_queue.as_ref().map(|queue| (**queue).clone());
        let game = next.game_globals.as_mut().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "CE critical MAP action requires loaded GAME_GLOBAL_VARS",
            )
        })?;
        let executed = next
            .scripts
            .runtime
            .execute_critical_local_action_with_host_bounded(
                planned.instance_id,
                planned.action,
                now_millis,
                max_steps,
                &mut |opcode, stack| {
                    if opcode == ADD_TIMER_EVENT_OPCODE || opcode == REMOVE_TIMER_EVENT_OPCODE {
                        let queue = queued.as_mut().ok_or_else(|| {
                            io::Error::new(
                                io::ErrorKind::InvalidData,
                                "CE critical timer native requires explicitly owned event queue",
                            )
                        })?;
                        apply_timer_native_opcode(
                            &bound,
                            queue,
                            clock.clock.ticks,
                            &timer_bindings,
                            opcode,
                            stack,
                        )
                    } else {
                        apply_mutable_condition_native_with_vars(
                            snapshot,
                            &invocation,
                            &mut next.map_locals,
                            game,
                            planned.instance_id,
                            opcode,
                            stack,
                        )
                    }
                },
            )?
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "CE planned critical MAP procedure disappeared",
                )
            })?;
        next.invocations.insert(planned.instance_id, invocation);
        if let (Some(original), Some(staged)) = (timer_queue, queued) {
            *original = staged;
        }
        *self = next;
        Ok(executed)
    }

    /// Prepare an entire original-order MAP lifecycle phase, using the
    /// existing scr_exec_map_* selection rules and one atomic Rust-owned
    /// transaction. A missing built-in procedure remains absent, not an
    /// invented call. Each selected instance receives its source local
    /// executeProc/setupCall frame and action/fixedParam state. The caller
    /// can subsequently run advance_script_state_tick_bounded to interpret
    /// these prepared NONCRITICAL procedure bodies.
    ///
    /// Unlike original CE, which may leave already-started scripts changed
    /// after an error, a later unsupported critical/imported/argumented
    /// procedure aborts the whole candidate without partial VM or invocation
    /// writes. Duplicate instance entries fail instead of overwriting a
    /// pending call frame. Critical lifecycle actions are NOT silently
    /// deferred; they require synchronous interpret in the full port.
    pub fn prepare_noncritical_map_lifecycle_transactional(
        &mut self,
        phase: MapLifecyclePhase,
        map_script_id: i32,
        map_flags: i32,
    ) -> io::Result<Vec<PlannedLocalActionFrame>> {
        let selected = self
            .scripts
            .plan_map_lifecycle(phase, map_script_id, map_flags)?;
        let mut next = self.clone();
        let mut staged = Vec::with_capacity(selected.len());
        let mut seen = std::collections::BTreeSet::new();
        for action in &selected {
            if !seen.insert(action.instance_id) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "CE MAP lifecycle repeated one installed script instance",
                ));
            }
            staged.push(next.prepare_noncritical_map_action_transactional(action)?);
        }
        *self = next;
        Ok(staged)
    }

    /// Commit a complete source-ordered MAP lifecycle phase including
    /// original CE executeProc's two distinct paths: noncritical local
    /// procedures stage a frame for the next interpreter burst, while
    /// CRITICAL local procedures execute synchronously with interpret(0).
    /// Each returned pair carries the source plan and either the number
    /// of immediately interpreted critical opcodes or None for a deferred
    /// noncritical call.
    ///
    /// The whole phase runs on a cloned MAP environment. Earlier critical
    /// quest-variable writes are visible to later actions; any unsupported
    /// native opcode, ambiguous owner, repeated instance, or exhausted
    /// critical budget rolls back the entire phase (unlike CE longjmp
    /// side effects). Real gameplay callbacks and automated MAP loading
    /// remain separate acceptance gates.
    pub fn apply_map_lifecycle_phase_transactional_bounded(
        &mut self,
        phase: MapLifecyclePhase,
        map_script_id: i32,
        map_flags: i32,
        clock: ScriptClockContext,
        now_millis: u32,
        max_critical_steps: usize,
    ) -> io::Result<Vec<(PlannedMapAction, Option<usize>)>> {
        let selected = self
            .scripts
            .plan_map_lifecycle(phase, map_script_id, map_flags)?;
        let mut next = self.clone();
        let mut seen = std::collections::BTreeSet::new();
        let mut applied = Vec::with_capacity(selected.len());
        for action in selected {
            if !seen.insert(action.instance_id) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "CE MAP phase repeated an installed script instance",
                ));
            }
            let critical = next.local_frame_for_planned(&action)?.critical;
            let immediately_executed = if critical {
                Some(next.execute_critical_map_action_transactional_bounded(
                    &action,
                    clock,
                    now_millis,
                    max_critical_steps,
                )?)
            } else {
                next.prepare_noncritical_map_action_transactional(&action)?;
                None
            };
            applied.push((action, immediately_executed));
        }
        *self = next;
        Ok(applied)
    }

    /// Process already-queued original CE EVENT_TYPE_SCRIPT records in
    /// chronological FIFO order. Pinned script_q_process restores each
    /// event's fixedParam, then calls SCRIPT_PROC_TIMED (22). Critical
    /// procedure bodies run immediately and noncritical ones stage frames.
    /// The caller provides an explicit game-time sample and trusted queue:
    /// this method does not advance time or synthesize timer ownership.
    ///
    /// Queue entries for missing script IDs or timed procedures are consumed
    /// without inventing a VM. Unsupported due event types, stale bindings,
    /// invalid native effects, and exceeded event/instruction limits fail
    /// the entire transaction, preserving both the queue and MAP/quest state.
    /// Original CE can leave partial effects on native error; Rust Age
    /// deliberately holds this safer fail-closed acceptance boundary.
    pub fn process_due_script_timers_transactional_bounded(
        &mut self,
        queue: &mut EventQueue,
        clock: ScriptClockContext,
        now_ticks: i32,
        now_millis: u32,
        max_events: usize,
        max_critical_steps: usize,
    ) -> io::Result<Vec<(ScriptTimerEvent, Option<usize>)>> {
        if max_events > 4096 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "CE script timer processing exceeds event safety bound",
            ));
        }
        let mut candidate = self.clone();
        let mut pending = queue.clone();
        let mut applied = Vec::new();
        while applied.len() < max_events {
            let Some(event) = pending.pop_due(now_ticks) else {
                break;
            };
            if event.kind != EventType::Script {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "CE game queue contains unsupported due event before script timer",
                ));
            }
            let timer = ScriptTimerEvent::decode(&event)?;
            let steps = if let Some(plan) = candidate.scripts.plan_script_timer(&timer)? {
                if candidate.local_frame_for_planned(&plan)?.critical {
                    Some(
                        candidate
                            .execute_critical_map_action_with_timer_queue_transactional_bounded(
                                &plan,
                                clock,
                                now_millis,
                                max_critical_steps,
                                Some(&mut pending),
                            )?,
                    )
                } else {
                    candidate.prepare_noncritical_map_action_transactional(&plan)?;
                    None
                }
            } else {
                None
            };
            applied.push((timer, steps));
        }
        if !pending.is_empty() && pending.next_time() <= now_ticks {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "CE due script event queue exceeded bounded processing budget",
            ));
        }
        *self = candidate;
        *queue = pending;
        Ok(applied)
    }

    /// Execute pinned CE game_time_advance's whole-day and signed
    /// remainder chunks, running a due EVENT_TYPE_SCRIPT queue poll after
    /// EACH increment, including the mandatory final zero increment.
    /// Timing arithmetic follows TimeAdvancePlan's signed C++ division.
    /// Only source-backed script timers and allowlisted quest/MAP effects
    /// are supported; other due queue kinds fail closed.
    ///
    /// The entire installed script environment, game clock and queue are
    /// cloned before the first increment. Even when a later queue poll or
    /// critical timed procedure fails, no intermediate clock, quest state
    /// or queue events are committed. This is a Rust transactional safety
    /// boundary, NOT a full CE game_time_advance native dispatch from an
    /// actively interpreting script or a general world/actor time loop.
    pub fn advance_game_time_with_script_timers_transactional_bounded(
        &mut self,
        queue: &mut EventQueue,
        clock: &mut ScriptGameClock,
        delta_ticks: i32,
        now_millis: u32,
        budget: ScriptTimeAdvanceBudget,
    ) -> io::Result<ExecutedScriptTimeAdvance> {
        let plan = TimeAdvancePlan::from_ticks(delta_ticks, budget.max_queue_polls)?;
        let mut state = self.clone();
        let mut queued = queue.clone();
        let mut next_clock = *clock;
        let mut per_poll = Vec::with_capacity(plan.queue_polls());
        for &increment in plan.increments_before_queue_process() {
            next_clock.ticks = next_clock.ticks.checked_add(increment).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "CE game_time_advance tick arithmetic exceeds signed 32-bit range",
                )
            })?;
            let now = ScriptClockContext {
                clock: next_clock,
                last_visit_ticks: Some(state.map_last_visit_ticks),
            };
            per_poll.push(state.process_due_script_timers_transactional_bounded(
                &mut queued,
                now,
                next_clock.ticks,
                now_millis,
                budget.max_events_per_poll,
                budget.max_critical_steps,
            )?);
        }
        *self = state;
        *queue = queued;
        *clock = next_clock;
        Ok(ExecutedScriptTimeAdvance {
            increments: plan.increments_before_queue_process().to_vec(),
            due_events_per_poll: per_poll,
        })
    }

    /// Find the source-derived CE owner of an installed, unstarted Rust VM.
    /// Returns None when no verified instance/record mapping is available.
    pub fn owner_for_instance(&self, instance_id: u64) -> Option<ScriptOwnerResolution> {
        self.script_owners
            .resolution_for_instance(&self.scripts, instance_id)
    }

    /// Prepare event metadata for one actually installed INT instance.
    /// No event callbacks or script procedures execute here.
    pub fn detached_event_for_instance(&self, instance_id: u64) -> Option<DetachedScriptEvent> {
        self.owner_for_instance(instance_id)
            .map(DetachedScriptEvent::new)
    }

    /// Verify the source-derived local executeProc/setupCall frame of one
    /// already-selected MAP action. It does not create a live CE call frame,
    /// move a VM program counter, dispatch callbacks or invoke a procedure.
    pub fn local_frame_for_planned(
        &self,
        planned: &PlannedMapAction,
    ) -> io::Result<PlannedLocalActionFrame> {
        if planned.category == 1
            && planned.action == 2
            && self.spatial_sources.contains_key(&planned.instance_id)
        {
            // A real CE spatial script may have a runtime-synthesized
            // self owner which this subset cannot invent. Authorize its
            // *separate* scr_set_objs source/NULL target only when the
            // spatial script record and actual source actor key are
            // unambiguous. The self_obj native still fails closed.
            let source = self.spatial_sources[&planned.instance_id];
            if self.object_queries.handle_for(source).is_none() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "CE spatial action has an unbound MAP source object",
                ));
            }
            let mut entries = self
                .scripts
                .entries
                .iter()
                .filter(|entry| entry.instance_id == Some(planned.instance_id));
            let entry = entries.next().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "CE planned spatial MAP instance missing",
                )
            })?;
            if entries.next().is_some()
                || entry.binding.script_id != planned.script_id
                || entry.binding.category != planned.category
                || entry.binding.ordinal != planned.ordinal
                || self.owner_for_instance(planned.instance_id).is_none()
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "CE planned spatial record identity or owner changed",
                ));
            }
        } else {
            let _ = self.detached_event_for_planned(planned)?;
        }
        let frame = self
            .scripts
            .runtime
            .inspect_local_action_frame(planned.instance_id, planned.action)?
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "CE planned local action procedure absent",
                )
            })?;
        if frame.procedure_index != planned.procedure_index {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "CE planned local action frame does not match procedure",
            ));
        }
        Ok(frame)
    }

    /// Run only the supported local core opcodes on a CLONED installed VM,
    /// after the MAP plan and original CE local frame have been checked.
    /// A no-value local return is None; unsupported original opcodes error.
    /// This diagnostic does not enter the game event loop, mutate script
    /// locals/quest state or use the native/engine callback host.
    pub fn preview_planned_local_core_bounded(
        &self,
        planned: &PlannedMapAction,
        max_steps: usize,
    ) -> io::Result<Option<ScriptValue>> {
        let frame = self.local_frame_for_planned(planned)?;
        let preview = self
            .scripts
            .runtime
            .preview_local_action_core_bounded(planned.instance_id, planned.action, max_steps)?
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "CE local action vanished between preflight and preview",
                )
            })?;
        if preview.frame != frame {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "CE local action frame changed during read-only preview",
            ));
        }
        Ok(preview.returned)
    }

    /// Expose the complete discarded post-action local VM frame after
    /// validating the original MAP lifecycle plan and CE frame binding.
    /// Core opcodes only; no native reads, gameplay callbacks, commits, or
    /// persistent script/MAP/quest state changes are possible.
    pub fn preview_planned_local_frame_core_bounded(
        &self,
        planned: &PlannedMapAction,
        max_steps: usize,
    ) -> io::Result<PreviewedBoundLocalActionVmFrame> {
        let inspected = self.local_frame_for_planned(planned)?;
        let result = self
            .scripts
            .runtime
            .preview_local_action_frame_core_bounded(
                planned.instance_id,
                planned.action,
                max_steps,
            )?
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "CE local action missing after source-plan preflight",
                )
            })?;
        if result.binding != inspected || result.candidate.program_counter != 20 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "CE candidate action frame does not match verified MAP binding",
            ));
        }
        Ok(result)
    }

    /// Run a validated local MAP script action on CLONED VM, MAP-variable,
    /// GAME_GLOBAL_VARS and invocation state. Only source-derived variable
    /// reads/writes, script_overrides, and pure object/clock/event queries
    /// can run. Every other native/gameplay opcode fails closed. Returned
    /// variable stores are speculative; caller must not confuse them with
    /// committed original CE state or actual quest effects.
    pub fn preview_planned_sandbox_bounded(
        &self,
        planned: &PlannedMapAction,
        clock: ScriptClockContext,
        max_steps: usize,
    ) -> io::Result<PreviewedSandboxAction> {
        let event = self.detached_event_for_planned(planned)?;
        self.preview_event_sandbox_inner(planned, &event, clock, max_steps)
    }

    /// Run the speculative CE action against a previously established
    /// detached script event. The event must match the same verified MAP
    /// plan and remain active; speculative overrides, globals and locals
    /// are discarded separately from the real event session.
    pub fn preview_active_sandbox_bounded(
        &self,
        sessions: &DetachedMapEventSessions,
        planned: &PlannedMapAction,
        clock: ScriptClockContext,
        max_steps: usize,
    ) -> io::Result<PreviewedSandboxAction> {
        let _ = self.local_frame_for_planned(planned)?;
        let event = sessions.get(planned.instance_id).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "CE sandbox active event session missing",
            )
        })?;
        let action = i32::try_from(planned.action).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidData, "CE sandbox action exceeds i32")
        })?;
        if !event.is_active()
            || event.invocation().action() != action
            || Some(event.owner_resolution()) != self.owner_for_instance(planned.instance_id)
            || planned
                .external_parameter
                .is_some_and(|fixed| event.invocation().fixed_parameter() != fixed)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "CE active sandbox event does not match verified plan",
            ));
        }
        self.preview_event_sandbox_inner(planned, event, clock, max_steps)
    }

    fn preview_event_sandbox_inner(
        &self,
        planned: &PlannedMapAction,
        event: &DetachedScriptEvent,
        clock: ScriptClockContext,
        max_steps: usize,
    ) -> io::Result<PreviewedSandboxAction> {
        let frame = self.local_frame_for_planned(planned)?;
        let snapshot = event.snapshot(&self.object_queries, clock)?;
        let mut map_locals = self.map_locals.clone();
        let mut game_globals = self.game_globals.clone();
        let baseline_map = map_locals.clone();
        let baseline_game_globals = game_globals.clone();
        let baseline_procedures = self
            .scripts
            .runtime
            .vm(planned.instance_id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "CE sandbox VM missing"))?
            .runtime_procedures()
            .to_vec();
        let mut invocation = *event.invocation();
        let preview = self
            .scripts
            .runtime
            .preview_local_action_frame_with_host_bounded(
                planned.instance_id,
                planned.action,
                max_steps,
                &mut |opcode, stack| match opcode {
                    LOCAL_VAR_OPCODE | SET_LOCAL_VAR_OPCODE | MAP_VAR_OPCODE
                    | SET_MAP_VAR_OPCODE => apply_map_variable_opcode(
                        &mut map_locals,
                        planned.instance_id,
                        opcode,
                        stack,
                    ),
                    crate::game_globals::GLOBAL_VAR_OPCODE
                    | crate::game_globals::SET_GLOBAL_VAR_OPCODE => game_globals
                        .as_mut()
                        .ok_or_else(|| {
                            io::Error::new(
                                io::ErrorKind::InvalidData,
                                "CE sandbox game globals absent",
                            )
                        })?
                        .apply_opcode(opcode, stack),
                    SCRIPT_OVERRIDES_OPCODE => {
                        invocation.mark_overrides();
                        Ok(())
                    }
                    _ => apply_readonly_event_native_opcode(snapshot, &invocation, opcode, stack),
                },
            )?
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "CE sandbox local action missing after preflight",
                )
            })?;
        if frame != preview.binding {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "CE sandbox action frame changed during preview",
            ));
        }
        Ok(PreviewedSandboxAction {
            returned: preview.candidate.returned,
            vm_frame: preview,
            map_locals,
            game_globals,
            overrides: invocation.overrides(),
            baseline_map,
            baseline_game_globals,
            baseline_procedures,
        })
    }

    /// Read a source-recorded CE conditional procedure and retain the
    /// speculative post-evaluation interpreter frame for inspection.
    /// Neither the installed VM nor MAP/quest globals can be committed by
    /// this API; candidate scheduled-procedure changes are diagnostic only.
    pub fn preview_planned_condition_frame_reads_bounded(
        &self,
        planned: &PlannedMapAction,
        procedure_index: usize,
        clock: ScriptClockContext,
        max_steps: usize,
    ) -> io::Result<PreviewedConditionFrame> {
        let event = self.detached_event_for_planned(planned)?;
        let vm = self
            .scripts
            .runtime
            .vm(planned.instance_id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "CE condition VM missing"))?;
        let procedure = vm
            .runtime_procedures()
            .get(procedure_index)
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "CE condition procedure missing")
            })?;
        if procedure.flags & 0x02 == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "CE procedure has no pending conditional event",
            ));
        }
        let snapshot = event.snapshot(&self.object_queries, clock)?;
        vm.preview_condition_frame_with_host_bounded(
            procedure.condition_offset,
            max_steps,
            &mut |opcode, stack| {
                apply_readonly_event_native_with_vars(
                    snapshot,
                    event.invocation(),
                    &self.map_locals,
                    self.game_globals.as_ref(),
                    planned.instance_id,
                    opcode,
                    stack,
                )
            },
        )
    }

    /// Inspect the truth of one verified CE condition without retaining
    /// speculative frame effects. This never consumes a scheduled event.
    pub fn preview_planned_condition_reads_bounded(
        &self,
        planned: &PlannedMapAction,
        procedure_index: usize,
        clock: ScriptClockContext,
        max_steps: usize,
    ) -> io::Result<bool> {
        self.preview_planned_condition_frame_reads_bounded(
            planned,
            procedure_index,
            clock,
            max_steps,
        )
        .map(|frame| frame.satisfied)
    }

    /// Inspect which scheduled CE script procedures are ready under current
    /// immutable MAP/quest/clock inputs. Conditional events take priority
    /// over timed events, and CE's strict time comparison is preserved.
    /// The installed VM, scheduler flags and world state never change.
    pub fn preview_planned_due_frame_reads_bounded(
        &self,
        planned: &PlannedMapAction,
        clock: ScriptClockContext,
        now_millis: u32,
        max_steps_per_condition: usize,
    ) -> io::Result<PreviewedDueProcedurePoll> {
        let event = self.detached_event_for_planned(planned)?;
        let vm = self
            .scripts
            .runtime
            .vm(planned.instance_id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "CE due VM missing"))?;
        let snapshot = event.snapshot(&self.object_queries, clock)?;
        vm.preview_due_procedure_poll_with_host_bounded(
            now_millis,
            max_steps_per_condition,
            &mut |opcode, stack| {
                apply_readonly_event_native_with_vars(
                    snapshot,
                    event.invocation(),
                    &self.map_locals,
                    self.game_globals.as_ref(),
                    planned.instance_id,
                    opcode,
                    stack,
                )
            },
        )
    }

    /// Convenience ready-index-only projection of the detached sequential
    /// MAP poll. The registered CE scheduler remains unchanged.
    pub fn preview_planned_due_reads_bounded(
        &self,
        planned: &PlannedMapAction,
        clock: ScriptClockContext,
        now_millis: u32,
        max_steps_per_condition: usize,
    ) -> io::Result<Vec<usize>> {
        self.preview_planned_due_frame_reads_bounded(
            planned,
            clock,
            now_millis,
            max_steps_per_condition,
        )
        .map(|candidate| candidate.ready)
    }

    /// Inspect ready CE procedures against an already-active detached MAP
    /// event with retained source/target/fixedParam context. Exact planned
    /// action identity and owner are verified before any condition read.
    /// This remains a read-only inspection, not CE event dispatch.
    pub fn preview_active_due_frame_reads_bounded(
        &self,
        sessions: &DetachedMapEventSessions,
        planned: &PlannedMapAction,
        clock: ScriptClockContext,
        now_millis: u32,
        max_steps_per_condition: usize,
    ) -> io::Result<PreviewedDueProcedurePoll> {
        let _ = self.local_frame_for_planned(planned)?;
        let event = sessions.get(planned.instance_id).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "CE active due session missing")
        })?;
        let action = i32::try_from(planned.action).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "CE active due action exceeds i32",
            )
        })?;
        if !event.is_active()
            || event.invocation().action() != action
            || Some(event.owner_resolution()) != self.owner_for_instance(planned.instance_id)
            || planned
                .external_parameter
                .is_some_and(|fixed| event.invocation().fixed_parameter() != fixed)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "CE active due event does not match verified plan",
            ));
        }
        let vm = self
            .scripts
            .runtime
            .vm(planned.instance_id)
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "CE active due VM missing")
            })?;
        let snapshot = event.snapshot(&self.object_queries, clock)?;
        vm.preview_due_procedure_poll_with_host_bounded(
            now_millis,
            max_steps_per_condition,
            &mut |opcode, stack| {
                apply_readonly_event_native_with_vars(
                    snapshot,
                    event.invocation(),
                    &self.map_locals,
                    self.game_globals.as_ref(),
                    planned.instance_id,
                    opcode,
                    stack,
                )
            },
        )
    }

    /// Project ready indices from an active, verified detached event poll.
    pub fn preview_active_due_reads_bounded(
        &self,
        sessions: &DetachedMapEventSessions,
        planned: &PlannedMapAction,
        clock: ScriptClockContext,
        now_millis: u32,
        max_steps_per_condition: usize,
    ) -> io::Result<Vec<usize>> {
        self.preview_active_due_frame_reads_bounded(
            sessions,
            planned,
            clock,
            now_millis,
            max_steps_per_condition,
        )
        .map(|candidate| candidate.ready)
    }

    /// Preview only supported CE object, event and clock READ opcodes
    /// inside the local procedure, using a cloned installed VM and a pure
    /// native adapter. No global/local state writes, overrides, timers,
    /// gameplay callbacks, or live CE event dispatch can occur.
    pub fn preview_planned_event_reads_bounded(
        &self,
        planned: &PlannedMapAction,
        clock: ScriptClockContext,
        max_steps: usize,
    ) -> io::Result<Option<ScriptValue>> {
        let frame = self.local_frame_for_planned(planned)?;
        let event = self.detached_event_for_planned(planned)?;
        let snapshot = event.snapshot(&self.object_queries, clock)?;
        let preview = self
            .scripts
            .runtime
            .preview_local_action_with_host_bounded(
                planned.instance_id,
                planned.action,
                max_steps,
                &mut |opcode, stack| {
                    apply_readonly_event_native_with_vars(
                        snapshot,
                        event.invocation(),
                        &self.map_locals,
                        self.game_globals.as_ref(),
                        planned.instance_id,
                        opcode,
                        stack,
                    )
                },
            )?
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "CE read-only local action missing after preflight",
                )
            })?;
        if preview.frame != frame {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "CE read-only local action frame changed during preview",
            ));
        }
        Ok(preview.returned)
    }

    /// Preview an already-active, explicitly begun MAP script event
    /// against a CLONED installed INT. Unlike the initial-plan-only preview,
    /// this reads the retained per-script source/target/fixedParam context.
    /// Validation and execution are read-only: failure neither consumes the
    /// active event nor updates the registered VM, MAP or game globals.
    pub fn preview_active_event_reads_bounded(
        &self,
        sessions: &DetachedMapEventSessions,
        planned: &PlannedMapAction,
        clock: ScriptClockContext,
        max_steps: usize,
    ) -> io::Result<Option<ScriptValue>> {
        let frame = self.local_frame_for_planned(planned)?;
        let event = sessions.get(planned.instance_id).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "CE active event session missing",
            )
        })?;
        let action = i32::try_from(planned.action).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidData, "CE planned action exceeds i32")
        })?;
        if !event.is_active()
            || event.invocation().action() != action
            || Some(event.owner_resolution()) != self.owner_for_instance(planned.instance_id)
            || planned
                .external_parameter
                .is_some_and(|fixed| event.invocation().fixed_parameter() != fixed)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "CE active event session does not match planned action",
            ));
        }
        let snapshot = event.snapshot(&self.object_queries, clock)?;
        let preview = self
            .scripts
            .runtime
            .preview_local_action_with_host_bounded(
                planned.instance_id,
                planned.action,
                max_steps,
                &mut |opcode, stack| {
                    apply_readonly_event_native_with_vars(
                        snapshot,
                        event.invocation(),
                        &self.map_locals,
                        self.game_globals.as_ref(),
                        planned.instance_id,
                        opcode,
                        stack,
                    )
                },
            )?
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "CE active event local action missing after preflight",
                )
            })?;
        if preview.frame != frame {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "CE active event local frame changed during preview",
            ));
        }
        Ok(preview.returned)
    }

    /// Compose an already-planned CE MAP lifecycle action with its exact
    /// installed Rust VM record and source-derived owner. A stale or altered
    /// plan is rejected before allocating detached action metadata.
    /// No INT bytecode or native game callback runs.
    pub fn detached_event_for_planned(
        &self,
        planned: &PlannedMapAction,
    ) -> io::Result<DetachedScriptEvent> {
        let mut candidates = self
            .scripts
            .entries
            .iter()
            .filter(|entry| entry.instance_id == Some(planned.instance_id));
        let entry = candidates.next().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "CE planned script instance missing",
            )
        })?;
        if candidates.next().is_some()
            || entry.binding.script_id != planned.script_id
            || entry.binding.category != planned.category
            || entry.binding.ordinal != planned.ordinal
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "CE planned script record identity mismatch",
            ));
        }
        let procedure = self
            .scripts
            .runtime
            .resolve_action(planned.instance_id, planned.action)?;
        if procedure.is_none_or(|procedure| procedure.procedure_index != planned.procedure_index) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "CE planned script procedure mismatch",
            ));
        }
        let owner = self
            .owner_for_instance(planned.instance_id)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "CE planned script owner unresolved",
                )
            })?;
        let mut event = DetachedScriptEvent::new(owner);
        event.begin_action(
            planned.action,
            planned.external_parameter,
            &self.object_queries,
        )?;
        Ok(event)
    }
}

/// Opt-in detached CE script-event state keyed by installed Rust VM ID.
/// This is separate from MAP loading and the script scheduler. It never
/// executes an INT or invokes original game callbacks.
#[derive(Debug, Clone, Default)]
pub struct DetachedMapEventSessions {
    sessions: BTreeMap<u64, DetachedScriptEvent>,
}

impl DetachedMapEventSessions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, instance_id: u64) -> Option<&DetachedScriptEvent> {
        self.sessions.get(&instance_id)
    }

    /// Mimic an explicit scr_set_objs call for an installed script; absent
    /// participants are true NULL, and unsafe ObjectKeys are rejected.
    pub fn set_objs(
        &mut self,
        environment: &PreparedScriptEnvironment,
        instance_id: u64,
        source: Option<ObjectKey>,
        target: Option<ObjectKey>,
    ) -> io::Result<()> {
        let owner = environment.owner_for_instance(instance_id).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "CE script event instance unresolved",
            )
        })?;
        // Stage changes before inserting a new session or mutating an
        // existing one. Invalid ObjectKeys must not leave empty entries.
        let mut candidate = self
            .sessions
            .get(&instance_id)
            .copied()
            .unwrap_or_else(|| DetachedScriptEvent::new(owner));
        if candidate.owner_resolution() != owner {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "CE script event session owner changed",
            ));
        }
        candidate.set_objs(&environment.object_queries, source, target)?;
        self.sessions.insert(instance_id, candidate);
        Ok(())
    }

    /// Revalidate the exact planned VM/SID/procedure before accessing any
    /// per-script state. Existing target and fixed parameter persist.
    /// An unsuccessful entry leaves the previous event metadata untouched.
    pub fn begin_planned<'a>(
        &'a mut self,
        environment: &PreparedScriptEnvironment,
        planned: &PlannedMapAction,
    ) -> io::Result<&'a mut DetachedScriptEvent> {
        // A planning record alone is insufficient to authorize a detached
        // action: inspect its actual local CE executeProc body as well.
        let _ = environment.local_frame_for_planned(planned)?;
        let owner = environment
            .owner_for_instance(planned.instance_id)
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "CE script event owner missing")
            })?;
        let mut candidate = self
            .sessions
            .get(&planned.instance_id)
            .copied()
            .unwrap_or_else(|| DetachedScriptEvent::new(owner));
        if candidate.owner_resolution() != owner {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "CE script event session owner changed",
            ));
        }
        candidate.begin_action(
            planned.action,
            planned.external_parameter,
            &environment.object_queries,
        )?;
        self.sessions.insert(planned.instance_id, candidate);
        self.sessions.get_mut(&planned.instance_id).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "CE script event session missing",
            )
        })
    }

    /// Explicit normal-completion boundary. The source is cleared and the
    /// target retained, just as in pinned CE exec_script_proc.
    pub fn finish(&mut self, instance_id: u64) -> io::Result<()> {
        self.sessions
            .get_mut(&instance_id)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "CE script event session missing",
                )
            })?
            .finish_action()
    }
}

impl StaticScene {
    /// Open an original MAP in the user's master game-data bank.
    pub fn load(assets: &mut GameAssets, path: &str) -> io::Result<Option<Self>> {
        match assets.read(Bank::Master, path)? {
            Some(bytes) => Self::decode(assets, &bytes).map(Some),
            None => Ok(None),
        }
    }

    /// Construct a static scene without executing any script.
    pub fn decode(assets: &mut GameAssets, bytes: &[u8]) -> io::Result<Self> {
        let header = MapHeader::decode(bytes)?;
        let variables = header.decode_variables(bytes)?;
        let squares = header.decode_squares(bytes)?;
        let (scripts, objects, bytes_consumed) = squares.decode_objects_with(bytes, |pid| {
            let proto = assets.prototype_by_pid(pid)?.ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("missing original prototype {pid:#010X}"),
                )
            })?;
            Ok(match proto.body {
                PrototypeBody::Item { item_type, .. } => Some(item_type),
                PrototypeBody::Scenery { scenery_type, .. } => Some(scenery_type),
                _ => None,
            })
        })?;
        let collision = CollisionIndex::from_map(&objects);
        Ok(Self {
            header,
            variables,
            squares,
            scripts,
            objects,
            collision,
            bytes_consumed,
        })
    }

    /// Construct immutable native object reads from top-level MAP objects.
    /// Serialized object IDs may repeat; handles use ObjectKey identity.
    pub fn object_query_snapshot(&self) -> io::Result<MapObjectReadBindings> {
        MapObjectReadBindings::from_tables(&self.objects)
    }

    /// Read-only association between each serialized MAP script and its
    /// installed .INT path/flags. Does not execute script bytecode.
    pub fn script_bindings(&self, assets: &mut GameAssets) -> io::Result<Vec<ScriptBinding>> {
        bind_scripts(assets, &self.scripts)
    }

    /// Resolve installed INT procedure actions for a MAP without running
    /// the VM or invoking engine callbacks. Missing assets stay visible.
    pub fn script_procedure_bindings(
        &self,
        assets: &mut GameAssets,
    ) -> io::Result<Vec<ScriptProcedureBinding>> {
        bind_script_procedures(assets, &self.scripts)
    }

    /// Decode installed .INT programs into independent, unstarted Rust VMs.
    /// This is a read-only preparation path, not map script lifecycle entry.
    pub fn unstarted_script_runtime(
        &self,
        assets: &mut GameAssets,
    ) -> io::Result<BoundScriptRuntime> {
        install_map_scripts(assets, &self.scripts)
    }

    /// Prepare the decoded MAP's attached Rust script instances together
    /// with independent mutable MAP-local storage. This does not execute
    /// START, MAP_ENTER, game native opcodes or any background event.
    pub fn unstarted_script_state(
        &self,
        assets: &mut GameAssets,
    ) -> io::Result<(BoundScriptRuntime, MapScriptLocals)> {
        let scripts = self.unstarted_script_runtime(assets)?;
        let locals = scripts.script_local_state(&self.variables)?;
        Ok((scripts, locals))
    }

    /// Prepare installed MAP scripts, their mutable MAP slots and separate
    /// game-wide quest defaults as owned Rust state. This is a read/decode
    /// boundary only: no procedure, native opcode, timer or world update
    /// is executed by this function.
    pub fn unstarted_script_environment(
        &self,
        assets: &mut GameAssets,
    ) -> io::Result<PreparedScriptEnvironment> {
        let (scripts, map_locals) = self.unstarted_script_state(assets)?;
        let game_globals = assets.game_global_defaults()?;
        let object_queries = self.object_query_snapshot()?;
        let script_owners =
            MapScriptOwners::from_map(&self.scripts, &self.objects, &object_queries)?;
        Ok(PreparedScriptEnvironment {
            scripts,
            map_locals,
            game_globals,
            object_queries,
            script_owners,
            map_last_visit_ticks: self.header.last_visit_time,
            invocations: BTreeMap::new(),
            spatial_sources: BTreeMap::new(),
        })
    }

    /// CE-style numeric route through the initial static object collision.
    /// The caller supplies a viewport and the actor's current identity.
    /// Door scripts and moving blockers are not handled here.
    pub fn route(
        &self,
        view: &HexViewport,
        from: i32,
        to: i32,
        elevation: usize,
        excluded_actor: Option<ObjectKey>,
        not_in_combat: bool,
    ) -> Option<Vec<u8>> {
        make_path(view, from, to, not_in_combat, true, |tile| {
            self.collision
                .is_passable(view, tile, elevation, excluded_actor)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::StaticScene;
    use crate::assets::GameAssets;
    use crate::hex::HexViewport;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn composes_all_empty_map_sections_and_routes_without_runtime() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("rust-age-scene-{}-{stamp}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("master.dat"), [0_u8; 16]).unwrap();
        fs::write(root.join("critter.dat"), [0_u8; 16]).unwrap();
        fs::create_dir_all(root.join("data")).unwrap();
        fs::write(
            root.join("data").join("VAULT13.GAM"),
            b"GAME_GLOBAL_VARS:\nA=7\nB=-13\n",
        )
        .unwrap();
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&19_i32.to_be_bytes());
        bytes.extend_from_slice(b"TEST.MAP\0\0\0\0\0\0\0\0");
        for value in [20099_i32, 0, 0, 1, -1, 2 | 4 | 8, 0, 2, 0, 0] {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        bytes.extend_from_slice(&[0_u8; 44 * 4]);
        for value in [12_i32, -8, 99] {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        bytes.extend_from_slice(&[0_u8; 20 + 16]);
        let mut assets = GameAssets::open(&root).unwrap();
        let scene = StaticScene::decode(&mut assets, &bytes).unwrap();
        assert_eq!(scene.variables.globals, [12, -8]);
        assert_eq!(scene.variables.locals, [99]);
        assert_eq!(scene.scripts.script_count(), 0);
        assert_eq!(scene.objects.total_objects, 0);
        assert_eq!(scene.bytes_consumed, bytes.len());
        let (prepared, locals) = scene.unstarted_script_state(&mut assets).unwrap();
        assert!(prepared.runtime.is_empty());
        assert!(prepared.entries.is_empty());
        assert_eq!(locals.globals(), [12, -8]);
        assert_eq!(locals.locals(), [99]);
        assert_eq!(scene.variables.locals, [99]); // no mutation of MAP
        let environment = scene.unstarted_script_environment(&mut assets).unwrap();
        assert!(environment.scripts.runtime.is_empty());
        assert!(environment.object_queries.snapshots().is_empty());
        assert!(environment.script_owners.entries().is_empty());
        assert_eq!(environment.owner_for_instance(1), None);
        assert_eq!(environment.map_locals.globals(), [12, -8]);
        assert_eq!(environment.map_locals.locals(), [99]);
        assert_eq!(
            environment.map_last_visit_ticks,
            scene.header.last_visit_time
        );
        assert_eq!(environment.game_globals.unwrap().values(), [7, -13]);
        assert_eq!(scene.variables.globals, [12, -8]); // game globals stay separate.

        // An installed MAP-owned Rust VM now reaches the transactional
        // normal CE interpreter tick through PreparedScriptEnvironment.
        // The synthetic INT writes a source-backed GAME_GLOBAL_VARS slot,
        // while the decoded MAP source and its own variables stay intact.
        let mut active = scene.unstarted_script_environment(&mut assets).unwrap();
        let mut int_bytes = vec![0_u8; 54];
        int_bytes[0..2].copy_from_slice(&0xC001_u16.to_be_bytes());
        int_bytes[2..6].copy_from_slice(&0_i32.to_be_bytes());
        int_bytes[6..8].copy_from_slice(&0xC001_u16.to_be_bytes());
        int_bytes[8..12].copy_from_slice(&99_i32.to_be_bytes());
        int_bytes[12..14].copy_from_slice(&0x80C6_u16.to_be_bytes());
        int_bytes[14..16].copy_from_slice(&0x8011_u16.to_be_bytes());
        let vm = crate::script_vm::ScriptVm::new(
            crate::script_program::ScriptProgram::decode(&int_bytes).unwrap(),
        );
        active.scripts.runtime.register(1, vm).unwrap();
        let binding = crate::script_binding::ScriptBinding {
            category: 0,
            ordinal: 0,
            script_id: 0,
            script_index: 0,
            owner_id: -1,
            path: None,
            run_info: None,
            local_variable_offset: -1,
            local_variable_count: 0,
        };
        active
            .scripts
            .entries
            .push(crate::script_binding::InstalledScriptBinding {
                binding,
                instance_id: Some(1),
            });
        active.map_locals = active.scripts.script_local_state(&scene.variables).unwrap();
        let mut map_scripts = crate::script_data::ScriptTables {
            categories: std::array::from_fn(|_| Vec::new()),
            bytes_consumed: 0,
        };
        map_scripts.categories[0].push(crate::script_data::ScriptRecord {
            script_id: 0,
            next: -1,
            subtype: vec![],
            flags: 0,
            script_index: 0,
            program_token: 0,
            owner_id: -1,
            local_variable_offset: -1,
            local_variable_count: 0,
            remaining: [0; 8],
        });
        active.script_owners = crate::script_owner_binding::MapScriptOwners::from_map(
            &map_scripts,
            &scene.objects,
            &active.object_queries,
        )
        .unwrap();
        assert_eq!(
            active.owner_for_instance(1),
            Some(crate::script_owner_binding::ScriptOwnerResolution::NullOwner)
        );
        let tick = active
            .advance_script_state_tick_bounded(
                crate::script_native_vars::ScriptClockContext {
                    clock: crate::script_clock::ScriptGameClock { ticks: 0 },
                    last_visit_ticks: Some(scene.header.last_visit_time),
                },
                100,
                4,
                0,
            )
            .unwrap();
        assert_eq!(tick.executed_steps, [(1, 4)]);
        assert!(tick.prepared_calls.is_empty());
        assert_eq!(active.game_globals.as_ref().unwrap().values(), [99, -13]);
        assert_eq!(active.map_locals.globals(), [12, -8]);
        assert_eq!(scene.variables.globals, [12, -8]);

        // Pinned executeProc prepares a NONCRITICAL START procedure; the
        // next Rust CE burst interprets its own SCRIPT_ACTION native using
        // the installed action value, not an invented default action zero.
        let mut start_bytes = vec![0_u8; 42];
        start_bytes[20..22].copy_from_slice(&0x801A_u16.to_be_bytes()); // POP arg
        start_bytes[22..24].copy_from_slice(&0xC001_u16.to_be_bytes());
        start_bytes[24..28].copy_from_slice(&1_i32.to_be_bytes());
        start_bytes[28..30].copy_from_slice(&0x80C7_u16.to_be_bytes()); // SCRIPT_ACTION
        start_bytes[30..32].copy_from_slice(&0x80C6_u16.to_be_bytes()); // SET_GAME_GLOBAL
        start_bytes[32..34].copy_from_slice(&0x8021_u16.to_be_bytes()); // POP_FLAGS_EXIT
        start_bytes.extend_from_slice(&2_i32.to_be_bytes());
        for fields in [[4_i32, 0, 0, 0, 0, 0], [9_i32, 0, 0, 0, 20, 0]] {
            for field in fields {
                start_bytes.extend_from_slice(&field.to_be_bytes());
            }
        }
        start_bytes.extend_from_slice(&11_i32.to_be_bytes());
        start_bytes.extend_from_slice(b"main\0start\0");
        start_bytes.extend_from_slice(&0_i32.to_be_bytes());
        let start_vm = crate::script_vm::ScriptVm::new(
            crate::script_program::ScriptProgram::decode(&start_bytes).unwrap(),
        );
        active.scripts.runtime.register(2, start_vm).unwrap();
        active
            .scripts
            .entries
            .push(crate::script_binding::InstalledScriptBinding {
                binding: crate::script_binding::ScriptBinding {
                    category: 0,
                    ordinal: 1,
                    script_id: 1,
                    script_index: 1,
                    owner_id: -1,
                    path: None,
                    run_info: None,
                    local_variable_offset: -1,
                    local_variable_count: 0,
                },
                instance_id: Some(2),
            });
        map_scripts.categories[0].push(crate::script_data::ScriptRecord {
            script_id: 1,
            next: -1,
            subtype: vec![],
            flags: 0,
            script_index: 1,
            program_token: 0,
            owner_id: -1,
            local_variable_offset: -1,
            local_variable_count: 0,
            remaining: [0; 8],
        });
        active.map_locals = active.scripts.script_local_state(&scene.variables).unwrap();
        active.script_owners = crate::script_owner_binding::MapScriptOwners::from_map(
            &map_scripts,
            &scene.objects,
            &active.object_queries,
        )
        .unwrap();
        let plans = active
            .scripts
            .plan_map_lifecycle(crate::script_lifecycle::MapLifecyclePhase::Start, -1, 0)
            .unwrap();
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].instance_id, 2);
        assert_eq!(plans[0].action, 1);
        let frame = active
            .prepare_noncritical_map_action_transactional(&plans[0])
            .unwrap();
        assert_eq!(frame.body_offset, 20);
        assert_eq!(active.scripts.runtime.vm(2).unwrap().pc(), 20);
        assert_eq!(active.invocations.get(&2).unwrap().action(), 1);
        assert_eq!(active.game_globals.as_ref().unwrap().values(), [99, -13]);
        let action_tick = active
            .advance_script_state_tick_bounded(
                crate::script_native_vars::ScriptClockContext {
                    clock: crate::script_clock::ScriptGameClock { ticks: 0 },
                    last_visit_ticks: Some(scene.header.last_visit_time),
                },
                101,
                5,
                0,
            )
            .unwrap();
        assert_eq!(action_tick.executed_steps, [(2, 5), (1, 0)]);
        assert_eq!(active.game_globals.as_ref().unwrap().values(), [99, 1]);
        assert_eq!(active.map_locals.globals(), [12, -8]);
        assert_eq!(scene.variables.globals, [12, -8]);

        fs::remove_file(root.join("data").join("VAULT13.GAM")).unwrap();
        assert!(scene
            .unstarted_script_environment(&mut assets)
            .unwrap()
            .game_globals
            .is_none());
        let view = HexViewport::new(200, 200, 640, 380, 20099).unwrap();
        assert_eq!(
            scene.route(&view, 20099, 20098, 0, None, true),
            Some(vec![1])
        );
        fs::remove_dir_all(root).unwrap();
    }
}
