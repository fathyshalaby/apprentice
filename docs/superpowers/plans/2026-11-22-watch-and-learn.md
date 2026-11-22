# Watch and learn: record a skill — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Behind a Labs switch, let a person record a narrated demonstration ("Watch this"), draft a `SKILL.md` from it with a local vision model after one approval, answer up to 5 "why?" questions, approve the skill, and install it into coding agents.

**Architecture:** A lesson is an Annotate capture with its own purpose flag: whole screen, Accessibility names under clicks on, never copied or routed, marked by a sidecar file. A new `src/apprentice/` module turns a lesson bundle into bounded evidence (text + downscaled key frames), calls Ollama (or Apple Intelligence, text only), parses a JSON skill draft, and stores drafts and skills owner-only under the data folder. A new Skills pane drives it; `agent_install` is generalised to install any skill folder.

**Tech Stack:** Rust, Rune/rune-kit UI, ureq 3 over `net::loopback_agent`, serde_json, `scrawl_capture` (bundle model, `render::{decode_png, downscale, encode_png}`), Ollama HTTP API (`/api/tags`, `/api/show`, `/api/chat`, `/api/pull`), `apple_intelligence::generate_text`.

**Spec:** `docs/superpowers/specs/2026-11-22-watch-and-learn-design.md`

## Global Constraints

- New feature is a Labs feature, **off by default**, audience Everyone, **macOS only**, gated where work starts (lesson start, drafting, install), not only in the UI.
- Local only: Ollama URL must be loopback (`net::loopback_agent` refuses others); no cloud model; **never switch models by itself**.
- Every model run, skill approval, skill edit save, install, and update is a separate explicit click. Nothing runs or saves before it.
- A lesson records the **whole screen** with no picker; its button note reads "Records your whole screen and your voice".
- Lessons are written to the normal captures folder and follow Annotate's retention; a lesson is **never copied to the clipboard or routed**.
- Video and audio are **never** sent to a model; key frames are sent to vision models only, downscaled to 1024 px wide, at most 4 per call and 12 per draft.
- Decisions ("why?") are capped at **5** per draft.
- Recommended models (exact strings): `gemma4:26b` (≈17 GB), `qwen3.8:27b` (≈18 GB), `gemma4:12b` (≈8 GB), `qwen3-vl:8b` (≈6 GB). Default Ollama URL `http://127.0.0.1:11434`.
- Skill files: `<support>/skills/<slug>/SKILL.md` + `skill.json`; drafts `<support>/skills/drafts/<id>/`; dirs 0700, files 0600 (`app_paths::create_private_dir`, `durable::write_private`).
- Agent install folder name: `apprentice-<slug>`; Apprentice never writes through a symlink and never replaces or removes a file it didn't write.
- `live.ndjson` events carry names, IDs and counts only — never narration, step text, or answers.
- Copy: sentence case, product name "Apprentice", all strings through `tr`/`tr_fill`, every language filled. UI uses `rune_ui/controls.rs`/`lists.rs` pieces and theme tokens; one primary button per screen.
- Code shape: no source file above 800 non-test lines for new files; `src/rune_ui/` holds 59 files (fail above 60) → the pane is `rune_ui/skills.rs` + `rune_ui/skills/`; `src/capture_host.rs` is at 3003 of its 3100 ceiling → keep its net growth under 60 lines; `rune_ui.rs` has ~21 lines of room; `desktop/i18n/{french,portuguese,russian}.rs` are full → new strings go in a new `desktop/i18n/apprentice.rs`. Avoid `x.rs` + `x_*.rs` prefix families.
- Tests run with `cargo test --release --bin hex -- <filter>` (cargo-nextest is not installed) and `cargo test --manifest-path crates/capture/Cargo.toml <filter>`. Known baseline failures to ignore: the 6 in `app_settings::…`, `local_api::tests::authenticated_server_reports_live_health_and_cleans_up_discovery`, `macos_rune::tests::input_source_choices_…`; skip `tests::only_app_launches_prewarm_the_gpu` in release.

## Review Focus

