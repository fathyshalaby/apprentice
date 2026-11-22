//! A watched folder as a set of inodes, and the diff of two such sets. Pure
//! file-system reading: nothing here writes, follows links, or looks inside
//! packages. Later tasks consume these items.
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub ino: u64,
    /// Path relative to the watched root.
    pub rel: PathBuf,
    pub folder: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub entries: HashMap<u64, Entry>,
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
/// order, so a cut-off is deterministic. Errs only when the root itself
/// can't be read; a subfolder that can't be is skipped.
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
                if snapshot.entries.len() >= max_entries {
                    snapshot.truncated = true;
                    return Ok(snapshot);
                }
                let rel = parent.join(name_str);
                snapshot.entries.insert(
                    meta.ino(),
                    Entry {
                        ino: meta.ino(),
                        rel: rel.clone(),
                        folder,
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

/// What changed between two snapshots of one folder, matched by inode.
/// Sorted by destination folder, then name.
pub fn diff(old: &Snapshot, new: &Snapshot) -> Vec<Change> {
    let mut changes = Vec::new();
    for (ino, now) in &new.entries {
        let folder = parent_of(&now.rel);
        let name = name_of(&now.rel);
        match old.entries.get(ino) {
            None => changes.push(Change {
                kind: ChangeKind::Added,
                folder: now.folder,
                from: folder.clone(),
                to: folder,
                name,
            }),
            Some(before) => {
                let from = parent_of(&before.rel);
                let renamed = name_of(&before.rel) != name;
                let kind = if from != folder {
                    ChangeKind::Moved { renamed }
                } else if renamed {
                    ChangeKind::Renamed
                } else {
                    continue;
                };
                changes.push(Change {
                    kind,
                    folder: now.folder,
                    from,
                    to: folder,
                    name,
                });
            }
        }
    }
    for (ino, before) in &old.entries {
        if !new.entries.contains_key(ino) {
            let folder = parent_of(&before.rel);
            changes.push(Change {
                kind: ChangeKind::Removed,
                folder: before.folder,
                from: folder.clone(),
                to: folder,
                name: name_of(&before.rel),
            });
        }
    }
    changes.sort_by(|a, b| (&a.to, &a.name).cmp(&(&b.to, &b.name)));
    changes
}

/// Collapses runs: digits to `#`, letters to `a` (when `keep_letters` is
/// false), everything else kept.
fn shape(text: &str, keep_letters: bool) -> String {
    let mut out = String::new();
    let mut last = ' ';
    for c in text.chars() {
        let class = if c.is_ascii_digit() {
            '#'
        } else if !keep_letters && c.is_alphabetic() {
            'a'
        } else {
            out.push(c);
            last = c;
            continue;
        };
        if class != last || !matches!(last, '#' | 'a') {
            out.push(class);
        }
        last = class;
    }
    out
}

/// A file name with its words and numbers abstracted, extension kept:
/// `2026-11-03 Telekom.pdf` becomes `#-#-# a.pdf`.
pub fn name_shape(name: &str) -> String {
    if let Some((stem, ext)) = name.rsplit_once('.')
        && !stem.is_empty()
        && !ext.is_empty()
        && ext.len() <= 5
        && ext.chars().all(|c| c.is_ascii_alphanumeric())
        && ext.chars().any(|c| c.is_ascii_alphabetic())
    {
        return format!("{}.{ext}", shape(stem, false));
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
