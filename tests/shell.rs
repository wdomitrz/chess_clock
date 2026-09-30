// Copyright (c) 2026 Witalis Domitrz <witekdomitrz@gmail.com>
// AGPL License

//! Invariants of the app shell source, and of what is and is not committed.
//!
//! Everything here reads committed files. `dist/` is build output and is
//! gitignored, so the release gate — which exports the candidate tree — never
//! has it, and cannot build it either: that needs the wasm target, a pinned
//! `wasm-bindgen` CLI and the `--cfg=web_sys_unstable_apis` flag the Wake Lock
//! API is behind. A test asserting on `dist/` would therefore run only in a
//! developer's checkout, which is exactly where it is least likely to catch
//! anything, so those assertions are gone rather than skipped.
//!
//! What covers the built output is running the two build steps, in the order
//! AGENTS.md gives them. `.github/workflows/build.yml` does exactly that on
//! every push and then inspects what came out. A test on a leftover directory
//! cannot do that job.

use std::path::Path;

/// The repository root.
fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// The app shell, as committed.
///
/// `build.rs` copies this into `dist/index.html` byte for byte, so asserting
/// on it asserts on exactly what gets published.
fn shell() -> String {
    let path = root().join("src/ui.html");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()))
}

/// Tracked file names, or `None` outside a checkout.
///
/// The release gate exports the candidate as a bare directory with no `.git`,
/// so there is no index to ask. Callers decide what that means.
fn tracked_files() -> Option<String> {
    let inside = std::process::Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .current_dir(root())
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false);
    if !inside {
        return None;
    }
    let output = std::process::Command::new("git")
        .args(["ls-files"])
        .current_dir(root())
        .output()
        .expect("git ls-files");
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// The shell is the app, and the app is wasm. A hand-written ABI would mean the
/// clock no longer shared the library the tests cover.
#[test]
fn the_page_loads_generated_bindings_not_a_manual_wasm_abi() {
    let page = shell();
    assert!(
        page.contains("<!DOCTYPE html>"),
        "the shell must be a document"
    );
    assert!(page.contains("<script type=\"module\">"), "a module script");
    assert!(
        page.contains("import('./app.js')"),
        "the page must load the generated bindings"
    );
    assert_eq!(
        page.matches("<script").count(),
        1,
        "exactly one script tag:\n{page}"
    );
    for obsolete in [
        "instantiateStreaming",
        "alloc_buf",
        "wasm.exports",
        "fetch(",
    ] {
        assert!(!page.contains(obsolete), "{obsolete} in the static shell");
    }
}

/// The loader must *call* the generated initializer, not merely import it.
///
/// This is the one failure of the rewrite that produces no error at all: a
/// `wasm-bindgen` module runs its `start` function only when its default
/// export is invoked, so `import('./app.js').then(() => {})` yields a valid
/// module that never starts. The page loads, the shell renders, the service
/// worker registers, and the clock is simply inert — with nothing in the
/// console to say why. Lego Mosaic's loader is the reference: `m.default()`.
#[test]
fn the_loader_initialises_the_bindings_rather_than_merely_importing_them() {
    let page = shell();
    // Strip comments first: this file explains the failure in a comment that
    // necessarily contains the very text being asserted on, and a substring
    // search over the raw page would find the explanation before the code.
    let code: String = strip_comments(&page);
    assert!(
        code.contains("m.default()"),
        "the loader must call the generated initializer, or the app never starts"
    );
    // And it must be the import chain's continuation, not dead code beside it.
    let import_at = code
        .find("import('./app.js')")
        .expect("the bindings import");
    let default_at = code.find("m.default()").expect("the initializer call");
    assert!(
        default_at > import_at,
        "the initializer call must follow the import it belongs to"
    );
    // A rejected import has to reach the catch, or a failure is silent.
    assert!(
        code.contains(".catch("),
        "a failed load must show the failure message, not a blank page"
    );
}

/// Drop `//` line comments and `/* ... */` blocks, so an assertion about what
/// the code *does* cannot be satisfied by a comment saying what it does.
fn strip_comments(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut chars = html.chars().peekable();
    let mut in_block = false;
    while let Some(c) = chars.next() {
        if in_block {
            if c == '*' && chars.peek() == Some(&'/') {
                chars.next();
                in_block = false;
            }
            continue;
        }
        if c == '/' && chars.peek() == Some(&'/') {
            for next in chars.by_ref() {
                if next == '\n' {
                    out.push('\n');
                    break;
                }
            }
            continue;
        }
        if c == '/' && chars.peek() == Some(&'*') {
            chars.next();
            in_block = true;
            continue;
        }
        out.push(c);
    }
    out
}

