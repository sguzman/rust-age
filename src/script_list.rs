//! CE scripts/scripts.lst positional lookup: script index -> .INT name.
//! Reference: pinned CE src/game/scripts.cc scr_index_to_name,
//! scr_list_str and script_make_path. Game data stays external.
use std::io;

/// CE scripts.lst metadata read by scr_find_str_run_info; no procedures run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScriptRunInfo {
    pub map_init: bool,
    pub map_exit: bool,
    pub local_variables: Option<i32>,
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Bound the legacy atoi-style decimal prefix conversion and reject overflow.
fn local_variable_count(value: &[u8]) -> io::Result<i32> {
    let mut text = value;
    while let Some((space, rest)) = text.split_first() {
        if !space.is_ascii_whitespace() {
            break;
        }
        text = rest;
    }
    let mut sign = 1_i64;
    if let Some((first, rest)) = text.split_first() {
        if *first == b'-' {
            sign = -1;
            text = rest;
        } else if *first == b'+' {
            text = rest;
        }
    }
    let mut count = 0_i64;
    let mut digits = 0_usize;
    for digit in text {
        if !digit.is_ascii_digit() {
            break;
        }
        count = count
            .checked_mul(10)
            .and_then(|number| number.checked_add(i64::from(digit - b'0')))
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "overflowed CE local_vars")
            })?;
        digits += 1;
    }
    if digits == 0 {
        return Ok(0); // C atoi parses invalid/no decimal prefix as zero.
    }
    i32::try_from(sign * count)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "overflowed CE local_vars"))
}

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

    /// CE scr_find_str_run_info searches only the comment after '#'.
    /// The flags can be present in arbitrary order, while local_vars=
    /// accepts a signed atoi-style decimal prefix.
    pub fn run_info(&self, index: usize) -> io::Result<Option<ScriptRunInfo>> {
        let Some(line) = self.rows.get(index) else {
            return Ok(None);
        };
        let Some(hash) = line.iter().position(|&ch| ch == b'#') else {
            return Ok(Some(ScriptRunInfo::default()));
        };
        let comment = &line[hash..];
        let vars = if let Some(offset) = find_bytes(comment, b"local_vars=") {
            Some(local_variable_count(
                &comment[offset + b"local_vars=".len()..],
            )?)
        } else {
            None
        };
        Ok(Some(ScriptRunInfo {
            map_init: find_bytes(comment, b"map_init").is_some(),
            map_exit: find_bytes(comment, b"map_exit").is_some(),
            local_variables: vars,
        }))
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
    fn reads_ce_map_init_exit_flags_and_atoi_style_local_variable_count() {
        let scripts = ScriptList::parse(b"ONE.ssl # map_init map_exit local_vars=12 trailing\nTWO.ssl # local_vars=-3 extra\nTHREE.ssl # local_vars=not-a-number\nFOUR.ssl\nFIVE.ssl # MAP_INIT\n");
        let first = scripts.run_info(0).unwrap().unwrap();
        assert!(first.map_init);
        assert!(first.map_exit);
        assert_eq!(first.local_variables, Some(12));
        assert_eq!(
            scripts.run_info(1).unwrap().unwrap().local_variables,
            Some(-3)
        );
        assert_eq!(
            scripts.run_info(2).unwrap().unwrap().local_variables,
            Some(0)
        );
        assert_eq!(scripts.run_info(3).unwrap().unwrap().local_variables, None);
        assert!(!scripts.run_info(4).unwrap().unwrap().map_init);
        assert_eq!(scripts.run_info(5).unwrap(), None);
    }

    #[test]
    fn rejects_overflowed_ce_local_variables_without_panicking() {
        let scripts =
            ScriptList::parse(b"BAD.ssl # local_vars=999999999999999999999999999999999\n");
        assert!(scripts.run_info(0).is_err());
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
