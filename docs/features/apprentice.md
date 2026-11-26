# Watch and learn (macOS, Labs)

Watch and learn lets a person show Apprentice a task once, then review and run
the steps on this Mac. They record a narrated demonstration (a lesson), a
local model they chose drafts a skill from it, they answer up to five "why?"
questions, and approve it. Approval saves a bounded run plan in `skill.json`;
Apprentice performs it only after the person reads the plan and confirms.
Design: [the spec](../superpowers/specs/2026-11-22-watch-and-learn-design.md);
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
│   a run (rune_ui/skills/runs.rs -> apprentice::execution), and the menu
│   bar's Watch This…
├── One pane predicate (rune_ui::skills::allowed) hides the sidebar row,
│   refuses the window destination, and gates every model/URL/draft/skill
│   write; a locked pane never leaves an enabled control that silently no-ops
├── hex window show skills answers unsupported while the lab is off, instead
│   of claiming it opened a pane it never selected
├── hex status --json reports watch_and_learn and skills from that same gate,
│   plus the saved apprentice_model (null until a person chooses one)
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
// the_skills_destination_does_nothing_while_the_lab_is_off,
// a_locked_pane_refuses_model_and_address_writes,
// a_locked_pane_cannot_approve_a_draft, a_locked_pane_cannot_arm_a_skill_delete,
// macos_rune::status_item::tests::watch_this_is_hidden_with_the_lab_off,
// control::cli::tests::status_reports_watch_and_learn_and_reads_older_replies_without_it

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
│   title, clicked role "label" in app, dragged named thing down and right let
│   go over app — window, drew: circled "label", scrolled down, pressed ⌘T ×3),
│   cut into segments at narration gaps and app changes, each under
│   limits.max_segment_chars
├── Word timings are best; narration without them reads transcript segments.
│   Unnamed drags under 0.15 of the screen say nothing; quick repeats of one
│   key or scroll fold into one line; Secure-field and missing labels are left
│   out. Video and audio never leave the bundle
├── Key frames only for a model that sees images: downscaled to 1024 px wide,
│   at most 4 per call and 12 per draft, named with their times in the prompt
└── The pane reads each lesson's text once (no frames decoded) to flag a thin
    one: "Little to learn from: no narration, no click names"; a named drag or
    drawing counts as a name
// apprentice::evidence::tests::*

Choose a model                           // apprentice.model
├── Skills pane › Model, or hex settings set apprentice.model
│   apple-intelligence | ollama:<name> | none -> ApprenticeSettings::choice()
├── The picker lists None (Use clears the saved choice), Ollama's models on
│   this computer (each checked with /api/show for vision), the recommended
│   ones not downloaded with Download (/api/pull; the click is the approval,
│   progress shown, Cancel), and Apple Intelligence (text only)
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
// rune_ui::skills::tests::a_download_is_its_own_click, a_remote_ollama_address_is_refused,
// the_picker_saves_each_explicit_model_choice, none_clears_the_saved_choice_without_running_a_model

Draft a skill                            // apprentice.draft
├── Draft skill on a lesson card is the approval for one run; with no model
│   chosen the button reads Choose a model first and opens the picker; with
│   no choice the card says None chosen
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
// without_a_model_the_lesson_asks_for_one_first, a_lesson_without_a_model_says_none_is_chosen,
// a_text_only_model_never_reads_screenshots,
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

Run a learned skill                      // apprentice.run-plan
├── Approval derives a run plan from structured interaction events only: app
│   changes, clicks, keys, scrolls, and drags. Narration, window titles, page
│   addresses, and SKILL.md never become steps (apprentice/execution.rs)
├── The saved skill's card shows every planned step — "Open Mail", "Click",
│   "Press ⌘S" — from `skill.json`; the saved `SKILL.md` is never read back
├── Bounded: at most 40 steps, identical keys within 3 s folded to one, unnamed
│   drags under 0.15 of the screen skipped, and one step every 180 ms
├── Run skill is two clicks: the first shows the count and asks "Run these 3
│   steps?", the second starts. Dry run checks the same bounds without acting,
│   Not now leaves it alone, and Stop refuses the next step
├── Execution is macOS only: clicks and keys use the capture engine's and
│   keyboard's synthetic-input seams, scrolls and drags use CoreGraphics, and
│   an app is opened with `open -a` only when it is already running. Elsewhere
│   the card answers "Running learned skills is available on macOS only"
├── Progress reads "Step n of {of}"; typed outcomes are Dry, Done, Failed, and
│   Cancelled. A failed or empty plan is refused before any step runs
└── Run events carry skill_run_started, skill_run_step, skill_run_finished,
    skill_run_failed, and skill_run_cancelled with a slug/id and count only:
    never step text, coordinates, app names, narration, or paths
