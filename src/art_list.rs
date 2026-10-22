//! CE-style art/*.lst reader: fixed file IDs index original filenames.
//!
//! Reference: src/game/art.cc art_read_lst. Every original line occupies an
//! index even if its filename is empty; first punctuation/space terminates
//! the name, which is limited to the first 12 bytes.
//! This is source data, not a general-purpose Unicode text format.

use std::io;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtList {
    entries: Vec<Vec<u8>>,
}

impl ArtList {
    /// Parse the original art_read_lst list semantics without borrowing data
    /// from a Fallout installation or shipping game files.
    pub fn parse(bytes: &[u8]) -> Self {
        let mut entries = Vec::new();
        if bytes.is_empty() {
            return Self { entries };
        }
        for line in bytes.split_inclusive(|ch| *ch == b'\n') {
            let limit = line
                .iter()
                .position(|ch| b" ,;\r\t\n".contains(ch))
                .unwrap_or(line.len());
            entries.push(line[..limit.min(12)].to_vec());
        }
        Self { entries }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn name_bytes(&self, id: usize) -> Option<&[u8]> {
        self.entries.get(id).map(Vec::as_slice)
    }

    /// Game art filenames in the original lists are ASCII. Reject a bad
    /// filename rather than substituting Unicode characters into a path.
    pub fn file_name(&self, id: usize) -> io::Result<Option<&str>> {
        let Some(bytes) = self.name_bytes(id) else {
            return Ok(None);
        };
        let name = std::str::from_utf8(bytes)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "non-UTF8 art filename"))?;
        if name.contains('/') || name.contains('\\') || name == ".." {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unsafe art filename",
            ));
        }
        Ok(Some(name))
    }
}

#[cfg(test)]
mod tests {
    use super::ArtList;

    #[test]
    fn ce_lines_preserve_art_id_and_parse_comments() {
        let list =
            ArtList::parse(b"GRID001.FRM ; comment\r\n\nLONG_FILENAME.FRM,8\nother.frm\t#alias");
        assert_eq!(list.len(), 4);
        assert_eq!(list.file_name(0).unwrap(), Some("GRID001.FRM"));
        assert_eq!(list.file_name(1).unwrap(), Some(""));
        assert_eq!(list.file_name(2).unwrap(), Some("LONG_FILENAM"));
        assert_eq!(list.file_name(3).unwrap(), Some("other.frm"));
        assert_eq!(list.file_name(4).unwrap(), None);
    }

    #[test]
    fn empty_list_and_non_utf8_names_are_handled() {
        assert!(ArtList::parse(b"").is_empty());
        assert!(ArtList::parse(&[0xFF, b'\n']).file_name(0).is_err());
        assert!(ArtList::parse(b"../file.frm\n").file_name(0).is_err());
        assert!(ArtList::parse(b"subdir/file.frm\n").file_name(0).is_err());
    }
}
