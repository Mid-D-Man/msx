// src/lib.rs
//! Thin re-export facade so `tests/roundtrip.rs` and `benches/compare.rs`
//! can `use msx::{compile, decode, parse_scene, render, Scene};` against
//! one crate name instead of importing `msx_ast`/`msx_binary`/
//! `msx_parser`/`msx_render_svg` separately. This crate has no logic of
//! its own — everything here is a straight re-export from the real
//! workspace member crates (see this crate's own `Cargo.toml` for the
//! four dependencies: `msx-ast`, `msx-binary`, `msx-parser`,
//! `msx-render-svg`).
//!
//! This file has been silently overwritten by an unrelated automated
//! process at least twice this session (a "chore: sync project structure
//! from template [skip ci]" commit, apparently from repo-template-sync
//! tooling) — each time replacing this real content with a bare
//! `// Auto-generated stub` and breaking the whole workspace build
//! (`tests/roundtrip.rs` can't find `compile`/`decode`/`parse_scene`/
//! `render` with nothing here to provide them). That's a repo/tooling
//! configuration issue outside this crate, not something fixable from
//! within this file — whatever triggers that sync will likely overwrite
//! this again unless it's disabled or reconfigured for this repo.

pub use msx_ast::Scene;
pub use msx_binary::{compile, decode};
pub use msx_parser::parse_scene;
pub use msx_render_svg::render;
