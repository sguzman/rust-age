//! Read-only Fallout 1 CE .INT script-program tables.
//!
//! Source: pinned CE src/int/intrpret.cc allocateProgram, fetchWord,
//! fetchLong, interpretGetName and interpretGetString, plus intrpret.h
//! Procedure (six i32 fields). This is NOT an interpreter.
//! No original Fallout script bytecode is committed.

use std::io;

const PROGRAM_HEADER_BYTES: usize = 42;
const PROCEDURE_BYTES: usize = 24;
const MAX_PROCEDURES: usize = 16_384;
const MAX_PROGRAM_BYTES: usize = 128 * 1024 * 1024;

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn read_u16(bytes: &[u8], offset: usize) -> io::Result<u16> {
    let end = offset
        .checked_add(2)
        .ok_or_else(|| invalid("CE opcode offset overflow"))?;
    let data = bytes
        .get(offset..end)
        .ok_or_else(|| invalid("truncated CE interpreter opcode"))?;
    Ok(u16::from_be_bytes([data[0], data[1]]))
}

fn read_i32(bytes: &[u8], offset: usize) -> io::Result<i32> {
    let end = offset
        .checked_add(4)
        .ok_or_else(|| invalid("CE interpreter i32 offset overflow"))?;
    let data = bytes
        .get(offset..end)
        .ok_or_else(|| invalid("truncated CE interpreter i32"))?;
    Ok(i32::from_be_bytes([data[0], data[1], data[2], data[3]]))
}

/// Original six-i32 CE `Procedure`, not a native C++ memory pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcedureEntry {
    pub identifier_offset: i32,
    pub flags: i32,
    pub event_time: i32,
    pub condition_offset: i32,
    pub body_offset: i32,
    pub argument_count: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptProgram {
    data: Vec<u8>,
    procedures: Vec<ProcedureEntry>,
    identifiers_offset: usize,
    identifiers_end: usize,
    static_strings_offset: usize,
    static_strings_end: usize,
}

impl ScriptProgram {
    /// Decode CE's 42-byte preamble and procedure/identifier/string
    /// sections without following serialized pointers or running opcodes.
    pub fn decode(bytes: &[u8]) -> io::Result<Self> {
        if bytes.len() > MAX_PROGRAM_BYTES {
            return Err(invalid("CE script exceeds safe program size"));
        }
        let count = read_i32(bytes, PROGRAM_HEADER_BYTES)?;
        let count = usize::try_from(count).map_err(|_| invalid("negative CE procedure count"))?;
        if count > MAX_PROCEDURES {
            return Err(invalid("CE procedure count exceeds safe limit"));
        }
        let procedures_start = PROGRAM_HEADER_BYTES + 4;
        let procedures_end = procedures_start
            .checked_add(
                count
                    .checked_mul(PROCEDURE_BYTES)
                    .ok_or_else(|| invalid("CE procedure table size overflow"))?,
            )
            .ok_or_else(|| invalid("CE procedure table offset overflow"))?;
        if bytes.get(procedures_start..procedures_end).is_none() {
            return Err(invalid("truncated CE procedure table"));
        }
        let mut procedures = Vec::with_capacity(count);
        for index in 0..count {
            let start = procedures_start + index * PROCEDURE_BYTES;
            let mut fields = [0_i32; 6];
            for (i, value) in fields.iter_mut().enumerate() {
                *value = read_i32(bytes, start + i * 4)?;
            }
            procedures.push(ProcedureEntry {
                identifier_offset: fields[0],
                flags: fields[1],
                event_time: fields[2],
                condition_offset: fields[3],
                body_offset: fields[4],
                argument_count: fields[5],
            });
        }

        // CE identifiers points at the 32-bit section size. Relative
        // identifier offsets are measured from that pointer, not +4.
        let identifier_size = read_i32(bytes, procedures_end)?;
        let identifier_size =
            usize::try_from(identifier_size).map_err(|_| invalid("negative CE identifier size"))?;
        let identifiers_offset = procedures_end;
        let identifiers_end = identifiers_offset
            .checked_add(4)
            .and_then(|pos| pos.checked_add(identifier_size))
            .ok_or_else(|| invalid("CE identifier section overflow"))?;
        if bytes.get(identifiers_offset..identifiers_end).is_none() {
            return Err(invalid("truncated CE identifiers"));
        }

        let static_size = read_i32(bytes, identifiers_end)?;
        let static_size =
            usize::try_from(static_size).map_err(|_| invalid("negative CE static string size"))?;
        let static_strings_offset = identifiers_end;
        let static_strings_end = static_strings_offset
            .checked_add(4)
            .and_then(|pos| pos.checked_add(static_size))
            .ok_or_else(|| invalid("CE string section overflow"))?;
        if bytes
            .get(static_strings_offset..static_strings_end)
            .is_none()
        {
            return Err(invalid("truncated CE static strings"));
        }
        Ok(Self {
            data: bytes.to_vec(),
            procedures,
            identifiers_offset,
            identifiers_end,
            static_strings_offset,
            static_strings_end,
        })
    }

