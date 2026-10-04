// Copyright (c) 2026 Witalis Domitrz <witekdomitrz@gmail.com>
// AGPL License

//! Write the static app shell to `dist/` while the crate compiles.
//!
//! Chess Clock is a browser application, so publishing it is a file copy, not
//! a program run. Six of the eight shell files are committed as they are; the
//! other two are derived:
//!
//! * `service-worker.js` carries a `__VERSION__` placeholder standing for a
//!   cache name derived from the bytes of every *other* file in `dist/`,
//!   including the wasm. Deriving it here rather than in the page is what
//!   makes the cache name change exactly when the app does.
//! * `manifest.webmanifest` is assembled from `assets/icon.svg` and the icon
//!   PNGs this same script rasterizes, so its icon list cannot drift from
//!   what was actually produced.
//!
//! `assets/icon.svg` is the author's original, committed byte for byte, and is
//! the source of truth for every icon in the app. The 192 and 512 PNGs are
//! derived from it here and exist only in `dist/`; they are build output and
//! are never committed.
//!
//! Everything is written to `dist/`, which is the whole site and the only
//! copy. `cargo build --release` therefore leaves a publishable site behind,
//! and no `cargo run` step exists — the crate has no `[[bin]]`.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

/// The files this script copies from the source tree, unchanged.
const SHELL: &[(&str, &str)] = &[
    ("index.html", "src/ui.html"),
    // The SVG travels to the site as well as being the source the PNGs are
    // rasterized from. The shell links it directly as the page icon, and it is
    // in the service worker's allowlist, so it has to be one of the eight.
    ("icon.svg", "assets/icon.svg"),
];

/// The install icon sizes, in pixels, derived from `assets/icon.svg`.
const ICON_SIZES: [u32; 2] = [192, 512];

/// The manifest. The original's `manifest.json` carried a `name` and no
/// `short_name` at all — it had been removed in the commit before last — and no
/// theme or background colour, so those are chosen here to match the UI the
/// original shipped: the page is Tailwind `gray-900` and the install splash
/// should not flash a lighter grey behind it before the app paints.
///
/// See AGENTS.md: `theme_color` is the page background (`gray-900`),
/// `#111827`, and `background_color` is the darker `#0b1220` the splash uses.
/// A two-hash raw string, because the manifest is full of `#rrggbb` colours
/// and a one-hash raw string would end at the first one it met.
const MANIFEST: &str = r##"{
  "id": "./",
  "name": "Chess Clock",
  "short_name": "Chess",
  "start_url": "./",
  "scope": "./",
  "display": "standalone",
  "background_color": "#0b1220",
  "theme_color": "#111827",
  "orientation": "any",
  "icons": [
    {
      "src": "icon-192.png",
      "sizes": "192x192",
      "type": "image/png",
      "purpose": "any maskable"
    },
    {
      "src": "icon-512.png",
      "sizes": "512x512",
      "type": "image/png",
      "purpose": "any maskable"
    }
  ]
}
"##;

fn main() {
    let root = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());

    // The site is built only for the host target. This script also runs during
    // `cargo build --lib --target wasm32-unknown-unknown`, and at that moment
    // `dist/app.js` and `dist/app_bg.wasm` are the *output* of that build:
    // they do not exist yet, and writing the site there would fail — or worse,
    // succeed while deleting the artefacts the step is producing. The host
    // build that follows the wasm-bindgen pass is the one that publishes.
    let target = std::env::var("TARGET").unwrap_or_default();
    if target.starts_with("wasm") {
        // The Screen Wake Lock API is behind `web_sys_unstable_apis` in
        // `web-sys` 0.3.105, and the whole of it: `Navigator::wake_lock`,
        // `WakeLock::request`, `WakeLockSentinel` and `WakeLockType` each carry
        // the cfg. Enabling the features is necessary and not sufficient.
        //
        // The cfg that actually matters is NOT emitted here. `cargo:rustc-cfg`
        // reaches only the *building package's own units*, and `web_sys` is a
        // registry dependency compiled in its own unit, so the items the crate
        // needs are never compiled at all — which looks like the technique
        // works right up until the real error names no feature to add. The
        // load-bearing setting is `.cargo/config.toml`, with `RUSTFLAGS` in the
        // CI workflow as the second source. Do not remove these two lines
        // expecting the build to break, and do not rely on them expecting it to
        // succeed.
        //
        // Scoped to the wasm build, so the host build — and the `cargo test`
        // that runs under it — is unaffected by an unstable-API flag it has no
        // use for.
        println!("cargo:rustc-check-cfg=cfg(web_sys_unstable_apis)");
        println!("cargo:rustc-cfg=web_sys_unstable_apis");
        return;
    }

    // Watch the SOURCE paths, not the output names: cargo compares these
    // against real files, so `ui.html` has to be named as itself. Watching a
    // destination name watches a file that never changes, and the script then
    // never re-runs.
    for (_, source) in SHELL {
        println!("cargo:rerun-if-changed={source}");
    }
    println!("cargo:rerun-if-changed=src/service-worker.js");
    println!("cargo:rerun-if-changed=assets/icon.svg");
    println!("cargo:rerun-if-changed=build.rs");

    // Owned names: the icons are named here rather than read from `SHELL`,
    // so the site's file list is built up rather than written out twice.
    let mut built: Vec<(String, Vec<u8>)> = Vec::with_capacity(SHELL.len() + ICON_SIZES.len() + 2);
    for (name, source) in SHELL {
        let bytes = std::fs::read(root.join(source))
            .unwrap_or_else(|error| panic!("reading {source}: {error}"));
        built.push(((*name).to_string(), bytes));
    }

    // The icons, rasterized from the committed SVG. They enter the site before
    // the cache version is taken, so changing the icon changes the version.
    for (size, png) in rasterize_icons(&root) {
        built.push((format!("icon-{size}.png"), png));
    }

    // The manifest, assembled here rather than committed, so its icon list
    // cannot drift from what was rasterized: if an icon is missing from the
    // directory it is not in the manifest either.
    let manifest = assemble_manifest(&built);
    built.push(("manifest.webmanifest".to_string(), manifest.into_bytes()));

    // The worker's own template is hashed too, and deliberately kept out of
    // `built` so it is hashed exactly once, in template form. A change to the
    // caching logic must invalidate the cache: a client holding the old worker
    // would otherwise keep running stale logic against new assets.
    let template = std::fs::read_to_string(root.join("src/service-worker.js"))
        .unwrap_or_else(|error| panic!("reading src/service-worker.js: {error}"));
    let version = cache_version(&built, &template);
    let worker = template.replace("__VERSION__", &version);
    assert!(
        !worker.contains("__VERSION__"),
        "the service worker still contains the version placeholder"
    );
    built.push(("service-worker.js".to_string(), worker.into_bytes()));

    write_tree(&root.join("dist"), &built);
}

