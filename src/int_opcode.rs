//! Complete numeric inventory of Fallout 1 CE core interpreter opcodes.
//! Generated from pinned CE src/int/intrpret.h enum Opcode and checked
//! against initInterpreter registration in src/int/intrpret.cc.
//! Recognition is NOT execution: script VM work remains outstanding.

pub const FIRST_OPCODE: u16 = 0x8000;
pub const LAST_OPCODE: u16 = 0x804B;
pub const CORE_OPCODE_COUNT: usize = 76;

pub const CORE_OPCODES: [&str; CORE_OPCODE_COUNT] = [
    "OPCODE_NOOP",                             // 0x8000
    "OPCODE_PUSH",                             // 0x8001
    "OPCODE_ENTER_CRITICAL_SECTION",           // 0x8002
    "OPCODE_LEAVE_CRITICAL_SECTION",           // 0x8003
    "OPCODE_JUMP",                             // 0x8004
    "OPCODE_CALL",                             // 0x8005
    "OPCODE_CALL_AT",                          // 0x8006
    "OPCODE_CALL_WHEN",                        // 0x8007
    "OPCODE_CALLSTART",                        // 0x8008
    "OPCODE_EXEC",                             // 0x8009
    "OPCODE_SPAWN",                            // 0x800A
    "OPCODE_FORK",                             // 0x800B
    "OPCODE_A_TO_D",                           // 0x800C
    "OPCODE_D_TO_A",                           // 0x800D
    "OPCODE_EXIT",                             // 0x800E
    "OPCODE_DETACH",                           // 0x800F
    "OPCODE_EXIT_PROGRAM",                     // 0x8010
    "OPCODE_STOP_PROGRAM",                     // 0x8011
    "OPCODE_FETCH_GLOBAL",                     // 0x8012
    "OPCODE_STORE_GLOBAL",                     // 0x8013
    "OPCODE_FETCH_EXTERNAL",                   // 0x8014
    "OPCODE_STORE_EXTERNAL",                   // 0x8015
    "OPCODE_EXPORT_VARIABLE",                  // 0x8016
    "OPCODE_EXPORT_PROCEDURE",                 // 0x8017
    "OPCODE_SWAP",                             // 0x8018
    "OPCODE_SWAPA",                            // 0x8019
    "OPCODE_POP",                              // 0x801A
    "OPCODE_DUP",                              // 0x801B
    "OPCODE_POP_RETURN",                       // 0x801C
    "OPCODE_POP_EXIT",                         // 0x801D
    "OPCODE_POP_ADDRESS",                      // 0x801E
    "OPCODE_POP_FLAGS",                        // 0x801F
    "OPCODE_POP_FLAGS_RETURN",                 // 0x8020
    "OPCODE_POP_FLAGS_EXIT",                   // 0x8021
    "OPCODE_POP_FLAGS_RETURN_EXTERN",          // 0x8022
    "OPCODE_POP_FLAGS_EXIT_EXTERN",            // 0x8023
    "OPCODE_POP_FLAGS_RETURN_VAL_EXTERN",      // 0x8024
    "OPCODE_POP_FLAGS_RETURN_VAL_EXIT",        // 0x8025
    "OPCODE_POP_FLAGS_RETURN_VAL_EXIT_EXTERN", // 0x8026
    "OPCODE_CHECK_PROCEDURE_ARGUMENT_COUNT",   // 0x8027
    "OPCODE_LOOKUP_PROCEDURE_BY_NAME",         // 0x8028
    "OPCODE_POP_BASE",                         // 0x8029
    "OPCODE_POP_TO_BASE",                      // 0x802A
    "OPCODE_PUSH_BASE",                        // 0x802B
    "OPCODE_SET_GLOBAL",                       // 0x802C
    "OPCODE_FETCH_PROCEDURE_ADDRESS",          // 0x802D
    "OPCODE_DUMP",                             // 0x802E
    "OPCODE_IF",                               // 0x802F
    "OPCODE_WHILE",                            // 0x8030
    "OPCODE_STORE",                            // 0x8031
    "OPCODE_FETCH",                            // 0x8032
    "OPCODE_EQUAL",                            // 0x8033
    "OPCODE_NOT_EQUAL",                        // 0x8034
    "OPCODE_LESS_THAN_EQUAL",                  // 0x8035
    "OPCODE_GREATER_THAN_EQUAL",               // 0x8036
    "OPCODE_LESS_THAN",                        // 0x8037
    "OPCODE_GREATER_THAN",                     // 0x8038
    "OPCODE_ADD",                              // 0x8039
    "OPCODE_SUB",                              // 0x803A
    "OPCODE_MUL",                              // 0x803B
    "OPCODE_DIV",                              // 0x803C
    "OPCODE_MOD",                              // 0x803D
    "OPCODE_AND",                              // 0x803E
    "OPCODE_OR",                               // 0x803F
    "OPCODE_BITWISE_AND",                      // 0x8040
    "OPCODE_BITWISE_OR",                       // 0x8041
    "OPCODE_BITWISE_XOR",                      // 0x8042
    "OPCODE_BITWISE_NOT",                      // 0x8043
    "OPCODE_FLOOR",                            // 0x8044
    "OPCODE_NOT",                              // 0x8045
    "OPCODE_NEGATE",                           // 0x8046
    "OPCODE_WAIT",                             // 0x8047
    "OPCODE_CANCEL",                           // 0x8048
    "OPCODE_CANCEL_ALL",                       // 0x8049
    "OPCODE_START_CRITICAL",                   // 0x804A
    "OPCODE_END_CRITICAL",                     // 0x804B
];

/// Map exact CE numeric interpreter opcode to its original symbolic name.
pub fn opcode_name(value: u16) -> Option<&'static str> {
    if !(FIRST_OPCODE..=LAST_OPCODE).contains(&value) {
        return None;
    }
    Some(CORE_OPCODES[usize::from(value - FIRST_OPCODE)])
}

#[cfg(test)]
mod tests {
    use super::{opcode_name, CORE_OPCODE_COUNT, FIRST_OPCODE, LAST_OPCODE};

    #[test]
    fn every_registered_core_opcode_is_addressable_in_ce_order() {
        assert_eq!(
            usize::from(LAST_OPCODE - FIRST_OPCODE) + 1,
            CORE_OPCODE_COUNT
        );
        for value in FIRST_OPCODE..=LAST_OPCODE {
            assert!(opcode_name(value).is_some(), "missing {value:#06X}");
        }
        assert_eq!(opcode_name(0x8000), Some("OPCODE_NOOP"));
        assert_eq!(opcode_name(0x8001), Some("OPCODE_PUSH"));
        assert_eq!(opcode_name(0x8039), Some("OPCODE_ADD"));
        assert_eq!(opcode_name(0x804B), Some("OPCODE_END_CRITICAL"));
        assert_eq!(opcode_name(0x7FFF), None);
        assert_eq!(opcode_name(0x804C), None);
    }
}
