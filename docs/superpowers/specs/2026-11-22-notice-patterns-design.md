# Notice patterns (design)

Date: 2026-11-22. Status: the owner chose the approach ("1, go ahead") and
handed the rest to the builder; every decision below not quoted from the
owner is marked **(call)**.

## Intent

The Clippy half of Apprentice ("B" in the Watch and learn design). Apprentice
keeps a thin, local eye on routine work, notices when the person does the same
thing on several days, and, at the moment the routine is likely to start
again, offers: "You usually file these. Show me how this time?" One click
starts a Watch-and-learn lesson, so the skill still comes from a narrated
demonstration (the owner chose this over drafting skills from background
observation alone).

## Goals

1. **Watch folders the person picks.** Files added, moved, renamed, or
   removed in those folders are noted. Nothing is watched until a folder is
   added. **(call: no default folder.)**
2. **Find routines.** A routine is the same kind of filing done in a watched
   folder on at least 3 different days within the last 14. **(call)**
3. **Offer at the right moment.** When a new file arrives that a known
   routine usually starts from, a small capsule offers to learn it; Show me
   starts a lesson at once (`apprentice::lessons::start`).
4. **Show what was noticed.** The Skills pane gets a Noticing section: the
   watched folders (add, remove), the routines found, each with Show me and
   Forget, and Forget everything.

## Non-goals

- Drafting a skill without a lesson (option 2, declined by the owner).
- App-sequence routines (the same apps in the same order each morning):
  noisy without window titles, and titles are what this design keeps out.
  The front app is recorded only as context of a file event. **(call)**
- Any model call. Detection and wording are deterministic Rust. **(call)**
- Linux and Windows: the offer capsule and front-app read are macOS code, as
  Watch and learn is. **(call)**

## Anti-goals (failure even if it works)

- Reading file contents, window titles, keystrokes, the clipboard, or the
  screen in the background.
- Anything noticed leaving the Mac.
- Watching anything the person didn't pick, or anything at all while the lab
  is off.
- An offer that steals focus, records by itself, or nags (see limits).

## Constraints from this codebase

- A new Labs feature, off by default: `NoticePatterns`, name
  `notice-patterns`, key `labs.notice_patterns`, group Annotate, audience
  Everyone, macOS, added 20261122; card, description, i18n (all six
  languages plus the French, Portuguese and Russian tables Watch and learn
  uses), policy lock under Annotate like Watch and learn, kept off the web
  page like Watch and learn (`labs::web::on_the_web`). It only works while
  Watch and learn is also on (its button starts a lesson); the card says so
  and the gate checks both. **(call)**
- Gate where the work starts: the watcher thread is not started (and stops)
  unless both labs are on and at least one folder is watched.
- No new dependency. Folders are polled, as `personal_commands` polls the
  config (**call**: polling over an FSEvents crate; see Approaches).
- Owner-only storage under the data folder (`app_paths::create_private_dir`,
  `durable::write_private`, `open_private_append`).
- UI per `docs/design-system.md`; the capsule reuses Rune overlays the way
  `rune_ui::meeting_capsules` does (non-activating, input in drawn regions,
  left out of screen recordings, closes on timeout without acting).
- Files stay under 800 non-test lines; new code lives in
  `src/apprentice/notice/` and `src/rune_ui/skills/noticing.rs` plus a
  capsule module.

## Approaches considered

1. **Poll watched folders, deterministic routines (chosen).** A thread lists
   each watched folder two levels deep every 5 s and diffs snapshots by
   inode, which tells adds, removes, renames, and moves apart. No new crate,
   testable as a pure diff, bounded by an entry cap.
2. **FSEvents through the `notify` crate.** Immediate and cheaper on very
   large folders, but a new dependency, coalesced events that still need a
   snapshot to tell moves from renames, and harder to test.
3. **App-and-window timeline.** Rejected: needs window titles to mean
   anything.

## Design

### Settings

`ApprenticeSettings.watched_folders: Vec<PathBuf>` (default empty, omitted
when empty), `hex settings` key `apprentice.watched_folders` (`Kind::Json`,
a list of absolute paths). A folder that is missing or unreadable is skipped
and shown as such in the pane; it is never removed by itself.

### Watching (`apprentice::notice::watch`)

- One thread, `apprentice-notice`, every 5 s while gated on. Each watched
  root is listed to depth 2 (the root, its subfolders, theirs), skipping
  hidden entries and packages (`.app`, `.scrawl`, …) and stopping at 5,000
  entries per root (log once, keep the partial snapshot). **(call)**
- Snapshot entry: inode, relative path, is-folder. The diff between two
  snapshots yields events:
  - `added` (new inode), `removed` (inode gone),
  - `renamed` (same inode, same folder, new name),
  - `moved` (same inode, different folder; a move that also renames is
    `moved` with `renamed: true`).
  The first snapshot of a root yields nothing.
- Each event is stamped with time and the front app's name
  (`dictation::context::frontmost_other_application`, `None` when Apprentice
  itself is in front).

### What is kept (`apprentice::notice::log`)

`<data>/apprentice/noticed/events.ndjson`, owner-only, one line per event:

