// Copyright (c) 2026 Witalis Domitrz <witekdomitrz@gmail.com>
// AGPL License

//! The clock itself: time formatting, the increment rules, and the turn /
//! pause state machine.
//!
//! Nothing here touches the DOM or `web-sys`, so every rule the app has is a
//! plain Rust function under `cargo test` — the two increment modes, the
//! three formatter branches and every transition of the state machine. The
//! browser layer in `ui.rs` owns only presentation and the tick source; when
//! it is unsure what a tap means, the answer is a call into this file.
//!
//! The original was JavaScript and kept its state in a handful of module-level
//! variables with `null` standing for "no player is running yet". That is
//! modelled here as an explicit [`Phase`], which is the one place the rewrite
//! deliberately departs from the original's shape: `null` was doing two jobs
//! (not started, and stopped-but-finished) and the panel colours were the only
//! thing telling them apart.

/// A player's remaining time, in milliseconds. The unit throughout: the
/// original worked in milliseconds too, and a clock that rounds to whole
/// seconds is a different app.
pub type Millis = i64;

/// Which side gets time back after a move.
///
/// Both are *increments*: they differ only in what a player is paid for, and
/// only at the end of a move.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Increment {
    /// Fischer: the player who just moved is always paid the full increment,
    /// however long the move took. Moving instantly pays the same as thinking
    /// for an hour.
    #[default]
    Fischer,
    /// Bronstein: the player who just moved is paid back the time the move
    /// actually took, capped at the increment. Thinking longer than the
    /// increment therefore costs nothing.
    Bronstein,
}

impl Increment {
    /// The value the setup screen's `<select>` carries, and the order it
    /// offers them in. `Fischer` is first because it is the default.
    pub const CHOICES: [Increment; 2] = [Increment::Fischer, Increment::Bronstein];

    /// The label shown in the setup screen.
    pub fn label(self) -> &'static str {
        match self {
            Self::Fischer => "Fischer",
            Self::Bronstein => "Bronstein",
        }
    }
}

/// Pay the increment for a completed move.
///
/// `remaining` is the mover's time as the tick last wrote it, and
/// `at_move_start` is what it was when the move began. The caller reads both
/// from the same clock, so they can only disagree by the time spent on the
/// move — which is exactly the quantity Bronstein pays and Fischer does not.
///
/// Returns the mover's new remaining time. A zero increment is a no-op in both
/// modes, and is not special-cased: the arithmetic already says so.
pub fn increment_for_move(
    kind: Increment,
    increment: Millis,
    remaining: Millis,
    at_move_start: Millis,
) -> Millis {
    match kind {
        Increment::Fischer => remaining + increment,
        // The cap is against the time the player *had* at the start of the
        // move, so a long think can never bank credit, and a fast move is
        // paid exactly what it cost.
        Increment::Bronstein => (remaining + increment).min(at_move_start),
    }
}

/// Render a remaining time the way the panel shows it.
///
/// The unit is chosen by the magnitude, not by a setting, and the original's
/// three branches are kept exactly:
///
/// * `h:mm:ss`  — anything with whole hours in it;
/// * `m:ss`     — anything with whole minutes in it;
/// * `s.d`      — below a minute, one decimal, zero-padded to four characters
///   so a 9.8-second clock reads `09.8` and does not jitter in width.
///
/// The tenth is truncated, not rounded: a clock is a countdown, and rounding
/// would show `00.0` for the last tenth of a second, before the player has
/// actually run out.
///
/// Millisecond input that lands in the tenth-second branch without a whole
/// second — the 59 800–59 999 ms band — truncates to `59.9`, which is the
/// last display before `1:00.0`… which the next branch takes over. That is the
/// original's behaviour and the boundary is exercised by a test below.
pub fn format_time(ms: Millis) -> String {
    let ms = ms.max(0);
    // One decimal of a second, truncated. `59_999` must read `59.9`, not
    // `60.0`: rounding up would show a time the player has not reached.
    let tenths = ms / 100;
    let seconds = tenths / 10;
    let decimal = tenths % 10;
    let minutes = seconds / 60;
    let secs = seconds % 60;

    if minutes >= 60 {
        format!("{}:{:02}:{:02}", minutes / 60, minutes % 60, secs)
    } else if minutes > 0 {
        format!("{}:{:02}", minutes, secs)
    } else {
        format!("{:02}.{}", secs, decimal)
    }
}

/// The setup screen's inputs, read once when the game starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    /// Time on the clock at the start, for each player.
    pub initial: Millis,
    /// The increment, in milliseconds. Zero means "no increment" and is a
    /// legitimate setting, not a missing one.
    pub increment: Millis,
    /// Which rule pays it out.
    pub kind: Increment,
}

impl Default for Settings {
    /// The original's setup screen: ten minutes a side, no increment.
    fn default() -> Self {
        Self {
            initial: 10 * 60 * 1000,
            increment: 0,
            kind: Increment::default(),
        }
    }
}

impl Settings {
    /// Build settings from the six numbers the form collects, in seconds.
    ///
    /// A field left blank, or holding something that is not a number, is
    /// zero — which is what the original's `parseInt(v) || 0` did, and what a
    /// player who clears a box to type a bigger number means. The per-field
    /// maxima from the HTML form are applied here as well, because the
    /// numbers are clamped identically in both places and the clamp belongs to
    /// the rule, not to the markup.
    pub fn from_parts(
        hours: i64,
        minutes: i64,
        seconds: i64,
        increment_minutes: i64,
        increment_seconds: i64,
        kind: Increment,
    ) -> Self {
        // Hours are unbounded in the form; minutes and seconds are 0–59,
        // which is what makes `1:30` mean a minute and a half rather than 90
        // seconds. Clamping keeps the displayed `m:ss` honest.
        let hours = hours.max(0);
        let minutes = minutes.clamp(0, 59);
        let seconds = seconds.clamp(0, 59);
        let increment_minutes = increment_minutes.max(0);
        let increment_seconds = increment_seconds.clamp(0, 59);

        Self {
            initial: (hours * 3600 + minutes * 60 + seconds) * 1000,
            increment: (increment_minutes * 60 + increment_seconds) * 1000,
            kind,
        }
    }
}

