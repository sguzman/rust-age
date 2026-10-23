//! Linux synthetic MAP/script-list binding integration without game assets.
use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn put(bytes: &mut Vec<u8>, value: i32) {
    bytes.extend_from_slice(&value.to_be_bytes());
}

fn synthetic_map() -> Vec<u8> {
    let mut bytes = Vec::new();
    put(&mut bytes, 19);
    bytes.extend_from_slice(b"TEST.MAP\0\0\0\0\0\0\0\0");
    for value in [20099, 0, 0, 0, -1, 2 | 4 | 8, 0, 0, 0, 0] {
        put(&mut bytes, value);
    }
    bytes.extend_from_slice(&[0_u8; 44 * 4]);
    put(&mut bytes, 1); // one script in the system category
    for ordinal in 0..16 {
        for value in [
            if ordinal == 0 { 1 } else { 0 },
            -1,
            0,
            if ordinal == 0 { 0 } else { -1 },
            -1,
            -1,
            -1,
            0,
        ] {
            put(&mut bytes, value);
        }
        for _ in 0..8 {
            put(&mut bytes, 0);
        }
    }
    put(&mut bytes, 1); // populated slots
    put(&mut bytes, 0); // legacy next pointer ignored
    for _ in 1..5 {
        put(&mut bytes, 0); // remaining script categories empty
    }
    for _ in 0..4 {
        put(&mut bytes, 0); // object total and three elevations
    }
    bytes
}

#[test]
fn reports_ce_map_script_index_and_run_flags_without_executing() {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "rust-age-map-script-{}-{stamp}",
        std::process::id()
    ));
    let maps = root.join("data").join("maps");
    let scripts = root.join("data").join("scripts");
    fs::create_dir_all(&maps).unwrap();
    fs::create_dir_all(&scripts).unwrap();
    fs::write(root.join("master.dat"), [0_u8; 16]).unwrap();
    fs::write(root.join("critter.dat"), [0_u8; 16]).unwrap();
    fs::write(maps.join("test.map"), synthetic_map()).unwrap();
    fs::write(
        scripts.join("scripts.lst"),
        b"START.ssl # map_init local_vars=3\n",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_rust-age"))
        .arg("--inspect-map-scripts")
        .arg(&root)
        .arg("maps/test.map")
        .output()
        .unwrap();
    fs::remove_dir_all(root).unwrap();
    assert!(output.status.success(), "{:?}", output);
    let output = String::from_utf8(output.stdout).unwrap();
    assert!(
        output.contains("Original MAP script bindings: 1"),
        "{output}"
    );
    assert!(
        output.contains("script index 0 -> scripts/START.int"),
        "{output}"
    );
    assert!(
        output.contains("map_init=true map_exit=false list_local_vars=Some(3)"),
        "{output}"
    );
    assert!(
        output.contains("no Fallout script has executed"),
        "{output}"
    );
}
