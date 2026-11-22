//! The owner-only, 14-day log of noticed file events. Names are kept only as
//! shapes (`snapshot::name_shape`); folder paths are kept as the person named
//! them. Nothing here leaves the Mac.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::snapshot::{Change, ChangeKind, known_extension, name_shape};

const KEEP_MS: u64 = 14 * 24 * 60 * 60 * 1000;
const MAX_LINES: usize = 20_000;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    Added,
    Removed,
    Renamed,
    Moved,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Event {
    pub t: u64,
    pub root: usize,
    pub kind: EventKind,
    pub folder: bool,
    pub ext: String,
    pub from: String,
    pub to: String,
    /// The file name's shape, never the name.
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
}

/// The event for one change. A move that renames is `moved` with the new
/// name's shape. `ext` is only ever an allow-listed extension.
pub fn event(change: &Change, root: usize, t: u64, app: Option<String>) -> Event {
    let kind = match change.kind {
        ChangeKind::Added => EventKind::Added,
        ChangeKind::Removed => EventKind::Removed,
        ChangeKind::Renamed => EventKind::Renamed,
        ChangeKind::Moved { .. } => EventKind::Moved,
    };
    let ext = if change.folder {
        String::new()
    } else {
        known_extension(&change.name).unwrap_or_default()
    };
    Event {
        t,
        root,
        kind,
        folder: change.folder,
        ext,
        from: change.from.to_string_lossy().into_owned(),
        to: change.to.to_string_lossy().into_owned(),
        name: name_shape(&change.name),
        app,
    }
}

pub struct Log {
    path: PathBuf,
}

impl Log {
    pub fn open(dir: &Path) -> io::Result<Log> {
        crate::app_paths::create_private_dir(dir)?;
        Ok(Log {
            path: dir.join("events.ndjson"),
        })
    }

    pub fn append(&self, events: &[Event]) -> io::Result<()> {
        if events.is_empty() {
            return Ok(());
        }
        let mut buffer = Vec::new();
        for event in events {
            serde_json::to_writer(&mut buffer, event).map_err(io::Error::other)?;
            buffer.push(b'\n');
        }
        let mut file = crate::app_paths::open_private_append(&self.path)?;
        file.write_all(&buffer)
    }

    /// Every readable event, oldest first; bad lines (including invalid
    /// UTF-8) are skipped, and an unreadable log reads as empty.
    pub fn read(&self) -> Vec<Event> {
        self.try_read().unwrap_or_default()
    }

    /// Like `read`, but an I/O error is an error; a missing file is empty.
    fn try_read(&self) -> io::Result<Vec<Event>> {
        let bytes = match std::fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        Ok(bytes
            .split(|byte| *byte == b'\n')
            .filter_map(|line| serde_json::from_slice(line).ok())
            .collect())
    }

    /// Drops events older than 14 days and keeps the newest 20,000.
    pub fn prune(&self, now_ms: u64) -> io::Result<()> {
        let cutoff = now_ms.saturating_sub(KEEP_MS);
        let mut events = self.try_read()?;
        events.retain(|event| event.t >= cutoff);
        if events.len() > MAX_LINES {
            events.drain(..events.len() - MAX_LINES);
        }
        self.rewrite(&events)
    }

    /// Re-indexes roots by path after the watched list changed; events of a
    /// removed root are dropped.
    pub fn remap(&self, old: &[PathBuf], new: &[PathBuf]) -> io::Result<()> {
        let events: Vec<Event> = self
            .try_read()?
            .into_iter()
            .filter_map(|mut event| {
                let path = old.get(event.root)?;
                event.root = new.iter().position(|candidate| candidate == path)?;
                Some(event)
            })
            .collect();
        self.rewrite(&events)
    }

    /// Drops the events `matching` selects, in one root or in all.
    pub fn forget(&self, root: Option<usize>, matching: impl Fn(&Event) -> bool) -> io::Result<()> {
        let mut events = self.try_read()?;
        events.retain(|event| !(root.is_none_or(|root| event.root == root) && matching(event)));
        self.rewrite(&events)
    }

    pub fn clear(&self) -> io::Result<()> {
        self.rewrite(&[])
    }

    fn rewrite(&self, events: &[Event]) -> io::Result<()> {
        let mut buffer = Vec::new();
        for event in events {
            serde_json::to_writer(&mut buffer, event).map_err(io::Error::other)?;
            buffer.push(b'\n');
        }
        crate::durable::write_private(&self.path, &buffer)
    }
}

