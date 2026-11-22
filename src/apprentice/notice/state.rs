//! What Apprentice knows about each routine it noticed, in the owner-only
//! `routines.json` beside the event log: its steps, its sentence, and whether
//! it may still be offered.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::routine::{Routine, Step};

const FILE: &str = "routines.json";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RoutineState {
    /// Noticed, never offered.
    New,
    /// Offered by capsule; `last_ms` is the latest offer.
    Offered { last_ms: u64 },
    /// Show me was pressed: no more offers.
    Shown,
    /// Stop asking or Forget: never offered or listed again.
    Ignored,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Known {
    pub id: String,
    pub root: PathBuf,
    pub steps: Vec<Step>,
    pub description: String,
    pub days: usize,
    pub last_seen_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger: Option<String>,
    pub state: RoutineState,
}

#[derive(Serialize, Deserialize, Default)]
struct File {
    routines: Vec<Known>,
}

pub struct Routines {
    path: PathBuf,
    pub known: Vec<Known>,
}

impl Routines {
    /// The routines in `dir`, for reading only: like [`Routines::try_load`],
    /// but a file that can't be read reads as empty.
    pub fn load(dir: &Path) -> Routines {
        Routines::try_load(dir).unwrap_or_else(|error| {
            tracing::warn!(%error, "could not read the noticed routines");
            Routines {
                path: dir.join(FILE),
                known: Vec::new(),
            }
        })
    }

    /// The routines in `dir`. A missing file is empty; a file that doesn't
    /// parse is set aside as `routines.json.unreadable-<sha256 prefix>` and
    /// the state starts empty. Any other read error is an error, so nothing
    /// saves over a file that may be fine.
    pub fn try_load(dir: &Path) -> io::Result<Routines> {
        let path = dir.join(FILE);
        let known = match std::fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice::<File>(&bytes) {
                Ok(file) => file.routines,
                Err(error) => {
                    tracing::warn!(%error, "the noticed routines don't read; starting over");
                    set_aside(&path, &bytes);
                    Vec::new()
                }
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(error) => return Err(error),
        };
        Ok(Routines { path, known })
    }

    pub fn save(&self) -> io::Result<()> {
        if let Some(dir) = self.path.parent() {
            crate::app_paths::create_private_dir(dir)?;
        }
        let file = File {
            routines: self.known.clone(),
        };
        let bytes = serde_json::to_vec_pretty(&file).map_err(io::Error::other)?;
        crate::durable::write_private(&self.path, &bytes)
    }

    /// Takes in what detection found: a known routine keeps its state and
    /// gets the new days, last seen, trigger, and sentence; a new one starts
    /// `New`. Routines of folders no longer in `roots` are dropped, except
    /// ignored ones, which stay so they are never noticed again. Returns the
    /// ids noticed for the first time.
    pub fn merge(
        &mut self,
        found: Vec<Routine>,
        roots: &[PathBuf],
        describe: impl Fn(&Routine) -> String,
    ) -> Vec<String> {
        let mut noticed = Vec::new();
        for routine in found {
            let Some(root) = roots.get(routine.root) else {
                continue;
            };
            let description = describe(&routine);
            match self.known.iter_mut().find(|known| known.id == routine.id) {
                Some(known) => {
                    known.root = root.clone();
                    known.steps = routine.steps;
                    known.description = description;
                    known.days = routine.days;
                    known.last_seen_ms = routine.last_seen_ms;
                    known.trigger = routine.trigger;
                }
                None => {
                    noticed.push(routine.id.clone());
                    self.known.push(Known {
                        id: routine.id,
                        root: root.clone(),
                        steps: routine.steps,
                        description,
                        days: routine.days,
                        last_seen_ms: routine.last_seen_ms,
                        trigger: routine.trigger,
                        state: RoutineState::New,
                    });
                }
            }
        }
        self.known
            .retain(|known| known.state == RoutineState::Ignored || roots.contains(&known.root));
        noticed
    }

    /// The routines the pane lists: every one not ignored.
    pub fn listed(&self) -> impl Iterator<Item = &Known> {
        self.known
            .iter()
            .filter(|known| known.state != RoutineState::Ignored)
    }

    pub fn set(&mut self, id: &str, state: RoutineState) {
        if let Some(known) = self.known.iter_mut().find(|known| known.id == id) {
            known.state = state;
        }
    }

    /// Never offered or listed again.
    pub fn forget(&mut self, id: &str) {
        self.set(id, RoutineState::Ignored);
    }
}

/// Renames a state file that doesn't parse to
/// `<name>.unreadable-<sha256 prefix>`, so nothing is lost and the next save
/// starts clean. Identical files share one name.
pub(super) fn set_aside(path: &Path, bytes: &[u8]) {
    let digest: String = Sha256::digest(bytes)
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let mut name = path.as_os_str().to_owned();
    name.push(format!(".unreadable-{digest}"));
    if let Err(error) = std::fs::rename(path, PathBuf::from(name)) {
        tracing::warn!(%error, "could not set an unreadable state file aside");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apprentice::notice::log::EventKind;

    struct Dir(PathBuf);

    impl Dir {
        fn new(name: &str) -> Dir {
            Dir(std::env::temp_dir().join(format!(
                "hex-routines-{name}-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            )))
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn routine(id: &str, days: usize, last_seen_ms: u64) -> Routine {
        Routine {
            id: id.into(),
            root: 0,
            steps: vec![Step {
                kind: EventKind::Moved,
                folder: false,
                ext: "pdf".into(),
                to: "Invoices #-#".into(),
                name: None,
            }],
            days,
            last_seen_ms,
            example_to: Some("Invoices 2026-11".into()),
            trigger: Some("pdf".into()),
        }
    }

    fn roots() -> Vec<PathBuf> {
        vec![PathBuf::from("/Users/x/Downloads")]
    }

    fn describe(routine: &Routine) -> String {
        format!("routine {} on {} days", routine.id, routine.days)
    }

    #[test]
    fn merge_keeps_state_and_updates_days() {
        let dir = Dir::new("merge");
        let mut routines = Routines::load(&dir.0);
        assert!(routines.known.is_empty());
        let noticed = routines.merge(vec![routine("a", 3, 10)], &roots(), describe);
        assert_eq!(noticed, vec!["a".to_string()]);
        assert_eq!(routines.known[0].state, RoutineState::New);
        assert_eq!(routines.known[0].root, roots()[0]);
        assert_eq!(routines.known[0].trigger.as_deref(), Some("pdf"));
        routines.set("a", RoutineState::Offered { last_ms: 7 });
        routines.save().unwrap();

        let mut routines = Routines::load(&dir.0);
        let noticed = routines.merge(vec![routine("a", 4, 20)], &roots(), describe);
        assert!(noticed.is_empty());
        let known = &routines.known[0];
        assert_eq!(known.days, 4);
        assert_eq!(known.last_seen_ms, 20);
        assert_eq!(known.description, "routine a on 4 days");
        assert_eq!(known.state, RoutineState::Offered { last_ms: 7 });

        // A routine not found again is kept; one of a folder no longer
        // watched is dropped.
        routines.merge(Vec::new(), &roots(), describe);
        assert_eq!(routines.known.len(), 1);
        routines.merge(Vec::new(), &[PathBuf::from("/Users/x/Desktop")], describe);
        assert!(routines.known.is_empty());
    }

    #[test]
    fn an_ignored_routine_is_never_listed_again() {
        let dir = Dir::new("ignored");
        let mut routines = Routines::load(&dir.0);
        routines.merge(
            vec![routine("a", 3, 10), routine("b", 3, 10)],
            &roots(),
            describe,
        );
        routines.forget("a");
        routines.save().unwrap();
        let mut routines = Routines::load(&dir.0);
        let listed: Vec<&str> = routines.listed().map(|known| known.id.as_str()).collect();
        assert_eq!(listed, ["b"]);
        // Found again, even in a folder no longer watched: still ignored and
        // not newly noticed.
        let noticed = routines.merge(vec![routine("a", 5, 30)], &roots(), describe);
        assert!(noticed.is_empty());
        routines.merge(Vec::new(), &[], describe);
        assert_eq!(routines.known.len(), 1);
        assert_eq!(routines.known[0].state, RoutineState::Ignored);
        assert_eq!(routines.listed().count(), 0);
    }

    #[test]
    fn an_unreadable_state_file_is_set_aside() {
        let dir = Dir::new("unreadable");
        std::fs::create_dir_all(&dir.0).unwrap();
        std::fs::write(dir.0.join(FILE), b"{ not json").unwrap();
        let routines = Routines::load(&dir.0);
        assert!(routines.known.is_empty());
        assert!(!dir.0.join(FILE).exists());
        let aside: Vec<String> = std::fs::read_dir(&dir.0)
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(aside.len(), 1, "{aside:?}");
        assert!(
            aside[0].starts_with("routines.json.unreadable-"),
            "{aside:?}"
        );
        assert_eq!(std::fs::read(dir.0.join(&aside[0])).unwrap(), b"{ not json");
        routines.save().unwrap();
        assert!(Routines::load(&dir.0).known.is_empty());
    }

    #[test]
    fn a_read_error_is_an_error_and_moves_nothing() {
        let dir = Dir::new("ioerror");
        std::fs::create_dir_all(dir.0.join(FILE)).unwrap();
        assert!(Routines::try_load(&dir.0).is_err());
        assert!(Routines::load(&dir.0).known.is_empty());
        assert!(dir.0.join(FILE).is_dir());
        assert_eq!(std::fs::read_dir(&dir.0).unwrap().count(), 1);
    }

    #[test]
    fn files_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = Dir::new("mode");
        let mut routines = Routines::load(&dir.0.join("noticed"));
        routines.merge(vec![routine("a", 3, 10)], &roots(), describe);
        routines.save().unwrap();
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&dir.0.join("noticed")), 0o700);
        assert_eq!(mode(&dir.0.join("noticed").join(FILE)), 0o600);
    }
}