/// The site is mounted under an arbitrary prefix, so every URL in it is
/// relative. One build, any subdirectory.
#[test]
fn the_shell_is_mountable_anywhere() {
    let page = shell();
    assert!(
        !page.contains("http://") && !page.contains("https://"),
        "an absolute URL would break the site outside its own origin"
    );
    assert!(
        page.contains("./app.js"),
        "bindings must be referenced relatively"
    );
    assert!(
        page.contains("manifest.webmanifest"),
        "the page must register a manifest"
    );
    assert!(
        page.contains("icon.svg"),
        "the page must link the committed SVG icon"
    );
}

/// An accessible browser app, not a canvas demo. The controls have to exist
/// and be labelled, and the two panels have to announce themselves, because
/// the thing that makes this app work — readings rotated for a phone lying flat
/// between two players — is invisible to a screen reader.
#[test]
fn the_page_has_accessible_setup_and_two_labelled_panels() {
    let page = shell();
    for id in [
        "setup",
        "setup-form",
        "hours",
        "minutes",
        "seconds",
        "increment-type",
        "increment-minutes",
        "increment-seconds",
        "start",
        "game",
        "panel-0",
        "panel-1",
        "reading-0",
        "reading-1",
        "controls",
        "pause",
        "back",
        "load-error",
    ] {
        assert!(page.contains(&format!("id=\"{id}\"")), "missing #{id}");
    }
    // Every input is labelled, and the panels are focusable buttons.
    assert_eq!(
        page.matches("<label").count(),
        6,
        "the six setup fields must be labelled"
    );
    assert_eq!(
        page.matches("tabindex=\"0\"").count(),
        2,
        "both panels must be reachable by keyboard"
    );
    assert_eq!(
        page.matches("role=\"button\"").count(),
        2,
        "both panels must announce as buttons"
    );
    // A `role="timer"` that repaints ten times a second would talk over the
    // room, so the readings are explicitly not live regions and the panels
    // carry a hidden explanation instead.
    assert!(
        page.contains("visually-hidden"),
        "the rotated panels need a text description for a screen reader"
    );
    assert_eq!(
        page.matches("aria-live=\"off\"").count(),
        2,
        "the readings must not be live regions"
    );
}

/// The shell respects the platform's motion and colour preferences, and
/// declares the dark scheme its CSS actually implements.
#[test]
fn the_shell_respects_platform_preferences() {
    let page = shell();
    assert!(
        page.contains("prefers-reduced-motion"),
        "a clock that flashes on colour change is unreadable for some users"
    );
    assert!(
        page.contains("name=\"color-scheme\" content=\"dark\""),
        "the app is dark-only and must say so, or form controls render light"
    );
    assert!(
        page.contains("viewport-fit=cover"),
        "the panels run edge to edge on a phone with a notch"
    );
}

/// The service worker is a committed template with exactly one placeholder, and
/// `build.rs` substitutes it. A template with no placeholder would mean the
/// cache never invalidates; a second one would mean the substitution is not
/// the only edit.
#[test]
fn the_service_worker_template_has_exactly_one_placeholder() {
    let worker = std::fs::read_to_string(root().join("src/service-worker.js"))
        .expect("the committed worker template");
    assert_eq!(
        worker.matches("__VERSION__").count(),
        1,
        "the template must carry exactly one version placeholder"
    );
    assert!(
        worker.contains("new URL('./', self.location.href)"),
        "the worker must resolve its cache from its own location"
    );
    // No `skipWaiting`: swapping the wasm under a live game would replace the
    // clock mid-move. The *call* is what is forbidden — the file's own header
    // comment names the API to explain why it is absent, so matching the bare
    // word would fail on the explanation.
    assert!(
        !worker.contains("self.skipWaiting"),
        "an update must not swap the app under a live tab"
    );
    // The cache covers the eight files the two build steps produce, and the
    // assertion that they are all *published* is the separate test below.
    for asset in [
        "'./'",
        "'app.js'",
        "'app_bg.wasm'",
        "'manifest.webmanifest'",
        "'icon-192.png'",
        "'icon-512.png'",
        "'icon.svg'",
        "'index.html'",
    ] {
        assert!(worker.contains(asset), "the cache omits {asset}");
    }
}