/// `<data>/apprentice/noticed`.
pub fn noticed_dir() -> PathBuf {
    crate::app_paths::support_dir()
        .unwrap_or_else(|_| std::env::temp_dir().join("apprentice"))
        .join("noticed")
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Dir(PathBuf);

    impl Dir {
        fn new(name: &str) -> Dir {
            Dir(std::env::temp_dir().join(format!(
                "hex-noticed-{name}-{}",
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

    fn change(name: &str, kind: ChangeKind) -> Change {
        Change {
            kind,
            folder: false,
            from: PathBuf::new(),
            to: PathBuf::from("Invoices 2026-11"),
            name: name.to_string(),
        }
    }

    fn ev(t: u64, root: usize) -> Event {
        event(&change("a.pdf", ChangeKind::Added), root, t, None)
    }

    #[test]
    fn events_keep_shapes_not_names() {
        let e = event(
            &change(
                "2026-11-03 Telekom.pdf",
                ChangeKind::Moved { renamed: true },
            ),
            0,
            5,
            Some("Finder".into()),
        );
        assert_eq!(e.name, "#-#-# a.pdf");
        assert_eq!(e.ext, "pdf");
        assert_eq!(e.kind, EventKind::Moved);
        let line = serde_json::to_string(&e).unwrap();
        assert!(!line.contains("Telekom"));
        assert!(!line.contains("2026-11-03"));

        let e = event(&change("J.Smith", ChangeKind::Added), 0, 5, None);
        assert_eq!(e.ext, "");
        assert!(!serde_json::to_string(&e).unwrap().contains("Smith"));
    }

    #[test]
    fn append_then_read_round_trips_and_files_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = Dir::new("rt");
        let log = Log::open(&dir.0.join("noticed")).unwrap();
        let events = vec![ev(1, 0), ev(2, 1)];
        log.append(&events).unwrap();
        assert_eq!(log.read(), events);
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&dir.0.join("noticed")), 0o700);
        assert_eq!(mode(&dir.0.join("noticed/events.ndjson")), 0o600);
        log.prune(10).unwrap();
        assert_eq!(mode(&dir.0.join("noticed/events.ndjson")), 0o600);
    }

    #[test]
    fn prune_drops_old_events_and_caps_lines() {
        let dir = Dir::new("prune");
        let log = Log::open(&dir.0).unwrap();
        let now = 100 * 24 * 60 * 60 * 1000;
        log.append(&[ev(now - KEEP_MS - 1, 0), ev(now - KEEP_MS, 0)])
            .unwrap();
        log.prune(now).unwrap();
        assert_eq!(log.read().len(), 1);
        let many: Vec<Event> = (0..MAX_LINES as u64 + 5)
            .map(|i| ev(now - 1000 + i, 0))
            .collect();
        log.clear().unwrap();
        log.append(&many).unwrap();
        log.prune(now + 10_000).unwrap();
        let kept = log.read();
        assert_eq!(kept.len(), MAX_LINES);
        assert_eq!(kept.last().unwrap().t, now - 1000 + MAX_LINES as u64 + 4);
        assert_eq!(kept[0].t, now - 1000 + 5);
    }

    #[test]
    fn removing_a_folder_drops_its_events_and_remaps_the_rest() {
        let dir = Dir::new("remap");
        let log = Log::open(&dir.0).unwrap();
        log.append(&[ev(1, 0), ev(2, 1), ev(3, 2)]).unwrap();
        let (a, b, c) = (
            PathBuf::from("/a"),
            PathBuf::from("/b"),
            PathBuf::from("/c"),
        );
        log.remap(&[a.clone(), b, c.clone()], &[a, c]).unwrap();
        let read: Vec<(u64, usize)> = log.read().iter().map(|e| (e.t, e.root)).collect();
        assert_eq!(read, vec![(1, 0), (3, 1)]);
    }

    #[test]
    fn forget_and_clear() {
        let dir = Dir::new("forget");
        let log = Log::open(&dir.0).unwrap();
        log.append(&[ev(1, 0), ev(2, 1)]).unwrap();
        log.forget(Some(1), |_| true).unwrap();
        assert_eq!(log.read().len(), 1);
        log.forget(None, |e| e.t == 1).unwrap();
        assert!(log.read().is_empty());
        log.append(&[ev(1, 0)]).unwrap();
        log.clear().unwrap();
        assert!(log.read().is_empty());
    }

    #[test]
    fn an_invalid_utf8_line_costs_only_that_line() {
        let dir = Dir::new("utf8");
        let log = Log::open(&dir.0).unwrap();
        log.append(&[ev(100, 0)]).unwrap();
        let path = dir.0.join("events.ndjson");
        let mut bytes = std::fs::read(&path).unwrap();
        bytes.extend_from_slice(b"\xff\xfe{\n");
        std::fs::write(&path, &bytes).unwrap();
        log.append(&[ev(200, 0)]).unwrap();
        assert_eq!(log.read().len(), 2);
        log.prune(300).unwrap();
        assert_eq!(log.read().len(), 2);
    }

    #[test]
    fn a_read_error_aborts_a_rewrite() {
        let dir = Dir::new("rderr");
        let log = Log::open(&dir.0).unwrap();
        std::fs::create_dir(dir.0.join("events.ndjson")).unwrap();
        assert!(log.prune(10).is_err());
        assert!(log.clear().is_err());
        assert!(log.forget(None, |_| true).is_err());
        assert!(log.remap(&[], &[]).is_err());
        assert!(dir.0.join("events.ndjson").is_dir());
    }

    #[test]
    fn a_lowercase_length_change_in_a_name_works() {
        let e = event(&change("x.\u{212A}ey", ChangeKind::Added), 0, 1, None);
        assert_eq!(e.name, "a.key");
        assert_eq!(e.ext, "key");
    }

    #[test]
    fn bad_lines_are_skipped() {
        let dir = Dir::new("bad");
        let log = Log::open(&dir.0).unwrap();
        log.append(&[ev(1, 0)]).unwrap();
        let path = dir.0.join("events.ndjson");
        let mut text = std::fs::read_to_string(&path).unwrap();
        text.push_str("not json\n{\"t\":1}\n");
        std::fs::write(&path, text).unwrap();
        log.append(&[ev(2, 0)]).unwrap();
        assert_eq!(log.read().len(), 2);
    }
}
