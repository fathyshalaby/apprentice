//! Routines: filing steps repeated on at least three days. Pure: events in,
//! routines out, plus the one sentence that describes a routine.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::log::{Event, EventKind};
use super::snapshot::folder_shape;
use crate::desktop::i18n::{tr, tr_named};

/// A quiet stretch longer than this ends an episode.
pub const EPISODE_GAP_MS: u64 = 10 * 60_000;
/// Distinct local days an episode must repeat on.
pub const MIN_DAYS: usize = 3;
/// Only episodes this recent count.
pub const WINDOW_MS: u64 = 14 * 24 * 60 * 60 * 1000;
/// How far before an episode a file's arrival can trigger it.
pub const TRIGGER_WINDOW_MS: u64 = 2 * 60 * 60 * 1000;

/// One thing the person did, abstracted so months and numbers match.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Step {
    pub kind: EventKind,
    pub folder: bool,
    pub ext: String,
    /// The destination folder's shape.
    pub to: String,
    /// The new name's shape, for renames and new folders.
    pub name: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Routine {
    pub id: String,
    pub root: usize,
    pub steps: Vec<Step>,
    pub days: usize,
    pub last_seen_ms: u64,
    /// The latest real folder name files were moved into.
    pub example_to: Option<String>,
    /// The extension of the file whose arrival usually starts it.
    pub trigger: Option<String>,
}

/// The events of each root, split where more than [`EPISODE_GAP_MS`] passes.
pub fn episodes(events: &[Event]) -> Vec<Vec<Event>> {
    let mut sorted: Vec<&Event> = events.iter().collect();
    sorted.sort_by_key(|event| (event.root, event.t));
    let mut out: Vec<Vec<Event>> = Vec::new();
    for event in sorted {
        let continues = out
            .last()
            .and_then(|episode| episode.last())
            .is_some_and(|last| {
                last.root == event.root && event.t.saturating_sub(last.t) <= EPISODE_GAP_MS
            });
        if continues {
            out.last_mut().expect("checked").push(event.clone());
        } else {
            out.push(vec![event.clone()]);
        }
    }
    out
}

/// The ordered, de-duplicated steps of one episode. A new file is not a step
/// (downloads arrive on their own); a new folder is.
pub fn signature(episode: &[Event]) -> Vec<Step> {
    let mut ordered: Vec<&Event> = episode.iter().collect();
    ordered.sort_by_key(|event| event.t);
    let mut steps: Vec<Step> = Vec::new();
    for event in &ordered {
        if event.kind == EventKind::Added && !event.folder {
            continue;
        }
        let name = match (event.kind, event.folder) {
            (EventKind::Renamed, _) => Some(event.name.clone()),
            (EventKind::Added, true) => Some(new_folder_name(event, &ordered)),
            _ => None,
        };
        let step = Step {
            kind: event.kind,
            folder: event.folder,
            ext: event.ext.clone(),
            to: folder_shape(Path::new(&event.to)),
            name,
        };
        if !steps.contains(&step) {
            steps.push(step);
        }
    }
    steps
}

/// A new folder's name shape: from the move that fills it when the episode
/// has one (the event's own shape only keeps letters and digits), else the
/// event's.
fn new_folder_name(added: &Event, episode: &[&Event]) -> String {
    let parent = Path::new(&added.to);
    episode
        .iter()
        .filter(|event| event.kind == EventKind::Moved && event.t >= added.t)
        .find_map(|event| {
            let target = Path::new(&event.to);
            let name = (target.parent() == Some(parent)).then(|| target.file_name())??;
            (folder_shape(Path::new(name)) == added.name).then_some(name)
        })
        .map_or_else(|| added.name.clone(), |name| folder_shape(Path::new(name)))
}

/// Putting something away: a move into a folder (not the root), a rename,
/// or a new folder. Deleting and moving to the root are not.
fn is_filing(step: &Step) -> bool {
    match step.kind {
        EventKind::Moved => !step.to.is_empty(),
        EventKind::Renamed => true,
        EventKind::Added => step.folder,
        EventKind::Removed => false,
    }
}

struct Group<'a> {
    root: usize,
    steps: Vec<Step>,
    episodes: Vec<&'a [Event]>,
}

