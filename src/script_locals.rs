//! Bounded CE MAP script-local value storage, without engine callbacks.
//! Pinned reference: game/scripts.cc scr_get_local_var/scr_set_local_var
//! and game/map.cc map_malloc_local_var/map_get_local_var/map_set_local_var.
//! Pointer values are opaque i32 handles, not dereferenced native pointers.
//! Game native opcodes and pointer persistence are not yet connected.
use crate::map::MapVariables;
use crate::script_binding::BoundScriptRuntime;
use crate::script_vm::ScriptValue;
use std::collections::BTreeMap;
use std::io;

const MAX_LOCAL_SLOTS: usize = 1_000_000;

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptLocalRange {
    pub declared: usize,
    pub offset: Option<usize>,
    pub is_system: bool,
}

/// Mutable Rust-owned copy of MAP locals; original decoded MAP is unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapScriptLocals {
    globals: Vec<i32>,
    global_pointers: Vec<Option<i32>>,
    locals: Vec<i32>,
    pointers: Vec<Option<i32>>,
    instances: BTreeMap<u64, ScriptLocalRange>,
}

impl BoundScriptRuntime {
    pub fn script_local_state(&self, map: &MapVariables) -> io::Result<MapScriptLocals> {
        if map.globals.len() > MAX_LOCAL_SLOTS || map.locals.len() > MAX_LOCAL_SLOTS {
            return Err(invalid("CE MAP local variable array exceeds Rust bound"));
        }
        let mut instances = BTreeMap::new();
        for entry in &self.entries {
            let Some(id) = entry.instance_id else {
                continue;
            };
            if self.runtime.vm(id).is_none() {
                return Err(invalid("CE MAP local state references missing VM"));
            }
            let binding = &entry.binding;
            // CE consults scripts.lst when the script record has no local
            // declaration yet. The initial MAP record takes precedence.
            let declared = if binding.local_variable_count != 0 {
                binding.local_variable_count
            } else {
                binding
                    .run_info
                    .and_then(|info| info.local_variables)
                    .unwrap_or(0)
            };
            let declared = usize::try_from(declared)
                .map_err(|_| invalid("negative CE script local variable count"))?;
            if declared > MAX_LOCAL_SLOTS {
                return Err(invalid("CE script local count exceeds Rust bound"));
            }
            let offset = if binding.local_variable_offset >= 0 && declared > 0 {
                let offset = usize::try_from(binding.local_variable_offset)
                    .map_err(|_| invalid("negative CE script local offset"))?;
                let end = offset
                    .checked_add(declared)
                    .ok_or_else(|| invalid("CE local variable range overflow"))?;
                if end > map.locals.len() {
                    return Err(invalid("CE script local variable range outside saved MAP"));
                }
                Some(offset)
            } else {
                None
            };
            if instances
                .insert(
                    id,
                    ScriptLocalRange {
                        declared,
                        offset,
                        is_system: binding.category == 0,
                    },
                )
                .is_some()
            {
                return Err(invalid("duplicate CE Rust script instance local state"));
            }
        }
        Ok(MapScriptLocals {
            globals: map.globals.clone(),
            global_pointers: vec![None; map.globals.len()],
            locals: map.locals.clone(),
            pointers: vec![None; map.locals.len()],
            instances,
        })
    }
}

impl MapScriptLocals {
    pub fn globals(&self) -> &[i32] {
        &self.globals
    }

    fn global_slot(&self, index: i32) -> io::Result<usize> {
        let index = usize::try_from(index).map_err(|_| invalid("negative CE MAP global index"))?;
        if index >= self.globals.len() {
            return Err(invalid("CE MAP global index out of bounds"));
        }
        Ok(index)
    }

    /// CE map_get_global_var with original integer and pointer side arrays.
    /// Pointer tags are opaque, never native Rust or C++ memory references.
    pub fn read_global_typed(&self, index: i32) -> io::Result<ScriptValue> {
        let slot = self.global_slot(index)?;
        Ok(match self.global_pointers[slot] {
            Some(pointer) => ScriptValue::Pointer(pointer),
            None => ScriptValue::Int(self.globals[slot]),
        })
    }

    /// CE map_set_global_var stores opaque pointers out of band, setting the
    /// serialized integer slot to zero. Non-pointer writes clear that tag.
    pub fn write_global_typed(&mut self, index: i32, value: ScriptValue) -> io::Result<()> {
        if !matches!(value, ScriptValue::Int(_) | ScriptValue::Pointer(_)) {
            return Err(invalid("unsupported CE MAP global value type"));
        }
        let slot = self.global_slot(index)?;
        match value {
            ScriptValue::Int(number) => {
                self.globals[slot] = number;
                self.global_pointers[slot] = None;
            }
            ScriptValue::Pointer(pointer) => {
                self.globals[slot] = 0;
                self.global_pointers[slot] = (pointer != 0).then_some(pointer);
            }
            _ => unreachable!(),
        }
        Ok(())
    }

