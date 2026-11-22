# Watch and learn: record a skill (design)

Date: 2026-11-22. Status: approved by the owner on 2026-11-22.

## Intent

Apprentice is a local, privacy-first, Clippy-style assistant built on this
Hex fork. Its core is **recording a skill**: the person shows Apprentice a
task once, narrating as they go, and Apprentice turns that demonstration
into a reusable `SKILL.md` that coding agents (Claude Code, Codex,
OpenCode, Pi) can follow. Later work builds on this: Apprentice noticing
common patterns on its own (the Clippy part, "B"), then Apprentice
replaying a skill itself.

Order agreed with the owner: (A) record on request → (B) notice repeated
patterns; agents carry out skills first → Apprentice replays later.

Product principles that bind this work: local-first (no cloud model, no
silent fallback), explicit approval for every model run and every skill
saved or installed, visible control (the person sees what was captured,
what the model is asked, and what it wrote), no keystroke or clipboard
capture.

## Goals (this version)

1. **Watch this.** Start a recording that is marked as a lesson, from the
   Skills pane or the menu bar; stop it from the usual capsule.
2. **Draft.** After the recording is written, ask once for approval, then
   run the chosen local model over the recording's text evidence and draft
   a skill.
3. **Why?** The draft marks steps that look like decisions; Apprentice asks
   the person "why?" for each (at most 5), and their answers go into the
   skill.
4. **Review and approve.** The person reads and edits the draft, then
   approves it. Nothing becomes a skill without that approval.
5. **Install into agents.** An approved skill can be installed into the
   coding agents found on this Mac, and removed again.

## Non-goals (this version)

- Noticing repeated patterns in the background (B). Next project.
- Apprentice replaying a skill, autonomy promotion, sandbox practice. The
  skill records an autonomy level (always `observe` for now) so later work
  has a place for it.
- Sending video or audio to a model. Key frames (still images) are sent to
  a vision model; see Drafting.
- Linux and Windows. macOS first; the platform difference is recorded in
  `platform::DIFFERENCES` as pending.
- Cloud or remote models of any kind.

## Anti-goals (failure even if it works)

- Any recording text, frame, or skill leaving the Mac without an explicit,
  visible choice by the person.
- A skill saved or installed without the person approving it.
- Capturing keystrokes or clipboard contents.
- Overwriting an agent skill Apprentice didn't write.

## Constraints from this codebase

- New features land in **Labs, off by default** (`labs::Feature`, its
  settings key, a card, a `hex settings` key), gated where the work starts,
  not only in the UI. Add the lab's entry to `site/src/labs.json`.
- UI follows `docs/design-system.md`: Rune + `rune-kit`, Hex's pieces in
  `rune_ui/controls.rs` / `lists.rs`, theme tokens only, sentence case, one
  primary action per screen. Every control has an accessibility name. All
  copy through `tr` with every language filled.
- Recording reuses Annotate (`capture_host`): it already writes a bundle
  with `recording.mp4`, `audio.wav`, narration words with timings, key
  frames, `scrawl.json` (clicks; the window log of app and title changes,
  but only for full-screen captures), and `summary.md`. Hex turns off the
  engine's `agentContext` experiment (`hex_engine_config()`), so clicks
  carry coordinates only, not the AX role/label under them; it can be set
  per session. Annotate must be on for Watch this to work. Hex never sends
  key events to the capture engine, so a lesson records no keys.
- Annotate's `CaptureCommand::Clip` is the precedent: a capture whose
  written bundle goes to its own consumer instead of the clipboard and the
  router. A lesson works the same way (`CaptureCommand::Lesson(target)` or
  a lesson flag), so a lesson is never pasted or routed to an agent.
- Model access: Ollama on loopback with a multimodal (vision) model is the
  main path (owner's decision: the model must see the screen). Apple
  Intelligence (`apple_intelligence::generate_text`, text only, small
  context) stays available as a text-only choice. No other path. There is
  no Ollama client in the fork yet; it is new work over `net` (loopback
  only).
- Files under the data folder are owner-only (0600/0700), like captures.
- Source files stay under 800 non-test lines; folders keep their shape
  rules (`scripts/code-shape.py`).

## Approaches considered

1. **New Apprentice module inside the fork** (chosen). Rust code beside
   Annotate: lesson capture, compiler over the bundle using the existing
   model bridges, a skills store, a Skills pane behind a Labs switch.
   Reuses approvals, i18n, design system, settings, and agent install.
   Most work up front, but it fits the app and is what replay builds on.
2. Bring in the earlier prototype's `apprentice-core` crate. Faster to
   wire, but it is thin and ignores this codebase's rules (Labs gating,
   i18n, settings), so it would be rewritten anyway. Its data shapes (skill
   fields, the four autonomy levels) are borrowed; its code is not.
