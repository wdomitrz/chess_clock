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

/// The entry point the page's one-line loader calls.
///
/// It is `#[wasm_bindgen(start)]` rather than an exported `start()` the shell
/// would have to find and call: the page then does exactly one thing, import
/// the bindings, and the app starts itself. See `ui.html`.
///
/// Nothing here can fail in a way worth reporting — the shell's loader
/// catches a rejected import, and an app that cannot start prints its own
/// message either way.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    ui::mount();
}
