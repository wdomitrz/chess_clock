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

/// The committed service worker template.
///
/// A helper rather than a repeated read, because the worker's invariants are
/// now asserted from more than one test and the path to it is long enough that
/// a copy is worth naming.
fn worker_template() -> String {
    let path = root().join("src/service-worker.js");
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

/// Every rule in the stylesheet that sets a `display`, as `(selector, value)`.
///
/// A declaration inside a bare `{ ... }` block has no selector of its own, so
/// it is attached to the selector that preceded it. That is enough for the
/// question being asked -- which `display` a given element ends up with -- and
/// deliberately not a CSS parser.
fn display_rules(page: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut selector = String::new();
    for line in page.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(open) = line.find('{') {
            let head = line[..open].trim();
            if !head.is_empty() {
                selector = head.to_string();
            }
            // `selector { display: none }` written on one line.
            if let Some(value) = declaration(&line[open + 1..], "display") {
                out.push((selector.clone(), value));
            }
        } else if line == "}" {
            selector.clear();
        } else if let Some(value) = declaration(line, "display") {
            if !selector.is_empty() {
                out.push((selector.clone(), value));
            }
        }
    }
    out
}

/// The value of `property` in a run of CSS, or `None` if it is not set there.
/// The value stops at the `!important` marker, so a caller can ask separately
/// whether it was marked.
fn declaration(css: &str, property: &str) -> Option<String> {
    let index = css.find(property)?;
    let after = css[index + property.len()..].trim_start();
    let after = after.strip_prefix(':')?.trim_start();
    let end = after
        .find(|c: char| c == ';' || c == '}' || c == '!' || c.is_whitespace())
        .unwrap_or(after.len());
    Some(after[..end].trim().to_string())
}

/// Whether the `[hidden]` rule's `display` declaration is marked `!important`.
fn hidden_display_is_important(page: &str) -> bool {
    let mut selector = String::new();
    for line in page.lines() {
        let line = line.trim();
        if let Some(open) = line.find('{') {
            let head = line[..open].trim();
            if !head.is_empty() {
                selector = head.to_string();
            }
            if selector.contains("[hidden]") {
                if let Some(index) = line[open + 1..].find("display") {
                    return line[open + 1..][index..].contains("!important");
                }
            }
        } else if line == "}" {
            selector.clear();
        } else if selector.contains("[hidden]") {
            if let Some(index) = line.find("display") {
                return line[index..].contains("!important");
            }
        }
    }
    false
}

