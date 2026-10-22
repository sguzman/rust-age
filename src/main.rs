use std::env;
use std::path::Path;

fn usage() {
    println!("Rust Age - Fallout 1 CE Rust port (not yet playable)");
    println!("Usage: rust-age --check-installation /path/to/Fallout");
    println!("       rust-age --help");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    match args.as_slice() {
        [_] => usage(),
        [_, flag] if flag == "--help" || flag == "-h" => usage(),
        [_, flag] if flag == "--version" => println!("rust-age {}", env!("CARGO_PKG_VERSION")),
        [_, flag, directory] if flag == "--check-installation" => {
            let root = Path::new(directory);
            if !root.is_dir() {
                eprintln!("Not a directory: {}", root.display());
                std::process::exit(2);
            }
            let required = ["master.dat", "critter.dat", "data"];
            let missing: Vec<_> = required.into_iter().filter(|name| {
                let path = root.join(name);
                if *name == "data" { !path.is_dir() } else { !path.is_file() }
            }).collect();
            if missing.is_empty() {
                println!("Fallout 1 asset paths present in {}", root.display());
                println!("Archive contents not validated; engine not yet playable.");
            } else {
                eprintln!("Missing game assets: {}", missing.join(", "));
                eprintln!("Use a legitimately acquired Fallout 1 installation. Names are case-sensitive on Linux.");
                std::process::exit(2);
            }
        }
        _ => {
            usage();
            std::process::exit(2);
        }
    }
}
