//! The Skills pane (Labs › Watch and learn, macOS): Watch this records a
//! lesson, Draft skill turns one into a draft with the model the person
//! chose, the draft is read, answered, and approved, and an approved skill's
//! saved plan can be run in the app. Every model run, download, approval,
//! saved edit, run, dry run, and stop is its own click; nothing runs before
//! it. The pane reads the captures folder and the skills store only while it
//! shows, off the window's thread, and runs drafts and skills on threads of
//! their own.

mod approval;
mod draft_view;
mod fixtures;
mod lesson_cards;
mod model_picker;
pub(super) mod noticing;
pub(crate) mod routine_offer;
mod runs;
#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use rune::{El, Icon, TextEdit, row, text};

use super::Ui;
use super::controls::{ButtonKind, DESCRIPTION, METRICS, Paint, button_enabled, page};
use crate::apprentice::evidence::{self, Limits, Thin};
use crate::apprentice::execution::Progress as RunProgress;
use crate::apprentice::lessons::{self, Lesson, Media};
use crate::apprentice::run::{self, Progress};
use crate::apprentice::skill::Skill;
use crate::apprentice::store::{Draft, Store};
use crate::capture_host::{CaptureCommand, CaptureTarget};
use crate::desktop::i18n::{tr, tr_fill};
pub(super) use fixtures::Fixture;

pub(super) static SKILLS_ICON: Icon = Icon {
    name: "apprentice-skills",
    view: 24.0,
    body: r#"<path d="M2.5 12s3.5-6.5 9.5-6.5 9.5 6.5 9.5 6.5-3.5 6.5-9.5 6.5S2.5 12 2.5 12z" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linejoin="round"/><circle cx="12" cy="12" r="2.75" fill="none" stroke="currentColor" stroke-width="1.75"/>"#,
};

/// How often the shown pane reads the folders again.
const REREAD: Duration = Duration::from_secs(3);

/// What a scene's pane did instead of doing it.
#[derive(Debug, Default)]
pub(super) struct Recorded {
    pub(super) commands: Vec<CaptureCommand>,
    /// Each draft started: the lesson and the model.
    pub(super) drafts: Vec<(String, crate::app_settings::ModelChoice)>,
    /// Each download started.
    pub(super) pulls: Vec<String>,
    /// Each answer the Noticing section gave.
    pub(super) noticing: Vec<noticing::Asked>,
    /// Each run, dry run, or stop a scene's skill card asked for.
    pub(super) runs: Vec<RunCommand>,
}

/// A lesson's evidence as the pane needs it: what it is thin on, and the
/// text a draft keeps.
#[derive(Clone, Debug)]
pub(super) struct Notes {
    pub(super) thin: Thin,
    pub(super) text: String,
}

/// What a scene's skill card did instead of acting on the Mac.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum RunCommand {
    Run(String),
    Dry(String),
    Stop(String),
}

/// The stage of one skill run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum RunStage {
    Starting,
    Step { n: usize, of: usize },
    Stopping,
}

/// A skill run following its progress on a thread of its own. `None` in a
/// scene, which records the click instead.
pub(super) struct Running {
    pub(super) slug: String,
    pub(super) stage: RunStage,
    rx: Option<Receiver<RunProgress>>,
    cancel: Arc<AtomicBool>,
}

/// A typed run outcome, shown on the skill's card until the next run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum RunOutcome {
    Dry { of: usize },
    Done,
    Failed(String),
    Cancelled,
}

/// A draft running for a lesson.
pub(super) struct Drafting {
    pub(super) lesson: String,
    pub(super) stage: Stage,
    /// `None` in scenes, which run nothing.
    rx: Option<Receiver<Progress>>,
    cancel: Arc<AtomicBool>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum Stage {
    Starting,
    Part {
        n: usize,
        of: usize,
    },
    Merging,
    /// Cancel was pressed; the call under way ends first.
    Stopping,
}

/// Why the last draft of a lesson stopped.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum Failure {
    Message(String),
    /// The model answered, but not with a draft: its raw answer.
    Unparsed(String),
}

/// A destructive choice waiting for its second click.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum Armed {
    Lesson(String),
    Skill(String),
}

/// What one read of the folders found.
struct Snapshot {
    generation: u64,
    lessons: Vec<Lesson>,
    drafts: Vec<Draft>,
    skills: Vec<Skill>,
    notes: Vec<(String, Option<Notes>)>,
}

