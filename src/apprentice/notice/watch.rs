//! The `apprentice-notice` thread: every [`POLL`] while Labs › Notice
//! patterns is open and a folder is watched, it reads each watched folder,
//! keeps what changed in the event log, offers a known routine when its
//! trigger arrives, and looks for routines once an episode goes quiet.
//!
//! The thread and the pane's actions ([`accept`], [`ignore`],
//! [`forget_everything`]) take one process-wide lock around `routines.json`
//! and the event log; [`status`] reads both from disk and folder health from
//! what the thread last saw.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::SyncSender;
use std::sync::{Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime};

use super::log::{self, Event, Log};
use super::offer::{self, Busy};
use super::routine::{self, EPISODE_GAP_MS, Routine};
use super::snapshot::{self, Change, ChangeKind, Snapshot};
use super::state::{Known, RoutineState, Routines};
use crate::apprentice::events::{self, ApprenticeAction};
use crate::desktop::i18n::tr_named;

/// How often the watched folders are read.
pub const POLL: Duration = Duration::from_secs(5);
/// How often the event log drops what is older than 14 days.
const PRUNE_EVERY_MS: u64 = 60 * 60 * 1000;
/// The folder list the log's root indexes refer to.
const ROOTS_FILE: &str = "roots.json";

/// What the thread tells the app.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NoticeEvent {
    /// Show the offer capsule for routine `id`.
    Offer {
        id: String,
        /// "New PDF in Downloads".
        title: String,
        /// "You usually file these (3 days). Show me how?"
        line: String,
    },
    /// Routines or folder health changed: the pane refreshes.
    Changed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FolderHealth {
    Ok,
    /// Missing, or macOS refuses it (no Files and Folders permission yet).
    Unreadable,
}

// Its callers, the offer capsule and the Skills pane, come later.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Debug, PartialEq)]
pub struct NoticeStatus {
    /// Every watched folder in settings, in order.
    pub folders: Vec<(PathBuf, FolderHealth)>,
    /// The routines listed for those folders.
    pub routines: Vec<Known>,
}

/// Serializes `routines.json` and the event log between the thread and the
/// pane's actions.
static STORE: Mutex<()> = Mutex::new(());
/// Folder health as the thread last read it.
static HEALTH: Mutex<Vec<(PathBuf, FolderHealth)>> = Mutex::new(Vec::new());

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|error| error.into_inner())
}

/// Starts the `apprentice-notice` thread. It never stops; while the gate is
/// closed or no folder is watched each tick does nothing. Without a data
/// folder it ends at once.
pub fn start(events: SyncSender<NoticeEvent>, wake: impl Fn() + Send + 'static) -> JoinHandle<()> {
    std::thread::Builder::new()
        .name("apprentice-notice".into())
        .spawn(move || {
            let Some(dir) = log::noticed_dir() else {
                tracing::warn!(
                    "Notice patterns can't run: Apprentice's data folder is unavailable"
                );
                return;
            };
            let mut watcher = Watcher::new(dir, Box::new(settings_folders), events, Box::new(wake));
            loop {
                watcher.tick(crate::desktop::events::now_ms());
                *lock(&HEALTH) = watcher.health.clone();
                std::thread::sleep(POLL);
            }
        })
        .expect("the apprentice-notice thread starts")
}

/// Show me: no more offers, and a lesson starts.
// Its callers, the offer capsule and the Skills pane, come later.
#[allow(dead_code)]
pub fn accept(id: &str) {
    if let Some(dir) = log::noticed_dir()
        && set_state(&dir, id, RoutineState::Shown)
    {
        events::emit(ApprenticeAction::RoutineAccepted, id, None);
    }
    if let Err(error) = crate::apprentice::lessons::start() {
        tracing::warn!(%error, "could not start a lesson for a noticed routine");
    }
}

/// Stop asking or Forget: never offered or listed again, and its events are
/// dropped.
// Its callers, the offer capsule and the Skills pane, come later.
#[allow(dead_code)]
pub fn ignore(id: &str) {
    let Some(dir) = log::noticed_dir() else {
        return;
    };
    if ignore_in(&dir, id) {
        events::emit(ApprenticeAction::RoutineIgnored, id, None);
    }
}

