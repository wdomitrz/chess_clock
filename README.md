# chess_clock

A two-player chess clock with Fischer or Bronstein increment, in Rust.

The clock runs in the browser and nowhere else: all the logic, state and
rendering is Rust compiled to WebAssembly, and `cargo build` produces the whole
static site into `dist/`. There is no server, no binary and no runtime
dependency on this repository — any file host can serve it.

The device lies flat on the table between the two players, so each clock
reads sideways to the screen and upright to its player. The first tap on a
panel starts the *other* player's clock — tap your own clock to send your
opponent first — and after that, tapping the running player's panel hands the
turn over and pays the increment.

AGPL-3.0-only. See `LICENSE`.

This began as the JavaScript PWA `wdomitrz/chess_clock` and keeps that app's
behaviour, layout and icon; the implementation is now Rust. See `AGENTS.md` for
the contract and the build.

## Build

Two steps, because there are two targets. Neither artefact is committed.

```
rustup target add wasm32-unknown-unknown

# 1. the app: compile to wasm and run the bindings generator
RUSTFLAGS="--cfg=web_sys_unstable_apis" \
  cargo build --locked --lib --target wasm32-unknown-unknown --release
wasm-bindgen --target web --no-typescript --out-dir dist --out-name app \
  target/wasm32-unknown-unknown/release/chess_clock.wasm

# 2. the rest of the site
touch build.rs && cargo build --release --locked
```

Step 1 writes `dist/app.js` and `dist/app_bg.wasm` — the app itself. Step 2
writes the other six files and derives the service worker's cache version from
the bytes of all of them, so **the order matters**: run step 2 first and the
cache is pinned to whatever the previous build left behind.

The `RUSTFLAGS` is not optional. The Screen Wake Lock API — all of it,
including the `WakeLockSentinel` type — sits behind `web_sys_unstable_apis` in
`web-sys` 0.3.105. `build.rs` sets the cfg for the wasm target so `cargo build`
and `cargo test` work unaided; the variable is only needed for the `clippy`
wasm run, which builds the lib outside a `build.rs` pass. See `AGENTS.md`.

`wasm-bindgen` must be exactly `0.2.128`, matching the `=0.2.128` pin in
`Cargo.toml`; a mismatched generator emits bindings the runtime will not load.

Then serve `dist/` with anything:

```
python3 -m http.server --directory dist
```

The site is self-contained and location-independent: every path it references
is relative to the page, and the service worker resolves its own directory from
`self.location`. So the same `dist/` works at a domain root, under a
subdirectory, or on GitHub Pages at `/chess_clock/` — nothing needs rewriting
per host.

## Deploy to GitHub Pages

`.github/workflows/pages.yml` builds the site and deploys it to Pages. It runs
the same two steps, in the same order, as the build above.

Deploys are **automatic on `master`**: every commit that lands there is built
and published, and a push to any other branch builds without publishing. There
is nothing to tag and nothing to click. The `github-pages` environment still
applies the repository's own review and branch rules on top, so a protected
branch can hold the live site back.

Turning the site on in the repository: **Settings → Pages → Source → GitHub
Actions**. The build needs a runner that can produce the wasm; the Actions
runner already has the pinned toolchain.

## Test

```
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
RUSTFLAGS="--cfg=web_sys_unstable_apis" \
  cargo clippy --locked --lib --target wasm32-unknown-unknown -- -D warnings
```

No browser, no Node, no Chromium. The increment rules, the time formatter and
the turn/pause state machine are plain Rust in `src/clock.rs` and are covered
by unit tests; `tests/shell.rs` covers the committed shell and what is not
committed. `.github/workflows/build.yml` builds the site and inspects all eight
files.