/// Routines in `events`: an episode signature repeated on at least
/// [`MIN_DAYS`] distinct days within [`WINDOW_MS`]. An episode that ended
/// less than [`EPISODE_GAP_MS`] before `now_ms` may still be going and waits.
pub fn detect(
    events: &[Event],
    roots: &[PathBuf],
    now_ms: u64,
    day_of: impl Fn(u64) -> i64,
) -> Vec<Routine> {
    let all = episodes(events);
    let mut groups: BTreeMap<(usize, Vec<Step>), Group> = BTreeMap::new();
    for episode in &all {
        let (Some(first), Some(last)) = (episode.first(), episode.last()) else {
            continue;
        };
        let end = episode.iter().map(|event| event.t).max().unwrap_or(last.t);
        if now_ms.saturating_sub(end) < EPISODE_GAP_MS || end < now_ms.saturating_sub(WINDOW_MS) {
            continue;
        }
        let steps = signature(episode);
        if !steps.iter().any(is_filing) {
            continue;
        }
        groups
            .entry((first.root, steps.clone()))
            .or_insert_with(|| Group {
                root: first.root,
                steps,
                episodes: Vec::new(),
            })
            .episodes
            .push(episode);
    }
    groups
        .into_values()
        .filter_map(|group| routine_of(group, events, roots, &day_of))
        .collect()
}

fn routine_of(
    group: Group,
    events: &[Event],
    roots: &[PathBuf],
    day_of: &impl Fn(u64) -> i64,
) -> Option<Routine> {
    let days: BTreeSet<i64> = group
        .episodes
        .iter()
        .filter_map(|episode| episode.iter().map(|event| event.t).min())
        .map(day_of)
        .collect();
    if days.len() < MIN_DAYS {
        return None;
    }
    let last_seen_ms = group
        .episodes
        .iter()
        .flat_map(|episode| episode.iter().map(|event| event.t))
        .max()
        .unwrap_or(0);
    // Use the same filing step `describe` will name, so a PNG moved into a
    // different folder cannot supply the example for a PDF sentence.
    let filing = group
        .steps
        .iter()
        .find(|step| step.kind == EventKind::Moved && !step.folder && !step.to.is_empty());
    let example_to = filing.and_then(|step| {
        group
            .episodes
            .iter()
            .flat_map(|episode| episode.iter())
            .filter(|event| {
                event.kind == EventKind::Moved
                    && !event.folder
                    && event.ext == step.ext
                    && folder_shape(Path::new(&event.to)) == step.to
            })
            .max_by_key(|event| event.t)
            .map(|event| event.to.clone())
    });
    let root_path = roots.get(group.root).map_or_else(
        || format!("missing root {}", group.root),
        |path| path.to_string_lossy().into_owned(),
    );
    Some(Routine {
        id: routine_id(&root_path, &group.steps),
        root: group.root,
        trigger: trigger(&group, events),
        steps: group.steps,
        days: days.len(),
        last_seen_ms,
        example_to,
    })
}

fn routine_id(root: &str, steps: &[Step]) -> String {
    let mut hash = Sha256::new();
    hash.update(root.as_bytes());
    hash.update([0]);
    hash.update(serde_json::to_string(steps).unwrap_or_default().as_bytes());
    hash.finalize()
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The extension most episodes' files had when they arrived in the root
/// itself (not a folder in it, as offers only come for those) within
/// [`TRIGGER_WINDOW_MS`] before the episode, when at least half the
/// episodes have one.
fn trigger(group: &Group, events: &[Event]) -> Option<String> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    let mut with_one = 0;
    for episode in &group.episodes {
        let start = episode.iter().map(|event| event.t).min()?;
        let mut here: HashMap<&str, usize> = HashMap::new();
        for event in events {
            if event.root == group.root
                && event.kind == EventKind::Added
                && !event.folder
                && event.to.is_empty()
                && !event.ext.is_empty()
                && event.t <= start
                && start - event.t <= TRIGGER_WINDOW_MS
            {
                *here.entry(event.ext.as_str()).or_default() += 1;
            }
        }
        if let Some(ext) = most_common(&here) {
            with_one += 1;
            *counts.entry(ext).or_default() += 1;
        }
    }
    if with_one * 2 < group.episodes.len() {
        return None;
    }
    most_common(&counts).map(str::to_owned)
}

/// The highest count, the alphabetically first on a tie.
fn most_common<'a>(counts: &HashMap<&'a str, usize>) -> Option<&'a str> {
    counts
        .iter()
        .max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0)))
        .map(|(ext, _)| *ext)
}

/// The local calendar day number of a millisecond timestamp.
pub fn local_day(ms: u64) -> i64 {
    let seconds = (ms / 1000) as libc::time_t;
    // SAFETY: `localtime_r` writes only into the provided zeroed `tm`.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&seconds, &mut tm) };
    (seconds as i64 + tm.tm_gmtoff).div_euclid(86_400)
}

