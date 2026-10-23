use rust_age::assets::{Bank, GameAssets};
use rust_age::dat::DatArchive;
use rust_age::framebuffer::IndexedSurface;
use rust_age::frm::Frm;
use rust_age::hex::HexViewport;
use rust_age::map_preview::FloorPreview;
use rust_age::object_preview::ObjectPreview;
use rust_age::palette::Palette;
use rust_age::prototype::Prototype;
use rust_age::scene::StaticScene;
use rust_age::script_lifecycle::MapLifecyclePhase;
use rust_age::script_procedures::{build_lookup_table, SCRIPT_PROC_NAMES};
use rust_age::script_program::ScriptProgram;
use std::env;
use std::fs::File;
use std::path::Path;

fn usage() {
    println!("Rust Age - Fallout 1 CE Rust port (not yet playable)");
    println!("Usage: rust-age --check-installation /path/to/Fallout");
    println!("       rust-age --verify-installation /path/to/Fallout");
    println!("       rust-age --inspect-dat /path/to/master.dat");
    println!("       rust-age --inspect-frm /path/to/Fallout master art/items/item.frm");
    println!("       rust-age --inspect-map /path/to/Fallout maps/v13.map");
    println!("       rust-age --inspect-map-scripts /path/to/Fallout maps/v13.map");
    println!("       rust-age --inspect-map-procedures /path/to/Fallout maps/v13.map");
    println!("       rust-age --inspect-map-lifecycle /path/to/Fallout maps/v13.map enter MAP_SID MAP_FLAGS");
    println!("       rust-age --inspect-map-route /path/to/Fallout maps/v13.map 20099 20098");
    println!("       rust-age --inspect-pro /path/to/Fallout proto/items/example.pro");
    println!("       rust-age --inspect-int /path/to/Fallout scripts/example.int");
    println!("       rust-age --inspect-script-index /path/to/Fallout 0");
    println!("       rust-age --export-map-floor-preview /path/to/Fallout maps/v13.map floor.ppm");
    println!("       rust-age --export-map-roof-preview /path/to/Fallout maps/v13.map roof.ppm");
    println!(
        "       rust-age --export-map-object-preview /path/to/Fallout maps/v13.map objects.ppm"
    );
    println!("       rust-age --export-frm-preview /path/to/Fallout master art/items/item.frm 0 0 preview.ppm");
    println!("       rust-age --help");
}

fn fail(message: impl std::fmt::Display) -> ! {
    eprintln!("{message}");
    std::process::exit(2);
}

