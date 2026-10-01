# chess_clock

Chess Clock: a two-player clock with Fischer or Bronstein increment. The
behaviour, layout and icon are the author's, from the JavaScript PWA
`wdomitrz/chess_clock`; the implementation is now Rust, with all logic, state
and rendering in one crate compiled to WebAssembly. `cargo build` generates the
**static PWA** into `./dist` — a front-end-only site any file host can serve,
with no server and no runtime dependency on this repository.

AGPL-3.0-only. See `LICENSE`.

This is a standalone, **private** repository. It was seeded with the history of
the public `wdomitrz/chess_clock`; `upstream` may be read, `origin` is here,
and it is never made public.

## Build and run

Two builds, because there are two targets. Nothing generated is committed.

```
# 1. the app: compile the crate to wasm and run the bindings generator
rustup target add wasm32-unknown-unknown
RUSTFLAGS="--cfg=web_sys_unstable_apis" \
  cargo build --locked --lib --target wasm32-unknown-unknown --release
wasm-bindgen --target web --no-typescript --out-dir dist --out-name app \
  target/wasm32-unknown-unknown/release/chess_clock.wasm

# 2. the rest of the site
touch build.rs && cargo build --release --locked
```

Step 1 writes `dist/app.js` and `dist/app_bg.wasm` — the app, and the only two
files that exist nowhere else in the tree. Step 2 adds the six files
`build.rs` owns and derives the service worker's cache version from the bytes
of all of them. **The order matters**: step 2's cache hash covers the wasm, so
running it first would pin a version to whatever the previous build left
behind.

`build.rs` writes only the files it owns, in place, each under a scratch name
and renamed into place. It does not replace `dist/`, because the wasm-bindgen
step owns two files in there and a wholesale swap would delete them — leaving
a publishable-looking site with no app in it and no error.

The `touch build.rs` is not redundant: the script writes into the source tree
rather than `OUT_DIR`, so Cargo cannot see that anything changed and will not
re-run it for a second, otherwise identical invocation. A fresh CI runner
reproduces the resulting half-built site every time.

There is **no run step, no server and no `[[bin]]`**: this is a browser
application, so the build is the whole story and the crate is a library.

`wasm-bindgen` installs to `~/.cargo/bin`, on `PATH` in a normal login shell;
in a bare or non-login shell call it by absolute path
(`~/.cargo/bin/wasm-bindgen`). The version is pinned to `=0.2.128` in
`Cargo.toml` and must match the CLI exactly — a mismatched generator produces
bindings the runtime will not load, and the page then fails to start with
"Chess Clock could not start". No npm or JS build tool is needed. A tiny
dynamic-import loader in `ui.html`, the small service-worker cache/lifecycle
file, and the generated wasm-bindgen bindings are the *only* JavaScript in the
app: there is no handwritten JavaScript application and no raw-pointer ABI.

### `RUSTFLAGS` and the Wake Lock

The Screen Wake Lock API is what keeps the screen on with the phone lying flat
between two players, and in `web-sys` 0.3.105 **all of it is behind
`#[cfg(web_sys_unstable_apis)]`** — not merely feature-gated:

| item | feature needed | also behind the cfg |
|---|---|---|
| `Navigator::wake_lock()` | `Navigator`, `WakeLock` | yes |
| `WakeLock::request(WakeLockType)` | `WakeLock`, `WakeLockType` | yes |
| the `WakeLockSentinel` type | `WakeLockSentinel` | yes |
| `document.visibilityState` | `Document`, `VisibilityState` | no |

Enabling the four features is necessary and **not** sufficient. With them on
and the cfg off, the build fails with
`cannot find WakeLockSentinel in crate web_sys` and names no feature to add.

So `build.rs` emits `cargo:rustc-cfg=web_sys_unstable_apis` for the wasm
target. This is the only mechanism that actually reaches the crate:

* `RUSTFLAGS` as an environment variable works, but is one thing a builder has
  to remember and one thing a fresh CI runner forgets;
* a `[target.'cfg(...)'.rustflags]` table in `Cargo.toml` is **ignored** for
  non-path dependencies — cargo emits `unused manifest key` and moves on;
