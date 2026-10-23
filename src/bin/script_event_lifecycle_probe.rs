//! Source-derived synthetic script-event state transitions versus CE C++.
use rust_age::collision::ObjectKey;
use rust_age::object_data::{ObjectHeader, ObjectRecord, ObjectUpdate};
use rust_age::object_tables::{MapObject, ObjectTables};
use rust_age::script_event_state::DetachedScriptEvent;
use rust_age::script_clock::ScriptGameClock;
use rust_age::script_native_vars::ScriptClockContext;
use rust_age::script_object_binding::MapObjectReadBindings;
use rust_age::script_owner_binding::ScriptOwnerResolution;
use std::error::Error;

fn object() -> MapObject {
    MapObject {
        record: ObjectRecord {
            header: ObjectHeader {
                id: 77,
                tile: 20099,
                x: 0,
                y: 0,
                screen_x: 0,
                screen_y: 0,
                frame: 0,
                rotation: 0,
                fid: 1,
                flags: 0,
                elevation: 0,
                pid: 0x0300_0001,
                cid: -1,
                light_distance: 0,
                light_intensity: 0,
                serialized_outline: 0,
                script_id: -1,
                field_80: 0,
            },
            inventory_length: 0,
            inventory_capacity: 0,
            update: ObjectUpdate::Generic {
                flags: 0,
                subtype_fields: vec![],
            },
        },
        inventory: vec![],
    }
}

fn key(value: i32) -> Option<ObjectKey> {
    if value == 0 {
        None
    } else {
        Some(ObjectKey {
            elevation: 0,
            ordinal: (value - 1) as usize,
        })
    }
}

fn probe(parts: &[&str]) -> Result<String, Box<dyn Error>> {
    let values = parts[1..].iter().map(|s| s.parse::<i32>())
        .collect::<Result<Vec<_>, _>>()?;
    let [source, target, owner, fixed, action] = values.as_slice() else {
        return Err("bad event fields".into());
    };
    let tables = ObjectTables {
        elevations: [vec![object(), object()], vec![], vec![]],
        total_objects: 2,
        bytes_consumed: 0,
    };
    let bindings = MapObjectReadBindings::from_tables(&tables)?;
    let resolution = if *owner == 0 {
        ScriptOwnerResolution::NullOwner
    } else {
        ScriptOwnerResolution::Bound {
            key: key(*owner).ok_or("bad owner key")?,
            handle: *owner,
            saved_oid_matches: false,
        }
    };
    let mut event = DetachedScriptEvent::new(resolution);
    event.set_objs(&bindings, key(*source), key(*target))?;
    event.begin_action(*action as usize, Some(*fixed), &bindings)?;
    let clock = ScriptClockContext {
        clock: ScriptGameClock { ticks: 0 },
        last_visit_ticks: None,
    };
    let first = event.snapshot(&bindings, clock)?;
    let a = format!(
        "A:{},{},{},{}",
        first.source.ok_or("no source")?,
        first.target.ok_or("no target")?,
        event.invocation().action(),
        event.invocation().fixed_parameter(),
    );
    event.finish_action()?;
    event.begin_action(23, None, &bindings)?;
    let second = event.snapshot(&bindings, clock)?;
    let b = format!(
        "B:{},{},{},{}",
        second.source.ok_or("no source")?,
        second.target.ok_or("no target")?,
        event.invocation().action(),
        event.invocation().fixed_parameter(),
    );
    Ok(format!("{a} {b}"))
}

fn main() {
    for line in include_str!("../../tests/script_event_lifecycle_cases.txt").lines() {
        let parts = line.split_whitespace().collect::<Vec<_>>();
        if parts.len() != 6 {
            continue;
        }
        println!(
            "{} {}",
            parts[0],
            probe(&parts).unwrap_or_else(|_| "ERR".to_owned())
        );
    }
}
