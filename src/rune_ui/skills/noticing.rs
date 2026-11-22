//! The Skills pane's Noticing section (Labs › Notice patterns, macOS): the
//! folders Apprentice watches, each with Remove (and, for one macOS refuses,
//! Open Settings), Add folder…, the routines it found with Show me and
//! Forget, and Forget everything behind a second click. Folders are saved
//! in `apprentice.watched_folders` like every setting; the watcher reads
//! them again on its next tick. What was noticed is read, and every answer
//! runs, on a thread of its own: they take the notice store's lock and may
//! sync to disk.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};

use rune::{El, col, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize};

use super::super::Ui;
use super::super::controls::{
    self, DESCRIPTION, METRICS, Paint, PushSize, described_row, folder_label, setting_row,
};
use super::super::lists::small_button;
use super::{allowed, small};
use crate::apprentice::notice::state::Known;
use crate::apprentice::notice::watch::{self, FolderHealth, NoticeStatus};
use crate::desktop::file_dialog::{self, Choice, Dialog, Pending, Target};
use crate::desktop::i18n::{tr, tr_fill, tr_named};

/// Files and Folders in System Settings › Privacy & Security.
const FILES_AND_FOLDERS: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_FilesAndFolders";

/// Forget everything's Cancel is at least this wide, so it covers the middle
/// of the Forget everything it replaces, where a double click lands.
const CANCEL_WIDTH: f32 = 88.0;

/// What a scene's section would have done.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::rune_ui) enum Asked {
    ChooseFolder,
    OpenSettings,
    ShowMe(String),
    Forget(String),
    ForgetEverything,
}

#[derive(Default)]
pub(in crate::rune_ui) struct Noticing {
    /// What Apprentice noticed, once read.
    pub(in crate::rune_ui) status: Option<NoticeStatus>,
    /// The Add folder… dialog, while it is open.
    pub(in crate::rune_ui) folder_choice: Option<Pending>,
    /// Forget everything waits for its second click.
    pub(in crate::rune_ui) confirm_forget: bool,
    /// Show me is waiting for the listener to confirm the lesson started.
    lesson_pending: bool,
    /// What was noticed, after an answer and why the answer failed, if it
    /// did.
    reading: Option<Receiver<(NoticeStatus, Option<String>)>>,
    /// [`watch::changed_generation`] at the last read; `None` reads again
    /// when the pane next shows.
    seen: Option<u64>,
}

impl Noticing {
    /// Runs `act` and then reads what was noticed, on a thread of its own;
    /// a read under way is dropped for it. `act` returns why it failed.
    fn run(&mut self, act: impl FnOnce() -> Option<String> + Send + 'static) {
        let (send, reading) = mpsc::sync_channel(1);
        let spawned = std::thread::Builder::new()
            .name("apprentice-noticing".into())
            .spawn(move || {
                let failed = act();
                let _ = send.send((watch::status(), failed));
            });
        match spawned {
            Ok(_) => self.reading = Some(reading),
            Err(error) => tracing::warn!(%error, "could not read what Apprentice noticed"),
        }
    }
}

/// Whether the section shows and may act: Notice patterns is on, and so is
/// Watch and learn, whose lesson Show me starts.
fn open(ui: &Ui) -> bool {
    ui.settings.labs.notice_patterns
        && allowed(ui)
        && (ui.skills.fixture.is_some() || crate::labs::notice_patterns())
}

/// Reads what was noticed when the pane opens and whenever the watcher says
/// it changed, and takes the folder dialog's answer.
pub(in crate::rune_ui) fn poll(ui: &mut Ui, visible: bool) -> bool {
    let mut changed = false;
    if !visible {
        ui.skills.noticing.seen = None;
        ui.skills.noticing.confirm_forget = false;
        ui.skills.noticing.folder_choice = None;
        return changed;
    }
    // An answer that lands while the section is hidden or the lab is off
    // must not change the watched folders.
    if !open(ui) {
        ui.skills.noticing.folder_choice = None;
        return changed;
    }
    if ui.skills.fixture.is_some() {
        return changed;
    }
    changed |= poll_chooser(ui);
    let received = {
        let noticing = &mut ui.skills.noticing;
        if let Some(reading) = &noticing.reading {
            match reading.try_recv() {
                Ok((status, failed)) => {
                    noticing.status = Some(status);
                    noticing.reading = None;
                    Some((
                        failed,
                        std::mem::replace(&mut noticing.lesson_pending, false),
                    ))
                }
                Err(TryRecvError::Empty) => return changed,
                Err(TryRecvError::Disconnected) => {
                    noticing.reading = None;
                    None
                }
            }
        } else {
            None
        }
    };
    if let Some((failed, lesson)) = received {
        if let Some(error) = failed {
            if lesson {
                ui.skills.recording = false;
            }
            refused(ui, &error);
            return true;
        }
        if lesson {
            // The listener confirmed the lesson; don't wait for the capture
            // snapshot's next poll to show it.
            ui.skills.recording = true;
        }
        changed = true;
    }
    let noticing = &mut ui.skills.noticing;
    let generation = watch::changed_generation();
    if noticing.seen != Some(generation) {
        noticing.seen = Some(generation);
        noticing.run(|| None);
    }
    changed
}

