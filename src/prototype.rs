//! Read-only Fallout 1 CE .PRO prototype structures.
//!
//! Reference: src/game/proto.cc, proto_read_protoSubNode,
//! proto_read_item_data and proto_read_scenery_data.
//! Critter stat arrays are not implemented yet; remaining bytes stay opaque.

use std::io::{self, Cursor, Read};

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn word(reader: &mut impl Read) -> io::Result<i32> {
    let mut data = [0_u8; 4];
    reader.read_exact(&mut data)?;
    Ok(i32::from_be_bytes(data))
}

fn words(reader: &mut impl Read, count: usize) -> io::Result<Vec<i32>> {
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        values.push(word(reader)?);
    }
    Ok(values)
}

fn byte(reader: &mut impl Read) -> io::Result<u8> {
    let mut value = [0_u8; 1];
    reader.read_exact(&mut value)?;
    Ok(value[0])
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectKind {
    Item,
    Critter,
    Scenery,
    Wall,
    Tile,
    Misc,
}

impl ObjectKind {
    pub fn from_pid(pid: u32) -> io::Result<Self> {
        match pid >> 24 {
            0 => Ok(Self::Item),
            1 => Ok(Self::Critter),
            2 => Ok(Self::Scenery),
            3 => Ok(Self::Wall),
            4 => Ok(Self::Tile),
            5 => Ok(Self::Misc),
            _ => Err(invalid("unsupported Fallout 1 prototype object type")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LightFields {
    pub distance: i32,
    pub intensity: i32,
    pub flags: i32,
    pub extended_flags: i32,
    pub script_id: i32,
}

impl LightFields {
    fn read(input: &mut impl Read) -> io::Result<Self> {
        Ok(Self {
            distance: word(input)?,
            intensity: word(input)?,
            flags: word(input)?,
            extended_flags: word(input)?,
            script_id: word(input)?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrototypeBody {
    Item {
        light: LightFields,
        item_type: i32,
        material: i32,
        size: i32,
        weight: i32,
        cost: i32,
        inventory_fid: i32,
        legacy_marker: u8,
        /// Exact CE type-specific 32-bit fields, in original read order.
        details: Vec<i32>,
        weapon_sound_code: Option<u8>,
    },
    Critter {
        light: LightFields,
        head_fid: i32,
        ai_packet: i32,
        team: i32,
    },
    Scenery {
        light: LightFields,
        scenery_type: i32,
        material: i32,
        legacy_marker: u8,
        details: Vec<i32>,
    },
    Wall {
        light: LightFields,
        material: i32,
    },
    Tile {
        flags: i32,
        extended_flags: i32,
        script_id: i32,
        material: i32,
    },
    Misc {
        distance: i32,
        intensity: i32,
        flags: i32,
        extended_flags: i32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prototype {
    pub pid: u32,
    pub message_id: i32,
    pub fid: i32,
    pub kind: ObjectKind,
    pub body: PrototypeBody,
    /// Raw, unimplemented data (normally critter stat arrays).
    pub remaining_bytes: Vec<u8>,
}

impl Prototype {
    pub fn decode(bytes: &[u8]) -> io::Result<Self> {
        let mut input = Cursor::new(bytes);
        let pid = word(&mut input)? as u32;
        let message_id = word(&mut input)?;
        let fid = word(&mut input)?;
        let kind = ObjectKind::from_pid(pid)?;
        let body = match kind {
            ObjectKind::Item => {
                let light = LightFields::read(&mut input)?;
                let item_type = word(&mut input)?;
                let material = word(&mut input)?;
                let size = word(&mut input)?;
                let weight = word(&mut input)?;
                let cost = word(&mut input)?;
                let inventory_fid = word(&mut input)?;
                let legacy_marker = byte(&mut input)?;
                // CE types: armor, container, drug, weapon, ammo, misc, key.
                // Only weapon has a final trailing one-byte sound code.
                let field_count = match item_type {
                    0 => 18,
                    1 => 2,
                    2 => 17,
                    3 => 16,
                    4 => 6,
                    5 => 3,
                    6 => 1,
                    _ => return Err(invalid("unsupported Fallout 1 item type")),
                };
                let details = words(&mut input, field_count)?;
                let weapon_sound_code = if item_type == 3 {
                    Some(byte(&mut input)?)
                } else {
                    None
                };
                PrototypeBody::Item {
                    light,
                    item_type,
                    material,
                    size,
                    weight,
                    cost,
                    inventory_fid,
                    legacy_marker,
                    details,
                    weapon_sound_code,
                }
            }
            ObjectKind::Critter => {
                let light = LightFields::read(&mut input)?;
                let head_fid = word(&mut input)?;
                let ai_packet = word(&mut input)?;
                let team = word(&mut input)?;
                PrototypeBody::Critter {
                    light,
                    head_fid,
                    ai_packet,
                    team,
                }
            }
            ObjectKind::Scenery => {
                let light = LightFields::read(&mut input)?;
                let scenery_type = word(&mut input)?;
                let material = word(&mut input)?;
                let legacy_marker = byte(&mut input)?;
                // Door, stairs, elevator, ladder-up, ladder-down, generic.
                let count = match scenery_type {
                    0..=2 => 2,
                    3..=5 => 1,
                    _ => return Err(invalid("unsupported Fallout 1 scenery type")),
                };
                let details = words(&mut input, count)?;
                PrototypeBody::Scenery {
                    light,
                    scenery_type,
                    material,
                    legacy_marker,
                    details,
                }
            }
            ObjectKind::Wall => {
                let light = LightFields::read(&mut input)?;
                PrototypeBody::Wall {
                    light,
                    material: word(&mut input)?,
                }
            }
            ObjectKind::Tile => PrototypeBody::Tile {
                flags: word(&mut input)?,
                extended_flags: word(&mut input)?,
                script_id: word(&mut input)?,
                material: word(&mut input)?,
            },
            ObjectKind::Misc => PrototypeBody::Misc {
                distance: word(&mut input)?,
                intensity: word(&mut input)?,
                flags: word(&mut input)?,
                extended_flags: word(&mut input)?,
            },
        };
        let remaining_bytes = bytes[input.position() as usize..].to_vec();
        Ok(Self {
            pid,
            message_id,
            fid,
            kind,
            body,
            remaining_bytes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{ObjectKind, Prototype, PrototypeBody};

    fn encode(values: &[i32]) -> Vec<u8> {
        values.iter().flat_map(|n| n.to_be_bytes()).collect()
    }

    #[test]
    fn decodes_ce_tile_prototype_fields() {
        let bytes = encode(&[0x0400_0001, 21, 55, 1, 2, 3, 4]);
        let proto = Prototype::decode(&bytes).unwrap();
        assert_eq!(proto.kind, ObjectKind::Tile);
        assert_eq!(proto.message_id, 21);
        assert_eq!(proto.fid, 55);
        assert!(proto.remaining_bytes.is_empty());
        assert_eq!(
            proto.body,
            PrototypeBody::Tile {
                flags: 1,
                extended_flags: 2,
                script_id: 3,
                material: 4,
            }
        );
    }

    #[test]
    fn decodes_item_container_subtype_with_two_words() {
        let mut bytes = encode(&[
            0x0000_0010, 7, 4, 2, 3, 4, 5, -1, 1, 10, 20, 30, 40,
        ]);
        bytes.push(0);
        bytes.extend_from_slice(&encode(&[50, 60]));
        let proto = Prototype::decode(&bytes).unwrap();
        match proto.body {
            PrototypeBody::Item {
                item_type,
                size,
                details,
                ..
            } => {
                assert_eq!(item_type, 1);
                assert_eq!(size, 20);
                assert_eq!(details, vec![50, 60]);
            }
            _ => panic!("expected item"),
        }
    }

    #[test]
    fn critter_extra_data_remains_uninterpreted() {
        let mut bytes = encode(&[
            0x0100_0010, 2, 3, 4, 5, 6, 7, -1, 8, 9, 10,
        ]);
        bytes.extend_from_slice(&[1, 2, 3, 4]);
        let parsed = Prototype::decode(&bytes).unwrap();
        assert_eq!(parsed.kind, ObjectKind::Critter);
        assert_eq!(parsed.remaining_bytes, vec![1, 2, 3, 4]);
    }

    #[test]
    fn rejects_bad_pid_and_truncated_item_data() {
        assert!(Prototype::decode(&encode(&[0x0600_0001, 0, 0])).is_err());
        let mut bytes = encode(&[
            0x0000_0010, 7, 4, 2, 3, 4, 5, -1, 1, 10, 20, 30, 40,
        ]);
        bytes.push(0);
        bytes.extend_from_slice(&encode(&[50]));
        assert!(Prototype::decode(&bytes).is_err());
    }
}