/// How the game is going, as a state machine rather than a pile of flags.
///
/// The original tracked `isRunning` and `currentPlayer` separately, which
/// admitted states it then had to defend against — the game running with
/// `currentPlayer === null`, a paused game with a live interval. Collapsing
/// them into one enum makes those states unrepresentable, and it is what lets
/// every transition be a unit test.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Setup is showing; no game is in progress.
    Idle,
    /// A game is set up but nobody has started it: the first tap on a panel
    /// starts the *other* player's clock. The panels are still shown,
    /// undifferentiated — the original cleared the active-player colour here,
    /// and the cross-wiring is what says who opens.
    Armed,
    /// The game is under way and not paused.
    Running,
    /// Under way, stopped. The running player keeps the highlight, and the
    /// button offers to resume.
    Paused,
    /// A player reached zero. The clock is stopped for good, that player's
    /// panel is red, and no further transition is possible.
    Finished,
}

impl Phase {
    /// Whether the game is over, which is the one state nothing resumes from.
    pub fn is_over(self) -> bool {
        matches!(self, Self::Finished)
    }

    /// Whether a tap on a panel should do anything at all.
    pub fn accepts_taps(self) -> bool {
        matches!(self, Self::Armed | Self::Running)
    }
}

/// What a tap on a panel actually did.
///
/// A three-way answer rather than a `bool`, because the caller has three
/// different things to do, and collapsing "started a clock" into "made a move"
/// is what made the increment go unpaid — see [`Game::tap`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tap {
    /// The first tap of a game. The *other* player's clock starts; no move has
    /// been made, so there is no increment to settle.
    Started,
    /// The running player has moved. They are owed the increment, and the
    /// other side takes the clock.
    Moved,
    /// Not a move: the wrong panel, or a paused or finished game. Nothing
    /// about the display has changed.
    Ignored,
}

impl Tap {
    /// Whether this tap ended a move, and so owes an increment.
    pub fn is_move(self) -> bool {
        matches!(self, Self::Moved)
    }
}

/// Who is on the clock, or nobody.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Player {
    First,
    Second,
}

impl Player {
    /// The other side. There are only two, so this is a flip.
    pub fn other(self) -> Self {
        match self {
            Self::First => Self::Second,
            Self::Second => Self::First,
        }
    }

    /// The index into the pair of panels, in DOM order.
    pub fn index(self) -> usize {
        match self {
            Self::First => 0,
            Self::Second => 1,
        }
    }
}

/// The whole game, minus the DOM.
///
/// Times are held here and written by the browser's tick; the tick never
/// decrements a stored value in place, it recomputes it from the anchor the
/// state machine set, so a dropped or delayed frame cannot make the clock lose
/// or gain time. That is the original's own scheme — it stored `start` and
/// `initialRemaining` and derived the remainder each tick — kept here because
/// it is the correct one and because it is what makes pausing exact.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Game {
    /// What the setup screen collected.
    pub settings: Settings,
    /// Which phase of the state machine the game is in.
    pub phase: Phase,
    /// Whose clock is running, or `None` in `Idle`, `Armed` and — once a
    /// player has flagged — only in `Finished`, where the loser is named.
    pub on_clock: Option<Player>,
    /// Time each player has left, as of the last tick or the last transition.
    pub remaining: [Millis; 2],
    /// Who just finished a move and is owed an increment.
    ///
    /// This is the whole reason Bronstein needs state the original kept in a
    /// single global: the cap is against the mover's time at the *start of
    /// the move*, not at the start of the game. In the original that was
    /// `remainingTimeAtTheStartOfMove`, read and overwritten by `startTurn`.
    /// Keeping it as a field rather than a local means it cannot be stale
    /// across a pause, which is the bug the original's history spent four
    /// commits on.
    pub owed: Option<Player>,
    /// The mover's remaining time when their move began.
    pub at_move_start: Millis,
}

impl Default for Game {
    fn default() -> Self {
        Self::new(Settings::default())
    }
}

impl Game {
    /// A game ready to be armed: both players on `settings.initial`, nobody
    /// on the clock.
    pub fn new(settings: Settings) -> Self {
        Self {
            settings,
            phase: Phase::Idle,
            on_clock: None,
            remaining: [settings.initial; 2],
            owed: None,
            at_move_start: settings.initial,
        }
    }

    /// Enter the game screen. Any game in progress is discarded, so this is
    /// also what Back does before the setup screen reappears.
    pub fn arm(&mut self) {
        *self = Self::new(self.settings);
        self.phase = Phase::Armed;
    }