fn poll_chooser(ui: &mut Ui) -> bool {
    let Some(choice) = ui
        .skills
        .noticing
        .folder_choice
        .as_ref()
        .and_then(Pending::poll)
    else {
        return false;
    };
    ui.skills.noticing.folder_choice = None;
    chosen(ui, choice)
}

/// Add folder…: the folder dialog, starting at the home folder.
fn add_folder(ui: &mut Ui) {
    if !open(ui) || ui.skills.noticing.folder_choice.is_some() {
        return;
    }
    if let Some(recorded) = &mut ui.skills.fixture {
        recorded.noticing.push(Asked::ChooseFolder);
        return;
    }
    let waker = ui.modes.waker.clone();
    ui.skills.noticing.folder_choice = Some(file_dialog::open(
        Dialog {
            title: tr("Folder to watch").into(),
            target: Target::Folder,
            directory: crate::app_paths::home_dir(),
        },
        move || {
            if let Some(waker) = &waker {
                waker();
            }
        },
    ));
}

/// The folder dialog's answer: a folder not watched yet is saved; a cancel
/// changes nothing. True when the section shows something new.
pub(super) fn chosen(ui: &mut Ui, choice: Choice) -> bool {
    if !open(ui) {
        return false;
    }
    match choice {
        Ok(Some(folder)) => {
            if ui.settings.apprentice.watched_folders.contains(&folder) {
                return false;
            }
            if ui.update(|settings| settings.apprentice.watched_folders.push(folder)) {
                reread(ui);
            }
            true
        }
        Ok(None) => false,
        Err(error) => {
            ui.skills.error = Some(tr_fill("Couldn't choose a folder: {}", &error));
            true
        }
    }
}

/// Remove: the folder is no longer watched. Its routines leave the list at
/// once, and the watcher drops its noticed events at its next read.
fn remove(ui: &mut Ui, folder: &Path) {
    if !open(ui) {
        return;
    }
    if ui.update(|settings| settings.apprentice.watched_folders.retain(|f| f != folder)) {
        reread(ui);
    }
}

/// Reads again after a settings change, outside scenes.
fn reread(ui: &mut Ui) {
    if ui.skills.fixture.is_none() {
        ui.skills.noticing.run(|| None);
    }
}

fn open_settings(ui: &mut Ui) {
    if let Some(recorded) = &mut ui.skills.fixture {
        recorded.noticing.push(Asked::OpenSettings);
        return;
    }
    if let Err(error) = crate::desktop::opencode_catalog::open_url(FILES_AND_FOLDERS) {
        tracing::warn!(%error, "could not open Files and Folders settings");
    }
}

/// Show me: a lesson starts (it waits for a model, as every lesson does),
/// and then the routine is offered no more. Like Watch this, nothing starts
/// while Annotate is off or a capture records; the pane says why.
fn show_me(ui: &mut Ui, id: &str) {
    if !open(ui) {
        return;
    }
    if !ui.settings.capture.enabled {
        refused(ui, tr("Turn on Annotate to record a lesson"));
        return;
    }
    if ui.skills.recording {
        refused(ui, tr("Recording… stop it from the capsule"));
        return;
    }
    ui.skills.error = None;
    if let Some(recorded) = &mut ui.skills.fixture {
        recorded.noticing.push(Asked::ShowMe(id.to_owned()));
        return;
    }
    let id = id.to_owned();
    ui.skills.noticing.lesson_pending = true;
    ui.skills.noticing.run(move || watch::accept(&id).err());
}

