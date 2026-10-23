//! Opt-in, read-only Fallout 1 CE object-property native opcode subset.
//! Pinned src/int/support/intextra.cc: op_tile_num (0x80D4),
//! op_elevation (0x80EC), op_obj_pid (0x8100), and op_obj_art_fid (0x8149).
//! The original reads an Object* and pushes an integer property; this Rust
//! adapter only reads caller-owned snapshots through opaque, non-address
//! handles. No host pointer can be dereferenced and no world state changes.
use crate::script_vm::{ScriptValue, ScriptVm};
use std::io;

pub const TILE_CONTAINS_OBJ_PID_OPCODE: u16 = 0x80BB;
pub const TILE_NUM_OPCODE: u16 = 0x80D4;
pub const ELEVATION_OPCODE: u16 = 0x80EC;
pub const OBJ_PID_OPCODE: u16 = 0x8100;
pub const OBJ_ART_FID_OPCODE: u16 = 0x8149;

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

/// Detached CE object fields supplied explicitly by the caller.
/// A handle is an arbitrary integer token, never a native address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptObjectSnapshot {
    pub handle: i32,
    pub tile: i32,
    pub elevation: i32,
    /// Which CE objectTable[] contains the object; this can differ from
    /// the serialized header elevation read by op_elevation.
    pub table_elevation: usize,
    pub pid: i32,
    pub fid: i32,
}

/// Reproduce CE's null-object fallbacks: tile=-1, elevation=0,
/// PID=-1, art FID=0. A non-null, unbound or ambiguous opaque handle
/// is rejected safely rather than treated as a real C++ pointer.
/// The popped pointer is replaced by the integer result only on success.
pub fn apply_object_read_opcode(
    objects: &[ScriptObjectSnapshot],
    opcode: u16,
    stack: &mut Vec<ScriptValue>,
) -> io::Result<()> {
    if !matches!(
        opcode,
        TILE_NUM_OPCODE | ELEVATION_OPCODE | OBJ_PID_OPCODE | OBJ_ART_FID_OPCODE
    ) {
        return Err(invalid("unknown CE object-property opcode"));
    }
    let Some(&ScriptValue::Pointer(handle)) = stack.last() else {
        return Err(invalid("CE object-property opcode expects pointer operand"));
    };
    let property = if handle == 0 {
        match opcode {
            TILE_NUM_OPCODE | OBJ_PID_OPCODE => -1,
            ELEVATION_OPCODE | OBJ_ART_FID_OPCODE => 0,
            _ => unreachable!(),
        }
    } else {
        let mut matches = objects.iter().filter(|object| object.handle == handle);
        let object = matches
            .next()
            .ok_or_else(|| invalid("CE object handle not present in snapshot"))?;
        if matches.next().is_some() {
            return Err(invalid("ambiguous CE object snapshot handle"));
        }
        match opcode {
            TILE_NUM_OPCODE => object.tile,
            ELEVATION_OPCODE => object.elevation,
            OBJ_PID_OPCODE => object.pid,
            OBJ_ART_FID_OPCODE => object.fid,
            _ => unreachable!(),
        }
    };
    stack.pop();
    stack.push(ScriptValue::Int(property));
    Ok(())
}

/// Pinned intextra.cc op_tile_contains_obj_pid (0x80BB) pops PID,
/// elevation and tile (in that order), scans the given elevation's
/// top-level obj_find_first_at/next list and pushes integer 1/0.
/// No callback, engine pointer, or child inventory traversal occurs.
pub fn apply_tile_contains_obj_pid(
    objects: &[ScriptObjectSnapshot],
    opcode: u16,
    stack: &mut Vec<ScriptValue>,
) -> io::Result<()> {
    if opcode != TILE_CONTAINS_OBJ_PID_OPCODE {
        return Err(invalid("unsupported CE tile-contains native opcode"));
    }
    if stack.len() < 3 {
        return Err(invalid("CE tile-contains requires tile, elevation and PID"));
    }
    let at = stack.len() - 3;
    let (ScriptValue::Int(tile), ScriptValue::Int(elevation), ScriptValue::Int(pid)) =
        (stack[at], stack[at + 1], stack[at + 2])
    else {
        return Err(invalid("CE tile-contains operands must be integers"));
    };
    let found = usize::try_from(elevation).is_ok_and(|e| {
        objects
            .iter()
            .any(|object| object.table_elevation == e && object.tile == tile && object.pid == pid)
    });
    stack.truncate(at);
    stack.push(ScriptValue::Int(i32::from(found)));
    Ok(())
}

