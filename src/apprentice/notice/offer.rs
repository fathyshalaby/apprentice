//! When a new file may be offered as the start of a known routine. Pure: the
//! watcher supplies the time, today's count, and what is busy.

use std::path::Path;

use super::state::{Known, RoutineState};

/// No routine is offered twice within this.
pub const REOFFER_MS: u64 = 24 * 60 * 60 * 1000;
/// Offers in one local day, across routines.
pub const MAX_PER_DAY: usize = 3;

/// What would make an offer an interruption, or one Show me can't answer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Busy {
    pub dictating: bool,
    pub meeting: bool,
    /// A capture or a lesson records.
    pub capturing: bool,
    /// Annotate is off, so Show me's lesson couldn't record.
    pub annotate_off: bool,
}

impl Busy {
    fn any(self) -> bool {
        self.dictating || self.meeting || self.capturing || self.annotate_off
    }
}

/// The routine to offer for a file with `added_ext` that arrived in `root`:
/// one whose trigger it is, still `New` or `Offered` more than a day ago,
/// while fewer than three offers were made today, nothing records, and
/// Annotate is on. The routine seen on the most days wins.
pub fn pick(
    known: &[Known],
    added_ext: &str,
    root: &Path,
    now_ms: u64,
    offers_today: usize,
    busy: Busy,
) -> Option<String> {
    if busy.any() || offers_today >= MAX_PER_DAY {
        return None;
    }
    known
        .iter()
        .filter(|known| known.root == root && known.trigger.as_deref() == Some(added_ext))
        .filter(|known| match known.state {
            RoutineState::New => true,
            RoutineState::Offered { last_ms } => now_ms.saturating_sub(last_ms) >= REOFFER_MS,
            RoutineState::Shown | RoutineState::Ignored => false,
        })
        .max_by_key(|known| (known.days, known.last_seen_ms))
        .map(|known| known.id.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const NOW: u64 = 30 * REOFFER_MS;

    fn known(id: &str, state: RoutineState) -> Known {
        Known {
            id: id.into(),
            root: PathBuf::from("/Users/x/Downloads"),
            steps: Vec::new(),
            description: String::new(),
            days: 3,
            last_seen_ms: 0,
            trigger: Some("pdf".into()),
            state,
        }
    }

    fn downloads() -> PathBuf {
        PathBuf::from("/Users/x/Downloads")
    }

    fn pick_one(known: &[Known], offers_today: usize, busy: Busy) -> Option<String> {
        pick(known, "pdf", &downloads(), NOW, offers_today, busy)
    }

    #[test]
    fn offers_on_a_trigger() {
        let routines = [known("a", RoutineState::New)];
        assert_eq!(
            pick_one(&routines, 0, Busy::default()).as_deref(),
            Some("a")
        );
        let more = [
            known("a", RoutineState::New),
            Known {
                days: 5,
                ..known("b", RoutineState::New)
            },
        ];
        assert_eq!(pick_one(&more, 0, Busy::default()).as_deref(), Some("b"));
    }

    #[test]
    fn not_twice_in_a_day() {
        let recent = [known(
            "a",
            RoutineState::Offered {
                last_ms: NOW - REOFFER_MS + 1,
            },
        )];
        assert_eq!(pick_one(&recent, 1, Busy::default()), None);
        let yesterday = [known(
            "a",
            RoutineState::Offered {
                last_ms: NOW - REOFFER_MS,
            },
        )];
        assert_eq!(
            pick_one(&yesterday, 0, Busy::default()).as_deref(),
            Some("a")
        );
    }

    #[test]
    fn not_more_than_three_a_day() {
        let routines = [known("a", RoutineState::New)];
        assert!(pick_one(&routines, 2, Busy::default()).is_some());
        assert_eq!(pick_one(&routines, 3, Busy::default()), None);
    }

    #[test]
    fn not_while_busy() {
        let routines = [known("a", RoutineState::New)];
        for busy in [
            Busy {
                dictating: true,
                ..Busy::default()
            },
            Busy {
                meeting: true,
                ..Busy::default()
            },
            Busy {
                capturing: true,
                ..Busy::default()
            },
            Busy {
                annotate_off: true,
                ..Busy::default()
            },
        ] {
            assert_eq!(pick_one(&routines, 0, busy), None, "{busy:?}");
        }
    }

    #[test]
    fn never_for_shown_or_ignored() {
        for state in [RoutineState::Shown, RoutineState::Ignored] {
            assert_eq!(pick_one(&[known("a", state)], 0, Busy::default()), None);
        }
    }

    #[test]
    fn not_for_another_folder_or_kind() {
        let routines = [known("a", RoutineState::New)];
        let desktop = PathBuf::from("/Users/x/Desktop");
        assert_eq!(
            pick(&routines, "pdf", &desktop, NOW, 0, Busy::default()),
            None
        );
        assert_eq!(
            pick(&routines, "png", &downloads(), NOW, 0, Busy::default()),
            None
        );
        let untriggered = [Known {
            trigger: None,
            ..known("a", RoutineState::New)
        }];
        assert_eq!(pick_one(&untriggered, 0, Busy::default()), None);
    }
}