1. **Ollama stops or the chosen model is deleted after choosing it** → drafting fails with "Ollama isn't running" / "<model> isn't downloaded", the lesson stays ready to draft, and no other model is tried. (Task 6 test `a_missing_model_fails_without_switching`.)
2. **Model output is not valid JSON, has no steps, or more than 5 decisions** → raw text is kept and shown with Try again; extra decisions beyond 5 are dropped, not saved. (Task 5 tests.)
3. **A lesson whose video/audio retention already ran** (`.media-cleaned`, no `recording.mp4`) → still drafts from `scrawl.json` and `frames/`; a lesson with no `scrawl.json` reads "This lesson expired" and can't be drafted. (Task 3 + Task 4 tests.)
4. **An agent folder already holds a same-named skill the person wrote, or a symlink** → status reads Foreign/Linked, install and remove refuse and write nothing. (Task 8 tests.)
5. **A silent lesson or one without Accessibility names** (no words, clicks with no labels) → evidence still builds from window titles and frames, and the pane says "Little to learn from: no narration" / "no click names". (Task 4 test `thin_evidence_is_flagged`.)

---

### Task 1: The Watch and learn lab and its settings

**Files:**
- Modify: `src/labs/feature.rs` (variant `WatchAndLearn`, `ALL` 78→79), `src/labs/catalog.rs` (audience Everyone, `added` `20261122`, `oses` macOS, `name` `"watch-and-learn"`, `settings_key` `"labs.watch_and_learn"`), `src/labs/group.rs` (`Feature::group` → `Group::Annotate`), `src/labs/card.rs` (title "Watch and learn"; summary "Show Apprentice a task once and it writes a skill your coding agents can follow"), `src/labs/description.rs`, `src/labs.rs` (`on`, `switches`, `set`, own `static WATCH_AND_LEARN: AtomicBool`, load in `state()`/`apply()`, `watch_and_learn_enabled`), `src/labs/gates.rs` (gate), `src/labs/web.rs` (`on_the_web` excludes it like the other Annotate-dependent labs), `src/policy.rs` (`labs_lock`/`switches(Lock::Annotate)`), `src/app_settings/labs.rs` (`watch_and_learn: bool`, default false), `src/rune_ui/labs.rs` (row inside the macOS Annotate block, `row_key` `"lab-watch-and-learn"`), `src/control/settings/registry.rs` (`labs.watch_and_learn`, `apprentice.model`, `apprentice.ollama_url`).
- Create: `src/app_settings/apprentice.rs` (`ApprenticeSettings`), `src/desktop/i18n/apprentice.rs` (new string tables; chain in `i18n.rs` `all_translations()` and the French/Portuguese/Russian merge in `i18n/screenshots.rs`), `docs/features/apprentice.md` (+ index entry in `docs/features/README.md`).
- Test: inline in `src/labs.rs`, `src/app_settings/apprentice.rs`.

**Interfaces:**
- Produces: `crate::labs::watch_and_learn() -> bool` (gate: lab on, shipped, not managed, macOS, Annotate not locked; template `annotate_agent_tools_under`).
- Produces: `pub struct ApprenticeSettings { pub model: Option<String>, pub ollama_url: String }` in `AppSettings.apprentice` (`#[serde(default)]`); `model` is `None` or `"apple-intelligence"` or `"ollama:<name>"`; `ollama_url` default `"http://127.0.0.1:11434"`. `None` model is omitted on save.
- Produces: `pub enum ModelChoice { AppleIntelligence, Ollama { model: String } }` and `ApprenticeSettings::choice(&self) -> Option<ModelChoice>` (unknown strings → `None`).

- [ ] **Step 1: Write failing tests** — `watch_and_learn_is_its_own_switch_and_macos_only` (copy of `learn_from_corrections_is_its_own_switch_and_macos_only` with the new key), `apprentice_settings_default_to_no_model_and_local_ollama` (`choice()` is `None`, URL equals `"http://127.0.0.1:11434"`), `model_strings_parse` (`"ollama:gemma4:26b"` → `Ollama { model: "gemma4:26b" }`, `"apple-intelligence"` → `AppleIntelligence`, `"cloud:x"` → `None`).
- [ ] **Step 2: Run** `cargo test --release --bin hex -- watch_and_learn apprentice_settings model_strings` → FAIL (not defined).
- [ ] **Step 3: Implement** the lab across the files above, following `LearnFromCorrections` and the Annotate lock pattern; add `ApprenticeSettings`; register the three `hex settings` keys (`labs.watch_and_learn` Bool "Labs: watch and learn (macOS)"; `apprentice.model` String; `apprentice.ollama_url` String, validated loopback http URL); add card strings to `i18n/labs.rs` and the new module's French/Portuguese/Russian tables; write `docs/features/apprentice.md` with the entry points and checks from this plan.
- [ ] **Step 4: Run** the three tests plus `settings_keys_name_their_feature every_persisted_setting_has_a_cli_key every_lab_card_string_has_every_language every_translated_string_has_every_language keys_are_unique labs::web` → PASS.
- [ ] **Step 5: Commit** `feat(apprentice): add the Watch and learn lab and its settings`.