* `cargo:rustc-cfg` is the documented, supported way, and is scoped to the
  build script's own target, so the host build and `cargo test` compile without
  the flag and cannot be affected by it.

`RUSTFLAGS` therefore appears above only on the two commands that build the
wasm lib *outside* a `build.rs` pass: the step 1 build (where it is redundant
but harmless, and kept so the command is copy-pasteable) and the wasm `clippy`
run. The flag is `--cfg=web_sys_unstable_apis` in one argument — cargo splits
`RUSTFLAGS` on whitespace, so `-C cfg=web_sys_unstable_apis` fails with
`unknown codegen option: cfg`.

The alternative — binding the one method by hand with `js_sys` — is the
rewrite's stated prohibition on a manual ABI, so it is not taken.

## The static site

`dist/` is the whole form of the app, and nothing in it is committed. The eight
files arrive from two builds:

- `app.js` and `app_bg.wasm` are written by `wasm-bindgen` in step 1. They are
  the app, and they exist nowhere else in the tree.
- The other six come from `build.rs` in step 2, from committed sources:
  `src/ui.html`, `assets/icon.svg`, the two rasterized icons, the assembled
  `manifest.webmanifest`, and `src/service-worker.js` with its cache name
  pinned to a version derived from the bytes of every other file in the
  directory **and its own source** — so a change to the caching logic
  invalidates the cache too, and changing the wasm moves the version.

`build.rs` writes each file under a scratch name and renames it into place, so
a host serving `dist/` never sees a half-written file.

Everything the shell references is relative (`./app.js`,
`new URL('./', self.location.href)`, `start_url: "./"`), so one build works
from any subdirectory. Any file host can publish it: nginx, Caddy, GitHub
Pages, `python3 -m http.server`.

`dist/` is gitignored. It is reproducible: the same sources and the same pinned
toolchain produce the same bytes.

## The icon

`assets/icon.svg` is the author's original, committed **byte for byte** from
the upstream PWA: a Material Symbols "chess" pawn, `#434343`, on a transparent
48px field. It is never deleted, never redrawn, never replaced by a PNG. It is
the source of truth, and the 192 and 512 install PNGs are derived from it at
build time by `usvg` + `resvg` + `tiny-skia` — build output, in `dist/` only,
never committed and never present in the source tree.

`tests/shell.rs` records the upstream file's SHA-256
(`9cdd9adaca7a8a60af11a0d25cb2ccc9d5b4784ff864217ba0dec31f09afc306`) so a
redraw cannot pass unnoticed, and asserts the SVG's provenance strings.

The shell links the SVG directly (`<link rel="icon" href="icon.svg">` and
`rel="apple-touch-icon"` for the PNG); the manifest declares both PNGs with
`"purpose": "any maskable"`.

`tiny-skia` must be `0.12` to match `resvg` `0.48`, and the rasterize scale is
**f32**: `size as f32 / tree.size().width() as f32`. In `0.12`,
`tiny_skia::Pixmap::new` returns an `Option`, not a `Result` — the one call in
the loop that fails without an `Err`.

## The manifest

Assembled in `build.rs` with `serde_json` so its icon list cannot drift from
what was actually rasterized: if an icon is not in the directory it is not in
the manifest either, and the build says so.

The original `manifest.json` carried `"name": "Chess Clock"` and no
`short_name` at all — it had been removed in the commit before last — and no
theme or background colour. Those are therefore *chosen here*, to match the UI
the original shipped, whose page was Tailwind `gray-900`:

| key | value | why |
|---|---|---|
| `name` | `Chess Clock` | the original's, unchanged |
| `short_name` | `Chess` | supplied, as the rewrite requires |
| `display` | `standalone` | the original's, unchanged |
| `theme_color` | `#111827` | the page background, so the browser UI does not flash a different colour |
| `background_color` | `#0b1220` | a slightly darker slate for the install splash, behind the app before it paints |
| `id` / `start_url` / `scope` | `"./"` | so the site mounts anywhere |

The theme colour is the oklch `gray-900` of the original stylesheet
(`oklch(21% 0.034 264.665)`) as hex, and every other colour in `ui.html` is the
oklch equivalent of the same Tailwind v4 palette, converted once and written as
a custom property. The look is the author's; only the 13 KiB of vendored
Tailwind is gone.

