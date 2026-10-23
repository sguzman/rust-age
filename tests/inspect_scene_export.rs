//! Synthetic end-to-end read-only MAP object PPM export.
//! No copyrighted game files or screenshots are stored in this repository.
use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn put(bytes: &mut Vec<u8>, value: i32) {
    bytes.extend_from_slice(&value.to_be_bytes());
}

fn tiny_frm() -> Vec<u8> {
    let mut sprite = Vec::new();
    put(&mut sprite, 4);
    sprite.extend_from_slice(&10_i16.to_be_bytes());
    sprite.extend_from_slice(&0_i16.to_be_bytes());
    sprite.extend_from_slice(&1_i16.to_be_bytes());
    sprite.extend_from_slice(&[0_u8; 24]);
    sprite.extend_from_slice(&[0_u8; 24]);
    put(&mut sprite, 16);
    sprite.extend_from_slice(&2_i16.to_be_bytes());
    sprite.extend_from_slice(&2_i16.to_be_bytes());
    put(&mut sprite, 4);
    sprite.extend_from_slice(&[0_u8; 4]);
    sprite.extend_from_slice(&[7, 0, 0, 9]);
    sprite
}

fn map_with_one_object() -> Vec<u8> {
    let mut map = Vec::new();
    put(&mut map, 19);
    map.extend_from_slice(b"TEST.MAP\0\0\0\0\0\0\0\0");
    for field in [20099, 0, 0, 0, -1, 4 | 8, 0, 0, 0, 0] {
        put(&mut map, field);
    }
    map.extend_from_slice(&[0_u8; 44 * 4]);
    for _ in 0..10_000 {
        map.extend_from_slice(&0x0001_0001_u32.to_be_bytes());
    }
    map.extend_from_slice(&[0_u8; 5 * 4]); // five empty script tables
    put(&mut map, 1); // total top-level objects
    put(&mut map, 1); // elevation zero objects
    for value in [
        42, 20099, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x0400_0001, 0, 0, 0, 0, -1, 0,
    ] {
        put(&mut map, value);
    }
    for value in [0, 0, 0, 0, 0, 0] {
        // inventory length/capacity/pointer, flags, elevation1, elevation2
        put(&mut map, value);
    }
    map
}

#[test]
fn cli_exports_static_item_sprite_into_original_640x380_scene() {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "rust-age-scene-export-{}-{stamp}",
        std::process::id()
    ));
    let art_tiles = root.join("data").join("art").join("tiles");
    let art_items = root.join("data").join("art").join("items");
    let maps = root.join("data").join("maps");
    fs::create_dir_all(&art_tiles).unwrap();
    fs::create_dir_all(&art_items).unwrap();
    fs::create_dir_all(&maps).unwrap();
    fs::write(root.join("master.dat"), [0_u8; 16]).unwrap();
    fs::write(root.join("critter.dat"), [0_u8; 16]).unwrap();
    fs::write(art_tiles.join("tiles.lst"), b"TEST.FRM\nEMPTY.FRM\n").unwrap();
    fs::write(art_items.join("items.lst"), b"ICON.FRM\n").unwrap();
    fs::write(art_items.join("ICON.FRM"), tiny_frm()).unwrap();
    fs::write(maps.join("test.map"), map_with_one_object()).unwrap();
    let mut palette = vec![0_u8; 768 + 32768];
    palette[7 * 3..7 * 3 + 3].copy_from_slice(&[63, 0, 0]);
    palette[9 * 3..9 * 3 + 3].copy_from_slice(&[0, 63, 0]);
    fs::write(root.join("data").join("color.pal"), palette).unwrap();
    let output_path = root.join("preview.ppm");

    let output = Command::new(env!("CARGO_BIN_EXE_rust-age"))
        .arg("--export-map-object-preview")
        .arg(&root)
        .arg("maps/test.map")
        .arg(&output_path)
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Static scene sprites: 1 drawn"), "{stdout}");
    let ppm = fs::read(output_path).unwrap();
    let header = b"P6\n640 380\n255\n";
    assert!(ppm.starts_with(header));
    let data = &ppm[header.len()..];
    assert_eq!(&data[(189 * 640 + 319) * 3..(189 * 640 + 319) * 3 + 3], &[255, 0, 0]);
    assert_eq!(&data[(190 * 640 + 320) * 3..(190 * 640 + 320) * 3 + 3], &[0, 255, 0]);
    fs::remove_dir_all(root).unwrap();
}
