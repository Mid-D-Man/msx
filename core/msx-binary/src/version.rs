// core/msx-binary/src/version.rs
//! The wire version a scene needs.
//!
//! NOTICE: see docs/format-spec.md, section "Wire-format versioning".
//!
//! A file is written with the lowest version that can hold its content, so a
//! scene that uses nothing new stays at `MIN_VERSION` and still opens in an
//! older reader. An encoder that writes bytes an older reader would misread
//! calls `require` with the version that introduced them, and `compile` puts
//! the highest of those in the header. The record is per thread and `compile`
//! resets it first, so one compile never sees another's.

use std::cell::Cell;

use crate::header::MIN_VERSION;

thread_local! {
    static NEEDED: Cell<u8> = const { Cell::new(MIN_VERSION) };
}

/// Starts a record for one compile.
pub(crate) fn begin() {
    NEEDED.with(|n| n.set(MIN_VERSION));
}

/// Notes that the bytes being written need at least `version`.
pub(crate) fn require(version: u8) {
    NEEDED.with(|n| n.set(n.get().max(version)));
}

/// The highest version required since `begin`.
pub(crate) fn needed() -> u8 {
    NEEDED.with(Cell::get)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_highest_requirement_wins_and_begin_resets() {
        begin();
        assert_eq!(needed(), MIN_VERSION);
        require(3);
        require(2);
        assert_eq!(needed(), 3);
        begin();
        assert_eq!(needed(), MIN_VERSION);
    }
}
