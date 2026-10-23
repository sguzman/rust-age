//! Fallout 1 Community Edition art FID fields and sprite-name generation.
//! Source: pinned CE src/game/art.cc (art_get_name, art_get_code,
//! art_alias_fid), src/game/anim.h and src/game/art.h.
//! No commercial sprites or animation tables are bundled.
use std::io;

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

const HEAD_1: &[u8] = b"gggnnnbbbgnb";
const HEAD_2: &[u8] = b"vfngfbnfvppp";

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

    pub fn directory(self) -> io::Result<&'static str> {
        match self.category() {
            0 => Ok("items"),
            1 => Ok("critters"),
            2 => Ok("scenery"),
            3 => Ok("walls"),
            4 => Ok("tiles"),
            5 => Ok("misc"),
            6 => Ok("intrface"),
            7 => Ok("inven"),
            8 => Ok("heads"),
            9 => Ok("backgrnd"),
            10 => Ok("skilldex"),
            _ => Err(invalid("unknown Fallout 1 art FID category")),
        }
    }

    /// Kept for old diagnostic callers that expect only direct list names.
    pub fn direct_category(self) -> io::Result<&'static str> {
        match self.category() {
            1 | 8 => Err(invalid("animation-specific FID requires generated filename")),
            _ => self.directory(),
        }
    }

    /// CE art_alias_fid uses an alternate critter base for certain death art.
    pub fn needs_critter_alias(self) -> bool {
        self.category() == 1
            && matches!(self.animation(), 27 | 29 | 30 | 33 | 55 | 57 | 58 | 64)
    }

    /// CE art_get_code, restricted to the declared animation 0..=64 and
    /// weapon 0..=10. The original uses ASCII character arithmetic.
    pub fn critter_code(self) -> io::Result<(char, char)> {
        if self.category() != 1 {
            return Err(invalid("critter code requires critter FID"));
        }
        let anim = self.animation();
        let weapon = self.weapon_animation();
        if anim > 64 || weapon > 10 {
            return Err(invalid("invalid critter animation or weapon code"));
        }
        let weapon_letter = char::from(b'd' + weapon.saturating_sub(1));
        let (a, b) = match anim {
            38..=47 => {
                if weapon == 0 {
                    return Err(invalid("weapon animation requires weapon type"));
                }
                (weapon_letter, char::from(b'c' + anim - 38))
            }
            36 => ('c', 'h'),
            37 => ('j', 'c'),
            64 => ('a', 'n'),
            48..=63 => ('r', char::from(b'a' + anim - 48)),
            20..=35 => ('b', char::from(b'a' + anim - 20)),
            18 => match weapon {
                1 => ('d', 'm'),
                4 => ('g', 'm'),
                _ => ('a', 's'),
            },
            13 => {
                if weapon == 0 {
                    ('a', 'n')
                } else {
                    (weapon_letter, 'e')
                }
            }
            0..=1 if weapon > 0 => (weapon_letter, char::from(b'a' + anim)),
            _ => ('a', char::from(b'a' + anim)),
        };
        Ok((a, b))
    }

    /// CE head1/head2 suffix maps. "f" heads include a weapon/fidget digit.
    pub fn head_suffix(self) -> io::Result<String> {
        if self.category() != 8 {
            return Err(invalid("head suffix requires talking-head FID"));
        }
        let anim = usize::from(self.animation());
        let Some((&first, &second)) = HEAD_1.get(anim).zip(HEAD_2.get(anim)) else {
            return Err(invalid("invalid Fallout 1 head animation"));
        };
        let suffix = if second == b'f' {
            format!("{}{}{}", char::from(first), char::from(second), self.weapon_animation())
        } else {
            format!("{}{}", char::from(first), char::from(second))
        };
        Ok(suffix)
    }

    /// Original art_get_name filename generation, once the art-list base and
    /// any critter alias have been resolved by the caller.
    pub fn filename(self, list_name: &str) -> io::Result<String> {
        if list_name.is_empty()
            || list_name == "."
            || list_name == ".."
            || list_name.contains(['/', '\\', ':', '\0'])
        {
            return Err(invalid("unsafe art list filename"));
        }
        match self.category() {
            1 => {
                let (first, second) = self.critter_code()?;
                let suffix = if self.rotation() == 0 {
                    "frm".to_string()
                } else {
                    format!("fr{}", self.rotation() - 1)
                };
                Ok(format!("{list_name}{first}{second}.{suffix}"))
            }
            8 => Ok(format!("{list_name}{}.frm", self.head_suffix()?)),
            _ => {
                self.direct_category()?;
                Ok(list_name.to_string())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ArtFid;

    fn fid(kind: u32, index: u32, animation: u32, weapon: u32, rotation: u32) -> ArtFid {
        ArtFid((rotation << 28) | (kind << 24) | (animation << 16) | (weapon << 12) | index)
    }

    #[test]
    fn extracts_original_ce_bitfields() {
        let art = ArtFid(0x350A_B123);
        assert_eq!(art.category(), 5);
        assert_eq!(art.index(), 0x123);
        assert_eq!(art.animation(), 0x0A);
        assert_eq!(art.weapon_animation(), 0xB);
        assert_eq!(art.rotation(), 3);
        assert_eq!(art.directory().unwrap(), "misc");
    }

    #[test]
    fn direct_ce_art_category_names_and_bounds() {
        let categories = [
            "items", "critters", "scenery", "walls", "tiles", "misc",
            "intrface", "inven", "heads", "backgrnd", "skilldex",
        ];
        for (kind, directory) in categories.iter().enumerate() {
            assert_eq!(ArtFid((kind as u32) << 24).directory().unwrap(), *directory);
        }
        assert!(ArtFid(0x0F00_0000).directory().is_err());
        assert!(ArtFid(0x0100_0000).direct_category().is_err());
        assert!(ArtFid(0x0800_0000).direct_category().is_err());
    }

    #[test]
    fn critter_animation_names_mirror_ce_dispatch() {
        assert_eq!(fid(1, 0, 0, 0, 0).filename("hmjmps").unwrap(), "hmjmpsaa.frm");
        assert_eq!(fid(1, 0, 1, 0, 1).filename("hmjmps").unwrap(), "hmjmpsab.fr0");
        assert_eq!(fid(1, 0, 0, 1, 0).filename("hmjmps").unwrap(), "hmjmpsda.frm");
        assert_eq!(fid(1, 0, 13, 0, 0).filename("hmjmps").unwrap(), "hmjmpsan.frm");
        assert_eq!(fid(1, 0, 13, 1, 0).filename("hmjmps").unwrap(), "hmjmpsde.frm");
        assert_eq!(fid(1, 0, 18, 4, 0).filename("hmjmps").unwrap(), "hmjmpsgm.frm");
        assert_eq!(fid(1, 0, 20, 0, 0).filename("hmjmps").unwrap(), "hmjmpsba.frm");
        assert_eq!(fid(1, 0, 36, 0, 0).filename("hmjmps").unwrap(), "hmjmpsch.frm");
        assert_eq!(fid(1, 0, 38, 1, 0).filename("hmjmps").unwrap(), "hmjmpsdc.frm");
        assert_eq!(fid(1, 0, 47, 10, 0).filename("hmjmps").unwrap(), "hmjmpsmm.frm");
        assert_eq!(fid(1, 0, 63, 0, 0).filename("hmjmps").unwrap(), "hmjmpsrp.frm");
        assert_eq!(fid(1, 0, 64, 0, 0).filename("hmjmps").unwrap(), "hmjmpsan.frm");
        assert!(fid(1, 0, 38, 0, 0).filename("hmjmps").is_err());
        assert!(fid(1, 0, 65, 0, 0).filename("hmjmps").is_err());
        assert!(fid(1, 0, 0, 11, 0).filename("hmjmps").is_err());
        assert!(fid(1, 0, 55, 0, 0).needs_critter_alias());
        assert!(!fid(1, 0, 54, 0, 0).needs_critter_alias());
    }

    #[test]
    fn talking_head_fidget_and_regular_names_match_ce() {
        assert_eq!(fid(8, 0, 0, 0, 0).filename("VREDFL").unwrap(), "VREDFLgv.frm");
        assert_eq!(fid(8, 0, 1, 3, 0).filename("VREDFL").unwrap(), "VREDFLgf3.frm");
        assert_eq!(fid(8, 0, 4, 9, 0).filename("VREDFL").unwrap(), "VREDFLn f9.frm".replace(' ', ""));
        assert!(fid(8, 0, 12, 0, 0).filename("VREDFL").is_err());
        assert_eq!(fid(4, 0, 0, 0, 0).filename("grid001.frm").unwrap(), "grid001.frm");
        assert!(fid(4, 0, 0, 0, 0).filename("../bad").is_err());
    }
}
