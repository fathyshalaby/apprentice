# Notice patterns Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Apprentice watches folders the person picks, finds filing routines repeated on 3+ days, and offers (capsule or Skills pane) to learn one through a Watch-and-learn lesson.

**Architecture:** A polling thread (`apprentice-notice`) diffs depth-2 folder snapshots by inode into events, keeps them 14 days in an owner-only NDJSON log with file names reduced to shapes, and a pure detector turns them into routines with state in `routines.json`. A Rune overlay capsule (modeled on `rune_ui::meeting_capsules`) offers Show me when a routine's trigger file arrives; the Skills pane gets a Noticing section.

**Tech Stack:** Rust 2024, Rune + rune-kit (UI), serde_json, existing `app_paths` / `durable` storage helpers. No new dependency.

**Spec:** `docs/superpowers/specs/2026-11-22-notice-patterns-design.md` (read it; it is the authority).

## Global Constraints

- New Labs feature, off by default: `Feature::NoticePatterns`, name `notice-patterns`, settings key `labs.notice_patterns`, group Annotate, audience Everyone, OSes macOS, added `20261122`, policy lock under Annotate, excluded from the web page exactly like `WatchAndLearn` (`labs::web::on_the_web`).
- The gate `labs::notice_patterns()` is true only when `labs::watch_and_learn()` is also true; work (the thread, the capsule, the pane section) starts only behind it.
- macOS only: every new module is `#[cfg(all(target_os = "macos", hex_capture))]`, like `src/apprentice/`.
- No new crate in `Cargo.toml`. No model calls. Never read file contents, window titles, keystrokes, clipboard, or the screen in the background.
- Storage under `<data>/apprentice/noticed/`, owner-only: dirs via `app_paths::create_private_dir`, rewrites via `durable::write_private`, appends via `app_paths::open_private_append`.
- Values: poll every 5 s; depth 2; 5,000 entries per root; keep 14 days; cap 20,000 log lines; episode gap 10 min; routine = 3 distinct local days in last 14; trigger window 2 h before an episode; offers: one per routine per 24 h, 3 per day total; capsule closes after 30 s doing nothing.
- Name shape: digit runs → `#`, letter runs → `a`, other characters kept, extension kept (`2026-11-03 Telekom.pdf` → `#-#-# a.pdf`). Folder names kept as-is in the log; folder shape (for signatures) = digit runs → `#`.
- All copy through `tr` / `tr_fill`, sentence case, product name "Apprentice"; every language filled (the six-column tables plus the French, Portuguese, Russian tables `src/desktop/i18n/apprentice.rs` already uses). Every control has an accessibility name. Theme tokens only. One primary button per surface.
- Activity events carry a routine id only (never a path or name): `RoutineNoticed`, `RoutineOffered`, `RoutineAccepted`, `RoutineIgnored`.
- Files under 800 non-test lines; run `python3 scripts/code-shape.py --check`.
- Run rustfmt only on files you changed (`rustfmt --edition 2024 <files>`), never `cargo fmt` on the workspace.
- Tests: `cargo test --locked --bin hex <filter>`; known unrelated failures exist (6 locale/env tests and 2 timing flakes) — compare against them, don't fix them.

## Review Focus

1. A watched folder that disappears, is renamed, or is unreadable (TCC) mid-run: the thread keeps going, the folder shows as unreadable, nothing panics, and no events are invented for its vanished files (Task 2 test `a_root_that_vanishes_yields_no_removals`, Task 5 test `an_unreadable_root_is_reported_and_skipped`).
2. The watched-folder list changes while events exist: events of a removed folder are dropped and indices re-map (Task 3 test `removing_a_folder_drops_its_events_and_remaps_the_rest`).
3. Day boundaries: "3 distinct days" uses the local calendar day, and 3 episodes on one day are not a routine (Task 4 test `three_episodes_on_one_day_are_not_a_routine`).
4. The person turns the lab off (or Watch and learn off) while the thread runs or a capsule shows: the thread stops within one tick and the capsule hides (Task 5 test `the_thread_stops_when_the_gate_closes`, Task 6 test `the_offer_hides_when_the_gate_closes`).
5. Apprentice's own files: lesson bundles written into a watched folder (`.scrawl`), hidden files, and Apprentice moving files itself never become routines (Task 2 test `packages_and_hidden_entries_are_skipped`).

