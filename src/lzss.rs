//! LZSS decoder translated from Fallout 1 CE src/plib/db/lzss.cc.
//! See docs/PROVENANCE.md. Sustainable Use License 1.0.
//! Pending real-asset differential verification.
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    TruncatedToken,
    OutputLimitExceeded,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TruncatedToken => write!(f, "truncated LZSS token"),
            Self::OutputLimitExceeded => write!(f, "LZSS output limit exceeded"),
        }
    }
}

impl std::error::Error for DecodeError {}

/// LSB-first flags: 1 = literal, 0 = 12-bit offset + 4-bit match length (+3).
pub fn decode(input: &[u8], max_output: usize) -> Result<Vec<u8>, DecodeError> {
    const START: usize = 4078;
    let mut ring = [0_u8; 4096];
    ring[..START].fill(b' ');
    let mut head = START;
    let mut pos = 0usize;
    let mut out = Vec::new();
    while pos < input.len() {
        let flags = input[pos];
        pos += 1;
        for bit in 0..8 {
            if pos == input.len() {
                break;
            }
            if ((flags >> bit) & 1) != 0 {
                if out.len() >= max_output {
                    return Err(DecodeError::OutputLimitExceeded);
                }
                let byte = input[pos];
                pos += 1;
                out.push(byte);
                ring[head] = byte;
                head = (head + 1) & 0xFFF;
            } else {
                if input.len() - pos < 2 {
                    return Err(DecodeError::TruncatedToken);
                }
                let low = usize::from(input[pos]);
                let high = usize::from(input[pos + 1]);
                pos += 2;
                let offset = low | ((high & 0xF0) << 4);
                let count = (high & 0x0F) + 3;
                if count > max_output.saturating_sub(out.len()) {
                    return Err(DecodeError::OutputLimitExceeded);
                }
                for i in 0..count {
                    let byte = ring[(offset + i) & 0xFFF];
                    out.push(byte);
                    ring[head] = byte;
                    head = (head + 1) & 0xFFF;
                }
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{decode, DecodeError};
    #[test]
    fn empty() { assert_eq!(decode(&[], 32).unwrap(), b""); }
    #[test]
    fn literals() { assert_eq!(decode(&[7, b'A', b'B', b'C'], 32).unwrap(), b"ABC"); }
    #[test]
    fn backreference() {
        assert_eq!(decode(&[7, b'A', b'B', b'C', 0xEE, 0xF0], 32).unwrap(), b"ABCABC");
    }
    #[test]
    fn truncated() { assert_eq!(decode(&[0, 0xEE], 32), Err(DecodeError::TruncatedToken)); }
    #[test]
    fn bounded() { assert_eq!(decode(&[7, b'A', b'B', b'C'], 2), Err(DecodeError::OutputLimitExceeded)); }
}
