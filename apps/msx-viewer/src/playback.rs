// apps/msx-viewer/src/playback.rs
//! Pure playback-clock logic for live keyframe playback — no winit/pixels/
//! wgpu knowledge at all, so this is fully unit-testable without a real
//! window or event loop. `window.rs` owns the actual `std::time::Instant`;
//! this module only ever sees plain `f64` seconds of elapsed playback
//! time, which is what makes it testable without real wall-clock sleeps.
//!
//! This drives ONE of the project's two independent clocks (see the
//! README's "Animation" section) — `msx-anim`'s keyframe timeline, via
//! `msx_anim::resolve_at_time`, already reused as-is from the bake-to-GIF
//! path (`msx-cli`'s `cmd_animate`). The other clock — `msx-render-gpu`'s
//! shader `time` uniform — isn't driven here; live GPU playback needs its
//! own pass (see `renderer.rs`'s module doc for why: `pixels` bundles its
//! own internal `wgpu`, separate from `msx-render-gpu`'s pinned one, and
//! running both live against one window is a separate piece of work).

use std::time::Duration;

use msx_ast::LoopMode;

/// Fixed viewer-side playback cadence. Live playback deliberately does
/// NOT read the scene's own `--fps`-style export rate (that's a GIF-bake
/// sampling interval, not part of the timeline itself — see `msx-anim`'s
/// own module doc) or attempt to detect the display's real refresh rate.
/// A flat ~60Hz ceiling is the simplest thing that works for a first
/// pass — `about_to_wait`'s `ControlFlow::WaitUntil` scheduling means
/// this is a ceiling the OS is free to coalesce or delay, not a busy
/// loop, so it costs nothing when nothing's actually animating.
pub const FRAME_INTERVAL: Duration = Duration::from_micros(16_667);

/// Whether the viewer should keep scheduling live redraws for a scene
/// whose keyframe timeline has been playing for `elapsed` seconds.
///
/// `Loop`/`PingPong` never settle — a timeline that repeats forever
/// always has a next, different-looking frame to show. `Once` settles
/// the moment `elapsed` reaches `duration`: `resolve_at_time` clamps to
/// the timeline's final pose past that point (see `LoopMode::Once`'s own
/// `resolve_time`), so every further sample would be byte-identical to
/// the one already on screen — continuing to redraw would just burn
/// CPU/battery on a frame that can never change again.
///
/// Once this returns `false`, the caller drops back to today's
/// on-demand-only redraw behaviour — the same path a non-animated scene
/// already takes.
pub fn should_keep_playing(loop_mode: LoopMode, elapsed: f64, duration: f64) -> bool {
    match loop_mode {
        LoopMode::Once => elapsed < duration,
        LoopMode::Loop | LoopMode::PingPong => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn once_keeps_playing_before_duration() {
        assert!(should_keep_playing(LoopMode::Once, 0.0, 2.0));
        assert!(should_keep_playing(LoopMode::Once, 1.999, 2.0));
    }

    #[test]
    fn once_stops_at_or_past_duration() {
        assert!(!should_keep_playing(LoopMode::Once, 2.0, 2.0));
        assert!(!should_keep_playing(LoopMode::Once, 5.0, 2.0));
    }

    #[test]
    fn once_with_zero_duration_never_plays() {
        // A degenerate/instant timeline shouldn't spin the redraw loop
        // forever waiting for a duration it'll never reach.
        assert!(!should_keep_playing(LoopMode::Once, 0.0, 0.0));
    }

    #[test]
    fn loop_never_settles() {
        assert!(should_keep_playing(LoopMode::Loop, 0.0, 2.0));
        assert!(should_keep_playing(LoopMode::Loop, 1_000_000.0, 2.0));
    }

    #[test]
    fn ping_pong_never_settles() {
        assert!(should_keep_playing(LoopMode::PingPong, 0.0, 2.0));
        assert!(should_keep_playing(LoopMode::PingPong, 1_000_000.0, 2.0));
    }

    #[test]
    fn frame_interval_is_roughly_60hz() {
        let hz = 1.0 / FRAME_INTERVAL.as_secs_f64();
        assert!((hz - 60.0).abs() < 0.1, "expected ~60Hz, got {hz}");
    }
}
