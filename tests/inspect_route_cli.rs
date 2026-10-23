//! Read-only Linux CLI smoke test with invented MAP bytes. No game data.
use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn synthetic_map() -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(&19_i32.to_be_bytes());
    data.extend_from_slice(b"TEST.MAP\0\0\0\0\0\0\0\0");
    for value in [20099_i32, 0, 2, 0, -1, 2 | 4 | 8, 0, 0, 0, 0] {
        data.extend_from_slice(&value.to_be_bytes());
    }
    data.extend_from_slice(&[0_u8; 44 * 4]);
    data.extend_from_slice(&[0_u8; 5 * 4]); // five empty script tables
    data.extend_from_slice(&[0_u8; 4 * 4]); // object total and three elevations
    data
}

#[test]
fn cli_reads_an_original_format_map_and_reports_path_without_moving_actor() {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root =
        std::env::temp_dir().join(format!("rust-age-route-cli-{}-{stamp}", std::process::id()));
    fs::create_dir_all(root.join("data").join("maps")).unwrap();
    fs::write(root.join("master.dat"), [0_u8; 16]).unwrap();
    fs::write(root.join("critter.dat"), [0_u8; 16]).unwrap();
    fs::write(
        root.join("data").join("maps").join("test.map"),
        synthetic_map(),
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_rust-age"))
        .arg("--inspect-map-route")
        .arg(&root)
        .arg("maps/test.map")
        .arg("20099")
        .arg("20098")
        .output()
        .unwrap();

    fs::remove_dir_all(root).unwrap();
    assert!(output.status.success(), "{:?}", output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Steps: 1"), "{stdout}");
    assert!(stdout.contains("Rotations: 1"), "{stdout}");
    assert!(stdout.contains("No actor moved"), "{stdout}");
}