---

### Task 1: The lab, the gate, and the watched-folders setting

**Files:**
- Modify (mirror every `WatchAndLearn` / `watch_and_learn` occurrence): `src/labs/feature.rs`, `src/labs/catalog.rs`, `src/labs/group.rs`, `src/labs/card.rs`, `src/labs/description.rs`, `src/labs/gates.rs`, `src/labs.rs`, `src/labs/web.rs`, `src/app_settings/labs.rs`, `src/policy.rs`, `src/rune_ui/labs.rs`, `src/control/settings.rs`, `src/control/settings/registry.rs`, `src/desktop/i18n/labs.rs`, `src/desktop/i18n/apprentice.rs`
- Modify: `src/app_settings/apprentice.rs`

**Interfaces:**
- Produces: `crate::labs::notice_patterns() -> bool` (gate: own switch && `watch_and_learn()` && on this OS && not policy-locked); `crate::labs::notice_patterns_under(policy) -> bool`; `ApprenticeSettings.watched_folders: Vec<PathBuf>` (`#[serde(default, skip_serializing_if = "Vec::is_empty")]`); `hex settings` key `apprentice.watched_folders` (`Kind::Json`, value a JSON array of absolute path strings; reject relative paths and non-arrays with the registry's usual error).
- Card copy (title "Notice patterns"; line): "Watches folders you pick for filing you repeat, and offers to learn it with a lesson. Needs Watch and learn." Description: "Adds a Noticing section to Skills (macOS): Apprentice watches only the folders you add, keeps file kinds, folder names and times for 14 days on this Mac, never file contents or names, and offers to learn a routine you repeat on 3 days. Needs Watch and learn (hex settings labs.notice_patterns)".

- [ ] **Step 1: Write failing tests** — in `src/labs.rs` tests (next to the Watch and learn ones): `notice_patterns_needs_watch_and_learn` (switch on + watch off → gate false; both on → true); `notice_patterns_is_off_by_default`; in `src/app_settings/apprentice.rs` tests: `watched_folders_round_trip_and_are_omitted_when_empty` (serialize default → no `watched_folders` key; `["/a","/b"]` round-trips); in registry tests: `watched_folders_key_takes_a_list_of_absolute_paths` (`["/x"]` ok, `["rel"]` error, `"/x"` error).
- [ ] **Step 2: Run** `cargo test --locked --bin hex notice_patterns watched_folders` — expect FAIL (missing items).
- [ ] **Step 3: Implement** by mirroring `WatchAndLearn` at every recon-listed site (feature.rs variant + `ALL` length bump; catalog audience/added/oses/name/settings_key; group Annotate; card; description; its own `AtomicBool` like `WATCH_AND_LEARN` in labs.rs and gates.rs; app_settings/labs.rs field + default false; policy lock under Annotate; rune_ui/labs.rs row with key `lab-notice-patterns`; control/settings.rs macOS-only message; registry `labs.notice_patterns` Bool; web.rs exclusion + its assert; i18n strings in all languages). Then the setting field and registry key.
- [ ] **Step 4: Run** the filters again (PASS), then `cargo test --locked --bin hex labs::` and `HEX_UPDATE_LABS_JSON=1 cargo test --locked --bin hex labs::web` (labs.json must stay unchanged — the lab is off the web) and `cargo test --locked --bin hex i18n` (every string has every language).
- [ ] **Step 5: Commit** `feat(apprentice): Notice patterns lab and watched folders setting`.

### Task 2: Folder snapshots and their diff (pure)

**Files:**
- Create: `src/apprentice/notice.rs` (module root: `pub(crate) mod snapshot; pub(crate) mod log; pub(crate) mod routine; pub(crate) mod state; pub(crate) mod offer; pub(crate) mod watch;` — add each as its task lands), `src/apprentice/notice/snapshot.rs`
- Modify: `src/apprentice.rs` (`pub(crate) mod notice;`)

**Interfaces:**
- Produces:
  - `pub struct Entry { pub ino: u64, pub rel: PathBuf, pub folder: bool }`
  - `pub struct Snapshot { pub entries: HashMap<u64, Entry>, pub truncated: bool }`
  - `pub fn read(root: &Path, max_depth: usize, max_entries: usize) -> std::io::Result<Snapshot>` (Err only when the root itself can't be read)
  - `pub enum ChangeKind { Added, Removed, Renamed, Moved { renamed: bool } }`
  - `pub struct Change { pub kind: ChangeKind, pub folder: bool, pub from: PathBuf /* rel folder */, pub to: PathBuf /* rel folder */, pub name: String /* current (or last) file name */ }`
  - `pub fn diff(old: &Snapshot, new: &Snapshot) -> Vec<Change>` sorted by `to`, then name
  - `pub fn name_shape(name: &str) -> String`, `pub fn folder_shape(rel: &Path) -> String`
  - consts `DEPTH: usize = 2`, `MAX_ENTRIES: usize = 5_000`

- [ ] **Step 1: Write failing tests** (tempdir fixtures under `std::env::temp_dir()`, removed on drop, like `evidence::tests::Fixture`): `adds_removes_renames_and_moves_are_told_apart` (one of each, plus a move that renames → `Moved { renamed: true }`); `depth_two_is_the_limit` (a file at depth 3 is not seen); `the_entry_cap_truncates` (cap 3 with 5 files → 3 entries, `truncated`); `packages_and_hidden_entries_are_skipped` (`.DS_Store`, `.hidden/`, `x.app/`, `y.scrawl/` contents never listed; the package dir itself is also skipped); `a_root_that_vanishes_yields_no_removals` (`read` → Err; the caller keeps the old snapshot — assert `read` errors); `name_shapes` (`"2026-11-03 Telekom.pdf"` → `"#-#-# a.pdf"`, `"INV-4471.pdf"` → `"a-#.pdf"`, `"Screenshot 2026-11-20 at 09.14.png"` → `"a #-#-# a #.#.png"`, `"README"` → `"a"`); `folder_shapes` (`"Invoices 2026-11"` → `"Invoices #-#"`, `""` → `""`).
- [ ] **Step 2: Run** `cargo test --locked --bin hex notice::snapshot` — FAIL.
- [ ] **Step 3: Implement** with `std::fs::read_dir` + `std::os::unix::fs::MetadataExt::ino`, `symlink_metadata` (never follow links). Packages = folders whose extension is one of `app, scrawl, bundle, photoslibrary, pkg, framework, xcodeproj` (**call**).
- [ ] **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** `feat(apprentice): diff watched folders by inode`.

### Task 3: The event log

**Files:** Create `src/apprentice/notice/log.rs`.

**Interfaces:**
- Consumes: `snapshot::{Change, ChangeKind, name_shape}`.
- Produces:
  - `#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)] pub struct Event { pub t: u64, pub root: usize, pub kind: EventKind, pub folder: bool, pub ext: String, pub from: String, pub to: String, pub name: String /* shape */, #[serde(default, skip_serializing_if = "Option::is_none")] pub app: Option<String> }` with `#[serde(rename_all = "snake_case")] pub enum EventKind { Added, Removed, Renamed, Moved }` (a move that renames stores `kind: moved` and the new name's shape).
  - `pub fn event(change: &Change, root: usize, t: u64, app: Option<String>) -> Event`
  - `pub struct Log { path: PathBuf }`; `Log::open(dir: &Path) -> io::Result<Log>` (creates `dir` private); `append(&self, events: &[Event]) -> io::Result<()>`; `read(&self) -> Vec<Event>` (skips bad lines); `prune(&self, now_ms: u64) -> io::Result<()>` (drop older than `KEEP_MS = 14 days`, keep newest `MAX_LINES = 20_000`); `remap(&self, old: &[PathBuf], new: &[PathBuf]) -> io::Result<()>` (re-index roots by path; drop events of removed roots); `forget(&self, root: Option<usize>, matching: impl Fn(&Event) -> bool) -> io::Result<()>`; `clear(&self) -> io::Result<()>`
  - `pub fn noticed_dir() -> PathBuf` (`<data>/apprentice/noticed`, via `app_paths`)

- [ ] **Step 1: Failing tests:** `events_keep_shapes_not_names` (a Change named `"2026-11-03 Telekom.pdf"` → `name == "#-#-# a.pdf"`, `ext == "pdf"`; the raw name appears nowhere in the serialized line); `append_then_read_round_trips_and_files_are_owner_only` (mode 0600, dir 0700); `prune_drops_old_events_and_caps_lines`; `removing_a_folder_drops_its_events_and_remaps_the_rest` (roots `[A,B,C]` → `[A,C]`: B's events gone, C's `root` becomes 1); `bad_lines_are_skipped`.
- [ ] **Step 2: Run** `cargo test --locked --bin hex notice::log` — FAIL.
- [ ] **Step 3: Implement** (rewrites through `durable::write_private`; appends through `app_paths::open_private_append`).
- [ ] **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** `feat(apprentice): keep noticed file events for 14 days`.

### Task 4: Routines (pure detection and wording)

**Files:** Create `src/apprentice/notice/routine.rs`.

**Interfaces:**
- Consumes: `log::{Event, EventKind}`, `snapshot::folder_shape`.
- Produces:
  - `pub struct Step { pub kind: EventKind, pub folder: bool, pub ext: String, pub to: String /* folder shape */, pub name: Option<String> /* shape, renames only */ }` (`Eq, Hash, Ord`)
  - `pub struct Routine { pub id: String, pub root: usize, pub steps: Vec<Step>, pub days: usize, pub last_seen_ms: u64, pub example_to: Option<String> /* latest real folder name */, pub trigger: Option<String> /* ext */ }`
  - `pub fn episodes(events: &[Event]) -> Vec<Vec<Event>>` (per root, split on gaps > `EPISODE_GAP_MS = 10 * 60_000`)
  - `pub fn signature(episode: &[Event]) -> Vec<Step>` (file `Added` events are not steps; folder `Added` is; ordered by first appearance, de-duplicated)
  - `pub fn detect(events: &[Event], roots: &[PathBuf], now_ms: u64, day_of: impl Fn(u64) -> i64) -> Vec<Routine>` (≥ `MIN_DAYS = 3` distinct `day_of` values within `WINDOW_MS = 14 days`; an episode that ended less than 10 min before `now_ms` is still open and not counted; id = 16 hex chars of SHA-256 over root path + serialized steps — `sha2` is already a dependency, check `Cargo.toml`)
  - `pub fn local_day(ms: u64) -> i64` (local calendar day number via `libc::localtime_r`, as `src/screenshots/save.rs:23` does: days since the epoch of the local date)
  - `pub fn describe(routine: &Routine, root_name: &str) -> String` (through `tr_fill`; e.g. "Moves PDFs from Downloads into folders like Invoices 2026-11, then renames them like #-#-# a.pdf")
- Trigger: for each counted episode, the most common `ext` among file `Added` events in the same root during the 2 h (`TRIGGER_WINDOW_MS`) before its first event; `None` when fewer than half the episodes have one.

- [ ] **Step 1: Failing tests:** `the_invoice_routine_is_found_on_three_days` (episodes on 3 days: folder `Added` "Invoices 2026-09/10/11", `Moved` pdf into it, `Renamed` to `#-#-# a.pdf` → one routine, `days == 3`, `example_to == Some("Invoices 2026-11")`, `trigger == Some("pdf")` when each episode is preceded by a pdf `Added`); `two_days_are_not_enough`; `three_episodes_on_one_day_are_not_a_routine`; `days_outside_fourteen_are_ignored`; `downloads_alone_are_not_steps` (only file `Added` events → no routine); `an_open_episode_waits`; `ids_are_stable_and_differ_by_root`; `describe_reads_like_a_sentence` (exact string for the invoice routine in English).
- [ ] **Step 2: Run** `cargo test --locked --bin hex notice::routine` — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** `feat(apprentice): find routines repeated on three days`.

### Task 5: Routine state, offer rules, and the watcher thread

**Files:** Create `src/apprentice/notice/state.rs`, `src/apprentice/notice/offer.rs`, `src/apprentice/notice/watch.rs`. Modify `src/desktop/events.rs` (four `ApprenticeAction` variants), `src/apprentice/events.rs` if it maps actions, `src/macos_rune.rs` (start the watcher where the apprentice log is set up).

**Interfaces:**
- Consumes: Tasks 2–4; `labs::notice_patterns()`; `app_settings` (current `ApprenticeSettings`, re-read each tick); `dictation::context::frontmost_other_application()`; busy checks `transcription::engine::dictation_busy()`, `meeting::recording::recording()`, `capture_host::snapshot().recording.is_some()`.
- Produces:
  - `#[serde(rename_all = "snake_case")] pub enum RoutineState { New, Offered { last_ms: u64 }, Shown, Ignored }`
  - `pub struct Known { pub id: String, pub root: PathBuf, pub steps: Vec<Step>, pub description: String, pub days: usize, pub last_seen_ms: u64, pub trigger: Option<String>, pub state: RoutineState }`
  - `pub struct Routines { path: PathBuf, pub known: Vec<Known> }`: `load(dir) -> Routines` (unreadable file renamed to `routines.json.unreadable-<sha256 prefix>`, start empty); `save(&self) -> io::Result<()>`; `merge(&mut self, found: Vec<Routine>, roots: &[PathBuf], describe: impl Fn(&Routine) -> String) -> Vec<String> /* ids newly noticed */`; `listed(&self) -> impl Iterator<Item = &Known>` (not `Ignored`); `set(&mut self, id: &str, state: RoutineState)`; `forget(&mut self, id: &str)` (→ `Ignored`)
  - `offer.rs`: `pub struct Busy { pub dictating: bool, pub meeting: bool, pub capturing: bool }`; `pub fn pick(known: &[Known], added_ext: &str, root: &Path, now_ms: u64, offers_today: usize, busy: Busy) -> Option<String /* id */>` (rules from the spec: state New/Offered, trigger matches, last offer ≥ 24 h ago, `offers_today < 3`, not busy)
  - `watch.rs`: `pub enum NoticeEvent { Offer { id: String, title: String /* "New PDF in Downloads" */, line: String /* "You usually file these (3 days). Show me how?" */ }, Changed /* routines or folders changed: pane refresh */ }`; `pub fn start(events: SyncSender<NoticeEvent>, wake: impl Fn() + Send + 'static) -> std::thread::JoinHandle<()>` (thread `apprentice-notice`, ticks every `POLL = 5 s`, does nothing while the gate is closed or no folder is watched, drops its snapshots when the gate closes); `pub fn accept(id: &str)` (→ `Shown`, emits `RoutineAccepted`, calls `lessons::start()`); `pub fn ignore(id: &str)` (→ `Ignored`, emits `RoutineIgnored`, forgets its events); `pub fn forget_everything()`; `pub fn status() -> NoticeStatus` with `pub struct NoticeStatus { pub folders: Vec<(PathBuf, FolderHealth)>, pub routines: Vec<Known> }`, `pub enum FolderHealth { Ok, Unreadable }`.
  - Tick: re-read settings; `Log::remap` when the folder list changed; per root `snapshot::read` (Err → `Unreadable`, keep the old snapshot); `diff`; drop folder changes other than `Added` (spec: folders are noted only as added); stamp with `frontmost_other_application()` name; `append`; on file `Added` with a known trigger → `offer::pick` → send `Offer` (+ `RoutineOffered`, state `Offered`); when the newest episode closed (10 min quiet) and at start → `detect` + `merge` (+ `RoutineNoticed` per new id, send `Changed`); `prune` at start and hourly. Count offers today in memory (resets at local midnight).

- [ ] **Step 1: Failing tests:** state: `merge_keeps_state_and_updates_days`, `an_ignored_routine_is_never_listed_again`, `an_unreadable_state_file_is_set_aside`, files owner-only; offer: one test per rule — `offers_on_a_trigger`, `not_twice_in_a_day`, `not_more_than_three_a_day`, `not_while_busy` (each of the three flags), `never_for_shown_or_ignored`, `not_for_another_folder_or_kind`; watch (with an injectable clock and temp data dir through a `Watcher` struct whose `tick(&mut self, now_ms)` the thread calls): `a_routine_replayed_on_three_days_then_a_new_pdf_offers` (write past-dated events via `Log::append`, then create a pdf in the temp root, two ticks → one `Offer`), `an_unreadable_root_is_reported_and_skipped`, `the_thread_stops_when_the_gate_closes` (gate closure → `tick` touches nothing; snapshots cleared).
- [ ] **Step 2: Run** `cargo test --locked --bin hex notice::` — FAIL.
- [ ] **Step 3: Implement.** Give `watch.rs` a `Watcher { data_dir, clock, gate: fn() -> bool, front_app: fn() -> Option<String>, busy: fn() -> Busy, ... }` so tests inject everything; `start` builds the real one.
- [ ] **Step 4: Run** — PASS; `cargo test --locked --bin hex desktop::events` for the new variants.
- [ ] **Step 5: Commit** `feat(apprentice): watch folders and offer known routines`.

### Task 6: The offer capsule

**Files:** Create `src/rune_ui/routine_offer.rs` (+ `routine_offer/tests.rs` if needed). Modify `src/rune_ui.rs` (module), `src/macos_rune.rs` (install next to `MeetingCapsules::install`, feed `NoticeEvent::Offer` from the watcher's receiver in the pump), `src/desktop/i18n/apprentice.rs`.

**Interfaces:**
- Consumes: `notice::watch::{NoticeEvent, accept, ignore}`, `labs::notice_patterns()`.
- Produces: `pub(crate) struct RoutineOffer`; `RoutineOffer::install<S>(app: &mut rune::App<S>) -> Self`; `fn show(&mut self, id: String, title: String, line: String, now: Instant)`; `fn tick(&mut self, now: Instant)` (hides after `TIMEOUT = 30 s`, and at once when the gate is closed); `pub(crate) fn view(state: &OfferState, p: Paint) -> El` (pure, for tests and a playground story).
- Overlay: `OverlaySpec` named "Apprentice routine offer", top edge, `Align::End`, opposite corner from the meeting capsule when they'd collide (read `MeetingIndicatorCorner`; if the meeting corner is top-right use top-left), `interactive: true`, `OverlayInput::Regions`, `exclude_from_capture`, non-activating. Buttons (rune-kit `Button`): "Not now" (Ghost), "Stop asking" (Secondary), "Show me" (Primary). Show me → `accept(id)` then hide; Stop asking → `ignore(id)` then hide; Not now → hide. Buttons only hand the action on (spawn nothing heavy on the main thread; `accept` sends the lesson command without waiting).

- [ ] **Step 1: Failing tests:** `the_offer_names_every_button` (accessibility names "Show me", "Not now", "Stop asking" present in `view`); `the_offer_closes_after_thirty_seconds_without_acting` (tick at 29 s visible, 31 s hidden, `accept`/`ignore` not called — inject them as `fn(&str)` in `OfferState`); `the_offer_hides_when_the_gate_closes`; `a_new_offer_replaces_the_old_one`.
- [ ] **Step 2: Run** `cargo test --locked --bin hex routine_offer` — FAIL.
- [ ] **Step 3: Implement**; add a playground story "Apprentice/Routine offer" next to the meeting capsule stories if the playground has that pattern.
- [ ] **Step 4: Run** — PASS; render the story or the view with `--snap` if a scene exists for capsules; otherwise note it.
- [ ] **Step 5: Commit** `feat(apprentice): offer to learn a routine`.

### Task 7: The Noticing section in Skills

**Files:** Create `src/rune_ui/skills/noticing.rs`. Modify `src/rune_ui/skills.rs` (state, poll, section between `model_picker::section` and `lesson_cards::section`, empty-state condition), `src/rune_ui/skills/fixtures.rs`, `src/rune_ui/skills/tests.rs`, `src/rune_ui/scenes.rs` (scenes `mac-skills-noticing-empty`, `mac-skills-noticing`), `src/desktop/i18n/apprentice.rs`.

**Interfaces:**
- Consumes: `notice::watch::{status, accept, ignore, forget_everything, NoticeStatus, FolderHealth}`, `desktop::file_dialog::{open, Dialog, Target::Folder, Pending}`, the settings save path the pane already uses for `apprentice.*`.
- Produces: `pub(super) fn section(ui: &Ui, p: Paint) -> Option<El>` (None unless `labs::notice_patterns()`); pane state `noticing: Noticing { status: Option<NoticeStatus>, folder_choice: Option<Pending>, confirm_forget: bool }` refreshed on `NoticeEvent::Changed` and when the pane opens (read off the UI thread like the pane's other snapshots).
- Layout per the spec's sketch: "Noticing" label; "Watched folders" rows (path with `~`, "Can't read this folder" note + "Open Settings" link to `x-apple.systempreferences:com.apple.preference.security?Privacy_FilesAndFolders` for `Unreadable`, Remove); "Add folder…"; "Routines" rows (description + "(N days)", Show me, Forget); the privacy line "Apprentice keeps file kinds, folder names and times for 14 days, on this Mac."; "Forget everything" with two-step confirm (second click within the same view, like Discard). With Watch and learn's model unset, Show me still starts a lesson (the lesson waits for a model, as today).
- Adding/removing a folder saves `apprentice.watched_folders` through the same save path as the model picker, then sends nothing else: the watcher re-reads settings next tick.

- [ ] **Step 1: Failing tests:** `noticing_is_hidden_while_the_lab_is_off`; `noticing_lists_folders_and_routines` (fixture status: one ok folder, one unreadable, one routine) — assert row texts and accessibility names; `forget_everything_asks_twice`; `adding_a_folder_saves_the_setting` (simulate the dialog's answer); `removing_a_folder_saves_the_setting`.
- [ ] **Step 2: Run** `cargo test --locked --bin hex skills::` — FAIL.
- [ ] **Step 3: Implement**; keep `skills.rs` under its size ceiling (move code into `noticing.rs`).
- [ ] **Step 4: Run** — PASS; render `--snap` for both new scenes in light and dark and look at them.
- [ ] **Step 5: Commit** `feat(apprentice): Noticing section in Skills`.

### Task 8: Docs and the feature map

**Files:** Modify `docs/features/apprentice.md` (a "Notice patterns" part: entry points, what is kept, rules, recovery, checks, what's not verified), `docs/features/README.md` if it lists Apprentice parts, `docs/features/labs.md` if it lists labs, `AGENTS.md` Labs list ("…Watch and learn… Notice patterns (macOS)…" only if Watch and learn is listed there; mirror it).

- [ ] **Step 1:** Write the sections from the spec and the code as built (exact values, file paths).
- [ ] **Step 2:** `cargo test --locked --bin hex` (compare with the known failures), `python3 scripts/code-shape.py --check`, `git diff --check`.
- [ ] **Step 3: Commit** `docs(apprentice): notice patterns in the feature map`.
