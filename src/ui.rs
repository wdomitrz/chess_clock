// Copyright (c) 2026 Witalis Domitrz <witekdomitrz@gmail.com>
// AGPL License

//! The browser half: read the form, drive the clock, paint the two panels.
//!
//! Everything here is presentation and plumbing. No rule about how a clock
//! works lives in this file — every question of the form "what happens when
//! this player taps" is a call into [`crate::clock`], and the answer is a
//! value there.
//!
//! Four things are worth pointing out, because each is a decision the original
//! made implicitly, and each is a place a rewrite could easily lose behaviour:
//!
//! * **The tick does not decrement.** It recomputes the running player's
//!   remaining time from an anchor — the value and the timestamp at the last
//!   repaint — so a dropped, delayed or coalesced frame cannot make the clock
//!   lose time. The original did this too, and it is why pausing is exact
//!   rather than approximately exact.
//! * **A wake lock is a held handle, not a call.** `navigator.wakeLock.request`
//!   resolves with a sentinel that has to be kept alive; dropping it releases
//!   the lock. The browser also releases it whenever the page is hidden, which
//!   is why `visibilitychange` re-requests it: a phone put down mid-game is
//!   exactly when the screen must not time out.
//! * **The tick is a chain, not an interval.** `setTimeout` schedules the
//!   *next* tick from when the current one finished, so there is no interval
//!   id to clear when a game ends and no way for a stopped game to keep a
//!   timer running. The original's 100 ms `setInterval` is kept as the period.
//! * **`window().navigator()` and `navigator.wake_lock()` are not optional** in
//!   `web-sys` 0.3.105, however much the browser support is optional. Wrapping
//!   them in an `Option` does not compile, and pretending otherwise is how a
//!   rewrite ends up with an unreachable "not supported" branch.
//!
//! The application is an `Rc<RefCell<_>>`, because the closures the DOM holds
//! need to reach it and `web-sys` offers no other way. `RefCell` is interior
//! mutability that costs one borrow check at run time and no `unsafe`. No
//! callback re-enters another through the same borrow, so the check never
//! actually fires.

use std::cell::{RefCell, RefMut};
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{Element, HtmlButtonElement, HtmlInputElement, HtmlSelectElement};

use crate::clock::{format_time, Game, Increment, Millis, Phase, Player, Settings};

/// How often the display is refreshed while a clock runs.
///
/// The original updated every 100 ms. Nothing depends on the exact figure —
/// readings are recomputed from the anchor, not accumulated — so this is
/// display smoothing only.
const TICK_MS: i32 = 100;

/// The live application, shared with every listener the DOM holds.
type Shared = Rc<RefCell<App>>;

/// The game, its anchor, and the browser handles the app owns.
struct App {
    /// The game. Every rule about it lives in `clock`.
    game: Game,
    /// The running player's remaining time at the last repaint, and when that
    /// was. The tick derives the display from these two alone.
    anchor: (Millis, f64),
    /// The wake lock, if one is held. Dropping it releases the lock.
    wake_lock: Option<web_sys::WakeLockSentinel>,
    /// A strong handle to this application.
    ///
    /// The Wake Lock request resolves in a callback that cannot borrow from
    /// the call that started it, so it needs a second route back to the
    /// sentinel slot. This is that route, and it is why the application is
    /// built with `Rc::new_cyclic` before its listeners rather than after:
    /// patching a handle in afterwards would need a placeholder, and every
    /// placeholder for a `RefCell<App>` is either `unsafe` or a lie about the
    /// type.
    shared: Shared,
    /// The elements, resolved once at mount: a 10 Hz tick should not spend
    /// six DOM lookups a second on six nodes it will use all game.
    dom: Dom,
}

/// The elements the app touches.
struct Dom {
    setup: Element,
    game: Element,
    controls: Element,
    back: HtmlButtonElement,
    panels: [Element; 2],
    readings: [Element; 2],
    pause: HtmlButtonElement,
    hours: HtmlInputElement,
    minutes: HtmlInputElement,
    seconds: HtmlInputElement,
    increment_minutes: HtmlInputElement,
    increment_seconds: HtmlInputElement,
    increment_type: HtmlSelectElement,
    increment_fields: [Element; 2],
}

