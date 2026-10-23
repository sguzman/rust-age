//! Explicit-clock, bounded Fallout 1 CE game-time native opcode subset.
//! Pinned src/int/support/intextra.cc op_game_time (0x80EA),
//! op_game_time_in_seconds (0x80EB), op_game_ticks (0x80F2),
//! op_game_time_hour (0x80F6), and op_get_month/day (0x8118/0x8119).
//! Date conversion follows pinned src/game/scripts.cc game_time_date:
//! 365-day years starting December 5, 2161; no leap-day adjustment.
//! No wall clock, event scheduler, actor callback, or game loop runs here.
use crate::script_vm::{ScriptValue, ScriptVm};
use std::io;

pub const GAME_TIME_OPCODE: u16 = 0x80EA;
pub const GAME_TIME_IN_SECONDS_OPCODE: u16 = 0x80EB;
pub const GAME_TICKS_OPCODE: u16 = 0x80F2;
pub const GAME_TIME_HOUR_OPCODE: u16 = 0x80F6;
pub const GET_MONTH_OPCODE: u16 = 0x8118;
pub const GET_DAY_OPCODE: u16 = 0x8119;
const MAX_STACK_VALUES: usize = 0x800;

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptGameClock {
    /// CE's game_time() returns this game-time tick count. One second is
    /// ten ticks; it is NOT a wall-clock millisecond timestamp.
    pub ticks: i32,
}