#[derive(Default)]
pub(super) struct SkillsPane {
    /// A scene's pane: what it would do is recorded, and it never reads.
    pub(super) fixture: Option<Recorded>,
    /// The skills store, opened at the first read; a test's own folder.
    pub(super) store: Option<Store>,
    pub(super) lessons: Vec<Lesson>,
    /// Each lesson's notes once read; `None` when they can't be.
    pub(super) notes: HashMap<String, Option<Notes>>,
    pub(super) drafts: Vec<Draft>,
    pub(super) skills: Vec<Skill>,
    /// One skill running in the app, followed until it ends.
    pub(super) running: Option<Running>,
    /// The last typed outcome of each skill, by slug.
    pub(super) outcomes: HashMap<String, RunOutcome>,
    /// The skill whose Run click is waiting for confirmation.
    pub(super) run_confirm: Option<String>,
    /// A capture records now, so Watch this waits.
    pub(super) recording: bool,
    pub(super) drafting: Option<Drafting>,
    /// The lesson whose last draft stopped, and why.
    pub(super) failed: Option<(String, Failure)>,
    pub(super) editor: Option<draft_view::Editor>,
    pub(super) picker: model_picker::Picker,
    pub(super) noticing: noticing::Noticing,
    /// The skill whose plan and controls show.
    pub(super) expanded: Option<String>,
    pub(super) armed: Option<Armed>,
    /// The last thing the pane couldn't do, for the person.
    pub(super) error: Option<String>,
    /// Stands in for a field whose editor just closed.
    scratch: TextEdit,
    reading: Option<Receiver<Snapshot>>,
    read_at: Option<Instant>,
    /// Bumped by every change the pane makes, so an older read is dropped.
    generation: u64,
    /// A test's stand-in for a skills folder Apprentice can't open.
    #[cfg(test)]
    pub(super) refuse_store: bool,
}

impl SkillsPane {
    /// Follows a running draft and download always (a finished draft is
    /// saved even while the pane is hidden); reads the folders, the capture
    /// state, and (for a chosen Ollama model) Ollama's models only while
    /// the pane shows.
    pub(super) fn poll(
        &mut self,
        visible: bool,
        settings: &crate::app_settings::ApprenticeSettings,
    ) -> bool {
        let mut changed = self.poll_drafting();
        changed |= self.poll_running();
        changed |= self.picker.poll();
        if !visible || self.fixture.is_some() {
            return changed;
        }
        if let Some(crate::app_settings::ModelChoice::Ollama { .. }) = settings.choice() {
            self.picker.list_once(&settings.ollama_url);
        }
        let recording = crate::capture_host::snapshot().recording.is_some();
        changed |= std::mem::replace(&mut self.recording, recording) != recording;
        changed | self.poll_read()
    }

    fn poll_read(&mut self) -> bool {
        let mut changed = false;
        if let Some(reading) = &self.reading {
            match reading.try_recv() {
                Ok(snapshot) => {
                    self.reading = None;
                    if snapshot.generation == self.generation {
                        self.apply(snapshot);
                        changed = true;
                    } else {
                        self.read_at = None;
                    }
                }
                Err(TryRecvError::Empty) => return false,
                Err(TryRecvError::Disconnected) => self.reading = None,
            }
        }
        if self.reading.is_none() && self.read_at.is_none_or(|at| at.elapsed() >= REREAD) {
            self.start_read();
        }
        changed
    }

    fn start_read(&mut self) {
        if self.store.is_none() {
            self.store = self.open_store();
        }
        let captures = crate::app_settings::capture_settings().captures_dir();
        let known: HashSet<String> = self.notes.keys().cloned().collect();
        let generation = self.generation;
        let (send, reading) = mpsc::sync_channel(1);
        let spawned = std::thread::Builder::new()
            .name("apprentice-skills-read".into())
            .spawn(move || {
                let _ = send.send(read(captures, known, generation));
            });
        match spawned {
            Ok(_) => self.reading = Some(reading),
            Err(error) => tracing::warn!(%error, "could not read the skills"),
        }
        self.read_at = Some(Instant::now());
    }

    fn apply(&mut self, snapshot: Snapshot) {
        self.lessons = snapshot.lessons;
        self.drafts = snapshot.drafts;
        self.skills = snapshot.skills;
        self.notes.extend(snapshot.notes);
    }

    /// Makes sure changes have somewhere to go: the store, opened again if
    /// the last try failed. `Ok(false)` for a scene's pane without one,
    /// whose changes are shown only; in the app, `Err` with what to tell the
    /// person, and nothing may change.
    pub(super) fn ready_store(&mut self) -> Result<bool, String> {
        if self.store.is_some() {
            return Ok(true);
        }
        if self.fixture.is_some() {
            return Ok(false);
        }
        self.store = self.open_store();
        match self.store {
            Some(_) => Ok(true),
            None => Err(tr("Apprentice can't open its skills folder").to_owned()),
        }
    }

