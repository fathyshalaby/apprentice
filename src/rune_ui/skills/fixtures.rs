//! The Skills pane's scenes: nothing yet, a new lesson, a draft with why?
//! questions, the model picker, saved skills with their agents, and Labs ›
//! Notice patterns' Noticing section. A scene's pane records what it would
//! do and never reads or writes.

use std::path::PathBuf;

use super::{AgentRow, Notes, Recorded, SkillsPane};
use crate::agent_install::Agent;
use crate::apple_intelligence::Availability;
use crate::apprentice::evidence::Thin;
use crate::apprentice::install::Installed;
use crate::apprentice::lessons::{Lesson, Media};
use crate::apprentice::notice::state::{Known, RoutineState};
use crate::apprentice::notice::watch::{FolderHealth, NoticeStatus};
use crate::apprentice::ollama;
use crate::apprentice::skill::{Autonomy, Skill, Step};
use crate::apprentice::store::Draft;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::rune_ui) enum Fixture {
    Empty,
    Lesson,
    Draft,
    Models,
    Skills,
    /// Notice patterns on, nothing watched yet.
    NoticingEmpty,
    /// Two watched folders, one Apprentice can't read, and a routine.
    Noticing,
}

/// A folder under the home folder, so it reads from `~`.
fn home(relative: &str) -> PathBuf {
    crate::app_paths::home_dir()
        .unwrap_or_else(|| PathBuf::from("/Users/alex"))
        .join(relative)
}

/// What the Noticing scene noticed.
fn noticed() -> NoticeStatus {
    let downloads = home("Downloads");
    NoticeStatus {
        folders: vec![
            (downloads.clone(), FolderHealth::Ok),
            (home("Documents/Scans"), FolderHealth::Unreadable),
        ],
        routines: vec![Known {
            id: "5f2a9c".into(),
            root: downloads,
            steps: Vec::new(),
            description: "Moves PDFs from Downloads into folders like Invoices 2026-11, then \
                renames them like #-#-# a.pdf"
                .into(),
            days: 3,
            last_seen_ms: crate::history::now_ms(),
            trigger: Some("pdf".into()),
            state: RoutineState::New,
        }],
    }
}

fn step(title: &str, detail: &str, question: Option<&str>, why: Option<&str>) -> Step {
    Step {
        title: title.into(),
        detail: detail.into(),
        question: question.map(str::to_owned),
        why: why.map(str::to_owned),
    }
}

fn skill(name: &str, lesson: &str, steps: Vec<Step>) -> Skill {
    let now = crate::history::now_ms();
    Skill {
        slug: crate::apprentice::skill::slug(name),
        name: name.into(),
        description: "Downloads this month's invoice and files it in the accounts folder.".into(),
        when_to_use: "At the start of each month, once the invoice email arrives.".into(),
        inputs: vec!["The month".into()],
        steps,
        verification: vec!["The PDF is in Accounts/Invoices under this month's name".into()],
        edge_cases: vec!["The email can arrive a day late".into()],
        autonomy: Autonomy::Observe,
        version: 1,
        created_ms: now,
        updated_ms: now,
        lesson_id: lesson.into(),
    }
}

fn invoice(lesson: &str) -> Skill {
    skill(
        "File the monthly invoice",
        lesson,
        vec![
            step(
                "Open the billing email",
                "Find the newest email from Billing in Mail and open it.",
                None,
                None,
            ),
            step(
                "Download the March invoice",
                "Click Download PDF under the invoice summary.",
                Some("Why did you pick the March invoice?"),
                Some("It is the one for the month that just ended."),
            ),
            step(
                "Rename the file",
                "Name it 2026-03 Invoice.pdf.",
                Some("Why that name?"),
                None,
            ),
            step(
                "Move it to Accounts/Invoices",
                "Drag the file into Accounts/Invoices in Finder.",
                None,
                None,
            ),
        ],
    )
}

