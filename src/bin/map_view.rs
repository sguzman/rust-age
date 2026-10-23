//! Native Linux read-only preview of original Fallout 1 MAP floor art.
//!
//! Not a game: scripts, lighting, actors, roofs, collision, save files,
//! and world simulation are absent. All assets come from the user installation.

use rust_age::assets::{Bank, GameAssets};
use rust_age::map_preview::FloorPreview;
use rust_age::palette::Palette;
use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use sdl2::pixels::PixelFormatEnum;
use std::env;
use std::io;
use std::path::Path;

const WIDTH: usize = 640;
const HEIGHT: usize = 380;

fn error(message: impl ToString) -> io::Error {
    io::Error::other(message.to_string())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 {
        eprintln!("Usage: rust-age-map-view /path/to/Fallout maps/example.map");
        eprintln!("Display only: Escape closes the window. This is NOT the game.");
        std::process::exit(2);
    }

    let mut assets = GameAssets::open(Path::new(&args[1]))?;
    let palette_bytes = assets
        .read(Bank::Master, "color.pal")?
        .ok_or_else(|| error("original color.pal is missing"))?;
    let palette = Palette::decode(&palette_bytes)?;
    let map_bytes = assets
        .read(Bank::Master, &args[2])?
        .ok_or_else(|| error("original MAP file is missing"))?;
    let preview = FloorPreview::render(&mut assets, &map_bytes, WIDTH, HEIGHT)?;
    let roof_preview =
        FloorPreview::render_layers(&mut assets, &map_bytes, WIDTH, HEIGHT, true)?;

    let sdl = sdl2::init().map_err(error)?;
    let video = sdl.video().map_err(error)?;
    let title = format!(
        "Rust Age - floor diagnostic ({} tiles, {} art)",
        preview.tile_count, preview.unique_art
    );
    let window = video
        .window(&title, 1280, 760)
        .position_centered()
        .resizable()
        .build()
        .map_err(error)?;
    let mut canvas = window.into_canvas().software().build().map_err(error)?;
    canvas
        .set_logical_size(WIDTH as u32, HEIGHT as u32)
        .map_err(error)?;
    let textures = canvas.texture_creator();
    let mut texture = textures
        .create_texture_streaming(PixelFormatEnum::RGB24, WIDTH as u32, HEIGHT as u32)
        .map_err(error)?;
    let mut show_roofs = false;
    let mut events = sdl.event_pump().map_err(error)?;
    loop {
        let scene = if show_roofs { &roof_preview } else { &preview };
        texture
            .with_lock(None, |pixels, pitch| {
                for (y, row) in scene
                    .surface
                    .indices()
                    .as_chunks::<WIDTH>()
                    .0
                    .iter()
                    .enumerate()
                {
                    let destination = &mut pixels[y * pitch..y * pitch + WIDTH * 3];
                    for (x, index) in row.iter().enumerate() {
                        let offset = x * 3;
                        destination[offset..offset + 3]
                            .copy_from_slice(&palette.rgb8_preview(*index));
                    }
                }
            })
            .map_err(error)?;


        canvas.clear();
        canvas.copy(&texture, None, None).map_err(error)?;
        canvas.present();

        // Event-driven presentation, without an artificial emulator tick.
        match events.wait_event() {
            Event::Quit { .. }
            | Event::KeyDown {
                keycode: Some(Keycode::Escape),
                ..
            } => break,
            Event::KeyDown {
                keycode: Some(Keycode::R),
                repeat: false,
                ..
            } => show_roofs = !show_roofs,
            _ => {}
        }
    }
    Ok(())
}