/// A screen hidden with the `hidden` attribute must leave the document flow.
///
/// The app switches screens by toggling that attribute alone, so `hidden` is
/// load-bearing for layout here and not merely for accessibility. It cannot be
/// left to win on specificity: this file gives `#game` an id selector, which
/// beats the `.screen[hidden]` that used to hide it. So the hidden game screen
/// stayed a full `100dvh` block in the flow, and the setup screen grew a
/// scroll bar that scrolled down to the game screen sitting behind it.
///
/// Asserting that a `[hidden]` rule merely *exists* cannot catch that -- it did
/// exist, and it lost. So assert the thing that was actually wrong: nothing
/// that sets a screen's `display` may out-specify the `[hidden]` rule.
#[test]
fn a_hidden_screen_cannot_be_beaten_by_a_more_specific_display_rule() {
    let page = strip_comments(&shell());
    let rules = display_rules(&page);

    let (hidden_selector, hidden_value) = rules
        .iter()
        .find(|(selector, _)| selector.contains("[hidden]"))
        .unwrap_or_else(|| {
            panic!("the shell must carry a [hidden] rule: the app hides a screen by attribute")
        });

    assert_eq!(
        hidden_value, "none",
        "`{hidden_selector}` must set display: none, or a hidden screen still renders"
    );

    // `.screen[hidden]` (0,2,0) is too weak to survive `#game` (1,0,0). The
    // one declaration that cannot lose, whatever id selector is added later,
    // is the one `!important` makes win.
    assert!(
        hidden_display_is_important(&page),
        "the [hidden] rule must set `display: none !important` -- without it, \
         any element given an id selector stops being hideable by attribute"
    );

    // And spell the failure out at the elements that actually broke, so the
    // message names them if the general rule is ever weakened again. Any rule
    // that matches `#game` — bare, or qualified with `[hidden]` or a class —
    // is an opponent, and must not be the one laying the element out.
    for (id, expected) in [("game", "grid"), ("controls", "flex")] {
        let given = rules
            .iter()
            .filter(|(selector, _)| {
                selector
                    .split(',')
                    .any(|part| part.trim().starts_with(&format!("#{id}")))
            })
            .map(|(_, value)| value.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            given,
            vec![expected],
            "#{id} must be laid out with `display: {expected}` from exactly one \
             rule -- a second one (say `#{id}[hidden]`) would re-add the \
             element to the flow while it is hidden"
        );
    }
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

/// The worker only ever answers for a URL inside its own app's directory.
///
/// This is the guard that stops one of these apps from taking over the pages it
/// shares an origin with. A service worker registered for a scope is consulted
/// for every URL under that scope, and these apps are all served from the same
/// origin as pages that are not apps at all — so "the scope is small" is a
/// promise, and this test is what keeps it one.
#[test]
fn the_worker_never_answers_outside_its_own_directory() {
    let worker = std::fs::read_to_string(root().join("src/service-worker.js"))
        .expect("the committed worker template");
    assert!(
        worker.contains("new URL('./', self.location.href)"),
        "the worker must resolve its own directory from its location"
    );
    // The guard is a prefix test against that directory, on the request URL,
    // applied before the allowlist decides anything.
    assert!(
        worker.contains("IS_OWN(url)"),
        "the fetch handler must check the request is inside this app's directory; \
         without it a mis-scoped registration serves whatever it cached"
    );
    assert!(
        worker.contains("const IS_OWN = url => url.startsWith(ROOT.href)"),
        "the directory guard must be a prefix test against the worker's own root"
    );
}

/// The page states the worker's scope instead of inheriting it, and cleans up a
/// wider registration left behind by an earlier version.
///
/// A registration outlives the page that created it, and nothing short of an
/// explicit `unregister` takes one away. So the second half is what makes this
/// recoverable without the user clearing their browser: a stale registration
/// is not fixed by a reload, and the newer worker cannot take control of a
/// scope it does not own.
#[test]
fn the_page_states_the_scope_and_releases_a_wider_one() {
    // Comments are stripped first: this file's whole job here is to say what
    // the code does, and `src/ui.rs` explains both the scope and the cleanup
    // in prose, so an assertion on the raw text would be satisfied by the
    // explanation of the thing it is supposed to be checking for.
    let ui =
        strip_rust_comments(&std::fs::read_to_string(root().join("src/ui.rs")).expect("src/ui.rs"));
    assert!(
        ui.contains("register_with_options"),
        "the worker must be registered with an explicit scope; left to default, \
         the scope is whatever directory the registering page sits in"
    );
    assert!(
        ui.contains("RegistrationOptions::new()") && ui.contains("set_scope(SCOPE)"),
        "the scope has to be actually stated, not merely a named constant"
    );
    assert!(
        ui.contains("get_registrations") && ui.contains("unregister"),
        "a stale wider registration survives a reload, a version bump and a \
         reinstall; only an explicit unregister clears it"
    );
}

/// A worker's script is compared by suffix, not by `trim_end_matches`.
///
/// `trim_end_matches` strips a *set of characters*, so a directory whose name
/// ends in those letters is silently treated as ours — and a registration
/// belonging to a sibling app would be torn down. This is a regression test for
/// a real bug in the first version of this code.
#[test]
fn the_script_comparison_strips_a_suffix_rather_than_a_character_set() {
    let ui = std::fs::read_to_string(root().join("src/ui.rs")).expect("src/ui.rs");
    // The prose in this file names the method to explain why it is not used, so
    // the assertion is about code: a call, not the word.
    let calls: Vec<&str> = ui
        .lines()
        .filter(|line| {
            let code = line.split("//").next().unwrap_or(line);
            code.contains("trim_end_matches(")
        })
        .collect();
    assert!(
        calls.is_empty(),
        "`trim_end_matches` strips a character set, not a filename: it would eat \
         any directory ending in those letters and tear down a sibling's worker. \
         Found: {calls:?}"
    );
    assert!(
        ui.contains("strip_suffix(\"service-worker.js\")"),
        "the comparison must strip the one filename it expects"
    );
}

/// The scope is named once, and the page and the worker agree on the directory.
///
/// Two independent resolutions of "where am I" — the page's `./` and the worker's
/// `new URL('./', self.location.href)`. They have to describe the same
/// directory, or the page registers a scope the worker's guard does not match.
#[test]
fn the_scope_is_a_relative_directory_shared_with_the_worker() {
    let ui =
        strip_rust_comments(&std::fs::read_to_string(root().join("src/ui.rs")).expect("src/ui.rs"));
    assert!(
        ui.contains("const SCOPE: &str = \"./\";"),
        "the scope must be the app's own directory, relative — so one build works \
         from any subdirectory"
    );
    assert!(
        worker_template().contains("new URL('./', self.location.href)"),
        "the worker must resolve the same directory the page registered"
    );
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

/// The wake-lock cfg must travel with the repository, and the documentation
/// must not claim otherwise.
///
/// `web_sys_unstable_apis` is behind a cfg, and `cargo:rustc-cfg` in `build.rs`
/// does **not** supply it: that applies only to the building package's own
/// units, and `web-sys` is a registry dependency compiled in its own unit, so
/// the cfg never reaches it. The build then fails with five errors naming
/// `WakeLockSentinel`, `WakeLockType` and `wake_lock`.
///
/// So `.cargo/config.toml` has to be committed, and the docs have to say it is
/// what works. Asserting only the file's presence would let the prose drift
/// back into claiming `build.rs` is sufficient — which is exactly what it did,
/// and exactly what makes a fresh clone fail with the documented commands.
#[test]
fn the_wake_lock_cfg_travels_with_the_repository() {
    // The committed config, and the flag in it.
    let config = std::fs::read_to_string(root().join(".cargo/config.toml"))
        .unwrap_or_else(|error| panic!("reading .cargo/config.toml: {error}"));
    assert!(
        config.contains("--cfg=web_sys_unstable_apis"),
        ".cargo/config.toml must carry --cfg=web_sys_unstable_apis; without it a fresh clone \
         cannot build the wasm target at all, because build.rs's cargo:rustc-cfg never reaches \
         the web-sys dependency",
    );
    // Scoped to the wasm target: the host build compiles no web-sys code, and
    // inheriting an unstable-API flag it has no use for is noise at best.
    assert!(
        config.contains("wasm32-unknown-unknown"),
        ".cargo/config.toml must scope the flag to [target.wasm32-unknown-unknown]",
    );

    // And no document may claim `build.rs` is what makes the build work. The
    // failure mode this guards is a reader trusting the prose and deleting the
    // config file as redundant, which is not recoverable without CI.
    for (name, text) in [
        (
            "README.md",
            std::fs::read_to_string(root().join("README.md")),
        ),
        (
            "AGENTS.md",
            std::fs::read_to_string(root().join("AGENTS.md")),
        ),
    ] {
        let text = text.unwrap_or_else(|error| panic!("reading {name}: {error}"));
        let stripped = strip_comments(&text);
        for lie in [
            "`build.rs` sets the cfg for the wasm target",
            "the only mechanism that actually reaches the crate",
            "is guaranteed to reach the crate being",
        ] {
            assert!(
                !stripped.contains(lie),
                "{name} still claims that `build.rs` supplies the wake-lock cfg (\"{lie}\"). That \
                 is false: cargo:rustc-cfg reaches only the building package's own units, and \
                 web-sys is compiled in its own unit. See .cargo/config.toml.",
            );
        }
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

/// The app must not touch the document from a start function.
///
/// This is the regression test for the bug that made the app unusable while
/// every test passed. `#[wasm_bindgen(start)]` runs when the wasm module is
/// **instantiated**, which is before the document is parsed — so a start
/// function that resolved the shell found nothing, panicked, and left the user
/// looking at a correctly rendered setup form that did nothing when touched.
/// The only clue was `RuntimeError: unreachable` in the console.
///
/// No unit test could see it, because the unit tests never touch a document,
/// and no build check could see it, because the shell and the bindings were
/// each individually correct. So the check is static: the start function must
/// hand off to the readiness gate, and that gate must be what resolves the
/// DOM. A browser check is the only way to *prove* the fix, and it is a
/// one-off, not a test suite — see AGENTS.md.
#[test]
fn startup_defers_to_document_readiness_before_touching_the_dom() {
    let source = std::fs::read_to_string(root().join("src/lib.rs")).expect("src/lib.rs");
    let code = strip_rust_comments(&source);

    // The start function exists, so a panic inside it is still a rejected
    // import the shell can report.
    assert!(
        code.contains("wasm_bindgen(start)"),
        "the app must still start itself on instantiation, so a failure is catchable"
    );
    // And it must not call `mount()` directly. That single call is the whole
    // bug: mount resolves the shell, and at instantiation time there is no
    // shell to resolve.
    assert!(
        !code.contains("ui::mount()"),
        "the start function must not mount directly; it must wait for the \
         document to be parsed (RuntimeError: unreachable on every load)"
    );
    assert!(
        code.contains("ui::start_when_ready()"),
        "the start function must hand off to the readiness gate"
    );

    let ui = std::fs::read_to_string(root().join("src/ui.rs")).expect("src/ui.rs");
    let ui = strip_rust_comments(&ui);
    // The gate checks readiness before mounting.
    assert!(
        ui.contains("ready_state"),
        "the startup path must consult document.readyState before resolving the DOM"
    );
    assert!(
        ui.contains("DOMContentLoaded"),
        "when the document is still loading, startup must wait for DOMContentLoaded"
    );
    // And resolving the shell must never panic.
    assert!(
        !ui.contains(".expect(\"the setup form\")"),
        "startup must not unwrap a DOM lookup; report instead"
    );
    assert!(
        !ui.contains(".expect(\"a panel\")") && !ui.contains(".expect(\"listen for"),
        "listener installation must not unwrap; report instead"
    );
}

/// Nothing the user can read may name the implementation.
///
/// The interface is a chess clock. "Rust", "WebAssembly", "wasm", "JavaScript",
/// "bindings" and "compile" are the authors' and the repository's business, not
/// the player's — a player whose clock will not start needs to be told what to
/// do about it, not what is underneath it. Source comments, `AGENTS.md` and
/// `README.md` are exempt and may say as much as they like.
#[test]
fn no_visible_text_names_the_implementation() {
    let page = shell();
    // What a player can actually be shown: the markup's own text and
    // attributes, with the header comment, the inline stylesheet and the
    // loader script removed. Those three are excluded because the loader and
    // its comments are the one place these words legitimately appear in the
    // file, and the sweep below is about what a player reads.
    let markup = match page.find("<!--") {
        Some(start) => match page[start..].find("-->") {
            Some(end) => &page[start + end + 3..],
            None => "",
        },
        None => page.as_str(),
    };
    let markup: String = markup
        .split("<script")
        .next()
        .unwrap_or("")
        .split("<style")
        .next()
        .unwrap_or("")
        .to_string();
    for banned in [
        "Rust",
        "rust",
        "WebAssembly",
        "wasm",
        "Wasm",
        "JavaScript",
        "javascript",
        "bindings",
        "compile",
        "wasm-bindgen",
    ] {
        assert!(
            !markup.contains(banned),
            "{banned:?} appears in the page's visible markup; the interface must \
             not name the implementation"
        );
    }
    // The failure message is the one string most likely to drift, so check it
    // explicitly rather than trusting the sweep above.
    let ui = std::fs::read_to_string(root().join("src/ui.rs")).expect("src/ui.rs");
    for banned in ["Rust", "WebAssembly", "wasm", "JavaScript", "bindings"] {
        assert!(
            !ui.contains(&format!("\"{banned}")),
            "{banned:?} is used in a rendered string in ui.rs"
        );
    }
}

/// The two readings must turn in OPPOSITE directions, and neither element may
/// set both `rotate` and `transform`.
///
/// Both were wrong at once, and a screenshot was the only thing that showed
/// either:
///
/// * **The double rotation.** The original Tailwind build set one property
///   (`rotate: 90deg`). This set both `rotate: 90deg` and
///   `transform: rotate(90deg)`, "for a browser that only knows the old
///   one". They are two independent transform functions and a browser
///   composes them, so the readings rendered at **180°** — upside down, not
///   rotated. Measured in Chromium on a 200x40 box with a marker on its left
///   edge: `transform:rotate(90deg)` alone puts the marker at (80, -80);
///   `rotate:90deg` alone at (80, -80); both together at (180, 0).
/// * **The direction.** Rotating both panels the same way makes them face
///   each other across the table, and rotating them opposite ways can still
///   point them *inward* rather than outward — which is what a first
///   attempt at this did, and which the tests did not catch, because they
///   only asserted that the two rotations differed. With the phone flat, the
///   top edge faces the player above it and the bottom edge the player below,
///   so each reading has to run *away* from the device's midline: the top
///   panel's digits run upward, the bottom panel's run downward. Which of the
///   two angles achieves that is asserted here by value, not by inequality.
#[test]
fn the_readings_turn_outward_and_never_by_both_properties_at_once() {
    let page = shell();
    let code = strip_comments(&page);

    // Exactly three rotations: the top reading, the bottom reading, the
    // buttons. One per element, and the two panels must differ.
    let rotations: Vec<&str> = code
        .match_indices("transform: rotate(")
        .map(|(at, _)| {
            code[at..]
                .find("deg)")
                .map(|end| &code[at..at + end + 4])
                .unwrap_or("")
        })
        .collect();
    assert_eq!(
        rotations.len(),
        2,
        "two rules are rotated: the shared reading rule and the buttons; \
         found {rotations:?}"
    );
    assert!(
        rotations.iter().all(|r| r.contains("transform: rotate(")),
        "every rotation must be a transform"
    );

    // No element may set `rotate:` and `transform: rotate` together: they are
    // two independent transform functions and a browser composes them, so two
    // 90s make a 180 and the element ends up upside down.
    //
    // Compared per *declaration block*, splitting on `}` — the previous
    // version split on the same character but required `!contains("transform:")`
    // on the same chunk, which a declaration block always satisfies by the
    // time the second property appears... it did not, and the bug slipped
    // through: reintroducing the bug kept this test green. So the two
    // properties are now looked for independently, in one pass over every
    // block, and a block containing both fails.
    for rule in code.split('}') {
        let has_individual_rotate = rule.lines().any(|line| {
            let line = line.trim();
            line.starts_with("rotate:") && line.ends_with(';')
        });
        let has_transform_rotate = rule.contains("transform: rotate(");
        assert!(
            !(has_individual_rotate && has_transform_rotate),
            "a rule sets both `rotate` and `transform: rotate`; they compose and \
             the element turns twice as far as intended: {rule}"
        );
    }

    // One rotation for the readings, one for the buttons: two in total, both
    // 90deg. Not three, and not a pair of opposite angles.
    //
    // Getting here took three attempts, and the wrong ones are worth recording:
    //
    //   * The author's original at `006f70c` gave BOTH spans `.rotate-90`
    //     (`rotate: 90deg`) and both buttons `.rotate-90`. Four elements, one
    //     rotation, facing left.
    //   * This rewrite first rotated each element by both `rotate` and
    //     `transform`, which composes into 180deg -- upside down.
    //   * It then "fixed" that into OPPOSITE angles, 90 on the top panel and
    //     270 on the bottom, on a theory that the two clocks should face
    //     outward from the device's centre. That theory was invented here and
    //     not read off the original, and it was wrong: the original turns both
    //     the same way. Two of its own tests had to be weakened into shape
    //     checks ("the rotations differ") to accommodate it.
    //   * So there is now one 90deg on the shared `.reading` selector, and the
    //     buttons match.
    //
    // Measured against the unmodified original in Chromium: both of its
    // readings compute `rotate: 90deg`; the tops of the glyphs point screen
    // right, so you tilt your head LEFT to read it -- the text faces left.
    let reading_rule = |sel: &str| -> Option<i64> {
        let marker = format!("{sel} {{");
        let at = code.find(&marker)?;
        let tail = &code[at + marker.len()..];
        let block = &tail[..tail.find('}')?];
        let rotate = block.find("rotate(")?;
        let value = &block[rotate + "rotate(".len()..];
        let end_deg = value.find("deg)")?;
        value[..end_deg].parse().ok()
    };

    let readings = reading_rule(".panel .reading")
        .expect(".panel .reading must set the rotation; one shared rule covers both panels");
    assert_eq!(
        readings, 90,
        "the readings are turned 90deg, as in the author's original, which leaves \
         the text facing left -- you tilt your head left to read it"
    );

    // And no per-panel override may reintroduce a second angle: both panels
    // share one rotation, exactly as they do in the original.
    assert!(
        !code.contains("#panel-0 .reading") && !code.contains("#panel-1 .reading"),
        "both panels share one rotation, as in the original; a per-panel rule is \
         how the readings came to be turned in opposite directions"
    );

    let buttons_at = code.find("#controls button {").expect("the controls rule");
    let tail = &code[buttons_at + "#controls button {".len()..];
    let button_block = &tail[..tail.find('}').expect("a closed block")];
    assert!(
        button_block.contains("rotate(90deg)"),
        "the buttons are turned the same way as the readings, as in the original"
    );
}

/// The rotation is the whole point of the two-panel layout, so the panel order
/// is load-bearing: the first row is the player at the top edge.
#[test]
fn the_game_screen_puts_panel_zero_first() {
    let page = shell();
    let first = page.find("id=\"panel-0\"").expect("panel-0");
    let second = page.find("id=\"panel-1\"").expect("panel-1");
    assert!(
        first < second,
        "panel-0 must be the first row; the rotation depends on which edge of \
         the device each player is sitting at"
    );
    let buttons = page.find("id=\"controls\"").expect("the controls");
    assert!(
        second < buttons,
        "the controls are centred between the panels"
    );
}

/// The `apple-mobile-web-app-capable` meta is deprecated and Chromium logs a
/// warning for it on every load. The current spelling is equivalent.
#[test]
fn the_shell_does_not_carry_the_deprecated_web_app_capable_meta() {
    let page = shell();
    assert!(
        !page.contains("apple-mobile-web-app-capable"),
        "Chromium logs a deprecation warning for this on every load"
    );
    assert!(
        page.contains("mobile-web-app-capable"),
        "the current spelling must be present instead"
    );
}

/// Drop `//` line comments, `/* ... */` blocks and `///` doc comments, so an
/// assertion about what the code *does* cannot be satisfied by a comment
/// saying what it does.
fn strip_rust_comments(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let bytes: Vec<char> = source.chars().collect();
    let mut index = 0;
    while index < bytes.len() {
        // A line comment runs to the end of the line, but a `//` inside a
        // string literal is not a comment. The strings this file checks for
        // contain no `//`, so a plain scan is enough and a tokenizer is not
        // worth the complexity here.
        if bytes[index] == '"' {
            out.push('"');
            index += 1;
            while index < bytes.len() {
                out.push(bytes[index]);
                if bytes[index] == '\\' {
                    index += 1;
                    if index < bytes.len() {
                        out.push(bytes[index]);
                    }
                } else if bytes[index] == '"' {
                    index += 1;
                    break;
                }
                index += 1;
            }
            continue;
        }
        if bytes[index] == '/' && index + 1 < bytes.len() && bytes[index + 1] == '/' {
            while index < bytes.len() && bytes[index] != '\n' {
                index += 1;
            }
            continue;
        }
        if bytes[index] == '/' && index + 1 < bytes.len() && bytes[index + 1] == '*' {
            index += 2;
            while index + 1 < bytes.len() && !(bytes[index] == '*' && bytes[index + 1] == '/') {
                index += 1;
            }
            index += 2;
            continue;
        }
        out.push(bytes[index]);
        index += 1;
    }
    out
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

/// A workflow file, as committed.
///
/// Nonexistent is not a reason to fail. The Pages workflow is the only one
/// here that a fresh clone of a *release branch* need not have, and a test
/// that hard-fails on its absence would make a partial export red for a reason
/// that says nothing about the app. Callers say which they want.
fn workflow(name: &str) -> Option<String> {
    let path = root().join(".github/workflows").join(name);
    std::fs::read_to_string(&path).ok()
}

/// The Pages deployment cannot drift from the crate it publishes.
///
/// The deploy job installs its own `wasm-bindgen`, pinned to a literal in the
/// YAML, and compares its digest against a second literal in the same file.
/// `Cargo.toml` pins the same version the crate compiles against. Move the
/// dependency and the workflow keeps building happily: it generates bindings
/// for a runtime the page does not have, and the only symptom is a live site
/// that fails at startup with "Chess Clock could not start" -- for every
/// visitor, and only in the browser. So the two are asserted equal here rather
/// than trusted to be edited together.
#[test]
fn the_pages_build_generates_bindings_for_the_pinned_runtime() {
    let Some(pages) = workflow("pages.yml") else {
        return;
    };
    let manifest = std::fs::read_to_string(root().join("Cargo.toml")).expect("Cargo.toml");

    // Read the pin as Cargo writes it: `wasm-bindgen = "=0.2.128"`, an exact
    // requirement. A looser form ("0.2.128", "^0.2") would resolve to whatever
    // is newest in the lockfile, and the workflow's literal would then be
    // naming one arbitrary version of several -- so the exact form is
    // required, and a manifest that stops using it fails here instead of
    // producing a workflow that builds whatever happened to be current.
    let expected = manifest
        .lines()
        .find_map(|line| {
            let rest = line.trim().strip_prefix("wasm-bindgen")?;
            let rest = rest.trim_start().strip_prefix('=')?;
            Some(rest.trim().trim_matches('"').to_owned())
        })
        .unwrap_or_else(|| panic!("Cargo.toml pins no exact wasm-bindgen version"));

    assert!(
        expected.starts_with('='),
        "Cargo.toml must pin wasm-bindgen exactly ({expected:?}), so there is one version \
         for the crate and one for the bindings generator",
    );
    let version = expected.trim_start_matches('=').trim_matches('"');

    // The version has to reach the job that generates the bindings, as the
    // `version=` it installs -- not merely somewhere in the file, where a
    // mention in a comment would satisfy this and the job would still install
    // whatever else it found. Two spellings are accepted, because `build.yml`
    // sets it in `env:` and reads it back as a shell variable, and a literal
    // is equally correct: what is rejected is a job that names neither.
    let literal = format!("version=\"{version}\"");
    let indirect = "version=\"$WASM_BINDGEN_VERSION\"";
    assert!(
        pages.contains(&literal) || pages.contains(indirect),
        "pages.yml must install wasm-bindgen {version} (`{literal}` or `{indirect}`); it cannot \
         drift from the Cargo.toml pin, or the site fails to start in the browser and nowhere else",
    );
    // A version passed through `env:` is only a single source of truth if the
    // variable is actually declared at the same literal value. An unset
    // variable expands to nothing, and the job would install the empty string
    // -- which passes every check here and publishes bindings for no runtime.
    if pages.contains(indirect) {
        let declared = format!("WASM_BINDGEN_VERSION: {version}");
        assert!(
            pages.contains(&declared),
            "pages.yml installs $WASM_BINDGEN_VERSION but never declares it as {version}; an \
             undeclared variable expands to nothing and the generator step installs nothing",
        );
    }
}

/// The wake-lock cfg is a compiler flag, and only a flag on the compiling
/// invocation turns it on.
///
/// `build.rs` emits `cargo:rustc-cfg=web_sys_unstable_apis` for the wasm
/// target, and that reads like it makes the flag unnecessary. It does not:
/// the wasm build fails without `RUSTFLAGS` in the environment, which is not
/// obvious from the script and cost this workflow's first run its deploy. The
/// two mechanisms are not substitutes, so the workflow has to carry the
/// variable whatever the script does.
///
/// Without it the job dies at the first wasm build with "cannot find
/// `WakeLockSentinel` in crate `web_sys`", naming no feature to add.
#[test]
fn the_pages_build_passes_the_wake_lock_cfg() {
    let Some(pages) = workflow("pages.yml") else {
        return;
    };
    // A commented-out `# RUSTFLAGS:` is not a setting. The check has to read
    // the workflow's own lines with comments removed, or the exact mutation
    // that caused this bug -- commenting the line out instead of deleting it
    // -- satisfies the assertion and ships a job that cannot build.
    let live = strip_yaml_comments(&pages);
    assert!(
        live.contains("RUSTFLAGS: --cfg=web_sys_unstable_apis"),
        "pages.yml must set RUSTFLAGS=--cfg=web_sys_unstable_apis; build.rs's cargo:rustc-cfg \
         does not reach the crate it is emitted from, and the wasm build fails without this with \
         `cannot find WakeLockSentinel in crate web_sys`",
    );
    // And it must be in the top-level `env:`, not on one step: the clippy run
    // builds the wasm lib outside a build.rs pass, so a step-scoped variable
    // would leave that half without the cfg. Top-level keys are at two-space
    // indentation; anything deeper belongs to a job or a step.
    let top_level = live
        .lines()
        .find(|line| line.trim_start().starts_with("RUSTFLAGS:"))
        .expect("RUSTFLAGS must be set somewhere in the workflow");
    assert!(
        top_level.starts_with("  RUSTFLAGS:") && !top_level.starts_with("   "),
        "RUSTFLAGS must be set in the workflow's top-level `env:`, not on a single step, so the \
         clippy wasm run sees it too (found: {top_level:?})",
    );
}

/// The workflow with its comments removed.
///
/// A `#` inside a quoted string is not a comment, and a `#` in a value is not
/// one either -- but this workflow quotes nothing in the keys it is read for
/// and carries no `#` in any value it depends on, so a line-wise cut at the
/// first `#` is enough and a YAML parser is not worth a dependency.
fn strip_yaml_comments(source: &str) -> String {
    source
        .lines()
        .map(|line| match line.find('#') {
            Some(index) => &line[..index],
            None => line,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// A deploy that can run from any branch is a deploy a stranger can run.
///
/// `pages: write` and `id-token: write` are the two permissions that make a
/// GitHub Actions job able to overwrite the live site, and the token behind
/// them is minted for the repository however the workflow was reached. The
/// project *wants* an automatic deploy on every merge to master -- that is the
/// point, and it is why nobody has to remember to publish. What it does not
/// want is that same power on every other ref, so the invariant asserted here
/// is the narrow one that survives the convenience: master is the only ref
/// that can reach the live site, and the publishing permissions live in the
/// one job that is gated on it.
#[test]
fn only_master_can_reach_the_live_site() {
    let Some(pages) = workflow("pages.yml") else {
        return;
    };

    // The trigger must be the named branch, not a bare `push:`. A bare
    // `push:` deploys from every branch that exists, including a contributor's
    // feature branch, and it also changes what `on:` means for pull requests --
    // which is the opposite of the intent.
    assert!(
        pages.contains("branches: [master]"),
        "pages.yml must trigger on `branches: [master]`, not a bare `push:`; a bare push \
         deploys from every branch, including other people's",
    );
    // A tag trigger alongside the branch trigger would publish a version that
    // was never on master.
    assert!(
        !pages.contains("tags:"),
        "pages.yml must not also deploy on tags; a tagged commit that never reached master \
         would be published to the live site",
    );

    // The deploy job's gate, named so the assertion cannot be satisfied by a
    // gate on some other job. This is the check that actually holds when the
    // trigger is widened by accident.
    let deploy_job = pages
        .split("\n  deploy:")
        .nth(1)
        .expect("pages.yml must have a `deploy:` job");
    assert!(
        deploy_job.contains("if:") && deploy_job.contains("github.ref == 'refs/heads/master'"),
        "the `deploy` job must be gated on the build being for master",
    );

    // And the permissions that can actually publish must be scoped to that job
    // rather than granted workflow-wide, so a build step or a third-party
    // action added later cannot spend them.
    let build_job = pages
        .split("\n  build:")
        .nth(1)
        .and_then(|after| after.split("\n  deploy:").next())
        .expect("pages.yml must have a `build:` job");
    assert!(
        !build_job.contains("pages: write") && !build_job.contains("id-token: write"),
        "the `build` job must not hold pages: write or id-token: write; those belong to `deploy`",
    );
}

/// The deploy has to be handed something the upload actually produced.
///
/// `deploy-pages` v5 takes `artifact_name`. There is no `artifact_id` input:
/// passing one is reported as `Unexpected input(s) 'artifact_id'` and the
/// action falls back to its own default, which is only the right answer while
/// the upload side also defaults to the same string. Change one side and the
/// deploy finds no artifact and fails with a bare `HttpError: Not Found` --
/// which says nothing about the artifact, so the cause has to be read out of
/// the workflow, not out of the error.
#[test]
fn the_deploy_is_handed_the_artifact_the_build_uploaded() {
    let Some(pages) = workflow("pages.yml") else {
        return;
    };
    let live = strip_yaml_comments(&pages);

    // The input that v5 does not have. Its presence is a warning at run time
    // and never an error, so nothing else would ever report it.
    assert!(
        !live.contains("artifact_id:"),
        "pages.yml passes `artifact_id` to deploy-pages v5, which has no such input; it is \
         warned about and ignored, leaving the deploy to guess the artifact name",
    );

    // Both sides name the artifact the same way. Read the two keys out of the
    // live text rather than asserting on a fixed string, so the invariant is
    // the agreement and not the particular name.
    //
    // Only the two are read, and only where they are the artifact's own keys:
    // the file also carries the workflow's `name:` and every step's `name:`,
    // and a plain prefix match picks up whichever of those comes first.
    // `artifact_name:` is unique, and the upload's `name:` is the one indented
    // ten spaces -- a step's own `name:` is eight.
    let name_of = |key: &str, indent: usize| {
        live.lines().find_map(|line| {
            let prefix = format!("{}{key}: ", " ".repeat(indent));
            let rest = line.strip_prefix(prefix.as_str())?;
            Some(rest.trim().trim_matches('"').to_owned())
        })
    };
    let uploaded = name_of("name", 10).unwrap_or_else(|| {
        panic!("pages.yml must state the upload step's artifact `name:` so the deploy can match it")
    });
    let deployed = name_of("artifact_name", 10).unwrap_or_else(|| {
        panic!(
            "pages.yml must pass `artifact_name:` to deploy-pages, or it uses a default that \
                can drift from the upload"
        )
    });
    assert_eq!(
        uploaded, deployed,
        "the artifact the build uploads ({uploaded:?}) and the one the deploy asks for \
         ({deployed:?}) must be the same name",
    );
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
