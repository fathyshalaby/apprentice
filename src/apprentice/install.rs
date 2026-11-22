//! Apprentice's own skills in coding agents' folders, beside whatever the
//! person keeps there. A skill lands in `apprentice-<slug>` with a mark file
//! (`.apprentice-skill`) saying Apprentice wrote it; a file without the mark
//! is the person's, and is never replaced or removed. Nothing is written
//! through a symlink.

// Task 10's Skills pane is the first caller of everything here.
#![allow(dead_code)]

use crate::agent_install::{self, Agent, Paths, SkillFile};
use crate::apprentice::events::{self, ApprenticeAction};
use crate::apprentice::skill::{self, Skill};
use crate::desktop::i18n::tr_fill;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const MARK: &str = ".apprentice-skill";

/// What an agent's `apprentice-<slug>` folder holds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Installed {
    /// No SKILL.md there.
    Missing,
    /// Apprentice wrote it and it is this version, unedited.
    Current,
    /// Apprentice wrote it, but the skill has a newer version or the file
    /// was edited since.
    Older,
    /// A SKILL.md Apprentice didn't write.
    Foreign,
    /// The file or its folder is a symlink, managed elsewhere.
    Linked,
}

#[derive(Debug, Deserialize, Serialize)]
struct Mark {
    slug: String,
    version: u32,
}

/// The folder name, built only from the skill's sanitized slug.
fn folder(slug: &str) -> String {
    format!("apprentice-{}", skill::slug(slug))
}

fn mark_path(skill_md: &Path) -> PathBuf {
    skill_md.with_file_name(MARK)
}

fn read_mark(skill_md: &Path) -> Option<Mark> {
    serde_json::from_slice(&std::fs::read(mark_path(skill_md)).ok()?).ok()
}

fn read(paths: &Paths, agent: Agent, skill: &Skill) -> (PathBuf, Installed) {
    let slug = skill::slug(&skill.slug);
    let path = paths.skill_path(agent, &folder(&slug));
    if agent_install::linked(&path) {
        return (path, Installed::Linked);
    }
    let state = match std::fs::read(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Installed::Missing,
        // Unreadable: not something Apprentice can vouch for.
        Err(_) => Installed::Foreign,
        Ok(bytes) => match read_mark(&path) {
            Some(mark) if mark.slug == slug => {
                if mark.version >= skill.version && bytes == skill::markdown(skill).as_bytes() {
                    Installed::Current
                } else {
                    Installed::Older
                }
            }
            _ => Installed::Foreign,
        },
    };
    (path, state)
}

/// Each agent's copy of this skill. Codex and Pi share a folder, so an
/// install there reads installed for both.
pub fn status(paths: &Paths, skill: &Skill) -> Vec<(Agent, Installed)> {
    Agent::ALL
        .into_iter()
        .map(|agent| (agent, read(paths, agent, skill).1))
        .collect()
}

/// Writes the skill's SKILL.md, then its mark, into the agent's
/// `apprentice-<slug>` folder. A person's own file or a link is refused.
pub fn install(paths: &Paths, agent: Agent, skill: &Skill) -> Result<(), String> {
    let (path, state) = read(paths, agent, skill);
    match state {
        Installed::Foreign => {
            return Err(tr_fill(
                "{} isn't Apprentice's, so it was left alone",
                &path.display().to_string(),
            ));
        }
        Installed::Linked => {
            return Err(tr_fill(
                "{} is a link, so nothing was written",
                &path.display().to_string(),
            ));
        }
        Installed::Missing | Installed::Current | Installed::Older => {}
    }
    let slug = skill::slug(&skill.slug);
    let id = slug.clone();
    let markdown = skill::markdown(skill);
    let file = SkillFile {
        folder: &folder(&slug),
        contents: markdown.as_bytes(),
    };
    let path = agent_install::install(paths, agent, &file)?;
    let mark = serde_json::to_vec(&Mark {
        slug,
        version: skill.version,
    })
    .map_err(|error| error.to_string())?;
    crate::durable::write(&mark_path(&path), &mark)
        .map_err(|error| format!("{}: {}", mark_path(&path).display(), error))?;
    events::emit(ApprenticeAction::SkillInstalled, &id, None);
    Ok(())
}