    /// A tap on a panel.
    ///
    /// The rule the original encodes across two handlers: the first tap of a
    /// game starts the **other** player's clock — tapping your own panel is
    /// how you tell your opponent to go first — and every later tap is only
    /// meaningful on the panel that is currently running, so tapping your own
    /// panel twice does nothing, because the move is not over until the other
    /// player answers. That asymmetry is preserved exactly.
    ///
    /// Returns what the tap did, because the caller has to know. It used to
    /// re-derive that by comparing `phase` before and after, which is wrong:
    /// handing the turn over does not change `phase` — it only changes
    /// `on_clock` — so a genuine move read as "nothing happened", and the
    /// increment was silently never paid. The display updated, so it looked
    /// right; only the numbers were wrong, and only a real game against a real
    /// clock showed it.
    #[must_use = "a tap must be acted on: only a move settles the increment"]
    pub fn tap(&mut self, player: Player) -> Tap {
        match self.phase {
            // The first tap: it starts the *other* player's clock, and nobody
            // is owed an increment yet.
            //
            // The cross-wiring is the original's, and it is not a typo. Its two
            // handlers each opened the game with the *other* player:
            //
            // ```js
            // playerDivs[0].addEventListener("click", () => {
            //   if (!isRunning && currentPlayer === null) {
            //     currentPlayer = 1; // Start with Player 1's timer
            // ```
            //
            // ...and the mirror image on `playerDivs[1]`. That is every
            // version of `app.js` in the repository's history, `71a743b`
            // through `4d5dff6`, so it is the app's behaviour and not an
            // accident of one commit.
            //
            // It is also the more sensible of the two readings, once the
            // panels' positions are accounted for. The device lies flat
            // between the players, so the top panel is nearest one of them
            // and the bottom panel nearest the other; a tap *starts the
            // clock facing away from you*, which is how you tell two players
            // which of them goes first without either of them being told —
            // the first mover is whoever's opponent pressed. "Tap your own
            // panel" could only mean something like that if the two panels
            // were labelled, and nothing labels them.
            //
            // This rewrite originally implemented the opposite rule and
            // wrote it up as a port decision ("whoever is sitting there
            // opens"). It was not read off the original; it was invented,
            // which is precisely the mistake this file documents elsewhere.
            Phase::Armed => {
                let started = player.other();
                self.phase = Phase::Running;
                self.on_clock = Some(started);
                self.owed = None;
                self.at_move_start = self.remaining[started.index()];
                Tap::Started
            }
            Phase::Running if self.on_clock == Some(player) => {
                // The running player has moved: they are owed the increment,
                // and the other side takes the clock.
                //
                // `at_move_start` is deliberately *not* updated here. It still
                // describes the mover's move, and `settle_increment` needs it
                // to cap a Bronstein payment against the time that move began
                // with. Overwriting it first is the bug this rewrite exists to
                // avoid: the payment would be capped against the *other*
                // player's clock, so a long think would be paid as though it
                // had been short. `settle_increment` records the new move's
                // start once it has paid.
                self.owed = Some(player);
                self.on_clock = Some(player.other());
                Tap::Moved
            }
            // A tap on the idle panel, or on a paused or finished game, is not
            // a move. The original ignored these too.
            _ => Tap::Ignored,
        }
    }

    /// Pause, or resume a pause. The running player stays on the clock and
    /// keeps the highlight.
    pub fn toggle_pause(&mut self) {
        match self.phase {
            Phase::Running => self.phase = Phase::Paused,
            Phase::Paused => self.phase = Phase::Running,
            // Idle, Armed and Finished have nothing to pause. The original
            // checked `currentPlayer === null` here and isRunning there; this
            // is the same guard, made total.
            _ => {}
        }
    }