## The app

**Setup.** Hours, minutes and seconds (10:00:00 by default) and an increment
type — Fischer or Bronstein — with its own minutes and seconds. The two
increment fields are *disabled and dimmed when the increment is zero*, because
a select that silently does nothing is a bug report waiting to happen. This is
the one behaviour the rewrite adds.

**The game.** Two full-height panels, each showing its remaining time rotated
90°, so that with the phone lying flat on the table between them each player
reads their own clock the right way up. That rotation is why the readings are
rotated and the Pause/Back buttons are too, and why the panels are the only
large touch targets on the screen.

**The rules**, all in `src/clock.rs` and all unit-tested:

- The first tap on a panel starts the **other** player's clock. This is
  counter-intuitive, so it is worth saying why, and worth saying where it
  came from: the original's two handlers each opened the game on the *other*
  panel —

  ```js
  playerDivs[0].addEventListener("click", () => {
    if (!isRunning && currentPlayer === null) {
      currentPlayer = 1; // Start with Player 1's timer
  ```

  — and that is every version of `app.js` in this repository's history,
  `71a743b` through `4d5dff6`. With the device lying flat, the top panel is
  nearest one player and the bottom nearest the other, so tapping *your own*
  clock is how you tell your opponent to go first: the two panels are
  unlabelled, so this cross-wiring is the only thing that says who opens.
  The panels show no active highlight while armed for the same reason.

  This rewrite shipped the opposite rule and documented it as a port
  decision, with a unit test pinning it — the failure this file already
  records twice, below and under "Orientation": an invariant invented to
  make a behaviour look principled while the thing being ported sat unread
  in the repository's own history.
- After that, only a tap on the **running** panel is a move. A tap on the
  waiting panel does nothing, so the increment cannot be farmed.
- **Fischer** pays the full increment to the player who just moved, however
  long the move took. **Bronstein** pays `min(time spent, increment)`, capped
  at the time that move *began* with, so a long think costs nothing beyond the
  increment and a short move is not paid the full amount.
- Reaching zero turns the panel red and stops the game for good.
- Pause/Resume keeps the running player's highlight; Back discards the game,
  including a flagged one, so no panel is left red.

**Formatting**, exactly the original's three branches: `h:mm:ss` when there are
whole hours, `m:ss` when there are whole minutes, and `s.d` below a minute —
one decimal, truncated and zero-padded to four characters, so a 9.8-second
clock reads `09.8` and does not jitter in width. The tenth truncates rather
than rounds, because rounding would show `00.0` before the player has actually
run out.

The display refreshes every 100 ms, as the original did. The tick does not
decrement a counter: it recomputes the running clock from an anchor — the
remaining time and the timestamp at the last repaint — so a dropped, delayed or
coalesced frame cannot cost a player a tenth of a second, and pausing is exact.
The tick is a chain of `setTimeout`s rather than an interval, so a stopped game
schedules nothing and there is no id to clear.

The Screen Wake Lock is requested when a game starts and again on every
`visibilitychange` back to visible, because the browser releases the lock on
every hide. The sentinel is **held** in the app for as long as the game runs —
a wake lock is a handle, not a call — and dropped on Back and on a flag, so the
phone is not held awake over a form or a results screen.

## Two bugs that only a browser could find

Both shipped in the first release. Both passed every unit test, both passed
`clippy` on both targets, and both passed the CI build and its eight-file site
check. They were found by loading the built site in Chromium and playing a
game. The reasons are the same in both cases and are worth stating as a rule,
because nothing in the test suite can see either.

**Never touch the DOM from a start function.** `#[wasm_bindgen(start)]` runs
when the wasm module is *instantiated*, which is before the document is
parsed. A start function that resolved the shell found nothing and trapped,
leaving a correctly rendered setup form that did nothing when touched, with
`RuntimeError: unreachable` as the only clue. So `start()` now does no work: it
calls `start_when_ready()`, which consults `document.readyState` and mounts
immediately if the document is already parsed, or on `DOMContentLoaded` if it
is not. `tests/shell.rs` asserts the ordering statically — the start function
must not call `mount()` — because that is the one property a unit test *can*
check. Note that `Document::readyState` is a plain `String` in web-sys
0.3.105; there is no `ReadyState` enum and no feature to enable.