### Task 2: Lessons as an Annotate capture purpose

**Files:**
- Modify: `src/capture_host.rs` (replace the `clip: bool` thread with `purpose: Purpose`; `CaptureCommand::Lesson`; lesson branch in `finish_captures`; `start_request` sets `agent_context`), `crates/capture/src/capture/start.rs` (`Start.agent_context: Option<bool>`), `crates/capture/src/capture.rs:2563` (`let agent_context = req.agent_context.unwrap_or_else(|| engine_cfg.experiment(crate::handoff::AGENT_CONTEXT));`).
- Create: `src/capture_host/lesson.rs` (purpose enum, sidecar, hook) to keep `capture_host.rs` under its ceiling.
- Test: `src/capture_host/lesson.rs` inline; `capture_host.rs` tests using `FakeEngine.starts`.

**Interfaces:**
- Produces: `pub enum Purpose { Capture, Clip, Lesson }` with `fn handoff(self) -> bool` (only `Capture` copies/routes) and `fn folds_capsule(self) -> bool` (only `Clip`).
- Produces: `CaptureCommand::Lesson(CaptureTarget)`; the pane and menu send `CaptureControl::Command(CaptureCommand::Lesson(CaptureTarget::Display { at: scrawl_capture::screens::pointer().unwrap_or_default() }))` via `capture_host::send_to_listener`.
- Produces: `pub const LESSON_MARK: &str = ".apprentice-lesson.json"`; sidecar JSON `{"version":1,"startedAtMs":<u64>}` written owner-only into the bundle when a lesson is written.
- Produces: `pub fn on_lesson_written(written: fn(PathBuf))` (its own `OnceLock`, independent of `on_clip_written`).
- Produces: `RecordingCapture.lesson: bool` for the capsule/menu (capsule does **not** fold for a lesson).

