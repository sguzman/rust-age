//! Synthetic Linux native game-global inspection without original game data.
use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn inspect_original_game_global_defaults_and_missing_assets() {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "rust-age-inspect-game-global-{}-{stamp}",
        std::process::id()
    ));
    fs::create_dir_all(root.join("data")).unwrap();
    fs::write(root.join("master.dat"), [0_u8; 16]).unwrap();
    fs::write(root.join("critter.dat"), [0_u8; 16]).unwrap();

    let absent = Command::new(env!("CARGO_BIN_EXE_rust-age"))
        .arg("--inspect-game-global")
        .arg(&root)
        .arg("0")
        .output()
        .unwrap();
    assert!(!absent.status.success());

    fs::write(
        root.join("data").join("VAULT13.GAM"),
        b"GAME_GLOBAL_VARS:\nGVAR_ZERO=8\nGVAR_ONE=-12\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_rust-age"))
        .arg("--inspect-game-global")
        .arg(&root)
        .arg("1")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("GAME_GLOBAL_VARS defaults: 2"), "{stdout}");
    assert!(stdout.contains("index 1: -12"), "{stdout}");
    assert!(stdout.contains("status: present"), "{stdout}");
    assert!(
        stdout.contains("no Fallout script has executed"),
        "{stdout}"
    );

    let invalid = Command::new(env!("CARGO_BIN_EXE_rust-age"))
        .arg("--inspect-game-global")
        .arg(&root)
        .arg("-1")
        .output()
        .unwrap();
    assert!(invalid.status.success(), "{invalid:?}");
    let stderr = String::from_utf8(invalid.stdout).unwrap();
    assert!(stderr.contains("index -1: 0"), "{stderr}");
    assert!(stderr.contains("status: out of range"), "{stderr}");

    fs::remove_dir_all(root).unwrap();
}
