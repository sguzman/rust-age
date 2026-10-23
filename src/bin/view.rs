//! Read-only native SDL2 preview of user-owned Fallout 1 FRM sprites.
//!
//! This is a rendering diagnostic, not the playable Fallout engine.
//! SDL2 chooses Wayland/X11 according to the local Linux installation.

use rust_age::animation::AnimationTimeline;
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
use std::time::Instant;

const WIDTH: usize = 640;
const HEIGHT: usize = 480;

fn error(error: impl ToString) -> io::Error {
    io::Error::other(error.to_string())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<String> = env::args().collect();
    if arguments.len() != 4 {
        eprintln!("Usage: rust-age-view /path/to/Fallout <master|critter> <FRM path>");
        eprintln!("Controls: Left/Right = frame, Up/Down = direction, Space = play/pause, Escape = quit.");
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
    let timeline = AnimationTimeline::from_frm(&frm)?;

    let mut direction = 0usize;
    let mut frame_index = 0usize;
    let mut base_frame = 0usize;
    let mut playing = false;
    let mut animation_origin = Instant::now();
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
        if playing {
            frame_index = (base_frame + timeline.frame_at(animation_origin.elapsed())) % frames.len();
        } else {
            frame_index %= frames.len();
        }
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

        // The animation frame depends only on monotonic elapsed time, not
        // the number of draw calls. SDL waits for input or the next frame.
        let event = if playing {
            events.wait_event_timeout((1000 / timeline.frames_per_second()).max(1))
        } else {
            Some(events.wait_event())
        };
        match event {
            Some(Event::Quit { .. })
            | Some(Event::KeyDown {
                keycode: Some(Keycode::Escape),
                ..
            }) => break,
            Some(Event::KeyDown {
                keycode: Some(Keycode::Space),
                repeat: false,
                ..
            }) => {
                if playing {
                    playing = false;
                } else {
                    base_frame = frame_index;
                    animation_origin = Instant::now();
                    playing = true;
                }
            },
            Some(Event::KeyDown {
                keycode: Some(Keycode::Right),
                repeat: false,
                ..
            }) => {
                playing = false;
                frame_index = (frame_index + 1) % frames.len();
            },
            Some(Event::KeyDown {
                keycode: Some(Keycode::Left),
                repeat: false,
                ..
            }) => {
                playing = false;
                frame_index = (frame_index + frames.len() - 1) % frames.len();
            },
            Some(Event::KeyDown {
                keycode: Some(Keycode::Up),
                repeat: false,
                ..
            }) => {
                playing = false;
                direction = (direction + 1) % 6;
                frame_index = 0;
            }
            Some(Event::KeyDown {
                keycode: Some(Keycode::Down),
                repeat: false,
                ..
            }) => {
                playing = false;
                direction = (direction + 5) % 6;
                frame_index = 0;
            }
            _ => {}
        }
    }
    Ok(())
}
