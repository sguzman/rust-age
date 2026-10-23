//! CE obj_bound sprite footprint calculation, separate from pixel drawing.
//! Source: pinned Fallout 1 CE src/game/object.cc obj_bound.
//! Uses original FRM orientation offsets and object movement offsets.
//! No lighting, outlines, clipping or draw-order logic is implemented here.
use crate::frm::Frm;
use crate::hex::HexViewport;
use crate::object_data::ObjectHeader;
use std::io;

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpriteBounds {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl SpriteBounds {
    pub fn width(self) -> i64 {
        i64::from(self.right) - i64::from(self.left) + 1
    }

    pub fn height(self) -> i64 {
        i64::from(self.bottom) - i64::from(self.top) + 1
    }

    pub fn compute(
        view: &HexViewport,
        object: &ObjectHeader,
        art: &Frm,
        outlined: bool,
    ) -> io::Result<Self> {
        let direction = usize::try_from(object.rotation)
            .map_err(|_| invalid("negative object sprite rotation"))?;
        let index =
            usize::try_from(object.frame).map_err(|_| invalid("negative object sprite frame"))?;
        let Some(frames) = art.frames.get(direction) else {
            return Err(invalid(
                "object sprite rotation exceeds CE six-direction limit",
            ));
        };
        let Some(frame) = frames.get(index) else {
            return Err(invalid("object sprite frame index out of bounds"));
        };
        let w = i64::from(frame.width);
        let h = i64::from(frame.height);
        if w == 0 || h == 0 {
            return Err(invalid("zero-sized Fallout object sprite"));
        }
        let (left, top) = if object.tile == -1 {
            (i64::from(object.screen_x), i64::from(object.screen_y))
        } else {
            let Some((x, y)) = view.tile_coord(object.tile) else {
                return Err(invalid("object tile outside original 200x200 hex map"));
            };
            let anchor_x =
                i64::from(x) + 16 + i64::from(art.x_offsets[direction]) + i64::from(object.x);
            let anchor_y =
                i64::from(y) + 8 + i64::from(art.y_offsets[direction]) + i64::from(object.y);
            (anchor_x - w / 2, anchor_y - h + 1)
        };
        let expand = i64::from(outlined);
        let to_i32 =
            |value: i64| i32::try_from(value).map_err(|_| invalid("object bounds overflow i32"));
        Ok(Self {
            left: to_i32(left - expand)?,
            top: to_i32(top - expand)?,
            right: to_i32(left + w - 1 + expand)?,
            bottom: to_i32(top + h - 1 + expand)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::SpriteBounds;
    use crate::frm::{Frame, Frm};
    use crate::hex::HexViewport;
    use crate::object_data::ObjectHeader;

    fn fixture() -> (HexViewport, ObjectHeader, Frm) {
        let frame = Frame {
            width: 20,
            height: 30,
            x: 0,
            y: 0,
            pixels: vec![0; 600],
        };
        let frm = Frm {
            version: 4,
            frames_per_second: 10,
            action_frame: 0,
            frame_count: 1,
            x_offsets: [3; 6],
            y_offsets: [-2; 6],
            frames: std::array::from_fn(|_| vec![frame.clone()]),
        };
        let object = ObjectHeader {
            id: 1,
            tile: 20099,
            x: 4,
            y: 5,
            screen_x: 20,
            screen_y: 10,
            frame: 0,
            rotation: 2,
            fid: 0x0100_0000,
            flags: 0,
            elevation: 0,
            pid: 0x0100_0000,
            cid: -1,
            light_distance: 0,
            light_intensity: 0,
            serialized_outline: 0,
            script_id: -1,
            field_80: 0,
        };
        (
            HexViewport::new(200, 200, 640, 380, 20099).unwrap(),
            object,
            frm,
        )
    }

    #[test]
    fn placed_actor_uses_original_hex_anchor_frm_offsets_and_frame_size() {
        let (view, object, frm) = fixture();
        let bounds = SpriteBounds::compute(&view, &object, &frm, false).unwrap();
        assert_eq!(
            (bounds.left, bounds.top, bounds.right, bounds.bottom),
            (317, 164, 336, 193)
        );
        assert_eq!(bounds.width(), 20);
        assert_eq!(bounds.height(), 30);
        let outlined = SpriteBounds::compute(&view, &object, &frm, true).unwrap();
        assert_eq!(
            (outlined.left, outlined.top, outlined.right, outlined.bottom),
            (316, 163, 337, 194)
        );
    }

    #[test]
    fn floating_object_uses_saved_screen_position() {
        let (view, mut object, frm) = fixture();
        object.tile = -1;
        let bounds = SpriteBounds::compute(&view, &object, &frm, false).unwrap();
        assert_eq!(
            (bounds.left, bounds.top, bounds.right, bounds.bottom),
            (20, 10, 39, 39)
        );
    }

    #[test]
    fn invalid_rotation_frame_and_tile_are_rejected() {
        let (view, mut object, frm) = fixture();
        object.rotation = 6;
        assert!(SpriteBounds::compute(&view, &object, &frm, false).is_err());
        object.rotation = 0;
        object.frame = 99;
        assert!(SpriteBounds::compute(&view, &object, &frm, false).is_err());
        object.frame = 0;
        object.tile = 40000;
        assert!(SpriteBounds::compute(&view, &object, &frm, false).is_err());
    }
}
