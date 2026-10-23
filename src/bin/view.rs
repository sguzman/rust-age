//! Read-only native SDL2 preview of user-owned Fallout 1 FRM sprites.
//!
//! This is a rendering diagnostic, not the playable Fallout engine.
//! SDL2 chooses Wayland/X11 according to the local Linux installation.

use rust_age::assets::{Bank, GameAssets};
use rust_age::framebuffer::IndexedSurface;
use rust_age::frm::Frm;
use rust_age::palette::Palette;
use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use sdl2::pixels::PixelFormatEnum;
use std::env;
use std::io;
use std::path::Path;

const WIDTH: usize = 640;
const HEIGHT: usize = 480;

fn error(error: impl ToString) -> io::Error {
    io::Error::other(error.to_string())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<String> = env::args().collect();
    if arguments.len() != 4 {
        eprintln!("Usage: rust-age-view /path/to/Fallout <master|critter> <FRM path>");
        eprintln!("Controls: Left/Right = frame, Up/Down = direction, Escape = quit.");
        std::process::exit(2);
    }
    let bank = match arguments[2].as_str() {
        "master" => Bank::Master,
        "critter" => Bank::Critter,
        _ => return Err(error("Bank must be master or critter").into()),
    };
    let mut assets = GameAssets::open(Path::new(&arguments[1]))?;
    let palette_bytes = assets
        .read(Bank::Master, "color.pal")?
        .ok_or_else(|| error("color.pal missing from original game data"))?;
    let palette = Palette::decode(&palette_bytes)?;
    let frm_bytes = assets
        .read(bank, &arguments[3])?
        .ok_or_else(|| error("sprite not found in original game installation"))?;
    let frm = Frm::decode(&frm_bytes)?;
    if frm.frames.iter().all(Vec::is_empty) {
        return Err(error("FRM contains no frames").into());
    }

    let sdl = sdl2::init().map_err(error)?;
    let video = sdl.video().map_err(error)?;
    let window = video
        .window("Rust Age - original Fallout 1 sprite preview", 960, 720)
        .position_centered()
        .resizable()
        .build()
        .map_err(error)?;
    let mut canvas = window.into_canvas().software().build().map_err(error)?;
    canvas
        .set_logical_size(WIDTH as u32, HEIGHT as u32)
        .map_err(error)?;
    let texture_creator = canvas.texture_creator();
    let mut texture = texture_creator
        .create_texture_streaming(PixelFormatEnum::RGB24, WIDTH as u32, HEIGHT as u32)
        .map_err(error)?;
    let mut events = sdl.event_pump().map_err(error)?;

    let mut direction = 0usize;
    let mut frame_index = 0usize;
    loop {
        let frames = &frm.frames[direction];
        if frames.is_empty() {
            direction = frm
                .frames
                .iter()
                .position(|rotation| !rotation.is_empty())
                .ok_or_else(|| error("No usable FRM direction"))?;
            frame_index = 0;
        }
        let frames = &frm.frames[direction];
        frame_index %= frames.len();
        let frame = &frames[frame_index];
        let mut indexed = IndexedSurface::new(WIDTH, HEIGHT)?;
        indexed.blit_frame(
            frame,
            (WIDTH as i32 - i32::from(frame.width)) / 2,
            (HEIGHT as i32 - i32::from(frame.height)) / 2,
            true,
        )?;
        texture
            .with_lock(None, |pixels, pitch| {
                for (y, src) in indexed.indices().as_chunks::<WIDTH>().0.iter().enumerate() {
                    let row = &mut pixels[y * pitch..y * pitch + WIDTH * 3];
                    for (x, index) in src.iter().enumerate() {
                        let offset = x * 3;
                        row[offset..offset + 3].copy_from_slice(&palette.rgb8_preview(*index));
                    }
                }
            })
            .map_err(error)?;
        canvas.clear();
        canvas.copy(&texture, None, None).map_err(error)?;
        canvas.present();

        // No background redraw: event driven until the user moves through
        // the original animation frames or SDL requests a window repaint.
        match events.wait_event() {
            Event::Quit { .. }
            | Event::KeyDown {
                keycode: Some(Keycode::Escape),
                ..
            } => break,
            Event::KeyDown {
                keycode: Some(Keycode::Right),
                repeat: false,
                ..
            } => frame_index = (frame_index + 1) % frames.len(),
            Event::KeyDown {
                keycode: Some(Keycode::Left),
                repeat: false,
                ..
            } => frame_index = (frame_index + frames.len() - 1) % frames.len(),
            Event::KeyDown {
                keycode: Some(Keycode::Up),
                repeat: false,
                ..
            } => {
                direction = (direction + 1) % 6;
                frame_index = 0;
            }
            Event::KeyDown {
                keycode: Some(Keycode::Down),
                repeat: false,
                ..
            } => {
                direction = (direction + 5) % 6;
                frame_index = 0;
            }
            _ => {}
        }
    }
    Ok(())
}
