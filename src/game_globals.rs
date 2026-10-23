//! Rust-owned Fallout 1 game-global (quest flag) integers and stack adapter.
//! Pinned reference: src/game/game.cc game_get_global_var/game_set_global_var,
//! src/int/support/intextra.cc op_global_var/op_set_global_var.
//!
//! GAME_GLOBAL_VARS (data/vault13.gam) is a separate domain from MAP globals.
//! Supports bounded parsing of CE's original GAME_GLOBAL_VARS: defaults.
//! Persistence, native VM opcode hooks and quest effects remain disconnected.
use crate::script_vm::ScriptValue;
use std::io;

pub const GLOBAL_VAR_OPCODE: u16 = 0x80C5;
pub const SET_GLOBAL_VAR_OPCODE: u16 = 0x80C6;
const MAX_GAME_GLOBALS: usize = 1_000_000;
const MAX_STACK_VALUES: usize = 0x800;

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameGlobalState {
    values: Vec<i32>,
}

impl GameGlobalState {
    pub fn from_initial_values(values: &[i32]) -> io::Result<Self> {
        if values.len() > MAX_GAME_GLOBALS {
            return Err(invalid("CE game global variable count exceeds Rust bound"));
        }
        Ok(Self {
            values: values.to_vec(),
        })
    }

    /// Decode the original GAME_GLOBAL_VARS section in data/vault13.gam.
    /// Mirrors game_load_info_vars: match the first 16 bytes of the section
    /// marker, skip comment lines beginning // and empty LF lines, remove
    /// semicolon comments, and parse the decimal prefix following '='.
    /// Rows without '=' become zero and still consume an index. Parsing
    /// continues until EOF, as in the pinned CE implementation.
    ///
    /// This safely rejects oversized lines/counts and i32 overflow; an
    /// invalid sscanf field becomes zero rather than uninitialized memory.
    /// No game assets are embedded or executed by this decoder.
    pub fn decode_vault13_defaults(bytes: &[u8]) -> io::Result<Self> {
        let mut found_section = false;
        let mut values = Vec::new();
        for raw_line in bytes.split_inclusive(|&ch| ch == b'\n') {
            if raw_line.len() > 258 {
                return Err(invalid("oversized CE vault13.gam variable line"));
            }
            // CE's fgets null-terminates C strings. Embedded NUL bytes
            // therefore end the visible part of a line.
            let line = raw_line.split(|&ch| ch == 0).next().unwrap_or_default();
            if !found_section {
                if line.starts_with(b"GAME_GLOBAL_VARS") {
                    found_section = true;
                }
                continue;
            }
            if line.is_empty() || line[0] == b'\n' || line.starts_with(b"//") {
                continue;
            }
            if values.len() == MAX_GAME_GLOBALS {
                return Err(invalid("CE game globals exceed Rust allocation bound"));
            }
            let uncommented = line.split(|&ch| ch == b';').next().unwrap_or_default();
            let value = match uncommented.iter().position(|&ch| ch == b'=') {
                Some(at) => Self::parse_decimal_prefix(&uncommented[at + 1..])?,
                None => 0,
            };
            values.push(value);
        }
        Ok(Self { values })
    }

    fn parse_decimal_prefix(bytes: &[u8]) -> io::Result<i32> {
        let mut data = bytes;
        while data.first().is_some_and(u8::is_ascii_whitespace) {
            data = &data[1..];
        }
        let sign = match data.first() {
            Some(b'-') => {
                data = &data[1..];
                -1_i64
            }
            Some(b'+') => {
                data = &data[1..];
                1_i64
            }
            _ => 1_i64,
        };
        let mut number = 0_i64;
        let mut digits = 0;
        for &byte in data {
            if !byte.is_ascii_digit() {
                break;
            }
            number = number
                .checked_mul(10)
                .and_then(|n| n.checked_add(i64::from(byte - b'0')))
                .ok_or_else(|| invalid("overflowed CE game global decimal"))?;
            digits += 1;
        }
        if digits == 0 {
            return Ok(0);
        }
        i32::try_from(sign * number).map_err(|_| invalid("overflowed CE game global i32"))
    }

    pub fn values(&self) -> &[i32] {
        &self.values
    }

    /// CE op_global_var returns -1 when no globals were loaded, but
    /// game_get_global_var itself returns 0 for an invalid index.
    pub fn read(&self, index: i32) -> i32 {
        if self.values.is_empty() {
            return -1;
        }
        match usize::try_from(index).ok().and_then(|n| self.values.get(n)) {
            Some(value) => *value,
            None => 0,
        }
    }

    /// CE game_set_global_var reports -1 for invalid indices. The native
    /// op_set_global_var ignores the return code and still consumes operands.
    pub fn write(&mut self, index: i32, value: i32) -> bool {
        let Some(slot) = usize::try_from(index)
            .ok()
            .and_then(|n| self.values.get_mut(n))
        else {
            return false;
        };
        *slot = value;
        true
    }

