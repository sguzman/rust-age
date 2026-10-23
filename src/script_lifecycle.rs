//! Read-only selection/order for CE MAP scripts at lifecycle boundaries.
//! Pinned reference: src/game/scripts.cc scr_load_all_scripts,
//! scr_exec_map_enter_scripts, scr_exec_map_update_scripts, scr_exec_map_exit_scripts.
use crate::script_binding::{BoundScriptRuntime, InstalledScriptBinding};
use std::io;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapLifecyclePhase {
    Start,
    Enter,
    Update,
    Exit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlannedMapAction {
    /// This is a Rust MAP-record ID, not an original CE SID.
    pub instance_id: u64,
    pub script_id: i32,
    pub category: usize,
    pub ordinal: usize,
    pub action: usize,
    pub procedure_index: usize,
    /// CE map-enter sets fixedParam to the inverse of map_data.flags bit 0.
    pub external_parameter: Option<i32>,
}

impl BoundScriptRuntime {
    fn plan_one(
        &self,
        entry: &InstalledScriptBinding,
        action: usize,
        require_nonzero: bool,
        external_parameter: Option<i32>,
    ) -> io::Result<Option<PlannedMapAction>> {
        let Some(id) = entry.instance_id else {
            return Ok(None);
        };
        let Some(proc) = self.runtime.resolve_action(id, action)? else {
            return Ok(None);
        };
        // Original CE map-enter/update checks procs[action] > 0 on non-map
        // scripts. A procedure at index zero does not satisfy that check.
        if require_nonzero && proc.procedure_index == 0 {
            return Ok(None);
        }
        Ok(Some(PlannedMapAction {
            instance_id: id,
            script_id: entry.binding.script_id,
            category: entry.binding.category,
            ordinal: entry.binding.ordinal,
            action,
            procedure_index: proc.procedure_index,
            external_parameter,
        }))
    }

    /// Select only existing named actions, never execute any procedure.
    /// The map SID must be supplied by the caller. START scans all scripts;
    /// ENTER scans non-map scripts; UPDATE visits map script then others;
    /// EXIT visits only the map script. script.lst map_init/map_exit markers
    /// are not substitutes for these procedure calls.
    ///
    /// This planner does not simulate CE's first-load/start fallback, or
    /// runtime script_engine_running and uninitialized lookup-table state.
    pub fn plan_map_lifecycle(
        &self,
        phase: MapLifecyclePhase,
        map_script_id: i32,
        map_flags: i32,
    ) -> io::Result<Vec<PlannedMapAction>> {
        let slot = match phase {
            MapLifecyclePhase::Start => 1,
            MapLifecyclePhase::Enter => 15,
            MapLifecyclePhase::Update => 23,
            MapLifecyclePhase::Exit => 16,
        };
        let map_entry = self
            .entries
            .iter()
            .find(|entry| entry.binding.script_id == map_script_id);
        let mut actions = Vec::new();
        if phase == MapLifecyclePhase::Update || phase == MapLifecyclePhase::Exit {
            if let Some(entry) = map_entry {
                if let Some(action) = self.plan_one(entry, slot, false, None)? {
                    actions.push(action);
                }
            }
        }
        if phase == MapLifecyclePhase::Exit {
            return Ok(actions);
        }
        for entry in &self.entries {
            if phase != MapLifecyclePhase::Start && entry.binding.script_id == map_script_id {
                continue;
            }
            let strict = phase == MapLifecyclePhase::Enter || phase == MapLifecyclePhase::Update;
            let param = if phase == MapLifecyclePhase::Enter {
                Some(i32::from(map_flags & 1 == 0))
            } else {
                None
            };
            if let Some(action) = self.plan_one(entry, slot, strict, param)? {
                actions.push(action);
            }
        }
        Ok(actions)
    }
}

#[cfg(test)]
mod tests {
    use super::MapLifecyclePhase;
    use crate::script_binding::{BoundScriptRuntime, InstalledScriptBinding, ScriptBinding};
    use crate::script_program::ScriptProgram;
    use crate::script_runtime::ScriptRuntime;
    use crate::script_vm::ScriptVm;

    fn vm(names: &[&[u8]]) -> ScriptVm {
        let mut bytes = vec![0_u8; 42];
        bytes.extend_from_slice(&(names.len() as i32).to_be_bytes());
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

    fn record(
        category: usize,
        ordinal: usize,
        sid: i32,
        id: Option<u64>,
    ) -> InstalledScriptBinding {
        InstalledScriptBinding {
            binding: ScriptBinding {
                category,
                ordinal,
                script_id: sid,
                script_index: 0,
                owner_id: -1,
                path: Some("scripts/SYNTH.int".to_owned()),
                run_info: None,
                local_variable_offset: -1,
                local_variable_count: 0,
            },
            instance_id: id,
        }
    }

    fn fixture() -> BoundScriptRuntime {
        let mut runtime = ScriptRuntime::new();
        runtime
            .register(
                1,
                vm(&[b"main", b"start", b"map_update_p_proc", b"map_exit_p_proc"]),
            )
            .unwrap();
        runtime
            .register(
                2,
                vm(&[b"main", b"start", b"map_enter_p_proc", b"map_update_p_proc"]),
            )
            .unwrap();
        runtime
            .register(4, vm(&[b"main", b"start", b"MAP_ENTER_P_PROC"]))
            .unwrap();
        BoundScriptRuntime {
            runtime,
            entries: vec![
                record(0, 0, 500, Some(1)),
                record(0, 1, 501, Some(2)),
                record(1, 0, 601, None),
                record(1, 1, 602, Some(4)),
            ],
        }
    }

    #[test]
    fn ce_start_enter_update_and_exit_source_order() {
        let bound = fixture();
        let start = bound
            .plan_map_lifecycle(MapLifecyclePhase::Start, 500, 0)
            .unwrap();
        assert_eq!(
            start.iter().map(|a| a.script_id).collect::<Vec<_>>(),
            [500, 501, 602]
        );
        assert!(start.iter().all(|a| a.action == 1));
        let enter = bound
            .plan_map_lifecycle(MapLifecyclePhase::Enter, 500, 0)
            .unwrap();
        assert_eq!(
            enter.iter().map(|a| a.script_id).collect::<Vec<_>>(),
            [501, 602]
        );
        assert_eq!(enter[0].procedure_index, 2);
        assert!(enter
            .iter()
            .all(|a| a.action == 15 && a.external_parameter == Some(1)));
        let revisited = bound
            .plan_map_lifecycle(MapLifecyclePhase::Enter, 500, 1)
            .unwrap();
        assert!(revisited.iter().all(|a| a.external_parameter == Some(0)));
        let update = bound
            .plan_map_lifecycle(MapLifecyclePhase::Update, 500, 0)
            .unwrap();
        assert_eq!(
            update.iter().map(|a| a.script_id).collect::<Vec<_>>(),
            [500, 501]
        );
        assert_eq!(
            update.iter().map(|a| a.procedure_index).collect::<Vec<_>>(),
            [2, 3]
        );
        let exit = bound
            .plan_map_lifecycle(MapLifecyclePhase::Exit, 500, 0)
            .unwrap();
        assert_eq!(exit.len(), 1);
        assert_eq!(exit[0].script_id, 500);
        assert_eq!(exit[0].procedure_index, 3);
        assert!(!bound.runtime.vm(1).unwrap().halted());
        assert!(bound.runtime.vm(1).unwrap().stack().is_empty());
    }

    #[test]
    fn absent_map_or_missing_int_never_invents_actions() {
        let bound = fixture();
        assert!(bound
            .plan_map_lifecycle(MapLifecyclePhase::Exit, 999, 0)
            .unwrap()
            .is_empty());
        assert_eq!(
            bound
                .plan_map_lifecycle(MapLifecyclePhase::Enter, 999, 0)
                .unwrap()
                .len(),
            2
        );
        let update = bound
            .plan_map_lifecycle(MapLifecyclePhase::Update, 501, 0)
            .unwrap();
        assert_eq!(
            update.iter().map(|a| a.script_id).collect::<Vec<_>>(),
            [501, 500]
        );
    }
}