/// Shows why Show me's lesson didn't start in the pane's error line.
pub(super) fn refused(ui: &mut Ui, why: &str) {
    ui.skills.error = Some(tr_fill("Apprentice couldn't start the lesson: {}", why));
}

/// Forget: the routine is never offered or listed again.
fn forget(ui: &mut Ui, id: &str) {
    if !open(ui) {
        return;
    }
    let noticing = &mut ui.skills.noticing;
    if let Some(status) = &mut noticing.status {
        status.routines.retain(|known| known.id != id);
    }
    if let Some(recorded) = &mut ui.skills.fixture {
        recorded.noticing.push(Asked::Forget(id.to_owned()));
        return;
    }
    let id = id.to_owned();
    noticing.run(move || {
        watch::ignore(&id);
        None
    });
}

/// Forget everything: the first click asks, the second empties the event
/// log and the routines. The folders stay watched.
fn forget_everything(ui: &mut Ui) {
    if !open(ui) {
        return;
    }
    let noticing = &mut ui.skills.noticing;
    if !noticing.confirm_forget {
        noticing.confirm_forget = true;
        return;
    }
    noticing.confirm_forget = false;
    if let Some(status) = &mut noticing.status {
        status.routines.clear();
    }
    if let Some(recorded) = &mut ui.skills.fixture {
        recorded.noticing.push(Asked::ForgetEverything);
        return;
    }
    noticing.run(|| {
        watch::forget_everything();
        None
    });
}

/// The routines listed for folders still watched.
fn routines(ui: &Ui) -> impl Iterator<Item = &Known> {
    let watched = &ui.settings.apprentice.watched_folders;
    ui.skills
        .noticing
        .status
        .iter()
        .flat_map(|status| &status.routines)
        .filter(|known| watched.contains(&known.root))
}

/// Whether the section lists a routine, so the pane's empty state gives way.
pub(super) fn lists_routines(ui: &Ui) -> bool {
    open(ui) && routines(ui).next().is_some()
}

/// A folder as its last read saw it; a path macOS can't resolve can't be
/// read, and one not read yet is taken as fine. Relative paths never reach
/// the watcher, so they are handled separately in the row.
fn health(ui: &Ui, folder: &Path) -> FolderHealth {
    ui.skills
        .noticing
        .status
        .iter()
        .flat_map(|status| &status.folders)
        .find(|(path, _)| path == folder)
        .map_or(FolderHealth::Ok, |(_, health)| *health)
}

/// The Noticing section, while the lab is on.
pub(super) fn section(ui: &Ui, p: Paint) -> Option<El> {
    if !open(ui) {
        return None;
    }
    let mut folders = vec![setting_row(
        p,
        "watched-folders",
        tr("Watched folders"),
        None,
        small(
            p,
            "add-folder",
            tr("Add folder…"),
            ui.skills.noticing.folder_choice.is_none(),
            |ui: &mut Ui, _| add_folder(ui),
        ),
    )];
    folders.extend(
        ui.settings
            .apprentice
            .watched_folders
            .iter()
            .enumerate()
            .map(|(index, folder)| folder_row(ui, p, index, folder)),
    );
    let mut section = col()
        .key("noticing")
        .gap(METRICS.section_gap)
        .child(controls::section(p, "folders", tr("Noticing"), folders));
    let routines: Vec<El> = routines(ui).map(|known| routine_row(p, known)).collect();
    if !routines.is_empty() {
        section = section.child(controls::section(p, "routines", tr("Routines"), routines));
    }
    Some(section.child(privacy(ui, p)))
}

fn folder_row(ui: &Ui, p: Paint, index: usize, folder: &Path) -> El {
    let relative = !folder.is_absolute();
    let unreadable = !relative && health(ui, folder) == FolderHealth::Unreadable;
    let problem = if relative {
        Some(tr("Use a full path").to_owned())
    } else if unreadable {
        Some(tr("Can't read this folder").to_owned())
    } else {
        None
    };
    let mut actions = row().key("folder-actions").gap(8.0).center_y();
    if unreadable {
        actions = actions.child(
            Button::new(tr("Open Settings"))
                .variant(ButtonVariant::Ghost)
                .control_size(ControlSize::Small)
                .on_press(|ui: &mut Ui| open_settings(ui))
                .build(p.k)
                .key(format!("open-settings-{index}")),
        );
    }
    let path: PathBuf = folder.to_path_buf();
    actions = actions.child(small_button(
        p,
        &format!("remove-folder-{index}"),
        tr("Remove"),
        move |ui: &mut Ui, _| remove(ui, &path),
    ));
    described_row(
        p,
        &format!("folder-{index}"),
        &folder_label(folder),
        problem.map(|copy| (copy, if relative { p.muted() } else { p.danger() })),
        actions,
    )
}

