//! Unlit object-sprite inspection overlay on the existing indexed surface.
//!
//! The CE object layout and art ID rules are retained, but this is a
//! diagnostic only: original tile traversal, light tables, egg masking,
//! alpha effects and roof/object ordering are not faithfully ported yet.
//! No scripts, state changes or actor animations are executed.
use crate::assets::GameAssets;
use crate::framebuffer::IndexedSurface;
use crate::frm::Frm;
use crate::hex::HexViewport;
use crate::object_bounds::SpriteBounds;
use crate::object_tables::ObjectTables;
use std::collections::BTreeMap;
use std::io;

const OBJECT_HIDDEN: i32 = 1;
const OBJECT_FLAT: i32 = 8;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ObjectPreviewStats {
    pub rendered: usize,
    pub hidden: usize,
    pub missing_art: usize,
}

pub struct ObjectPreview;

impl ObjectPreview {
    /// Draw static top-level MAP actors on an indexed viewport. This does
    /// not resolve inventory sprites (they are not on the ground), but does
    /// include critters, items, scenery and walls when their art exists.
    pub fn draw(
        assets: &mut GameAssets,
        tables: &ObjectTables,
        view: &HexViewport,
        elevation: usize,
        surface: &mut IndexedSurface,
    ) -> io::Result<ObjectPreviewStats> {
        let objects = tables
            .elevations
            .get(elevation)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid MAP elevation"))?;
        let mut frames: BTreeMap<u32, Option<Frm>> = BTreeMap::new();
        let mut stats = ObjectPreviewStats::default();

        // CE draws flat objects before ordinary objects. Within these two
        // groups we preserve serialized order; original tile/hex ordering and
        // roof passes must be implemented before claiming visual parity.
        for flat in [true, false] {
            for object in objects {
                let header = &object.record.header;
                if header.flags & OBJECT_HIDDEN != 0 {
                    if flat {
                        stats.hidden += 1;
                    }
                    continue;
                }
                if (header.flags & OBJECT_FLAT != 0) != flat {
                    continue;
                }
                let fid = header.fid as u32;
                if let std::collections::btree_map::Entry::Vacant(entry) = frames.entry(fid) {
                    let decoded = match assets.art_path_by_fid(fid)? {
                        Some((bank, path)) => match assets.read(bank, &path)? {
                            Some(bytes) => Some(Frm::decode(&bytes)?),
                            None => None,
                        },
                        None => None,
                    };
                    entry.insert(decoded);
                }
                let Some(Some(art)) = frames.get(&fid) else {
                    stats.missing_art += 1;
                    continue;
                };
                let direction = usize::try_from(header.rotation).map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidData, "negative sprite rotation")
                })?;
                let frame_number = usize::try_from(header.frame).map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidData, "negative sprite frame")
                })?;
                let Some(frame) = art
                    .frames
                    .get(direction)
                    .and_then(|direction| direction.get(frame_number))
                else {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "MAP object frame not found in original FRM",
                    ));
                };
                let bounds = SpriteBounds::compute(view, header, art, false)?;
                surface.blit_frame(frame, bounds.left, bounds.top, true)?;
                stats.rendered += 1;
            }
        }
        Ok(stats)
    }
}

#[cfg(test)]
mod tests {
    use super::ObjectPreview;
    use crate::assets::GameAssets;
    use crate::framebuffer::IndexedSurface;
    use crate::hex::HexViewport;
    use crate::object_data::{ObjectHeader, ObjectRecord, ObjectUpdate};
    use crate::object_tables::{MapObject, ObjectTables};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn synthetic_object(flags: i32) -> MapObject {
        MapObject {
            record: ObjectRecord {
                header: ObjectHeader {
                    id: 42,
                    tile: 20099,
                    x: 0,
                    y: 0,
                    screen_x: 0,
                    screen_y: 0,
                    frame: 0,
                    rotation: 0,
                    fid: 0,
                    flags,
                    elevation: 0,
                    pid: 1,
                    cid: -1,
                    light_distance: 0,
                    light_intensity: 0,
                    serialized_outline: 0,
                    script_id: -1,
                    field_80: 0,
                },
                inventory_length: 0,
                inventory_capacity: 0,
                update: ObjectUpdate::Generic {
                    flags: 0,
                    subtype_fields: vec![],
                },
            },
            inventory: vec![],
        }
    }

    #[test]
    fn blits_ce_art_fid_object_using_original_tile_anchor() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "rust-age-obj-preview-{}-{stamp}",
            std::process::id()
        ));
        let art = root.join("data").join("art").join("items");
        fs::create_dir_all(&art).unwrap();
        fs::write(root.join("master.dat"), [0_u8; 16]).unwrap();
        fs::write(root.join("critter.dat"), [0_u8; 16]).unwrap();
        fs::write(art.join("items.lst"), b"ICON.FRM\n").unwrap();
        let mut frm = Vec::new();
        frm.extend_from_slice(&4_i32.to_be_bytes());
        frm.extend_from_slice(&10_i16.to_be_bytes());
        frm.extend_from_slice(&0_i16.to_be_bytes());
        frm.extend_from_slice(&1_i16.to_be_bytes());
        frm.extend_from_slice(&[0_u8; 24]);
        frm.extend_from_slice(&[0_u8; 24]);
        frm.extend_from_slice(&16_i32.to_be_bytes());
        frm.extend_from_slice(&2_i16.to_be_bytes());
        frm.extend_from_slice(&2_i16.to_be_bytes());
        frm.extend_from_slice(&4_i32.to_be_bytes());
        frm.extend_from_slice(&0_i16.to_be_bytes());
        frm.extend_from_slice(&0_i16.to_be_bytes());
        frm.extend_from_slice(&[7, 0, 0, 9]);
        fs::write(art.join("ICON.FRM"), frm).unwrap();

        let tables = ObjectTables {
            elevations: [
                vec![synthetic_object(1), synthetic_object(0)],
                vec![],
                vec![],
            ],
            total_objects: 2,
            bytes_consumed: 0,
        };
        let mut assets = GameAssets::open(&root).unwrap();
        let view = HexViewport::new(200, 200, 640, 380, 20099).unwrap();
        let mut surface = IndexedSurface::new(640, 380).unwrap();
        let stats = ObjectPreview::draw(&mut assets, &tables, &view, 0, &mut surface).unwrap();
        assert_eq!(stats.rendered, 1);
        assert_eq!(stats.hidden, 1);
        assert_eq!(stats.missing_art, 0);
        assert_eq!(surface.indices()[189 * 640 + 319], 7);
        assert_eq!(surface.indices()[189 * 640 + 320], 0);
        assert_eq!(surface.indices()[190 * 640 + 320], 9);
        fs::remove_dir_all(root).unwrap();
    }
}
