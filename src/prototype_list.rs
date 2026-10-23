//! CE proto/*.lst filenames, indexed by the lower 24 bits of a PID.
//! CE proto_list_str uses one-based line numbers and space/newline delimiting.
use std::io;

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrototypeList {
    entries: Vec<Vec<u8>>,
}
impl PrototypeList {
    pub fn parse(bytes: &[u8]) -> Self {
        let mut entries = Vec::new();
        for line in bytes.split_inclusive(|&ch| ch == b'\n') {
            let limit = line
                .iter()
                .position(|&ch| matches!(ch, b' ' | b'\r' | b'\n'))
                .unwrap_or(line.len());
            entries.push(line[..limit].to_vec());
        }
        Self { entries }
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn get(&self, id: u32) -> io::Result<Option<&str>> {
        let Some(index) = id.checked_sub(1).and_then(|x| usize::try_from(x).ok()) else {
            return Ok(None);
        };
        let Some(name) = self.entries.get(index) else {
            return Ok(None);
        };
        let name = std::str::from_utf8(name)
            .map_err(|_| invalid("prototype list filename is not UTF-8"))?;
        if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\', ':', '\0']) {
            return Err(invalid("unsafe or empty prototype filename"));
        }
        Ok(Some(name))
    }
}
#[cfg(test)]
mod tests {
    use super::PrototypeList;
    #[test]
    fn preserves_one_based_pid_rows_and_comments() {
        let list = PrototypeList::parse(b"00000001.PRO comment\r\n00000002.PRO\n\n00000004.PRO\n");
        assert_eq!(list.len(), 4);
        assert_eq!(list.get(1).unwrap(), Some("00000001.PRO"));
        assert_eq!(list.get(2).unwrap(), Some("00000002.PRO"));
        assert!(list.get(3).is_err());
        assert_eq!(list.get(4).unwrap(), Some("00000004.PRO"));
        assert_eq!(list.get(5).unwrap(), None);
        assert_eq!(list.get(0).unwrap(), None);
    }
    #[test]
    fn rejects_traversal_and_invalid_names() {
        assert!(PrototypeList::parse(b"../secret\n").get(1).is_err());
        assert!(PrototypeList::parse(b"sub/path.pro\n").get(1).is_err());
        assert!(PrototypeList::parse(&[0xFF, b'\n']).get(1).is_err());
        assert!(PrototypeList::parse(b"").is_empty());
    }
}