impl ScriptGameClock {
    /// Original CE game_time_date arithmetic, including its fixed 365-day
    /// year and December origin. This deliberately does not substitute a
    /// real Gregorian calendar, leap years, or wall-clock timestamps.
    /// The returned values are (month 1..12, day 1..31, year).
    pub fn date(self) -> (i32, i32, i32) {
        let elapsed_days = self.ticks / 864_000 + 4;
        let mut year = elapsed_days / 365 + 2161;
        let mut month = 11_usize; // CE starts on December 5.
        let mut day = elapsed_days % 365;
        const DAYS_IN_MONTH: [i32; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
        loop {
            let days = DAYS_IN_MONTH[month];
            if day < days {
                break;
            }
            day -= days;
            month += 1;
            if month == 12 {
                year += 1;
                month = 0;
            }
        }
        (month as i32 + 1, day + 1, year)
    }

    /// Detached original CE native stack operation: time reads push a
    /// 32-bit integer, and game_ticks pops signed seconds, clamps negative
    /// values to zero, then multiplies by ten. This Rust safety boundary
    /// rejects overflowing multiplication rather than exposing C++ UB.
    pub fn apply_opcode(&self, opcode: u16, stack: &mut Vec<ScriptValue>) -> io::Result<()> {
        match opcode {
            GAME_TIME_OPCODE
            | GAME_TIME_IN_SECONDS_OPCODE
            | GAME_TIME_HOUR_OPCODE
            | GET_MONTH_OPCODE
            | GET_DAY_OPCODE => {
                if stack.len() >= MAX_STACK_VALUES {
                    return Err(invalid("CE game-time result exceeds bounded stack"));
                }
                // CE game_time_hour returns hhmm as an integer. Division
                // and remainder truncate toward zero exactly as in C++.
                let value = match opcode {
                    GAME_TIME_OPCODE => self.ticks,
                    GAME_TIME_IN_SECONDS_OPCODE => self.ticks / 10,
                    GAME_TIME_HOUR_OPCODE => {
                        100 * ((self.ticks / 600) / 60 % 24) + (self.ticks / 600) % 60
                    }
                    GET_MONTH_OPCODE => self.date().0,
                    GET_DAY_OPCODE => self.date().1,
                    _ => unreachable!(),
                };
                stack.push(ScriptValue::Int(value));
            }
            GAME_TICKS_OPCODE => {
                let Some(&ScriptValue::Int(seconds)) = stack.last() else {
                    return Err(invalid("CE game_ticks expected signed integer seconds"));
                };
                let converted = seconds
                    .max(0)
                    .checked_mul(10)
                    .ok_or_else(|| invalid("CE game_ticks multiplication overflow"))?;
                stack.pop();
                stack.push(ScriptValue::Int(converted));
            }
            _ => return Err(invalid("unsupported CE game-time opcode")),
        }
        Ok(())
    }

    /// Opt-in native clock host for isolated synthetic INT execution.
    /// Normal ScriptVm::step remains unable to run game-time opcodes.
    pub fn run_bounded(&self, vm: &mut ScriptVm, max_steps: usize) -> io::Result<usize> {
        vm.run_variable_subset_bounded(max_steps, &mut |opcode, stack| {
            self.apply_opcode(opcode, stack)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ScriptGameClock, GAME_TICKS_OPCODE, GAME_TIME_HOUR_OPCODE, GAME_TIME_IN_SECONDS_OPCODE,
        GAME_TIME_OPCODE, GET_DAY_OPCODE, GET_MONTH_OPCODE,
    };
    use crate::script_program::ScriptProgram;
    use crate::script_vm::{ScriptValue, ScriptVm};

    #[test]
    fn source_exact_game_time_tick_second_and_negative_clamp() {
        let clock = ScriptGameClock { ticks: 125 };
        let mut stack = Vec::new();
        clock.apply_opcode(GAME_TIME_OPCODE, &mut stack).unwrap();
        clock
            .apply_opcode(GAME_TIME_IN_SECONDS_OPCODE, &mut stack)
            .unwrap();
        assert_eq!(stack, [ScriptValue::Int(125), ScriptValue::Int(12)]);
        stack.push(ScriptValue::Int(-9));
        clock.apply_opcode(GAME_TICKS_OPCODE, &mut stack).unwrap();
        assert_eq!(stack[2], ScriptValue::Int(0));
        stack.push(ScriptValue::Int(21));
        clock.apply_opcode(GAME_TICKS_OPCODE, &mut stack).unwrap();
        assert_eq!(stack[3], ScriptValue::Int(210));
    }

    #[test]
    fn original_ce_hhmm_hour_handles_morning_evening_and_day_wrap() {
        for (ticks, expected) in [
            (0, 0),
            (288_000, 800),
            (558_000, 1530),
            (863_400, 2359),
            (864_000, 0),
            (1_152_000, 800),
            (-28_800, -48),
        ] {
            let mut stack = Vec::new();
            ScriptGameClock { ticks }
                .apply_opcode(GAME_TIME_HOUR_OPCODE, &mut stack)
                .unwrap();
            assert_eq!(stack, [ScriptValue::Int(expected)], "ticks {ticks}");
        }
    }

    #[test]
    fn ce_date_uses_fixed_december_origin_without_leap_years() {
        for (ticks, date) in [
            (0, (12, 5, 2161)),
            (264_600, (12, 5, 2161)),
            (26 * 864_000, (12, 31, 2161)),
            (27 * 864_000, (1, 1, 2162)),
            (31 * 864_000, (1, 5, 2162)),
            (365 * 864_000, (12, 5, 2162)),
            (-864_000, (12, 4, 2161)),
        ] {
            let clock = ScriptGameClock { ticks };
            assert_eq!(clock.date(), date, "ticks {ticks}");
            let mut stack = Vec::new();
            clock.apply_opcode(GET_MONTH_OPCODE, &mut stack).unwrap();
            clock.apply_opcode(GET_DAY_OPCODE, &mut stack).unwrap();
            assert_eq!(stack, [ScriptValue::Int(date.0), ScriptValue::Int(date.1)]);
        }
    }

    #[test]
    fn unsafe_game_time_inputs_error_without_mutating_stack() {
        let clock = ScriptGameClock { ticks: 12 };
        for (opcode, original) in [
            (GAME_TICKS_OPCODE, Vec::new()),
            (GAME_TICKS_OPCODE, vec![ScriptValue::Pointer(9)]),
            (GAME_TICKS_OPCODE, vec![ScriptValue::Int(i32::MAX)]),
            (0x80F3, vec![ScriptValue::Int(2)]),
        ] {
            let mut stack = original.clone();
            assert!(clock.apply_opcode(opcode, &mut stack).is_err());
            assert_eq!(stack, original);
        }
    }

    #[test]
    fn synthetic_native_time_bytecode_requires_explicit_clock_host() {
        let mut code = Vec::new();
        code.extend_from_slice(&GAME_TIME_IN_SECONDS_OPCODE.to_be_bytes());
        code.extend_from_slice(&0xC001_u16.to_be_bytes());
        code.extend_from_slice(&3_i32.to_be_bytes());
        code.extend_from_slice(&GAME_TICKS_OPCODE.to_be_bytes());
        code.extend_from_slice(&0x8039_u16.to_be_bytes());
        code.extend_from_slice(&0x8011_u16.to_be_bytes());
        let mut bytes = vec![0_u8; 42];
        bytes[..code.len()].copy_from_slice(&code);
        bytes.extend_from_slice(&[0_u8; 12]);
        let program = ScriptProgram::decode(&bytes).unwrap();
        let mut vm = ScriptVm::new(program.clone());
        let clock = ScriptGameClock { ticks: 125 };
        assert_eq!(clock.run_bounded(&mut vm, 8).unwrap(), 5);
        assert_eq!(vm.stack(), [ScriptValue::Int(42)]);
        assert!(vm.halted());
        let mut ungated = ScriptVm::new(program);
        assert!(ungated.step().is_err());
    }
}