**`Rc::new_cyclic` cannot give you a strong self-reference.** Its closure runs
*before* the `Rc` exists, so `weak.upgrade()` inside it returns `None` by
construction. The code was

```rust
shared: weak.upgrade().expect("new_cyclic hands the closure a live weak reference"),
```

which panicked on every load, and the `.expect()` asserted the exact opposite
of the truth. The field is now a `Weak<RefCell<App>>`, upgraded where it is
used, which is also better on the merits: a callback should not keep alive the
thing it is a callback on. This one is worth dwelling on, because the line is
*visually identical* to the correct version that sits one line below it.

**A browser check is the only thing that would have caught either**, and it is
a one-off rather than a test suite — no browser tests are committed, in line
with the rest of the family. What it did: load the built `dist/` from a local
server, start a 20-second game with a 3-second increment, tap a panel, watch
the clock run, hand over, and compare the numbers. Which found a third bug:

**Never infer "was that a move?" from `phase`.** `ui.rs` compared `phase`
before and after `Game::tap` to decide whether to settle the increment. A
hand-over does not change `phase` — it changes `on_clock` — so every genuine
move read as "nothing happened" and the increment was **never paid**, while the
display updated and looked entirely normal. The app played with no increment at
all. `Game::tap` now returns a `Tap` (`Started` / `Moved` / `Ignored`), marked
`#[must_use]`, and the caller settles on `Moved`. The unit tests missed this
because they all called `settle_increment` themselves and never consulted what
`tap` reported; `tap_reports_exactly_what_it_did` and
`a_reported_move_settles_its_increment` now pin it.

The lesson for the other five repos: a green suite and a green build prove the
*parts* are right, and this is a whole app failing to start. Load it.

## Orientation: both readings face left, exactly as the original

Three attempts, two of them wrong, and the lesson is about where the
authority is.

**One rotation per element.** The original Tailwind build set one property
(`rotate: 90deg`). This rewrite first set both `rotate: 90deg` and
`transform: rotate(90deg)` on the readings, "for a browser that only knows the
old one" — they are two independent transform functions and a browser composes
them, so the readings rendered at **180°**, upside down. Measured in Chromium
on a 200×40 box with a marker on its left edge:

| declaration | marker lands at | angle |
|---|---|---|
| `transform: rotate(90deg)` | (80, −80) | 90° |
| `rotate: 90deg` | (80, −80) | 90° |
| **both** | **(180, 0)** | **180°** |

The fallback bought nothing: `transform: rotate()` works everywhere the app has
ever run. Only `transform` is used now, which is also the spelling that
survives browsers without the independent `rotate` property (Safari before
14.1).

**Both readings are turned the same way, and they face left.** The second
attempt made them *opposite* — 90° on the top panel and 270° on the bottom —
because the two players sit on opposite edges of a device lying flat, so the
clocks "should face outward from the centre line". That reasoning was
**invented here rather than read off the original**, and it was wrong. At
`006f70c` both spans carry the same class:

```html
<span class="rotate-90 block">05:00</span>
<span class="rotate-90 block">05:00</span>
```

One rotation, four elements (two readings, two buttons), all facing left.

"Face left" is the direction you tilt your head to read it, so the tops of the
glyphs point screen-right. Measured on the unmodified original in Chromium:
both readings compute `rotate: 90deg`, the tops of the glyphs point right, so
you tilt your head left. The build as shipped now measures identically, on all
four elements:

| element | transform | direction |
|---|---|---|
| `#reading-0`, `#reading-1` | `matrix(0, 1, -1, 0, 0, 0)` | faces left |
| `#pause`, `#back` | `matrix(0, 1, -1, 0, 0, 0)` | faces left |

The rule is a single `transform: rotate(90deg)` on the shared `.panel .reading`
selector — there is no per-panel rule at all, and
`the_readings_turn_outward_and_never_by_both_properties_at_once` asserts both
facts, and fails if a per-panel override reappears.