// apprentice::execution::tests::*, rune_ui::skills::tests::a_saved_skill_shows_its_recorded_plan,
// a_run_needs_a_second_click, a_dry_run_checks_without_acting,
// stopping_a_run_ends_it_before_the_next_step, a_failed_plan_never_runs,
// and_progress_and_typed_outcomes_are_shown,
// apprentice::events::tests::emitting_writes_only_the_action_id_and_count,
// desktop::events::tests::run_actions_name_only_their_kind

Events                                   // apprentice.events
├── live.ndjson gets {"kind":"apprentice","action":…,"id":…,"count"?}:
│   lesson_written, draft_started, draft_finished (count = steps),
│   draft_failed, skill_approved, skill_edited (count = version),
│   skill_run_started, skill_run_step, skill_run_finished,
│   skill_run_failed, skill_run_cancelled, model_downloaded
└── Only IDs, slugs, model names, and counts: never narration, step text,
    answers, paths, or addresses
// apprentice::events::tests::emitting_writes_only_the_action_id_and_count,
// desktop::events::tests::apprentice_events_round_trip

Notice patterns                          // apprentice.notice
├── Turn it on
│   ├── Labs › Notice patterns (labs.notice_patterns, off by default, macOS,
│   │   group Annotate, added 20261122; `hex settings set labs.notice_patterns`,
│   │   `hex labs on|off notice-patterns`) -> labs::notice_patterns()
│   ├── Needs Watch and learn on too (labs::gates::notice_patterns_gate:
│   │   its own switch and Watch and learn's, no lock); the DisableAnnotate
│   │   lock holds both off (policy.rs), and the card reads managed
│   ├── Adds the Noticing section to the Skills pane (rune_ui/skills/
│   │   noticing.rs); no folder is watched until the person adds one
│   └── Not on Hex's website, like Watch and learn (labs::web leaves it out)
├── Watch
│   ├── Folders: `apprentice.watched_folders`, absolute paths only, each one
│   │   the person added (Add folder… in the pane, or `hex settings set`,
│   │   which refuses a relative path; settings.json is not validated, so
│   │   the watcher skips a relative path and the pane shows it unreadable)
│   ├── The `apprentice-notice` thread starts at launch (macos_rune) and
│   │   idles while the gate is closed: one gate check per 5 s, no folder,
│   │   front-app, or settings read, and a closed gate forgets its
│   │   snapshots, so what happens while it is closed is never noticed
│   ├── Open gate and a folder: every 5 s (watch::POLL) each folder is read
│   │   (snapshot::read): 2 levels deep (DEPTH), at most 5,000 entries
│   │   (MAX_ENTRIES, cut off in name order; nothing past the cut reads as
│   │   added or removed), never following links
│   ├── Skipped: hidden entries, package folders (.app .scrawl .bundle
│   │   .photoslibrary .pkg .framework .xcodeproj), browser partial downloads
│   │   (*.crdownload .part .partial .download, so the finished file is one
│   │   plain Added), and a hard link's later names
│   ├── Files are matched by identity (device, inode), so a rename or move is
│   │   one event: Added, Removed, Renamed, Moved (a move that renames is
│   │   Moved); a renamed or moved folder is one change; of folders only a
│   │   new one is kept; a file whose name, size, and time all changed under
│   │   one identity is a Removed and an Added
│   ├── Snapshots are committed only after a log opens, remaps, and loads
│   │   routines successfully; an empty folder list creates no noticed/
│   │   files and starts no watcher thread
│   ├── A folder that can't be read (missing, or macOS refuses it) is
│       reported unreadable and keeps its last snapshot; settings that
│       can't be read, or whose apprentice.watched_folders had to be
│       repaired (a wrong type), keep the last good folder list
│       (AppSettings::read_trusted_from; never read as "no folders", which
│       would drop the log); with none yet the tick waits
├── Kept (owner-only, <Application Support>/apprentice/noticed/, or
│   apprentice-test in a macOS test build, or under
│   HEX_APPLICATION_SUPPORT_DIR when set; without a data folder nothing is
│   noticed)
│   ├── events.ndjson: one line per event {t, root, kind, folder, ext, from,
│   │   to, name, app?}, written one line at a time under the lock; `folder`
│   │   is a boolean (a folder event); `from` and `to` are folder paths
│   │   relative to the watched folder, as the person named them; `name` is
│   │   a shape
│   │   (name_shape: letters become a, digits #, so `2026-11-03 Telekom.pdf`
│   │   reads `#-#-# a.pdf`); `ext` only from an allow-list of common
│   │   extensions (snapshot::KNOWN_EXTENSIONS, lowercased; any other
│   │   extension is shaped like the rest of the name, so a surname after a
│   │   dot never leaks); `app` is the front app's name when the change was
│   │   seen, never its window
│   ├── Never file contents, never file names; no page address or title
│   ├── 14 days (log::KEEP_MS) and the newest 20,000 lines (MAX_LINES),
│   │   pruned when the gate opens and hourly; a rewrite is owner-only and
│   │   only swapped in after the new log is fully written, and a failure
│   │   keeps the old one for the next tick
│   ├── roots.json: the folder list the events' `root` indexes refer to;
│   │   a changed list re-indexes the log, and a removed folder's events are
│   │   dropped at the next tick
│   ├── routines.json: each routine's steps, sentence, days, last seen,
│   │   trigger, and state (New, Offered{last_ms}, Shown, Ignored)
│   └── Torn or invalid log lines are skipped; no log line, routine, or
│       folder is written to live.ndjson (only ids, below)
├── A routine (routine.rs; pure)
│   ├── An episode is one watched folder's events with no gap over 10 minutes
│   │   (EPISODE_GAP_MS); one still within 10 minutes of now may be going and
│   │   waits
│   ├── Its signature is the ordered, de-duplicated steps: kind, folder?,
│   │   ext, the destination folder's shape (numbers abstracted, words kept:
│   │   `Invoices 2026-11` is `Invoices #-#`), and for a rename or new folder
│   │   the name's shape; a new file is not a step (downloads arrive on
│   │   their own), a new folder is; an add and its later move are paired by
│   │   file identity, and a folder add uses the folder shape, not the file
│   ├── A routine is a signature repeated in at least 3 distinct local days
│   │   (MIN_DAYS) within 14 days (WINDOW_MS) in one folder: three episodes
│   │   on one day, or days older than 14, don't count
│   ├── It must contain a filing step: a move into a non-root folder, a
│   │   rename, or a new folder; deleting alone, or moving to the root, never
│   │   is one
│   ├── Its id is the first 8 bytes of SHA-256(folder path, steps) in hex,
│   │   stable across runs and different per folder
│   ├── Its trigger is the extension of the new file that arrived in the root
│   │   itself (not a subfolder, as for offers) within 2 hours before
│   │   (TRIGGER_WINDOW_MS, counting the episode's own first event) most
│   │   episodes, when at least half have one; ties go to the alphabetically
│   │   first
│   ├── Looked for when the gate opens and once an episode has been quiet for
│   │   10 minutes, over the whole log; a known routine keeps its state and
│   │   gets new days, last seen, trigger, and sentence
│   └── Its sentence is one line in the interface language, such as "Moves
│       PDF files from Downloads into folders like Invoices 2026-11, then
│       renames them like #-#-# a.pdf" (the latest real folder name is the
│       example), "Renames …", or "Makes folders in …"; the kind is
│       translated instead of adding an English plural
├── The offer (offer::pick, watch.rs; routine_offer.rs)
│   ├── Only a new file with an allow-listed extension in the watched folder
│   │   itself (not a subfolder) can start one, and only for a routine of
│   │   that folder whose trigger is that extension; the one seen on the most
│   │   days wins
│   ├── State New, or Offered at least 24 h ago (REOFFER_MS); never Shown or
│   │   Ignored; at most 3 offers a local day across routines (MAX_PER_DAY)
│   ├── Never while dictating, a meeting records, a capture or lesson
│   │   records, or Annotate is off (offer::Busy); the offer is not queued
│   ├── One capsule at the top of the screen: "New PDF in Downloads" and "You
│   │   usually file these (3 days). Show me how?", with Show me, Not now, and
│   │   Stop asking; it takes no focus, takes the pointer only where it
│   │   draws, is left out of screen recordings, and sits clear of the
│   │   meeting capsule's corner as it changes; a new offer replaces the one
│   │   shown
│   ├── Closes by itself after 30 s (TIMEOUT), doing nothing and staying
│   │   offerable again after 24 h; Not now only closes; closing the gate
│   │   closes it at once
│   ├── Show me (watch::accept, off the main thread) refuses at once while
│   │   Annotate is off or a capture records, in the pane's words; otherwise
│   │   it asks the listener for a lesson (lessons::start_confirmed ->
│   │   capture_host::ask_listener) and waits at most 2 s (LESSON_ANSWER)
│   │   for its answer. Only an answer that the lesson started recording
│   │   marks the routine Shown (no more offers): a capture reply with
│   │   changed: false means a capture or replay had already started between
│   │   the pane's check and the listener's command, so it counts as a refused
│   │   start, not a lesson. A refusal, that busy answer, a listener that
│   │   doesn't answer in time, or a refused start is logged and the routine
│   │   stays offerable (a lesson the listener starts after the 2 s still
│   │   records, and the routine may be offered again). The lesson is the
│   │   usual Watch and learn lesson (whole display, narrated), never started
│   │   by the watcher alone
│   ├── Stop asking marks it Ignored and drops its episodes' events
│   └── Show me and Stop asking (capsule or pane) tell an open pane to read
│       again (watch::note_changed)
├── The pane (Skills › Noticing, while the lab is on)
│   ├── Watched folders: each with Remove, and Add folder… (the folder
│   │   dialog from the home folder); it is ignored once the pane or lab
│   │   closes. A relative folder reads "Use a full path" without Open
│   │   Settings; one that can't be read says "Can't read this folder" with
│   │   Open Settings (Files and Folders in System Settings › Privacy &
│   │   Security)
│   ├── Routines: each routine's sentence and "(3 days)", with Show me
│   │   (the same refusals as Watch this: "Turn on Annotate to record a
│   │   lesson", "Recording… stop it from the capsule"; a start failure
│   │   shows in the pane's error line) and Forget. A confirmed lesson
│   │   marks the pane recording at once, before the capture appears; a
│   │   refusal leaves it off and shows the pane's error
│   ├── "Apprentice keeps file kinds, folder names and times for 14 days, on
│   │   this Mac." and Forget everything…, which asks again with Cancel in
│   │   its place, then empties the event log and the routines; folders stay
│   │   watched
│   ├── Reads what was noticed off the UI thread when the pane opens and each
│   │   time the watcher reports a change; Remove drops the folder's
│   │   routines from the list at once and its events at the next tick
│   └── Scenes mac-skills-noticing-empty, mac-skills-noticing
├── Recover
│   ├── Settings or routines.json that can't be read stop the tick and are
│   │   logged once; nothing saves over them, and the log is never dropped
│   │   for it; a routines.json or roots.json that is not valid JSON is set
│   │   aside as <name>.unreadable-<sha256 prefix> and state starts empty
│   │   (an unparsable roots.json also empties the log, whose indexes can't
│   │   be trusted)
│   ├── No Files and Folders permission: the folder is unreadable and keeps
│   │   its last snapshot; granting it in System Settings is picked up on the
│   │   next 5 s read
│   └── Ignore, Forget, and Forget everything are the person's only controls
│       over what is kept; turning the lab off stops the watcher and the
│       offers but leaves what was kept (it ages out in 14 days or goes with
│       Forget everything)
└── Events: routine_noticed, routine_offered, routine_accepted,
    routine_ignored, each with the routine's hash as id; no count, folder, or
    name (desktop/events.rs)
// apprentice::notice::snapshot::tests::*, log::tests::*, routine::tests::*,
// state::tests::*, offer::tests::*,
// watch::tests::a_routine_replayed_on_three_days_then_a_new_pdf_offers,
// an_unreadable_root_is_reported_and_skipped, the_thread_stops_when_the_gate_closes,
// removing_a_folder_remaps_the_log, ignoring_forgets_the_routine_and_its_events,
// show_me_marks_the_routine_shown_only_once_the_lesson_starts,
// apprentice::lessons::tests::only_the_listeners_yes_counts_as_started,
// show_me_refuses_as_the_pane_does_while_annotate_is_off_or_a_capture_records,
// answers_tell_an_open_pane_to_read_again,
// broken_settings_keep_the_folders_and_the_log, only_a_file_in_the_root_itself_offers,
// an_unparsable_folder_list_is_set_aside_and_the_log_dropped,
// a_state_file_that_cant_be_read_skips_the_tick,
// app_settings::persistence::tests::a_repaired_watched_folder_list_is_not_trusted,
// rune_ui::skills::routine_offer::tests::*,
// rune_ui::skills::tests::noticing_is_hidden_while_the_lab_is_off,
// noticing_lists_folders_and_routines, forget_everything_asks_twice,
// show_me_and_forget_answer_their_routine, adding_a_folder_saves_the_setting,
// a_folder_answer_after_the_lab_closes_changes_nothing,
// a_relative_folder_asks_for_a_full_path,
// removing_a_folder_saves_the_setting, show_me_waits_like_watch_this,
// a_lesson_show_me_could_not_start_is_shown,
// rune_ui::skills::noticing::tests::a_confirmed_lesson_marks_recording_without_waiting_for_the_capture,
// a_refused_lesson_leaves_recording_off_and_says_why,
// control::settings::tests::watched_folders_key_takes_a_list_of_absolute_paths,
// desktop::events::tests::routine_actions_name_only_the_routine,
// labs::tests (feature registry)
```

## Entry points

- Settings › Labs › Annotate group: the Watch and learn card
  (`rune_ui/labs.rs`, row key `lab-watch-and-learn`; macOS with Annotate).
- The Skills pane in the sidebar while the lab is on (`rune_ui/skills.rs`,
  `rune_ui/skills/`); scenes `mac-skills-empty`, `mac-skills-lesson`,
  `mac-skills-draft`, `mac-skills-models`, `mac-skills-list`.
- The menu bar's Watch This… (`macos_rune/status_item.rs`), and
  `hex window show skills` (the running app's `window-show` control).
- `hex status --json` (`features.watch_and_learn`, `features.skills`,
  `features.apprentice_model`).
- Notice patterns: Settings › Labs › Annotate group, its card (row key
  `lab-notice-patterns`, needs Watch and learn); the Skills pane's Noticing
  section (`rune_ui/skills/noticing.rs`, scenes `mac-skills-noticing-empty`,
  `mac-skills-noticing`); the offer capsule (`rune_ui/skills/routine_offer.rs`,
  no scene); the watcher (`apprentice/notice/`, started in `macos_rune.rs`).
- `hex labs on|off watch-and-learn`; `hex settings set labs.watch_and_learn`,
  `apprentice.model`, `apprentice.ollama_url`; `hex labs on|off notice-patterns`,
  `hex settings set labs.notice_patterns`, `apprentice.watched_folders`.

## Platforms

macOS only; Linux and Windows compile none of it. The spec asks for a
`platform::DIFFERENCES` "pending" entry, but that table records only
`platform::Capabilities` fields, and its parity test rejects an entry that
names no capability. Watch and learn is not a capability: it is gated at
compile time (`cfg(all(target_os = "macos", hex_capture))`) and by its Labs
feature's OS list (`labs::Feature::oses`), so it has no entry there.

## Observed on this Mac

```ts
2026-11-22, automated checks
├─ cargo test --bin hex                passing apart from the known baseline failures
├─ cargo test (crates/capture)         passing
├─ scripts/code-shape.py --check       passing
└─ Skills scenes rendered with --snap  looked at, light appearance

2026-11-22, the answered-reply follow-up
├─ cargo test --bin hex apprentice::   139 passed, including the real
│                                      ScrawlCapture reply-shape test
├─ cargo clippy --locked --bin hex     clean with -D warnings (after the
│                                      store.rs sort_by_key lint fix)
├─ bun test (site)                     13 passed
└─ scripts/check                       blocked before tests by 55 files with
                                       rustfmt diffs left by the rename commit;
                                       the tests above were run directly

2026-11-22, the installed app (/Applications, German interface), driven
through its accessibility tree, Apple M6, 32 GB, Ollama 0.40.17
├─ lesson   a hand-built bundle in the captures folder: five real
│           screenshots of filing invoices in Finder, scripted narration
│           and click names (no microphone or screen permission was granted)
│  ├─ a keyframe reason Annotate never writes → "Diese Lektion ist
│  │  abgelaufen" (the manifest doesn't load); fixed in the bundle
│  └─ listed as "Neue Lektion · 1:02", reads narration, clicks, titles,
│     screenshots; Draft is "Wähle zuerst ein Modell" until a model is set
├─ Apple Intelligence   draft in 6 s, 5 questions, approved, saved
│  owner-only; text only, so one step invented a drag
├─ gemma4:12b (vision)  the reads line adds screenshots; the draft failed
│  at the 180 s call limit: Ollama thinks by default and generated ~9,500
│  tokens without finishing (a replay ran 15 min)
└─ the same request replayed against Ollama
   ├─ thinking on    gemma4:26b 101 s (17,534 characters of thinking)
   ├─ thinking off   gemma4:12b 22 s
   └─ optional schema fields were skipped by every model: inputs, checks,
      edge cases, and questions folded into whenToUse or a step's detail,
      so no why? questions reached the person

fixed: chat sends "think": false (a model that can't think accepts it;
"think": true is refused), and the schema requires every field, with an
empty question for steps that aren't decisions. Replayed:
├─ gemma4:26b     15 s, 5 steps, 2 questions, no edge cases
├─ gemma4:12b     16 s, 4 steps, 1 question, no edge cases
└─ qwen3.8:27b    51 s, 6 steps, 3 questions, both edge cases the person
                  named (non-invoices stay, the invoices' month not today's)

the fixed build, installed, same lesson, gemma4:12b with screenshots
├─ draft in 19 s: 5 steps, inputs, checks, one edge case, 1 question
├─ the question answered in the pane → "1 von 1 Warum-Fragen beantwortet"
└─ approved: SKILL.md (owner-only) carries the answer under Decisions

2026-11-26, installed 3.4.0 after the permission grants
├─ hex status --json       permissions.ready true, microphone and Accessibility
│                          ready, transcription_model true, capture.state idle
├─ hex settings get        apprentice.model ollama:gemma4:12b; URL
│                          http://127.0.0.1:11434
├─ Ollama /api/tags        qwen3.8:27b, gemma4:26b, gemma4:12b, each vision
└─ mac-skills-models       a headless accessibility tree, not a live click:
                           choose-model, use-llama3-2-3b, download-qwen3-vl-8b,
                           use-apple-intelligence, save-ollama-url

2026-11-26, after reinstalling the merged picker build
├─ hex window show skills  answered "Opened the window"; status reported
│                          window.pane skills
└─ permissions             reset to needs-request / needs-settings by the new
                           ad-hoc build; a live lesson needs fresh grants

2026-11-26, after reinstalling the gate build
├─ settings off            labs.watch_and_learn false -> `window show skills`
│                          answered unsupported, "Watch and learn is off in Labs"
├─ settings on             the same command opened the pane again; status
│                          reported window.pane skills
└─ permissions             still needs-request / needs-settings after the
                           reinstall; fresh grants are needed before a lesson

2026-11-26, status fields (tests only; the installed app was not rebuilt yet)
├─ scripts/check control:: 87 passed, including the status fields and the
│                          older-reply default test
├─ scripts/check labs::tests::watch_and_learn  1 passed
└─ scripts/check rune_ui::skills              57 passed
```

### Not verified

- Notice patterns has had no live run on a real Mac: no real folder was
  watched, no capsule was shown on screen, and no Files and Folders
  permission prompt was seen. Every behavior in its tree is read from the
  code and its tests (fixture folders and replayed events), not observed in
  the installed app; the pane was seen only as `--snap` scenes.
- Whether a person's real filing (Finder moves, browser downloads, Preview
  renames) produces the events and routines the tests replay is unchecked,
  as is the 5 s poll's cost on a large folder.

- Recording a real lesson: Screen Recording, Microphone, and Accessibility
  were granted to the earlier ad-hoc build (`permissions.ready: true`), but
  reinstalling the merged picker build reset those grants. No lesson was
  recorded in this pass, so Watch this and the capsule, a lesson's real
  transcript and click names, and the Skills pane opening on its own after a
  lesson were not run.
- A skill approved with questions left unanswered lists each as "Why: not
  answered" in `SKILL.md` (seen with the Apple Intelligence draft); whether
  a person is better off without those lines is open.
- Running a learned skill has had no live end-to-end run on a real Mac: the
  plan, confirmation, bounds, cancellation, and typed outcomes were checked
  in tests and fixture scenes only, never by replaying a recorded lesson.
- The model-default trial: qwen3.8:27b wrote the most complete skill on one
  lesson; `RECOMMENDED` is unchanged on that alone.
- Linux and Windows builds: no Linux or Windows target is installed here, so
  the cfg gates were checked by reading only.
- Linux goldens: the Skills scenes are macOS-only and the golden tests run
  only on Linux, so no golden covers this pane.
