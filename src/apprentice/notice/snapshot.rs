//! A watched folder as a set of file identities, and the diff of two such
//! sets. Pure file-system reading: nothing here writes, follows links, or
//! looks inside packages. A file moved out of the tree (or below depth 2)
//! reads as Removed, and one moved in reads as Added. Later tasks consume
//! these items.
#![cfg_attr(not(test), allow(dead_code))]

use std::collections::HashMap;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

/// How deep below the watched folder an entry may sit (its own children and
/// their children).
pub const DEPTH: usize = 2;
/// The most entries one snapshot keeps.
pub const MAX_ENTRIES: usize = 5_000;

/// Folders with these extensions are packages: neither they nor their
/// contents are listed.
const PACKAGE_EXTENSIONS: [&str; 7] = [
    "app",
    "scrawl",
    "bundle",
    "photoslibrary",
    "pkg",
    "framework",
    "xcodeproj",
];

/// Extensions `name_shape` keeps; any other extension is shaped like the
/// rest of the name, so a surname after a dot never leaks.
const KNOWN_EXTENSIONS: &[&str] = &[
    "pdf", "doc", "docx", "txt", "rtf", "odt", "md", "pages", "numbers", "key", "xls", "xlsx",
    "csv", "ppt", "pptx", "png", "jpg", "jpeg", "gif", "heic", "tif", "tiff", "bmp", "webp", "svg",
    "psd", "mp3", "wav", "m4a", "aac", "flac", "mp4", "mov", "m4v", "avi", "mkv", "webm", "zip",
    "tar", "gz", "7z", "rar", "dmg", "json", "xml", "yaml", "yml", "html", "css", "js", "ts", "rs",
    "py", "swift", "sh", "sql", "log", "ics", "vcf", "epub", "eml",
];

/// Identity of a file on a volume.
pub type Key = (u64, u64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub dev: u64,
    pub ino: u64,
    /// Path relative to the watched root.
    pub rel: PathBuf,
    pub folder: bool,
    pub size: u64,
    pub mtime_ns: i128,
}

impl Entry {
    fn key(&self) -> Key {
        (self.dev, self.ino)
    }
}

#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub entries: HashMap<Key, Entry>,
    /// The entry cap cut the listing short.
    pub truncated: bool,
}