/// Empties the event log and the routine state.
// Its callers, the offer capsule and the Skills pane, come later.
#[allow(dead_code)]
pub fn forget_everything() {
    if let Some(dir) = log::noticed_dir() {
        forget_everything_in(&dir);
    }
}

/// The watched folders with their health, and the routines listed for them.
// Its callers, the offer capsule and the Skills pane, come later.
#[allow(dead_code)]
pub fn status() -> NoticeStatus {
    status_in(log::noticed_dir().as_deref(), &settings_folders())
}

fn status_in(dir: Option<&Path>, saved: &[PathBuf]) -> NoticeStatus {
    let health = lock(&HEALTH).clone();
    let mut folders: Vec<(PathBuf, FolderHealth)> = Vec::new();
    for folder in saved {
        if folders.iter().any(|(known, _)| known == folder) {
            continue;
        }
        let state = if folder.is_absolute() {
            health
                .iter()
                .find(|(path, _)| path == folder)
                .map_or(FolderHealth::Ok, |(_, state)| *state)
        } else {
            FolderHealth::Unreadable
        };
        folders.push((folder.clone(), state));
    }
    let routines = dir.map_or_else(Vec::new, |dir| {
        let _store = lock(&STORE);
        Routines::load(dir)
            .listed()
            .filter(|known| saved.contains(&known.root))
            .cloned()
            .collect()
    });
    NoticeStatus { folders, routines }
}

fn set_state(dir: &Path, id: &str, state: RoutineState) -> bool {
    let _store = lock(&STORE);
    let mut routines = Routines::load(dir);
    if !routines.known.iter().any(|known| known.id == id) {
        return false;
    }
    routines.set(id, state);
    if let Err(error) = routines.save() {
        tracing::warn!(%error, "could not save the noticed routines");
    }
    true
}

fn ignore_in(dir: &Path, id: &str) -> bool {
    let _store = lock(&STORE);
    let mut routines = Routines::load(dir);
    let Some(known) = routines.known.iter().find(|known| known.id == id).cloned() else {
        return false;
    };
    routines.forget(id);
    if let Err(error) = routines.save() {
        tracing::warn!(%error, "could not save the noticed routines");
    }
    if let Err(error) = forget_events(dir, &known) {
        tracing::warn!(%error, "could not forget a routine's events");
    }
    true
}

/// Drops the events of every episode of `known`'s folder whose steps are
/// its steps.
fn forget_events(dir: &Path, known: &Known) -> std::io::Result<()> {
    let Some(index) =
        read_roots(dir).and_then(|roots| roots.iter().position(|root| *root == known.root))
    else {
        return Ok(());
    };
    let log = Log::open(dir)?;
    let spans: Vec<(u64, u64)> = routine::episodes(&log.read())
        .iter()
        .filter(|episode| episode.first().is_some_and(|event| event.root == index))
        .filter(|episode| routine::signature(episode) == known.steps)
        .filter_map(|episode| {
            let times = episode.iter().map(|event| event.t);
            Some((times.clone().min()?, times.max()?))
        })
        .collect();
    if spans.is_empty() {
        return Ok(());
    }
    log.forget(Some(index), |event| {
        spans
            .iter()
            .any(|(start, end)| (*start..=*end).contains(&event.t))
    })
}

fn forget_everything_in(dir: &Path) {
    let _store = lock(&STORE);
    if let Err(error) = Log::open(dir).and_then(|log| log.clear()) {
        tracing::warn!(%error, "could not empty the noticed events");
    }
    let mut routines = Routines::load(dir);
    routines.known.clear();
    if let Err(error) = routines.save() {
        tracing::warn!(%error, "could not empty the noticed routines");
    }
}