    /// Integer-only MAP-global accessor, rejects nonzero opaque pointers.
    pub fn read_global(&self, index: i32) -> io::Result<i32> {
        match self.read_global_typed(index)? {
            ScriptValue::Int(value) => Ok(value),
            _ => Err(invalid(
                "CE global contains opaque pointer; typed access required",
            )),
        }
    }

    pub fn write_global(&mut self, index: i32, value: i32) -> io::Result<()> {
        self.write_global_typed(index, ScriptValue::Int(value))
    }

    pub fn locals(&self) -> &[i32] {
        &self.locals
    }

    pub fn range(&self, instance: u64) -> Option<ScriptLocalRange> {
        self.instances.get(&instance).copied()
    }

    fn slot(&mut self, instance: u64, variable: i32) -> io::Result<Option<usize>> {
        let local = self
            .instances
            .get_mut(&instance)
            .ok_or_else(|| invalid("CE script local references unknown instance"))?;
        if local.is_system {
            return Err(invalid("CE system/map script local access is unsupported"));
        }
        if local.declared == 0 {
            return Ok(None); // CE get returns success without assigning a value.
        }
        let index =
            usize::try_from(variable).map_err(|_| invalid("negative CE script local index"))?;
        if index >= local.declared {
            return Err(invalid("CE script local index exceeds declared count"));
        }
        if local.offset.is_none() {
            let start = self.locals.len();
            let end = start
                .checked_add(local.declared)
                .ok_or_else(|| invalid("CE lazy local allocation overflow"))?;
            if end > MAX_LOCAL_SLOTS {
                return Err(invalid("CE lazy local allocation exceeds Rust bound"));
            }
            self.locals.resize(end, 0); // original map_malloc_local_var.
            self.pointers.resize(end, None); // original map_local_pointers.
            local.offset = Some(start);
        }
        Ok(Some(local.offset.unwrap() + index))
    }

    /// Snapshot-only CE script-local read for already allocated/saved ranges.
    /// It never calls the mutating CE map_malloc_local_var equivalent.
    /// Unallocated lazy ranges are deliberately unsupported so a diagnostic
    /// preview cannot silently omit the source engine's allocation effect.
    /// Both integer and opaque pointer tags are retained without dereference.
    pub fn peek_saved_typed(
        &self,
        instance: u64,
        variable: i32,
    ) -> io::Result<Option<ScriptValue>> {
        let range = self
            .instances
            .get(&instance)
            .ok_or_else(|| invalid("CE read-only local references unknown instance"))?;
        if range.is_system {
            return Err(invalid("CE system/map script local access is unsupported"));
        }
        if range.declared == 0 {
            return Ok(None);
        }
        let index =
            usize::try_from(variable).map_err(|_| invalid("negative CE script local index"))?;
        if index >= range.declared {
            return Err(invalid("CE script local index exceeds declared count"));
        }
        let offset = range
            .offset
            .ok_or_else(|| invalid("CE read-only local would allocate lazy MAP storage"))?;
        let slot = offset
            .checked_add(index)
            .ok_or_else(|| invalid("CE read-only local slot overflow"))?;
        let number = *self
            .locals
            .get(slot)
            .ok_or_else(|| invalid("CE read-only local slot outside MAP"))?;
        let tag = self
            .pointers
            .get(slot)
            .ok_or_else(|| invalid("CE read-only local pointer tag outside MAP"))?;
        Ok(Some(match tag {
            Some(pointer) => ScriptValue::Pointer(*pointer),
            None => ScriptValue::Int(number),
        }))
    }

    /// CE script-local integer read, with original lazy allocation.
    /// None means no declared locals; CE get would leave its output untouched.
    pub fn read(&mut self, instance: u64, index: i32) -> io::Result<Option<i32>> {
        match self.slot(instance, index)? {
            Some(slot) if self.pointers[slot].is_some() => Err(invalid(
                "CE local contains opaque pointer; typed access required",
            )),
            Some(slot) => Ok(Some(self.locals[slot])),
            None => Ok(None),
        }
    }

    /// CE map_get_local_var: return a pointer-tagged value if its separate
    /// side table is populated, else a 32-bit integer. Null pointer values
    /// are read as integer zero, mirroring original CE map_set_local_var.
    pub fn read_typed(&mut self, instance: u64, index: i32) -> io::Result<Option<ScriptValue>> {
        Ok(self
            .slot(instance, index)?
            .map(|slot| match self.pointers[slot] {
                Some(pointer) => ScriptValue::Pointer(pointer),
                None => ScriptValue::Int(self.locals[slot]),
            }))
    }

