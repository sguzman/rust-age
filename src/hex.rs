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

/// The 32x16 hit-mask constructed by CE tile_init, in the same 3 passes.
const fn make_hit_mask() -> [u8; 512] {
    let mut mask = [0_u8; 512];
    let mut pos = 0;
    let mut threshold = 0_i32;
    while threshold < 64 {
        let mut value = 64_i32;
        while value > 0 {
            mask[pos] = if value > threshold { 1 } else { 0 };
            pos += 1;
            value -= 4;
        }
        while value < 64 {
            mask[pos] = if value > threshold { 2 } else { 0 };
            pos += 1;
            value += 4;
        }
        threshold += 16;
    }

    let mut row = 0;
    while row < 8 {
        let mut col = 0;
        while col < 32 {
            mask[pos] = 0;
            pos += 1;
            col += 1;
        }
        row += 1;
    }

    threshold = 0;
    while threshold < 64 {
        let mut value = 0_i32;
        while value < 64 {
            mask[pos] = if value > threshold { 0 } else { 3 };
            pos += 1;
            value += 4;
        }
        while value > 0 {
            mask[pos] = if value > threshold { 0 } else { 4 };
            pos += 1;
            value -= 4;
        }
        threshold += 16;
    }
    mask
}

const TILE_HIT_MASK: [u8; 512] = make_hit_mask();

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

    /// Translate a native framebuffer coordinate to its Fallout 1 hex ID.
    ///
    /// This ports CE tile_num's floor-division and hit-mask corrections.
    /// Returning None means a coordinate outside the loaded hex grid.
    pub fn screen_to_tile(&self, screen_x: i32, screen_y: i32) -> Option<i32> {
        let relative_y = screen_y - self.offset_y;
        let row_band = relative_y.div_euclid(12);
        let relative_x = screen_x - self.offset_x - 16 * row_band;
        let mask_y = relative_y - 12 * row_band;
        let column_band = relative_x.div_euclid(64);
        let mut mask_x = relative_x - 64 * column_band;
        let mut column = 2 * column_band;
        if mask_x >= 32 {
            mask_x -= 32;
            column += 1;
        }
        let mut grid_y = self.center_y + column_band + row_band;
        let mut grid_x = self.center_x + column;
        match TILE_HIT_MASK[(32 * mask_y + mask_x) as usize] {
            2 => {
                grid_x += 1;
                if grid_x & 1 != 0 {
                    grid_y -= 1;
                }
            }
            1 => grid_y -= 1,
            3 => {
                grid_x -= 1;
                if grid_x & 1 == 0 {
                    grid_y += 1;
                }
            }
            4 => grid_y += 1,
            _ => {}
        }
        let index_x = self.grid_width - 1 - grid_x;
        if index_x < 0 || index_x >= self.grid_width || grid_y < 0 || grid_y >= self.grid_height {
            return None;
        }
        Some(self.grid_width * grid_y + index_x)
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
            let parity = ((current % width) & 1) as usize;
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
        for tile in [20099, 20098, 20100, 20299, 16080, 30020] {
            let (x, y) = view.tile_coord(tile).unwrap();
            assert_eq!(
                view.screen_to_tile(x + 16, y + 8),
                Some(tile),
                "failed tile {tile}"
            );
        }
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
        assert_eq!(view.screen_to_tile(-100_000, -100_000), None);
        assert_eq!(view.tile_coord(40000), None);
        assert_eq!(view.move_in_direction(0, 4, 1), Some(0));
        assert_eq!(view.move_in_direction(20099, 6, 1), None);
    }
}
