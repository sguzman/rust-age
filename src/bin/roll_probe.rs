//! Original Fallout 1 CE fixed-seed RNG differential probe.
use rust_age::random_roll::FalloutRng;
use std::error::Error;

fn run(line: &str) -> Result<String, Box<dyn Error>> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() != 6 {
        return Err("invalid CE roll case".into());
    }
    let mut rng = FalloutRng::from_seed(parts[2].parse()?);
    match parts[1] {
        "S" => {
            let low = parts[3].parse()?;
            let high = parts[4].parse()?;
            let count = parts[5].parse::<usize>()?;
            let rolls = (0..count)
                .map(|_| rng.between(low, high).map(|value| value.to_string()))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rolls.join(","))
        }
        "C" => {
            let difficulty = parts[3].parse()?;
            let modifier = parts[4].parse()?;
            let ticks = parts[5].parse()?;
            let (outcome, delta) = rng.check(difficulty, modifier, ticks)?;
            Ok(format!("D:{delta},R:{}", outcome as i32))
        }
        _ => Err("unknown CE roll probe mode".into()),
    }
}

fn main() {
    for line in include_str!("../../tests/roll_cases.txt").lines() {
        let Some(name) = line.split_whitespace().next() else {
            continue;
        };
        let result = run(line).unwrap_or_else(|_| "ERR".to_owned());
        println!("{name} {result}");
    }
}