    fn open_store(&self) -> Option<Store> {
        #[cfg(test)]
        if self.refuse_store {
            return None;
        }
        Store::open().ok()
    }

    /// Marks the shown lists as newer than any read under way.
    fn changed(&mut self) {
        self.generation += 1;
    }

    /// Reads the running draft's progress; a finished draft is saved and
    /// opens in the draft view.
    fn poll_drafting(&mut self) -> bool {
        let Some(drafting) = &mut self.drafting else {
            return false;
        };
        let Some(rx) = &drafting.rx else {
            return false;
        };
        let mut changed = false;
        let ended = loop {
            match rx.try_recv() {
                Ok(Progress::Segment { n, of }) => {
                    if drafting.stage != Stage::Stopping {
                        drafting.stage = Stage::Part { n, of };
                    }
                    changed = true;
                }
                Ok(Progress::Merging) => {
                    if drafting.stage != Stage::Stopping {
                        drafting.stage = Stage::Merging;
                    }
                    changed = true;
                }
                Ok(Progress::Done(skill)) => break Ok(*skill),
                Ok(Progress::Failed(message)) => break Err(Failure::Message(message)),
                Ok(Progress::Unparsed(error)) => {
                    tracing::info!(reason = %error.reason, "a draft's answer couldn't be read");
                    break Err(Failure::Unparsed(error.raw));
                }
                Err(TryRecvError::Empty) => return changed,
                // The drafting thread ended without a last word.
                Err(TryRecvError::Disconnected) => {
                    break Err(Failure::Message(
                        tr("Drafting stopped unexpectedly").to_owned(),
                    ));
                }
            }
        };
        let cancelled = drafting.cancel.load(Ordering::Acquire);
        let lesson = drafting.lesson.clone();
        self.drafting = None;
        match ended {
            Ok(skill) => self.drafted(lesson, skill),
            // A cancelled draft just returns to its lesson.
            Err(_) if cancelled => {}
            Err(failure) => self.failed = Some((lesson, failure)),
        }
        true
    }

    /// Follows a skill run to its typed last word. Like a draft, a run is
    /// followed even while the pane is hidden: stopping it must still work.
    fn poll_running(&mut self) -> bool {
        let Some(running) = &mut self.running else {
            return false;
        };
        let Some(rx) = &running.rx else {
            return false;
        };
        let mut changed = false;
        let outcome = loop {
            match rx.try_recv() {
                Ok(RunProgress::Step { n, of }) => {
                    if running.stage != RunStage::Stopping {
                        running.stage = RunStage::Step { n, of };
                    }
                    changed = true;
                }
                Ok(RunProgress::Done) => break RunOutcome::Done,
                Ok(RunProgress::Failed(message)) => break RunOutcome::Failed(message),
                Ok(RunProgress::Cancelled) => break RunOutcome::Cancelled,
                Err(TryRecvError::Empty) => return changed,
                // The run thread ended without a last word.
                Err(TryRecvError::Disconnected) => {
                    break RunOutcome::Failed(tr("Running stopped unexpectedly").to_owned());
                }
            }
        };
        let slug = running.slug.clone();
        self.running = None;
        self.run_confirm = None;
        self.outcomes.insert(slug, outcome);
        true
    }

    /// Saves a finished draft with the lesson's text and opens it.
    fn drafted(&mut self, lesson: String, skill: Skill) {
        let text = match self.notes.get(&lesson) {
            Some(Some(notes)) => notes.text.clone(),
            _ => self
                .lessons
                .iter()
                .find(|l| l.id == lesson)
                .and_then(notes_of)
                .map(|notes| notes.text)
                .unwrap_or_default(),
        };
        if let Err(message) = self.ready_store() {
            self.failed = Some((lesson, Failure::Message(message)));
            return;
        }
        let now = crate::history::now_ms();
        let id = match &self.store {
            Some(store) => store.new_draft_id(now),
            None => format!("{now}-0000"),
        };
        let draft = Draft {
            id,
            lesson_id: lesson.clone(),
            skill,
            evidence_text: text,
        };
        if let Some(store) = &self.store
            && let Err(error) = store.save_draft(&draft)
        {
            let message = tr_fill("Couldn't save the draft: {}", &error.to_string());
            self.failed = Some((lesson, Failure::Message(message)));
            return;
        }
        // An open draft or skill keeps its typing: the new draft waits
        // under Drafts waiting instead.
        if self.editor.is_none() {
            self.editor = Some(draft_view::Editor::for_draft(&draft));
        }
        self.drafts.insert(0, draft);
        self.changed();
    }

