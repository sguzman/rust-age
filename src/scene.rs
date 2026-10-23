//! Read-only Fallout 1 CE MAP loading assembly.
//!
//! Mirrors map_load_file's data order: header, global/local variables,
//! floor/roof squares, script records, and object/inventory tables.
//! Only source file formats and collision are loaded. This is not a running
//! gameplay world: no scripts, party, lighting, time, or actor movement.
use crate::assets::{Bank, GameAssets};
use crate::collision::{CollisionIndex, ObjectKey};
use crate::hex::HexViewport;
use crate::map::{MapHeader, MapSquares, MapVariables};
use crate::object_tables::ObjectTables;
use crate::pathfinding::make_path;
use crate::prototype::PrototypeBody;
use crate::script_binding::{
    bind_script_procedures, bind_scripts, install_map_scripts, BoundScriptRuntime, ScriptBinding,
    ScriptProcedureBinding,
};
use crate::script_data::ScriptTables;
use crate::script_locals::MapScriptLocals;
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
        let view = HexViewport::new(200, 200, 640, 380, 20099).unwrap();
        assert_eq!(
            scene.route(&view, 20099, 20098, 0, None, true),
            Some(vec![1])
        );
        fs::remove_dir_all(root).unwrap();
    }
}
