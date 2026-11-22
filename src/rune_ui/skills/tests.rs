//! The Skills pane driven headlessly from its scenes: the lab shows it,
//! Watch this starts a lesson, Draft skill is the only way a model runs,
//! approving saves the draft, and an expired lesson can't be drafted.

use std::path::PathBuf;

use rune::text::TextSystem;
use rune::{Env, Runtime, TextEdit};

use super::super::{Pane, SCENES, Service, Ui, offscreen, on_key, poll, view};
use super::Recorded;
use crate::app_settings::{AppSettings, ModelChoice};
use crate::apprentice::lessons::Media;
use crate::apprentice::store::Store;
use crate::capture_host::{CaptureCommand, CaptureTarget};

/// Tall enough that every card of a scene is on screen.
const SIZE: (f32, f32) = (980.0, 2400.0);

fn runtime(scene: &str) -> Runtime<Ui> {
    let mut state = Ui::with(AppSettings::default(), Service::Fixture(Vec::new()));
    let (_, apply) = SCENES
        .iter()
        .find(|(name, _)| *name == scene)
        .unwrap_or_else(|| panic!("no scene {scene}"));
    apply(&mut state);
    let env = Env {
        system_dark: false,
        scale: 2.0,
    };
    let text = TextSystem::new(rune_theme_opencode::FONTS, rune_theme_opencode::FAMILIES);
    let mut rt = offscreen::runtime(state, view, SIZE, env, text);
    rt.poll = Some(poll);
    rt.on_key = Some(on_key);
    rt.settle_all();
    rt.frame(0.0);
    rt
}

fn settle(rt: &mut Runtime<Ui>) {
    for _ in 0..30 {
        rt.frame(1.0 / 30.0);
    }
    rt.settle_all();
    rt.frame(0.0);
}

/// Redraws after a test changes state directly.
fn refresh(rt: &mut Runtime<Ui>) {
    rt.invalidate();
    settle(rt);
}

fn tap(rt: &mut Runtime<Ui>, key: &str) {
    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(rt);
}

fn texts(rt: &Runtime<Ui>) -> Vec<String> {
    rt.elements().into_iter().filter_map(|e| e.text).collect()
}

fn recorded(rt: &Runtime<Ui>) -> &Recorded {
    rt.state
        .skills
        .fixture
        .as_ref()
        .expect("scenes record what the pane would do")
}

/// A fresh, empty folder for a store.
fn temp(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "apprentice-skills-pane-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&path);
    path
}

#[test]
fn the_skills_pane_shows_only_with_its_lab() {
    let mut rt = runtime("mac-settings");
    assert!(!rt.state.settings.labs.watch_and_learn);
    assert!(
        rt.find("nav-skills").is_none(),
        "hidden while the lab is off"
    );
    rt.state.settings.labs.watch_and_learn = true;
    refresh(&mut rt);
    tap(&mut rt, "nav-skills");
    assert_eq!(rt.state.pane, Pane::Skills);
    assert!(rt.find("watch-this").is_some());
}

#[test]
fn watch_this_sends_a_lesson_command() {
    let mut rt = runtime("mac-skills-empty");
    assert!(recorded(&rt).commands.is_empty());
    tap(&mut rt, "watch-this");
    assert!(
        matches!(
            recorded(&rt).commands.as_slice(),
            [CaptureCommand::Lesson(CaptureTarget::Display { .. })]
        ),
        "{:?}",
        recorded(&rt).commands
    );

    // While a capture records, Watch this can't be pressed.
    rt.state.skills.recording = true;
    refresh(&mut rt);
    tap(&mut rt, "watch-this");
    assert_eq!(recorded(&rt).commands.len(), 1);

    // With the lab off nothing starts, even when asked directly.
    rt.state.skills.recording = false;
    rt.state.settings.labs.watch_and_learn = false;
    super::watch(&mut rt.state);
    assert_eq!(recorded(&rt).commands.len(), 1);
}