/// The folder list the log was written against, if one was saved.
fn read_roots(dir: &Path) -> Option<Vec<PathBuf>> {
    let bytes = std::fs::read(dir.join(ROOTS_FILE)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn write_roots(dir: &Path, roots: &[PathBuf]) -> std::io::Result<()> {
    let bytes = serde_json::to_vec(roots).map_err(std::io::Error::other)?;
    crate::durable::write_private(&dir.join(ROOTS_FILE), &bytes)
}

/// `apprentice.watched_folders` as saved, read again only when the settings
/// file changes.
fn settings_folders() -> Vec<PathBuf> {
    type Stamp = (Option<SystemTime>, u64);
    static CACHE: Mutex<Option<(Stamp, Vec<PathBuf>)>> = Mutex::new(None);
    let Ok(path) = crate::app_settings::path() else {
        return Vec::new();
    };
    let stamp: Option<Stamp> = std::fs::metadata(&path)
        .ok()
        .map(|meta| (meta.modified().ok(), meta.len()));
    let mut cache = lock(&CACHE);
    if let (Some(stamp), Some((cached, folders))) = (stamp, cache.as_ref())
        && stamp == *cached
    {
        return folders.clone();
    }
    let folders = crate::app_settings::AppSettings::read_from(&path)
        .map(|(settings, _)| settings.apprentice.watched_folders)
        .unwrap_or_default();
    *cache = stamp.map(|stamp| (stamp, folders.clone()));
    folders
}

/// The folders to watch: absolute paths only (`settings.json` may hold
/// others; only `hex settings` validates), each once, in order.
fn watchable(folders: &[PathBuf]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for folder in folders {
        if folder.is_absolute() && !out.contains(folder) {
            out.push(folder.clone());
        }
    }
    out
}

fn gate_now() -> bool {
    crate::labs::notice_patterns()
}

/// The front app's name, never its window; `None` while Apprentice is in
/// front.
fn front_app_now() -> Option<String> {
    crate::dictation::context::frontmost_other_application().map(|(name, _)| name)
}

fn busy_now() -> Busy {
    Busy {
        dictating: crate::transcription::engine::dictation_busy(),
        meeting: crate::meeting::recording(),
        capturing: crate::capture_host::snapshot().recording.is_some(),
    }
}

fn root_name(root: &Path) -> String {
    root.file_name().map_or_else(
        || root.to_string_lossy().into_owned(),
        |name| name.to_string_lossy().into_owned(),
    )
}

fn offer_title(ext: &str, root: &Path) -> String {
    tr_named(
        "New {kind} in {root}",
        &[("kind", &ext.to_uppercase()), ("root", &root_name(root))],
    )
}

fn offer_line(days: usize) -> String {
    tr_named(
        "You usually file these ({days} days). Show me how?",
        &[("days", &days.to_string())],
    )
}

/// Only new folders are kept of folder changes.
fn kept(change: &Change) -> bool {
    !change.folder || change.kind == ChangeKind::Added
}

/// One watcher; the thread calls [`Watcher::tick`] with the time, and tests
/// call it with theirs.
pub struct Watcher {
    dir: PathBuf,
    gate: fn() -> bool,
    folders: Box<dyn Fn() -> Vec<PathBuf> + Send>,
    front_app: fn() -> Option<String>,
    busy: fn() -> Busy,
    day_of: fn(u64) -> i64,
    events: SyncSender<NoticeEvent>,
    wake: Box<dyn Fn() + Send>,
    /// The folders being read, and their last good snapshots.
    roots: Vec<PathBuf>,
    snapshots: Vec<Option<Snapshot>>,
    health: Vec<(PathBuf, FolderHealth)>,
    /// The folder list the log was last written against.
    logged_roots: Option<Vec<PathBuf>>,
    /// Folders already reported as larger than the cap.
    truncated: HashSet<PathBuf>,
    /// Since the gate opened, the log was pruned and routines looked for.
    started: bool,
    pruned_ms: u64,
    /// The newest kept event, and the newest one detection has seen.
    newest_ms: Option<u64>,
    detected_ms: Option<u64>,
    /// The local day and the offers made on it.
    offers: (i64, usize),
}

impl Watcher {
    fn new(
        dir: PathBuf,
        folders: Box<dyn Fn() -> Vec<PathBuf> + Send>,
        events: SyncSender<NoticeEvent>,
        wake: Box<dyn Fn() + Send>,
    ) -> Watcher {
        Watcher {
            dir,
            gate: gate_now,
            folders,
            front_app: front_app_now,
            busy: busy_now,
            day_of: routine::local_day,
            events,
            wake,
            roots: Vec::new(),
            snapshots: Vec::new(),
            health: Vec::new(),
            logged_roots: None,
            truncated: HashSet::new(),
            started: false,
            pruned_ms: 0,
            newest_ms: None,
            detected_ms: None,
            offers: (i64::MIN, 0),
        }
    }

    /// One poll at `now_ms`. With the gate closed it only forgets its
    /// snapshots, touching nothing on disk.
    pub fn tick(&mut self, now_ms: u64) {
        if !(self.gate)() {
            self.close();
            return;
        }
        let folders = watchable(&(self.folders)());
        // Folders are read before the lock, so the pane never waits on a
        // slow one.
        let (changes, mut changed) = if folders.is_empty() {
            (Vec::new(), false)
        } else {
            self.read(&folders)
        };
        let _store = lock(&STORE);
        let log = match Log::open(&self.dir) {
            Ok(log) => log,
            Err(error) => {
                tracing::warn!(%error, "could not open the noticed events");
                return;
            }
        };
        if !self.adopt(&log, &folders) {
            return;
        }
        if folders.is_empty() {
            self.close();
            return;
        }
        if !self.started || now_ms.saturating_sub(self.pruned_ms) >= PRUNE_EVERY_MS {
            if let Err(error) = log.prune(now_ms) {
                tracing::warn!(%error, "could not prune the noticed events");
            }
            self.pruned_ms = now_ms;
        }
        let added: Vec<(usize, String)> = changes
            .iter()
            .filter(|(_, change)| !change.folder && change.kind == ChangeKind::Added)
            .filter_map(|(index, change)| Some((*index, snapshot::known_extension(&change.name)?)))
            .collect();
        if !changes.is_empty() {
            let app = (self.front_app)();
            let events: Vec<Event> = changes
                .iter()
                .map(|(index, change)| log::event(change, *index, now_ms, app.clone()))
                .collect();
            match log.append(&events) {
                Ok(()) => self.newest_ms = Some(now_ms),
                Err(error) => tracing::warn!(%error, "could not keep the noticed events"),
            }
        }
        let quiet = self.newest_ms.is_some_and(|newest| {
            now_ms.saturating_sub(newest) >= EPISODE_GAP_MS && self.detected_ms != Some(newest)
        });
        if !self.started || quiet {
            changed |= self.detect(&log, now_ms);
            self.detected_ms = self.newest_ms;
            self.started = true;
        }
        self.offer(&added, now_ms);
        if changed {
            self.send(NoticeEvent::Changed);
        }
    }

    /// Forgets the snapshots, so a reopened gate starts with a fresh first
    /// snapshot and notes nothing that happened while it was closed.
    fn close(&mut self) {
        self.roots.clear();
        self.snapshots.clear();
        self.health.clear();
        self.started = false;
    }

    /// Re-indexes the log when the folder list differs from the one it was
    /// written against; false when that failed and the tick must stop. A
    /// log without a saved list (the first run, events written by hand) is
    /// taken as written against this one.
    fn adopt(&mut self, log: &Log, folders: &[PathBuf]) -> bool {
        if self.logged_roots.is_none() {
            self.logged_roots = read_roots(&self.dir);
        }
        match &self.logged_roots {
            Some(old) if old.as_slice() == folders => return true,
            Some(old) => {
                if let Err(error) = log.remap(old, folders) {
                    tracing::warn!(%error, "could not re-index the noticed events");
                    return false;
                }
            }
            None => {}
        }
        if let Err(error) = write_roots(&self.dir, folders) {
            tracing::warn!(%error, "could not save the watched folder list");
        }
        self.logged_roots = Some(folders.to_vec());
        true
    }

    /// Reads every folder and returns the kept changes by root index, and
    /// whether any folder's health changed. A folder that can't be read is
    /// unreadable and keeps its last snapshot.
    fn read(&mut self, folders: &[PathBuf]) -> (Vec<(usize, Change)>, bool) {
        if self.roots != folders {
            let mut old: Vec<(PathBuf, Snapshot)> = self
                .roots
                .drain(..)
                .zip(self.snapshots.drain(..))
                .filter_map(|(root, snapshot)| Some((root, snapshot?)))
                .collect();
            self.snapshots = folders
                .iter()
                .map(|folder| {
                    let at = old.iter().position(|(root, _)| root == folder)?;
                    Some(old.swap_remove(at).1)
                })
                .collect();
            self.roots = folders.to_vec();
        }
        let mut changes = Vec::new();
        let mut health = Vec::with_capacity(self.roots.len());
        for (index, root) in self.roots.iter().enumerate() {
            match snapshot::read(root, snapshot::DEPTH, snapshot::MAX_ENTRIES) {
                Ok(new) => {
                    if new.truncated && self.truncated.insert(root.clone()) {
                        tracing::info!(
                            root = index,
                            cap = snapshot::MAX_ENTRIES,
                            "a watched folder has more entries than Apprentice reads"
                        );
                    }
                    if let Some(old) = &self.snapshots[index] {
                        changes.extend(
                            snapshot::diff(old, &new)
                                .into_iter()
                                .filter(kept)
                                .map(|change| (index, change)),
                        );
                    }
                    self.snapshots[index] = Some(new);
                    health.push((root.clone(), FolderHealth::Ok));
                }
                Err(error) => {
                    tracing::debug!(%error, root = index, "a watched folder can't be read");
                    health.push((root.clone(), FolderHealth::Unreadable));
                }
            }
        }
        let changed = health != self.health;
        self.health = health;
        (changes, changed)
    }

    /// Looks for routines in the whole log and takes them in; true when the
    /// known routines changed.
    fn detect(&self, log: &Log, now_ms: u64) -> bool {
        let found = routine::detect(&log.read(), &self.roots, now_ms, self.day_of);
        let mut routines = Routines::load(&self.dir);
        let before = routines.known.clone();
        let roots = &self.roots;
        let describe = |found: &Routine| {
            let root = roots.get(found.root).map(|root| root_name(root));
            routine::describe(found, root.as_deref().unwrap_or_default())
        };
        let noticed = routines.merge(found, roots, describe);
        if routines.known == before {
            return false;
        }
        if let Err(error) = routines.save() {
            tracing::warn!(%error, "could not save the noticed routines");
        }
        for id in &noticed {
            events::emit(ApprenticeAction::RoutineNoticed, id, None);
        }
        true
    }

    /// Offers a known routine for each new file whose kind starts one.
    fn offer(&mut self, added: &[(usize, String)], now_ms: u64) {
        if added.is_empty() {
            return;
        }
        let day = (self.day_of)(now_ms);
        if self.offers.0 != day {
            self.offers = (day, 0);
        }
        let busy = (self.busy)();
        let mut routines = Routines::load(&self.dir);
        let mut offered = false;
        for (index, ext) in added {
            let Some(root) = self.roots.get(*index) else {
                continue;
            };
            let Some(id) = offer::pick(&routines.known, ext, root, now_ms, self.offers.1, busy)
            else {
                continue;
            };
            let days = routines
                .known
                .iter()
                .find(|known| known.id == id)
                .map_or(0, |known| known.days);
            let event = NoticeEvent::Offer {
                id: id.clone(),
                title: offer_title(ext, root),
                line: offer_line(days),
            };
            if !self.send(event) {
                continue;
            }
            routines.set(&id, RoutineState::Offered { last_ms: now_ms });
            self.offers.1 += 1;
            offered = true;
            events::emit(ApprenticeAction::RoutineOffered, &id, None);
        }
        if offered && let Err(error) = routines.save() {
            tracing::warn!(%error, "could not save the noticed routines");
        }
    }

    /// Hands `event` to the app without waiting; false when it wasn't taken.
    fn send(&self, event: NoticeEvent) -> bool {
        if self.events.try_send(event).is_err() {
            return false;
        }
        (self.wake)();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apprentice::notice::log::EventKind;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{Receiver, sync_channel};

    const MIN: u64 = 60_000;
    const DAY: u64 = 24 * 60 * MIN;
    const NOW: u64 = 40 * DAY;

    struct Dir(PathBuf);

    impl Dir {
        fn new(name: &str) -> Dir {
            let dir = std::env::temp_dir().join(format!(
                "hex-watch-{name}-{}",
                SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            Dir(dir)
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn open() -> bool {
        true
    }

    fn finder() -> Option<String> {
        Some("Finder".into())
    }

    fn idle() -> Busy {
        Busy::default()
    }

    fn day_of(ms: u64) -> i64 {
        (ms / DAY) as i64
    }

    fn watcher(data: &Path, folders: Vec<PathBuf>) -> (Watcher, Receiver<NoticeEvent>) {
        let (sender, receiver) = sync_channel(16);
        let mut watcher = Watcher::new(
            data.to_path_buf(),
            Box::new(move || folders.clone()),
            sender,
            Box::new(|| {}),
        );
        watcher.gate = open;
        watcher.front_app = finder;
        watcher.busy = idle;
        watcher.day_of = day_of;
        (watcher, receiver)
    }

    fn event(t: u64, kind: EventKind, folder: bool, ext: &str, to: &str, name: &str) -> Event {
        let at = if kind == EventKind::Added && folder {
            String::new()
        } else {
            to.to_owned()
        };
        Event {
            t,
            root: 0,
            kind,
            folder,
            ext: ext.into(),
            from: at.clone(),
            to: at,
            name: name.into(),
            app: Some("Finder".into()),
        }
    }

    /// A pdf arrives, a month folder is made, the pdf moves in and is
    /// renamed.
    fn invoice_episode(start: u64, month: &str) -> Vec<Event> {
        let folder = format!("Invoices 2026-{month}");
        vec![
            event(start, EventKind::Added, false, "pdf", "", "a.pdf"),
            event(
                start + MIN,
                EventKind::Added,
                true,
                "",
                &folder,
                "Invoices #-#",
            ),
            event(
                start + 2 * MIN,
                EventKind::Moved,
                false,
                "pdf",
                &folder,
                "a.pdf",
            ),
            event(
                start + 3 * MIN,
                EventKind::Renamed,
                false,
                "pdf",
                &folder,
                "#-#-# a.pdf",
            ),
        ]
    }

    fn drain(receiver: &Receiver<NoticeEvent>) -> Vec<NoticeEvent> {
        receiver.try_iter().collect()
    }

    fn offers(events: &[NoticeEvent]) -> Vec<&NoticeEvent> {
        events
            .iter()
            .filter(|event| matches!(event, NoticeEvent::Offer { .. }))
            .collect()
    }

    #[test]
    fn a_routine_replayed_on_three_days_then_a_new_pdf_offers() {
        let dir = Dir::new("replay");
        let data = dir.0.join("noticed");
        let downloads = dir.0.join("Downloads");
        std::fs::create_dir_all(&downloads).unwrap();
        let log = Log::open(&data).unwrap();
        for (days_ago, month) in [(3, "09"), (2, "10"), (1, "11")] {
            log.append(&invoice_episode(NOW - days_ago * DAY, month))
                .unwrap();
        }
        let (mut watcher, receiver) = watcher(&data, vec![downloads.clone()]);

        // Start: the routine is found; the first snapshot notes nothing.
        watcher.tick(NOW);
        let first = drain(&receiver);
        assert!(first.contains(&NoticeEvent::Changed), "{first:?}");
        assert!(offers(&first).is_empty());
        let routines = Routines::load(&data);
        assert_eq!(routines.known.len(), 1);
        assert_eq!(routines.known[0].state, RoutineState::New);
        assert_eq!(routines.known[0].trigger.as_deref(), Some("pdf"));
        assert_eq!(routines.known[0].root, downloads);
        let id = routines.known[0].id.clone();

        std::fs::write(downloads.join("2026-11-20 Telekom.pdf"), b"%PDF").unwrap();
        watcher.tick(NOW + 5_000);
        let second = drain(&receiver);
        assert_eq!(
            offers(&second),
            vec![&NoticeEvent::Offer {
                id: id.clone(),
                title: "New PDF in Downloads".into(),
                line: "You usually file these (3 days). Show me how?".into(),
            }]
        );
        assert_eq!(
            Routines::load(&data).known[0].state,
            RoutineState::Offered {
                last_ms: NOW + 5_000
            }
        );
        let noted = log.read();
        let added = noted.last().unwrap();
        assert_eq!(
            (added.t, added.kind, added.ext.as_str(), added.name.as_str()),
            (NOW + 5_000, EventKind::Added, "pdf", "#-#-# a.pdf")
        );
        assert_eq!(added.app.as_deref(), Some("Finder"));

        // Not again within a day.
        std::fs::write(downloads.join("another.pdf"), b"%PDF").unwrap();
        watcher.tick(NOW + 10_000);
        assert!(offers(&drain(&receiver)).is_empty());
    }

    #[test]
    fn an_unreadable_root_is_reported_and_skipped() {
        let dir = Dir::new("unreadable");
        let data = dir.0.join("noticed");
        let missing = dir.0.join("Missing");
        let flaky = dir.0.join("Flaky");
        std::fs::create_dir_all(flaky.join("Old")).unwrap();
        std::fs::write(flaky.join("kept.pdf"), b"x").unwrap();
        let (mut watcher, receiver) = watcher(&data, vec![missing.clone(), flaky.clone()]);

        watcher.tick(NOW);
        assert_eq!(
            watcher.health,
            vec![
                (missing.clone(), FolderHealth::Unreadable),
                (flaky.clone(), FolderHealth::Ok),
            ]
        );
        assert!(drain(&receiver).contains(&NoticeEvent::Changed));

        // A folder that goes away keeps its last snapshot: nothing reads as
        // removed, and nothing as added when it comes back.
        let away = dir.0.join("Away");
        std::fs::rename(&flaky, &away).unwrap();
        watcher.tick(NOW + 5_000);
        assert_eq!(watcher.health[1], (flaky.clone(), FolderHealth::Unreadable));
        std::fs::rename(&away, &flaky).unwrap();
        watcher.tick(NOW + 10_000);
        assert_eq!(watcher.health[1], (flaky.clone(), FolderHealth::Ok));
        assert!(Log::open(&data).unwrap().read().is_empty());

        std::fs::write(flaky.join("new.pdf"), b"x").unwrap();
        watcher.tick(NOW + 15_000);
        let noted = Log::open(&data).unwrap().read();
        assert_eq!(noted.len(), 1, "{noted:?}");
        assert_eq!(noted[0].root, 1);
    }

    #[test]
    fn only_new_folders_are_noted_and_relative_folders_are_skipped() {
        let dir = Dir::new("folders");
        let data = dir.0.join("noticed");
        let root = dir.0.join("Root");
        std::fs::create_dir_all(&root).unwrap();
        let (mut watcher, _receiver) =
            watcher(&data, vec![PathBuf::from("Relative"), root.clone()]);
        watcher.tick(NOW);
        assert_eq!(watcher.roots, vec![root.clone()]);
        std::fs::create_dir(root.join("Invoices 2026-11")).unwrap();
        watcher.tick(NOW + 5_000);
        std::fs::rename(root.join("Invoices 2026-11"), root.join("Paid 2026-11")).unwrap();
        watcher.tick(NOW + 10_000);
        let noted = Log::open(&data).unwrap().read();
        assert_eq!(noted.len(), 1, "{noted:?}");
        assert!(noted[0].folder);
        assert_eq!(noted[0].kind, EventKind::Added);
        assert_eq!(noted[0].root, 0);
    }

    static GATE: AtomicBool = AtomicBool::new(true);

    fn gate() -> bool {
        GATE.load(Ordering::SeqCst)
    }

    #[test]
    fn the_thread_stops_when_the_gate_closes() {
        let dir = Dir::new("gate");
        let data = dir.0.join("noticed");
        let root = dir.0.join("Root");
        std::fs::create_dir_all(&root).unwrap();
        let (mut watcher, receiver) = watcher(&data, vec![root.clone()]);
        watcher.gate = gate;
        GATE.store(true, Ordering::SeqCst);
        watcher.tick(NOW);
        assert_eq!(watcher.snapshots.len(), 1);
        drain(&receiver);

        GATE.store(false, Ordering::SeqCst);
        let before: Vec<String> = std::fs::read_dir(&data)
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        std::fs::write(root.join("while-closed.pdf"), b"x").unwrap();
        watcher.tick(NOW + 5_000);
        assert!(watcher.snapshots.is_empty());
        assert!(watcher.roots.is_empty());
        assert!(watcher.health.is_empty());
        assert!(drain(&receiver).is_empty());
        let after: Vec<String> = std::fs::read_dir(&data)
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(before, after);
        assert!(Log::open(&data).unwrap().read().is_empty());

        // Open again: a fresh first snapshot, so what arrived while closed is
        // never noted.
        GATE.store(true, Ordering::SeqCst);
        watcher.tick(NOW + 10_000);
        assert_eq!(watcher.snapshots.len(), 1);
        assert!(Log::open(&data).unwrap().read().is_empty());
    }

    #[test]
    fn removing_a_folder_remaps_the_log() {
        let dir = Dir::new("remap");
        let data = dir.0.join("noticed");
        let (a, b) = (dir.0.join("A"), dir.0.join("B"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        let both = std::sync::Arc::new(Mutex::new(vec![a.clone(), b.clone()]));
        let shared = both.clone();
        let (sender, _receiver) = sync_channel(16);
        let mut watcher = Watcher::new(
            data.clone(),
            Box::new(move || lock(&shared).clone()),
            sender,
            Box::new(|| {}),
        );
        watcher.gate = open;
        watcher.front_app = finder;
        watcher.busy = idle;
        watcher.tick(NOW);
        std::fs::write(a.join("a.pdf"), b"x").unwrap();
        std::fs::write(b.join("b.pdf"), b"x").unwrap();
        watcher.tick(NOW + 5_000);
        assert_eq!(Log::open(&data).unwrap().read().len(), 2);
        *lock(&both) = vec![b.clone()];
        watcher.tick(NOW + 10_000);
        let noted = Log::open(&data).unwrap().read();
        assert_eq!(noted.len(), 1, "{noted:?}");
        assert_eq!(noted[0].root, 0);
        assert_eq!(read_roots(&data), Some(vec![b]));
    }

    #[test]
    fn ignoring_forgets_the_routine_and_its_events() {
        let dir = Dir::new("ignore");
        let data = dir.0.join("noticed");
        let downloads = dir.0.join("Downloads");
        std::fs::create_dir_all(&downloads).unwrap();
        let log = Log::open(&data).unwrap();
        for (days_ago, month) in [(3, "09"), (2, "10"), (1, "11")] {
            log.append(&invoice_episode(NOW - days_ago * DAY, month))
                .unwrap();
        }
        let unrelated = event(NOW - 5 * DAY, EventKind::Removed, false, "zip", "", "a.zip");
        log.append(std::slice::from_ref(&unrelated)).unwrap();
        let (mut watcher, _receiver) = watcher(&data, vec![downloads.clone()]);
        watcher.tick(NOW);
        let id = Routines::load(&data).known[0].id.clone();
        assert_eq!(
            status_in(Some(&data), std::slice::from_ref(&downloads))
                .routines
                .len(),
            1
        );

        assert!(ignore_in(&data, &id));
        assert_eq!(Routines::load(&data).known[0].state, RoutineState::Ignored);
        assert_eq!(log.read(), vec![unrelated]);
        let status = status_in(Some(&data), &[downloads.clone(), PathBuf::from("x")]);
        assert!(status.routines.is_empty());
        assert_eq!(status.folders[1].1, FolderHealth::Unreadable);

        forget_everything_in(&data);
        assert!(log.read().is_empty());
        assert!(Routines::load(&data).known.is_empty());
    }
}