/// One sentence for a routine, in the interface language.
pub fn describe(routine: &Routine, root_name: &str) -> String {
    let step = |kind: EventKind, folder: bool| {
        routine
            .steps
            .iter()
            .find(|step| step.kind == kind && step.folder == folder)
    };
    let kind_of = |ext: &str| {
        if ext.is_empty() {
            tr("files").to_owned()
        } else {
            tr_named("{kind} files", &[("kind", &ext.to_uppercase())])
        }
    };
    if let Some(moved) = routine
        .steps
        .iter()
        .find(|step| step.kind == EventKind::Moved && !step.folder && !step.to.is_empty())
    {
        let folder = routine
            .example_to
            .clone()
            .unwrap_or_else(|| moved.to.clone());
        let mut sentence = tr_named(
            "Moves {kind} from {root} into folders like {folder}",
            &[
                ("kind", &kind_of(&moved.ext)),
                ("root", root_name),
                ("folder", &folder),
            ],
        );
        if let Some(shape) = step(EventKind::Renamed, false).and_then(|s| s.name.as_deref()) {
            sentence.push_str(&tr_named(
                ", then renames them like {shape}",
                &[("shape", shape)],
            ));
        }
        sentence
    } else if let Some(renamed) = step(EventKind::Renamed, false) {
        tr_named(
            "Renames {kind} in {root} like {shape}",
            &[
                ("kind", &kind_of(&renamed.ext)),
                ("root", root_name),
                ("shape", renamed.name.as_deref().unwrap_or_default()),
            ],
        )
    } else {
        let folder = routine
            .example_to
            .clone()
            .filter(|name| !name.is_empty())
            .or_else(|| step(EventKind::Added, true).and_then(|made| made.name.clone()))
            .filter(|name| !name.is_empty());
        match folder {
            Some(folder) => tr_named(
                "Makes folders like {folder} in {root}",
                &[("folder", &folder), ("root", root_name)],
            ),
            None => tr_named("Makes folders in {root}", &[("root", root_name)]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: u64 = 60_000;
    const DAY: u64 = 24 * 60 * MIN;
    const NOW: u64 = 30 * DAY;

    fn day_of(ms: u64) -> i64 {
        (ms / DAY) as i64
    }

    fn event(t: u64, kind: EventKind, folder: bool, ext: &str, to: &str, name: &str) -> Event {
        let parent = Path::new(to)
            .parent()
            .map_or(String::new(), |p| p.to_string_lossy().into_owned());
        let at = if kind == EventKind::Added && folder {
            parent.clone()
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
            app: None,
        }
    }

    /// One filing session starting at `start`: a pdf arrives, a month folder
    /// is made, the pdf moves in, and is renamed.
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

    fn roots() -> Vec<PathBuf> {
        vec![PathBuf::from("/Users/x/Downloads")]
    }

    fn three_days() -> Vec<Event> {
        let mut events = Vec::new();
        for (days_ago, month) in [(3, "09"), (2, "10"), (1, "11")] {
            events.extend(invoice_episode(NOW - days_ago * DAY, month));
        }
        events
    }

    #[test]
    fn the_invoice_routine_is_found_on_three_days() {
        let found = detect(&three_days(), &roots(), NOW, day_of);
        assert_eq!(found.len(), 1);
        let routine = &found[0];
        assert_eq!(routine.days, 3);
        assert_eq!(routine.example_to.as_deref(), Some("Invoices 2026-11"));
        assert_eq!(routine.trigger.as_deref(), Some("pdf"));
        assert_eq!(routine.steps.len(), 3);
        assert_eq!(routine.steps[0].name.as_deref(), Some("Invoices #-#"));
        assert_eq!(routine.steps[1].to, "Invoices #-#");
        assert_eq!(routine.last_seen_ms, NOW - DAY + 3 * MIN);
    }

    #[test]
    fn each_new_folder_gets_the_move_into_itself() {
        let events = vec![
            event(1, EventKind::Added, false, "pdf", "", "a.pdf"),
            event(
                2,
                EventKind::Added,
                true,
                "",
                "Invoices 2026-11",
                "Invoices #-#",
            ),
            event(
                3,
                EventKind::Added,
                true,
                "",
                "Archive 2026-11",
                "Archive #-#",
            ),
            event(
                4,
                EventKind::Moved,
                false,
                "pdf",
                "Archive 2026-11",
                "b.pdf",
            ),
            event(
                5,
                EventKind::Moved,
                false,
                "pdf",
                "Invoices 2026-11",
                "a.pdf",
            ),
        ];
        let made: Vec<_> = signature(&events)
            .into_iter()
            .filter(|step| step.kind == EventKind::Added && step.folder)
            .collect();
        assert_eq!(made.len(), 2);
        assert_eq!(made[0].name.as_deref(), Some("Invoices #-#"));
        assert_eq!(made[1].name.as_deref(), Some("Archive #-#"));
    }

    #[test]
    fn the_example_folder_comes_from_the_sentence_s_filing_step() {
        let mut events = three_days();
        for days_ago in [3, 2, 1] {
            let start = NOW - days_ago * DAY;
            events.push(event(
                start + 5 * MIN,
                EventKind::Moved,
                false,
                "png",
                &format!("Screenshots 2026-{:02}", 12 - days_ago),
                "shot.png",
            ));
        }
        let found = detect(&events, &roots(), NOW, day_of);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].example_to.as_deref(), Some("Invoices 2026-11"));
        assert_eq!(found[0].trigger.as_deref(), Some("pdf"));
    }

    #[test]
    fn only_a_file_added_to_the_root_itself_triggers() {
        // Offers come only for files that arrive in the root, so a file that
        // arrives in a subfolder never makes the trigger.
        let events: Vec<Event> = three_days()
            .into_iter()
            .map(|mut event| {
                if event.kind == EventKind::Added && !event.folder {
                    event.from = "Inbox".into();
                    event.to = "Inbox".into();
                }
                event
            })
            .collect();
        let found = detect(&events, &roots(), NOW, day_of);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].trigger, None);
    }

    #[test]
    fn two_days_are_not_enough() {
        let events: Vec<Event> = three_days().into_iter().skip(4).collect();
        assert!(detect(&events, &roots(), NOW, day_of).is_empty());
    }

    #[test]
    fn three_episodes_on_one_day_are_not_a_routine() {
        let mut events = Vec::new();
        for (hours, month) in [(1, "09"), (3, "10"), (5, "11")] {
            events.extend(invoice_episode(NOW - DAY - hours * 60 * MIN, month));
        }
        assert!(detect(&events, &roots(), NOW, day_of).is_empty());
    }

    #[test]
    fn days_outside_fourteen_are_ignored() {
        let mut events = invoice_episode(NOW - 20 * DAY, "08");
        events.extend(invoice_episode(NOW - 2 * DAY, "10"));
        events.extend(invoice_episode(NOW - DAY, "11"));
        assert!(detect(&events, &roots(), NOW, day_of).is_empty());
    }

    #[test]
    fn downloads_alone_are_not_steps() {
        let mut events = Vec::new();
        for days_ago in [3, 2, 1] {
            events.push(event(
                NOW - days_ago * DAY,
                EventKind::Added,
                false,
                "pdf",
                "",
                "a.pdf",
            ));
        }
        assert!(signature(&events).is_empty());
        assert!(detect(&events, &roots(), NOW, day_of).is_empty());
    }

    #[test]
    fn an_open_episode_waits() {
        let mut events = invoice_episode(NOW - 3 * DAY, "09");
        events.extend(invoice_episode(NOW - 2 * DAY, "10"));
        events.extend(invoice_episode(NOW - 4 * MIN, "11"));
        assert!(detect(&events, &roots(), NOW, day_of).is_empty());
        assert_eq!(detect(&events, &roots(), NOW + 10 * MIN, day_of).len(), 1);
    }

    #[test]
    fn deleting_downloads_daily_is_not_a_routine() {
        let mut events = Vec::new();
        for days_ago in [3, 2, 1] {
            events.push(event(
                NOW - days_ago * DAY,
                EventKind::Removed,
                false,
                "pdf",
                "",
                "a.pdf",
            ));
        }
        assert!(detect(&events, &roots(), NOW, day_of).is_empty());
    }

    #[test]
    fn moving_into_the_root_is_not_filing() {
        let mut events = Vec::new();
        for days_ago in [3, 2, 1] {
            events.push(event(
                NOW - days_ago * DAY,
                EventKind::Moved,
                false,
                "dmg",
                "",
                "a.dmg",
            ));
        }
        assert!(detect(&events, &roots(), NOW, day_of).is_empty());
    }

    #[test]
    fn ids_are_stable_and_differ_by_root() {
        let first = detect(&three_days(), &roots(), NOW, day_of);
        let again = detect(&three_days(), &roots(), NOW + MIN, day_of);
        assert_eq!(first[0].id, again[0].id);
        assert_eq!(first[0].id.len(), 16);
        let elsewhere = detect(
            &three_days(),
            &[PathBuf::from("/Users/x/Desktop")],
            NOW,
            day_of,
        );
        assert_ne!(first[0].id, elsewhere[0].id);
    }

    #[test]
    fn describe_reads_like_a_sentence() {
        let found = detect(&three_days(), &roots(), NOW, day_of);
        assert_eq!(
            describe(&found[0], "Downloads"),
            "Moves PDF files from Downloads into folders like Invoices 2026-11, then renames them like #-#-# a.pdf"
        );
    }

    #[test]
    fn local_days_advance_by_one_a_day() {
        assert_eq!(local_day(NOW + 2 * DAY), local_day(NOW) + 2);
    }
}