/// The monotonic clock, in milliseconds.
///
/// `performance.now()` is what the original used and is the right source: it
/// is monotonic, so a system clock change mid-game cannot move a chess clock.
/// The value is only ever *differenced* against an anchor, so neither its
/// sub-millisecond precision nor its float representation ever reaches the
/// display.
fn now() -> f64 {
    web_sys::window()
        .and_then(|window| window.performance())
        .map(|performance| performance.now())
        // `performance` is on every `Window` there is. This is a total
        // function rather than a best-effort one, because a tick has to have
        // a clock to read.
        .unwrap_or_else(js_sys::Date::now)
}

/// The window. A wasm page always has one, so the unwrap is a panic rather
/// than an `Option` threaded through every tick.
fn window() -> web_sys::Window {
    web_sys::window().expect("a wasm page has a window")
}

/// Read a number field. An empty or non-numeric field is zero, which is what
/// the original's `parseInt(value) || 0` did and what clearing a box in order
/// to type a larger number means. A negative value is zero, matching the
/// inputs' `min="0"`.
fn number_field(input: &HtmlInputElement) -> i64 {
    input.value().trim().parse::<i64>().unwrap_or(0).max(0)
}

/// The player whose panel is `index` — the DOM order of the two panels.
fn player_at(index: usize) -> Player {
    if index == 0 {
        Player::First
    } else {
        Player::Second
    }
}

/// Assemble the application and install it. Called once, by the crate's
/// `start` function, which `wasm-bindgen` runs when the bindings load.
pub fn mount() {
    let Some(document) = window().document() else {
        return;
    };
    let Some(dom) = Dom::resolve(&document) else {
        // The shell and the app are committed together and `tests/shell.rs`
        // checks the ids they share, so a built site cannot reach this.
        return;
    };

    let app: Shared = Rc::new_cyclic(|weak| {
        RefCell::new(App {
            game: Game::new(Settings::default()),
            anchor: (0, 0.0),
            wake_lock: None,
            dom,
            shared: weak
                .upgrade()
                .expect("new_cyclic hands its closure a live weak reference"),
        })
    });

    // Start is a submit inside a form, as in the original — which means the
    // keyboard's Enter key starts a game with no separate key handler.
    {
        let app = Rc::clone(&app);
        let form = document
            .get_element_by_id("setup-form")
            .expect("the setup form");
        let handler = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
            event.prevent_default();
            start_game(&app);
        });
        form.add_event_listener_with_callback("submit", handler.as_ref().unchecked_ref())
            .expect("listen for submit");
        handler.forget();
    }

    // The increment type has no effect when the increment is zero, so the two
    // increment fields are dimmed whenever they would do nothing. This is the
    // one behaviour the rewrite adds: the original left the select live and
    // silently ignored it, which is the kind of thing that gets reported as a
    // bug. The fields are disabled rather than hidden, so the form does not
    // change shape under the user's finger.
    {
        let app = Rc::clone(&app);
        let handler = Closure::<dyn FnMut(web_sys::Event)>::new(move |_event: web_sys::Event| {
            let app = app.borrow();
            sync_increment_fields(&app);
        });
        for id in ["increment-type", "increment-minutes", "increment-seconds"] {
            let field = document.get_element_by_id(id).expect("an increment field");
            for event in ["input", "change"] {
                field
                    .add_event_listener_with_callback(event, handler.as_ref().unchecked_ref())
                    .expect("listen for a field event");
            }
        }
        handler.forget();
    }

    // The two panels. A tap on a panel is the only way a turn changes.
    for (index, player) in [Player::First, Player::Second].into_iter().enumerate() {
        // The element is resolved *before* the clone is moved into the
        // closure: `move` takes the `Rc` by value, so anything read from
        // `app` afterwards has to have been read first.
        let panel = app.borrow().dom.panels[index].clone();
        let app = Rc::clone(&app);
        let handler = Closure::<dyn FnMut(web_sys::Event)>::new(move |_event: web_sys::Event| {
            tap_panel(&app, player);
        });
        // `click`, as the original used: it is the one event that fires for a
        // tap, for Enter or Space on a focused panel, and for a synthetic
        // click. The panels carry `role="button"` and `tabindex="0"`, so a
        // keyboard can switch turns too.
        panel
            .add_event_listener_with_callback("click", handler.as_ref().unchecked_ref())
            .expect("listen for clicks on a panel");
        handler.forget();
    }

    {
        let pause = app.borrow().dom.pause.clone();
        let app = Rc::clone(&app);
        let handler = Closure::<dyn FnMut(web_sys::Event)>::new(move |_event: web_sys::Event| {
            toggle_pause(&app);
        });
        pause
            .add_event_listener_with_callback("click", handler.as_ref().unchecked_ref())
            .expect("listen for pause");
        handler.forget();
    }

    {
        let back = app.borrow().dom.back.clone();
        let app = Rc::clone(&app);
        let handler = Closure::<dyn FnMut(web_sys::Event)>::new(move |_event: web_sys::Event| {
            go_back(&app);
        });
        back.add_event_listener_with_callback("click", handler.as_ref().unchecked_ref())
            .expect("listen for back");
        handler.forget();
    }

    // The screen wake lock. Requested when a game starts, and again whenever
    // the page becomes visible: the browser releases the lock on every hide,
    // so without this the second time a player turns the phone over there is
    // no lock, and a chess clock that dims mid-game is the app failing at the
    // one job it has.
    {
        let app = Rc::clone(&app);
        let handler = Closure::<dyn FnMut(web_sys::Event)>::new(move |_event: web_sys::Event| {
            let Some(document) = window().document() else {
                return;
            };
            if document.visibility_state() == web_sys::VisibilityState::Visible {
                request_wake_lock(&app);
            } else {
                // Explicitly drop the handle while hidden. The browser has
                // already released it; forgetting the sentinel lets the page
                // forget too, so a later request is a fresh one rather than a
                // handle for a lock that no longer exists.
                app.borrow_mut().wake_lock = None;
            }
        });
        document
            .add_event_listener_with_callback("visibilitychange", handler.as_ref().unchecked_ref())
            .expect("listen for visibilitychange");
        handler.forget();
    }

    // The service worker. The original registered `sw.js` on load and did
    // nothing else; this registers this app's `service-worker.js` the same
    // way, from Rust, so the JavaScript budget stays at the generated
    // bindings, the one loader line and the worker itself. Registration is not
    // awaited and its failure is ignored: an offline-capable install is a
    // bonus, and failing to register one must not stop the clock.
    {
        let handler = Closure::<dyn FnMut(web_sys::Event)>::new(move |_event: web_sys::Event| {
            let _ = window()
                .navigator()
                .service_worker()
                .register("./service-worker.js");
        });
        document
            .add_event_listener_with_callback("DOMContentLoaded", handler.as_ref().unchecked_ref())
            .expect("listen for DOMContentLoaded");
        handler.forget();
    }

    // The first paint: the setup screen, both readings at their starting time,
    // and the increment fields' state. Nothing is scheduled until a clock is
    // actually running, so an idle page has no timer at all.
    {
        let app = app.borrow();
        sync_increment_fields(&app);
        render(&app);
    }

    // `app` is not stored anywhere else, and needs no be: every closure
    // installed above owns a clone, the DOM holds the closures, and the page
    // holds the DOM. The application therefore lives exactly as long as the
    // listeners that drive it.
}