```json
{"t": 1795339945843, "root": 0, "kind": "moved", "ext": "pdf",
 "from": "", "to": "Invoices 2026-11", "name": "#-#-# a.pdf", "app": "Finder"}
```

- `root` is the index into `watched_folders` at the time (re-mapped if the
  list changes; events of a removed folder are dropped).
- `from` / `to` are folder paths relative to the root (`""` = the root).
  Folder names are kept: they name the routine ("Invoices 2026-11").
- `name` is the file name's **shape**, never the name: digit runs become `#`,
  letter runs become `a`, other characters stay, extension kept
  ("2026-11-03 Telekom.pdf" → "#-#-# a.pdf"). **(call)**
- Kept 14 days; pruned at start and hourly by rewriting
  (`durable::write_private`), and capped at 20,000 lines (oldest dropped).
- Folders themselves (is-folder entries) are noted only as `added` (a new
  folder is often step one of a routine).

### Routines (`apprentice::notice::routine`, pure)

- **Episode:** the events of one root with no gap over 10 minutes.
- **Step signature** of an event: kind + extension + the destination
  folder's shape (`to` with digit runs as `#`, so "Invoices 2026-11" and
  "Invoices 2026-12" match) + the new name's shape for renames. `added`
  files are not steps (downloads arrive on their own); a folder `added` is.
- **Episode signature:** the ordered, de-duplicated list of its steps; an
  episode without a step is ignored.
- **Routine:** an episode signature seen on at least 3 distinct local days in
  the last 14. Its id is a hash of root path + signature. Its **trigger**
  is the extension most of its episodes' files had when they were `added`
  to the root within 2 hours before the episode, if any.
- **Description** (through `tr`, deterministic): built from the steps, for
  example "Moves PDFs from Downloads into folders like Invoices 2026-11, then
  renames them like 2026-11-03 …" (latest real folder name, the shape for
  names), plus "on 3 days".

### Routine state (`<data>/apprentice/noticed/routines.json`, owner-only)

Per routine: id, root path, signature, description, days seen, last seen,
and state: `new`, `offered` (with last offer time), `shown` (Show me was
pressed: no more offers), `ignored` (Stop asking / Forget: never offered or
listed again; Forget also drops its events). Detection runs after each
episode ends (10 min quiet) and at start.

### The offer

- **When:** a file with a routine's trigger extension is `added` to its root,
  the routine is `new` or `offered`, and none of these hold: an offer for
  this routine in the last 24 h; 3 offers already today; a capture or lesson
  recording; a meeting recording; a dictation recording. **(call)**
- **What:** a capsule at the top right, non-activating, left out of screen
  recordings, input only in its drawn region:

  ```
  ┌──────────────────────────────────────────────────────┐
  │ ◉ New PDF in Downloads                               │
  │   You usually file these (3 days). Show me how?      │
  │                    [Not now] [Stop asking] [Show me] │
  └──────────────────────────────────────────────────────┘
  ```

  Show me (primary): starts a lesson and marks the routine `shown`. Not now:
  closes. Stop asking: marks `ignored`. Closes by itself after 30 s, doing
  nothing. Avoids the meeting capsule's corner.
- A routine without a trigger is never offered by capsule; it is listed in
  the pane, where Show me starts a lesson the same way.

### The Skills pane

A "Noticing" section between the model picker and New lessons, shown when
the lab is on:

```
Noticing
  Watched folders
    ~/Downloads                                  [Remove]
    [Add folder…]
  Routines
    Moves PDFs from Downloads into folders like
    Invoices 2026-11, then renames them (3 days) [Show me] [Forget]
  Apprentice keeps file kinds, folder names and when, for 14 days,
  on this Mac. [Forget everything]
```

Add folder… uses `desktop::file_dialog` (`Target::Folder`). Forget
everything empties the event log and the routine state (two-step confirm,
like Discard).

### Activity log

`ApprenticeAction` gains `RoutineNoticed`, `RoutineOffered`,
`RoutineAccepted`, `RoutineIgnored` with the routine id only, never a path
or name.

### Recovery

- A watched folder macOS refuses (no Files and Folders permission yet):
  listed with "Apprentice can't read this folder" and an Open Settings
  link; nothing else changes.
- Unreadable state files are set aside like Hex's settings
  (`routines.json.unreadable-<sha>`), and start empty.

## Testing

- Pure: snapshot diff (add, remove, rename, move, move+rename, depth and cap
  limits, hidden/package skipping), name shapes, episode splitting, step and
  episode signatures, routine detection (3 days yes, 2 no, outside 14 days
  no), trigger, description text, offer rules (each limit).
- Store: log append/prune/cap, routine state transitions, forget.
- UI: Skills scenes for the Noticing section (empty, folders, a routine) and
  the capsule's view; accessibility names on every control.
- Live: on this Mac, watch a scratch folder, replay a routine on 3 faked
  days (events written with past timestamps), drop a new file, see the
  capsule, press Show me.

## Left to the builder

Exact module split under `src/apprentice/notice/`, the capsule's file and
installer shape (modeled on `meeting_capsules`), the description templates'
wording beyond the example, and the i18n strings.
