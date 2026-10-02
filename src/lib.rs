// Copyright (c) 2026 Witalis Domitrz <witekdomitrz@gmail.com>
// AGPL License

//! Chess Clock: a two-player clock with Fischer or Bronstein increment, in
//! Rust.
//!
//! The split is the whole point of the rewrite. `clock` is the game — the
//! increment rules, the formatter and the turn/pause machine — and it knows
//! nothing about the browser, so it is all unit-testable under `cargo test`
//! with no web-sys and no page. `ui` is the browser half and exists only on
//! wasm: it reads the form, drives the clock, and paints the two panels.
//!
//! There is no server and no binary. `build.rs` and the `wasm-bindgen` step
//! together write the whole static site into `dist/`.

#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

pub mod clock;

pub use clock::{format_time, Game, Increment, Millis, Phase, Player, Settings};

#[cfg(target_arch = "wasm32")]
mod ui;

/// The entry point: the shell's one loader line imports the bindings, and
/// this runs as the module is instantiated.
///
/// The attribute stays, deliberately. It means the app starts without the
/// page having to find and call anything, and — more importantly — that a
/// panic here surfaces as a rejected import the shell's `.catch` can report,
/// rather than as a silent half-mounted app. What it must *not* do is touch
/// the DOM: instantiation happens before the document is parsed, so every
/// `get_element_by_id` would find nothing.
///
/// So this does no work itself. It hands off to the browser's own readiness
/// signal and mounts from there. The bug this fixes was exactly that
/// confusion: a start function that resolved the shell at instantiation, so
/// the app trapped with `RuntimeError: unreachable` on every load while the
/// form sat there looking fine and every test passed.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    ui::start_when_ready();
}
