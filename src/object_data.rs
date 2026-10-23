//! Read-only Fallout 1 CE serialized map-object fields.
//!
//! Source: pinned CE src/game/object.cc obj_read_obj and
//! src/game/proto.cc proto_read_protoUpdateData.
//!
//! This reads ONE object record (not inventories, not complete MAP object
//! tables). The caller must resolve item/scenery prototype subtypes to know
//! how many type-dependent fields are serialized. No gameplay is executed.

use crate::prototype::ObjectKind;
use std::io::{self, Read};

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn word(input: &mut impl Read) -> io::Result<i32> {
    let mut raw = [0_u8; 4];
    input.read_exact(&mut raw)?;
    Ok(i32::from_be_bytes(raw))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectHeader {
    pub id: i32,
    pub tile: i32,
    pub x: i32,
    pub y: i32,
    pub screen_x: i32,
    pub screen_y: i32,
    pub frame: i32,
    pub rotation: i32,
    pub fid: i32,
    pub flags: i32,
    pub elevation: i32,
    pub pid: u32,
    pub cid: i32,
    pub light_distance: i32,
    pub light_intensity: i32,
    pub serialized_outline: i32,
    pub script_id: i32,
    pub field_80: i32,
}

impl ObjectHeader {
    fn read(input: &mut impl Read) -> io::Result<Self> {
        Ok(Self {
            id: word(input)?,
            tile: word(input)?,
            x: word(input)?,
            y: word(input)?,
            screen_x: word(input)?,
            screen_y: word(input)?,
            frame: word(input)?,
            rotation: word(input)?,
            fid: word(input)?,
            flags: word(input)?,
            elevation: word(input)?,
            pid: word(input)? as u32,
            cid: word(input)?,
            light_distance: word(input)?,
            light_intensity: word(input)?,
            serialized_outline: word(input)?,
            script_id: word(input)?,
            field_80: word(input)?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectUpdate {
    /// Critter's field_0, 7-word combat record, HP, radiation, poison.
    Critter {
        field_0: i32,
        combat: [i32; 7],
        hit_points: i32,
        radiation: i32,
        poison: i32,
    },
    /// Noncritter flags and any subtype-dependent serialized fields.
    Generic {
        flags: i32,
        subtype_fields: Vec<i32>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectRecord {
    pub header: ObjectHeader,
    pub inventory_length: i32,
    pub inventory_capacity: i32,
    /// The next i32 is an original pointer and is deliberately discarded.
    pub update: ObjectUpdate,
}

impl ObjectRecord {
    /// Read one CE map object without inventory children. For items and
    /// scenery, caller MUST supply the corresponding `.PRO` subtype.
    pub fn read(input: &mut impl Read, subtype: Option<i32>) -> io::Result<Self> {
        let header = ObjectHeader::read(input)?;
        let kind = ObjectKind::from_pid(header.pid)?;
        let inventory_length = word(input)?;
        let inventory_capacity = word(input)?;
        let _legacy_inventory_pointer = word(input)?;
        if !(0..=10_000).contains(&inventory_length)
            || inventory_capacity < inventory_length
            || inventory_capacity > 100_000
        {
            return Err(invalid("invalid object inventory dimensions"));
        }
        let update = match kind {
            ObjectKind::Critter => {
                let field_0 = word(input)?;
                let mut combat = [0_i32; 7];
                for value in &mut combat {
                    *value = word(input)?;
                }
                ObjectUpdate::Critter {
                    field_0,
                    combat,
                    hit_points: word(input)?,
                    radiation: word(input)?,
                    poison: word(input)?,
                }
            }
            _ => {
                let mut flags = word(input)?;
                // Original CE repairs a long-standing uninitialized marker.
                if flags as u32 == 0xCCCC_CCCC {
                    flags = 0;
                }
                let n = match kind {
                    ObjectKind::Item => match subtype {
                        Some(3) => 2,         // weapon ammunition count and type
                        Some(4..=6) => 1, // ammo, misc charges, key
                        Some(0..=2) => 0,
                        _ => return Err(invalid("item prototype subtype required")),
                    },
                    ObjectKind::Scenery => match subtype {
                        Some(0 | 3 | 4) => 1, // door/ladder
                        Some(1 | 2) => 2,     // stairs/elevator
                        Some(5) => 0,
                        _ => return Err(invalid("scenery prototype subtype required")),
                    },
                    ObjectKind::Misc if (0x0500_0010..=0x0500_0017).contains(&header.pid) => 4,
                    _ => 0,
                };
                let mut subtype_fields = Vec::with_capacity(n);
                for _ in 0..n {
                    subtype_fields.push(word(input)?);
                }
                ObjectUpdate::Generic {
                    flags,
                    subtype_fields,
                }
            }
        };
        Ok(Self {
            header,
            inventory_length,
            inventory_capacity,
            update,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{ObjectRecord, ObjectUpdate};
    use std::io::{Cursor, Read};

    fn encode(values: &[i32]) -> Vec<u8> {
        values
            .iter()
            .flat_map(|value| value.to_be_bytes())
            .collect()
    }

    fn object_prefix(pid: i32) -> Vec<i32> {
        vec![
            500,
            20099,
            10,
            20,
            30,
            40,
            2,
            3,
            0x0300_0002,
            0,
            0,
            pid,
            -1,
            4,
            5,
            0,
            -1,
            0,
        ]
    }

    #[test]
    fn parses_noncritter_object_and_keeps_next_record_boundary() {
        let mut values = object_prefix(0x0400_0002);
        values.extend([0, 0, 0, 0xCCCC_CCCC_u32 as i32]);
        let mut bytes = encode(&values);
        bytes.extend_from_slice(&[11, 22, 33, 44]);
        let mut reader = Cursor::new(bytes);
        let parsed = ObjectRecord::read(&mut reader, None).unwrap();
        assert_eq!(parsed.header.id, 500);
        assert_eq!(parsed.header.tile, 20099);
        assert_eq!(parsed.header.frame, 2);
        assert_eq!(parsed.header.pid, 0x0400_0002);
        assert_eq!(parsed.inventory_length, 0);
        assert_eq!(
            parsed.update,
            ObjectUpdate::Generic {
                flags: 0,
                subtype_fields: vec![],
            }
        );
        let mut trailing = [0; 4];
        reader.read_exact(&mut trailing).unwrap();
        assert_eq!(trailing, [11, 22, 33, 44]);
    }

    #[test]
    fn parses_critter_combat_update_in_ce_order() {
        let mut values = object_prefix(0x0100_0003);
        values.extend([0, 0, 0, 9]); // inventory and field_0
        values.extend([1, 2, 3, 4, 5, 6, 7]);
        values.extend([80, 1, 0]);
        let mut reader = Cursor::new(encode(&values));
        let record = ObjectRecord::read(&mut reader, None).unwrap();
        assert_eq!(
            record.update,
            ObjectUpdate::Critter {
                field_0: 9,
                combat: [1, 2, 3, 4, 5, 6, 7],
                hit_points: 80,
                radiation: 1,
                poison: 0,
            }
        );
        assert_eq!(reader.position() as usize, values.len() * 4);
    }

    #[test]
    fn scenery_and_weapon_require_ce_prototype_types() {
        let mut values = object_prefix(0x0200_0001);
        values.extend([0, 0, 0, 1, 55, 66]); // flags and stairs destination
        assert!(ObjectRecord::read(&mut Cursor::new(encode(&values)), None).is_err());
        let record = ObjectRecord::read(&mut Cursor::new(encode(&values)), Some(1)).unwrap();
        assert_eq!(
            record.update,
            ObjectUpdate::Generic {
                flags: 1,
                subtype_fields: vec![55, 66],
            }
        );
        let mut weapon = object_prefix(0x0000_0001);
        weapon.extend([0, 0, 0, 0, 15, 0x0000_0010]);
        let record = ObjectRecord::read(&mut Cursor::new(encode(&weapon)), Some(3)).unwrap();
        assert_eq!(
            record.update,
            ObjectUpdate::Generic {
                flags: 0,
                subtype_fields: vec![15, 0x0000_0010],
            }
        );
    }
}