fn is_package(name: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| PACKAGE_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

/// Reads `root` breadth-first, shallow entries first and each folder in name
/// order, so a cut-off is deterministic. A file with several hard links
/// keeps its first name in that order; later names are skipped and don't
/// count toward the cap. Errs only when the root itself can't be read; a
/// subfolder that can't be is skipped.
pub fn read(root: &Path, max_depth: usize, max_entries: usize) -> std::io::Result<Snapshot> {
    let top: Vec<_> = std::fs::read_dir(root)?.filter_map(Result::ok).collect();
    let mut snapshot = Snapshot::default();
    let mut level: Vec<(PathBuf, Vec<std::fs::DirEntry>)> = vec![(PathBuf::new(), top)];
    for depth in 1..=max_depth {
        let mut next = Vec::new();
        for (parent, mut children) in level {
            children.sort_by_key(|c| c.file_name());
            for child in children {
                let name = child.file_name();
                let Some(name_str) = name.to_str() else {
                    continue;
                };
                if name_str.starts_with('.') {
                    continue;
                }
                let Ok(meta) = std::fs::symlink_metadata(child.path()) else {
                    continue;
                };
                let folder = meta.file_type().is_dir();
                if folder && is_package(name_str) {
                    continue;
                }
                let key = (meta.dev(), meta.ino());
                if snapshot.entries.contains_key(&key) {
                    continue;
                }
                if snapshot.entries.len() >= max_entries {
                    snapshot.truncated = true;
                    return Ok(snapshot);
                }
                let rel = parent.join(name_str);
                snapshot.entries.insert(
                    key,
                    Entry {
                        dev: key.0,
                        ino: key.1,
                        rel: rel.clone(),
                        folder,
                        size: meta.len(),
                        mtime_ns: i128::from(meta.mtime()) * 1_000_000_000
                            + i128::from(meta.mtime_nsec()),
                    },
                );
                if folder
                    && depth < max_depth
                    && let Ok(read) = std::fs::read_dir(child.path())
                {
                    next.push((rel, read.filter_map(Result::ok).collect()));
                }
            }
        }
        level = next;
    }
    Ok(snapshot)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Removed,
    Renamed,
    Moved { renamed: bool },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub kind: ChangeKind,
    pub folder: bool,
    /// Relative folder it came from ("" is the root); its own folder when
    /// added or removed.
    pub from: PathBuf,
    /// Relative folder it is in now; its own folder when added or removed.
    pub to: PathBuf,
    /// Current file name (last known one when removed).
    pub name: String,
}

fn parent_of(rel: &Path) -> PathBuf {
    rel.parent().map(Path::to_path_buf).unwrap_or_default()
}

fn name_of(rel: &Path) -> String {
    rel.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Reading order: shallower first, then by path.
fn order(rel: &Path) -> (usize, &Path) {
    (rel.components().count(), rel)
}

impl Snapshot {
    /// Identity of the folder holding `entry`; `None` is the root.
    fn parent_key(&self, entry: &Entry) -> Option<Key> {
        let parent = entry.rel.parent().filter(|p| !p.as_os_str().is_empty())?;
        self.entries
            .values()
            .find(|e| e.rel == parent)
            .map(Entry::key)
    }

    /// The last entry in reading order, where a cut-off snapshot stopped.
    fn cut(&self) -> Option<&Path> {
        if !self.truncated {
            return None;
        }
        self.entries
            .values()
            .map(|e| e.rel.as_path())
            .max_by(|a, b| order(a).cmp(&order(b)))
    }
}

/// Past a truncated snapshot's cut nothing is known, so it can't be said
/// that an entry there is missing.
fn beyond(cut: Option<&Path>, rel: &Path) -> bool {
    cut.is_some_and(|cut| order(rel) > order(cut))
}

/// Same key but not the same thing: a different kind, or a file whose
/// content (size and time) and name both changed.
fn replaced(before: &Entry, now: &Entry) -> bool {
    before.folder != now.folder
        || (!now.folder
            && name_of(&before.rel) != name_of(&now.rel)
            && before.size != now.size
            && before.mtime_ns != now.mtime_ns)
}

fn added(now: &Entry) -> Change {
    let folder = parent_of(&now.rel);
    Change {
        kind: ChangeKind::Added,
        folder: now.folder,
        from: folder.clone(),
        to: folder,
        name: name_of(&now.rel),
    }
}

fn removed(before: &Entry) -> Change {
    Change {
        kind: ChangeKind::Removed,
        ..added(before)
    }
}

/// What changed between two snapshots of one folder, matched by identity.
/// A renamed or moved folder is one change: its children are judged by
/// their parent folder's identity, not its path. When a snapshot was cut
/// short by the entry cap, additions and removals past its cut are not
/// reported. Sorted by destination folder, then name.
pub fn diff(old: &Snapshot, new: &Snapshot) -> Vec<Change> {
    let (old_cut, new_cut) = (old.cut(), new.cut());
    let mut changes = Vec::new();
    for (key, now) in &new.entries {
        match old.entries.get(key) {
            None => {
                if !beyond(old_cut, &now.rel) {
                    changes.push(added(now));
                }
            }
            Some(before) if replaced(before, now) => {
                changes.push(removed(before));
                changes.push(added(now));
            }
            Some(before) => {
                let renamed = name_of(&before.rel) != name_of(&now.rel);
                let kind = if old.parent_key(before) != new.parent_key(now) {
                    ChangeKind::Moved { renamed }
                } else if renamed {
                    ChangeKind::Renamed
                } else {
                    continue;
                };
                changes.push(Change {
                    kind,
                    folder: now.folder,
                    from: parent_of(&before.rel),
                    to: parent_of(&now.rel),
                    name: name_of(&now.rel),
                });
            }
        }
    }
    for (key, before) in &old.entries {
        if !new.entries.contains_key(key) && !beyond(new_cut, &before.rel) {
            changes.push(removed(before));
        }
    }
    changes.sort_by(|a, b| (&a.to, &a.name).cmp(&(&b.to, &b.name)));
    changes
}

fn is_mark(c: char) -> bool {
    matches!(c as u32, 0x300..=0x36F | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF | 0x20D0..=0x20FF | 0xFE20..=0xFE2F)
}

/// Collapses runs: numbers to `#`, and (unless `keep_letters`) letters and
/// marks to `a`; everything else is kept.
fn shape(text: &str, keep_letters: bool) -> String {
    let mut out = String::new();
    let mut run: Option<char> = None;
    for c in text.chars() {
        let class = if c.is_numeric() {
            Some('#')
        } else if !keep_letters && (c.is_alphanumeric() || is_mark(c)) {
            Some('a')
        } else {
            None
        };
        match class {
            Some(class) => {
                if run != Some(class) {
                    out.push(class);
                }
                run = Some(class);
            }
            None => {
                out.push(c);
                run = None;
            }
        }
    }
    out
}

/// A file name with its words and numbers abstracted, a well-known
/// extension kept: `2026-11-03 Telekom.pdf` becomes `#-#-# a.pdf`.
pub fn name_shape(name: &str) -> String {
    if let Some((stem, ext)) = name.rsplit_once('.') {
        let ext = ext.to_lowercase();
        if !stem.is_empty() && KNOWN_EXTENSIONS.contains(&ext.as_str()) {
            return format!("{}.{ext}", shape(stem, false));
        }
    }
    shape(name, false)
}

/// A relative folder path with its numbers abstracted, words kept:
/// `Invoices 2026-11` becomes `Invoices #-#`.
pub fn folder_shape(rel: &Path) -> String {
    shape(&rel.to_string_lossy(), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "hex-snapshot-{name}-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&root).unwrap();
            Self { root }
        }
        fn file(&self, rel: &str) {
            let path = self.root.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"x").unwrap();
        }
        fn dir(&self, rel: &str) {
            std::fs::create_dir_all(self.root.join(rel)).unwrap();
        }
        fn mv(&self, from: &str, to: &str) {
            std::fs::rename(self.root.join(from), self.root.join(to)).unwrap();
        }
        fn snap(&self) -> Snapshot {
            read(&self.root, DEPTH, MAX_ENTRIES).unwrap()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn rels(s: &Snapshot) -> Vec<String> {
        let mut v: Vec<_> = s
            .entries
            .values()
            .map(|e| e.rel.to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    }

    #[test]
    fn adds_removes_renames_and_moves_are_told_apart() {
        let f = Fixture::new("diff");
        f.dir("Invoices");
        f.file("gone.txt");
        f.file("old.txt");
        f.file("moved.txt");
        f.file("both.txt");
        let before = f.snap();
        std::fs::remove_file(f.root.join("gone.txt")).unwrap();
        f.file("new.txt");
        f.mv("old.txt", "fresh.txt");
        f.mv("moved.txt", "Invoices/moved.txt");
        f.mv("both.txt", "Invoices/other.txt");
        let changes = diff(&before, &f.snap());
        let find = |name: &str| changes.iter().find(|c| c.name == name).unwrap();
        assert_eq!(changes.len(), 5);
        assert_eq!(find("new.txt").kind, ChangeKind::Added);
        assert_eq!(find("gone.txt").kind, ChangeKind::Removed);
        assert_eq!(find("gone.txt").to, PathBuf::new());
        assert_eq!(find("fresh.txt").kind, ChangeKind::Renamed);
        let moved = find("moved.txt");
        assert_eq!(moved.kind, ChangeKind::Moved { renamed: false });
        assert_eq!(moved.from, PathBuf::new());
        assert_eq!(moved.to, PathBuf::from("Invoices"));
        assert_eq!(find("other.txt").kind, ChangeKind::Moved { renamed: true });
        assert!(!moved.folder);
        let names: Vec<_> = changes.iter().map(|c| (&c.to, &c.name)).collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
    }

    #[test]
    fn depth_two_is_the_limit() {
        let f = Fixture::new("depth");
        f.file("a/b/c.txt");
        f.file("a/b2.txt");
        let s = f.snap();
        assert_eq!(rels(&s), ["a", "a/b", "a/b2.txt"]);
    }

    #[test]
    fn the_entry_cap_truncates() {
        let f = Fixture::new("cap");
        for i in 0..5 {
            f.file(&format!("f{i}.txt"));
        }
        let s = read(&f.root, DEPTH, 3).unwrap();
        assert_eq!(s.entries.len(), 3);
        assert!(s.truncated);
        assert!(!f.snap().truncated);
    }

    #[test]
    fn packages_and_hidden_entries_are_skipped() {
        let f = Fixture::new("skip");
        f.file(".DS_Store");
        f.file(".hidden/inner.txt");
        f.file("x.app/Contents/a.txt");
        f.file("y.scrawl/b.txt");
        f.file("keep/c.txt");
        assert_eq!(rels(&f.snap()), ["keep", "keep/c.txt"]);
    }

    #[test]
    fn a_root_that_vanishes_yields_no_removals() {
        let f = Fixture::new("gone");
        assert!(read(&f.root.join("missing"), DEPTH, MAX_ENTRIES).is_err());
    }

    #[test]
    fn a_renamed_folder_is_one_change() {
        let f = Fixture::new("folder-rename");
        f.file("Invoices/a.pdf");
        let before = f.snap();
        f.mv("Invoices", "Bills");
        let changes = diff(&before, &f.snap());
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind, ChangeKind::Renamed);
        assert!(changes[0].folder);
        assert_eq!(changes[0].name, "Bills");
    }

    #[test]
    fn a_moved_folder_is_one_change() {
        let f = Fixture::new("folder-move");
        f.file("Inbox/a.pdf");
        f.dir("Archive");
        let before = read(&f.root, 3, MAX_ENTRIES).unwrap();
        f.mv("Inbox", "Archive/Inbox");
        let changes = diff(&before, &read(&f.root, 3, MAX_ENTRIES).unwrap());
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind, ChangeKind::Moved { renamed: false });
        assert_eq!(changes[0].to, PathBuf::from("Archive"));
    }

    #[test]
    fn an_early_addition_under_the_cap_is_one_change() {
        let f = Fixture::new("churn");
        for i in 1..=5 {
            f.file(&format!("f{i}.txt"));
        }
        let before = read(&f.root, DEPTH, 3).unwrap();
        f.file("a-first.txt");
        let after = read(&f.root, DEPTH, 3).unwrap();
        let changes = diff(&before, &after);
        assert_eq!(changes.len(), 1, "{changes:?}");
        assert_eq!(changes[0].kind, ChangeKind::Added);
        assert_eq!(changes[0].name, "a-first.txt");
    }

    #[test]
    fn a_kind_change_under_one_key_is_removed_and_added() {
        let mut old = Snapshot::default();
        let mut new = Snapshot::default();
        let entry = |folder| Entry {
            dev: 1,
            ino: 7,
            rel: PathBuf::from("x"),
            folder,
            size: 1,
            mtime_ns: 1,
        };
        old.entries.insert((1, 7), entry(false));
        new.entries.insert((1, 7), entry(true));
        let kinds: Vec<_> = diff(&old, &new).iter().map(|c| c.kind).collect();
        assert_eq!(kinds.len(), 2);
        assert!(kinds.contains(&ChangeKind::Added) && kinds.contains(&ChangeKind::Removed));
    }

    #[test]
    fn a_renamed_and_rewritten_file_is_removed_and_added() {
        let mut old = Snapshot::default();
        let mut new = Snapshot::default();
        let entry = |rel: &str, size, mtime_ns| Entry {
            dev: 1,
            ino: 7,
            rel: PathBuf::from(rel),
            folder: false,
            size,
            mtime_ns,
        };
        old.entries.insert((1, 7), entry("a.txt", 1, 1));
        new.entries.insert((1, 7), entry("b.txt", 2, 2));
        assert_eq!(diff(&old, &new).len(), 2);
    }

    #[test]
    fn a_hard_link_is_one_entry() {
        let f = Fixture::new("hardlink");
        f.file("a.txt");
        std::fs::hard_link(f.root.join("a.txt"), f.root.join("b.txt")).unwrap();
        let s = f.snap();
        assert_eq!(rels(&s), ["a.txt"]);
        assert!(!s.truncated);
        assert!(!read(&f.root, DEPTH, 1).unwrap().truncated);
    }

    #[test]
    fn name_shapes_do_not_leak_odd_extensions() {
        assert_eq!(name_shape("J.Smith"), "a.a");
        assert_eq!(name_shape("report.final"), "a.a");
        assert_eq!(name_shape("a.PDF"), "a.pdf");
        assert_eq!(name_shape("\u{663}.pdf"), "#.pdf");
        assert_eq!(name_shape("\u{FF13}.pdf"), "#.pdf");
        assert_eq!(name_shape("e\u{301}.txt"), "a.txt");
    }

    #[test]
    fn name_shapes() {
        assert_eq!(name_shape("2026-11-03 Telekom.pdf"), "#-#-# a.pdf");
        assert_eq!(name_shape("INV-4471.pdf"), "a-#.pdf");
        assert_eq!(
            name_shape("Screenshot 2026-11-20 at 09.14.png"),
            "a #-#-# a #.#.png"
        );
        assert_eq!(name_shape("README"), "a");
    }

    #[test]
    fn folder_shapes() {
        assert_eq!(folder_shape(Path::new("Invoices 2026-11")), "Invoices #-#");
        assert_eq!(folder_shape(Path::new("")), "");
    }
}