/// A routine: its sentence and how many days it was seen, Show me, and
/// Forget.
fn routine_row(p: Paint, known: &Known) -> El {
    let line = format!(
        "{} {}",
        known.description,
        tr_named("({days} days)", &[("days", &known.days.to_string())])
    );
    let show = known.id.clone();
    let drop = known.id.clone();
    // The row is named by its sentence, so the text stays out of the tree.
    row()
        .key(format!("routine-{}", known.id))
        .min_h(METRICS.row_h)
        .px(METRICS.row_px)
        .py(8.0)
        .gap(24.0)
        .center_y()
        .radius(p.k.radius.md)
        .label(&line)
        .child(
            text(line.clone())
                .key("label")
                .font(p.k.ty.ui)
                .color(p.text())
                .wrap()
                .grow(1.0)
                .min_w(0.0)
                .a11y_hidden(),
        )
        .child(
            row()
                .key("routine-actions")
                .gap(8.0)
                .center_y()
                .child(small_button(
                    p,
                    "show-me",
                    tr("Show me"),
                    move |ui: &mut Ui, _| show_me(ui, &show),
                ))
                .child(small_button(
                    p,
                    "forget",
                    tr("Forget"),
                    move |ui: &mut Ui, _| forget(ui, &drop),
                )),
        )
}

/// What is kept, and Forget everything or its confirmation. Cancel takes
/// Forget everything's place, so a double click never confirms.
fn privacy(ui: &Ui, p: Paint) -> El {
    let mut actions = row().key("forget-actions").gap(8.0).center_y();
    if ui.skills.noticing.confirm_forget {
        actions = actions
            .child(
                Button::new(tr("Forget everything"))
                    .variant(ButtonVariant::Danger)
                    .on_press_tx(|ui: &mut Ui, _| forget_everything(ui))
                    .build_small_push(p.k)
                    .key("confirm-forget-everything"),
            )
            .child(
                small_button(
                    p,
                    "cancel-forget-everything",
                    tr("Cancel"),
                    |ui: &mut Ui, _| ui.skills.noticing.confirm_forget = false,
                )
                .min_w(CANCEL_WIDTH),
            );
    } else {
        actions = actions.child(small_button(
            p,
            "forget-everything",
            tr("Forget everything…"),
            |ui: &mut Ui, _| forget_everything(ui),
        ));
    }
    row()
        .key("noticing-privacy")
        .px(METRICS.row_px)
        .gap(24.0)
        .center_y()
        .child(
            text(tr(
                "Apprentice keeps file kinds, folder names and times for 14 days, on this Mac.",
            ))
            .key("copy")
            .font(DESCRIPTION)
            .color(p.muted())
            .wrap()
            .grow(1.0)
            .min_w(0.0),
        )
        .child(actions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_settings::AppSettings;
    use crate::rune_ui::Service;

    fn empty_status() -> NoticeStatus {
        NoticeStatus {
            folders: Vec::new(),
            routines: Vec::new(),
        }
    }

    fn answered(failed: Option<String>) -> Ui {
        let mut ui = Ui::with(AppSettings::default(), Service::Fixture(Vec::new()));
        ui.settings.labs.watch_and_learn = true;
        ui.settings.labs.notice_patterns = true;
        crate::labs::apply(&ui.settings.labs);
        let (send, reading) = mpsc::sync_channel(1);
        send.send((empty_status(), failed)).unwrap();
        ui.skills.noticing.reading = Some(reading);
        ui.skills.noticing.lesson_pending = true;
        ui
    }

    #[test]
    fn a_confirmed_lesson_marks_recording_without_waiting_for_the_capture() {
        let mut ui = answered(None);
        assert!(poll(&mut ui, true));
        assert!(ui.skills.recording);
        assert!(ui.skills.error.is_none());
        crate::labs::apply(&AppSettings::default().labs);
    }

    #[test]
    fn a_refused_lesson_leaves_recording_off_and_says_why() {
        let mut ui = answered(Some("no capture engine".into()));
        assert!(poll(&mut ui, true));
        assert!(!ui.skills.recording);
        assert!(ui.skills.error.is_some());
        crate::labs::apply(&AppSettings::default().labs);
    }
}