#[test]
fn draft_skill_is_the_only_way_a_model_runs() {
    let mut rt = runtime("mac-skills-lesson");
    // Showing the lesson, opening the model picker, and closing it run
    // nothing.
    tap(&mut rt, "choose-model");
    tap(&mut rt, "choose-model");
    assert!(recorded(&rt).drafts.is_empty());
    assert!(recorded(&rt).pulls.is_empty());

    let lesson = rt.state.skills.lessons[0].id.clone();
    tap(&mut rt, "draft-skill");
    assert_eq!(
        recorded(&rt).drafts,
        [(
            lesson,
            ModelChoice::Ollama {
                model: "gemma4:26b".into()
            }
        )]
    );
    // The card now shows the run, with no second Draft skill to press.
    assert!(rt.find("draft-skill").is_none());
    assert!(rt.find("cancel-draft").is_some());
    assert_eq!(recorded(&rt).drafts.len(), 1);
}

#[test]
fn without_a_model_the_lesson_asks_for_one_first() {
    let mut rt = runtime("mac-skills-lesson");
    rt.state.settings.apprentice.model = None;
    refresh(&mut rt);
    assert!(rt.find("draft-skill").is_none());
    tap(&mut rt, "choose-model-first");
    assert!(rt.state.skills.picker.open, "the picker opens");
    assert!(recorded(&rt).drafts.is_empty());
}

