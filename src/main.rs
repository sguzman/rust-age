use rust_age::assets::{Bank, GameAssets};
use rust_age::dat::DatArchive;
use rust_age::framebuffer::IndexedSurface;
use rust_age::frm::Frm;
use rust_age::map::MapHeader;
use rust_age::palette::Palette;
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
        [_, flag, directory, path] if flag == "--inspect-map" => {
            let result = GameAssets::open(Path::new(directory))
                .and_then(|mut assets| assets.read(Bank::Master, path))
                .and_then(|bytes| {
                    bytes.ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))
                })
                .and_then(|bytes| {
                    let header = MapHeader::decode(&bytes)?;
                    let squares = header.decode_squares(&bytes)?;
                    Ok((header, squares))
                });
            match result {
                Ok((header, squares)) => {
                    println!("Fallout 1 map: {}", String::from_utf8_lossy(header.name_bytes()));
                    println!("Entering tile: {}", header.entering_tile);
                    println!("Entering elevation: {}", header.entering_elevation);
                    println!(
                        "Available square layers: {}",
                        squares.elevations.iter().filter(|layer| layer.is_some()).count()
                    );
                    println!("Original scripts begin at byte {}", squares.scripts_offset);
                    println!("Scripts/objects not yet decoded; game not playable.");
                }
                Err(error) => fail(format!("Cannot inspect Fallout 1 map: {error}")),
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