    pub fn procedures(&self) -> &[ProcedureEntry] {
        &self.procedures
    }

    /// Original signed 32-bit big-endian immediate at a file offset.
    /// Exposed for the isolated interpreter core; this does not execute code.
    pub fn long_at(&self, file_offset: usize) -> io::Result<i32> {
        read_i32(&self.data, file_offset)
    }

    pub fn file_len(&self) -> usize {
        self.data.len()
    }

    /// Original big-endian 16-bit opcode fetch at an absolute file offset.
    pub fn word_at(&self, file_offset: usize) -> io::Result<u16> {
        read_u16(&self.data, file_offset)
    }

    pub fn procedure_name(&self, index: usize) -> io::Result<&[u8]> {
        let entry = self
            .procedures
            .get(index)
            .ok_or_else(|| invalid("CE procedure index out of bounds"))?;
        let offset = usize::try_from(entry.identifier_offset)
            .map_err(|_| invalid("negative CE identifier offset"))?;
        let absolute = self
            .identifiers_offset
            .checked_add(offset)
            .ok_or_else(|| invalid("identifier offset overflow"))?;
        let source = self
            .data
            .get(absolute..self.identifiers_end)
            .ok_or_else(|| invalid("CE identifier offset out of bounds"))?;
        let nul = source
            .iter()
            .position(|byte| *byte == 0)
            .ok_or_else(|| invalid("unterminated CE procedure name"))?;
        Ok(&source[..nul])
    }

    /// CE static string offsets start just after the 4-byte size header.
    /// Preserve original 8-bit encoding; converting to Unicode is later work.
    pub fn static_string(&self, offset: usize) -> io::Result<&[u8]> {
        let start = self
            .static_strings_offset
            .checked_add(4)
            .and_then(|base| base.checked_add(offset))
            .ok_or_else(|| invalid("CE string offset overflow"))?;
        let data = self
            .data
            .get(start..self.static_strings_end)
            .ok_or_else(|| invalid("CE static string offset out of bounds"))?;
        let end = data
            .iter()
            .position(|byte| *byte == 0)
            .ok_or_else(|| invalid("unterminated CE static string"))?;
        Ok(&data[..end])
    }
}

#[cfg(test)]
mod tests {
    use super::ScriptProgram;

    fn put(b: &mut Vec<u8>, value: i32) {
        b.extend_from_slice(&value.to_be_bytes());
    }

    fn fixture() -> Vec<u8> {
        let mut data = vec![0_u8; 42];
        data[0..2].copy_from_slice(&0x8000_u16.to_be_bytes());
        put(&mut data, 2); // procedures
        for (offset, address, args) in [(4, 12, 0), (9, 28, 2)] {
            for value in [offset, 0, 0, 0, address, args] {
                put(&mut data, value);
            }
        }
        put(&mut data, 9);
        data.extend_from_slice(b"main\0foo\0");
        put(&mut data, 6);
        data.extend_from_slice(b"hello\0");
        data
    }

    #[test]
    fn decodes_ce_program_layout_identifiers_and_strings() {
        let program = ScriptProgram::decode(&fixture()).unwrap();
        assert_eq!(program.procedures().len(), 2);
        assert_eq!(program.procedures()[1].body_offset, 28);
        assert_eq!(program.procedures()[1].argument_count, 2);
        assert_eq!(program.procedure_name(0).unwrap(), b"main");
        assert_eq!(program.procedure_name(1).unwrap(), b"foo");
        assert_eq!(program.static_string(0).unwrap(), b"hello");
        assert_eq!(program.word_at(0).unwrap(), 0x8000);
    }

    #[test]
    fn rejects_truncated_procedure_and_string_tables() {
        assert!(ScriptProgram::decode(&[0_u8; 8]).is_err());
        let fixture = fixture();
        assert!(ScriptProgram::decode(&fixture[..80]).is_err());
        assert!(ScriptProgram::decode(&fixture[..fixture.len() - 1]).is_err());
    }

    #[test]
    fn rejects_bad_string_offsets_and_procedure_indices() {
        let program = ScriptProgram::decode(&fixture()).unwrap();
        assert!(program.procedure_name(2).is_err());
        assert!(program.static_string(6).is_err());
        assert!(program.word_at(usize::MAX).is_err());
    }
}