impl Dom {
    /// Resolve every element the app needs, or `None` if the shell is not the
    /// one this was written against.
    fn resolve(document: &web_sys::Document) -> Option<Self> {
        let get = |id: &str| document.get_element_by_id(id);
        let input = |id: &str| get(id)?.dyn_into::<HtmlInputElement>().ok();
        let button = |id: &str| get(id)?.dyn_into::<HtmlButtonElement>().ok();
        Some(Self {
            setup: get("setup")?,
            game: get("game")?,
            controls: get("controls")?,
            back: button("back")?,
            panels: [get("panel-0")?, get("panel-1")?],
            readings: [get("reading-0")?, get("reading-1")?],
            pause: button("pause")?,
            hours: input("hours")?,
            minutes: input("minutes")?,
            seconds: input("seconds")?,
            increment_minutes: input("increment-minutes")?,
            increment_seconds: input("increment-seconds")?,
            increment_type: get("increment-type")?
                .dyn_into::<HtmlSelectElement>()
                .ok()?,
            increment_fields: [
                get("field-increment-minutes")?,
                get("field-increment-seconds")?,
            ],
        })
    }
}

/// Read the form into settings.
fn read_settings(app: &App) -> Settings {
    let kind = match app.dom.increment_type.value().as_str() {
        "bronstein" => Increment::Bronstein,
        // Fischer is the default and the select's first option, so anything
        // unrecognised is Fischer rather than an error.
        _ => Increment::Fischer,
    };
    Settings::from_parts(
        number_field(&app.dom.hours),
        number_field(&app.dom.minutes),
        number_field(&app.dom.seconds),
        number_field(&app.dom.increment_minutes),
        number_field(&app.dom.increment_seconds),
        kind,
    )
}