/// Every file the service worker precaches must actually be published.
///
/// This is the assertion that catches a half-built site, and it exists because
/// the failure it catches is silent. `caches.addAll` **rejects the entire
/// install** if any listed URL 404s, so one missing file does not degrade the
/// cache — it removes the service worker and with it all offline support. The
/// symptom is "offline is broken", with nothing in the build output, no failed
/// request in the obvious place, and a favicon 404 that looks cosmetic and is
/// not. Two repos in this family shipped a seven-file `dist/` with no
/// `icon.svg` while the page linked it and the worker precached it.
///
/// The list is read out of the worker's own `ASSETS` array rather than written
/// out here, because a second copy of the list is exactly how the two drifted
/// apart in the first place: the build dropped a file, the test still asserted
/// the old eight, and nothing compared them. Deriving the expectation from the
/// template means the check asks the only question that matters — does the
/// build publish everything the worker asks for?
#[test]
fn every_precached_file_is_published_by_the_build() {
    let worker = std::fs::read_to_string(root().join("src/service-worker.js"))
        .expect("the committed worker template");
    let assets = worker_assets(&worker);

    assert!(
        !assets.is_empty(),
        "the ASSETS list could not be read out of the worker"
    );

    // What the build publishes, gathered from the two places it comes from:
    // `build.rs` copies SHELL and derives the icons, the manifest and the
    // worker, and the `wasm-bindgen` step writes the bindings and the wasm.
    let published = published_files();

    for asset in &assets {
        // `'./'` is the scope root, which is `index.html` on disk.
        let name = if asset == "./" { "index.html" } else { asset };
        assert!(
            published.iter().any(|file| file == name),
            "the service worker precaches {asset:?}, which the build does not publish; \
             caches.addAll rejects the whole install on one 404, so this silently \
             costs the app its offline support (published: {published:?})"
        );
    }

    // And the other direction: a published file the worker does not cache is
    // not an error — an uncached file is simply fetched from the network — but
    // the shell's own files all should be, so a dropped name is caught here
    // too rather than being a silent no-op in the cache.
    for file in ["app.js", "app_bg.wasm", "manifest.webmanifest"] {
        assert!(
            assets.iter().any(|asset| asset == file),
            "{file} is published but not precached"
        );
    }
}

/// The `ASSETS` entries from a service worker template, in order.
///
/// Reads the array as written rather than evaluating JavaScript: the entries
/// are string literals in a fixed list, and the test needs to know the list
/// even in a template that would not parse.
fn worker_assets(worker: &str) -> Vec<String> {
    let start = worker
        .find("const ASSETS = [")
        .expect("the worker must declare ASSETS");
    let body_start = start + "const ASSETS = [".len();
    let end = worker[body_start..]
        .find("]")
        .expect("the ASSETS list must be terminated");
    worker[body_start..body_start + end]
        .split(',')
        .filter_map(|entry| {
            let entry = entry.trim();
            let inner = entry.strip_prefix('\'')?.strip_suffix('\'')?;
            Some(inner.to_string())
        })
        .collect()
}

/// The names of the files a complete `dist/` holds, gathered from the build
/// script rather than written out here.
///
/// Two sources, because there are two builds: `build.rs` names the files it
/// copies and derives, and the bindings and the wasm are the two files the
/// `wasm-bindgen` step writes with `--out-name app`. Both are read from the
/// committed sources, so this needs no `dist/` — which is gitignored, and so
/// absent from the tree the release gate exports.
fn published_files() -> Vec<String> {
    let build = std::fs::read_to_string(root().join("build.rs")).expect("build.rs");
    let mut files = Vec::new();

    // `("index.html", "src/ui.html")` — the copied shell files.
    for line in build.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("(\"") else {
            continue;
        };
        let Some((name, _)) = rest.split_once("\",") else {
            continue;
        };
        files.push(name.to_string());
    }

    // The derived names: the two icons at the sizes ICON_SIZES lists, the
    // manifest, and the worker itself.
    if let Some(sizes) = build
        .lines()
        .find(|line| line.trim_start().starts_with("const ICON_SIZES"))
    {
        for size in sizes
            .trim_start_matches("const ICON_SIZES: [u32; 2] = [")
            .trim_end_matches("];")
            .split(',')
        {
            let size = size.trim();
            if !size.is_empty() {
                files.push(format!("icon-{size}.png"));
            }
        }
    }
    for name in ["manifest.webmanifest", "service-worker.js"] {
        if build.contains(&format!("(\"{name}\""))
            || build.contains(&format!("\"{name}\".to_string()"))
        {
            files.push(name.to_string());
        }
    }

    // The two files the `wasm-bindgen` step writes, from `--out-name`.
    files.push("app.js".to_string());
    files.push("app_bg.wasm".to_string());
    files
}

