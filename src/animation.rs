//! Deterministic Fallout 1 FRM animation frame timing.
//!
//! Reference: CE src/game/art.cc art_frame_fps (zero FPS defaults to ten).
//! Pure frame selection from elapsed time: no dependency on window refresh.
//! This is not yet the game's event queue, sound timing or actor scheduler.

use crate::frm::Frm;
use std::io;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnimationTimeline {
    fps: u32,
    frame_count: usize,
}

impl AnimationTimeline {
    /// A zero FPS header uses CE's original 10 FPS fallback. Negative frame
    /// rates cannot schedule forward animation and are rejected explicitly.
    pub fn new(raw_fps: i16, frame_count: usize) -> io::Result<Self> {
        let fps = if raw_fps == 0 { 10 } else { i32::from(raw_fps) };
        if fps <= 0 || frame_count == 0 || frame_count > 4096 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unsupported FRM animation timing",
            ));
        }
        Ok(Self {
            fps: fps as u32,
            frame_count,
        })
    }

    pub fn from_frm(sprite: &Frm) -> io::Result<Self> {
        Self::new(sprite.frames_per_second, sprite.frame_count)
    }

    pub fn frames_per_second(&self) -> u32 {
        self.fps
    }

    /// Return the looped frame at an elapsed monotonic duration. The caller
    /// owns the clock and decides when to present frames; no render-driven
    /// simulation timer exists in this primitive.
    pub fn frame_at(&self, elapsed: Duration) -> usize {
        let number = elapsed.as_nanos() * u128::from(self.fps) / 1_000_000_000_u128;
        (number % self.frame_count as u128) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::AnimationTimeline;
    use std::time::Duration;

    #[test]
    fn loops_with_exact_tenth_second_boundaries() {
        let timeline = AnimationTimeline::new(10, 3).unwrap();
        assert_eq!(timeline.frame_at(Duration::from_millis(0)), 0);
        assert_eq!(timeline.frame_at(Duration::from_millis(99)), 0);
        assert_eq!(timeline.frame_at(Duration::from_millis(100)), 1);
        assert_eq!(timeline.frame_at(Duration::from_millis(299)), 2);
        assert_eq!(timeline.frame_at(Duration::from_millis(300)), 0);
        assert_eq!(timeline.frame_at(Duration::from_secs(60)), 0);
    }

    #[test]
    fn zero_header_fps_uses_ce_default() {
        let timeline = AnimationTimeline::new(0, 6).unwrap();
        assert_eq!(timeline.frames_per_second(), 10);
        assert_eq!(timeline.frame_at(Duration::from_millis(250)), 2);
    }

    #[test]
    fn rejects_empty_or_negative_timing() {
        assert!(AnimationTimeline::new(12, 0).is_err());
        assert!(AnimationTimeline::new(-1, 2).is_err());
        assert!(AnimationTimeline::new(1, 4097).is_err());
    }
}
