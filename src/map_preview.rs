//! Read-only diagnostic floor preview of original Fallout 1 MAP tiles.
//!
//! This uses CE's map-square order and square_coord projection, but it is
//! NOT the final renderer. It lacks light-map interpolation, ceilings,
//! objects, actors, scrolling, alpha masks, and full viewport clipping.
//! Every game sprite and palette comes from the player's own installation.

use crate::assets::{Bank, GameAssets};
use crate::framebuffer::IndexedSurface;
use crate::frm::{Frame, Frm};
use crate::hex::HexViewport;
use crate::map::{MapHeader, SQUARE_COUNT};
use std::collections::BTreeMap;
use std::io;

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

pub struct FloorPreview {
    pub surface: IndexedSurface,
    /// Count of nonempty tile art IDs considered inside the diagnostic crop.
    pub tile_count: usize,
    /// Count of distinct successfully resolved FRM filenames.
    pub unique_art: usize,
}

fn load_tile_frame(assets: &mut GameAssets, name: &str) -> io::Result<Option<Frame>> {
    if name.is_empty() {
        return Ok(None);
    }
    let path = format!("art/tiles/{name}");
    let Some(bytes) = assets.read(Bank::Master, &path)? else {
        return Ok(None);
    };
    let frm = Frm::decode(&bytes)?;
    Ok(frm.frames[0].first().cloned())
}

impl FloorPreview {
    /// Draw static floor FRMs of one elevation without interpreting scripts.
    /// This is a diagnostic, not a faithful full map render.
    pub fn render(
        assets: &mut GameAssets,
        map_bytes: &[u8],
        width: usize,
        height: usize,
    ) -> io::Result<Self> {
        let header = MapHeader::decode(map_bytes)?;
        let squares = header.decode_squares(map_bytes)?;
        let elevation = usize::try_from(header.entering_elevation)
            .map_err(|_| invalid("invalid entering elevation"))?;
        let Some(layer) = squares.elevations.get(elevation).and_then(Option::as_ref) else {
            return Err(invalid("entering elevation has no static square tiles"));
        };
        let w = i32::try_from(width).map_err(|_| invalid("preview width too large"))?;
        let h = i32::try_from(height).map_err(|_| invalid("preview height too large"))?;
        let view = HexViewport::new(200, 200, w, h, header.entering_tile)
            .ok_or_else(|| invalid("invalid Fallout 1 viewport geometry"))?;
        let tile_names = assets.tile_art_list()?;
        let mut surface = IndexedSurface::new(width, height)?;
        let mut frame_cache: BTreeMap<u16, Option<Frame>> = BTreeMap::new();
        let mut tile_count = 0;

        // CE square_render_floor traverses square rows, then columns.
        // The crop bounds here are deliberately generous and diagnostic;
        // the final renderer must use CE's rectangle clipping and lighting.
        for (square, &word) in layer.iter().enumerate().take(SQUARE_COUNT) {
            if word & 0x1000 != 0 {
                continue;
            }
            let id = (word & 0x0FFF) as u16;
            // CE uses tile index 1 as its blank/empty square tile.
            if id == 1 {
                continue;
            }
            let Some((x, y)) = view.square_coord(square as i32, false) else {
                continue;
            };
            if x < -256 || y < -256 || x >= w + 256 || y >= h + 256 {
                continue;
            }

            if let std::collections::btree_map::Entry::Vacant(entry) = frame_cache.entry(id) {
                let frame = match tile_names.file_name(usize::from(id))? {
                    Some(name) => load_tile_frame(assets, name)?,
                    None => None,
                };
                entry.insert(frame);
            }
            if let Some(Some(frame)) = frame_cache.get(&id) {
                surface.blit_frame(frame, x, y, true)?;
                tile_count += 1;
            }
        }

        let unique_art = frame_cache.values().filter(|entry| entry.is_some()).count();
        Ok(Self {
            surface,
            tile_count,
            unique_art,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::FloorPreview;
    use crate::framebuffer::IndexedSurface;

    #[test]
    fn synthetic_map_resolves_original_tile_list_and_blits_frm() {
        use crate::assets::GameAssets;
        use std::fs;
        use std::time::{SystemTime, UNIX_EPOCH};

        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "rust-age-floor-preview-{}-{stamp}",
            std::process::id()
        ));
        let art = root.join("data").join("art").join("tiles");
        fs::create_dir_all(&art).unwrap();
        // Both DAT archives are structurally empty; only synthesized patch data
        // is read by this test, never licensed commercial assets.
        fs::write(root.join("master.dat"), [0_u8; 16]).unwrap();
        fs::write(root.join("critter.dat"), [0_u8; 16]).unwrap();
        fs::write(art.join("tiles.lst"), b"TEST.FRM\\nEMPTY.FRM\\n").unwrap();

        let mut sprite = Vec::new();
        sprite.extend_from_slice(&4_i32.to_be_bytes());
        sprite.extend_from_slice(&10_i16.to_be_bytes());
        sprite.extend_from_slice(&0_i16.to_be_bytes());
        sprite.extend_from_slice(&1_i16.to_be_bytes());
        sprite.extend_from_slice(&[0_u8; 24]);
        sprite.extend_from_slice(&[0_u8; 24]);
        sprite.extend_from_slice(&16_i32.to_be_bytes());
        sprite.extend_from_slice(&2_i16.to_be_bytes());
        sprite.extend_from_slice(&2_i16.to_be_bytes());
        sprite.extend_from_slice(&4_i32.to_be_bytes());
        sprite.extend_from_slice(&0_i16.to_be_bytes());
        sprite.extend_from_slice(&0_i16.to_be_bytes());
        sprite.extend_from_slice(&[7, 0, 0, 9]);
        fs::write(art.join("TEST.FRM"), sprite).unwrap();

        let mut map = Vec::new();
        map.extend_from_slice(&19_i32.to_be_bytes());
        map.extend_from_slice(b"TEST.MAP\\0\\0\\0\\0\\0\\0\\0\\0");
        for field in [20099_i32, 0, 2, 0, -1, 4 | 8, 0, 0, 0, 0] {
            map.extend_from_slice(&field.to_be_bytes());
        }
        map.extend_from_slice(&[0_u8; 44 * 4]);
        for square in 0..10_000 {
            // Every cell except the one visible example is blank tile #1.
            let word = if square == 5049 { 0_u32 } else { 1_u32 };
            map.extend_from_slice(&word.to_be_bytes());
        }

        let mut assets = GameAssets::open(&root).unwrap();
        let preview = FloorPreview::render(&mut assets, &map, 640, 380).unwrap();
        assert_eq!(preview.unique_art, 1);
        assert_eq!(preview.tile_count, 1);
        let (x, y) = (288_usize, 180_usize);
        let indices = preview.surface.indices();
        assert_eq!(indices[y * 640 + x], 7);
        assert_eq!(indices[y * 640 + x + 1], 0);
        assert_eq!(indices[(y + 1) * 640 + x + 1], 9);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn framebuffer_used_by_floor_renderer_is_indexed() {
        let surface = IndexedSurface::new(640, 380).unwrap();
        assert_eq!(surface.indices().len(), 640 * 380);
        assert_eq!(surface.width(), 640);
        assert_eq!(surface.height(), 380);
        let _name = std::any::type_name::<FloorPreview>();
    }
}