#[test]
fn approve_saves_the_draft() {
    let root = temp("approve");
    let mut rt = runtime("mac-skills-draft");
    let store = Store::at(root.clone());
    store.save_draft(&rt.state.skills.drafts[0]).unwrap();
    rt.state.skills.store = Some(store);
    // An edit in the draft view goes into the approved skill.
    rt.state.skills.editor.as_mut().unwrap().name = TextEdit::new("File the March invoice");
    refresh(&mut rt);
    tap(&mut rt, "approve-skill");

    let store = Store::at(root.clone());
    let skills = store.skills();
    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0].name, "File the March invoice");
    assert!(store.drafts().is_empty(), "the draft became the skill");
    assert!(rt.state.skills.editor.is_none());
    assert!(
        rt.state
            .skills
            .skills
            .iter()
            .any(|s| s.slug == skills[0].slug)
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn approving_over_an_existing_skill_asks_replace_or_keep_both() {
    let root = temp("clash");
    let mut rt = runtime("mac-skills-draft");
    let store = Store::at(root.clone());
    let first = rt.state.skills.drafts[0].clone();
    store.save_draft(&first).unwrap();
    store.approve(&first.id, None).unwrap();
    let mut again = first.clone();
    again.id = format!("{}-2", first.id);
    store.save_draft(&again).unwrap();
    rt.state.skills.store = Some(store);
    rt.state.skills.drafts = vec![again.clone()];
    rt.state.skills.editor = Some(super::draft_view::Editor::for_draft(&again));
    refresh(&mut rt);

    tap(&mut rt, "approve-skill");
    assert!(rt.find("keep-both").is_some(), "{:?}", texts(&rt));
    assert_eq!(
        Store::at(root.clone()).skills().len(),
        1,
        "nothing saved yet"
    );
    tap(&mut rt, "keep-both");
    assert_eq!(Store::at(root.clone()).skills().len(), 2);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn discarding_a_draft_takes_a_second_click() {
    let root = temp("discard");
    let mut rt = runtime("mac-skills-draft");
    let store = Store::at(root.clone());
    store.save_draft(&rt.state.skills.drafts[0]).unwrap();
    rt.state.skills.store = Some(store);
    refresh(&mut rt);
    tap(&mut rt, "discard-draft");
    assert_eq!(Store::at(root.clone()).drafts().len(), 1, "only asks");
    tap(&mut rt, "confirm-discard-draft");
    assert!(Store::at(root.clone()).drafts().is_empty());
    assert!(rt.state.skills.editor.is_none());
    // To the Trash, not deleted for good.
    let folder = root.join("drafts").join("1763800000000-ab12");
    assert!(!folder.exists());
    assert!(crate::apprentice::store::trashed(&folder));
    let _ = std::fs::remove_dir_all(root);
}

/// Pressing twice where Discard was: the second press lands on Cancel, so
/// a double click never confirms a discard.
fn double_click(rt: &mut Runtime<Ui>, key: &str) {
    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
    for _ in 0..2 {
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        settle(rt);
    }
}

#[test]
fn a_double_click_never_confirms_a_discard() {
    let root = temp("double-discard");
    let mut rt = runtime("mac-skills-draft");
    let store = Store::at(root.clone());
    store.save_draft(&rt.state.skills.drafts[0]).unwrap();
    rt.state.skills.store = Some(store);
    refresh(&mut rt);
    double_click(&mut rt, "discard-draft");
    assert_eq!(Store::at(root.clone()).drafts().len(), 1);
    let editor = rt
        .state
        .skills
        .editor
        .as_ref()
        .expect("the draft stays open");
    assert!(!editor.discard_armed, "the second press was Cancel");
    let _ = std::fs::remove_dir_all(root);

    let mut rt = runtime("mac-skills-lesson");
    double_click(&mut rt, "discard-lesson");
    assert_eq!(rt.state.skills.lessons.len(), 1);
    assert!(
        rt.state.skills.armed.is_none(),
        "the second press was Cancel"
    );
    // An expired lesson's only button is Discard: the same holds.
    rt.state.skills.lessons[0].media = Media::Expired;
    refresh(&mut rt);
    double_click(&mut rt, "discard-lesson");
    assert_eq!(rt.state.skills.lessons.len(), 1);
}

#[test]
fn answering_a_why_question_keeps_the_answer() {
    let mut rt = runtime("mac-skills-draft");
    let question = rt.state.skills.editor.as_ref().unwrap().question();
    let index = question.expect("the fixture draft asks why");
    rt.state.skills.editor.as_mut().unwrap().answer = TextEdit::new("It is the newest one");
    refresh(&mut rt);
    tap(&mut rt, "answer-why");
    let editor = rt.state.skills.editor.as_ref().unwrap();
    assert_eq!(
        editor.base.steps[index].why.as_deref(),
        Some("It is the newest one")
    );
    assert_ne!(editor.question(), Some(index), "the next question shows");
}

#[test]
fn an_expired_lesson_cannot_be_drafted() {
    let mut rt = runtime("mac-skills-lesson");
    rt.state.skills.lessons[0].media = Media::Expired;
    refresh(&mut rt);
    assert!(rt.find("draft-skill").is_none());
    assert!(
        texts(&rt).iter().any(|t| t == "This lesson expired"),
        "{:?}",
        texts(&rt)
    );
    let lesson = rt.state.skills.lessons[0].id.clone();
    super::draft(&mut rt.state, &lesson);
    assert!(recorded(&rt).drafts.is_empty());
    assert!(rt.state.skills.drafting.is_none());
}

#[test]
fn a_download_is_its_own_click() {
    let mut rt = runtime("mac-skills-models");
    assert!(recorded(&rt).pulls.is_empty());
    tap(&mut rt, "download-qwen3-vl-8b");
    assert_eq!(recorded(&rt).pulls, ["qwen3-vl:8b"]);
    // One download at a time: another model's Download waits.
    tap(&mut rt, "download-gemma4-12b");
    assert_eq!(recorded(&rt).pulls.len(), 1);
}

#[test]
fn a_remote_ollama_address_is_refused() {
    let mut rt = runtime("mac-skills-models");
    let saved = rt.state.settings.apprentice.ollama_url.clone();
    rt.state.skills.picker.url = TextEdit::new("http://example.com:11434");
    refresh(&mut rt);
    tap(&mut rt, "save-ollama-url");
    assert_eq!(rt.state.settings.apprentice.ollama_url, saved);
    assert!(rt.state.skills.picker.url_error.is_some());

    rt.state.skills.picker.url = TextEdit::new("http://localhost:11500");
    refresh(&mut rt);
    tap(&mut rt, "save-ollama-url");
    assert_eq!(
        rt.state.settings.apprentice.ollama_url,
        "http://localhost:11500"
    );
}

// ---------------------------------------------------------------------------
// A running draft, fed by hand: what each of its last words leaves behind.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{self, Sender};

use crate::apprentice::draft::ParseError;
use crate::apprentice::run::Progress;
use crate::apprentice::skill::Skill;

/// Starts a hand-fed draft of the scene's first lesson, with the pane
/// saving into a store at `root`.
fn running(rt: &mut Runtime<Ui>, root: &std::path::Path) -> Sender<Progress> {
    let (send, rx) = mpsc::channel();
    let pane = &mut rt.state.skills;
    pane.store = Some(Store::at(root.to_path_buf()));
    pane.drafting = Some(super::Drafting {
        lesson: pane.lessons[0].id.clone(),
        stage: super::Stage::Starting,
        rx: Some(rx),
        cancel: Arc::new(AtomicBool::new(false)),
    });
    refresh(rt);
    send
}

fn drafted_skill(lesson: &str) -> Skill {
    let mut skill = super::SkillsPane::fixture(super::Fixture::Draft).drafts[0]
        .skill
        .clone();
    skill.lesson_id = lesson.into();
    skill
}

fn shows(rt: &Runtime<Ui>, copy: &str) -> bool {
    texts(rt).iter().any(|t| t == copy)
}

#[test]
fn a_finished_draft_is_saved_with_its_text_and_opens() {
    let root = temp("done");
    let mut rt = runtime("mac-skills-lesson");
    let lesson = rt.state.skills.lessons[0].id.clone();
    if let Some(Some(notes)) = rt.state.skills.notes.get_mut(&lesson) {
        notes.text = "[00:01] said: rename the files".into();
    }
    let send = running(&mut rt, &root);
    send.send(Progress::Segment { n: 2, of: 3 }).unwrap();
    refresh(&mut rt);
    assert!(shows(&rt, "Drafting · part 2 of 3"), "{:?}", texts(&rt));
    send.send(Progress::Merging).unwrap();
    refresh(&mut rt);
    assert!(shows(&rt, "Putting it together…"));

    send.send(Progress::Done(drafted_skill(&lesson))).unwrap();
    refresh(&mut rt);
    assert!(rt.state.skills.drafting.is_none());
    let saved = Store::at(root.clone()).drafts();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].lesson_id, lesson);
    assert_eq!(saved[0].evidence_text, "[00:01] said: rename the files");
    let editor = rt.state.skills.editor.as_ref().expect("the draft opens");
    assert_eq!(editor.base.name, saved[0].skill.name);
    assert!(rt.find("approve-skill").is_some());
    assert!(rt.find("draft-skill").is_none(), "the lesson is drafted");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_failed_draft_says_why_and_offers_try_again() {
    let root = temp("failed");
    let mut rt = runtime("mac-skills-lesson");
    let send = running(&mut rt, &root);
    send.send(Progress::Failed("Ollama isn't running".into()))
        .unwrap();
    refresh(&mut rt);
    assert!(rt.state.skills.drafting.is_none());
    assert!(shows(&rt, "Ollama isn't running"), "{:?}", texts(&rt));
    assert!(rt.find("try-again").is_some());
    assert!(rt.state.skills.editor.is_none());

    tap(&mut rt, "try-again");
    assert_eq!(recorded(&rt).drafts.len(), 1, "Try again is one more run");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn an_unreadable_answer_shows_the_raw_text_and_offers_try_again() {
    let root = temp("unparsed");
    let mut rt = runtime("mac-skills-lesson");
    let send = running(&mut rt, &root);
    send.send(Progress::Unparsed(ParseError {
        raw: "I think the steps are: open Mail".into(),
        reason: "not JSON".into(),
    }))
    .unwrap();
    refresh(&mut rt);
    assert!(shows(&rt, "The model's answer couldn't be read"));
    assert!(shows(&rt, "I think the steps are: open Mail"));
    assert!(rt.find("try-again").is_some());
    assert!(Store::at(root.clone()).drafts().is_empty());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_draft_that_ends_without_a_last_word_reads_as_stopped() {
    let root = temp("gone");
    let mut rt = runtime("mac-skills-lesson");
    let send = running(&mut rt, &root);
    send.send(Progress::Segment { n: 1, of: 2 }).unwrap();
    drop(send);
    refresh(&mut rt);
    assert!(rt.state.skills.drafting.is_none());
    assert!(
        shows(&rt, "Drafting stopped unexpectedly"),
        "{:?}",
        texts(&rt)
    );
    assert!(rt.find("try-again").is_some());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_cancelled_draft_returns_quietly_to_its_lesson() {
    let root = temp("cancelled");
    let mut rt = runtime("mac-skills-lesson");
    let send = running(&mut rt, &root);
    tap(&mut rt, "cancel-draft");
    assert!(shows(&rt, "Stopping…"));
    // The run stops before its next call and says so.
    send.send(Progress::Failed("Cancelled".into())).unwrap();
    refresh(&mut rt);
    assert!(rt.state.skills.drafting.is_none());
    assert!(rt.state.skills.failed.is_none());
    assert!(!shows(&rt, "Cancelled"));
    assert!(rt.find("draft-skill").is_some());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_draft_finishing_under_an_open_editor_leaves_it_alone() {
    let root = temp("busy");
    let mut rt = runtime("mac-skills-draft");
    // A second lesson drafts while the first draft is being edited.
    let mut other = rt.state.skills.lessons[0].clone();
    other.id = "lesson-9".into();
    rt.state.skills.lessons.insert(0, other);
    rt.state.skills.editor.as_mut().unwrap().name = TextEdit::new("Typed, not saved");
    let open_id = rt.state.skills.drafts[0].id.clone();
    let send = running(&mut rt, &root);
    send.send(Progress::Done(drafted_skill("lesson-9")))
        .unwrap();
    refresh(&mut rt);

    let editor = rt.state.skills.editor.as_ref().unwrap();
    assert!(
        matches!(&editor.source, super::draft_view::Source::Draft { id, .. } if *id == open_id)
    );
    assert_eq!(editor.name.text, "Typed, not saved");
    assert_eq!(
        Store::at(root.clone()).drafts().len(),
        1,
        "the new draft is saved"
    );
    assert!(
        rt.find("review-draft").is_some(),
        "and waits to be reviewed"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn review_over_unsaved_typing_asks_first() {
    let mut rt = runtime("mac-skills-draft");
    let mut second = rt.state.skills.drafts[0].clone();
    second.id = "1763800000001-cd34".into();
    second.skill.name = "Send the weekly report".into();
    rt.state.skills.drafts.push(second.clone());
    refresh(&mut rt);
    // Nothing typed: Review just opens.
    tap(&mut rt, "review-draft");
    assert_eq!(
        rt.state.skills.editor.as_ref().unwrap().base.name,
        second.skill.name
    );

    rt.state.skills.editor.as_mut().unwrap().name = TextEdit::new("Typed, not saved");
    refresh(&mut rt);
    tap(&mut rt, "review-draft");
    let editor = rt.state.skills.editor.as_ref().unwrap();
    assert_eq!(editor.name.text, "Typed, not saved", "nothing replaced yet");
    tap(&mut rt, "keep-editing");
    assert_eq!(
        rt.state.skills.editor.as_ref().unwrap().name.text,
        "Typed, not saved"
    );
    assert!(rt.find("confirm-switch").is_none());

    tap(&mut rt, "review-draft");
    tap(&mut rt, "confirm-switch");
    assert_eq!(
        rt.state.skills.editor.as_ref().unwrap().base.name,
        "File the monthly invoice"
    );
}

#[test]
fn the_skills_destination_opens_the_pane_while_the_lab_is_on() {
    let mut rt = runtime("mac-settings");
    rt.state.settings.labs.watch_and_learn = true;
    refresh(&mut rt);
    rt.update(|ui, tx| crate::rune_ui::show(ui, crate::rune_ui::Destination::Skills, tx));
    assert_eq!(rt.state.pane, Pane::Skills);
}

/// Sets the chosen Ollama model and what the picker knows of it.
fn choose_ollama(rt: &mut Runtime<Ui>, model: &str, vision: bool) {
    rt.state.settings.apprentice.model = Some(format!("ollama:{model}"));
    rt.state.skills.picker.installed = Some(Ok(vec![crate::apprentice::ollama::Installed {
        name: model.into(),
        size: 2_000_000_000,
        vision,
    }]));
    refresh(rt);
}

#[test]
fn a_text_only_model_never_reads_screenshots() {
    let mut rt = runtime("mac-skills-lesson");
    choose_ollama(&mut rt, "llama3.2:3b", false);
    let shown = texts(&rt);
    assert!(
        shown.iter().all(|copy| !copy.contains("screenshots")),
        "{shown:?}"
    );
    assert!(
        shows(
            &rt,
            "Reads: narration, names of what you click, window titles · llama3.2:3b · text only"
        ),
        "{shown:?}"
    );
    choose_ollama(&mut rt, "gemma4:26b", true);
    assert!(shows(
        &rt,
        "Reads: narration, names of what you click, window titles, screenshots · gemma4:26b"
    ));
}

#[test]
fn inputs_are_edited_in_the_draft_view() {
    let root = temp("inputs");
    let mut rt = runtime("mac-skills-draft");
    let store = Store::at(root.clone());
    store.save_draft(&rt.state.skills.drafts[0]).unwrap();
    rt.state.skills.store = Some(store);
    refresh(&mut rt);
    assert!(rt.find("inputs").is_some(), "the draft view shows Inputs");
    let editor = rt.state.skills.editor.as_mut().unwrap();
    assert_eq!(editor.inputs.text, "The month");
    editor.inputs = TextEdit::new("The month\n\n  The invoice email ");
    refresh(&mut rt);
    tap(&mut rt, "approve-skill");
    let skills = Store::at(root.clone()).skills();
    assert_eq!(skills[0].inputs, ["The month", "The invoice email"]);
    let _ = std::fs::remove_dir_all(root);
}

/// The real app's pane (no scene) with a skills folder it can't open.
fn without_a_folder(scene: &str) -> Runtime<Ui> {
    let mut rt = runtime(scene);
    let pane = &mut rt.state.skills;
    pane.fixture = None;
    pane.store = None;
    pane.refuse_store = true;
    // Nothing reads the real folders or Ollama while the test runs.
    pane.read_at = Some(std::time::Instant::now());
    pane.picker.installed = Some(Ok(Vec::new()));
    refresh(&mut rt);
    rt
}

const NO_FOLDER: &str = "Apprentice can't open its skills folder";

#[test]
fn without_its_folder_nothing_changes_on_screen_only() {
    let mut rt = without_a_folder("mac-skills-draft");
    tap(&mut rt, "approve-skill");
    assert!(rt.state.skills.skills.is_empty(), "no skill on screen only");
    assert_eq!(rt.state.skills.drafts.len(), 1);
    assert!(rt.state.skills.editor.is_some());
    assert!(shows(&rt, NO_FOLDER), "{:?}", texts(&rt));
    tap(&mut rt, "discard-draft");
    tap(&mut rt, "confirm-discard-draft");
    assert_eq!(rt.state.skills.drafts.len(), 1, "nothing discarded");
    assert!(rt.state.skills.editor.is_some());

    let mut rt = without_a_folder("mac-skills-list");
    let before = rt.state.skills.skills.len();
    tap(&mut rt, "edit-skill");
    tap(&mut rt, "save-skill");
    assert!(rt.state.skills.editor.is_some(), "the edit stays open");
    assert!(shows(&rt, NO_FOLDER));
    tap(&mut rt, "close-skill");
    tap(&mut rt, "delete-skill");
    tap(&mut rt, "confirm-delete-skill");
    assert_eq!(rt.state.skills.skills.len(), before, "nothing deleted");
    assert_eq!(rt.state.skills.error.as_deref(), Some(NO_FOLDER));
}

// ---------------------------------------------------------------------------
// Labs › Notice patterns' Noticing section.

use super::noticing::{self, Asked};

/// Every node of the accessibility tree as (role, name).
fn named(tree: &serde_json::Value, out: &mut Vec<(String, String)>) {
    let role = tree["role"].as_str().unwrap_or_default().to_owned();
    let name = tree["name"].as_str().unwrap_or_default().trim().to_owned();
    out.push((role, name));
    for child in tree["children"].as_array().into_iter().flatten() {
        named(child, out);
    }
}

fn button_named(rt: &Runtime<Ui>, label: &str) -> bool {
    let mut nodes = Vec::new();
    named(&rt.accessibility_json(), &mut nodes);
    nodes
        .iter()
        .any(|(role, name)| role == "button" && name == label)
}

fn home(relative: &str) -> PathBuf {
    crate::app_paths::home_dir()
        .expect("a home folder")
        .join(relative)
}

#[test]
fn noticing_is_hidden_while_the_lab_is_off() {
    let rt = runtime("mac-skills-empty");
    assert!(rt.find("noticing").is_none(), "off by default");

    let mut rt = runtime("mac-skills-noticing");
    assert!(rt.find("noticing").is_some());
    rt.state.settings.labs.notice_patterns = false;
    refresh(&mut rt);
    assert!(rt.find("noticing").is_none());
    assert!(rt.find("add-folder").is_none());
    // Watch and learn off closes it too: its Show me starts a lesson.
    rt.state.settings.labs.notice_patterns = true;
    rt.state.settings.labs.watch_and_learn = false;
    assert!(
        super::noticing::section(&rt.state, crate::rune_ui::controls::Paint::new(false, 2.0))
            .is_none()
    );
}

#[test]
fn noticing_lists_folders_and_routines() {
    let rt = runtime("mac-skills-noticing");
    let shown = texts(&rt);
    for copy in [
        "Noticing",
        "Watched folders",
        "~/Downloads",
        "~/Documents/Scans",
        "Can't read this folder",
        "Routines",
        "Moves PDF files from Downloads into folders like Invoices 2026-11, then renames them like #-#-# a.pdf (3 days)",
        "Apprentice keeps file kinds, folder names and times for 14 days, on this Mac.",
    ] {
        assert!(shown.iter().any(|t| t == copy), "no {copy:?} in {shown:?}");
    }
    // Only the folder Apprentice can't read says so and links to Settings.
    assert!(rt.find("open-settings-1").is_some());
    assert!(rt.find("open-settings-0").is_none());
    for label in [
        "Add folder…",
        "Remove",
        "Open Settings",
        "Show me",
        "Forget",
        "Forget everything…",
    ] {
        assert!(button_named(&rt, label), "no button named {label:?}");
    }
    let mut nodes = Vec::new();
    named(&rt.accessibility_json(), &mut nodes);
    assert!(
        nodes.iter().any(|(_, name)| name == "~/Downloads"),
        "the folder's row is named by its path: {nodes:?}"
    );

    let empty = runtime("mac-skills-noticing-empty");
    assert!(empty.find("add-folder").is_some());
    assert!(empty.find("routines").is_none(), "no routines, no list");
    assert!(empty.find("skills-empty").is_some());
    // Listed routines are something to do: the empty state gives way.
    assert!(rt.find("skills-empty").is_none());
}

#[test]
fn forget_everything_asks_twice() {
    let mut rt = runtime("mac-skills-noticing");
    tap(&mut rt, "forget-everything");
    assert!(recorded(&rt).noticing.is_empty(), "only asks");
    assert!(rt.find("confirm-forget-everything").is_some());
    tap(&mut rt, "cancel-forget-everything");
    assert!(recorded(&rt).noticing.is_empty());
    assert!(rt.find("confirm-forget-everything").is_none());

    // A double click lands on Cancel.
    double_click(&mut rt, "forget-everything");
    assert!(recorded(&rt).noticing.is_empty());

    tap(&mut rt, "forget-everything");
    tap(&mut rt, "confirm-forget-everything");
    assert_eq!(recorded(&rt).noticing, [Asked::ForgetEverything]);
    assert!(rt.find("routines").is_none(), "nothing listed any more");
    assert!(
        rt.state.settings.apprentice.watched_folders.len() == 2,
        "the folders stay watched"
    );
}

#[test]
fn show_me_and_forget_answer_their_routine() {
    let mut rt = runtime("mac-skills-noticing");
    let id = rt.state.skills.noticing.status.as_ref().unwrap().routines[0]
        .id
        .clone();
    tap(&mut rt, "show-me");
    assert_eq!(recorded(&rt).noticing, [Asked::ShowMe(id.clone())]);
    // With no model chosen, Show me still starts the lesson.
    rt.state.settings.apprentice.model = None;
    refresh(&mut rt);
    tap(&mut rt, "show-me");
    assert_eq!(recorded(&rt).noticing.len(), 2);
    tap(&mut rt, "forget");
    assert_eq!(recorded(&rt).noticing[2], Asked::Forget(id));
    assert!(rt.find("routines").is_none(), "a forgotten routine leaves");
}

#[test]
fn adding_a_folder_saves_the_setting() {
    let mut rt = runtime("mac-skills-noticing-empty");
    assert!(rt.state.settings.apprentice.watched_folders.is_empty());
    tap(&mut rt, "add-folder");
    assert_eq!(recorded(&rt).noticing, [Asked::ChooseFolder]);
    let saved = rt.state.saved;

    // The dialog's answer: cancelled changes nothing.
    noticing::chosen(&mut rt.state, Ok(None));
    assert!(rt.state.settings.apprentice.watched_folders.is_empty());
    assert_eq!(rt.state.saved, saved);

    let folder = home("Downloads");
    noticing::chosen(&mut rt.state, Ok(Some(folder.clone())));
    refresh(&mut rt);
    assert_eq!(
        rt.state.settings.apprentice.watched_folders,
        std::slice::from_ref(&folder)
    );
    assert_eq!(rt.state.saved, saved + 1, "saved through the settings");
    assert!(texts(&rt).iter().any(|t| t == "~/Downloads"));

    // The same folder again is not watched twice.
    noticing::chosen(&mut rt.state, Ok(Some(folder.clone())));
    assert_eq!(rt.state.settings.apprentice.watched_folders, [folder]);
    assert_eq!(rt.state.saved, saved + 1);
}

#[test]
fn a_folder_answer_after_the_lab_closes_changes_nothing() {
    let mut rt = runtime("mac-skills-noticing-empty");
    rt.state.settings.labs.notice_patterns = false;
    let saved = rt.state.saved;
    noticing::chosen(&mut rt.state, Ok(Some(home("Downloads"))));
    assert!(rt.state.settings.apprentice.watched_folders.is_empty());
    assert_eq!(rt.state.saved, saved);
}

#[test]
fn a_relative_folder_asks_for_a_full_path() {
    let mut rt = runtime("mac-skills-noticing");
    rt.state.settings.apprentice.watched_folders = vec![PathBuf::from("Downloads")];
    refresh(&mut rt);
    assert!(shows(&rt, "Use a full path"));
    assert!(rt.find("open-settings-0").is_none());
}

#[test]
fn removing_a_folder_saves_the_setting() {
    let mut rt = runtime("mac-skills-noticing");
    let saved = rt.state.saved;
    tap(&mut rt, "remove-folder-0");
    assert_eq!(
        rt.state.settings.apprentice.watched_folders,
        [home("Documents/Scans")]
    );
    assert_eq!(rt.state.saved, saved + 1);
    assert!(!texts(&rt).iter().any(|t| t == "~/Downloads"));
    // Its routine goes with it.
    assert!(rt.find("routines").is_none());
}

#[test]
fn show_me_waits_like_watch_this() {
    // Annotate off: no lesson can record, so nothing is accepted and the
    // pane says why.
    let mut rt = runtime("mac-skills-noticing");
    rt.state.settings.capture.enabled = false;
    refresh(&mut rt);
    tap(&mut rt, "show-me");
    assert!(recorded(&rt).noticing.is_empty(), "nothing accepted");
    assert!(
        rt.state
            .skills
            .error
            .as_deref()
            .is_some_and(|error| error.starts_with("Apprentice couldn't start the lesson")),
        "{:?}",
        rt.state.skills.error
    );
    assert!(shows(&rt, rt.state.skills.error.clone().unwrap().as_str()));

    // While a capture records, the same.
    let mut rt = runtime("mac-skills-noticing");
    rt.state.skills.recording = true;
    refresh(&mut rt);
    tap(&mut rt, "show-me");
    assert!(recorded(&rt).noticing.is_empty());
    assert!(rt.state.skills.error.is_some());
}

#[test]
fn a_lesson_show_me_could_not_start_is_shown() {
    let mut rt = runtime("mac-skills-noticing");
    noticing::refused(&mut rt.state, "The capture engine isn't running");
    refresh(&mut rt);
    assert!(
        shows(
            &rt,
            "Apprentice couldn't start the lesson: The capture engine isn't running"
        ),
        "{:?}",
        texts(&rt)
    );
}

#[test]
fn forget_everything_ends_in_an_ellipsis() {
    let rt = runtime("mac-skills-noticing");
    assert!(button_named(&rt, "Forget everything…"));
}