/// Rasterize `assets/icon.svg` at each install size.
///
/// The SVG is the source of truth and is never redrawn, regenerated or
/// replaced: this only turns it into the raster sizes a manifest must
/// declare. Rendering is per size rather than render-then-downscale, so there
/// is no resampling step to lose the icon's edges.
fn rasterize_icons(root: &Path) -> Vec<(u32, Vec<u8>)> {
    let path = root.join("assets/icon.svg");
    let svg =
        std::fs::read(&path).unwrap_or_else(|error| panic!("reading {}: {error}", path.display()));
    let tree = usvg::Tree::from_data(&svg, &usvg::Options::default())
        .unwrap_or_else(|error| panic!("parsing {}: {error}", path.display()));

    let width = tree.size().width();
    assert!(
        width > 0.0,
        "{} has no width; there is nothing to rasterize",
        path.display()
    );

    ICON_SIZES
        .iter()
        .map(|&size| {
            // `Pixmap::new` is the one call in this loop that can fail without
            // an `Err`: in tiny-skia 0.12 it returns an `Option`, because the
            // only reason it fails is a size that cannot be allocated.
            let mut pixmap = tiny_skia::Pixmap::new(size, size)
                .unwrap_or_else(|| panic!("cannot allocate a {size}x{size} pixmap"));
            // f32, not f64: `resvg::render` takes an f32 transform, and
            // computing the scale in f64 and narrowing is one more chance to
            // round the wrong way at the icon's 48-unit viewBox.
            //
            // The SVG is square, so one scale for both axes is exact; a
            // non-square source would need the height factored in too, and
            // the assert above is where that would go.
            let scale = size as f32 / width;
            resvg::render(
                &tree,
                tiny_skia::Transform::from_scale(scale, scale),
                &mut pixmap.as_mut(),
            );
            let png = pixmap
                .encode_png()
                .unwrap_or_else(|error| panic!("encoding icon-{size}.png: {error}"));
            (size, png)
        })
        .collect()
}

/// Build the manifest, asserting that every icon it declares was produced.
///
/// The JSON is a constant with the icon list spliced in, so a size that fails
/// to rasterize fails the build rather than shipping a manifest that points at
/// a file the site does not have.
fn assemble_manifest(built: &[(String, Vec<u8>)]) -> String {
    for size in ICON_SIZES {
        let name = format!("icon-{size}.png");
        assert!(
            built.iter().any(|(candidate, _)| *candidate == name),
            "the manifest declares {name}, which was not rasterized"
        );
    }
    MANIFEST.to_string()
}

/// A cache name derived from the bytes of every shell file except the worker.
///
/// It deliberately covers the worker's own source: a change to the caching
/// logic must invalidate the cache too, or clients keep running the old logic
/// against new assets.
fn cache_version(built: &[(String, Vec<u8>)], worker_template: &str) -> String {
    let mut hasher = DefaultHasher::new();
    for (name, bytes) in built {
        name.hash(&mut hasher);
        bytes.hash(&mut hasher);
    }
    worker_template.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

/// Write every file this script owns into `dir`, in place.
///
/// Only the files named above are written, and the rest of the directory is
/// left alone: the two wasm artefacts live in `dist/` too, written by the
/// `wasm-bindgen` step that runs *before* this one, and a wholesale directory
/// swap would take them with it — leaving a publishable-looking site with no
/// app in it and no error. Each file is written under a scratch name and
/// renamed over its target, so a host serving `dist/` never observes a
/// half-written file.
fn write_tree(dir: &Path, built: &[(String, Vec<u8>)]) {
    std::fs::create_dir_all(dir).unwrap_or_else(|error| panic!("{}: {error}", dir.display()));

    for (name, bytes) in built {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create shell directory");
        }
        let scratch = dir.join(format!(".{name}.new"));
        std::fs::write(&scratch, bytes)
            .unwrap_or_else(|error| panic!("writing {}: {error}", scratch.display()));
        std::fs::rename(&scratch, &path)
            .unwrap_or_else(|error| panic!("publishing {}: {error}", path.display()));
    }
}
