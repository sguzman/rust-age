//! Numeric-only Rust output for the pinned CE C++ spatial oracle.
//! This binary does not load or redistribute any original Fallout assets.

use rust_age::hex::HexViewport;

fn main() {
    for center in [20099, 20098, 20100, 30000, 1, 39998] {
        let viewport = HexViewport::new(200, 200, 640, 380, center).unwrap();
        for tile in (0..40_000).step_by(137) {
            let (x, y) = viewport.tile_coord(tile).unwrap();
            println!("T {center} {tile} {x} {y}");
        }
        for square in (0..10_000).step_by(197) {
            let (x, y) = viewport.square_coord(square, false).unwrap();
            let (_, roof_y) = viewport.square_coord(square, true).unwrap();
            println!("S {center} {square} {x} {y} {roof_y}");
        }
    }
}
