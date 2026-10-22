//! Local Fallout 1 asset selection and CE-style loose-file overrides.
//!
//! References: CE src/game/gconfig.cc, src/game/game.cc,
//! src/plib/db/db.cc and src/platform_compat.cc at pinned revision.
//! The caller supplies a legitimately obtained Fallout 1 installation.

use crate::dat::DatArchive;
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bank {
    Master,
    Critter,
}

#[derive(Debug)]
pub struct GameAssets {
    root: PathBuf,
    master: DatArchive<File>,
    critter: DatArchive<File>,
}

/// Resolve a relative path case-insensitively, as CE does on Linux.
///
/// Does not allow absolute paths or ".." traversal. This matches the usual
/// installed-game asset path use, not every obscure direct-file CE behavior.
fn resolve_casefold(root: &Path, relative: &str) -> io::Result<Option<PathBuf>> {
    let replaced = relative.replace('\\', "/");
    if replaced.starts_with('/') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "absolute game-data path",
        ));
    }
    let mut path = root.to_path_buf();
    for component in replaced.split('/') {
        if component.is_empty() || component == "." {
            continue;
        }
        if component == ".." || component.contains(':') {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "unsafe game-data path",
            ));
        }
        let directory = match fs::read_dir(&path) {
            Ok(directory) => directory,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let mut found = None;
        for candidate in directory {
            let candidate = candidate?;
            if candidate
                .file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case(component)
            {
                found = Some(candidate.path());
                break;
            }
        }
        match found {
            Some(next) => path = next,
            None => return Ok(None),
        }
    }
    Ok(Some(path))
}

impl GameAssets {
    /// Open original master.dat and critter.dat, keeping CE's two selections
    /// separate. Both banks default to the same "data" loose-file patch dir.
    pub fn open(root: impl Into<PathBuf>) -> io::Result<Self> {
        let root = root.into();
        let missing = |name| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("missing Fallout 1 game file: {name}"),
            )
        };
        let master_path =
            resolve_casefold(&root, "master.dat")?.ok_or_else(|| missing("master.dat"))?;
        let critter_path =
            resolve_casefold(&root, "critter.dat")?.ok_or_else(|| missing("critter.dat"))?;
        let master = DatArchive::open(File::open(master_path)?)?;
        let critter = DatArchive::open(File::open(critter_path)?)?;
        Ok(Self {
            root,
            master,
            critter,
        })
    }

    pub fn entry_count(&self, bank: Bank) -> usize {
        match bank {
            Bank::Master => self.master.entries().count(),
            Bank::Critter => self.critter.entries().count(),
        }
    }

    /// Loose files in data/ override the selected DAT, exactly as CE's
    /// default patch-before-archive lookup order does for normal game paths.
    pub fn read(&mut self, bank: Bank, path: &str) -> io::Result<Option<Vec<u8>>> {
        let root = resolve_casefold(&self.root, "data")?;
        if let Some(root) = root {
            if root.is_dir() {
                if let Some(loose) = resolve_casefold(&root, path)? {
                    if loose.is_file() {
                        // Do not follow symlinks outside the game patch tree.
                        let canonical_root = root.canonicalize()?;
                        let canonical_file = loose.canonicalize()?;
                        if !canonical_file.starts_with(canonical_root) {
                            return Err(io::Error::new(
                                io::ErrorKind::PermissionDenied,
                                "game asset escapes patch directory",
                            ));
                        }
                        return fs::read(loose).map(Some);
                    }
                }
            }
        }

        match bank {
            Bank::Master => self.master.read(path),
            Bank::Critter => self.critter.read(path),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::resolve_casefold;
    use std::fs;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn resolves_windows_separators_and_mixed_case() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("rust-age-casefold-{}-{stamp}", std::process::id()));
        fs::create_dir_all(root.join("DaTa").join("Art")).unwrap();
        fs::write(root.join("DaTa").join("Art").join("TEST.FRM"), b"frame").unwrap();
        let resolved = resolve_casefold(&root, r"data\ART\test.frm")
            .unwrap()
            .unwrap();
        assert_eq!(fs::read(resolved).unwrap(), b"frame");
        assert!(resolve_casefold(&root, "data/missing.frm")
            .unwrap()
            .is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_parent_components_and_absolute_paths() {
        assert!(resolve_casefold(Path::new("."), "../outside").is_err());
        assert!(resolve_casefold(Path::new("."), "/etc/passwd").is_err());
    }
}
