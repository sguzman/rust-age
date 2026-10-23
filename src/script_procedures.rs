//! Fallout 1 CE's fixed built-in script procedure lookup, without execution.
//!
//! Reference: pinned CE src/game/scripts.cc procTableStrs and
//! scr_build_lookup_table, plus src/int/intrpret.cc interpretFindProcedure.
//! Slots are stable action indices. Missing names remain unbound.
use crate::script_program::ScriptProgram;
use std::io;

pub const SCRIPT_PROC_NAMES: [&str; 24] = [
    "no_p_proc",
    "start",
    "spatial_p_proc",
    "description_p_proc",
    "pickup_p_proc",
    "drop_p_proc",
    "use_p_proc",
    "use_obj_on_p_proc",
    "use_skill_on_p_proc",
    "none_x_bad",
    "none_x_bad",
    "talk_p_proc",
    "critter_p_proc",
    "combat_p_proc",
    "damage_p_proc",
    "map_enter_p_proc",
    "map_exit_p_proc",
    "create_p_proc",
    "destroy_p_proc",
    "none_x_bad",
    "none_x_bad",
    "look_at_p_proc",
    "timed_event_p_proc",
    "map_update_p_proc",
];

/// Reproduce CE scr_build_lookup_table's first-match result for each action.
/// This resolves names only: it never dispatches, loads, or runs a procedure.
pub fn build_lookup_table(program: &ScriptProgram) -> io::Result<[Option<usize>; 24]> {
    let mut procedures = [None; SCRIPT_PROC_NAMES.len()];
    for (action, name) in SCRIPT_PROC_NAMES.iter().enumerate() {
        procedures[action] = program.find_procedure(name.as_bytes())?;
    }
    Ok(procedures)
}

#[cfg(test)]
mod tests {
    use super::{build_lookup_table, SCRIPT_PROC_NAMES};
    use crate::script_program::ScriptProgram;

    fn put(bytes: &mut Vec<u8>, number: i32) {
        bytes.extend_from_slice(&number.to_be_bytes());
    }

    fn fixture(names: &[&[u8]]) -> ScriptProgram {
        let mut bytes = vec![0_u8; 42];
        put(&mut bytes, names.len() as i32);
        let mut names_data = Vec::new();
        for (i, name) in names.iter().enumerate() {
            put(&mut bytes, (4 + names_data.len()) as i32);
            put(&mut bytes, 0); // flags
            put(&mut bytes, 0); // event time
            put(&mut bytes, 0); // condition
            put(&mut bytes, (i * 2) as i32); // synthetic body address
            put(&mut bytes, 0); // arguments
            names_data.extend_from_slice(name);
            names_data.push(0);
        }
        put(&mut bytes, names_data.len() as i32);
        bytes.extend_from_slice(&names_data);
        put(&mut bytes, 0); // no static strings
        ScriptProgram::decode(&bytes).unwrap()
    }

    #[test]
    fn maps_ce_action_slots_with_first_case_insensitive_match() {
        let program = fixture(&[
            b"main",
            b"START",
            b"Map_Exit_P_Proc",
            b"none_x_bad",
            b"NO_P_PROC",
        ]);
        let names = build_lookup_table(&program).unwrap();
        assert_eq!(SCRIPT_PROC_NAMES.len(), 24);
        assert_eq!(names[0], Some(4));
        assert_eq!(names[1], Some(1));
        assert_eq!(names[16], Some(2));
        assert_eq!(names[9], Some(3));
        assert_eq!(names[10], Some(3));
        assert_eq!(names[19], Some(3));
        assert_eq!(names[20], Some(3));
        assert_eq!(names[11], None);
    }

    #[test]
    fn absent_or_empty_program_procedures_are_not_fabricated() {
        let program = fixture(&[]);
        assert!(build_lookup_table(&program)
            .unwrap()
            .iter()
            .all(Option::is_none));
    }

    #[test]
    fn duplicate_procedure_names_select_first_index_as_in_ce() {
        let program = fixture(&[b"start", b"START", b"other"]);
        let indices = build_lookup_table(&program).unwrap();
        assert_eq!(indices[1], Some(0));
    }
}