- [ ] **Step 1: Write failing tests** — `a_lesson_starts_the_whole_screen_with_names_under_clicks` (FakeEngine's recorded `Start` has `agent_context == Some(true)` and Display target); `an_ordinary_capture_keeps_names_off` (`agent_context == None`); `a_written_lesson_is_marked_and_never_copied_or_routed` (after finish: sidecar exists with mode 0600, clipboard/paste_source untouched, no routing job, lesson hook called once with the bundle); `a_lesson_refuses_to_start_with_the_lab_off` (gate false → no `Start`); in crates/capture: `an_explicit_agent_context_overrides_the_engine_setting`.
- [ ] **Step 2: Run** `cargo test --release --bin hex -- capture_host::lesson a_lesson an_ordinary_capture` and `cargo test --manifest-path crates/capture/Cargo.toml explicit_agent_context` → FAIL.
- [ ] **Step 3: Implement** `Purpose` in `capture_host/lesson.rs` and thread it where `clip` flows today (`begin`, `Active`, `switch`, `Unfinished`, `Outcomes::start`/`is_clip` → `purpose(bundle)`, `collect` `routed`, `finish_captures`, `RecordingCapture`); lesson start checks `crate::labs::watch_and_learn()`; lesson microphone/movie behave as `Capture`.
- [ ] **Step 4: Run** the new tests and `cargo test --release --bin hex -- capture_host capture_capsule screenshots` → PASS; `python3 scripts/code-shape.py --check` → no failure.
- [ ] **Step 5: Commit** `feat(apprentice): record lessons as an Annotate capture purpose`.

### Task 3: Finding lessons

**Files:**
- Create: `src/apprentice.rs` (module root, `mod` lines only), `src/apprentice/lessons.rs`.
- Modify: `src/main.rs` (`mod apprentice;` under `#[cfg(all(target_os = "macos", hex_capture))]`), `src/capture_host/library.rs` (`load` → `pub(crate)`).

**Interfaces:**
- Produces: `pub struct Lesson { pub bundle: PathBuf, pub id: String, pub created_ms: u64, pub duration_ms: u64, pub media: Media, pub frames: usize }`, `pub enum Media { Present, Cleaned, Expired }` (`Expired` = no `scrawl.json`).
- Produces: `pub fn list(captures: &Path) -> Vec<Lesson>` (newest first; only bundles with `LESSON_MARK`).

- [ ] **Step 1: Write failing tests** with inline `.scrawl` fixtures (pattern: `library.rs` tests): `only_marked_bundles_are_lessons`, `lessons_list_newest_first`, `cleaned_media_still_lists_with_frames` (`.media-cleaned` → `Media::Cleaned`), `a_lesson_without_scrawl_json_reads_expired`.
- [ ] **Step 2: Run** `cargo test --release --bin hex -- apprentice::lessons` → FAIL.
- [ ] **Step 3: Implement** `list`.
- [ ] **Step 4: Run** → PASS.
- [ ] **Step 5: Commit** `feat(apprentice): list lesson bundles`.

### Task 4: Evidence from a lesson

**Files:**
- Create: `src/apprentice/evidence.rs`.

**Interfaces:**
- Consumes: `Lesson`, `capture_host::library::load`, `scrawl_capture::render::{decode_png, downscale, encode_png}`.
- Produces: `pub struct Evidence { pub segments: Vec<Segment>, pub thin: Thin }`, `pub struct Segment { pub start_ms: u64, pub end_ms: u64, pub text: String, pub frames: Vec<Frame> }`, `pub struct Frame { pub t_ms: u64, pub png: Vec<u8> }`, `pub struct Thin { pub no_narration: bool, pub no_click_names: bool }`.
- Produces: `pub struct Limits { pub max_segment_chars: usize, pub frames_per_segment: usize, pub max_frames: usize, pub frame_width: u32 }` with `Limits::for_vision() = { 12_000, 4, 12, 1024 }` and `Limits::for_text(context_tokens)` (`max_segment_chars = context_tokens * 3 / 2`, no frames).
- Produces: `pub fn build(lesson: &Lesson, limits: &Limits) -> Result<Evidence, String>`.

Text per segment is lines of the form `[mm:ss] said: …`, `[mm:ss] window: <app> — <title>`, `[mm:ss] clicked <role> "<label>" in <app>` (app from the latest window/app_change), in time order. Segments break at an app change or a narration gap over 2 s once the segment is at least half full, and always before `max_segment_chars`. Frames are the bundle's keyframes in that segment (by `t`), evenly thinned to the limits.

- [ ] **Step 1: Write failing tests** — `evidence_lines_read_in_time_order` (fixture with words, a window change, a labelled click → exact three lines); `segments_never_exceed_the_limit`; `frames_are_downscaled_and_capped` (16 keyframes → 12 frames, each decoded width ≤ 1024); `text_limits_carry_no_frames`; `thin_evidence_is_flagged` (no words, unlabelled clicks → both flags true, `build` still `Ok`); `secure_or_missing_labels_are_left_out` (click without label → line `clicked at <x>,<y>` with no quotes).
- [ ] **Step 2: Run** `cargo test --release --bin hex -- apprentice::evidence` → FAIL.
- [ ] **Step 3: Implement** `build`.
- [ ] **Step 4: Run** → PASS.
- [ ] **Step 5: Commit** `feat(apprentice): turn a lesson into bounded evidence`.

### Task 5: The skill record, its Markdown, and parsing a draft

**Files:**
- Create: `src/apprentice/skill.rs`, `src/apprentice/draft.rs` (prompt text + JSON schema + parse).

**Interfaces:**
- Produces: `pub struct Skill { pub slug: String, pub name: String, pub description: String, pub when_to_use: String, pub inputs: Vec<String>, pub steps: Vec<Step>, pub verification: Vec<String>, pub edge_cases: Vec<String>, pub autonomy: Autonomy, pub version: u32, pub created_ms: u64, pub updated_ms: u64, pub lesson_id: String }`, `pub struct Step { pub title: String, pub detail: String, pub question: Option<String>, pub why: Option<String> }`, `pub enum Autonomy { Observe, Suggest, Assist, AutonomousReview }` (serde snake_case; always `Observe` here). Serde camelCase, `skill.json` shape.
- Produces: `pub fn slug(name: &str) -> String` (lowercase ASCII, `-` separated, ≤ 48 chars, never empty → `"skill"`), `pub fn markdown(skill: &Skill) -> String`.
- Produces: `pub fn instructions() -> &'static str`, `pub fn schema() -> serde_json::Value` (Ollama `format`), `pub fn segment_prompt(seg: &Segment, n: usize, of: usize) -> String`, `pub fn merge_prompt(step_lists: &[String]) -> String`, `pub fn parse(raw: &str, lesson_id: &str, now_ms: u64) -> Result<Skill, ParseError>`, `pub struct ParseError { pub raw: String, pub reason: String }`.

Markdown layout: front matter `---\nname: <slug>\ndescription: <description>\n---`, then `# <name>`, sections `## When to use`, `## Inputs`, `## Steps` (numbered; detail beneath), `## Decisions` (each step with a question: `**<title>** — <question>` and `Why: <why>` or `Why: not answered`), `## Verification`, `## Edge cases`, `## Evidence` (`Learned from lesson <lesson_id>, recorded <yyyy-mm-dd>. Autonomy: observe.`). Empty sections are omitted, except Steps.

- [ ] **Step 1: Write failing tests** — `slugs_are_safe` (`"File the March invoice!"` → `"file-the-march-invoice"`, `"   "` → `"skill"`, 80-char name → ≤ 48); `markdown_matches_the_agent_skill_format` (fixture skill → exact expected string); `a_draft_parses_from_schema_json`; `more_than_five_questions_keep_the_first_five`; `invalid_json_keeps_the_raw_text` (`ParseError.raw` equals input); `a_draft_without_steps_is_rejected`; `json_inside_a_code_fence_still_parses`.
- [ ] **Step 2: Run** `cargo test --release --bin hex -- apprentice::skill apprentice::draft` → FAIL.
- [ ] **Step 3: Implement** both files.
- [ ] **Step 4: Run** → PASS.
- [ ] **Step 5: Commit** `feat(apprentice): skill record, SKILL.md, and draft parsing`.

### Task 6: Local models (Ollama client, Apple Intelligence) and drafting

**Files:**
- Create: `src/apprentice/ollama.rs`, `src/apprentice/models.rs` (recommended list, installed list, availability), `src/apprentice/run.rs` (drafting job).
- Test: `src/apprentice/ollama.rs` against a loopback `std::net::TcpListener` fake server.

**Interfaces:**
- Produces (`ollama.rs`): `pub struct Installed { pub name: String, pub size: u64, pub vision: bool }`; `pub fn installed(url: &str) -> Result<Vec<Installed>, ModelError>` (`/api/tags`, then `/api/show` `capabilities` contains `"vision"`); `pub fn chat(url: &str, model: &str, system: &str, prompt: &str, images: &[Vec<u8>], format: &serde_json::Value, timeout: Duration) -> Result<String, ModelError>` (`/api/chat`, `stream:false`, images base64 — hoist `opencode_service::base64` to `crate::desktop::base64`); `pub fn pull(url: &str, model: &str, progress: &mut dyn FnMut(PullProgress), cancel: &AtomicBool) -> Result<(), ModelError>` (NDJSON via `BufReader` over the body reader); `pub struct PullProgress { pub status: String, pub completed: u64, pub total: u64 }`.
- Produces: `pub enum ModelError { NotRunning, NotDownloaded(String), NotLoopback, Refused, Timeout, Failed(String) }` with `fn message(&self) -> String` (translated: "Ollama isn't running", "<model> isn't downloaded", …).
- Produces (`models.rs`): `pub struct Recommended { pub name: &'static str, pub size_gb: u32, pub note: &'static str }`, `pub const RECOMMENDED: [Recommended; 4]` with the four Global Constraints models in that order.
- Produces (`run.rs`): `pub enum Progress { Segment { n: usize, of: usize }, Merging, Done(Skill), Failed(String), Unparsed(ParseError) }`; `pub fn draft(lesson: &Lesson, choice: &ModelChoice, url: &str, cancel: Arc<AtomicBool>) -> mpsc::Receiver<Progress>` (spawns one thread named `apprentice-draft`; at most 8 segment calls + 1 merge; 180 s per call; 10 min total; a single segment skips the merge). Checks `labs::watch_and_learn()` first.

- [ ] **Step 1: Write failing tests** — `installed_marks_vision_models` (fake `/api/tags` + `/api/show`); `chat_sends_images_and_the_schema` (fake server asserts body has `images` base64 and `format`, `stream:false`); `pull_reports_progress_lines`; `a_non_loopback_url_is_refused` (`http://example.com:11434` → `NotLoopback`, no connection); `nothing_listening_reads_not_running`; `a_missing_model_fails_without_switching` (404 "model not found" → `Failed`/`NotDownloaded`, run emits exactly one `Failed` and no second model call); `one_segment_skips_the_merge`; `drafting_refuses_with_the_lab_off`; `recommended_models_are_the_four_from_the_spec`.
- [ ] **Step 2: Run** `cargo test --release --bin hex -- apprentice::ollama apprentice::models apprentice::run` → FAIL.
- [ ] **Step 3: Implement**; Apple Intelligence path calls `apple_intelligence::generate_text(instructions(), prompt, 0.2, 180 s)` with `Limits::for_text(context_tokens())` and maps `GenerationError` to `ModelError`.
- [ ] **Step 4: Run** → PASS.
- [ ] **Step 5: Commit** `feat(apprentice): draft skills with a local model`.

### Task 7: The skills store

**Files:**
- Create: `src/apprentice/store.rs`.

**Interfaces:**
- Produces: `pub struct Store { root: PathBuf }`, `Store::at(root) -> Store`, `Store::default() -> Result<Store>` (`app_paths::support_dir()?.join("skills")`).
- Produces: `pub struct Draft { pub id: String, pub lesson_id: String, pub skill: Skill, pub evidence_text: String }`; `save_draft(&self, &Draft) -> Result<()>` (writes `draft.json` and `evidence.txt` — the text evidence copied so a skill never depends on its recording); `drafts(&self) -> Vec<Draft>`; `discard_draft(&self, id: &str) -> Result<()>`; `pub enum Clash { Replace, KeepBoth }`; `approve(&self, id: &str, clash: Option<Clash>) -> Result<Approved, StoreError>` (`StoreError::Clash(slug)` when the slug exists and `clash` is `None`; `KeepBoth` appends `-2`, `-3`…; writes `skill.json` + regenerated `SKILL.md`; removes the draft); `skills(&self) -> Vec<Skill>`; `save_edit(&self, skill: Skill) -> Result<Skill>` (version + 1, `updated_ms` now, regenerates `SKILL.md`); `delete_skill(&self, slug: &str) -> Result<()>`.

- [ ] **Step 1: Write failing tests** (temp root) — `drafts_round_trip_with_their_evidence`; `approving_moves_a_draft_to_skills`; `a_clashing_slug_asks_first`; `keep_both_adds_a_number`; `an_edit_bumps_the_version_and_regenerates_markdown`; `everything_is_owner_only` (dirs 0700, files 0600); `skill_md_is_never_read_back` (edit `SKILL.md` on disk, `skills()` still returns `skill.json` content).
- [ ] **Step 2: Run** `cargo test --release --bin hex -- apprentice::store` → FAIL.
- [ ] **Step 3: Implement**.
- [ ] **Step 4: Run** → PASS.
- [ ] **Step 5: Commit** `feat(apprentice): store drafts and skills`.

### Task 8: Installing skills into agents

**Files:**
- Modify: `src/agent_install.rs` (parameterize; keep `hex-annotate` behaviour identical), `src/rune_ui/scrawl.rs` (callers).
- Create: `src/apprentice/install.rs` (Apprentice's use of it).

**Interfaces:**
- Produces (`agent_install.rs`): `pub(crate) struct SkillFile<'a> { pub folder: &'a str, pub contents: &'a [u8] }`, `pub(crate) const ANNOTATE: SkillFile<'static>` (`"hex-annotate"`, `SKILL`); `Paths::skill_path(&self, agent: Agent, folder: &str) -> PathBuf`; `status(paths, &SkillFile) -> Vec<AgentStatus>`; `install(paths, agent, &SkillFile) -> Result<PathBuf, String>`; new `remove(paths, agent, folder: &str) -> Result<(), String>`. Existing statuses gain `Foreign` (file present, no Apprentice mark, contents differ) used only when a mark is required.
- Produces (`apprentice/install.rs`): mark file `.apprentice-skill` beside `SKILL.md` containing `{"slug":…,"version":…}`; `pub enum Installed { Missing, Current, Older, Foreign, Linked }`; `pub fn status(paths: &Paths, skill: &Skill) -> Vec<(Agent, Installed)>`; `pub fn install(paths, agent, skill) -> Result<(), String>` (refuses `Foreign`/`Linked`); `pub fn remove(paths, agent, slug) -> Result<(), String>` (only with our mark; never through a link).

- [ ] **Step 1: Write failing tests** (use `agent_install`'s `TempDir`) — existing hex-annotate tests unchanged and passing; `an_apprentice_skill_installs_into_its_own_folder` (`claude/skills/apprentice-<slug>/SKILL.md` + mark); `an_older_version_reads_older`; `a_persons_own_skill_is_never_replaced` (same folder, no mark → `Foreign`, install errors, file bytes unchanged); `a_linked_folder_is_never_written`; `remove_only_removes_what_apprentice_wrote`.
- [ ] **Step 2: Run** `cargo test --release --bin hex -- agent_install apprentice::install` → FAIL (new tests).
- [ ] **Step 3: Implement**.
- [ ] **Step 4: Run** → PASS.
- [ ] **Step 5: Commit** `feat(apprentice): install skills into coding agents`.

### Task 9: Events

**Files:**
- Modify: `src/desktop/events.rs` (variant + `timestamp_ms`/`kind`), `src/desktop/activity.rs`, `src/control/dashboard.rs`, `src/macos_rune.rs` (hand the `EventLog` clone to `apprentice::events::set_log`).
- Create: `src/apprentice/events.rs`.

**Interfaces:**
- Produces: `VoiceEvent::Apprentice { timestamp_ms: u64, action: ApprenticeAction, id: String, count: Option<u32> }`, `pub enum ApprenticeAction { LessonWritten, DraftStarted, DraftFinished, DraftFailed, SkillApproved, SkillEdited, SkillInstalled, SkillRemoved, ModelDownloaded }` (snake_case). `id` is a lesson ID, draft ID, slug, or model name — never text.
- Produces: `pub fn set_log(log: EventLog)`, `pub fn emit(action: ApprenticeAction, id: &str, count: Option<u32>)` (no-op before `set_log`).

- [ ] **Step 1: Write failing tests** — `apprentice_events_round_trip`; extend `the_log_never_keeps_what_was_said` with an Apprentice event (serialized line has no field besides kind, timestamp, action, id, count).
- [ ] **Step 2: Run** `cargo test --release --bin hex -- desktop::events apprentice::events` → FAIL.
- [ ] **Step 3: Implement**; call `emit` from Task 2's lesson hook registration point, `run.rs`, `store.rs` approve/edit, `install.rs`, and model download.
- [ ] **Step 4: Run** → PASS.
- [ ] **Step 5: Commit** `feat(apprentice): log what Apprentice did, without text`.

### Task 10: The Skills pane

**Files:**
- Create: `src/rune_ui/skills.rs` (pane state + layout), `src/rune_ui/skills/model_picker.rs`, `src/rune_ui/skills/draft_view.rs`, `src/rune_ui/skills/agents.rs`.
- Modify: `src/rune_ui/sidebar.rs` (`Pane::Skills`, `shown` = `ui.settings.labs.watch_and_learn`, title "Skills", key `"nav-skills"`, icon), `src/rune_ui/labs.rs` (`main_panes` optional list), `src/rune_ui.rs` (≤ 21 lines: field `skills: skills::SkillsPane`, init, `poll(visible)`, body dispatch), `src/rune_ui/scenes.rs` (scenes `mac-skills-empty`, `mac-skills-lesson`, `mac-skills-draft`, `mac-skills-list` using `mac_fixture()`), `src/rune_ui/playground.rs` (`PATHS`: `Window/Skills/…`), `src/desktop/i18n.rs` (`SOURCES` += the four new files), `src/desktop/i18n/apprentice.rs` (strings).

**Interfaces:**
- Consumes: `lessons::list`, `run::draft`, `models::RECOMMENDED`, `ollama::{installed, pull}`, `Store`, `install::{status, install, remove}`, `capture_host::send_to_listener`, `capture_host::snapshot()` (recording state), settings save via the existing pane save path.
- Produces: `pub struct SkillsPane`, `SkillsPane::poll(&mut self, visible: bool)` (reads disk and channels only while visible), `SkillsPane::fixture(kind: Fixture)`, `pub fn pane(ui: &mut Ui, p: &Palette) -> Element`.

Layout and copy follow the spec's sketch: header "Skills" with **Watch this** (primary only when nothing else is pending) and note "Records your whole screen and your voice"; lesson cards ("New lesson · <duration> · <time>", "Reads: narration, names of what you click, window titles, screenshots · <model>", or "· text only" for Apple Intelligence; **Draft skill**, Discard; "This lesson expired" when `Media::Expired`; thin-evidence note); draft view (editable fields with `rune_kit::TextArea`, why? questions one at a time with Answer/Skip, "<n> of <m> why? answered", **Approve skill**, Discard, Try again on `Unparsed` with the raw text shown); model picker (installed Ollama models with "sees images"/"text only", recommended not-downloaded models with size and **Download** + progress, Apple Intelligence row with "text only", URL field); skills list with agent rows (Install / Update — "Older version installed" / Remove / "Not Apprentice's" / "Linked elsewhere") and the note "An agent may send this skill's text to its own model, which can be a cloud model".

- [ ] **Step 1: Write failing tests** in `src/rune_ui/tests/panes.rs` — `the_skills_pane_shows_only_with_its_lab`; `watch_this_sends_a_lesson_command` (fixture host records `CaptureCommand::Lesson(Display { .. })`); `draft_skill_is_the_only_way_a_model_runs` (no `run::draft` before the click; one after); `approve_saves_the_draft` ; `an_expired_lesson_cannot_be_drafted`; plus the four scenes pass `every_control_in_every_scene_has_a_name` and `every_scene_but_live_has_a_path_under_a_group`.
- [ ] **Step 2: Run** `cargo test --release --bin hex -- skills every_control_in_every_scene_has_a_name every_scene_but_live` → FAIL.
- [ ] **Step 3: Implement** the pane.
- [ ] **Step 4: Run** → PASS; then `every_translated_string_has_every_language every_interface_string_has_a_french_translation` (and Portuguese/Russian equivalents) → PASS; `python3 scripts/code-shape.py --check` → no failure.
- [ ] **Step 5: Commit** `feat(apprentice): Skills pane`.

### Task 11: Menu bar item and the lesson hook

**Files:**
- Modify: `src/macos_rune/status_item.rs` (`Row::WatchThis` after the Annotate rows, `ROWS` 24→25, title `label("Watch This…", tr("Watch this…"))`, `sel!(watchThis:)`, hidden unless `labs::watch_and_learn()` in `menuNeedsUpdate:`, dimmed while a capture records), `src/macos_rune.rs` (register `capture_host::on_lesson_written(apprentice::lessons::written)` and `apprentice::events::set_log`), `src/desktop/i18n/apprentice.rs`.
- Modify: `src/apprentice/lessons.rs` — `pub fn written(bundle: PathBuf)` (emits `LessonWritten`, opens the Skills pane via the existing `StatusItemAction`/`Destination` pattern).

- [ ] **Step 1: Write failing tests** — `watch_this_is_hidden_with_the_lab_off` (status item row hidden-state helper, pattern `refresh_restore_capture`); `a_written_lesson_opens_the_skills_pane` (fake destination channel receives `Destination::Skills`).
- [ ] **Step 2: Run** `cargo test --release --bin hex -- watch_this a_written_lesson_opens` → FAIL.
- [ ] **Step 3: Implement**.
- [ ] **Step 4: Run** → PASS.
- [ ] **Step 5: Commit** `feat(apprentice): Watch this in the menu bar`.

### Task 12: End-to-end check on this Mac and the model default

**Files:**
- Modify: `docs/features/apprentice.md` (record what was observed), `src/apprentice/models.rs` only if the trial changes the order.

- [ ] **Step 1:** Full suite `cargo test --release --bin hex -- --skip tests::only_app_launches_prewarm_the_gpu` → only the known baseline failures listed in Global Constraints; `cargo test --manifest-path crates/capture/Cargo.toml` → PASS; `python3 scripts/code-shape.py --check` → PASS.
- [ ] **Step 2:** `HEX_RELEASE_UNSIGNED=1 sh scripts/build-app.sh`, launch `target/app/Apprentice.app`, turn on Labs › Watch and learn, confirm the Skills pane and menu item appear and vanish with the switch.
- [ ] **Step 3 (needs the owner's go-ahead: ~35 GB download):** pull `gemma4:26b` and `qwen3.8:27b` from the pane's Download buttons; record one 1–2 minute narrated lesson; draft it with each; answer one why?; approve; install into one agent; check the installed `SKILL.md`.
- [ ] **Step 4:** Write the observed results (time per draft, whether steps matched the demonstration) into `docs/features/apprentice.md`; reorder `RECOMMENDED` if the second model was clearly better.
- [ ] **Step 5: Commit** `docs(apprentice): record the first real lesson`.