    /// A lesson's notes, while it is ready to read.
    fn notes(&self, lesson: &str) -> Option<&Notes> {
        self.notes.get(lesson).and_then(Option::as_ref)
    }

    /// Lessons not drafted yet, newest first.
    fn new_lessons(&self) -> impl Iterator<Item = &Lesson> {
        self.lessons.iter().filter(|lesson| {
            !self.drafts.iter().any(|d| d.lesson_id == lesson.id)
                && !self.skills.iter().any(|s| s.lesson_id == lesson.id)
        })
    }
}

/// One read of the captures folder and the store, on the reading thread.
fn read(captures: PathBuf, known: HashSet<String>, generation: u64) -> Snapshot {
    let lessons = lessons::list(&captures);
    let store = Store::open().ok();
    let drafts = store.as_ref().map(Store::drafts).unwrap_or_default();
    let skills = store.as_ref().map(Store::skills).unwrap_or_default();
    let notes = lessons
        .iter()
        .filter(|lesson| lesson.media != Media::Expired && !known.contains(&lesson.id))
        .map(|lesson| (lesson.id.clone(), notes_of(lesson)))
        .collect();
    Snapshot {
        generation,
        lessons,
        drafts,
        skills,
        notes,
    }
}

/// The lesson's text and what it is thin on; frames are never decoded.
fn notes_of(lesson: &Lesson) -> Option<Notes> {
    let evidence = evidence::build(lesson, &Limits::for_text(0)).ok()?;
    let text = evidence
        .segments
        .iter()
        .map(|segment| segment.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    Some(Notes {
        thin: evidence.thin,
        text,
    })
}

/// A Secondary button inside a row, at the rows' 24 pt, that keeps its
/// place while it can't be pressed.
pub(super) fn small(
    p: Paint,
    key: &str,
    label: &str,
    enabled: bool,
    press: impl Fn(&mut Ui, &mut rune::Tx) + 'static,
) -> El {
    use super::controls::PushSize;
    rune_kit::Button::new(label)
        .variant(rune_kit::ButtonVariant::Neutral)
        .disabled(!enabled)
        .on_press_tx(press)
        .build_small_push(p.k)
        .key(key)
}

/// Whether the pane may be shown or changed: the saved lab is on, this build
/// has it, and no managed lock holds it off. Scenes keep their fixture visible
/// without depending on the process-wide gate. The sidebar, window
/// destination, and every mutating handler share this one predicate.
pub(super) fn allowed(ui: &Ui) -> bool {
    ui.settings.labs.watch_and_learn
        && (ui.skills.fixture.is_some()
            || (crate::labs::watch_and_learn_enabled(&ui.settings.labs)
                && crate::labs::Feature::WatchAndLearn.on_this_os()))
}

/// What the screen's one primary button is: Watch this, unless a draft is
/// open or a lesson waits to be drafted.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum Primary {
    Watch,
    Draft(String),
    Approve,
}

pub(super) fn primary(ui: &Ui) -> Primary {
    let pane = &ui.skills;
    if pane.editor.is_some() {
        return Primary::Approve;
    }
    let ready = pane.new_lessons().find(|lesson| {
        lesson.media != Media::Expired
            && pane.drafting.as_ref().is_none_or(|d| d.lesson != lesson.id)
    });
    match ready {
        Some(lesson) if pane.drafting.is_none() => Primary::Draft(lesson.id.clone()),
        _ => Primary::Watch,
    }
}

/// Watch this: a lesson of the whole display under the pointer. Nothing
/// starts while the lab is off, Annotate is off, or a capture records.
pub(super) fn watch(ui: &mut Ui) {
    if !allowed(ui) || !ui.settings.capture.enabled || ui.skills.recording {
        return;
    }
    if let Some(recorded) = &mut ui.skills.fixture {
        recorded
            .commands
            .push(CaptureCommand::Lesson(CaptureTarget::Display {
                at: [0.0; 2],
            }));
        return;
    }
    match lessons::start() {
        Ok(()) => {
            ui.skills.error = None;
            ui.skills.recording = true;
        }
        Err(error) => {
            ui.skills.error = Some(tr_fill(
                "Apprentice couldn't start the lesson: {}",
                &error.to_string(),
            ));
        }
    }
}

