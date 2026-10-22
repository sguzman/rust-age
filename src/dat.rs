//! Fallout 1 Community Edition DAT archive index and read paths.
//!
//! Porting references at the pinned CE revision:
//! - src/plib/assoc/assoc.cc (big-endian associative arrays)
//! - src/plib/db/db.cc (directory entries and read modes)
//! - src/plib/db/lzss.cc (compressed data, via crate::lzss)
//!
//! All data is supplied by the player; no Fallout archives are redistributed.
//! Synthetic fixtures test the wire format. Real-asset parity remains unverified.

use crate::lzss;
use std::collections::BTreeMap;
use std::io::{self, Read, Seek, SeekFrom};

const MAX_ASSOC_ENTRIES: usize = 1_000_000;
const MAX_ASSOC_DATA: usize = 1 << 20;
const MAX_EXTRACT_BYTES: usize = 1 << 30;

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub path: String,
    pub flags: i32,
    pub offset: u64,
    pub length: usize,
    pub packed_length: usize,
}

#[derive(Debug)]
pub struct DatArchive<R> {
    source: R,
    file_length: u64,
    entries: BTreeMap<String, Entry>,
}

struct AssocHeader {
    count: usize,
    datasize: usize,
}

fn be_i32(reader: &mut impl Read) -> io::Result<i32> {
    let mut bytes = [0; 4];
    reader.read_exact(&mut bytes)?;
    Ok(i32::from_be_bytes(bytes))
}

fn be_u16(reader: &mut impl Read) -> io::Result<u16> {
    let mut bytes = [0; 2];
    reader.read_exact(&mut bytes)?;
    Ok(u16::from_be_bytes(bytes))
}

fn header(reader: &mut impl Read) -> io::Result<AssocHeader> {
    let count = be_i32(reader)?;
    let capacity = be_i32(reader)?;
    let datasize = be_i32(reader)?;
    let _legacy_pointer = be_i32(reader)?;
    if count < 0 || capacity < count || datasize < 0 {
        return Err(invalid("invalid DAT associative-array header"));
    }
    let count = count as usize;
    let datasize = datasize as usize;
    if count > MAX_ASSOC_ENTRIES || datasize > MAX_ASSOC_DATA {
        return Err(invalid("unreasonably large DAT associative-array header"));
    }
    Ok(AssocHeader { count, datasize })
}