/// The committed manifest is valid JSON, mounted relatively, and names this
/// app. `build.rs` assembles it, so this asserts on the constants it holds —
/// which is the only copy a test can reach, since the built one is gitignored.
#[test]
fn the_committed_manifest_is_well_formed_and_relative() {
    let source = std::fs::read_to_string(root().join("build.rs")).expect("build.rs");
    // Pull the manifest constant out of the build script and parse it. This
    // is not a substitute for checking the built file — CI does that — but it
    // does mean a malformed manifest cannot be committed and then discovered
    // broken at release time.
    let start = source
        .find("const MANIFEST: &str = r##\"{")
        .expect("build.rs must hold the manifest");
    let body_start = start + "const MANIFEST: &str = r##\"".len();
    let end = source[body_start..]
        .find("\"##;")
        .expect("the manifest constant must be terminated");
    let json = &source[body_start..body_start + end];

    let manifest: serde_json::Value =
        serde_json::from_str(json).expect("the manifest is valid JSON");
    assert_eq!(manifest["name"], "Chess Clock");
    assert_eq!(manifest["short_name"], "Chess");
    assert_eq!(manifest["display"], "standalone");
    for key in ["id", "start_url", "scope"] {
        assert_eq!(manifest[key], "./", "{key} must be relative");
    }
    for key in ["theme_color", "background_color"] {
        let value = manifest[key]
            .as_str()
            .unwrap_or_else(|| panic!("{key} must be a string"));
        assert!(
            value.starts_with('#') && value.len() == 7,
            "{key} must be a #rrggbb colour, got {value:?}"
        );
    }
    // Both install icons, declared at the sizes they are rasterized at.
    let icons = manifest["icons"].as_array().expect("an icon list");
    assert_eq!(icons.len(), 2, "one icon per install size");
    for (icon, size) in icons.iter().zip(["192x192", "512x512"]) {
        assert_eq!(icon["sizes"], size);
        assert_eq!(icon["type"], "image/png");
        assert_eq!(icon["purpose"], "any maskable");
        assert_eq!(
            icon["src"],
            format!(
                "icon-{}.png",
                icon["sizes"].as_str().unwrap().split('x').next().unwrap()
            )
        );
    }
}

/// The committed `icon.svg` is the author's original, byte for byte, and it is
/// the only icon in the repository. Every PNG is derived from it at build time.
///
/// The digest below is of the upstream file as it stood at the last original
/// commit (`006f70c`), recorded here so a redraw cannot pass unnoticed. This is
/// the one invariant the shared spec asks to be cheap to record, and it is the
/// cheapest possible record.
#[test]
fn the_icon_is_the_original_svg_untouched() {
    let icon = std::fs::read(root().join("assets/icon.svg")).expect("assets/icon.svg");

    // sha256 of the author's Material Symbols pawn, #434343, 48px, transparent.
    const ORIGINAL_SHA256: &str =
        "9cdd9adaca7a8a60af11a0d25cb2ccc9d5b4784ff864217ba0dec31f09afc306";
    assert_eq!(
        hex(&sha256::digest(&icon)),
        ORIGINAL_SHA256,
        "assets/icon.svg must stay the author's original, byte for byte"
    );

    // It is an SVG, it is the Material Symbols pawn, and it is transparent.
    let text = String::from_utf8(icon).expect("the icon is text");
    assert!(text.contains("<svg"), "the icon must remain an SVG");
    assert!(
        text.contains("Material+Symbols"),
        "the icon is the Material Symbols 'chess' pawn; do not redraw it"
    );
    assert!(
        text.contains("fill: #434343"),
        "the pawn's fill is part of the original"
    );
}

/// No raster icon is committed anywhere. They exist only in `dist/`, which is
/// build output, and `build.rs` is what puts them there.
#[test]
fn no_raster_icon_is_committed() {
    if let Some(tracked) = tracked_files() {
        for line in tracked.lines() {
            assert!(
                !line.ends_with(".png"),
                "{line} is a raster icon; icons are rasterized from assets/icon.svg at build time"
            );
        }
    }
    for stray in [
        "assets/icon-192.png",
        "assets/icon-512.png",
        "icon-192.png",
        "icon-512.png",
    ] {
        assert!(
            !root().join(stray).exists(),
            "{stray} must not exist in the source tree"
        );
    }
}