/// Dim the increment fields when there is no increment, so the form says what
/// it will actually do.
fn sync_increment_fields(app: &App) {
    let zero = number_field(&app.dom.increment_minutes) == 0
        && number_field(&app.dom.increment_seconds) == 0;
    for field in &app.dom.increment_fields {
        let _ = field.set_attribute("data-inert", if zero { "true" } else { "false" });
    }
    // The type only matters when there is an increment for it to type, so it
    // goes with them.
    app.dom.increment_type.set_disabled(zero);
}

/// Start a game from the form.
fn start_game(app: &Shared) {
    let mut state: RefMut<App> = app.borrow_mut();
    state.game = Game::new(read_settings(&state));
    state.game.arm();

    let _ = state.dom.setup.set_attribute("hidden", "true");
    let _ = state.dom.game.remove_attribute("hidden");
    let _ = state.dom.controls.remove_attribute("hidden");

    // Taken here, while the tap on Start is still the page's most recent
    // gesture: the Wake Lock API requires a visible document, and browsers
    // that require user activation only honour it shortly after a gesture.
    request_wake_lock_from(&mut state);
    render(&state);
}

/// A tap on a panel: a start on the first tap of a game, a move after that.
fn tap_panel(app: &Shared, player: Player) {
    let mut state: RefMut<App> = app.borrow_mut();
    let phase_before = state.game.phase;
    state.game.tap(player);
    if state.game.phase == phase_before {
        // Not a move: the wrong panel, or a paused or finished game. Nothing
        // about the display has changed, so it is left alone.
        return;
    }

    if phase_before == Phase::Armed {
        // The first tap of a game *starts* a clock; it does not end a move,
        // so there is nothing to settle.
    } else {
        // A new move has begun, so the increment owed by the last one is paid
        // first — before the new turn is anchored, because an increment can
        // lift a player back over zero, and the clock must not be stopped on a
        // number that is about to change.
        state.game.settle_increment();
    }

    if let Some(on_clock) = state.game.on_clock {
        state.anchor = anchor(&state, on_clock);
    }
    render(&state);
    // Only now, with a clock actually running, is a tick worth scheduling.
    // Pausing needs no cleanup either way: the tick is a chained timeout that
    // checks the phase before it does anything, so it stops rescheduling by
    // itself and there is no interval id to clear.
    if state.game.phase == Phase::Running {
        schedule_tick(app);
    }
}

/// The pair the tick derives the running clock from: the remaining time at
/// the last repaint, and when that was.
///
/// Rebasing on every anchor is what makes a pause exact. The time already
/// spent is folded into `remaining` as each tick lands, so a fresh anchor's
/// elapsed restarts from zero against what is left — no time is double
/// counted, and none is skipped.
fn anchor(app: &App, player: Player) -> (Millis, f64) {
    (app.game.remaining[player.index()], now())
}

/// Pause or resume.
fn toggle_pause(app: &Shared) {
    let mut state: RefMut<App> = app.borrow_mut();
    let phase_before = state.game.phase;
    state.game.toggle_pause();
    if state.game.phase == phase_before {
        return; // nothing to pause, or nothing to resume
    }
    if state.game.phase == Phase::Running {
        // Resuming re-anchors, so the paused time is not lost.
        if let Some(on_clock) = state.game.on_clock {
            state.anchor = anchor(&state, on_clock);
        }
    }
    render(&state);
    if state.game.phase == Phase::Running {
        schedule_tick(app);
    }
}

/// Back to the setup screen. The game is discarded — including a flagged one,
/// so no panel is left red — and the wake lock released, because holding it
/// would keep a phone's screen awake over a form nobody is looking at.
fn go_back(app: &Shared) {
    let mut state: RefMut<App> = app.borrow_mut();
    state.game = Game::new(state.game.settings);
    state.wake_lock = None;

    let _ = state.dom.setup.remove_attribute("hidden");
    let _ = state.dom.game.set_attribute("hidden", "true");
    let _ = state.dom.controls.set_attribute("hidden", "true");
    render(&state);
}

/// Schedule one tick, which schedules the next.
fn schedule_tick(app: &Shared) {
    let closure = Closure::<dyn FnMut()>::new({
        let app = Rc::clone(app);
        move || tick(&app)
    });
    let _ = window().set_timeout_with_callback_and_timeout_and_arguments_0(
        closure.as_ref().unchecked_ref(),
        TICK_MS,
    );
    // A failure to schedule ends the chain; the clock still shows the value it
    // had, and the next tap re-anchors and schedules again.
    closure.forget();
}