3. Let a coding agent compile skills from a routed capture. Almost no code,
   but sends the recording to whatever model the agent uses, often a cloud
   one, breaking local-first. Ruled out.

## Design

### Where it lives

- `src/apprentice/` (new): `lesson.rs` (start/stop and receiving written
  lesson bundles), `evidence.rs` (bundle → bounded text evidence),
  `compile.rs` (prompting, parsing, chunking), `model.rs` (the chosen local
  model), `skill.rs` (the skill record and `SKILL.md` rendering),
  `store.rs` (drafts and skills on disk), `install.rs` (agent folders).
- `src/rune_ui/skills.rs` (new pane) and its scenes.
- Labs feature `watch-and-learn`, group Annotate (or a new Apprentice group
  if `Feature::group` gains one; builder's call), audience Everyone, macOS.

### Recording a lesson

- **Watch this** button in the Skills pane, and a **Watch this…** menu-bar
  item while the lab is on. It starts an Annotate recording of the whole
  screen, marked as a lesson, with no picker, because only full-screen
  captures keep the window log. The button's note says "Records your whole
  screen and your voice". *(Builder's call, made in review.)*
- The person demonstrates and narrates. They stop it with the capsule's
  stop as usual; discard works as usual.
- A written lesson bundle is not copied to the clipboard and not routed.
  It appears in the Skills pane as **New lesson · ready to draft**.
- **Storage follows Annotate** (owner's decision): lessons are written to
  the normal captures folder like any capture, show in Annotate's library
  and `hex capture list`, and follow Annotate's retention (by default the
  video and audio go to the Trash after 7 days, the bundle after 30). The
  bundle's `capture.json` (or an Apprentice sidecar) marks it as a lesson
  so the Skills pane finds it. Because the bundle can expire, drafting
  copies the text evidence it used into the draft folder, so a skill never
  depends on its recording still existing. A lesson past its retention can
  no longer be drafted; the pane says so.
- **Names under clicks** (owner's decision): lessons turn on the capture
  engine's `agentContext` for that session only
  (`reset_for_session_at_with_agent_context`), so each click records the
  app, AX role and label under it. Other Annotate captures are unchanged.
  The pane lists "names of what you click" among what is read.

### Drafting (approval-gated)

- The pane shows what will be read and by which model, then one primary
  button: **Draft skill**. Pressing it is the approval for that one run.
  Nothing runs before.
- Evidence sent to the model (bounded): the narration transcript with
  timings, the window log (app and window title changes), each click's
  app, AX role and label, `summary.md`'s step outline, and, for a vision
  model, the lesson's key frames (Annotate already picks them at spoken
  cues and clicks), downscaled and capped in number per call. Apple
  Intelligence gets the text alone, and the pane says it can't see the
  screen.
  Window titles are included here, which Annotate's `summary.md` leaves
  out on purpose; this is a local model, and the pane lists "window titles"
  among what is read.
- Long lessons: when evidence exceeds the model's context
  (`context_tokens()` for Apple Intelligence), it is cut into segments at
  narration pauses or window changes, each segment drafted to step lines,
  then merged in one final pass. Bounded number of calls and a total
  deadline; a failure leaves the lesson ready to draft again with the
  reason shown.
- Model choice: a setting in the Skills pane listing **Ollama** models
  (URL loopback only, validated) and **Apple Intelligence (on this Mac,
  text only)**. The pane lists the models Ollama already has, marking which
  can see images (Ollama's `/api/show` capabilities), plus recommended
  vision models that aren't downloaded yet, each with its size and a
  **Download** button that asks Ollama to pull it (`/api/pull`, progress
  shown); the click is the approval. Nothing is chosen until the person
  picks one. If the chosen model is unavailable, drafting says so and
  stops; it never switches models by itself.
- Recommended vision models (checked in the Ollama library on 2026-11-22,
  for a 32 GB Apple Silicon Mac; a list in code, easy to change):
  - `gemma4:26b` (≈17 GB; images, audio, tools; 256K context) — the
    default suggestion: strong and fits with room for frames.
  - `qwen3.8:27b` (≈18 GB; images, thinking, tools) — the strongest that
    fits; slower, tighter on memory.
  - `gemma4:12b` (≈8 GB) and `qwen3-vl:8b` (≈6 GB) — for Macs with 16 GB
    or when speed matters more.
  Quality on real lessons is unproven; the plan includes trying the first
  two on one recorded lesson before settling the default suggestion.
- Output parsed into the skill record; unparsable output keeps the raw
  text visible and offers Try again.

### Why? questions

- The draft marks up to 5 steps as decisions, each with a question in
  plain words ("Why did you pick the March invoice?").
- The pane shows them one by one under the draft; the person can answer,
  skip, or edit the step. Answers go into the skill's Decisions section
  verbatim. Answering does not call the model again in this version.

### The skill

- Record (borrowed from the prototype): name, slug, description, when to
  use, inputs, steps (each with optional decision and why), verification,
  edge cases, autonomy level (`observe` only for now), version, created and
  updated times, source lesson ID.
- `SKILL.md` uses the agent skills format: YAML front matter (`name`,
  `description`), then sections When to use, Inputs, Steps, Decisions,
  Verification, Edge cases, Evidence (the lesson's ID and date, not media).
- On disk: `<support>/skills/<slug>/SKILL.md` and `skill.json`; drafts in
  `<support>/skills/drafts/<id>/`. Owner-only. Approving moves a draft to
  skills; an existing slug asks to replace or keep both.
- Editing: the pane edits structured fields; `skill.json` is the source of
  truth and `SKILL.md` is always regenerated from it (never hand-edited and
  parsed back). Approved skills can be edited again; saving an edit is its
  own approval and bumps the version. *(Builder's call, made in review.)*

### Installing into agents

- Generalise `agent_install` from the fixed `hex-annotate` skill to any
  skill folder name. Apprentice installs to
  `<agent folder>/skills/apprentice-<slug>/SKILL.md` for each agent the
  person ticks, marking files it wrote so it only ever removes or replaces
  its own; a symlinked or foreign file is shown and left alone.
- Remove uninstalls from every agent Apprentice wrote to.
- Installed copies are not updated silently: after an edit, each agent row
  shows "Older version installed" with an Update button; pressing it is the
  approval. The install step says plainly that an agent may send the skill's
  text to its own model, which can be a cloud one. *(Builder's call, made
  in review.)*

### Process in this fork

Hex's AGENTS.md process rules for Anomaly (Rune pin bump each round, PRs
to Anomaly's `dev`, `check-linux.yml` golden dispatch, the Hex website) do
not apply to this fork. Work happens on a branch of
`fathyshalaby/apprentice`, merged into its `dev` once the owner approves.
`site/src/labs.json` is still regenerated because `labs::web`'s test checks
it. Linux goldens can't be rendered on this Mac; window scene changes are
checked by rendering scenes locally and noted as stale for Linux. *(Builder's
call, made in review.)*

### Skills pane (sketch)

```
Skills                                     [Watch this]
─────────────────────────────────────────────────────
New lesson · 2:14 · today 10:42      ready to draft
  Reads: narration, clicks, window titles · Apple Intelligence
  [Draft skill]   Discard

Draft · "File the monthly invoice"         2 of 3 why? answered
  Steps … (editable)
  Why did you pick the March invoice?  [answer…]   Skip
  [Approve skill]   Edit   Discard

Skills
  File the monthly invoice      Claude Code, OpenCode   ›
```

### Visible control

- Every lesson, draft run, approval, install, and removal is an event in
  `live.ndjson` (names and counts only, never narration or step text, as
  the log already guarantees), and appears in the pane's history of that
  skill.

### Verification

- Unit tests: evidence building from a fixture bundle (bounded, window
  titles included, no secure fields), segmenting, prompt → parse with a
  fake model, store round-trip and permissions, install/remove that never
  touches foreign or symlinked files, Labs gating (no lesson starts with
  the lab off), `hex settings` key, i18n completeness, accessibility
  names.
- Scenes for the Skills pane (empty, new lesson, drafting, draft with
  why?, skills list) in light and dark; Linux goldens regenerated in CI.
- A manual run on this Mac: record a short lesson, draft with Apple
  Intelligence, answer a why?, approve, install into one agent.

## Left to the builder

Module boundaries within `src/apprentice/`, prompt wording, the exact
segmenting heuristics and call bounds, Labs group naming, how the lesson
flag threads through `capture_host`, and the pane's exact layout within the
design system.
