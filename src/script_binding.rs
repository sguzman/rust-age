//! Immutable CE MAP script-to-program name binding.
//!
//! This joins serialized MAP script records and scripts/scripts.lst by their
//! original lower-24-bit index. It cannot instantiate or execute scripts.
//! Map-local variable allocation, triggers and VM lifecycle come later.
use crate::assets::GameAssets;
use crate::script_data::ScriptTables;
use crate::script_list::ScriptRunInfo;
use crate::script_procedures::build_lookup_table;
use std::collections::BTreeMap;
use std::io;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptBinding {
    /// Original CE script category 0..4.
    pub category: usize,
    /// Per-category sequence within the deserialized script list.
    pub ordinal: usize,
    pub script_id: i32,
    pub script_index: i32,
    pub owner_id: i32,
    pub path: Option<String>,
    pub run_info: Option<ScriptRunInfo>,
    pub local_variable_offset: i32,
    pub local_variable_count: i32,
}

/// Resolve all map script records against the original installed scripts.lst.
/// An absent list row or script index is represented by None, not fabricated.
pub fn bind_scripts(
    assets: &mut GameAssets,
    tables: &ScriptTables,
) -> io::Result<Vec<ScriptBinding>> {
    let mut bindings = Vec::with_capacity(tables.script_count());
    for (category, scripts) in tables.categories.iter().enumerate() {
        for (ordinal, script) in scripts.iter().enumerate() {
            let (path, run_info) = if script.script_index >= 0 {
                let index = script.script_index as u32;
                (
                    assets.script_path_by_index(index)?,
                    assets.script_run_info_by_index(index)?,
                )
            } else {
                (None, None)
            };
            bindings.push(ScriptBinding {
                category,
                ordinal,
                script_id: script.script_id,
                script_index: script.script_index,
                owner_id: script.owner_id,
                path,
                run_info,
                local_variable_offset: script.local_variable_offset,
                local_variable_count: script.local_variable_count,
            });
        }
    }
    Ok(bindings)
}

/// Read-only join of original MAP records, scripts.lst and installed INT
/// procedure tables. An absent or unresolvable program is represented by
/// None, never by an invented procedure. No script executes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptProcedureBinding {
    pub binding: ScriptBinding,
    pub procedure_count: Option<usize>,
    /// CE's original 24 action slots; duplicates preserve their indices.
    pub actions: Option<[Option<usize>; 24]>,
}

/// Decode each original script index only once per MAP inspection, even when
/// several serialized script records share the same installed INT program.
pub fn bind_script_procedures(
    assets: &mut GameAssets,
    tables: &ScriptTables,
) -> io::Result<Vec<ScriptProcedureBinding>> {
    let bindings = bind_scripts(assets, tables)?;
    let mut cache: BTreeMap<u32, Option<(usize, [Option<usize>; 24])>> = BTreeMap::new();
    let mut resolved = Vec::with_capacity(bindings.len());
    for binding in bindings {
        let metadata = if binding.script_index >= 0 && binding.path.is_some() {
            let index = (binding.script_index as u32) & 0x00FF_FFFF;
            if let Some(entry) = cache.get(&index) {
                *entry
            } else {
                let value = match assets.script_program_by_index(index)? {
                    Some(program) => {
                        Some((program.procedures().len(), build_lookup_table(&program)?))
                    }
                    None => None,
                };
                cache.insert(index, value);
                value
            }
        } else {
            None
        };
        resolved.push(ScriptProcedureBinding {
            binding,
            procedure_count: metadata.map(|entry| entry.0),
            actions: metadata.map(|entry| entry.1),
        });
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::{bind_script_procedures, bind_scripts};
    use crate::assets::GameAssets;
    use crate::script_data::{ScriptRecord, ScriptTables};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn record(sid: i32, index: i32) -> ScriptRecord {
        ScriptRecord {
            script_id: sid,
            next: -1,
            subtype: vec![],
            flags: 0,
            script_index: index,
            program_token: -1,
            owner_id: -1,
            local_variable_offset: -1,
            local_variable_count: 0,
            remaining: [0; 8],
        }
    }

    #[test]
    fn joins_original_map_script_category_with_list_index_and_flags() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "rust-age-bind-scripts-{}-{stamp}",
            std::process::id()
        ));
        let scripts = root.join("data").join("scripts");
        fs::create_dir_all(&scripts).unwrap();
        fs::write(root.join("master.dat"), [0_u8; 16]).unwrap();
        fs::write(root.join("critter.dat"), [0_u8; 16]).unwrap();
        fs::write(
            scripts.join("scripts.lst"),
            b"START.ssl # map_init local_vars=3\nEXIT.ssl # map_exit\n",
        )
        .unwrap();
        // Invented INT with one named procedure. Its body never executes.
        let mut int_bytes = vec![0_u8; 42];
        for number in [1_i32, 4, 0, 0, 0, 0, 0, 6] {
            int_bytes.extend_from_slice(&number.to_be_bytes());
        }
        int_bytes.extend_from_slice(b"start\0");
        int_bytes.extend_from_slice(&0_i32.to_be_bytes());
        fs::write(scripts.join("START.int"), int_bytes).unwrap();
        let tables = ScriptTables {
            categories: [
                vec![record(0, 0), record(1, 20)],
                vec![record(0x0100_0001, 1)],
                vec![],
                vec![],
                vec![record(0x0400_0001, -1)],
            ],
            bytes_consumed: 0,
        };
        let mut assets = GameAssets::open(&root).unwrap();
        let bindings = bind_scripts(&mut assets, &tables).unwrap();
        assert_eq!(bindings.len(), 4);
        assert_eq!(bindings[0].category, 0);
        assert_eq!(bindings[0].path.as_deref(), Some("scripts/START.int"));
        let flags = bindings[0].run_info.unwrap();
        assert!(flags.map_init);
        assert!(!flags.map_exit);
        assert_eq!(flags.local_variables, Some(3));
        assert_eq!(bindings[1].path, None);
        assert_eq!(bindings[1].run_info, None);
        assert_eq!(bindings[2].category, 1);
        assert_eq!(bindings[2].path.as_deref(), Some("scripts/EXIT.int"));
        assert!(bindings[2].run_info.unwrap().map_exit);
        assert_eq!(bindings[3].script_id, 0x0400_0001);
        assert_eq!(bindings[3].path, None);
        let detail = bind_script_procedures(&mut assets, &tables).unwrap();
        assert_eq!(detail.len(), 4);
        assert_eq!(detail[0].procedure_count, Some(1));
        assert_eq!(detail[0].actions.unwrap()[1], Some(0));
        assert_eq!(detail[1].procedure_count, None); // no scripts.lst entry
        assert_eq!(detail[2].binding.path.as_deref(), Some("scripts/EXIT.int"));
        assert_eq!(detail[2].procedure_count, None); // missing installed INT
        assert_eq!(detail[3].procedure_count, None); // negative index
        assert!(detail[3].actions.is_none());
        fs::remove_dir_all(root).unwrap();
    }
}