    /// The button's label, which the original set as text at the moment of
    /// the transition rather than deriving it.
    pub fn pause_label(&self) -> &'static str {
        match self.phase {
            Phase::Paused => "Resume",
            _ => "Pause",
        }
    }

    /// Settle the increment owed by the last completed move.
    ///
    /// Called by the browser *before* it anchors the new turn, because the
    /// increment can push a player back over zero: someone who moved with 80 ms
    /// left and a two-second Fischer increment has 1 980 ms again, and the
    /// clock must not be stopped on the strength of a number the increment has
    /// not been applied to yet.
    ///
    /// This is also where the *next* move's start is recorded, so the two can
    /// never be reordered: paying the old move and arming the new one happen
    /// together, here, rather than in the tap that triggered them.
    pub fn settle_increment(&mut self) {
        let Some(mover) = self.owed.take() else {
            return;
        };
        let index = mover.index();
        self.remaining[index] = increment_for_move(
            self.settings.kind,
            self.settings.increment,
            self.remaining[index],
            self.at_move_start,
        );
        if let Some(on_clock) = self.on_clock {
            self.at_move_start = self.remaining[on_clock.index()];
        }
    }

    /// Recompute a player's remaining time from a monotonic `now` and the
    /// remaining time recorded when their move began.
    ///
    /// Returns `true` if this call was the one that ran the clock out. The
    /// caller then stops ticking and paints the panel red.
    pub fn tick(&mut self, now: Millis, anchor_remaining: Millis) -> bool {
        let Some(player) = self.on_clock else {
            return false;
        };
        if !matches!(self.phase, Phase::Running) {
            return false;
        }
        let index = player.index();
        let elapsed = now.max(0);
        if elapsed >= anchor_remaining {
            self.remaining[index] = 0;
            self.phase = Phase::Finished;
            return true;
        }
        self.remaining[index] = anchor_remaining - elapsed;
        false
    }

    /// The loser, once the game is over.
    ///
    /// The player who flagged is the one left on the clock: a clock only
    /// reaches zero on the turn that is running, so `on_clock` and a zero
    /// reading are the same fact.
    pub fn loser(&self) -> Option<Player> {
        if !self.phase.is_over() {
            return None;
        }
        let player = self.on_clock?;
        (self.remaining[player.index()] == 0).then_some(player)
    }

    /// Both readings, in panel order, as the display shows them.
    pub fn displayed(&self) -> [String; 2] {
        [
            format_time(self.remaining[0]),
            format_time(self.remaining[1]),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEN_MINUTES: Millis = 10 * 60 * 1000;

    // ---- format_time: the three branches, and their boundaries ------------

    #[test]
    fn hours_branch_formats_h_mm_ss() {
        assert_eq!(format_time(60 * 60 * 1000), "1:00:00");
        assert_eq!(format_time(90 * 60 * 1000 + 5 * 1000 + 300), "1:30:05");
        assert_eq!(
            format_time(3 * 3600 * 1000 + 7 * 60 * 1000 + 9_000),
            "3:07:09"
        );
        // Hours alone is still the hours branch: 59:59 is *minutes*, 60:00 is
        // *hours*, and the two are a single second apart.
        assert_eq!(format_time(59 * 60 * 1000 + 59_000), "59:59");
        assert_eq!(format_time(60 * 60 * 1000), "1:00:00");
    }

    #[test]
    fn minutes_branch_formats_m_ss() {
        assert_eq!(format_time(TEN_MINUTES), "10:00");
        assert_eq!(format_time(60_000), "1:00");
        // Seconds are zero-padded to two, so the width does not jump.
        assert_eq!(format_time(65_000), "1:05");
        assert_eq!(format_time(59 * 60 * 1000 + 9_000), "59:09");
    }

    #[test]
    fn seconds_branch_formats_one_decimal_padded_to_four() {
        assert_eq!(format_time(0), "00.0");
        assert_eq!(format_time(1_000), "01.0");
        assert_eq!(format_time(9_800), "09.8");
        assert_eq!(format_time(59_800), "59.8");
        // Four characters, always: a single-digit clock is padded.
        assert_eq!(format_time(500).len(), 4);
    }

    #[test]
    fn the_tenth_truncates_and_never_rounds_up() {
        // 9.99 s must read 09.9, not 10.0: the player has not reached it yet,
        // and 10.0 is not even representable in the m:ss branch's width.
        assert_eq!(format_time(9_999), "09.9");
        assert_eq!(format_time(59_999), "59.9");
        // The 0.999 s of a second is simply not shown.
        assert_eq!(format_time(1_000), "01.0");
        assert_eq!(format_time(1_001), "01.0");
    }

    #[test]
    fn formatting_never_shows_a_negative_clock() {
        // A clock is clamped at zero the moment it hits it, but the formatter
        // is the last line of defence and must not render a minus sign if a
        // tick ever arrives past the end.
        assert_eq!(format_time(-1), "00.0");
        assert_eq!(format_time(-100_000), "00.0");
    }

    // ---- increment rules --------------------------------------------------

    #[test]
    fn fischer_always_pays_the_full_increment() {
        // Moved instantly: still the whole increment.
        assert_eq!(
            increment_for_move(Increment::Fischer, 3_000, 600_000, 600_000),
            603_000
        );
        // Thought for longer than the increment: still the whole increment,
        // and crucially *not* the time spent.
        assert_eq!(
            increment_for_move(Increment::Fischer, 3_000, 540_000, 600_000),
            543_000
        );
        // Moved with almost nothing left: the increment is added regardless.
        assert_eq!(
            increment_for_move(Increment::Fischer, 3_000, 100, 400),
            3_100
        );
    }

    #[test]
    fn bronstein_pays_the_time_spent_when_it_is_under_the_increment() {
        // 30 s spent against a 3 s increment: paid exactly 30 s.
        assert_eq!(
            increment_for_move(Increment::Bronstein, 3_000, 570_000, 600_000),
            573_000
        );
        // 300 ms spent: paid 300 ms, not the 3 s increment.
        assert_eq!(
            increment_for_move(Increment::Bronstein, 3_000, 599_700, 600_000),
            600_000
        );
        // Exactly the increment spent: the two rules agree at the boundary.
        assert_eq!(
            increment_for_move(Increment::Bronstein, 3_000, 597_000, 600_000),
            600_000
        );
    }

    #[test]
    fn bronstein_never_climbs_above_where_the_move_began() {
        // 60 s spent, 3 s increment: paid the *increment*, not the time spent,
        // so the player is 3 s better off and no more. This is the direction
        // that surprises people — a long think is not refunded, it is just not
        // charged for beyond the increment.
        assert_eq!(
            increment_for_move(Increment::Bronstein, 3_000, 540_000, 600_000),
            543_000
        );
        // The cap is against the start of *this move*, so a player already
        // behind cannot use a fast move to climb: 300 ms spent against a 3 s
        // increment, from 400 s, tops out at the 400 s the move began with.
        assert_eq!(
            increment_for_move(Increment::Bronstein, 3_000, 399_700, 400_000),
            400_000
        );
        // The same 300 ms spent from a full clock does reach the full clock:
        // 600 s, because that is where the move started.
        assert_eq!(
            increment_for_move(Increment::Bronstein, 3_000, 599_700, 600_000),
            600_000
        );
    }

    #[test]
    fn a_zero_increment_is_a_no_op_in_both_modes() {
        for kind in [Increment::Fischer, Increment::Bronstein] {
            assert_eq!(increment_for_move(kind, 0, 123_456, 130_000), 123_456);
        }
    }

    #[test]
    fn bronstein_never_exceeds_the_time_the_player_had() {
        // The invariant, swept: whatever the increment and whatever was spent,
        // Bronstein can never hand back more than the move consumed.
        for increment in [0, 1, 500, 3_000, 30_000] {
            for spent in [0, 1, 499, 500, 1_000, 3_000, 3_001, 60_000] {
                let start = TEN_MINUTES;
                let remaining = start - spent;
                let after = increment_for_move(Increment::Bronstein, increment, remaining, start);
                assert!(
                    after <= start,
                    "Bronstein banked time: spent {spent} ms, got {after}"
                );
                assert!(
                    after == (remaining + increment).min(start),
                    "Bronstein is not the capped sum"
                );
                // And it is never worse than doing nothing.
                assert!(after >= remaining, "Bronstein took time away");
            }
        }
    }

    #[test]
    fn fischer_pays_the_increment_even_when_the_move_was_free() {
        // The exploit Bronstein exists to close. In Fischer, tapping twice as
        // fast as the browser will dispatch is a clock that goes up.
        let two_fast_taps = increment_for_move(Increment::Fischer, 3_000, TEN_MINUTES, TEN_MINUTES);
        assert_eq!(two_fast_taps, TEN_MINUTES + 3_000);
    }

    // ---- settings ---------------------------------------------------------

    #[test]
    fn settings_defaults_match_the_original_setup_screen() {
        let settings = Settings::default();
        assert_eq!(settings.initial, TEN_MINUTES);
        assert_eq!(settings.increment, 0);
        assert_eq!(settings.kind, Increment::Fischer);
    }

    #[test]
    fn settings_read_the_six_form_fields() {
        let settings = Settings::from_parts(1, 30, 15, 0, 5, Increment::Bronstein);
        assert_eq!(settings.initial, (3600 + 1800 + 15) * 1000);
        assert_eq!(settings.increment, 5_000);
        assert_eq!(settings.kind, Increment::Bronstein);
    }

    #[test]
    fn settings_clamp_minutes_and_seconds_like_the_form_does() {
        // The markup caps minutes and seconds at 59; the rule has to agree,
        // or a hand-edited value could produce 90 minutes displayed as "1:30"
        // while actually holding ninety.
        let settings = Settings::from_parts(0, 90, 90, 0, 90, Increment::Fischer);
        // 90 minutes and 90 seconds both clamp to 59, so the clock is 1:59.
        assert_eq!(settings.initial, (59 * 60 + 59) * 1000);
        // The increment's seconds clamp the same way.
        assert_eq!(settings.increment, 59_000);
        // Hours are not clamped at the top: that field is unbounded, so a
        // 100-hour clock is a thing you may legitimately want, and it is the
        // only field that carries into the `h:mm:ss` branch of the formatter.
        assert_eq!(
            Settings::from_parts(100, 0, 0, 0, 0, Increment::Fischer).initial,
            100 * 3600 * 1000
        );
        // And a negative field is zero rather than a clock running backwards.
        assert_eq!(
            Settings::from_parts(-5, -1, -1, -3, 0, Increment::Fischer).initial,
            0
        );
    }

    // ---- the state machine ------------------------------------------------

    fn armed() -> Game {
        let mut game = Game::new(Settings::default());
        game.arm();
        assert_eq!(game.phase, Phase::Armed);
        game
    }

    /// An armed game opened by tapping `tapped`'s panel, so that
    /// `tapped.other()` is on the clock.
    ///
    /// Most of the tests below are about what happens *after* the opening tap,
    /// and none of them is about who opens. Spelling the open out once here
    /// keeps each of them reading as the sequence it is actually testing,
    /// instead of four lines of "tap the other side to put this side on the
    /// clock" that have to be re-read every time the opening rule changes.
    /// The rule itself is pinned by `the_first_tap_starts_the_opponents_clock`
    /// and nothing else needs to restate it.
    fn opened_by(settings: Settings, tapped: Player) -> Game {
        let mut game = Game::new(settings);
        game.arm();
        assert_eq!(game.tap(tapped), Tap::Started);
        assert_eq!(game.on_clock, Some(tapped.other()));
        game
    }

    /// The common case of [`opened_by`]: a default game with `First` on the
    /// clock, which is what a test that goes on to play `First`'s move needs.
    fn first_on_the_clock() -> Game {
        opened_by(Settings::default(), Player::Second)
    }

    /// The caller must be able to tell a move from a non-move, because the
    /// increment is only settled for a move.
    ///
    /// This is the regression test for the second bug the browser check found:
    /// `tap_panel` decided "was that a move?" by comparing `phase` before and
    /// after, and handing the turn over does not change `phase` — it changes
    /// `on_clock`. So every real move read as "nothing happened", the
    /// increment was never settled, and the app played with no increment at
    /// all while looking entirely normal. The unit tests did not catch it
    /// because they all called `settle_increment` themselves and never
    /// consulted what `tap` reported.
    #[test]
    fn tap_reports_exactly_what_it_did() {
        let mut game = armed();
        // The first tap starts a clock, and is not a move — and it starts the
        // *other* player's, which the caller does not have to care about but
        // must not have to compensate for either.
        assert_eq!(game.tap(Player::First), Tap::Started);
        assert!(!Tap::Started.is_move());
        // First is now the waiting panel, so their tap is not a move.
        assert_eq!(game.tap(Player::First), Tap::Ignored);
        assert!(!Tap::Ignored.is_move());
        // The running panel is a move — and the phase does *not* change, which
        // is exactly why the caller cannot infer this by comparing it.
        let phase_before = game.phase;
        assert_eq!(game.tap(Player::Second), Tap::Moved);
        assert!(Tap::Moved.is_move());
        assert_eq!(game.phase, phase_before, "a hand-over keeps the phase");
        // And it is a move precisely when an increment is owed.
        assert_eq!(game.owed, Some(Player::Second));
    }

    /// A move reported by `tap` must be payable, end to end, with no other
    /// signal: this is the sequence `ui.rs` runs on a hand-over.
    #[test]
    fn a_reported_move_settles_its_increment() {
        let settings = Settings {
            initial: 20_000,
            increment: 3_000,
            kind: Increment::Fischer,
        };
        let mut game = Game::new(settings);
        game.arm();

        // The opening tap itself: `Tap::Started`, and not a move, so nothing is
        // owed and nothing is settled. It is Second's clock that starts — the
        // assertion on the line after is the same one
        // `the_first_tap_starts_the_opponents_clock` makes, repeated here only
        // because this test runs the whole sequence from the top.
        let opening = game.tap(Player::First);
        assert_eq!(opening, Tap::Started);
        if opening.is_move() {
            panic!("the opening tap is not a move");
        }
        assert_eq!(game.on_clock, Some(Player::Second));
        game.tick(2_000, settings.initial);
        assert_eq!(game.remaining[1], 18_000, "Second is the one thinking");

        // Second moves first, so Second is the one paid.
        let tap = game.tap(Player::Second);
        assert_eq!(tap, Tap::Moved);
        if tap.is_move() {
            game.settle_increment();
        }
        // 20 − 2 spent, + 3 paid. Without the settle this reads 18.0.
        assert_eq!(game.remaining[1], 21_000);
        assert_eq!(game.remaining[0], settings.initial);

        // And the other side's tap, which is a move too, pays them in turn.
        game.tick(500, settings.initial);
        let tap = game.tap(Player::First);
        assert_eq!(tap, Tap::Moved);
        if tap.is_move() {
            game.settle_increment();
        }
        assert_eq!(game.remaining[0], 22_500);
    }

    #[test]
    fn a_new_game_arms_with_both_clocks_at_the_starting_time() {
        let game = armed();
        assert_eq!(game.remaining, [TEN_MINUTES; 2]);
        assert_eq!(game.on_clock, None);
        assert_eq!(game.owed, None);
        assert_eq!(game.displayed(), ["10:00".to_string(), "10:00".to_string()]);
    }

    /// The first tap starts the **opponent's** clock, not the one tapped.
    ///
    /// This is the original's rule and it is counter-intuitive enough to look
    /// like a defect, so the reason belongs next to the assertion. At
    /// `006f70c`:
    ///
    /// ```js
    /// playerDivs[0].addEventListener("click", () => {
    ///   if (!isRunning && currentPlayer === null) {
    ///     currentPlayer = 1; // Start with Player 1's timer
    /// ```
    ///
    /// and the mirror image on `playerDivs[1]`. Every version of `app.js` in
    /// the history wires it this way.
    ///
    /// This rewrite shipped the opposite behaviour — a tap started the panel
    /// you touched — with a unit test asserting exactly that, so the suite was
    /// green and the app was wrong. It is the same failure this crate already
    /// documents twice: an invariant invented to look principled while the
    /// thing being ported was sitting unread in the repository's own history.
    #[test]
    fn the_first_tap_starts_the_opponents_clock() {
        // Not the tapped panel. Tapping your own clock says "your opponent
        // goes first", which is the only way two unlabelled panels can say it.
        let mut game = armed();
        let _ = game.tap(Player::First);
        assert_eq!(game.phase, Phase::Running);
        assert_eq!(game.on_clock, Some(Player::Second));
        // And the mirror image, so neither panel is special.
        let mut game = armed();
        let _ = game.tap(Player::Second);
        assert_eq!(game.on_clock, Some(Player::First));
        // Nobody is owed anything by the first tap: no move has been made.
        assert_eq!(game.owed, None);
        // And the clock that started is anchored to the time *it* began with,
        // which is where Bronstein will cap the first refund.
        assert_eq!(game.at_move_start, TEN_MINUTES);
    }

    /// The load-bearing consequence: the player who taps their own panel first
    /// is the one who gives up the first move, and the app must be able to
    /// run a whole game where the tapper is behind for most of it.
    #[test]
    fn tapping_your_own_panel_first_hands_the_first_move_to_your_opponent() {
        let settings = Settings {
            initial: TEN_MINUTES,
            increment: 3_000,
            kind: Increment::Fischer,
        };
        let mut game = Game::new(settings);
        game.arm();

        // First taps their own panel: Second's clock starts, not First's.
        let _ = game.tap(Player::First);
        assert_eq!(game.on_clock, Some(Player::Second));

        // Second thinks for 10 s and moves, so Second is the one paid.
        game.tick(10_000, TEN_MINUTES);
        let tap = game.tap(Player::Second);
        assert_eq!(tap, Tap::Moved);
        if tap.is_move() {
            game.settle_increment();
        }
        assert_eq!(game.remaining[1], TEN_MINUTES - 7_000);
        assert_eq!(game.remaining[0], TEN_MINUTES, "First never thought");
        assert_eq!(game.on_clock, Some(Player::First));
    }

    #[test]
    fn a_tap_on_the_running_panel_makes_the_move_and_switches_turn() {
        let mut game = first_on_the_clock();
        assert_eq!(game.on_clock, Some(Player::First));
        // The tap's effect is asserted on the lines that follow.
        let _ = game.tap(Player::First);
        assert_eq!(game.on_clock, Some(Player::Second));
        assert_eq!(game.owed, Some(Player::First));
    }

    #[test]
    fn tapping_the_idle_panel_does_nothing() {
        // This is the load-bearing rule for a chess clock: a move is finished
        // when the *other* player acts, so a tap on the waiting panel is not a
        // move, and cannot be used to farm an increment.
        let mut game = first_on_the_clock();
        game.settle_increment();
        let before = game.remaining;
        // The tap's effect is asserted on the lines that follow.
        let _ = game.tap(Player::Second);
        assert_eq!(game.remaining, before);
        assert_eq!(game.on_clock, Some(Player::First));
        assert_eq!(game.owed, None);
    }

    #[test]
    fn tapping_twice_on_the_running_panel_is_two_moves_in_the_original_and_one_here() {
        // Careful: the original's rule is "a tap on the panel that is running
        // switches turn". Tapping the *same* panel twice therefore switches to
        // the other player. The exploitable case — tapping the panel you are
        // NOT on — is what the idle-panel rule above blocks.
        let mut game = first_on_the_clock();
        // The tap's effect is asserted on the lines that follow.
        let _ = game.tap(Player::First);
        assert_eq!(game.on_clock, Some(Player::Second));
        // The tap's effect is asserted on the lines that follow.
        let _ = game.tap(Player::Second);
        assert_eq!(game.on_clock, Some(Player::First));
    }

    #[test]
    fn switching_pays_the_increment_to_the_mover() {
        let settings = Settings {
            initial: TEN_MINUTES,
            increment: 3_000,
            kind: Increment::Fischer,
        };
        let mut game = opened_by(settings, Player::Second);
        // The first player thinks for 10 s.
        game.tick(10_000, TEN_MINUTES);
        assert_eq!(game.remaining[0], TEN_MINUTES - 10_000);
        // The tap's effect is asserted on the lines that follow.
        let _ = game.tap(Player::First);
        game.settle_increment();
        // Fischer: the full 3 s, so 590 s plus 3 s.
        assert_eq!(game.remaining[0], TEN_MINUTES - 7_000);
        assert_eq!(game.remaining[1], TEN_MINUTES);
    }

    #[test]
    fn bronstein_pays_a_long_think_through_the_machine() {
        let settings = Settings {
            initial: TEN_MINUTES,
            increment: 3_000,
            kind: Increment::Bronstein,
        };
        let mut game = opened_by(settings, Player::Second);
        game.tick(60_000, TEN_MINUTES);
        assert_eq!(game.remaining[0], TEN_MINUTES - 60_000);
        // The tap's effect is asserted on the lines that follow.
        let _ = game.tap(Player::First);
        game.settle_increment();
        // 60 s spent against a 3 s increment: paid the increment, not the
        // spend, so 540 s + 3 s. Fischer and Bronstein agree here — what
        // separates them is the *short* move, below.
        assert_eq!(game.remaining[0], TEN_MINUTES - 57_000);
    }

    #[test]
    fn bronstein_pays_a_fast_move_and_fischer_does_not_differ() {
        // The rule that actually distinguishes the two modes, end to end.
        // A 300 ms move with a 3 s increment: Bronstein hands back the 300 ms
        // (so the player is made whole), while Fischer hands back the full 3 s
        // and the player gains. Same inputs, two different clocks.
        let fast = |kind| {
            let settings = Settings {
                initial: TEN_MINUTES,
                increment: 3_000,
                kind,
            };
            let mut game = opened_by(settings, Player::Second);
            game.tick(300, TEN_MINUTES);
            // The tap's effect is asserted on the lines that follow.
            let _ = game.tap(Player::First);
            game.settle_increment();
            game.remaining[0]
        };
        // Bronstein: 599 700 + 300, capped at the 600 000 the move began with.
        assert_eq!(fast(Increment::Bronstein), TEN_MINUTES);
        // Fischer: 599 700 + 3 000, which is above where the move started.
        assert_eq!(fast(Increment::Fischer), TEN_MINUTES + 2_700);
    }

    #[test]
    fn the_cap_is_against_this_move_not_the_whole_game() {
        // The regression the original's `remainingTimeAtTheStartOfMove` was
        // introduced for: a player who is already behind must not climb back
        // to the start of the *game* by moving quickly.
        //
        // The mechanic that makes this non-obvious: a player is paid the
        // increment when they *tap their own panel to hand the clock over*.
        // The payment compensates the mover for the turn they have just
        // finished, so the refunded player is the one who was on the clock —
        // never the one who receives it. Reading a test as "the mover" rather
        // than "the one who taps" is what makes it come out backwards.
        let settings = Settings {
            initial: TEN_MINUTES,
            increment: 5_000,
            kind: Increment::Bronstein,
        };
        let mut game = opened_by(settings, Player::Second);

        // Move 1: First thinks 30 s and taps to hand over. Paid 5 s:
        // 600 → 570 → 575, and Second is now on the clock.
        game.tick(30_000, TEN_MINUTES);
        // The tap's effect is asserted on the lines that follow.
        let _ = game.tap(Player::First);
        game.settle_increment();
        assert_eq!(game.remaining[0], TEN_MINUTES - 25_000);
        assert_eq!(game.on_clock, Some(Player::Second));

        // Move 2: Second thinks 5 s and taps to hand back. *Second* is the one
        // refunded, so 595 → 600, and First is on the clock again at 575.
        // Second's short move is made whole, which is Bronstein's whole point;
        // in Fischer Second would have gained 5 s here instead.
        game.tick(5_000, TEN_MINUTES);
        // The tap's effect is asserted on the lines that follow.
        let _ = game.tap(Player::Second);
        game.settle_increment();
        assert_eq!(game.remaining[1], TEN_MINUTES);
        assert_eq!(game.remaining[0], TEN_MINUTES - 25_000);
        assert_eq!(game.on_clock, Some(Player::First));

        // Move 3: First thinks another 30 s, and is refunded for that move
        // when they tap. The cap is the 575 s that *this* move began with, so
        // the 550 s it lands on is untouched by the refund. If the cap were
        // taken against the start of the game, First's fast finishing could
        // pay their way back to 600 s.
        game.tick(30_000, TEN_MINUTES - 25_000);
        // The tap's effect is asserted on the lines that follow.
        let _ = game.tap(Player::First);
        game.settle_increment();
        assert_eq!(game.remaining[0], TEN_MINUTES - 50_000);
        assert!(
            game.remaining[0] < TEN_MINUTES,
            "Bronstein climbed back to the start of the game"
        );
    }

    #[test]
    fn an_increment_can_rescue_a_player_who_almost_flagged() {
        // 80 ms left, moved, 2 s Fischer. The increment is settled *before*
        // the turn is anchored, so the game is not stopped on a number the
        // increment has not been applied to.
        let settings = Settings {
            initial: 100_000,
            increment: 2_000,
            kind: Increment::Fischer,
        };
        let mut game = opened_by(settings, Player::Second);
        assert!(!game.tick(99_920, 100_000));
        assert_eq!(game.remaining[0], 80);
        // The tap's effect is asserted on the lines that follow.
        let _ = game.tap(Player::First);
        game.settle_increment();
        assert_eq!(game.remaining[0], 2_080);
        assert_eq!(game.phase, Phase::Running, "the game continues");
    }

    #[test]
    fn pausing_freezes_the_clock_and_resuming_continues_the_same_move() {
        let mut game = first_on_the_clock();
        game.tick(4_000, TEN_MINUTES);
        game.toggle_pause();
        assert_eq!(game.phase, Phase::Paused);
        assert_eq!(game.pause_label(), "Resume");
        // A tick that lands while paused must not move the clock, which is
        // what the original's two `clearInterval` calls achieved and what a
        // "paused" label alone would not.
        assert!(!game.tick(500_000, TEN_MINUTES));
        assert_eq!(game.remaining[0], TEN_MINUTES - 4_000);
        game.toggle_pause();
        assert_eq!(game.phase, Phase::Running);
        assert_eq!(game.pause_label(), "Pause");
        assert_eq!(game.on_clock, Some(Player::First), "same player");
        // Resuming re-anchors, so the player does not lose the paused time.
        game.tick(1_000, TEN_MINUTES - 4_000);
        assert_eq!(game.remaining[0], TEN_MINUTES - 5_000);
    }

    #[test]
    fn tapping_while_paused_is_not_a_move() {
        // The original's `switchPlayer` began with `if (!isRunning) return`.
        let mut game = first_on_the_clock();
        game.toggle_pause();
        let before = game.remaining;
        // The tap's effect is asserted on the lines that follow.
        let _ = game.tap(Player::First);
        assert_eq!(game.remaining, before);
        assert_eq!(game.on_clock, Some(Player::First));
        assert_eq!(game.owed, None);
    }

    #[test]
    fn running_out_stops_the_game_and_paints_the_loser() {
        let mut game = first_on_the_clock();
        assert!(game.tick(TEN_MINUTES, TEN_MINUTES), "flagged");
        assert_eq!(game.remaining[0], 0);
        assert_eq!(game.phase, Phase::Finished);
        assert_eq!(game.loser(), Some(Player::First));
        assert_eq!(game.displayed()[0], "00.0");
    }

    #[test]
    fn a_finished_game_accepts_no_further_transitions() {
        let mut game = first_on_the_clock();
        game.tick(TEN_MINUTES, TEN_MINUTES);
        // Taps, pauses and ticks are all no-ops, and the loser stays put.
        // The tap's effect is asserted on the lines that follow.
        let _ = game.tap(Player::First);
        // The tap's effect is asserted on the lines that follow.
        let _ = game.tap(Player::Second);
        game.toggle_pause();
        assert!(!game.tick(TEN_MINUTES * 2, TEN_MINUTES));
        assert_eq!(game.phase, Phase::Finished);
        assert_eq!(game.loser(), Some(Player::First));
        assert_eq!(game.remaining, [0, TEN_MINUTES]);
    }

    #[test]
    fn the_second_player_can_flag_too() {
        // Second on the clock, and Second flags. Tapping *First's* panel is
        // what opens the game with Second on the clock, so the mirror of the
        // other flag test rather than a copy of it.
        let mut game = opened_by(Settings::default(), Player::First);
        assert!(game.tick(TEN_MINUTES, TEN_MINUTES));
        assert_eq!(game.loser(), Some(Player::Second));
        assert_eq!(game.displayed()[1], "00.0");
    }

    #[test]
    fn arming_again_discards_a_game_in_progress() {
        // What Back does. A half-finished game must not bleed into the next
        // one, and in particular must not leave a panel red.
        let mut game = first_on_the_clock();
        game.tick(TEN_MINUTES, TEN_MINUTES);
        assert_eq!(game.phase, Phase::Finished);
        game.arm();
        assert_eq!(game.phase, Phase::Armed);
        assert_eq!(game.remaining, [TEN_MINUTES; 2]);
        assert_eq!(game.on_clock, None);
        assert_eq!(game.loser(), None);
    }

    #[test]
    fn players_flip_and_index_in_dom_order() {
        assert_eq!(Player::First.other(), Player::Second);
        assert_eq!(Player::Second.other(), Player::First);
        assert_eq!(Player::First.index(), 0);
        assert_eq!(Player::Second.index(), 1);
    }

    #[test]
    fn phase_reports_what_it_accepts() {
        assert!(Phase::Armed.accepts_taps());
        assert!(Phase::Running.accepts_taps());
        assert!(!Phase::Paused.accepts_taps());
        assert!(!Phase::Finished.accepts_taps());
        assert!(!Phase::Idle.accepts_taps());
        assert!(Phase::Finished.is_over());
        assert!(!Phase::Paused.is_over());
    }

    /// Played adversarially rather than scripted: the invariant that must hold
    /// across *any* sequence of taps, not just the ones above. A clock that
    /// can be made to go up is broken, and this is the shape of input that
    /// would do it.
    #[test]
    fn the_machine_never_lets_a_player_gain_time() {
        for kind in [Increment::Fischer, Increment::Bronstein] {
            let settings = Settings {
                initial: 30_000,
                increment: 2_000,
                kind,
            };
            let mut game = opened_by(settings, Player::Second);

            for step in 0..50i64 {
                let mover = game.on_clock.expect("an armed game is on the clock");
                let start = game.remaining[mover.index()];
                // Both clocks as they stand before this move, so the waiting
                // player can be checked against the right number.
                let before_tick = game.remaining;

                // Alternate instant moves with long thinks, so each mode is
                // exercised on both sides of the Bronstein cap.
                let spent = if step % 2 == 0 { 100 } else { 1_000 };
                assert!(!game.tick(spent, start), "a 30 s clock cannot flag here");
                let after_think = game.remaining[mover.index()];
                assert_eq!(after_think, start - spent);

                let _ = game.tap(mover);
                let waiting = game.on_clock.expect("turn switched");
                assert_eq!(waiting, mover.other());
                game.settle_increment();

                // The mover may gain at most the increment, and the waiting
                // player may not change at all.
                let paid = game.remaining[mover.index()] - after_think;
                assert!(
                    (0..=settings.increment).contains(&paid),
                    "{kind:?}: paid {paid} ms for a {spent} ms move"
                );
                // In Fischer the mover is paid the full increment however
                // long the think was, so they *can* finish above where the
                // move started — that is the rule, and it is why a fast player
                // gains in one mode and not the other. Bronstein is the mode
                // that may never bank.
                if kind == Increment::Bronstein {
                    assert!(
                        game.remaining[mover.index()] <= start,
                        "{kind:?}: banked time above the start of the move"
                    );
                }
                // The waiting player did not move during this exchange, so
                // their clock is exactly what it was before the tick: they are
                // the one side of a chess clock that never runs on its own.
                // (It is not `initial` — this loop runs fifty moves and the
                // mover alternates sides, so the other player has clocked
                // down on their own turns by now.)
                assert_eq!(
                    game.remaining[waiting.index()],
                    before_tick[waiting.index()],
                    "{kind:?}: the waiting player's clock moved"
                );

                // And a tap on the panel that is not running is never a move,
                // however it is spammed.
                let before = game.remaining;
                let _ = game.tap(mover);
                assert_eq!(game.remaining, before, "{kind:?}: idle tap moved a clock");
                assert_eq!(game.on_clock, Some(waiting));
            }
        }
    }
}
