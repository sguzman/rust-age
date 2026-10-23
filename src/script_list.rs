//! CE scripts/scripts.lst positional lookup: script index -> .INT name.
//! Reference: pinned CE src/game/scripts.cc scr_index_to_name,
//! scr_list_str and script_make_path. Game data stays external.
use std::io;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptList {
    rows: Vec<Vec<u8>>,
}
impl ScriptList {
    pub fn parse(bytes: &[u8]) -> Self {
        let mut rows = bytes
            .split(|byte| *byte == b'\n')
            .map(|line| line.strip_suffix(b"\r").unwrap_or(line).to_vec())
            .collect::<Vec<_>>();
        if bytes.last() == Some(&b'\n') {
            rows.pop();
        }
        Self { rows }
    }

    pub fn count(&self) -> usize {
        self.rows.len()
    }

    /// CE finds the *first* dot and substitutes .int. Rows are zero-based
    /// and comments following the extension are irrelevant for the name.
    /// Restrict the basename to safe installed asset names, no traversal.
    pub fn file_name(&self, index: usize) -> io::Result<Option<String>> {
        let Some(line) = self.rows.get(index) else {
            return Ok(None);
        };
        let Some(dot) = line.iter().position(|&b| b == b'.') else {
            return Ok(None);
        };
        let name = std::str::from_utf8(&line[..dot])
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "non-ASCII script filename"))?;
        if name.is_empty()
            || !name
                .as_bytes()
                .iter()
                .all(|ch| ch.is_ascii_alphanumeric() || *ch == b'_' || *ch == b'-')
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unsafe CE scripts.lst filename",
            ));
        }
        Ok(Some(format!("{name}.int")))
    }
}

#[cfg(test)]
mod tests {
    use super::ScriptList;

    #[test]
    fn original_zero_based_script_rows_ignore_source_extensions_and_comments() {
        let list = ScriptList::parse(
            b"ARROYO.ssl # map_init\nFOOBAR.ssl\r\nNO_DOT\nSCRIPT1.INT # local_vars=7\n",
        );
        assert_eq!(list.count(), 4);
        assert_eq!(list.file_name(0).unwrap().as_deref(), Some("ARROYO.int"));
        assert_eq!(list.file_name(1).unwrap().as_deref(), Some("FOOBAR.int"));
        assert_eq!(list.file_name(2).unwrap(), None);
        assert_eq!(list.file_name(3).unwrap().as_deref(), Some("SCRIPT1.int"));
        assert_eq!(list.file_name(4).unwrap(), None);
    }

    #[test]
    fn rejects_traversal_and_broken_script_basename() {
        for name in [
            b"../bad.ssl".as_slice(),
            b"nested/evil.ssl",
            b"C:\\bad.ssl",
            b".ssl",
            b"bad name.ssl",
        ] {
            assert!(ScriptList::parse(name).file_name(0).is_err());
        }
        let list = ScriptList::parse(b"okay.int\n");
        assert_eq!(list.file_name(0).unwrap().as_deref(), Some("okay.int"));
    }
}
