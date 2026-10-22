//! Fallout 1 CE .MSG localized text fields.
//!
//! Source: src/game/message.cc. Braced triples are {id}{audio}{text}, and
//! CE discards LF inside fields, replaces duplicate numeric IDs, and retains
//! sorted entries. Bytes remain in their original game encoding.

use std::collections::BTreeMap;
use std::io;

const MAX_FIELD_BYTES: usize = 1023;

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub number: i32,
    pub audio: Vec<u8>,
    pub text: Vec<u8>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Catalog {
    entries: BTreeMap<i32, Message>,
}

fn next_field(bytes: &[u8], cursor: &mut usize) -> io::Result<Option<Vec<u8>>> {
    while *cursor < bytes.len() {
        let ch = bytes[*cursor];
        *cursor += 1;
        if ch == b'}' {
            return Err(invalid("unmatched closing message brace"));
        }
        if ch == b'{' {
            break;
        }
    }
    if *cursor == bytes.len() && bytes.last() != Some(&b'{') {
        return Ok(None);
    }

    let mut field = Vec::new();
    while *cursor < bytes.len() {
        let ch = bytes[*cursor];
        *cursor += 1;
        if ch == b'}' {
            return Ok(Some(field));
        }
        if ch != b'\n' {
            if field.len() >= MAX_FIELD_BYTES {
                return Err(invalid("message field exceeds CE size limit"));
            }
            field.push(ch);
        }
    }
    Err(invalid("unterminated message field"))
}

impl Catalog {
    pub fn decode(bytes: &[u8]) -> io::Result<Self> {
        let mut entries = BTreeMap::new();
        let mut cursor = 0;
        loop {
            let Some(number) = next_field(bytes, &mut cursor)? else {
                break;
            };
            let audio = next_field(bytes, &mut cursor)?
                .ok_or_else(|| invalid("missing message audio field"))?;
            let text = next_field(bytes, &mut cursor)?
                .ok_or_else(|| invalid("missing message text field"))?;
            let number = std::str::from_utf8(&number)
                .map_err(|_| invalid("non-ASCII message number"))?
                .parse::<i32>()
                .map_err(|_| invalid("invalid message number"))?;
            entries.insert(
                number,
                Message {
                    number,
                    audio,
                    text,
                },
            );
        }
        Ok(Self { entries })
    }

    pub fn get(&self, number: i32) -> Option<&Message> {
        self.entries.get(&number)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Message> {
        self.entries.values()
    }
}

#[cfg(test)]
mod tests {
    use super::Catalog;

    #[test]
    fn parses_comment_interleaved_fields_and_duplicate_replacements() {
        let data = b"# comment\n{100}{}{First}\n{5}{voice}{Line\none}\n{100}{}{Replacement}";
        let catalog = Catalog::decode(data).unwrap();
        assert_eq!(catalog.iter().map(|m| m.number).collect::<Vec<_>>(), vec![5, 100]);
        assert_eq!(catalog.get(100).unwrap().text, b"Replacement");
        assert_eq!(catalog.get(5).unwrap().text, b"Lineone");
    }

    #[test]
    fn leaves_non_utf8_text_in_original_encoding() {
        let catalog = Catalog::decode(b"{1}{}{\xE9}").unwrap();
        assert_eq!(catalog.get(1).unwrap().text, [0xE9]);
    }

    #[test]
    fn invalid_triples_and_braces_rejected() {
        assert!(Catalog::decode(b"{9}{}{oops").is_err());
        assert!(Catalog::decode(b"}").is_err());
        assert!(Catalog::decode(b"{10}{}").is_err());
        assert!(Catalog::decode(b"{oops}{}{text}").is_err());
    }
}
