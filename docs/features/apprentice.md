# Watch and learn (macOS, Labs)

Watch and learn lets a person show Apprentice a task once and get a skill
file a coding agent can follow. They record a narrated demonstration (a
lesson), a local model they chose drafts a `SKILL.md` from it, they answer up
to five "why?" questions, approve the skill, and install it into their coding
agents. Design: [the spec](../superpowers/specs/2026-11-22-watch-and-learn-design.md);
plan: [the implementation plan](../superpowers/plans/2026-11-22-watch-and-learn.md).

Everything below is built. Behavior IDs are stable; each tree names its source
and the tests that check it. A test listed here is a locator, not an observed
run; what was observed is under [Observed on this Mac](#observed-on-this-mac).

```ts
Turn it on                               // apprentice.gate
├── Labs › Watch and learn (labs.watch_and_learn, off by default, macOS only,
│   group Annotate, added 20261122) -> labs::watch_and_learn()
├── Needs Annotate: the DisableAnnotate lock holds it off and the card reads
│   managed (policy::Lock::Annotate); the saved switch is never rewritten
├── Read where work starts, not only in the pane: a lesson start
│   (capture_host/lesson.rs refusal -> "feature-off"), a draft (run::draft),
│   an install (rune_ui/skills/agents.rs put), and the menu bar's Watch This…
├── Compiled only into macOS builds with captures: src/apprentice,
│   rune_ui/skills, Destination::Skills, and Pane::Skills are
│   cfg(all(target_os = "macos", hex_capture)); StatusItemAction::OpenSkills
│   is cfg(hex_capture) in macOS-only code
└── Not on Hex's website: labs::web leaves it out like the other
    Annotate-dependent labs, so site/src/labs.json is unchanged
// labs::tests::watch_and_learn_is_its_own_switch_and_macos_only,
// capture_host::tests::a_lesson_refuses_to_start_with_the_lab_off,
// capture_host::lesson::tests::only_a_lesson_is_gated,
// apprentice::run::tests::drafting_refuses_with_the_lab_off,
// rune_ui::skills::tests::the_skills_pane_shows_only_with_its_lab,
// macos_rune::status_item::tests::watch_this_is_hidden_with_the_lab_off

Record a lesson                          // apprentice.lesson
├── Watch this (the Skills pane's header, note "Records your whole screen and
│   your voice"; or the menu bar's Watch This… while the lab and Annotate are
│   on and nothing records) -> lessons::start() ->
│   CaptureCommand::Lesson(Display at the pointer)
├── capture_host begins it with Purpose::Lesson: the whole display, no picker,
│   Annotate's microphone timeline and movie, and Accessibility names under
│   clicks for this capture only (Start::agent_context = Some(true))
├── Stopped or discarded from the usual capsule, which keeps its stop button
│   and offers no target chips (RecordingCapture.lesson)
├── Written -> marked with .apprentice-lesson.json ({"version":1,
│   "startedAtMs":…}, 0600) before it settles -> lessons::written opens the
│   Skills pane; never copied, never given Paste Last provenance, never routed
├── Refused for a permission -> AskedBy::Lesson; the helper's grant starts a
│   lesson again (Action::Lesson -> CaptureCommand::Lesson), never an
│   Annotate capture
└── Storage follows Annotate: the captures folder, `hex capture list`, and
    Annotate's retention; a lesson whose bundle expired can only be discarded
// capture_host::tests::a_lesson_starts_the_whole_screen_with_names_under_clicks,
// an_ordinary_capture_keeps_names_off, a_written_lesson_is_marked_and_never_copied_or_routed,
// capture_host::permissions::tests::a_refused_capture_asks_for_what_it_was,
// rune_ui::capture_capsule::flow::tests::a_granted_lesson_permission_starts_a_lesson,
// rune_ui::capture_capsule::feed::tests::a_resumed_lesson_sends_a_lesson_command,
// a_lesson_shows_no_target_chips, apprentice::lessons::tests::*,
// rune_ui::skills::tests::watch_this_sends_a_lesson_command

Read a lesson                            // apprentice.evidence
├── evidence::build(lesson, limits): time-ordered lines (said:, window: app —
│   title, clicked role "label" in app), cut into segments at narration gaps
│   and app changes, each under limits.max_segment_chars
├── Secure-field and missing labels are left out; video and audio never
│   leave the bundle
├── Key frames only for a model that sees images: downscaled to 1024 px wide,
│   at most 4 per call and 12 per draft
└── The pane reads each lesson's text once (no frames decoded) to flag a thin
    one: "Little to learn from: no narration, no click names"
// apprentice::evidence::tests::*

Choose a model                           // apprentice.model
├── Skills pane › Model, or hex settings set apprentice.model
│   apple-intelligence | ollama:<name> | none -> ApprenticeSettings::choice()
├── The picker lists Ollama's models on this computer (each checked with
│   /api/show for vision), the recommended ones not downloaded with Download
│   (/api/pull; the click is the approval, progress shown, Cancel), and
│   Apple Intelligence (text only)
├── Ollama's cloud models are never offered: a -cloud or :cloud tag, or a
│   remote_host or remote_model in /api/tags or /api/show, leaves one out
├── Recommended (models::RECOMMENDED): gemma4:26b, qwen3.8:27b, gemma4:12b,
│   qwen3-vl:8b; none is a cloud model
├── Unchosen by default (omitted from settings.json); Apprentice never picks
│   or switches a model by itself
└── Ollama's address: apprentice.ollama_url, default http://127.0.0.1:11434;
    only http on localhost, 127.0.0.0/8, or [::1] is accepted
    (app_settings::is_loopback_http_url), and net::loopback_agent refuses a
    name that resolves elsewhere
// apprentice::ollama::tests::installed_marks_vision_models,
// installed_leaves_out_cloud_models, cloud_names_end_in_cloud,
// a_non_loopback_url_is_refused, pull_reports_progress_lines,
// a_cancelled_pull_reads_cancelled,
// apprentice::models::tests::recommended_models_are_the_four_from_the_spec,
// app_settings::apprentice::tests::*,
// rune_ui::skills::tests::a_download_is_its_own_click, a_remote_ollama_address_is_refused

Draft a skill                            // apprentice.draft
├── Draft skill on a lesson card is the approval for one run; with no model
│   chosen the button reads Choose a model first and opens the picker
├── The card says what is read and by which model: "screenshots" only for an
│   Ollama model the picker knows sees images (the shown pane lists Ollama's
│   models once); "text only" for Apple Intelligence and text-only models
├── run::draft -> thread apprentice-draft -> job, which checks the model first
│   ├── Ollama with a cloud tag, or /api/show naming a remote host or model ->
│   │   Failed "This model runs in the cloud, so Apprentice won't use it";
│   │   nothing is read or sent
│   ├── Ollama with vision -> Limits::for_vision (key frames)
│   ├── Ollama without vision -> Limits::for_text(16384), no frames
│   └── Apple Intelligence -> Limits::for_text(context_tokens()), text only
├── Every Ollama chat asks for num_ctx 16384, the answer's JSON schema, and
│   temperature 0.2; Apple Intelligence gets the schema in its instructions
├── At most 8 part calls (neighbours joined), then one merge; 180 s per call,
│   10 min in all; Cancel stops before the next call
├── Any model error ends the draft with its reason and Try again; nothing
│   retries or switches model; an unreadable answer shows the raw text
└── Done -> the draft is saved with the lesson's text (evidence.txt) and
    opens, unless another draft or skill is open: then it waits under Drafts
    waiting
// apprentice::run::tests::*, apprentice::draft::tests::*,
// apprentice::ollama::tests::chat_sends_images_and_the_schema,
// details_read_vision_and_the_cloud,
// rune_ui::skills::tests::draft_skill_is_the_only_way_a_model_runs,
// without_a_model_the_lesson_asks_for_one_first, a_text_only_model_never_reads_screenshots,
// a_finished_draft_is_saved_with_its_text_and_opens,
// a_failed_draft_says_why_and_offers_try_again,
// an_unreadable_answer_shows_the_raw_text_and_offers_try_again,
// a_cancelled_draft_returns_quietly_to_its_lesson,
// a_draft_finishing_under_an_open_editor_leaves_it_alone

Answer why?                              // apprentice.why
├── The draft marks at most 5 steps as decisions, each with a plain question
├── The draft view shows one at a time: Answer keeps the text verbatim as the
│   step's why (saved into the draft), Skip passes it for now
└── Answers go into SKILL.md's Decisions; answering never calls the model
// apprentice::draft::tests::more_than_five_questions_keep_the_first_five,
// rune_ui::skills::tests::answering_a_why_question_keeps_the_answer

Review and approve                       // apprentice.approve
├── The draft view edits the name, description, when to use, inputs (one per
│   line), each step's title and details, verification, and edge cases
├── Approve skill moves the draft to <support>/skills/<slug>/ (skill.json is
│   the source of truth; SKILL.md is always regenerated from it); a taken
│   slug asks Replace or Keep both; a skill named Drafts is stored as
│   drafts-skill, never in the drafts folder
├── SKILL.md: front matter name and an always double-quoted description, then
│   When to use, Inputs, Steps (multi-line details stay inside their step),
│   Decisions, Verification, Edge cases, Evidence (lesson ID and date)
├── Edit on a saved skill; Save changes is its own approval and bumps the
│   version; Review or Edit over unsaved typing asks first
├── Discard a draft: Discard, then Discard draft, which shows after Cancel in
│   Discard's place, so a double click never discards; the draft folder goes
│   to the Trash (rune_ui/skills/approval.rs, Store::discard_draft). A
│   lesson's Discard works the same way with Move to Trash
├── Owner-only: folders 0700, files 0600
└── When the skills folder can't be opened, approving, saving, discarding, a
    why? answer, Delete skill, and a finished draft say "Apprentice can't
    open its skills folder" and change nothing (SkillsPane::ready_store)
// apprentice::store::tests::*, apprentice::skill::tests::*,
// rune_ui::skills::tests::approve_saves_the_draft,
// approving_over_an_existing_skill_asks_replace_or_keep_both,
// discarding_a_draft_takes_a_second_click, a_double_click_never_confirms_a_discard,
// inputs_are_edited_in_the_draft_view, review_over_unsaved_typing_asks_first,
// without_its_folder_nothing_changes_on_screen_only

Install into agents                      // apprentice.install
├── A skill's card opens to the coding agents on this Mac (Claude Code, Codex,
│   OpenCode, Pi): Install, Update ("Older version installed"), or Remove,
│   each its own click; the card says an agent may send the skill's text to
│   its own model, which can be a cloud model
├── Written to <agent>/skills/apprentice-<slug>/SKILL.md with a
│   .apprentice-skill mark; Codex reads ~/.agents, and Pi does when a copy
│   is there
├── A SKILL.md without the mark (Not Apprentice's) or behind a symlink
│   (Linked elsewhere) is shown and never replaced or removed
└── Delete skill… then Delete skill (after Cancel in its place) removes
    Apprentice's own copies from every agent, then the skill
// apprentice::install::tests::*, agent_install::tests::*

Events                                   // apprentice.events
├── live.ndjson gets {"kind":"apprentice","action":…,"id":…,"count"?}:
│   lesson_written, draft_started, draft_finished (count = steps),
│   draft_failed, skill_approved, skill_edited (count = version),
│   skill_installed, skill_removed, model_downloaded
└── Only IDs, slugs, model names, and counts: never narration, step text,
    answers, paths, or addresses
// apprentice::events::tests::emitting_writes_only_the_action_id_and_count,
// desktop::events::tests::apprentice_events_round_trip
```

## Entry points

- Settings › Labs › Annotate group: the Watch and learn card
  (`rune_ui/labs.rs`, row key `lab-watch-and-learn`; macOS with Annotate).
- The Skills pane in the sidebar while the lab is on (`rune_ui/skills.rs`,
  `rune_ui/skills/`); scenes `mac-skills-empty`, `mac-skills-lesson`,
  `mac-skills-draft`, `mac-skills-models`, `mac-skills-list`.
- The menu bar's Watch This… (`macos_rune/status_item.rs`).
- `hex labs on|off watch-and-learn`; `hex settings set labs.watch_and_learn`,
  `apprentice.model`, `apprentice.ollama_url`.

## Platforms

macOS only; Linux and Windows compile none of it. The spec asks for a
`platform::DIFFERENCES` "pending" entry, but that table records only
`platform::Capabilities` fields, and its parity test rejects an entry that
names no capability. Watch and learn is not a capability: it is gated at
compile time (`cfg(all(target_os = "macos", hex_capture))`) and by its Labs
feature's OS list (`labs::Feature::oses`), so it has no entry there.

## Observed on this Mac

```ts
2026-11-22, automated checks only
├─ cargo test --release --bin hex      passing apart from the known baseline failures
├─ cargo test (crates/capture)         passing
├─ scripts/code-shape.py --check       passing
├─ Skills scenes rendered with --snap  looked at, light appearance
└─ launch                              an earlier run stopped at macOS's Desktop
                                       folder prompt, before any window
```

### Not verified

- Recording a real lesson in a running app, and the Skills pane and menu item
  appearing and vanishing with the switch.
- Drafting with a real model: no model was downloaded, no Ollama was
  contacted, and Apple Intelligence was not run.
- Installing a skill into a real agent and reading the installed `SKILL.md`.
- The model-default trial (`gemma4:26b` against `qwen3.8:27b`), so
  `RECOMMENDED` is unchanged.
- Linux and Windows builds: no Linux or Windows target is installed here, so
  the cfg gates were checked by reading only.
- Linux goldens: the Skills scenes are macOS-only and the golden tests run
  only on Linux, so no golden covers this pane.