    /// Isolated synthetic stack transition for two native CE game-global
    /// instructions. This never runs actual INT bytecode or quest handlers.
    /// Both index and write value are required to be integer-tagged.
    ///
    /// Unlike CE's fatal type/underflow path, invalid Rust inputs return
    /// an error before any operands are consumed.
    pub fn apply_opcode(&mut self, opcode: u16, stack: &mut Vec<ScriptValue>) -> io::Result<()> {
        let write = match opcode {
            GLOBAL_VAR_OPCODE => false,
            SET_GLOBAL_VAR_OPCODE => true,
            _ => return Err(invalid("unsupported CE game-global opcode")),
        };
        let count = if write { 2 } else { 1 };
        if stack.len() < count {
            return Err(invalid("CE game-global operand stack underflow"));
        }
        let index_position = stack.len() - count;
        let ScriptValue::Int(index) = stack[index_position] else {
            return Err(invalid("CE game-global index is not an integer"));
        };
        if write {
            let ScriptValue::Int(value) = stack[stack.len() - 1] else {
                return Err(invalid("CE game-global write value is not an integer"));
            };
            self.write(index, value);
            stack.truncate(index_position);
        } else {
            if stack.len() > MAX_STACK_VALUES {
                return Err(invalid("CE game-global stack exceeds Rust bound"));
            }
            let result = self.read(index);
            stack.pop();
            stack.push(ScriptValue::Int(result));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{GameGlobalState, GLOBAL_VAR_OPCODE, SET_GLOBAL_VAR_OPCODE};
    use crate::script_vm::ScriptValue;

    #[test]
    fn parses_original_vault13_global_indices_and_decimal_prefixes() {
        let data = b"PREVIOUS:\nA=777\nGAME_GLOBAL_VARS:\n// comment\n\nGVAR_ONE = 15 ; trailing comment\nGVAR_TWO=-3suffix\nNO_EQUALS\nGVAR_EMPTY=+\nGVAR_BIG= 2147483647\nGVAR_SMALL=-2147483648\n";
        let globals = GameGlobalState::decode_vault13_defaults(data).unwrap();
        assert_eq!(globals.values(), [15, -3, 0, 0, i32::MAX, i32::MIN]);
        assert_eq!(globals.read(1), -3);
        let absent = GameGlobalState::decode_vault13_defaults(b"OTHER:\nX=1\n").unwrap();
        assert!(absent.values().is_empty());
        assert_eq!(absent.read(0), -1);
    }

    #[test]
    fn malformed_vault13_defaults_fail_bounded_without_panic() {
        assert!(
            GameGlobalState::decode_vault13_defaults(b"GAME_GLOBAL_VARS:\nX=2147483648\n",)
                .is_err()
        );
        assert!(
            GameGlobalState::decode_vault13_defaults(b"GAME_GLOBAL_VARS:\nX=-2147483649\n",)
                .is_err()
        );
        let mut data = b"GAME_GLOBAL_VARS:\nX=".to_vec();
        data.extend(std::iter::repeat_n(b'1', 260));
        assert!(GameGlobalState::decode_vault13_defaults(&data).is_err());
    }

    #[test]
    fn ce_game_global_reads_and_writes_are_distinct_from_map_globals() {
        let initial = [12, -17, 0];
        let mut globals = GameGlobalState::from_initial_values(&initial).unwrap();
        let mut stack = vec![ScriptValue::Int(1)];
        globals.apply_opcode(GLOBAL_VAR_OPCODE, &mut stack).unwrap();
        assert_eq!(stack, [ScriptValue::Int(-17)]);
        stack = vec![ScriptValue::Int(1), ScriptValue::Int(50)];
        globals
            .apply_opcode(SET_GLOBAL_VAR_OPCODE, &mut stack)
            .unwrap();
        assert!(stack.is_empty());
        assert_eq!(globals.values(), [12, 50, 0]);
        assert_eq!(initial, [12, -17, 0]);
        assert_eq!(globals.read(-1), 0);
        assert_eq!(globals.read(3), 0);
        assert!(!globals.write(3, 100));
        assert_eq!(globals.values(), [12, 50, 0]);
    }

    #[test]
    fn ce_unloaded_game_globals_return_minus_one_and_discard_writes() {
        let mut globals = GameGlobalState::from_initial_values(&[]).unwrap();
        assert_eq!(globals.read(0), -1);
        let mut stack = vec![ScriptValue::Int(0)];
        globals.apply_opcode(GLOBAL_VAR_OPCODE, &mut stack).unwrap();
        assert_eq!(stack, [ScriptValue::Int(-1)]);
        stack = vec![ScriptValue::Int(0), ScriptValue::Int(40)];
        globals
            .apply_opcode(SET_GLOBAL_VAR_OPCODE, &mut stack)
            .unwrap();
        assert!(stack.is_empty());
        assert!(globals.values().is_empty());
    }

    #[test]
    fn invalid_native_game_global_arguments_preserve_state_and_stack() {
        let mut globals = GameGlobalState::from_initial_values(&[11]).unwrap();
        for (opcode, original) in [
            (0x80C4, vec![ScriptValue::Int(0)]),
            (GLOBAL_VAR_OPCODE, Vec::new()),
            (GLOBAL_VAR_OPCODE, vec![ScriptValue::Pointer(10)]),
            (SET_GLOBAL_VAR_OPCODE, vec![ScriptValue::Int(0)]),
            (
                SET_GLOBAL_VAR_OPCODE,
                vec![ScriptValue::Int(0), ScriptValue::Pointer(4)],
            ),
        ] {
            let mut stack = original.clone();
            assert!(globals.apply_opcode(opcode, &mut stack).is_err());
            assert_eq!(stack, original);
        }
        assert_eq!(globals.values(), [11]);
    }
}