    /// CE map_set_local_var: opaque pointers are stored out-of-band; their
    /// integer MAP slot is zeroed. Non-pointer inputs supported here are
    /// deliberately limited to integer values, not real engine object refs.
    pub fn write_typed(&mut self, instance: u64, index: i32, value: ScriptValue) -> io::Result<()> {
        if !matches!(value, ScriptValue::Int(_) | ScriptValue::Pointer(_)) {
            return Err(invalid("unsupported CE MAP local value type"));
        }
        let slot = self
            .slot(instance, index)?
            .ok_or_else(|| invalid("CE script local set without declaration"))?;
        match value {
            ScriptValue::Int(number) => {
                self.locals[slot] = number;
                self.pointers[slot] = None;
            }
            ScriptValue::Pointer(pointer) => {
                self.locals[slot] = 0;
                self.pointers[slot] = (pointer != 0).then_some(pointer);
            }
            _ => unreachable!(),
        }
        Ok(())
    }

    /// CE script-local integer write. Unlike get, no declaration is an error.
    pub fn write(&mut self, instance: u64, index: i32, value: i32) -> io::Result<()> {
        let slot = self
            .slot(instance, index)?
            .ok_or_else(|| invalid("CE script local set without declaration"))?;
        self.locals[slot] = value;
        self.pointers[slot] = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::map::MapVariables;
    use crate::script_binding::{BoundScriptRuntime, InstalledScriptBinding, ScriptBinding};
    use crate::script_list::ScriptRunInfo;
    use crate::script_program::ScriptProgram;
    use crate::script_runtime::ScriptRuntime;
    use crate::script_vm::{ScriptValue, ScriptVm};

    fn record(
        category: usize,
        id: u64,
        offset: i32,
        count: i32,
        declared: Option<i32>,
    ) -> InstalledScriptBinding {
        InstalledScriptBinding {
            binding: ScriptBinding {
                category,
                ordinal: (id - 1) as usize,
                script_id: id as i32,
                script_index: 0,
                owner_id: -1,
                path: Some("scripts/SYNTH.int".to_owned()),
                run_info: declared.map(|local_variables| ScriptRunInfo {
                    local_variables: Some(local_variables),
                    ..Default::default()
                }),
                local_variable_offset: offset,
                local_variable_count: count,
            },
            instance_id: Some(id),
        }
    }

    fn fixture() -> BoundScriptRuntime {
        let bytes = vec![0_u8; 42 + 12];
        let program = ScriptProgram::decode(&bytes).unwrap();
        let mut runtime = ScriptRuntime::new();
        for id in 1..=4 {
            runtime
                .register(id, ScriptVm::new(program.clone()))
                .unwrap();
        }
        BoundScriptRuntime {
            runtime,
            entries: vec![
                record(1, 1, 0, 2, Some(7)),
                record(1, 2, -1, 0, Some(3)),
                record(0, 3, -1, 0, Some(2)),
                record(1, 4, -1, 0, None),
            ],
        }
    }

    #[test]
    fn ce_saved_and_lazy_script_locals_share_map_storage_without_aliasing() {
        let bound = fixture();
        let original = MapVariables {
            globals: vec![99],
            locals: vec![10, 11],
        };
        let mut state = bound.script_local_state(&original).unwrap();
        assert_eq!(state.globals(), [99]);
        assert_eq!(state.read(1, 0).unwrap(), Some(10));
        assert_eq!(state.read(1, 1).unwrap(), Some(11));
        assert_eq!(state.range(2).unwrap().offset, None);
        assert_eq!(state.read(2, 1).unwrap(), Some(0));
        assert_eq!(state.range(2).unwrap().offset, Some(2));
        assert_eq!(state.locals(), [10, 11, 0, 0, 0]);
        state.write(2, 1, -5).unwrap();
        state.write(1, 0, 42).unwrap();
        assert_eq!(state.read(2, 1).unwrap(), Some(-5));
        assert_eq!(state.locals(), [42, 11, 0, -5, 0]);
        assert_eq!(original.locals, [10, 11]); // source MAP is immutable.
        assert_eq!(original.globals, [99]);
    }

    #[test]
    fn readonly_saved_local_peek_keeps_lazy_state_and_pointer_tags_intact() {
        let bound = fixture();
        let mut state = bound
            .script_local_state(&MapVariables {
                globals: vec![],
                locals: vec![10, 11],
            })
            .unwrap();
        assert_eq!(
            state.peek_saved_typed(1, 0).unwrap(),
            Some(ScriptValue::Int(10))
        );
        assert_eq!(
            state.peek_saved_typed(1, 1).unwrap(),
            Some(ScriptValue::Int(11))
        );
        assert!(state.peek_saved_typed(2, 0).is_err());
        assert_eq!(state.range(2).unwrap().offset, None);
        assert_eq!(state.locals(), [10, 11]);
        assert!(state.peek_saved_typed(3, 0).is_err()); // system script
        assert_eq!(state.peek_saved_typed(4, 0).unwrap(), None);
        assert!(state.peek_saved_typed(1, -1).is_err());
        assert!(state.peek_saved_typed(1, 2).is_err());
        assert!(state.peek_saved_typed(999, 0).is_err());

        state
            .write_typed(2, 0, ScriptValue::Pointer(-34))
            .unwrap();
        assert_eq!(
            state.peek_saved_typed(2, 0).unwrap(),
            Some(ScriptValue::Pointer(-34))
        );
        assert_eq!(
            state.peek_saved_typed(2, 1).unwrap(),
            Some(ScriptValue::Int(0))
        );
        assert_eq!(state.locals(), [10, 11, 0, 0, 0]);
    }

    #[test]
    fn ce_map_globals_preserve_integer_values_and_opaque_pointer_tags() {
        let bound = fixture();
        let original = MapVariables {
            globals: vec![12, -9],
            locals: vec![10, 11],
        };
        let mut state = bound.script_local_state(&original).unwrap();
        assert_eq!(state.read_global(0).unwrap(), 12);
        state
            .write_global_typed(1, ScriptValue::Pointer(-44))
            .unwrap();
        assert_eq!(state.globals(), [12, 0]);
        assert_eq!(
            state.read_global_typed(1).unwrap(),
            ScriptValue::Pointer(-44)
        );
        assert!(state.read_global(1).is_err());
        assert_eq!(state.read_global(0).unwrap(), 12);
        state.write_global(1, 7).unwrap();
        assert_eq!(state.read_global_typed(1).unwrap(), ScriptValue::Int(7));
        state
            .write_global_typed(0, ScriptValue::Pointer(0))
            .unwrap();
        assert_eq!(state.read_global_typed(0).unwrap(), ScriptValue::Int(0));
        assert!(state.read_global_typed(-1).is_err());
        assert!(state.write_global_typed(2, ScriptValue::Int(42)).is_err());
        assert!(state
            .write_global_typed(0, ScriptValue::Float(1.5))
            .is_err());
        assert_eq!(original.globals, [12, -9]);
        assert_eq!(original.locals, [10, 11]);
    }

    #[test]
    fn ce_pointer_tagged_locals_use_separate_side_array_without_dereference() {
        let bound = fixture();
        let original = MapVariables {
            globals: vec![99],
            locals: vec![10, 11],
        };
        let mut state = bound.script_local_state(&original).unwrap();
        state
            .write_typed(2, 1, ScriptValue::Pointer(0x1234))
            .unwrap();
        assert_eq!(state.locals(), [10, 11, 0, 0, 0]);
        assert_eq!(
            state.read_typed(2, 1).unwrap(),
            Some(ScriptValue::Pointer(0x1234))
        );
        assert!(state.read(2, 1).is_err());
        state.write(2, 1, -17).unwrap();
        assert_eq!(state.read_typed(2, 1).unwrap(), Some(ScriptValue::Int(-17)));
        state.write_typed(2, 0, ScriptValue::Pointer(0)).unwrap();
        assert_eq!(state.read_typed(2, 0).unwrap(), Some(ScriptValue::Int(0)));
        assert!(state.write_typed(2, 1, ScriptValue::Float(1.5)).is_err());
        assert_eq!(state.read_typed(2, 1).unwrap(), Some(ScriptValue::Int(-17)));
        assert_eq!(original.locals, [10, 11]);
    }

    #[test]
    fn ce_locals_reject_out_of_range_missing_system_and_undeclared_slots() {
        let bound = fixture();
        let original = MapVariables {
            globals: vec![],
            locals: vec![1, 2],
        };
        let mut state = bound.script_local_state(&original).unwrap();
        assert!(state.read(1, -1).is_err());
        assert!(state.read(1, 2).is_err());
        assert!(state.read(3, 0).is_err()); // system script, no local vars
        assert_eq!(state.read(4, 0).unwrap(), None);
        assert!(state.write(4, 0, 1).is_err());
        assert!(state.read(999, 0).is_err());
        assert_eq!(state.locals(), [1, 2]);
        let broken = MapVariables {
            globals: vec![],
            locals: vec![0],
        };
        assert!(bound.script_local_state(&broken).is_err());
        let mut invalid_bound = fixture();
        invalid_bound.entries[1].binding.local_variable_count = -1;
        assert!(invalid_bound.script_local_state(&original).is_err());
    }
}