fn name(reader: &mut impl Read) -> io::Result<String> {
    let mut length = [0];
    reader.read_exact(&mut length)?;
    let mut bytes = vec![0; usize::from(length[0])];
    reader.read_exact(&mut bytes)?;
    if bytes.contains(&0) {
        return Err(invalid("NUL inside DAT entry name"));
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn skip(reader: &mut impl Read, count: usize) -> io::Result<()> {
    let mut remaining = count;
    let mut buf = [0; 4096];
    while remaining > 0 {
        let n = remaining.min(buf.len());
        reader.read_exact(&mut buf[..n])?;
        remaining -= n;
    }
    Ok(())
}

fn normalize(path: &str) -> String {
    let replaced = path.replace('\\', "/");
    replaced
        .trim_start_matches("./")
        .trim_start_matches('/')
        .to_ascii_uppercase()
}

impl<R: Read + Seek> DatArchive<R> {
    /// Load a Fallout 1 DAT index from its initial, big-endian directory tables.
    ///
    /// Unlike Fallout 2's later DAT format, Fallout 1 uses associative arrays
    /// at the *start* of the archive. The two formats are not interchangeable.
    pub fn open(mut source: R) -> io::Result<Self> {
        let file_length = source.seek(SeekFrom::End(0))?;
        source.seek(SeekFrom::Start(0))?;
        let root = header(&mut source)?;
        let mut directories = Vec::with_capacity(root.count);
        for _ in 0..root.count {
            let directory = name(&mut source)?;
            skip(&mut source, root.datasize)?;
            directories.push(directory);
        }

        let mut entries = BTreeMap::new();
        for directory in directories {
            let h = header(&mut source)?;
            if h.datasize != 16 {
                return Err(invalid("unsupported DAT file-entry size"));
            }
            for _ in 0..h.count {
                let file_name = name(&mut source)?;
                let flags = be_i32(&mut source)?;
                let offset = be_i32(&mut source)?;
                let length = be_i32(&mut source)?;
                let packed_length = be_i32(&mut source)?;
                if flags < 0 || offset < 0 || length < 0 || packed_length < 0 {
                    return Err(invalid("negative DAT entry metadata"));
                }
                let length = length as usize;
                let packed_length = packed_length as usize;
                if length > MAX_EXTRACT_BYTES || packed_length > MAX_EXTRACT_BYTES {
                    return Err(invalid("DAT entry too large"));
                }
                let mode = if flags == 0 { 0x10 } else { flags & 0xF0 };
                if !matches!(mode, 0x10 | 0x20 | 0x40) {
                    return Err(invalid("unsupported DAT compression mode"));
                }
                let physical_size = match mode {
                    0x10 => packed_length,
                    0x20 => length,
                    _ => 0,
                };
                let offset = offset as u64;
                if offset > file_length
                    || offset
                        .checked_add(physical_size as u64)
                        .is_none_or(|end| end > file_length)
                {
                    return Err(invalid("DAT entry outside archive"));
                }
                let path = if directory.is_empty() {
                    file_name
                } else {
                    format!("{directory}/{file_name}")
                };
                let key = normalize(&path);
                if entries
                    .insert(
                        key,
                        Entry {
                            path,
                            flags,
                            offset,
                            length,
                            packed_length,
                        },
                    )
                    .is_some()
                {
                    return Err(invalid("duplicate DAT path"));
                }
            }
        }
        Ok(Self {
            source,
            file_length,
            entries,
        })
    }

    pub fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.entries.values()
    }

    pub fn entry(&self, path: &str) -> Option<&Entry> {
        self.entries.get(&normalize(path))
    }

    /// Extract a single game-data file, respecting CE's original three modes.
    pub fn read(&mut self, path: &str) -> io::Result<Option<Vec<u8>>> {
        let entry = match self.entry(path) {
            Some(entry) => entry.clone(),
            None => return Ok(None),
        };
        self.source.seek(SeekFrom::Start(entry.offset))?;
        let mode = if entry.flags == 0 {
            0x10
        } else {
            entry.flags & 0xF0
        };
        let decoded = match mode {
            0x10 => {
                let mut compressed = vec![0_u8; entry.packed_length];
                self.source.read_exact(&mut compressed)?;
                lzss::decode(&compressed, entry.length)
                    .map_err(|_| invalid("invalid LZSS entry"))?
            }
            0x20 => {
                let mut raw = vec![0; entry.length];
                self.source.read_exact(&mut raw)?;
                raw
            }
            0x40 => {
                let mut output = Vec::new();
                while output.len() < entry.length {
                    if self.source.stream_position()? + 2 > self.file_length {
                        return Err(invalid("truncated DAT chunk header"));
                    }
                    let chunk = be_u16(&mut self.source)?;
                    let is_raw = chunk & 0x8000 != 0;
                    let count = usize::from(chunk & 0x7FFF);
                    if count == 0
                        || self.source.stream_position()? + count as u64 > self.file_length
                    {
                        return Err(invalid("invalid DAT chunk length"));
                    }
                    let mut bytes = vec![0; count];
                    self.source.read_exact(&mut bytes)?;
                    if is_raw {
                        if bytes.len() > entry.length - output.len() {
                            return Err(invalid("raw DAT chunk exceeds expected file length"));
                        }
                        output.extend_from_slice(&bytes);
                    } else {
                        let available = entry.length - output.len();
                        let part = lzss::decode(&bytes, available)
                            .map_err(|_| invalid("invalid DAT chunk compression"))?;
                        if part.is_empty() {
                            return Err(invalid("empty compressed DAT chunk"));
                        }
                        output.extend_from_slice(&part);
                    }
                }
                output
            }
            _ => return Err(invalid("unsupported DAT entry mode")),
        };
        if decoded.len() != entry.length {
            return Err(invalid("DAT decompressed size mismatch"));
        }
        Ok(Some(decoded))
    }
}

#[cfg(test)]
mod tests {
    use super::{DatArchive, Entry};
    use std::io::Cursor;

    fn be(value: i32) -> [u8; 4] {
        value.to_be_bytes()
    }

    fn archive(flags: i32, plain_size: usize, packed_size: usize, payload: &[u8]) -> Vec<u8> {
        let mut data = Vec::new();
        // One root directory ("ART") with no serialized root value.
        for value in [1, 1, 0, 0] {
            data.extend_from_slice(&be(value));
        }
        data.extend_from_slice(&[3, b'A', b'R', b'T']);
        // One entry ("A.FRM") with 16-byte metadata.
        for value in [1, 1, 16, 0] {
            data.extend_from_slice(&be(value));
        }
        data.extend_from_slice(&[5, b'A', b'.', b'F', b'R', b'M']);
        data.extend_from_slice(&be(flags));
        let offset = (data.len() + 12) as i32;
        data.extend_from_slice(&be(offset));
        data.extend_from_slice(&be(plain_size as i32));
        data.extend_from_slice(&be(packed_size as i32));
        data.extend_from_slice(payload);
        data
    }

    #[test]
    fn raw_entry_and_case_insensitive_separator_lookup() {
        let bytes = archive(0x20, 3, 0, b"ABC");
        let mut dat = DatArchive::open(Cursor::new(bytes)).unwrap();
        assert_eq!(dat.entries().count(), 1);
        assert_eq!(dat.entry(r"art\a.frm").unwrap().path, "ART/A.FRM");
        assert_eq!(dat.read("art/a.frm").unwrap().unwrap(), b"ABC");
        assert_eq!(dat.read("missing.frm").unwrap(), None);
    }

    #[test]
    fn whole_file_lzss_legacy_zero_flags() {
        let packed = [7, b'A', b'B', b'C', 0xEE, 0xF0];
        let bytes = archive(0, 6, packed.len(), &packed);
        let mut dat = DatArchive::open(Cursor::new(bytes)).unwrap();
        assert_eq!(dat.read("ART/A.FRM").unwrap().unwrap(), b"ABCABC");
    }

    #[test]
    fn chunked_raw_and_compressed() {
        let raw = [0x80, 0x03, b'A', b'B', b'C'];
        let mut dat = DatArchive::open(Cursor::new(archive(0x40, 3, 0, &raw))).unwrap();
        assert_eq!(dat.read("art/a.frm").unwrap().unwrap(), b"ABC");
        let compressed = [0x00, 0x06, 7, b'A', b'B', b'C', 0xEE, 0xF0];
        let mut dat = DatArchive::open(Cursor::new(archive(0x40, 6, 0, &compressed))).unwrap();
        assert_eq!(dat.read("art/a.frm").unwrap().unwrap(), b"ABCABC");
    }

    #[test]
    fn truncated_entry_and_bad_metadata_rejected() {
        let mut invalid = archive(0x20, 9, 0, b"A");
        assert!(DatArchive::open(Cursor::new(&invalid)).is_err());
        invalid = archive(0x20, 1, 0, b"A");
        assert!(DatArchive::open(Cursor::new(&invalid)).is_ok());
        invalid[0] = 0xFF;
        assert!(DatArchive::open(Cursor::new(&invalid)).is_err());
    }

    #[test]
    fn entry_metadata_is_public_and_inspectable() {
        let dat = DatArchive::open(Cursor::new(archive(0x20, 1, 0, b"A"))).unwrap();
        let entry: &Entry = dat.entry("art/a.frm").unwrap();
        assert_eq!(entry.length, 1);
        assert_eq!(entry.flags, 0x20);
    }
}