**The method note, which is the actual lesson.** Three times this was settled
by argument before it was settled by measurement, and the argument was wrong
each time. The original app is in the repository's own history at `006f70c`; it
could have been read, run and measured from the first report instead of
inferred. When a task is a port, the thing being ported is the specification.
An invariant invented to make a bug look principled is a bug with a comment on
it.

Two of this file's tests had to be weakened into shape checks ("the two
rotations differ") to accommodate the wrong version, and both passed while
their bug was live. Every regression test here is now verified by
reintroducing the exact defect and watching it go red.

## Startup

`mount()` reports rather than panics, everywhere. Every DOM lookup on the
startup path goes through `Dom::resolve`, which returns an `Option`, and a
failure calls `report_start_failure` — which reuses the shell's own message
rather than trapping. `listen` attaches a listener or says why it could not,
instead of unwrapping a `Result`. A page that explains itself is a far better
failure than an opaque `unreachable`, and it is a failure someone can act on.

## Code map

- `clock.rs`: the game. Time formatting, both increment rules, the
  turn/pause/finish state machine, and the `Phase` enum that makes
  "running with no player" and "paused with a live interval" unrepresentable.
  No `web-sys`, no DOM, every rule unit-tested.
- `ui.rs`: wasm-only. Waits for the document, resolves the shell's elements
  once, installs the listeners, drives the tick, holds the wake lock, and
  paints. It contains no rule about how a clock works — it asks `clock` and
  renders the answer, and acts on what `clock` reports. See "Two bugs that only
  a browser could find" above, which are all in this file.
- `ui.html`: the static shell. One inline `<style>` with the original's
  colours as custom properties, one `<script type="module">` whose body is the
  dynamic import of the generated bindings. Rust owns everything dynamic. The
  readings' rotation is the one piece of layout that had to be measured rather
  than reasoned about — see "Orientation" above, and read it before changing
  it.
- `service-worker.js`: caches only a fixed app-shell allowlist, scope-specific
  content-versioned cache, atomic install, no `skipWaiting`, so an update
  cannot swap the wasm under a live game.
- `build.rs`: writes the six files it owns into `dist/`, rasterizes the install
  icons from `assets/icon.svg`, assembles the manifest, derives the worker's
  content-derived cache version, and sets the wake-lock cfg for the wasm
  target. It leaves `dist/app.js` and `dist/app_bg.wasm` to the wasm-bindgen
  step, so it writes files in place rather than replacing the directory.

## Tests

`cargo test --locked`, no browser and no external tool.

`src/clock.rs` carries 36 unit tests: the increment rules including the Bronstein
cap swept across increments and spends, the three formatter branches and their
boundaries (59:59 against 1:00:00, the truncation at 9.999 s, the four-character
padding), and every transition of the state machine — first tap, idle tap,
pause and resume, flagging from either side, a finished game accepting nothing,
re-arming, and what a tap reports — because the caller has to know, and
getting it wrong meant the increment was never paid (see above).

Two of them exist because the suite was *green and the app was wrong*. The
first-tap tests pin the cross-wiring above, and they are mutation-checked in
the only way that counts: reintroducing `let started = player;` turns
nineteen of the thirty-six red, `the_first_tap_starts_the_opponents_clock`
among them. The second, `the_cap_is_against_this_move_not_the_whole_game`,
exists because the earlier version of this file argued its way to the wrong
answer twice on the orientation and then wrote tests shaped to fit.

`tests/shell.rs` asserts the invariants of the committed shell — including the
panel rotation, which is the one visual property that no amount of unit testing
can check: that the page
loads the generated bindings and **calls** their initializer, that there is
exactly one script tag, that every URL is relative, that the worker's
`__VERSION__` placeholder appears exactly once and `skipWaiting` does not, that
the manifest constant in `build.rs` is valid JSON with relative
`id`/`start_url`/`scope`, that the icon is the upstream file byte for byte, and
that no generated file, no raster icon and none of the original
`app.js`/`sw.js`/`tw.css` are tracked.

Three of these deserve their reason written down:

- **The loader must call `m.default()`.** A `wasm-bindgen` module runs its
  `start` function only when the generated initializer is invoked, so
  `import('./app.js').then(() => {})` yields a perfectly valid module that
  never starts — the page loads, the shell renders, the service worker
  registers, and the clock is inert, with nothing in the console to say why.
  Lego Mosaic's loader is the reference.