/// Detached synthetic bytecode only. A plain ScriptVm still rejects these
/// native opcodes; installing a MAP does not execute them.
pub fn run_object_read_subset_bounded(
    objects: &[ScriptObjectSnapshot],
    vm: &mut ScriptVm,
    max_steps: usize,
) -> io::Result<usize> {
    vm.run_variable_subset_bounded(max_steps, &mut |opcode, stack| {
        if opcode == TILE_CONTAINS_OBJ_PID_OPCODE {
            apply_tile_contains_obj_pid(objects, opcode, stack)
        } else {
            apply_object_read_opcode(objects, opcode, stack)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{
        apply_object_read_opcode, apply_tile_contains_obj_pid, run_object_read_subset_bounded,
        ScriptObjectSnapshot, ELEVATION_OPCODE, OBJ_ART_FID_OPCODE, OBJ_PID_OPCODE,
        TILE_CONTAINS_OBJ_PID_OPCODE, TILE_NUM_OPCODE,
    };
    use crate::script_program::ScriptProgram;
    use crate::script_vm::{ScriptValue, ScriptVm};

    const OBJECT: ScriptObjectSnapshot = ScriptObjectSnapshot {
        handle: 77,
        tile: 20100,
        elevation: 2,
        table_elevation: 0,
        pid: 0x1000_0123,
        fid: 0x3400_0456,
    };

    #[test]
    fn ce_four_object_properties_and_null_fallbacks() {
        for (opcode, expected, null_value) in [
            (TILE_NUM_OPCODE, OBJECT.tile, -1),
            (ELEVATION_OPCODE, OBJECT.elevation, 0),
            (OBJ_PID_OPCODE, OBJECT.pid, -1),
            (OBJ_ART_FID_OPCODE, OBJECT.fid, 0),
        ] {
            let mut stack = vec![ScriptValue::Pointer(77)];
            apply_object_read_opcode(&[OBJECT], opcode, &mut stack).unwrap();
            assert_eq!(stack, [ScriptValue::Int(expected)]);
            let mut null = vec![ScriptValue::Pointer(0)];
            apply_object_read_opcode(&[], opcode, &mut null).unwrap();
            assert_eq!(null, [ScriptValue::Int(null_value)]);
        }
    }

    #[test]
    fn rejected_pointers_and_unsupported_opcodes_preserve_stack() {
        for (objects, opcode, original) in [
            (&[][..], ELEVATION_OPCODE, vec![ScriptValue::Pointer(77)]),
            (
                &[OBJECT, OBJECT][..],
                ELEVATION_OPCODE,
                vec![ScriptValue::Pointer(77)],
            ),
            (&[OBJECT][..], ELEVATION_OPCODE, vec![ScriptValue::Int(77)]),
            (&[OBJECT][..], TILE_NUM_OPCODE, Vec::new()),
            (&[OBJECT][..], 0x80ED, vec![ScriptValue::Pointer(77)]),
        ] {
            let mut stack = original.clone();
            assert!(apply_object_read_opcode(objects, opcode, &mut stack).is_err());
            assert_eq!(stack, original);
        }
    }

    #[test]
    fn tile_contains_uses_object_table_elevation_not_serialized_field() {
        let objects = [
            OBJECT,
            ScriptObjectSnapshot {
                handle: 78,
                tile: 20100,
                elevation: 0,
                table_elevation: 2,
                pid: 0x1000_0123,
                fid: 0,
            },
        ];
        for (tile, elevation, pid, expected) in [
            (20100, 0, 0x1000_0123, 1),
            (20100, 2, 0x1000_0123, 1),
            (20100, 1, 0x1000_0123, 0),
            (20100, -1, 0x1000_0123, 0),
            (20101, 0, 0x1000_0123, 0),
            (20100, 0, 0x1000_0124, 0),
        ] {
            let mut stack = vec![
                ScriptValue::Int(tile),
                ScriptValue::Int(elevation),
                ScriptValue::Int(pid),
            ];
            apply_tile_contains_obj_pid(&objects, TILE_CONTAINS_OBJ_PID_OPCODE, &mut stack)
                .unwrap();
            assert_eq!(stack, [ScriptValue::Int(expected)]);
        }
        let mut invalid = vec![
            ScriptValue::Pointer(1),
            ScriptValue::Int(0),
            ScriptValue::Int(0),
        ];
        let result = apply_tile_contains_obj_pid(&objects, TILE_CONTAINS_OBJ_PID_OPCODE, &mut invalid);
        assert!(result.is_err());
        assert_eq!(invalid[0], ScriptValue::Pointer(1));
    }

    #[test]
    fn native_object_reads_require_opt_in_vm_host() {
        let mut code = Vec::new();
        code.extend_from_slice(&0xE001_u16.to_be_bytes());
        code.extend_from_slice(&77_i32.to_be_bytes());
        code.extend_from_slice(&OBJ_PID_OPCODE.to_be_bytes());
        code.extend_from_slice(&0x8011_u16.to_be_bytes());
        let mut bytes = vec![0_u8; 42];
        bytes[..code.len()].copy_from_slice(&code);
        bytes.extend_from_slice(&[0_u8; 12]);
        let program = ScriptProgram::decode(&bytes).unwrap();
        let mut vm = ScriptVm::new(program.clone());
        assert_eq!(
            run_object_read_subset_bounded(&[OBJECT], &mut vm, 8).unwrap(),
            3
        );
        assert_eq!(vm.stack(), [ScriptValue::Int(OBJECT.pid)]);
        assert!(vm.halted());
        let mut ungated = ScriptVm::new(program);
        ungated.step().unwrap();
        assert!(ungated.step().is_err());
    }
}