/// Advance the display one tick, and queue the next if the game wants it.
fn tick(app: &Shared) {
    let mut state: RefMut<App> = app.borrow_mut();
    if state.game.phase != Phase::Running {
        // Paused or over: the chain ends here. There is no interval to clear,
        // because there never was one.
        return;
    }
    let Some(player) = state.game.on_clock else {
        return;
    };

    // Measured from the anchor, never accumulated: a frame that arrives late
    // does not cost the player a tenth of a second.
    let (anchor_remaining, anchor_at) = state.anchor;
    let elapsed = (now() - anchor_at).max(0.0) as Millis;
    let flagged = state.game.tick(elapsed, anchor_remaining);
    state.anchor = (state.game.remaining[player.index()], now());

    render(&state);

    if flagged {
        // Over. Nothing is rescheduled, and the wake lock is released: the
        // clock has stopped, so there is no reason to hold the screen on, and
        // a phone that never dims on the results screen is a flat battery.
        state.wake_lock = None;
        return;
    }
    schedule_tick(app);
}

/// Ask for a screen wake lock and hold the sentinel.
///
/// Failure is expected and ignored: the API is not universal, needs a secure
/// context, and is refused outright when the user or the platform says no. A
/// missing wake lock makes the screen dim. It does not make the clock wrong.
///
/// Split in two so a caller that is already holding the borrow does not have
/// to drop it to get here.
fn request_wake_lock(app: &Shared) {
    let mut state: RefMut<App> = app.borrow_mut();
    request_wake_lock_from(&mut state);
}

/// The request itself, with the borrow already taken.
fn request_wake_lock_from(state: &mut App) {
    // Only while a game is in progress. The original guarded on a
    // `gameStarted` flag for exactly this reason, so the setup screen does not
    // hold a lock.
    if !matches!(
        state.game.phase,
        Phase::Armed | Phase::Running | Phase::Paused
    ) {
        return;
    }
    // `request` hands back a promise that *resolves* with the sentinel; it
    // never returns a `Result`. A refusal — the document is hidden, the
    // platform declines, the API is missing — arrives as a rejection, and
    // lands in the second callback. Each of those is a reason for the screen
    // to dim, not a reason for the clock to stop, so it is logged and
    // dropped.
    let promise = window()
        .navigator()
        .wake_lock()
        .request(web_sys::WakeLockType::Screen);

    let app = Rc::clone(&state.shared);
    // The promise is typed `Promise<WakeLockSentinel>`, so the success
    // handler is handed the sentinel rather than a `JsValue` to unwrap.
    let granted = Closure::<dyn FnMut(web_sys::WakeLockSentinel)>::new(
        move |sentinel: web_sys::WakeLockSentinel| {
            // Storing the sentinel *is* holding the lock; it is released when
            // this goes away.
            app.borrow_mut().wake_lock = Some(sentinel);
        },
    );
    let denied = Closure::<dyn FnMut(JsValue)>::new(move |error: JsValue| {
        web_sys::console::error_1(&JsValue::from(
            "chess_clock: the screen wake lock was refused; the display may \
             dim during a game",
        ));
        let _ = error;
    });
    let _ = promise.then(&granted).catch(&denied);
    granted.forget();
    denied.forget();
}

/// Paint everything from the current state.
///
/// One function, called after every transition, so the display cannot describe
/// a state the machine is not in — which is the property the original had to
/// maintain by hand across eight separate DOM mutations.
fn render(app: &App) {
    let game = &app.game;

    for (index, panel) in app.dom.panels.iter().enumerate() {
        let mine = game.on_clock == Some(player_at(index));
        let state = if mine && game.phase.is_over() {
            "flagged"
        } else if mine {
            "active"
        } else {
            "idle"
        };
        let _ = panel.set_attribute("data-state", state);

        let reading = format_time(game.remaining[index]);
        app.dom.readings[index].set_text_content(Some(&reading));
        // The panel is a button and the reading is the only thing about it
        // that changes, so the accessible name is rebuilt with it.
        let _ = app.dom.readings[index]
            .set_attribute("aria-label", &format!("Player {}: {}", index + 1, reading));
    }

    if game.phase.is_over() {
        // A flag is the end of the game, and saying so on the button that used
        // to pause is the one announcement worth making: there is no live
        // region here, because a `role="timer"` updating ten times a second
        // would talk over the whole room.
        app.dom.pause.set_text_content(Some("Time"));
        app.dom.pause.set_disabled(true);
    } else {
        app.dom.pause.set_text_content(Some(game.pause_label()));
        app.dom.pause.set_disabled(false);
    }
}
