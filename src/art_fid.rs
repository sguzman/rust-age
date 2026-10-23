//! Original Fallout 1 CE FID bitfields and static art category names.
//!
//! Reference: pinned CE src/game/art.cc art_get_name and src/game/object_types.h.
//! Critter and head FIDs require generated animation suffixes, so those
//! deliberately remain unsupported rather than resolving an incorrect sprite.

use std::io;

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArtFid(pub u32);

impl ArtFid {
    pub fn category(self) -> u8 {
        ((self.0 >> 24) & 0x0F) as u8
    }

    pub fn index(self) -> usize {
        (self.0 & 0x0FFF) as usize
    }

    pub fn animation(self) -> u8 {
        ((self.0 >> 16) & 0xFF) as u8
    }

    pub fn weapon_animation(self) -> u8 {
        ((self.0 >> 12) & 0x0F) as u8
    }

    pub fn rotation(self) -> u8 {
        ((self.0 >> 28) & 0x07) as u8
    }

    /// Categories sharing a direct art-list filename in CE art_get_name.
    /// Critters and talking heads need a separate CE animation/alias resolver.
    pub fn direct_category(self) -> io::Result<&'static str> {
        match self.category() {
            0 => Ok("items"),
            2 => Ok("scenery"),
            3 => Ok("walls"),
            4 => Ok("tiles"),
            5 => Ok("misc"),
            6 => Ok("intrface"),
            7 => Ok("inven"),
            9 => Ok("backgrnd"),
            10 => Ok("skilldex"),
            1 | 8 => Err(invalid("animation-specific art FID not yet implemented")),
            _ => Err(invalid("unknown Fallout 1 art FID category")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ArtFid;

    #[test]
    fn extracts_original_ce_bitfields_without_sign_extension() {
        let fid = ArtFid(0x350A_B123);
        assert_eq!(fid.category(), 5);
        assert_eq!(fid.index(), 0x123);
        assert_eq!(fid.animation(), 0x0A);
        assert_eq!(fid.weapon_animation(), 0xB);
        assert_eq!(fid.rotation(), 3);
        assert_eq!(fid.direct_category().unwrap(), "misc");
    }

    #[test]
    fn identifies_all_direct_ce_art_directories() {
        let categories = [
            "items", "scenery", "walls", "tiles", "misc", "intrface",
            "inven", "backgrnd", "skilldex",
        ];
        for (n, type_id) in [0_u32, 2, 3, 4, 5, 6, 7, 9, 10].iter().enumerate() {
            assert_eq!(ArtFid(type_id << 24).direct_category().unwrap(), categories[n]);
        }
        assert!(ArtFid(0x0100_0000).direct_category().is_err());
        assert!(ArtFid(0x0800_0000).direct_category().is_err());
        assert!(ArtFid(0x0F00_0000).direct_category().is_err());
    }
}
