//! Exact integer primitives for Fallout 1 CE's staggered hex grid.
//!
//! Reference: src/game/tile.cc (tile_coord, tile_set_center,
//! tile_num_in_direction and tile_on_edge).
//! Map layer loading, mouse-to-tile masking, collision and travel are pending.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HexViewport {
    grid_width: i32,
    grid_height: i32,
    center_x: i32,
    center_y: i32,
    center_tile: i32,
    offset_x: i32,
    offset_y: i32,
}

impl HexViewport {
    /// Set CE's tile-centered viewport using game/native framebuffer sizes.
    pub fn new(
        grid_width: i32,
        grid_height: i32,
        viewport_width: i32,
        viewport_height: i32,
        center_tile: i32,
    ) -> Option<Self> {
        let size = grid_width.checked_mul(grid_height)?;
        if grid_width <= 1
            || grid_height <= 1
            || viewport_width < 32
            || viewport_height < 16
            || !(0..size).contains(&center_tile)
        {
            return None;
        }
        let mut center_x = grid_width - 1 - center_tile % grid_width;
        let center_y = center_tile / grid_width;
        let mut offset_x = (viewport_width - 32) / 2;
        let offset_y = (viewport_height - 16) / 2;
        if center_x & 1 != 0 {
            center_x -= 1;
            offset_x -= 32;
        }
        Some(Self {
            grid_width,
            grid_height,
            center_x,
            center_y,
            center_tile,
            offset_x,
            offset_y,
        })
    }

    /// Return the CE screen-space upper-left coordinate of a valid hex tile.
    ///
    /// Integer division is intentionally truncating, matching C++ exactly.
    pub fn tile_coord(&self, tile: i32) -> Option<(i32, i32)> {
        let size = self.grid_width * self.grid_height;
        if !(0..size).contains(&tile) {
            return None;
        }
        let col = self.grid_width - 1 - tile % self.grid_width;
        let row = tile / self.grid_width;
        let delta = col - self.center_x;
        let mut x = self.offset_x + 48 * (delta / 2);
        let mut y = self.offset_y + 12 * (delta / -2);
        if col & 1 != 0 {
            if col <= self.center_x {
                x -= 16;
                y += 12;
            } else {
                x += 32;
            }
        }
        let vertical = row - self.center_y;
        x += 16 * vertical;
        y += 12 * vertical;
        Some((x, y))
    }

    pub fn center_tile(&self) -> i32 {
        self.center_tile
    }

    fn on_edge(&self, tile: i32) -> bool {
        tile < self.grid_width
            || tile >= self.grid_width * (self.grid_height - 1)
            || tile % self.grid_width == 0
            || tile % self.grid_width == self.grid_width - 1
    }

    /// CE's hex neighbor step in rotation order 0..5. Movement halts on
    /// the perimeter before taking the next step.
    pub fn move_in_direction(&self, tile: i32, rotation: usize, distance: u32) -> Option<i32> {
        let size = self.grid_width * self.grid_height;
        if !(0..size).contains(&tile) || rotation >= 6 {
            return None;
        }
        let width = self.grid_width;
        let offsets = [
            [-1, width - 1, width, width + 1, 1, -width],
            [-width - 1, -1, width, 1, 1 - width, -width],
        ];
        let mut current = tile;
        for _ in 0..distance {
            if self.on_edge(current) {
                break;
            }
            let parity = (current % width & 1) as usize;
            current += offsets[parity][rotation];
        }
        Some(current)
    }
}

#[cfg(test)]
mod tests {
    use super::HexViewport;

    #[test]
    fn centered_tile_matches_original_640x480_geometry() {
        let view = HexViewport::new(200, 200, 640, 480, 20099).unwrap();
        assert_eq!(view.tile_coord(20099), Some((304, 232)));
        assert_eq!(view.tile_coord(20098), Some((336, 232)));
        assert_eq!(view.tile_coord(20100), Some((288, 244)));
        assert_eq!(view.tile_coord(20299), Some((320, 244)));
    }

    #[test]
    fn odd_center_column_keeps_screen_center() {
        let view = HexViewport::new(200, 200, 640, 480, 20098).unwrap();
        assert_eq!(view.tile_coord(20098), Some((304, 232)));
        assert_eq!(view.center_tile(), 20098);
    }

    #[test]
    fn neighbor_offsets_and_edge_halt_follow_ce_tables() {
        let view = HexViewport::new(200, 200, 640, 480, 20099).unwrap();
        assert_eq!(view.move_in_direction(20099, 0, 1), Some(19898));
        assert_eq!(view.move_in_direction(20100, 0, 1), Some(20099));
        assert_eq!(view.move_in_direction(20099, 4, 1), Some(19900));
    }

    #[test]
    fn bad_dimensions_and_out_of_bounds_are_rejected() {
        assert!(HexViewport::new(0, 200, 640, 480, 0).is_none());
        assert!(HexViewport::new(200, 200, 16, 480, 0).is_none());
        let view = HexViewport::new(200, 200, 640, 480, 20099).unwrap();
        assert_eq!(view.tile_coord(-1), None);
        assert_eq!(view.tile_coord(40000), None);
        assert_eq!(view.move_in_direction(0, 4, 1), Some(0));
        assert_eq!(view.move_in_direction(20099, 6, 1), None);
    }
}