/// Nothing generated may be tracked — not the wasm, not the bindings, not the
/// site. This is the test that would have caught them being committed.
#[test]
fn no_build_artifact_is_committed() {
    let Some(tracked) = tracked_files() else {
        return; // not a checkout: the gate's exported tree
    };
    for artefact in [
        "dist/index.html",
        "dist/app.js",
        "dist/app_bg.wasm",
        "dist/service-worker.js",
        "dist/manifest.webmanifest",
        "dist/icon-192.png",
        "dist/icon-512.png",
    ] {
        assert!(
            !tracked.lines().any(|line| line == artefact),
            "{artefact} is tracked; generated artefacts must never be committed"
        );
    }
    // And the hand-written JavaScript this rewrite replaced is gone: the whole
    // point is that the app is Rust, so `app.js` coming back would mean a
    // second implementation had crept in beside the real one.
    for gone in ["app.js", "sw.js", "tw.css", "index.html", "manifest.json"] {
        assert!(
            !tracked.lines().any(|line| line == gone),
            "{gone} is tracked; it was the original JavaScript app, now replaced by Rust"
        );
    }
}

/// `dist/` has to be ignored, or a build would leave the next commit dirty.
#[test]
fn dist_is_ignored() {
    if tracked_files().is_none() {
        return; // not a checkout
    }
    let ignored = std::process::Command::new("git")
        .args(["check-ignore", "-q", "dist/"])
        .current_dir(root())
        .status()
        .expect("git check-ignore")
        .success();
    assert!(ignored, "dist/ must be in .gitignore");
}

/// The crate is a library, not a program. There is no binary to run, no server
/// to start, and no `serve` subcommand: the build *is* the deployment.
#[test]
fn the_crate_is_a_library_with_no_binary() {
    let manifest = std::fs::read_to_string(root().join("Cargo.toml")).expect("Cargo.toml");
    assert!(
        !manifest.contains("[[bin]]"),
        "chess_clock is a browser app: a binary would only invite a run step"
    );
    assert!(
        manifest.contains("wasm-bindgen = \"=0.2.128\""),
        "the generator version must be pinned exactly"
    );
    // The Wake Lock API is behind this cfg in web-sys, and it has to be
    // enabled for the wasm target only.
    assert!(
        manifest.contains("[target.'cfg(target_arch = \"wasm32\")'.dependencies]"),
        "the web-sys features belong to the wasm target"
    );
    for feature in ["WakeLock", "WakeLockSentinel", "WakeLockType", "Navigator"] {
        assert!(
            manifest.contains(feature),
            "web-sys must enable {feature}: the wake lock is the app's only job on screen"
        );
    }
}

/// A SHA-256, so the icon digest above needs no dependency. This is the one
/// place a hash is wanted and a crate is not: the alternative is a
/// `build-dependency` used by exactly one assertion.
fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

#[cfg(test)]
mod sha256 {
    //! A minimal SHA-256, for the one digest in the suite.

    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    /// Hash `data`.
    pub fn digest(data: &[u8]) -> [u8; 32] {
        let mut h: [u32; 8] = [
            0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
            0x5be0cd19,
        ];

        let mut message = data.to_vec();
        let bits = (data.len() as u64) * 8;
        message.push(0x80);
        while message.len() % 64 != 56 {
            message.push(0);
        }
        message.extend_from_slice(&bits.to_be_bytes());

        let (blocks, _) = message.as_chunks::<64>();
        for block in blocks {
            let mut w = [0u32; 64];
            for (index, word) in block.as_chunks::<4>().0.iter().enumerate() {
                w[index] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
            }
            for index in 16..64 {
                let s0 = w[index - 15].rotate_right(7)
                    ^ w[index - 15].rotate_right(18)
                    ^ (w[index - 15] >> 3);
                let s1 = w[index - 2].rotate_right(17)
                    ^ w[index - 2].rotate_right(19)
                    ^ (w[index - 2] >> 10);
                w[index] = w[index - 16]
                    .wrapping_add(s0)
                    .wrapping_add(w[index - 7])
                    .wrapping_add(s1);
            }

            let mut v = h;
            for index in 0..64 {
                let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
                let ch = (v[4] & v[5]) ^ ((!v[4]) & v[6]);
                let temp1 = v[7]
                    .wrapping_add(s1)
                    .wrapping_add(ch)
                    .wrapping_add(K[index])
                    .wrapping_add(w[index]);
                let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
                let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
                let temp2 = s0.wrapping_add(maj);

                v[7] = v[6];
                v[6] = v[5];
                v[5] = v[4];
                v[4] = v[3].wrapping_add(temp1);
                v[3] = v[2];
                v[2] = v[1];
                v[1] = v[0];
                v[0] = temp1.wrapping_add(temp2);
            }
            for (slot, value) in h.iter_mut().zip(v) {
                *slot = slot.wrapping_add(value);
            }
        }

        let mut out = [0u8; 32];
        for (index, word) in h.iter().enumerate() {
            out[index * 4..index * 4 + 4].copy_from_slice(&word.to_be_bytes());
        }
        out
    }
}
