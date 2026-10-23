//! Read-only Fallout 1 CE MAP loading assembly.
//!
//! Mirrors map_load_file's data order: header, global/local variables,
//! floor/roof squares, script records, and object/inventory tables.
//! Only source file formats and collision are loaded. This is not a running
//! gameplay world: no scripts, party, lighting, time, or actor movement.
use crate::assets::{Bank, GameAssets};
use crate::collision::{CollisionIndex, ObjectKey};
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
use crate::script_data::ScriptTables;
use crate::script_locals::MapScriptLocals;
use crate::script_object_binding::MapObjectReadBindings;
use crate::script_owner_binding::MapScriptOwners;
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

/// Owned, unstarted CE script environment: per-record VMs and MAP-local
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
        let script_owners = MapScriptOwners::from_map(&self.scripts, &self.objects, &object_queries)?;
        Ok(PreparedScriptEnvironment {
            scripts,
            map_locals,
            game_globals,
            object_queries,
            script_owners,
            map_last_visit_ticks: self.header.last_visit_time,
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
        assert_eq!(environment.map_locals.globals(), [12, -8]);
        assert_eq!(environment.map_locals.locals(), [99]);
        assert_eq!(
            environment.map_last_visit_ticks,
            scene.header.last_visit_time
        );
        assert_eq!(environment.game_globals.unwrap().values(), [7, -13]);
        assert_eq!(scene.variables.globals, [12, -8]); // game globals stay separate.
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