- **Every precached file is checked against what the build publishes.** The
  test reads the worker's own `ASSETS` array and asserts each entry is a file
  the build emits, rather than asserting a list of eight names written out in
  the test. This catches a half-built site, and it catches it because the
  failure is otherwise invisible: `caches.addAll` **rejects the entire install**
  if any listed URL 404s, so one missing file does not degrade the cache — it
  removes the service worker and with it every bit of offline support. The
  symptom is "offline is broken", with nothing in the build output and a
  favicon 404 that looks cosmetic and is not. Two repos in this family shipped
  a seven-file `dist/` with no `icon.svg` while the page linked it and the
  worker precached it. Deriving the expectation from the template is the point:
  a second hardcoded copy of the list is exactly how the build and the worker
  drifted apart in the first place. The published set is likewise read from
  `build.rs` — the `SHELL` table, `ICON_SIZES`, and the two names the
  wasm-bindgen step writes — so neither side of the comparison is a literal.
  Verified to fail when `icon.svg` is dropped from `SHELL`, naming the file.
- **`icon.svg` is hashed, not merely checked for existing.** A redraw that
  kept the filename and the pawn shape would pass any structural test; the
  recorded digest is what catches it. The SHA-256 is implemented in the test
  file, ~40 lines, rather than adding a dependency for one assertion.

Four more assertions here earn their place for the same reason as the three
above: they check properties no unit test can reach. `startup_defers_to
_document_readiness_before_touching_the_dom` pins the start-function ordering.
`no_visible_text_names_the_implementation` keeps `Rust`, `WebAssembly`, `wasm`,
`JavaScript`, `bindings` and `compile` out of anything a player can read — the
interface is a chess clock, and the source, this file and `README.md` are free
to say as much as they like.
`the_shell_does_not_carry_the_deprecated_web_app_capable_meta` keeps Chromium
from logging a deprecation warning on every load; the current spelling is
`mobile-web-app-capable`.

The comment-stripping helpers in `tests/shell.rs` exist because the shell and
this crate both *explain* their failures in comments that necessarily contain
the very text being asserted on; a raw substring search finds the explanation
before the code.
*explains* the `m.default()` failure in a comment that necessarily contains the
text being asserted on; a raw substring search finds the explanation before the
code.

## Verification

```
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
RUSTFLAGS="--cfg=web_sys_unstable_apis" \
  cargo clippy --locked --lib --target wasm32-unknown-unknown -- -D warnings
```

Both clippy targets are required: the crate denies warnings, but only for the
target being compiled, so a clean host build says nothing about the wasm one.

`cargo build` writes `dist/` and the release gate does not carry it, so in a
checkout the site has to be rebuilt by hand to be looked at.
`.github/workflows/build.yml` does exactly that on every push and pull request,
in the documented order, and then inspects what came out: eight files, all
non-empty, no strays, the worker's `__VERSION__` substituted, bindings that
carry both an `export` and the `__wbindgen_start` the page's initializer runs,
the icons really being PNGs, and the page really calling `m.default()`. That
check is the only automated coverage of the built site there is, and it is
aimed at the one failure this shape has already had — a `dist/` that looks
publishable and has no app in it.

## Known limitations

- The wake lock needs a secure context and a browser that implements the API.
  Where it is refused the clock still works and the screen may dim; the refusal
  is logged once rather than shown, because a chess clock is used with two
  people watching it and an error banner is noise.
- The rotated readings are sized with `clamp()` against the viewport width, not
  against the panel's own aspect. A very wide, very short window — a phone in
  landscape with the app beside it — can clip a long `h:mm:ss` reading. The
  original had the same property, having the same fixed font size and the same
  rotation.
- A game is not persisted. Reloading the page loses it, as it did originally;
  the app has no storage, and adding some would be a feature rather than a
  port.
- The readings are sized with `clamp()` against the viewport width, not against
  the panel's own aspect, so a very wide, short window can clip a long
  `h:mm:ss`. The original had the same property, having the same fixed font size
  and the same rotation.
