//! Rust half of the synthetic CE vault13.gam defaults differential oracle.
use rust_age::game_globals::GameGlobalState;
use std::error::Error;

fn unescape(encoded: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let mut chars = encoded.bytes();
    while let Some(ch) = chars.next() {
        if ch == b'\\' {
            let next = chars.next().unwrap_or(ch);
            out.push(match next {
                b'n' => b'\n',
                b'r' => b'\r',
                b't' => b'\t',
                other => other,
            });
        } else {
            out.push(ch);
        }
    }
    out
}
fn probe(encoded: &str) -> Result<String, Box<dyn Error>> {
    let bytes = unescape(encoded);
    let vars = GameGlobalState::decode_vault13_defaults(&bytes)?;
    Ok(format!(
        "{}:{}",
        vars.values().len(),
        vars.values()
            .iter()
            .map(i32::to_string)
            .collect::<Vec<_>>()
            .join(",")
    ))
}
fn main() {
    for line in include_str!("../../tests/game_global_defaults_cases.txt").lines() {
        if let Some((name, input)) = line.split_once('|') {
            let result = probe(input).unwrap_or_else(|_| "ERR".to_owned());
            println!("{name} {result}");
        }
    }
}
