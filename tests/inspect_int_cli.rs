//! Synthetic Linux CLI .INT inspection test; never includes game scripts.
use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn put(bytes: &mut Vec<u8>, value: i32) {
    bytes.extend_from_slice(&value.to_be_bytes());
}

#[test]
fn linux_cli_lists_original_ce_procedure_metadata_without_executing() {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "rust-age-int-inspect-{}-{stamp}",
        std::process::id()
    ));
    let directory = root.join("data").join("scripts");
    fs::create_dir_all(&directory).unwrap();
    fs::write(root.join("master.dat"), [0_u8; 16]).unwrap();
    fs::write(root.join("critter.dat"), [0_u8; 16]).unwrap();
    let mut file = vec![0_u8; 42];
    put(&mut file, 1);
    for word in [4_i32, 0, 0, 0, 123, 2] {
        put(&mut file, word);
    }
    put(&mut file, 5);
    file.extend_from_slice(b"main\0");
    put(&mut file, 1);
    file.push(0);
    fs::write(directory.join("test.int"), file).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_rust-age"))
        .arg("--inspect-int")
        .arg(&root)
        .arg("scripts/test.int")
        .output()
        .unwrap();
    fs::remove_dir_all(root).unwrap();
    assert!(output.status.success(), "{:?}", output);
    let output = String::from_utf8(output.stdout).unwrap();
    assert!(output.contains("Original CE procedures: 1"), "{output}");
    assert!(output.contains("[0] main (offset 123, arguments 2)"), "{output}");
    assert!(output.contains("no bytecode executed"), "{output}");
}
