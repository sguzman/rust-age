//! Fallout 1 CE make_path_func search, with a read-only collision callback.
//! Reference: pinned CE src/game/anim.cc (make_path_func, EST, idist).
//!
//! Original implementation uses 2000 open and closed slots, marks a tile
//! visited upon enqueue, scans slots in insertion order for lowest f, applies
//! a 10-point rotation-change penalty outside combat and stores at most 800
//! backward rotations. These are preserved for CE behavioral comparison.
//!
//! Callback returns true when a tile is passable (doors may be permitted by
//! the caller). Actor occupancy, door logic, combat state and movement are
//! not implemented here.
use crate::hex::HexViewport;

const NODE_CAP: usize = 2_000;
const ROTATION_CAP: usize = 800;
const GRID_TILES: usize = 200 * 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Node {
    tile: i32,
    parent_tile: i32,
    rotation: u8,
    estimate: i32,
    cost: i32,
}

/// CE idist, not ordinary Manhattan or Euclidean distance.
pub fn screen_distance(x1: i32, y1: i32, x2: i32, y2: i32) -> i32 {
    let dx = (i64::from(x2) - i64::from(x1)).abs();
    let dy = (i64::from(y2) - i64::from(y1)).abs();
    (dx + dy - dx.min(dy) / 2).min(i64::from(i32::MAX)) as i32
}

fn estimate(view: &HexViewport, from: i32, to: i32) -> Option<i32> {
    let (fx, fy) = view.tile_coord(from)?;
    let (tx, ty) = view.tile_coord(to)?;
    Some(screen_distance(fx, fy, tx, ty))
}

/// Return a sequence of CE rotations 0..5 from source to destination.
/// A blocked destination is rejected only when `check_destination` is true,
/// matching CE's a5 flag. CE skips blocker checks on the destination during
/// neighbor expansion. The passable predicate can permit openable doors.
pub fn make_path<F>(
    view: &HexViewport,
    from: i32,
    to: i32,
    not_in_combat: bool,
    check_destination: bool,
    mut passable: F,
) -> Option<Vec<u8>>
where
    F: FnMut(i32) -> bool,
{
    if !((0..GRID_TILES as i32).contains(&from) && (0..GRID_TILES as i32).contains(&to)) {
        return None;
    }
    if check_destination && !passable(to) {
        return None;
    }
    let initial = Node {
        tile: from,
        parent_tile: -1,
        rotation: 0,
        estimate: estimate(view, from, to)?,
        cost: 0,
    };
    let mut visited = vec![false; GRID_TILES];
    visited[from as usize] = true;
    let mut open = vec![None; NODE_CAP];
    let mut closed: Vec<Node> = Vec::new();
    open[0] = Some(initial);
    let mut open_count = 1_usize;
    let mut result = None;

    while open_count != 0 {
        // CE finds the minimum (estimate + cost), preserving array order
        // when tied. It does not reopen already-seen tiles.
        let mut chosen = None;
        for (idx, candidate) in open.iter().enumerate() {
            let Some(node) = candidate else { continue };
            if chosen.is_none_or(|(_, previous): (usize, Node)| {
                node.estimate + node.cost < previous.estimate + previous.cost
            }) {
                chosen = Some((idx, *node));
            }
        }
        let (slot, current) = chosen?;
        open[slot] = None;
        open_count -= 1;
        if current.tile == to {
            result = Some(current);
            break;
        }
        if closed.len() == NODE_CAP - 1 {
            return None;
        }
        closed.push(current);

        for direction in 0_u8..6 {
            let neighbor = view.move_in_direction(current.tile, usize::from(direction), 1)?;
            if visited[neighbor as usize] {
                continue;
            }
            if neighbor != to && !passable(neighbor) {
                continue;
            }
            if open_count == NODE_CAP - 1 {
                return None;
            }
            let free_slot = open.iter().position(Option::is_none)?;
            let mut cost = current.cost + 50;
            if not_in_combat && current.rotation != direction {
                cost += 10;
            }
            open[free_slot] = Some(Node {
                tile: neighbor,
                parent_tile: current.tile,
                rotation: direction,
                estimate: estimate(view, neighbor, to)?,
                cost,
            });
            visited[neighbor as usize] = true;
            open_count += 1;
        }
    }

    let mut node = result?;
    let mut reverse = Vec::new();
    while node.tile != from && reverse.len() < ROTATION_CAP {
        reverse.push(node.rotation);
        node = *closed.iter().find(|parent| parent.tile == node.parent_tile)?;
    }
    reverse.reverse();
    Some(reverse)
}

#[cfg(test)]
mod tests {
    use super::{make_path, screen_distance};
    use crate::hex::HexViewport;
    use std::collections::HashSet;

    fn view() -> HexViewport {
        HexViewport::new(200, 200, 640, 480, 20099).unwrap()
    }

    fn walk(view: &HexViewport, start: i32, rotations: &[u8]) -> Vec<i32> {
        let mut current = start;
        let mut visited = vec![start];
        for &rotation in rotations {
            current = view.move_in_direction(current, usize::from(rotation), 1).unwrap();
            visited.push(current);
        }
        visited
    }

    #[test]
    fn ce_screen_distance_uses_half_of_smaller_axis() {
        assert_eq!(screen_distance(10, 20, 10, 20), 0);
        assert_eq!(screen_distance(0, 0, 32, 16), 40);
        assert_eq!(screen_distance(100, 100, 60, 20), 100);
    }

    #[test]
    fn neighboring_tiles_use_original_rotation_order() {
        let map = view();
        assert_eq!(make_path(&map, 20099, 20098, true, false, |_| true), Some(vec![1]));
        assert_eq!(make_path(&map, 20099, 20099, true, false, |_| true), Some(vec![]));
    }

    #[test]
    fn occupied_destination_respects_original_flag() {
        let map = view();
        assert_eq!(make_path(&map, 20099, 20098, true, true, |_| false), None);
        assert_eq!(make_path(&map, 20099, 20098, true, false, |_| false), Some(vec![1]));
    }

    #[test]
    fn route_avoids_impassable_tiles_and_reaches_destination() {
        let map = view();
        let blocked = HashSet::from([20098, 20097, 19898]);
        let path = make_path(&map, 20099, 20297, true, false, |t| !blocked.contains(&t)).unwrap();
        assert!(!path.is_empty());
        let tiles = walk(&map, 20099, &path);
        assert_eq!(tiles.last().copied(), Some(20297));
        assert!(tiles.iter().skip(1).all(|tile| !blocked.contains(tile)));
        assert_eq!(path.len(), tiles.len() - 1);
    }

    #[test]
    fn rejects_invalid_tiles_and_fully_enclosed_start() {
        let map = view();
        assert_eq!(make_path(&map, -1, 20098, true, false, |_| true), None);
        assert_eq!(make_path(&map, 20099, 40000, true, false, |_| true), None);
        assert_eq!(make_path(&map, 20099, 20100, true, false, |_| false), Some(vec![3]));
        assert_eq!(make_path(&map, 20099, 30000, true, false, |_| false), None);
    }
}