/// Draft skill: the one approval for one run of the chosen model over this
/// lesson. With no model chosen, the picker opens instead.
pub(super) fn draft(ui: &mut Ui, lesson: &str) {
    if !allowed(ui) || ui.skills.drafting.is_some() {
        return;
    }
    let Some(lesson) = ui.skills.lessons.iter().find(|l| l.id == lesson).cloned() else {
        return;
    };
    if lesson.media == Media::Expired {
        return;
    }
    let Some(choice) = ui.settings.apprentice.choice() else {
        model_picker::open(ui);
        return;
    };
    let pane = &mut ui.skills;
    pane.failed = None;
    pane.armed = None;
    let cancel = Arc::new(AtomicBool::new(false));
    let rx = match &mut pane.fixture {
        Some(recorded) => {
            recorded.drafts.push((lesson.id.clone(), choice));
            None
        }
        None => Some(run::draft(
            &lesson,
            &choice,
            &ui.settings.apprentice.ollama_url,
            cancel.clone(),
        )),
    };
    pane.drafting = Some(Drafting {
        lesson: lesson.id,
        stage: Stage::Starting,
        rx,
        cancel,
    });
}

/// Cancel: the draft stops before its next call.
pub(super) fn cancel_draft(ui: &mut Ui) {
    if let Some(drafting) = &mut ui.skills.drafting {
        drafting.cancel.store(true, Ordering::Release);
        drafting.stage = Stage::Stopping;
        if drafting.rx.is_none() {
            ui.skills.drafting = None;
        }
    }
}

/// Discard on a lesson: the first click asks, the second moves its bundle
/// to the Trash.
pub(super) fn discard_lesson(ui: &mut Ui, id: &str) {
    let pane = &mut ui.skills;
    if pane.armed.as_ref() != Some(&Armed::Lesson(id.to_owned())) {
        pane.armed = Some(Armed::Lesson(id.to_owned()));
        return;
    }
    pane.armed = None;
    let Some(at) = pane.lessons.iter().position(|l| l.id == id) else {
        return;
    };
    let lesson = pane.lessons.remove(at);
    if pane.failed.as_ref().is_some_and(|(failed, _)| failed == id) {
        pane.failed = None;
    }
    pane.changed();
    if pane.fixture.is_none() {
        super::screenshot_thumbnail::files::trash(lesson.bundle);
    }
}

/// The pane: Watch this and its note in the header; the model, new lessons,
/// the draft, and the skills below.
pub(super) fn pane(ui: &Ui, p: Paint) -> (Vec<El>, El) {
    let pane = &ui.skills;
    let primary = primary(ui);
    let note = if !ui.settings.capture.enabled {
        tr("Turn on Annotate to record a lesson")
    } else if pane.recording {
        tr("Recording… stop it from the capsule")
    } else {
        tr("Records your whole screen and your voice")
    };
    let watch = button_enabled(
        p,
        "watch-this",
        tr("Watch this"),
        if primary == Primary::Watch {
            ButtonKind::Primary
        } else {
            ButtonKind::Secondary
        },
        ui.settings.capture.enabled && !pane.recording,
        |ui: &mut Ui, _| watch(ui),
    )
    .description(note);
    let actions = vec![
        text(note)
            .key("watch-note")
            .font(DESCRIPTION)
            .color(p.muted())
            .a11y_hidden(),
        watch,
    ];
    let mut content = page("content").pt(8.0).pb(32.0);
    if let Some(error) = &pane.error {
        content = content.child(
            row().key("pane-error").px(METRICS.row_px).child(
                text(error.as_str())
                    .key("copy")
                    .font(DESCRIPTION)
                    .color(p.danger())
                    .wrap()
                    .grow(1.0),
            ),
        );
    }
    content = content.child(model_picker::section(ui, p));
    if let Some(noticing) = noticing::section(ui, p) {
        content = content.child(noticing);
    }
    if let Some(lessons) = lesson_cards::section(ui, p, &primary) {
        content = content.child(lessons);
    }
    if let Some(draft) = draft_view::section(ui, p) {
        content = content.child(draft);
    }
    if let Some(drafts) = draft_view::other_drafts(ui, p) {
        content = content.child(drafts);
    }
    if let Some(skills) = runs::section(ui, p) {
        content = content.child(skills);
    } else if pane.new_lessons().next().is_none()
        && pane.drafts.is_empty()
        && pane.editor.is_none()
        && !noticing::lists_routines(ui)
    {
        content = content.child(
            super::list_detail::empty_state(
                p,
                "skills-empty",
                &SKILLS_ICON,
                tr("No skills yet."),
                Some(tr(
                    "Press Watch this, do the task once, and say what you're doing.",
                )),
            )
            .pt(40.0),
        );
    }
    (actions, super::scroll_body(p.k, "scroll", content))
}