fn bank_for(name: &str) -> Bank {
    match name {
        "master" => Bank::Master,
        "critter" => Bank::Critter,
        _ => fail("Bank must be 'master' or 'critter'"),
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    match args.as_slice() {
        [_] => usage(),
        [_, flag] if flag == "--help" || flag == "-h" => usage(),
        [_, flag] if flag == "--version" => println!("rust-age {}", env!("CARGO_PKG_VERSION")),
        [_, flag, directory] if flag == "--check-installation" => {
            let root = Path::new(directory);
            if !root.is_dir() {
                fail(format!("Not a directory: {}", root.display()));
            }
            let required = ["master.dat", "critter.dat", "data"];
            let missing: Vec<_> = required
                .into_iter()
                .filter(|name| {
                    let path = root.join(name);
                    if *name == "data" {
                        !path.is_dir()
                    } else {
                        !path.is_file()
                    }
                })
                .collect();
            if missing.is_empty() {
                println!("Fallout 1 asset paths present in {}", root.display());
                println!("Archive contents not validated; engine not yet playable.");
            } else {
                fail(format!(
                    "Missing game assets: {}. Paths are case-sensitive in this basic check.",
                    missing.join(", ")
                ));
            }
        }
        [_, flag, directory] if flag == "--verify-installation" => {
            match GameAssets::open(Path::new(directory)) {
                Ok(assets) => {
                    println!("Fallout 1 DAT indexes loaded.");
                    println!("Master entries: {}", assets.entry_count(Bank::Master));
                    println!("Critter entries: {}", assets.entry_count(Bank::Critter));
                    println!("No gameplay engine yet. Archive contents not exhaustively decoded.");
                }
                Err(error) => fail(format!("Fallout 1 installation validation failed: {error}")),
            }
        }
        [_, flag, path] if flag == "--inspect-dat" => {
            let result = File::open(path).and_then(DatArchive::open);
            match result {
                Ok(dat) => {
                    println!("Archive: {path}");
                    println!("Entries indexed: {}", dat.entries().count());
                    println!("Index parsed. Game engine not yet playable.");
                }
                Err(error) => fail(format!("Cannot read Fallout 1 DAT archive: {error}")),
            }
        }
        [_, flag, directory, bank, path] if flag == "--inspect-frm" => {
            let bank = bank_for(bank);
            let result = GameAssets::open(Path::new(directory))
                .and_then(|mut assets| assets.read(bank, path))
                .and_then(|bytes| {
                    bytes.ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))
                })
                .and_then(|bytes| Frm::decode(&bytes));
            match result {
                Ok(sprite) => {
                    println!("Fallout 1 sprite: {path}");
                    println!("Frames per direction: {}", sprite.frame_count);
                    println!("Directions: {}", sprite.frames.len());
                    println!("FPS: {}", sprite.frames_per_second);
                    println!("No graphics renderer yet.");
                }
                Err(error) => fail(format!("Cannot decode sprite: {error}")),
            }
        }
        [_, flag, directory, path] if flag == "--inspect-int" => {
            let result = GameAssets::open(Path::new(directory))
                .and_then(|mut assets| assets.read(Bank::Master, path))
                .and_then(|bytes| {
                    bytes.ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))
                })
                .and_then(|bytes| ScriptProgram::decode(&bytes));
            match result {
                Ok(program) => {
                    println!("Fallout 1 .INT script: {path}");
                    println!("Original CE procedures: {}", program.procedures().len());
                    for (index, entry) in program.procedures().iter().take(16).enumerate() {
                        match program.procedure_name(index) {
                            Ok(name) => println!(
                                "  [{index}] {} (offset {}, arguments {})",
                                String::from_utf8_lossy(name),
                                entry.body_offset,
                                entry.argument_count
                            ),
                            Err(error) => println!("  [{index}] invalid procedure name: {error}"),
                        }
                    }
                    println!("Script program metadata only; no bytecode executed.");
                }
                Err(error) => fail(format!("Cannot inspect Fallout 1 .INT script: {error}")),
            }
        }
        [_, flag, directory, index] if flag == "--inspect-script-index" => {
            let index = index
                .parse::<u32>()
                .unwrap_or_else(|_| fail("Invalid nonnegative script index"));
            let result: std::io::Result<_> = (|| {
                let mut assets = GameAssets::open(Path::new(directory))?;
                let path = assets
                    .script_path_by_index(index)?
                    .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))?;
                let program = assets
                    .script_program_by_index(index)?
                    .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))?;
                let procedures = program
                    .procedures()
                    .iter()
                    .enumerate()
                    .map(|(index, entry)| {
                        Ok((
                            program.procedure_name(index)?.escape_ascii().to_string(),
                            entry.flags,
                            entry.body_offset,
                            entry.argument_count,
                        ))
                    })
                    .collect::<std::io::Result<Vec<_>>>()?;
                Ok((path, procedures, build_lookup_table(&program)?))
            })();
            match result {
                Ok((path, procedures, actions)) => {
                    println!("Fallout CE original script index: {index}");
                    println!("Original script file: {path}");
                    println!("Original procedures: {}", procedures.len());
                    for (proc_index, (name, flags, body, arguments)) in
                        procedures.iter().enumerate()
                    {
                        println!(
                            "  procedure [{proc_index}] {name} flags={flags:#X} body={body} args={arguments}"
                        );
                    }
                    println!(
                        "CE built-in action bindings: {} of {}",
                        actions.iter().filter(|entry| entry.is_some()).count(),
                        SCRIPT_PROC_NAMES.len()
                    );
                    for (action, procedure) in actions.iter().enumerate() {
                        if let Some(procedure) = procedure {
                            println!(
                                "  action [{action}] {} -> procedure [{procedure}]",
                                SCRIPT_PROC_NAMES[action]
                            );
                        }
                    }
                    println!("Read-only bytecode metadata; no script has been executed.");
                }
                Err(error) => fail(format!("Cannot inspect CE script index: {error}")),
            }
        }
        [_, flag, directory, path] if flag == "--inspect-pro" => {
            let result = GameAssets::open(Path::new(directory))
                .and_then(|mut assets| assets.read(Bank::Master, path))
                .and_then(|bytes| {
                    bytes.ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))
                })
                .and_then(|bytes| Prototype::decode(&bytes));
            match result {
                Ok(proto) => {
                    println!("Fallout 1 prototype type: {:?}", proto.kind);
                    println!("PID: {:#010X}", proto.pid);
                    println!("Art FID: {:#010X}", proto.fid);
                    println!("Message ID: {}", proto.message_id);
                    println!(
                        "Uninterpreted trailing bytes: {}",
                        proto.remaining_bytes.len()
                    );
                    println!("Prototype data inspection only; no gameplay object instantiated.");
                }
                Err(error) => fail(format!("Cannot inspect Fallout 1 prototype: {error}")),
            }
        }
        [_, flag, directory, path, phase_name, map_sid, map_flags]
            if flag == "--inspect-map-lifecycle" =>
        {
            let phase = match phase_name.as_str() {
                "start" => MapLifecyclePhase::Start,
                "enter" => MapLifecyclePhase::Enter,
                "update" => MapLifecyclePhase::Update,
                "exit" => MapLifecyclePhase::Exit,
                _ => fail("MAP lifecycle must be start, enter, update, or exit"),
            };
            let sid = map_sid
                .parse::<i32>()
                .unwrap_or_else(|_| fail("MAP script SID must be a decimal i32"));
            let flags = map_flags
                .parse::<i32>()
                .unwrap_or_else(|_| fail("MAP flags must be a decimal i32"));
            let result: std::io::Result<_> = (|| {
                let mut assets = GameAssets::open(Path::new(directory))?;
                let scene = StaticScene::load(&mut assets, path)?
                    .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))?;
                let runtime = scene.unstarted_script_runtime(&mut assets)?;
                let actions = runtime.plan_map_lifecycle(phase, sid, flags)?;
                Ok((runtime.entries.len(), runtime.runtime.len(), actions))
            })();
            match result {
                Ok((records, installed, actions)) => {
                    println!("CE MAP lifecycle plan ({phase_name}): {} actions", actions.len());
                    println!("Original MAP script records: {records}; installed INT VMs: {installed}");
                    for action in actions {
                        println!(
                            "  SID {:#010X} [{}:{}] instance {} action [{}] {} -> procedure [{}], ext_param={:?}",
                            action.script_id as u32,
                            action.category,
                            action.ordinal,
                            action.instance_id,
                            action.action,
                            SCRIPT_PROC_NAMES[action.action],
                            action.procedure_index,
                            action.external_parameter
                        );
                    }
                    println!("Selection only; no Fallout script has executed.");
                }
                Err(error) => fail(format!("Cannot inspect MAP lifecycle: {error}")),
            }
        }
        [_, flag, directory, path] if flag == "--inspect-map-procedures" => {
            let result: std::io::Result<_> = (|| {
                let mut assets = GameAssets::open(Path::new(directory))?;
                let scene = StaticScene::load(&mut assets, path)?
                    .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))?;
                scene.script_procedure_bindings(&mut assets)
            })();
            match result {
                Ok(bindings) => {
                    println!("Original MAP script procedure bindings: {}", bindings.len());
                    for entry in bindings {
                        let script = &entry.binding;
                        println!(
                            "  [{}:{}] SID {:#010X}, script index {} -> {}",
                            script.category,
                            script.ordinal,
                            script.script_id as u32,
                            script.script_index,
                            script.path.as_deref().unwrap_or("(unresolved)")
                        );
                        if let (Some(count), Some(actions)) = (entry.procedure_count, entry.actions)
                        {
                            println!(
                                "    INT procedures: {count}, CE action bindings: {} of {}",
                                actions.iter().filter(|item| item.is_some()).count(),
                                SCRIPT_PROC_NAMES.len()
                            );
                            for (action, procedure) in actions.iter().enumerate() {
                                if let Some(procedure) = procedure {
                                    println!(
                                        "    action [{action}] {} -> procedure [{procedure}]",
                                        SCRIPT_PROC_NAMES[action]
                                    );
                                }
                            }
                        } else {
                            println!("    Installed INT missing or script index unresolved");
                        }
                    }
                    println!("Metadata inspection only; no Fallout script has executed.");
                }
                Err(error) => fail(format!("Cannot inspect MAP script procedures: {error}")),
            }
        }
        [_, flag, directory, path] if flag == "--inspect-map-scripts" => {
            let result: std::io::Result<_> = (|| {
                let mut assets = GameAssets::open(Path::new(directory))?;
                let scene = StaticScene::load(&mut assets, path)?
                    .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))?;
                scene.script_bindings(&mut assets)
            })();
            match result {
                Ok(bindings) => {
                    println!("Original MAP script bindings: {}", bindings.len());
                    for entry in bindings {
                        let path = entry.path.as_deref().unwrap_or("(unresolved)");
                        let flags = entry.run_info.unwrap_or_default();
                        println!(
                            "  [{}:{}] SID {:#010X}, script index {} -> {}",
                            entry.category,
                            entry.ordinal,
                            entry.script_id as u32,
                            entry.script_index,
                            path
                        );
                        println!(
                            "    map_init={} map_exit={} list_local_vars={:?} map_local_offset={} map_local_count={}",
                            flags.map_init,
                            flags.map_exit,
                            flags.local_variables,
                            entry.local_variable_offset,
                            entry.local_variable_count
                        );
                    }
                    println!("Script names and flags only; no Fallout script has executed.");
                }
                Err(error) => fail(format!("Cannot inspect Fallout 1 MAP scripts: {error}")),
            }
        }
        [_, flag, directory, path] if flag == "--inspect-map" => {
            let result = GameAssets::open(Path::new(directory)).and_then(|mut assets| {
                StaticScene::load(&mut assets, path)?
                    .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))
            });
            match result {
                Ok(scene) => {
                    println!(
                        "Fallout 1 map: {}",
                        String::from_utf8_lossy(scene.header.name_bytes())
                    );
                    println!("Entering tile: {}", scene.header.entering_tile);
                    println!("Entering elevation: {}", scene.header.entering_elevation);
                    println!(
                        "Available square layers: {}",
                        scene
                            .squares
                            .elevations
                            .iter()
                            .filter(|layer| layer.is_some())
                            .count()
                    );
                    println!(
                        "Original scripts begin at byte {}",
                        scene.squares.scripts_offset
                    );
                    println!(
                        "Map variable slots: {} global, {} local",
                        scene.variables.globals.len(),
                        scene.variables.locals.len()
                    );
                    println!(
                        "Serialized script records: {}",
                        scene.scripts.script_count()
                    );
                    println!(
                        "Serialized top-level objects: {}",
                        scene.objects.total_objects
                    );
                    println!("Original object data ends at byte {}", scene.bytes_consumed);
                    println!("No scripts executed or gameplay objects instantiated.");
                }
                Err(error) => fail(format!("Cannot inspect Fallout 1 map: {error}")),
            }
        }
        [_, flag, directory, path, from, to] if flag == "--inspect-map-route" => {
            let from = from
                .parse::<i32>()
                .unwrap_or_else(|_| fail("Invalid starting hex tile"));
            let to = to
                .parse::<i32>()
                .unwrap_or_else(|_| fail("Invalid destination hex tile"));
            let result: std::io::Result<_> = (|| {
                let mut assets = GameAssets::open(Path::new(directory))?;
                let scene = StaticScene::load(&mut assets, path)?
                    .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))?;
                let view = HexViewport::new(200, 200, 640, 380, scene.header.entering_tile)
                    .ok_or_else(|| std::io::Error::other("invalid map viewport"))?;
                let elevation = usize::try_from(scene.header.entering_elevation)
                    .map_err(|_| std::io::Error::other("invalid map elevation"))?;
                Ok(scene.route(&view, from, to, elevation, None, true))
            })();
            match result {
                Ok(Some(rotations)) => {
                    println!("CE-style diagnostic route: {from} to {to}");
                    println!("Steps: {}", rotations.len());
                    println!(
                        "Rotations: {}",
                        rotations
                            .iter()
                            .map(u8::to_string)
                            .collect::<Vec<_>>()
                            .join(",")
                    );
                    println!("No actor moved; locked-door handling and runtime state are absent.");
                }
                Ok(None) => println!("No route found under current static MAP collision data."),
                Err(error) => fail(format!("Cannot inspect Fallout 1 map route: {error}")),
            }
        }
        [_, flag, directory, path, destination]
            if flag == "--export-map-floor-preview"
                || flag == "--export-map-roof-preview"
                || flag == "--export-map-object-preview" =>
        {
            let mut assets = GameAssets::open(Path::new(directory))
                .unwrap_or_else(|error| fail(format!("Cannot open installation: {error}")));
            let palette_bytes = assets
                .read(Bank::Master, "color.pal")
                .unwrap_or_else(|error| fail(format!("Cannot read palette: {error}")))
                .unwrap_or_else(|| fail("Original color.pal missing"));
            let palette = Palette::decode(&palette_bytes)
                .unwrap_or_else(|error| fail(format!("Cannot decode palette: {error}")));
            let map_bytes = assets
                .read(Bank::Master, path)
                .unwrap_or_else(|error| fail(format!("Cannot load original map: {error}")))
                .unwrap_or_else(|| fail(format!("Map not found: {path}")));
            let mut preview = FloorPreview::render_layers(
                &mut assets,
                &map_bytes,
                640,
                380,
                flag == "--export-map-roof-preview",
            )
            .unwrap_or_else(|error| fail(format!("Cannot draw MAP preview: {error}")));
            if flag == "--export-map-object-preview" {
                let scene = StaticScene::decode(&mut assets, &map_bytes)
                    .unwrap_or_else(|error| fail(format!("Cannot decode scene objects: {error}")));
                let view = HexViewport::new(200, 200, 640, 380, scene.header.entering_tile)
                    .unwrap_or_else(|| fail("Invalid map viewport"));
                let elevation = usize::try_from(scene.header.entering_elevation)
                    .unwrap_or_else(|_| fail("Invalid map elevation"));
                let stats = ObjectPreview::draw(
                    &mut assets,
                    &scene.objects,
                    &view,
                    elevation,
                    &mut preview.surface,
                )
                .unwrap_or_else(|error| fail(format!("Cannot draw scene objects: {error}")));
                println!(
                    "Static scene sprites: {} drawn, {} hidden, {} missing art",
                    stats.rendered, stats.hidden, stats.missing_art
                );
            }
            let output = File::create(destination)
                .unwrap_or_else(|error| fail(format!("Cannot create preview file: {error}")));
            preview
                .surface
                .write_ppm(&palette, output)
                .unwrap_or_else(|error| fail(format!("Cannot save preview: {error}")));
            println!("Saved static MAP image: {destination}");
            println!("Candidate tiles: {}", preview.tile_count);
            println!("Unique tile art: {}", preview.unique_art);
            println!("Roof tiles: {}", preview.roof_count);
            if flag == "--export-map-object-preview" {
                println!("Objects rendered diagnostically without original roof/lighting order.");
            } else {
                println!("Static tiles only; no object lighting, combat or gameplay.");
            }
        }
        [_, flag, directory, bank, path, direction, frame_number, destination]
            if flag == "--export-frm-preview" =>
        {
            let bank = bank_for(bank);
            let direction = direction
                .parse::<usize>()
                .unwrap_or_else(|_| fail("Invalid direction (expected 0..5)"));
            let frame_number = frame_number
                .parse::<usize>()
                .unwrap_or_else(|_| fail("Invalid frame number"));
            let mut assets = GameAssets::open(Path::new(directory))
                .unwrap_or_else(|error| fail(format!("Cannot open installation: {error}")));
            let palette_bytes = assets
                .read(Bank::Master, "color.pal")
                .unwrap_or_else(|error| fail(format!("Cannot read original palette: {error}")))
                .unwrap_or_else(|| fail("color.pal not found in original game data"));
            let palette = Palette::decode(&palette_bytes)
                .unwrap_or_else(|error| fail(format!("Invalid color.pal: {error}")));
            let sprite_bytes = assets
                .read(bank, path)
                .unwrap_or_else(|error| fail(format!("Cannot read sprite: {error}")))
                .unwrap_or_else(|| fail(format!("Sprite not found: {path}")));
            let sprite = Frm::decode(&sprite_bytes)
                .unwrap_or_else(|error| fail(format!("Cannot decode FRM: {error}")));
            let frame = sprite
                .frames
                .get(direction)
                .and_then(|rotation| rotation.get(frame_number))
                .unwrap_or_else(|| fail("Direction or frame out of range"));
            let mut surface = IndexedSurface::new(frame.width as usize, frame.height as usize)
                .unwrap_or_else(|error| {
                    fail(format!("Cannot allocate diagnostic framebuffer: {error}"))
                });
            surface
                .blit_frame(frame, 0, 0, false)
                .unwrap_or_else(|error| fail(format!("Cannot render indexed frame: {error}")));
            let output = File::create(destination)
                .unwrap_or_else(|error| fail(format!("Cannot create preview file: {error}")));
            surface
                .write_ppm(&palette, output)
                .unwrap_or_else(|error| fail(format!("Cannot write preview: {error}")));
            println!("Wrote diagnostic RGB preview: {destination}");
            println!("Does not include CE gamma, lighting, roofs or native window rendering.");
        }
        _ => {
            usage();
            std::process::exit(2);
        }
    }
}
