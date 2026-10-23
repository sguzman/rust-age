//! The user's original script list/bytecode are read from their installation.
//! Test data below is invented and cannot run real Fallout game scripts.
use rust_age::assets::GameAssets;
use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn script_index_lookup_reads_cached_ce_list_and_decodes_synthetic_int() {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "rust-age-script-list-{}-{stamp}",
        std::process::id()
    ));
    let scripts = root.join("data").join("scripts");
    fs::create_dir_all(&scripts).unwrap();
    fs::write(root.join("master.dat"), [0_u8; 16]).unwrap();
    fs::write(root.join("critter.dat"), [0_u8; 16]).unwrap();
    fs::write(
        scripts.join("scripts.lst"),
        b"FIRST.ssl # map_init\nSECOND.ssl\n",
    )
    .unwrap();
    let mut int_bytes = vec![0_u8; 42];
    let names: [&[u8]; 3] = [b"main", b"START", b"Map_Exit_P_Proc"];
    int_bytes.extend_from_slice(&(names.len() as i32).to_be_bytes());
    let mut identifiers = Vec::new();
    for name in names {
        for field in [4 + identifiers.len() as i32, 0, 0, 0, 0, 0] {
            int_bytes.extend_from_slice(&field.to_be_bytes());
        }
        identifiers.extend_from_slice(name);
        identifiers.push(0);
    }
    int_bytes.extend_from_slice(&(identifiers.len() as i32).to_be_bytes());
    int_bytes.extend_from_slice(&identifiers);
    int_bytes.extend_from_slice(&0_i32.to_be_bytes()); // empty static string pool
    fs::write(scripts.join("FIRST.int"), int_bytes).unwrap();
    let mut assets = GameAssets::open(&root).unwrap();
    assert_eq!(
        assets.script_path_by_index(0).unwrap().as_deref(),
        Some("scripts/FIRST.int")
    );
    assert_eq!(
        assets.script_path_by_index(0x0100_0000).unwrap().as_deref(),
        Some("scripts/FIRST.int")
    );
    assert_eq!(
        assets.script_path_by_index(1).unwrap().as_deref(),
        Some("scripts/SECOND.int")
    );
    assert_eq!(
        assets
            .script_program_by_index(0)
            .unwrap()
            .unwrap()
            .procedures()
            .len(),
        3
    );
    assert!(assets.script_program_by_index(1).unwrap().is_none());
    assert_eq!(assets.script_path_by_index(2).unwrap(), None);
    let output = Command::new(env!("CARGO_BIN_EXE_rust-age"))
        .arg("--inspect-script-index")
        .arg(&root)
        .arg("0")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("CE built-in action bindings: 2 of 24"),
        "{stdout}"
    );
    assert!(
        stdout.contains("action [1] start -> procedure [1]"),
        "{stdout}"
    );
    assert!(
        stdout.contains("action [16] map_exit_p_proc -> procedure [2]"),
        "{stdout}"
    );
    assert!(stdout.contains("no script has been executed"), "{stdout}");
    fs::write(scripts.join("scripts.lst"), b"OTHER.ssl\nSECOND.ssl\n").unwrap();
    assert_eq!(
        assets.script_path_by_index(0).unwrap().as_deref(),
        Some("scripts/FIRST.int")
    );
    assets.clear_lookup_cache();
    assert_eq!(
        assets.script_path_by_index(0).unwrap().as_deref(),
        Some("scripts/OTHER.int")
    );
    fs::remove_dir_all(root).unwrap();
}
