use rust_age::assets::{Bank, GameAssets};
use rust_age::collision::CollisionIndex;
use rust_age::dat::DatArchive;
use rust_age::framebuffer::IndexedSurface;
use rust_age::frm::Frm;
use rust_age::hex::HexViewport;
use rust_age::map::MapHeader;
use rust_age::map_preview::FloorPreview;
use rust_age::palette::Palette;
use rust_age::pathfinding::make_path;
use rust_age::prototype::{Prototype, PrototypeBody};
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
    println!("       rust-age --inspect-map-route /path/to/Fallout maps/v13.map 20099 20098");
    println!("       rust-age --inspect-pro /path/to/Fallout proto/items/example.pro");
    println!("       rust-age --export-map-floor-preview /path/to/Fallout maps/v13.map floor.ppm");
    println!("       rust-age --export-map-roof-preview /path/to/Fallout maps/v13.map roof.ppm");
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
        [_, flag, directory, path] if flag == "--inspect-map" => {
            let result: std::io::Result<_> = (|| {
                let mut assets = GameAssets::open(Path::new(directory))?;
                let bytes = assets
                    .read(Bank::Master, path)?
                    .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))?;
                let header = MapHeader::decode(&bytes)?;
                let squares = header.decode_squares(&bytes)?;
                let (scripts, objects, object_end) =
                    squares.decode_objects_with(&bytes, |pid| {
                        let proto = assets.prototype_by_pid(pid)?.ok_or_else(|| {
                            std::io::Error::new(
                                std::io::ErrorKind::NotFound,
                                format!("missing original prototype {pid:#010X}"),
                            )
                        })?;
                        Ok(match proto.body {
                            PrototypeBody::Item { item_type, .. } => Some(item_type),
                            PrototypeBody::Scenery { scenery_type, .. } => Some(scenery_type),
                            _ => None,
                        })
                    })?;
                Ok((header, squares, scripts, objects, object_end))
            })();
            match result {
                Ok((header, squares, scripts, objects, object_end)) => {
                    println!(
                        "Fallout 1 map: {}",
                        String::from_utf8_lossy(header.name_bytes())
                    );
                    println!("Entering tile: {}", header.entering_tile);
                    println!("Entering elevation: {}", header.entering_elevation);
                    println!(
                        "Available square layers: {}",
                        squares
                            .elevations
                            .iter()
                            .filter(|layer| layer.is_some())
                            .count()
                    );
                    println!("Original scripts begin at byte {}", squares.scripts_offset);
                    println!(
                        "Map variable slots: {} global, {} local",
                        header.global_variable_count, header.local_variable_count
                    );
                    println!("Serialized script records: {}", scripts.script_count());
                    println!("Serialized top-level objects: {}", objects.total_objects);
                    println!("Original object data ends at byte {object_end}");
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
                let bytes = assets
                    .read(Bank::Master, path)?
                    .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))?;
                let header = MapHeader::decode(&bytes)?;
                let squares = header.decode_squares(&bytes)?;
                let (_, objects, _) = squares.decode_objects_with(&bytes, |pid| {
                    let proto = assets.prototype_by_pid(pid)?.ok_or_else(|| {
                        std::io::Error::new(
                            std::io::ErrorKind::NotFound,
                            format!("missing original prototype {pid:#010X}"),
                        )
                    })?;
                    Ok(match proto.body {
                        PrototypeBody::Item { item_type, .. } => Some(item_type),
                        PrototypeBody::Scenery { scenery_type, .. } => Some(scenery_type),
                        _ => None,
                    })
                })?;
                let view = HexViewport::new(200, 200, 640, 380, header.entering_tile)
                    .ok_or_else(|| std::io::Error::other("invalid map viewport"))?;
                let collision = CollisionIndex::from_map(&objects);
                let elevation = usize::try_from(header.entering_elevation)
                    .map_err(|_| std::io::Error::other("invalid map elevation"))?;
                Ok(make_path(&view, from, to, true, true, |tile| {
                    collision.is_passable(&view, tile, elevation, None)
                }))
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
            if flag == "--export-map-floor-preview" || flag == "--export-map-roof-preview" =>
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
            let preview = FloorPreview::render_layers(
                &mut assets,
                &map_bytes,
                640,
                380,
                flag == "--export-map-roof-preview",
            )
            .unwrap_or_else(|error| fail(format!("Cannot draw MAP preview: {error}")));
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
            println!("No lighting, roofs, objects, combat or gameplay.");
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
