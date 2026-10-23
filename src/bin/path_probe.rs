//! Numeric-only differential probe against pinned CE C++ path search.
//! No Fallout game data is loaded.
use rust_age::hex::HexViewport;
use rust_age::pathfinding::make_path;

fn passable(tile: i32, mode: i32) -> bool {
    match mode {
        0 => true,
        1 => tile != 20098 && tile != 20097 && tile != 19898,
        _ => false,
    }
}

fn main() {
    let view = HexViewport::new(200, 200, 640, 380, 20099).unwrap();
    let cases = [
        (20099, 20098, 0, false, false),
        (20099, 20099, 0, true, false),
        (20099, 20297, 1, true, false),
        (20099, 30000, 0, true, false),
        (20099, 20297, 2, true, false),
        (20099, 20098, 2, true, true),
        (20099, 20098, 2, true, false),
        (20099, 20100, 0, false, false),
        (20098, 19999, 1, true, false),
        (30000, 30099, 0, false, false),
    ];
    for (from, to, mode, not_combat, check_dest) in cases {
        let result = make_path(&view, from, to, not_combat, check_dest, |tile| {
            passable(tile, mode)
        });
        let steps = match result {
            None => "none".to_string(),
            Some(rotations) if rotations.is_empty() => "-".to_string(),
            Some(rotations) => rotations
                .into_iter()
                .map(|direction| direction.to_string())
                .collect::<Vec<_>>()
                .join(","),
        };
        println!(
            "P {from} {to} {mode} {} {} {steps}",
            u8::from(not_combat),
            u8::from(check_dest)
        );
    }
}