/// Removes what Apprentice wrote for this slug: its SKILL.md and mark, and
/// the folder if nothing else is in it. Without the mark, or through a link,
/// nothing is touched.
pub fn remove(paths: &Paths, agent: Agent, slug: &str) -> Result<(), String> {
    let slug = skill::slug(slug);
    let folder = folder(&slug);
    let path = paths.skill_path(agent, &folder);
    if agent_install::linked(&path) {
        return Err(tr_fill(
            "{} is a link, so nothing was removed",
            &path.display().to_string(),
        ));
    }
    if read_mark(&path).is_none_or(|mark| mark.slug != slug) {
        return Err(tr_fill(
            "{} isn't Apprentice's, so it was left alone",
            &path.display().to_string(),
        ));
    }
    agent_install::remove(paths, agent, &folder)?;
    let mark = mark_path(&path);
    match std::fs::remove_file(&mark) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("{}: {}", mark.display(), error)),
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::remove_dir(parent);
    }
    events::emit(ApprenticeAction::SkillRemoved, &slug, None);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apprentice::skill::{Autonomy, Step, markdown};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU32, Ordering};

    struct TempDir(PathBuf);
    impl TempDir {
        fn new(tag: &str) -> Self {
            static N: AtomicU32 = AtomicU32::new(0);
            let path = std::env::temp_dir().join(format!(
                "apprentice-install-{}-{tag}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn skill(version: u32) -> Skill {
        Skill {
            slug: "file-expenses".into(),
            name: "File expenses".into(),
            description: "File an expense report".into(),
            when_to_use: "At month end".into(),
            inputs: vec![],
            steps: vec![Step {
                title: "Open the portal".into(),
                detail: String::new(),
                question: None,
                why: None,
            }],
            verification: vec![],
            edge_cases: vec![],
            autonomy: Autonomy::Observe,
            version,
            created_ms: 0,
            updated_ms: 0,
            lesson_id: "l".into(),
        }
    }

    fn setup(tag: &str) -> (TempDir, Paths, PathBuf) {
        let temp = TempDir::new(tag);
        let home = temp.0.join("home");
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        let folder = home.join(".claude/skills/apprentice-file-expenses");
        let paths = Paths {
            home,
            ..Paths::default()
        };
        (temp, paths, folder)
    }

    fn state(paths: &Paths, skill: &Skill, agent: Agent) -> Installed {
        status(paths, skill)
            .into_iter()
            .find(|(a, _)| *a == agent)
            .unwrap()
            .1
    }

    #[test]
    fn an_apprentice_skill_installs_into_its_own_folder() {
        let (_t, paths, folder) = setup("own");
        let s = skill(1);
        assert_eq!(state(&paths, &s, Agent::ClaudeCode), Installed::Missing);
        install(&paths, Agent::ClaudeCode, &s).unwrap();
        assert_eq!(
            std::fs::read_to_string(folder.join("SKILL.md")).unwrap(),
            markdown(&s)
        );
        let mark: serde_json::Value =
            serde_json::from_slice(&std::fs::read(folder.join(".apprentice-skill")).unwrap())
                .unwrap();
        assert_eq!(mark["slug"], "file-expenses");
        assert_eq!(mark["version"], 1);
        assert_eq!(state(&paths, &s, Agent::ClaudeCode), Installed::Current);
    }

    #[test]
    fn an_older_version_reads_older() {
        let (_t, paths, _) = setup("older");
        install(&paths, Agent::ClaudeCode, &skill(1)).unwrap();
        assert_eq!(
            state(&paths, &skill(2), Agent::ClaudeCode),
            Installed::Older
        );
        install(&paths, Agent::ClaudeCode, &skill(2)).unwrap();
        assert_eq!(
            state(&paths, &skill(2), Agent::ClaudeCode),
            Installed::Current
        );
    }

    #[test]
    fn a_persons_own_skill_is_never_replaced() {
        let (_t, paths, folder) = setup("foreign");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("SKILL.md"), "mine").unwrap();
        let s = skill(1);
        assert_eq!(state(&paths, &s, Agent::ClaudeCode), Installed::Foreign);
        assert!(install(&paths, Agent::ClaudeCode, &s).is_err());
        assert!(remove(&paths, Agent::ClaudeCode, "file-expenses").is_err());
        assert_eq!(
            std::fs::read_to_string(folder.join("SKILL.md")).unwrap(),
            "mine"
        );
        assert!(!folder.join(".apprentice-skill").exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_linked_folder_is_never_written() {
        let (t, paths, folder) = setup("linked");
        let outside = t.0.join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::create_dir_all(folder.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&outside, &folder).unwrap();
        let s = skill(1);
        assert_eq!(state(&paths, &s, Agent::ClaudeCode), Installed::Linked);
        assert!(install(&paths, Agent::ClaudeCode, &s).is_err());
        assert!(remove(&paths, Agent::ClaudeCode, "file-expenses").is_err());
        assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0);
    }

    #[test]
    fn remove_only_removes_what_apprentice_wrote() {
        let (_t, paths, folder) = setup("remove");
        install(&paths, Agent::ClaudeCode, &skill(1)).unwrap();
        std::fs::write(folder.join("NOTES.md"), "mine").unwrap();
        remove(&paths, Agent::ClaudeCode, "file-expenses").unwrap();
        assert!(!folder.join("SKILL.md").exists());
        assert!(!folder.join(".apprentice-skill").exists());
        assert_eq!(
            std::fs::read_to_string(folder.join("NOTES.md")).unwrap(),
            "mine"
        );
        // Alone in its folder, the folder goes too.
        install(&paths, Agent::ClaudeCode, &skill(1)).unwrap();
        std::fs::remove_file(folder.join("NOTES.md")).unwrap();
        remove(&paths, Agent::ClaudeCode, "file-expenses").unwrap();
        assert!(!folder.exists());
    }

    #[test]
    fn codex_and_pi_share_one_copy() {
        let (_t, paths, _) = setup("shared");
        std::fs::create_dir_all(paths.home.join(".codex")).unwrap();
        std::fs::create_dir_all(paths.home.join(".pi")).unwrap();
        let s = skill(1);
        install(&paths, Agent::Codex, &s).unwrap();
        assert_eq!(state(&paths, &s, Agent::Pi), Installed::Current);
        assert!(
            !paths
                .home
                .join(".pi/agent/skills/apprentice-file-expenses")
                .exists()
        );
    }

    #[allow(dead_code)]
    fn _unused(_: &Path) {}
}