fn lesson(id: &str, minutes_ago: u64) -> Lesson {
    Lesson {
        bundle: PathBuf::from(format!(
            "/Users/alex/Library/Application Support/Scrawl/captures/{id}.scrawl"
        )),
        id: id.into(),
        created_ms: crate::history::now_ms() - minutes_ago * 60_000,
        duration_ms: 134_000,
        media: Media::Present,
        frames: 9,
    }
}

impl SkillsPane {
    pub(in crate::rune_ui) fn fixture(kind: Fixture) -> Self {
        let mut pane = Self {
            fixture: Some(Recorded::default()),
            ..Self::default()
        };
        match kind {
            Fixture::Empty => {}
            Fixture::NoticingEmpty => {
                pane.noticing.status = Some(NoticeStatus {
                    folders: Vec::new(),
                    routines: Vec::new(),
                });
            }
            Fixture::Noticing => pane.noticing.status = Some(noticed()),
            Fixture::Lesson => {
                let lesson = lesson("lesson-1", 6);
                pane.notes.insert(
                    lesson.id.clone(),
                    Some(Notes {
                        thin: Thin {
                            no_narration: false,
                            no_click_names: true,
                        },
                        text: String::new(),
                    }),
                );
                pane.lessons = vec![lesson];
                // What Ollama said of the scene's model: it sees images.
                pane.picker.installed = Some(Ok(vec![ollama::Installed {
                    name: "gemma4:26b".into(),
                    size: 17_200_000_000,
                    vision: true,
                }]));
            }
            Fixture::Draft => {
                let draft = Draft {
                    id: "1763800000000-ab12".into(),
                    lesson_id: "lesson-2".into(),
                    skill: invoice("lesson-2"),
                    evidence_text: "[00:02] said: Every month I file the invoice.".into(),
                };
                pane.editor = Some(super::draft_view::Editor::for_draft(&draft));
                pane.lessons = vec![lesson("lesson-2", 40)];
                pane.drafts = vec![draft];
            }
            Fixture::Models => {
                pane.picker.open = true;
                pane.picker.url = rune::TextEdit::new(crate::app_settings::DEFAULT_OLLAMA_URL);
                pane.picker.apple = Some(Availability::Available);
                pane.picker.installed = Some(Ok(vec![
                    ollama::Installed {
                        name: "gemma4:26b".into(),
                        size: 17_200_000_000,
                        vision: true,
                    },
                    ollama::Installed {
                        name: "llama3.2:3b".into(),
                        size: 2_000_000_000,
                        vision: false,
                    },
                ]));
            }
            Fixture::Skills => {
                let invoice = invoice("lesson-3");
                let mut weekly = skill(
                    "Send the weekly report",
                    "lesson-4",
                    vec![step(
                        "Export the dashboard",
                        "Use Export › PDF.",
                        None,
                        None,
                    )],
                );
                weekly.description = "Exports the dashboard and emails it to the team.".into();
                let rows = |states: [(Agent, Installed, bool); 4]| {
                    states
                        .into_iter()
                        .map(|(agent, state, found)| AgentRow {
                            agent,
                            state,
                            found,
                        })
                        .collect::<Vec<_>>()
                };
                pane.agents.insert(
                    invoice.slug.clone(),
                    rows([
                        (Agent::ClaudeCode, Installed::Current, true),
                        (Agent::Codex, Installed::Older, true),
                        (Agent::OpenCode, Installed::Missing, true),
                        (Agent::Pi, Installed::Foreign, true),
                    ]),
                );
                pane.agents.insert(
                    weekly.slug.clone(),
                    rows([
                        (Agent::ClaudeCode, Installed::Missing, true),
                        (Agent::Codex, Installed::Missing, true),
                        (Agent::OpenCode, Installed::Linked, true),
                        (Agent::Pi, Installed::Missing, false),
                    ]),
                );
                pane.expanded = Some(invoice.slug.clone());
                pane.skills = vec![invoice, weekly];
            }
        }
        pane
    }
}
