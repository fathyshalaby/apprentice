//! Drafts and approved skills on disk, owner-only, under `<support>/skills`.

use crate::apprentice::events::{self, ApprenticeAction};
use crate::apprentice::skill::{self, Skill};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// A skill the model drafted, waiting for the person to approve it.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Draft {
    pub id: String,
    pub lesson_id: String,
    pub skill: Skill,
    /// The text evidence the draft came from, copied so a skill never
    /// depends on its recording.
    #[serde(skip)]
    pub evidence_text: String,
}

/// What to do when an approved skill's slug already exists.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Clash {
    Replace,
    KeepBoth,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StoreError {
    /// The slug already exists and no choice was made.
    Clash(String),
    NotFound,
    Io(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::Clash(slug) => write!(f, "a skill named {slug} already exists"),
            StoreError::NotFound => f.write_str("not found"),
            StoreError::Io(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<std::io::Error> for StoreError {
    fn from(error: std::io::Error) -> Self {
        StoreError::Io(error.to_string())
    }
}

/// A skill that was just approved, under the slug it was stored as.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Approved {
    pub slug: String,
    pub skill: Skill,
}

pub struct Store {
    root: PathBuf,
}

const DRAFTS: &str = "drafts";

fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && name != DRAFTS
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

impl Store {
    pub fn at(root: PathBuf) -> Store {
        Store { root }
    }

    /// The store in the data folder.
    pub fn open() -> color_eyre::Result<Store> {
        Ok(Store::at(crate::app_paths::support_dir()?.join("skills")))
    }

    fn drafts_dir(&self) -> PathBuf {
        self.root.join(DRAFTS)
    }

    fn private_dir(path: &Path) -> Result<(), StoreError> {
        Ok(crate::app_paths::create_private_dir(path)?)
    }

    fn write(path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
        Ok(crate::durable::write_private(path, bytes)?)
    }

    /// A fresh draft id, `<created_ms>-<4 hex>`, whose folder does not exist.
    pub fn new_draft_id(&self, created_ms: u64) -> String {
        loop {
            let mut bytes = [0u8; 2];
            if getrandom::fill(&mut bytes).is_err() {
                let nanos = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.subsec_nanos());
                bytes = [(nanos >> 8) as u8, nanos as u8];
            }
            let id = format!("{created_ms}-{:02x}{:02x}", bytes[0], bytes[1]);
            if !self.drafts_dir().join(&id).exists() {
                return id;
            }
        }
    }

    pub fn save_draft(&self, draft: &Draft) -> Result<(), StoreError> {
        if !safe_draft_id(&draft.id) {
            return Err(StoreError::NotFound);
        }
        let dir = self.drafts_dir().join(&draft.id);
        Self::private_dir(&self.root)?;
        Self::private_dir(&self.drafts_dir())?;
        Self::private_dir(&dir)?;
        let json = serde_json::to_vec_pretty(draft).map_err(|e| StoreError::Io(e.to_string()))?;
        Self::write(&dir.join("evidence.txt"), draft.evidence_text.as_bytes())?;
        Self::write(&dir.join("draft.json"), &json)
    }

    fn read_draft(&self, id: &str) -> Option<Draft> {
        let dir = self.drafts_dir().join(id);
        let mut draft: Draft =
            serde_json::from_slice(&fs::read(dir.join("draft.json")).ok()?).ok()?;
        draft.evidence_text = fs::read_to_string(dir.join("evidence.txt")).unwrap_or_default();
        Some(draft)
    }

    /// Drafts, newest first.
    pub fn drafts(&self) -> Vec<Draft> {
        let Ok(entries) = fs::read_dir(self.drafts_dir()) else {
            return Vec::new();
        };
        let mut drafts: Vec<Draft> = entries
            .flatten()
            .filter_map(|entry| {
                let id = entry.file_name().into_string().ok()?;
                let draft = safe_draft_id(&id).then(|| self.read_draft(&id)).flatten();
                if draft.is_none() {
                    tracing::warn!(folder = %id, "skipping an unreadable draft");
                }
                draft
            })
            .collect();
        drafts.sort_by(|a, b| {
            b.skill
                .created_ms
                .cmp(&a.skill.created_ms)
                .then_with(|| b.id.cmp(&a.id))
        });
        drafts
    }

    /// Moves a draft's folder to the Trash, where it can still be put back.
    pub fn discard_draft(&self, id: &str) -> Result<(), StoreError> {
        if !safe_draft_id(id) {
            return Err(StoreError::NotFound);
        }
        let dir = self.drafts_dir().join(id);
        if !dir.join("draft.json").is_file() {
            return Err(StoreError::NotFound);
        }
        Ok(move_to_trash(&dir)?)
    }

    fn write_skill(&self, skill: &Skill) -> Result<(), StoreError> {
        let dir = self.root.join(&skill.slug);
        Self::private_dir(&self.root)?;
        Self::private_dir(&dir)?;
        let json = serde_json::to_vec_pretty(skill).map_err(|e| StoreError::Io(e.to_string()))?;
        Self::write(&dir.join("skill.json"), &json)?;
        Self::write(&dir.join("SKILL.md"), skill::markdown(skill).as_bytes())
    }

    fn read_skill(&self, slug: &str) -> Option<Skill> {
        safe_name(slug).then_some(())?;
        serde_json::from_slice(&fs::read(self.root.join(slug).join("skill.json")).ok()?).ok()
    }

    pub fn approve(&self, id: &str, clash: Option<Clash>) -> Result<Approved, StoreError> {
        if !safe_draft_id(id) {
            return Err(StoreError::NotFound);
        }
        let draft = self.read_draft(id).ok_or(StoreError::NotFound)?;
        let mut skill = draft.skill;
        let wanted = folder_slug(&skill.slug);
        let exists = self.root.join(&wanted).join("skill.json").exists();
        skill.slug = wanted.clone();
        if exists {
            match clash {
                None => return Err(StoreError::Clash(wanted)),
                Some(Clash::Replace) => {
                    if let Some(old) = self.read_skill(&wanted) {
                        skill.created_ms = old.created_ms;
                        skill.version = old.version + 1;
                    }
                }
                Some(Clash::KeepBoth) => {
                    skill.slug = (2..)
                        .map(|n| format!("{wanted}-{n}"))
                        .find(|slug| !self.root.join(slug).exists())
                        .expect("an unbounded range has a free name");
                }
            }
        }
        self.write_skill(&skill)?;
        let _ = fs::remove_dir_all(self.drafts_dir().join(id));
        events::emit(ApprenticeAction::SkillApproved, &skill.slug, None);
        Ok(Approved {
            slug: skill.slug.clone(),
            skill,
        })
    }

    /// Approved skills, sorted by name.
    pub fn skills(&self) -> Vec<Skill> {
        let Ok(entries) = fs::read_dir(&self.root) else {
            return Vec::new();
        };
        let mut skills: Vec<Skill> = entries
            .flatten()
            .filter_map(|entry| {
                let folder = entry.file_name().into_string().ok()?;
                if folder == DRAFTS || !entry.path().join("skill.json").exists() {
                    return None;
                }
                let skill = self.read_skill(&folder);
                if skill.is_none() {
                    tracing::warn!(%folder, "skipping an unreadable skill");
                }
                skill
            })
            .collect();
        skills.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        skills
    }

    pub fn save_edit(&self, mut skill: Skill) -> Result<Skill, StoreError> {
        let old = self.read_skill(&skill.slug).ok_or(StoreError::NotFound)?;
        skill.created_ms = old.created_ms;
        skill.lesson_id = old.lesson_id;
        skill.version = old.version + 1;
        skill.updated_ms = crate::history::now_ms().max(old.updated_ms + 1);
        self.write_skill(&skill)?;
        events::emit(
            ApprenticeAction::SkillEdited,
            &skill.slug,
            Some(skill.version),
        );
        Ok(skill)
    }

    pub fn delete_skill(&self, slug: &str) -> Result<(), StoreError> {
        if !safe_name(slug) || !self.root.join(slug).join("skill.json").is_file() {
            return Err(StoreError::NotFound);
        }
        Ok(fs::remove_dir_all(self.root.join(slug))?)
    }
}

fn safe_draft_id(id: &str) -> bool {
    id != DRAFTS && safe_name(id)
}

/// `slug` as a skill's folder name: `drafts` is the drafts folder, so a
/// skill named Drafts is stored as `drafts-skill`.
fn folder_slug(slug: &str) -> String {
    let slug = skill::slug(slug);
    if slug == DRAFTS {
        format!("{slug}-skill")
    } else {
        slug
    }
}

/// Moves `path` to the Trash. Tests move it to a folder of their own
/// instead and note it ([`trashed`]), so nothing reaches the person's Trash.
fn move_to_trash(path: &Path) -> std::io::Result<()> {
    #[cfg(test)]
    return test_trash::put(path);
    #[cfg(not(test))]
    crate::screenshots::save::trash(path)
        .map_err(|error| std::io::Error::other(format!("{error:#}")))
}

/// Whether a test's [`Store`] moved `path` to the Trash.
#[cfg(test)]
pub(crate) fn trashed(path: &Path) -> bool {
    test_trash::TRASHED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .any(|moved| moved == path)
}

#[cfg(test)]
mod test_trash {
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    pub(super) static TRASHED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

    pub(super) fn put(path: &Path) -> std::io::Result<()> {
        let mut trashed = TRASHED.lock().unwrap_or_else(|e| e.into_inner());
        let bin = std::env::temp_dir().join(format!("apprentice-trash-{}", std::process::id()));
        std::fs::create_dir_all(&bin)?;
        std::fs::rename(path, bin.join(trashed.len().to_string()))?;
        trashed.push(path.to_owned());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apprentice::skill::{Autonomy, Step};
    use std::os::unix::fs::PermissionsExt;

    struct Temp(PathBuf);
    impl Temp {
        fn new(name: &str) -> Temp {
            let path = std::env::temp_dir()
                .join(format!("apprentice-store-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            Temp(path)
        }
        fn store(&self) -> Store {
            Store::at(self.0.clone())
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn skill(name: &str) -> Skill {
        Skill {
            slug: skill::slug(name),
            name: name.into(),
            description: "Does the thing.".into(),
            when_to_use: "When asked.".into(),
            inputs: vec![],
            steps: vec![Step {
                title: "Go".into(),
                detail: "Do it.".into(),
                question: None,
                why: None,
            }],
            verification: vec![],
            edge_cases: vec![],
            autonomy: Autonomy::Observe,
            version: 1,
            created_ms: 1_000,
            updated_ms: 1_000,
            lesson_id: "lesson-1".into(),
        }
    }

    fn draft(id: &str, name: &str) -> Draft {
        Draft {
            id: id.into(),
            lesson_id: "lesson-1".into(),
            skill: skill(name),
            evidence_text: "00:01 clicked Save".into(),
        }
    }

    #[test]
    fn drafts_round_trip_with_their_evidence() {
        let t = Temp::new("roundtrip");
        let store = t.store();
        store.save_draft(&draft("1-aaaa", "File invoice")).unwrap();
        store.save_draft(&draft("2-bbbb", "Other")).unwrap();
        let drafts = store.drafts();
        assert_eq!(drafts.len(), 2);
        assert_eq!(drafts[0].id, "2-bbbb");
        assert_eq!(drafts[1], draft("1-aaaa", "File invoice"));
        assert!(t.0.join("drafts/1-aaaa/evidence.txt").is_file());
        store.discard_draft("1-aaaa").unwrap();
        assert_eq!(store.drafts().len(), 1);
        assert!(store.discard_draft("../x").is_err());
    }

    /// Discard moves a draft to the Trash, never deletes it for good.
    #[test]
    fn discarding_a_draft_moves_it_to_the_trash() {
        let t = Temp::new("discard-trash");
        let store = t.store();
        store.save_draft(&draft("1-cccc", "File invoice")).unwrap();
        let folder = t.0.join("drafts/1-cccc");
        store.discard_draft("1-cccc").unwrap();
        assert!(!folder.exists(), "no folder is left in drafts");
        assert!(store.drafts().is_empty());
        assert!(trashed(&folder), "it went to the Trash");
        assert_eq!(store.discard_draft("1-cccc"), Err(StoreError::NotFound));
    }

    /// `drafts` is the drafts folder: a skill named Drafts gets its own.
    #[test]
    fn a_skill_named_drafts_never_lands_in_the_drafts_folder() {
        let t = Temp::new("reserved");
        let store = t.store();
        store.save_draft(&draft("1-dddd", "Drafts")).unwrap();
        store.save_draft(&draft("2-eeee", "Other")).unwrap();
        let approved = store.approve("1-dddd", None).unwrap();
        assert_eq!(approved.slug, "drafts-skill");
        assert!(t.0.join("drafts-skill/SKILL.md").is_file());
        assert_eq!(store.skills(), vec![approved.skill]);
        assert_eq!(store.drafts().len(), 1, "the other draft is untouched");
        // A second one keeps both under the next free name.
        store.save_draft(&draft("3-ffff", "Drafts")).unwrap();
        let again = store.approve("3-ffff", Some(Clash::KeepBoth)).unwrap();
        assert_eq!(again.slug, "drafts-skill-2");
    }

    #[test]
    fn approving_moves_a_draft_to_skills() {
        let t = Temp::new("approve");
        let store = t.store();
        store.save_draft(&draft("1-aaaa", "File invoice")).unwrap();
        let approved = store.approve("1-aaaa", None).unwrap();
        assert_eq!(approved.slug, "file-invoice");
        assert!(store.drafts().is_empty());
        assert_eq!(store.skills(), vec![approved.skill]);
        assert!(t.0.join("file-invoice/SKILL.md").is_file());
        assert_eq!(store.approve("nope", None), Err(StoreError::NotFound));
    }

    #[test]
    fn a_clashing_slug_asks_first() {
        let t = Temp::new("clash");
        let store = t.store();
        store.save_draft(&draft("1-aaaa", "File invoice")).unwrap();
        store.approve("1-aaaa", None).unwrap();
        store.save_draft(&draft("2-bbbb", "File invoice")).unwrap();
        assert_eq!(
            store.approve("2-bbbb", None),
            Err(StoreError::Clash("file-invoice".into()))
        );
        assert_eq!(store.drafts().len(), 1);
        let mut newer = draft("2-bbbb", "File invoice");
        newer.skill.description = "New.".into();
        newer.skill.created_ms = 5_000;
        store.save_draft(&newer).unwrap();
        let replaced = store.approve("2-bbbb", Some(Clash::Replace)).unwrap();
        assert_eq!(replaced.skill.version, 2);
        assert_eq!(replaced.skill.created_ms, 1_000);
        assert_eq!(replaced.skill.description, "New.");
        assert_eq!(store.skills().len(), 1);
    }

    #[test]
    fn keep_both_adds_a_number() {
        let t = Temp::new("both");
        let store = t.store();
        for id in ["1-aaaa", "2-bbbb", "3-cccc"] {
            store.save_draft(&draft(id, "File invoice")).unwrap();
        }
        store.approve("1-aaaa", None).unwrap();
        let b = store.approve("2-bbbb", Some(Clash::KeepBoth)).unwrap();
        let c = store.approve("3-cccc", Some(Clash::KeepBoth)).unwrap();
        assert_eq!(b.slug, "file-invoice-2");
        assert_eq!(b.skill.slug, "file-invoice-2");
        assert_eq!(c.slug, "file-invoice-3");
        assert_eq!(store.skills().len(), 3);
    }

    #[test]
    fn an_edit_bumps_the_version_and_regenerates_markdown() {
        let t = Temp::new("edit");
        let store = t.store();
        store.save_draft(&draft("1-aaaa", "File invoice")).unwrap();
        let approved = store.approve("1-aaaa", None).unwrap();
        let mut edited = approved.skill.clone();
        edited.description = "Edited words.".into();
        edited.created_ms = 99;
        edited.lesson_id = "other".into();
        let saved = store.save_edit(edited).unwrap();
        assert_eq!(saved.version, 2);
        assert_eq!(saved.created_ms, 1_000);
        assert_eq!(saved.lesson_id, "lesson-1");
        assert!(saved.updated_ms > 1_000);
        let md = fs::read_to_string(t.0.join("file-invoice/SKILL.md")).unwrap();
        assert!(md.contains("Edited words."));
        assert_eq!(store.skills(), vec![saved]);
        let mut ghost = skill("Ghost");
        ghost.slug = "ghost".into();
        assert_eq!(store.save_edit(ghost), Err(StoreError::NotFound));
        store.delete_skill("file-invoice").unwrap();
        assert!(store.skills().is_empty());
        assert!(store.delete_skill("drafts").is_err());
    }

    #[test]
    fn everything_is_owner_only() {
        let t = Temp::new("modes");
        let store = t.store();
        store.save_draft(&draft("1-aaaa", "File invoice")).unwrap();
        let mode = |p: &str| fs::metadata(t.0.join(p)).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(""), 0o700);
        assert_eq!(mode("drafts"), 0o700);
        assert_eq!(mode("drafts/1-aaaa"), 0o700);
        assert_eq!(mode("drafts/1-aaaa/draft.json"), 0o600);
        assert_eq!(mode("drafts/1-aaaa/evidence.txt"), 0o600);
        store.approve("1-aaaa", None).unwrap();
        assert_eq!(mode("file-invoice"), 0o700);
        assert_eq!(mode("file-invoice/skill.json"), 0o600);
        assert_eq!(mode("file-invoice/SKILL.md"), 0o600);
    }

    #[test]
    fn skill_md_is_never_read_back() {
        let t = Temp::new("md");
        let store = t.store();
        store.save_draft(&draft("1-aaaa", "File invoice")).unwrap();
        let approved = store.approve("1-aaaa", None).unwrap();
        fs::write(t.0.join("file-invoice/SKILL.md"), "tampered").unwrap();
        assert_eq!(store.skills(), vec![approved.skill]);
    }

    #[test]
    fn new_draft_ids_never_collide() {
        let t = Temp::new("ids");
        let store = t.store();
        let a = store.new_draft_id(5);
        store.save_draft(&draft(&a, "A")).unwrap();
        let b = store.new_draft_id(5);
        assert_ne!(a, b);
        assert!(a.starts_with("5-"));
    }
}
