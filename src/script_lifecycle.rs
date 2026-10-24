//! Read-only selection/order for CE MAP scripts at lifecycle boundaries.
//! Pinned reference: src/game/scripts.cc scr_load_all_scripts,
//! scr_exec_map_enter_scripts, scr_exec_map_update_scripts, scr_exec_map_exit_scripts.
use crate::hex::HexViewport;
use crate::script_binding::{BoundScriptRuntime, InstalledScriptBinding};
use crate::script_data::ScriptTables;
use crate::script_native_vars::ScriptInvocationState;
use crate::script_procedures::SCRIPT_PROC_NAMES;
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

impl PlannedMapAction {
    /// Convert a read-only lifecycle selection into detached invocation
    /// metadata. Original CE exec_script_proc sets script->action and resets
    /// overrides; MAP_ENTER also assigns fixedParam immediately beforehand.
    /// This never enters a procedure or mutates the installed Rust VM.
    pub fn detached_invocation(&self) -> io::Result<ScriptInvocationState> {
        if self.action >= SCRIPT_PROC_NAMES.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "CE lifecycle action outside built-in procedure table",
            ));
        }
        let action = i32::try_from(self.action).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "CE lifecycle action exceeds i32",
            )
        })?;
        let mut state = ScriptInvocationState::installed();
        state.begin_action(action);
        if let Some(parameter) = self.external_parameter {
            state.set_fixed_parameter(parameter);
        }
        Ok(state)
    }
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

    /// Select the original scr_chk_spatials_in spatial_p_proc calls
    /// after the caller has checked that an ordinary, non-flat, visible
    /// actor moved to this tile and spatial dispatch is enabled.
    ///
    /// Pinned scr_find_first_at/scr_find_next_at scan only non-disabled
    /// spatial records at the current elevation. Exact built-tile match
    /// takes precedence; nonzero radius falls back to CE tile_dist <=
    /// radius. The packed tile/elevation bits match object_types.h.
    /// Do not silently synthesize missing map scripts, VM instances,
    /// deferred procedure tables or native object participants.
    ///
    /// This is a source-ordered *selection*, not scr_set_objs or live
    /// exec_script_proc dispatch. The spatial source object's verified
    /// handle and NULL target must be supplied by a real scene adapter.
    pub fn plan_spatial_at(
        &self,
        tables: &ScriptTables,
        view: &HexViewport,
        tile: i32,
        elevation: usize,
    ) -> io::Result<Vec<PlannedMapAction>> {
        if elevation >= 3 || view.tile_distance(tile, tile).is_none() || tile < 10 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "CE spatial event tile or elevation is outside supported MAP bounds",
            ));
        }
        let built_current = (tile as u32) | ((elevation as u32) << 29);
        let mut selected = Vec::new();
        for (ordinal, script) in tables.categories[1].iter().enumerate() {
            if script.flags & 0x02 != 0 {
                continue;
            }
            if script.subtype.len() != 2 || ((script.script_id as u32) >> 24) != 1 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "CE spatial record has invalid category or subtype",
                ));
            }
            let built = script.subtype[0] as u32;
            if built >> 29 != elevation as u32 {
                continue;
            }
            let script_tile = (built & 0x03FF_FFFF) as i32;
            let radius = script.subtype[1];
            let matches = if built == built_current {
                true
            } else if radius > 0 {
                let distance = view.tile_distance(script_tile, tile).ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "CE spatial script radius source tile outside MAP",
                    )
                })?;
                distance <= radius as u32
            } else {
                false
            };
            if !matches {
                continue;
            }
            let mut candidates = self.entries.iter().filter(|entry| {
                entry.binding.category == 1 && entry.binding.ordinal == ordinal
            });
            let entry = candidates.next().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "CE matching spatial script has no installed MAP binding",
                )
            })?;
            if candidates.next().is_some()
                || entry.binding.script_id != script.script_id
                || entry.binding.script_index != script.script_index
                || entry.instance_id.is_none()
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "CE spatial script has missing or ambiguous VM identity",
                ));
            }
            if let Some(planned) = self.plan_one(entry, 2, false, None)? {
                selected.push(planned);
            }
        }
        Ok(selected)
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
        let mut context = enter[0].detached_invocation().unwrap();
        assert_eq!(context.action(), 15);
        assert_eq!(context.fixed_parameter(), 1);
        assert!(!context.overrides());
        assert!(context.begin_action(23));
        assert_eq!(context.fixed_parameter(), 1); // exec_script_proc does not clear it.
        assert!(enter
            .iter()
            .all(|a| a.action == 15 && a.external_parameter == Some(1)));
        let revisited = bound
            .plan_map_lifecycle(MapLifecyclePhase::Enter, 500, 1)
            .unwrap();
        assert!(revisited.iter().all(|a| a.external_parameter == Some(0)));
        assert_eq!(
            revisited[0]
                .detached_invocation()
                .unwrap()
                .fixed_parameter(),
            0
        );
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
