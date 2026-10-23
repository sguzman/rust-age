//! Local Fallout 1 asset selection and CE-style loose-file overrides.
//!
//! References: CE src/game/gconfig.cc, src/game/game.cc,
//! src/plib/db/db.cc and src/platform_compat.cc at pinned revision.
//! The caller supplies a legitimately obtained Fallout 1 installation.

use crate::art_fid::ArtFid;
use crate::art_list::ArtList;
use crate::config::Config;
use crate::dat::DatArchive;
use crate::message::Catalog;
use crate::prototype::Prototype;
use crate::prototype_list::PrototypeList;
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
    master_patches: String,
    critter_patches: String,
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
    /// Open Fallout 1's two databases. Apply CE defaults, then allow
    /// original fallout.cfg [system] asset-path overrides.
    pub fn open(root: impl Into<PathBuf>) -> io::Result<Self> {
        let root = root.into();
        let mut settings = Config::fallout1_defaults();
        if let Some(path) = resolve_casefold(&root, "fallout.cfg")? {
            let bytes = fs::read(path)?;
            settings.apply(&Config::parse(&String::from_utf8_lossy(&bytes)));
        }

        let master_name = settings.get("system", "master_dat").unwrap_or("master.dat");
        let critter_name = settings
            .get("system", "critter_dat")
            .unwrap_or("critter.dat");
        let missing = |name| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("missing Fallout 1 game file: {name}"),
            )
        };
        let master_path =
            resolve_casefold(&root, master_name)?.ok_or_else(|| missing(master_name))?;
        let critter_path =
            resolve_casefold(&root, critter_name)?.ok_or_else(|| missing(critter_name))?;
        let master = DatArchive::open(File::open(master_path)?)?;
        let critter = DatArchive::open(File::open(critter_path)?)?;
        Ok(Self {
            root,
            master,
            critter,
            master_patches: settings
                .get("system", "master_patches")
                .unwrap_or("data")
                .to_owned(),
            critter_patches: settings
                .get("system", "critter_patches")
                .unwrap_or("data")
                .to_owned(),
        })
    }

    /// Read the original localized MSG catalog under text/<language>/,
    /// using the master database's configured patch/archive resolution.
    pub fn message_catalog(&mut self, language: &str, path: &str) -> io::Result<Option<Catalog>> {
        let file = format!("text/{language}/{path}");
        match self.read(Bank::Master, &file)? {
            Some(bytes) => Catalog::decode(&bytes).map(Some),
            None => Ok(None),
        }
    }

    /// Resolve CE art_get_name for static art, critters and talking heads.
    /// Critters live in critter.dat; other types live in master.dat.
    /// Special death-frame aliases require an explicit original critters.lst
    /// alias entry rather than guessing a runtime-controlled fallback index.
    pub fn art_path_by_fid(&mut self, raw_fid: u32) -> io::Result<Option<(Bank, String)>> {
        let fid = ArtFid(raw_fid);
        let directory = fid.directory()?;
        let bank = if fid.category() == 1 { Bank::Critter } else { Bank::Master };
        let list_path = format!("art/{directory}/{directory}.lst");
        let Some(data) = self.read(bank, &list_path)? else {
            return Ok(None);
        };
        let names = ArtList::parse(&data);
        let mut index = fid.index();
        if fid.needs_critter_alias() {
            let line = data
                .split_inclusive(|&b| b == b'\n')
                .nth(index)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing critter alias row"))?;
            let raw_alias = line
                .split(|&b| b == b',')
                .nth(1)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing critter alias"))?;
            let alias = std::str::from_utf8(raw_alias)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "non-UTF8 critter alias"))?
                .trim()
                .parse::<usize>()
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid critter alias"))?;
            index = alias;
        }
        let Some(name) = names.file_name(index)? else {
            return Ok(None);
        };
        let filename = fid.filename(name)?;
        Ok(Some((bank, format!("art/{directory}/{filename}"))))
    }

    /// Compatibility wrapper for callers expecting direct (master-bank) art.
    pub fn direct_art_path(&mut self, fid: u32) -> io::Result<Option<String>> {
        ArtFid(fid).direct_category()?;
        self.art_path_by_fid(fid)
            .map(|resolved| resolved.map(|(_, path)| path))
    }

    /// Resolve CE's original tile art IDs to the filenames in tiles.lst.
    /// The list itself is loaded from user-owned master DAT/patch content.
    pub fn tile_art_list(&mut self) -> io::Result<ArtList> {
        let bytes = self
            .read(Bank::Master, "art/tiles/tiles.lst")?
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotFound, "art/tiles/tiles.lst missing")
            })?;
        Ok(ArtList::parse(&bytes))
    }

    /// Resolve original CE's one-based prototype list by PID type and ID.
    /// Read-only lookup: does not instantiate gameplay objects.
    pub fn prototype_by_pid(&mut self, pid: u32) -> io::Result<Option<Prototype>> {
        let directory = match pid >> 24 {
            0 => "items",
            1 => "critters",
            2 => "scenery",
            3 => "walls",
            4 => "tiles",
            5 => "misc",
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "unsupported PID kind",
                ))
            }
        };
        let index = pid & 0x00FF_FFFF;
        if index == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "prototype PID index is zero",
            ));
        }
        let list_path = format!("proto/{directory}/{directory}.lst");
        let Some(list_bytes) = self.read(Bank::Master, &list_path)? else {
            return Ok(None);
        };
        let list = PrototypeList::parse(&list_bytes);
        let Some(file_name) = list.get(index)? else {
            return Ok(None);
        };
        let path = format!("proto/{directory}/{file_name}");
        let Some(data) = self.read(Bank::Master, &path)? else {
            return Ok(None);
        };
        let proto = Prototype::decode(&data)?;
        if proto.pid != pid {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "prototype PID differs from requested list ID",
            ));
        }
        Ok(Some(proto))
    }

    pub fn entry_count(&self, bank: Bank) -> usize {
        match bank {
            Bank::Master => self.master.entries().count(),
            Bank::Critter => self.critter.entries().count(),
        }
    }

    /// Prefer the selected bank's configured loose-file patch directory.
    pub fn read(&mut self, bank: Bank, path: &str) -> io::Result<Option<Vec<u8>>> {
        let patch_dir = match bank {
            Bank::Master => &self.master_patches,
            Bank::Critter => &self.critter_patches,
        };
        if !patch_dir.is_empty() {
            if let Some(root) = resolve_casefold(&self.root, patch_dir)? {
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
        }

        match bank {
            Bank::Master => self.master.read(path),
            Bank::Critter => self.critter.read(path),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{resolve_casefold, Bank, GameAssets};
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
    #[test]
    fn applies_separate_master_and_critter_patch_paths_from_fallout_cfg() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("rust-age-config-{}-{stamp}", std::process::id()));
        fs::create_dir_all(root.join("Master_Patches").join("Art")).unwrap();
        fs::create_dir_all(root.join("Critter_Patches").join("Art")).unwrap();
        // Empty, syntactically valid synthetic DAT indexes.
        let empty_dat = [0_u8; 16];
        fs::write(root.join("MASTER.DAT"), empty_dat).unwrap();
        fs::write(root.join("CRITTER.DAT"), empty_dat).unwrap();
        fs::write(
            root.join("FALLOUT.CFG"),
            b"[system]\nmaster_dat=master.dat\ncritter_dat=critter.dat\nmaster_patches=MASTER_PATCHES\ncritter_patches=critter_patches\n",
        )
        .unwrap();
        fs::write(
            root.join("Master_Patches").join("Art").join("thing.frm"),
            b"master",
        )
        .unwrap();
        fs::write(
            root.join("Critter_Patches").join("Art").join("thing.frm"),
            b"critter",
        )
        .unwrap();
        let mut assets = GameAssets::open(&root).unwrap();
        assert_eq!(
            assets.read(Bank::Master, r"art\\THING.frm").unwrap(),
            Some(b"master".to_vec())
        );
        assert_eq!(
            assets.read(Bank::Critter, "ART/thing.frm").unwrap(),
            Some(b"critter".to_vec())
        );
        fs::create_dir_all(root.join("Master_Patches").join("TEXT").join("English")).unwrap();
        fs::write(
            root.join("Master_Patches")
                .join("TEXT")
                .join("English")
                .join("QUEST.MSG"),
            b"{10}{}{Water chip}",
        )
        .unwrap();
        let catalog = assets
            .message_catalog("english", "quest.msg")
            .unwrap()
            .unwrap();
        assert_eq!(catalog.get(10).unwrap().text, b"Water chip");
        fs::create_dir_all(root.join("Master_Patches").join("Art").join("Tiles")).unwrap();
        fs::write(
            root.join("Master_Patches")
                .join("Art")
                .join("Tiles")
                .join("Tiles.LST"),
            b"grid001.frm\nother.frm\n",
        )
        .unwrap();
        let tiles = assets.tile_art_list().unwrap();
        assert_eq!(tiles.file_name(0).unwrap(), Some("grid001.frm"));
        assert_eq!(tiles.file_name(1).unwrap(), Some("other.frm"));
        assert_eq!(
            assets.direct_art_path(0x0400_0001).unwrap(),
            Some("art/tiles/other.frm".to_owned())
        );
        assert!(assets.direct_art_path(0x0400_0002).unwrap().is_none());
        assert!(assets.direct_art_path(0x0100_0000).is_err());
        fs::create_dir_all(root.join("Critter_Patches").join("Art").join("Critters")).unwrap();
        fs::create_dir_all(root.join("Master_Patches").join("Art").join("Heads")).unwrap();
        fs::write(
            root.join("Critter_Patches")
                .join("Art")
                .join("Critters")
                .join("critters.lst"),
            b"hmjmps,1\nhfjmps\n",
        )
        .unwrap();
        fs::write(
            root.join("Master_Patches")
                .join("Art")
                .join("Heads")
                .join("heads.lst"),
            b"VREDFL\n",
        )
        .unwrap();
        assert_eq!(
            assets.art_path_by_fid(0x0100_0000).unwrap(),
            Some((Bank::Critter, "art/critters/hmjmpsaa.frm".to_string()))
        );
        assert_eq!(
            assets.art_path_by_fid(0x1101_0000).unwrap(),
            Some((Bank::Critter, "art/critters/hmjmpsab.fr0".to_string()))
        );
        assert_eq!(
            assets.art_path_by_fid(0x011B_0000).unwrap(),
            Some((Bank::Critter, "art/critters/hfjmpsbh.frm".to_string()))
        );
        assert_eq!(
            assets.art_path_by_fid(0x0801_3000).unwrap(),
            Some((Bank::Master, "art/heads/VREDFLgf3.frm".to_string()))
        );
        assert!(assets.art_path_by_fid(0x011B_0001).is_err());
        fs::create_dir_all(root.join("Master_Patches").join("Proto").join("Tiles")).unwrap();
        fs::write(
            root.join("Master_Patches")
                .join("Proto")
                .join("Tiles")
                .join("tiles.lst"),
            b"00000001.PRO\n00000002.PRO\n",
        )
        .unwrap();
        let mut record = Vec::new();
        for value in [0x0400_0002_i32, 99, 55, 10, 20, 30, 40] {
            record.extend_from_slice(&value.to_be_bytes());
        }
        fs::write(
            root.join("Master_Patches")
                .join("Proto")
                .join("Tiles")
                .join("00000002.PRO"),
            record,
        )
        .unwrap();
        let proto = assets.prototype_by_pid(0x0400_0002).unwrap().unwrap();
        assert_eq!(proto.message_id, 99);
        assert_eq!(proto.fid, 55);
        assert!(assets.prototype_by_pid(0x0400_0001).unwrap().is_none());
        assert!(assets.prototype_by_pid(0x0400_0000).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
