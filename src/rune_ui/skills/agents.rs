//! The Skills pane's approved skills: one card each, which opens to the
//! coding agents on this Mac with Install, Update, or Remove for the copy
//! Apprentice wrote there (a copy it didn't write, or a link, is shown and
//! left alone), and Edit and Delete skill. Each is its own click.

use rune::{El, col, row, text};

use super::super::Ui;
use super::super::controls::{
    ButtonKind, DESCRIPTION, METRICS, Paint, button, card, note, setting_row,
};
use super::{AgentRow, Armed, small};
use crate::agent_install::{Agent, Paths};
use crate::apprentice::install::{self, Installed};
use crate::apprentice::skill::Skill;
use crate::desktop::i18n::{tr, tr_fill};

/// Each agent's copy of `skill`; none without `paths` (a home folder
/// Apprentice can't read).
pub(super) fn rows(paths: Option<&Paths>, skill: &Skill) -> Vec<AgentRow> {
    let Some(paths) = paths else {
        return Vec::new();
    };
    install::status(paths, skill)
        .into_iter()
        .map(|(agent, state)| AgentRow {
            agent,
            state,
            found: paths.found(agent),
        })
        .collect()
}

/// Reads `slug`'s rows again after a change.
fn reread(ui: &mut Ui, slug: &str) {
    let pane = &mut ui.skills;
    if pane.fixture.is_some() {
        return;
    }
    if let Some(skill) = pane.skills.iter().find(|s| s.slug == slug) {
        let rows = rows(Paths::current().as_ref(), skill);
        pane.agents.insert(slug.to_owned(), rows);
    }
    pane.changed();
}

/// Install or Update: writes Apprentice's copy into one agent.
fn put(ui: &mut Ui, slug: &str, agent: Agent) {
    if !super::allowed(ui) {
        return;
    }
    let pane = &mut ui.skills;
    let Some(skill) = pane.skills.iter().find(|s| s.slug == slug).cloned() else {
        return;
    };
    pane.error = None;
    if pane.fixture.is_some() {
        set_state(ui, slug, agent, Installed::Current);
        return;
    }
    let result = Paths::current()
        .ok_or_else(|| tr("Apprentice couldn't find your home folder").to_owned())
        .and_then(|paths| install::install(&paths, agent, &skill));
    if let Err(error) = result {
        ui.skills.error = Some(tr_fill("Couldn't install the skill: {}", &error));
    }
    reread(ui, slug);
}

/// Remove: takes Apprentice's copy out of one agent.
fn take_out(ui: &mut Ui, slug: &str, agent: Agent) {
    ui.skills.error = None;
    if ui.skills.fixture.is_some() {
        set_state(ui, slug, agent, Installed::Missing);
        return;
    }
    let result = Paths::current()
        .ok_or_else(|| tr("Apprentice couldn't find your home folder").to_owned())
        .and_then(|paths| install::remove(&paths, agent, slug));
    if let Err(error) = result {
        ui.skills.error = Some(tr_fill("Couldn't remove the skill: {}", &error));
    }
    reread(ui, slug);
}

/// A scene's stand-in for a read after a change.
fn set_state(ui: &mut Ui, slug: &str, agent: Agent, state: Installed) {
    if let Some(rows) = ui.skills.agents.get_mut(slug) {
        for row in rows.iter_mut().filter(|row| row.agent == agent) {
            row.state = state;
        }
    }
}

/// Delete skill: the first click asks; the second removes Apprentice's own
/// copies from every agent, then the skill.
fn delete(ui: &mut Ui, slug: &str) {
    let pane = &mut ui.skills;
    if pane.armed.as_ref() != Some(&Armed::Skill(slug.to_owned())) {
        pane.armed = Some(Armed::Skill(slug.to_owned()));
        return;
    }
    pane.armed = None;
    let ours: Vec<Agent> = pane
        .agents
        .get(slug)
        .into_iter()
        .flatten()
        .filter(|row| matches!(row.state, Installed::Current | Installed::Older))
        .map(|row| row.agent)
        .collect();
    if pane.fixture.is_none() {
        let paths = Paths::current();
        let skill = pane.skills.iter().find(|s| s.slug == slug).cloned();
        for agent in ours {
            // Agents that share a folder (Codex and Pi) lose their copy with
            // the first removal; read each one again first.
            let still_ours = |paths: &Paths| {
                skill.as_ref().is_none_or(|skill| {
                    install::status(paths, skill).into_iter().any(|(a, state)| {
                        a == agent && matches!(state, Installed::Current | Installed::Older)
                    })
                })
            };
            let removed = paths
                .as_ref()
                .ok_or_else(|| tr("Apprentice couldn't find your home folder").to_owned())
                .and_then(|paths| {
                    if still_ours(paths) {
                        install::remove(paths, agent, slug)
                    } else {
                        Ok(())
                    }
                });
            if let Err(error) = removed {
                pane.error = Some(tr_fill("Couldn't remove the skill: {}", &error));
                pane.changed();
                return;
            }
        }
        if let Some(store) = &pane.store
            && let Err(error) = store.delete_skill(slug)
        {
            pane.error = Some(tr_fill("Couldn't delete the skill: {}", &error.to_string()));
            return;
        }
    }
    pane.skills.retain(|s| s.slug != slug);
    pane.agents.remove(slug);
    if pane.expanded.as_deref() == Some(slug) {
        pane.expanded = None;
    }
    pane.changed();
}

fn toggle(ui: &mut Ui, slug: &str) {
    let pane = &mut ui.skills;
    pane.expanded = if pane.expanded.as_deref() == Some(slug) {
        None
    } else {
        Some(slug.to_owned())
    };
    pane.armed = None;
}

/// "Installed in Claude Code, OpenCode", or where it is not yet.
fn summary(rows: &[AgentRow]) -> String {
    let installed: Vec<&str> = rows
        .iter()
        .filter(|row| matches!(row.state, Installed::Current | Installed::Older))
        .map(|row| row.agent.name())
        .collect();
    if installed.is_empty() {
        tr("Not installed in any agent").to_owned()
    } else {
        tr_fill("Installed in {}", &installed.join(", "))
    }
}

/// The Skills section, while any skill is saved.
pub(super) fn section(ui: &Ui, p: Paint) -> Option<El> {
    let pane = &ui.skills;
    if pane.skills.is_empty() {
        return None;
    }
    let cards: Vec<El> = pane
        .skills
        .iter()
        .map(|skill| skill_card(ui, p, skill))
        .collect();
    let mut section = col()
        .key("skills")
        .gap(METRICS.label_gap)
        .child(p.section_label(tr("Skills")).mx(METRICS.row_px));
    if let Some(error) = &pane.error {
        section = section.child(
            text(error.as_str())
                .key("skills-error")
                .font(DESCRIPTION)
                .color(p.danger())
                .wrap()
                .mx(METRICS.row_px),
        );
    }
    Some(section.child(col().key("cards").gap(12.0).children(cards)))
}

fn skill_card(ui: &Ui, p: Paint, skill: &Skill) -> El {
    let pane = &ui.skills;
    let slug = skill.slug.clone();
    let agents = pane
        .agents
        .get(&skill.slug)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let open = pane.expanded.as_deref() == Some(skill.slug.as_str());
    let toggle_slug = slug.clone();
    let mut rows = vec![setting_row(
        p,
        "skill",
        &skill.name,
        Some(&summary(agents)),
        small(
            p,
            "toggle-skill",
            if open { tr("Close") } else { tr("Manage") },
            true,
            move |ui: &mut Ui, _| toggle(ui, &toggle_slug),
        ),
    )];
    if !open {
        return card(p, &format!("skill-{slug}"), rows);
    }
    for row in agents {
        rows.push(agent_row(p, &slug, row));
    }
    rows.push(note(
        "agents-note",
        tr("An agent may send this skill's text to its own model, which can be a cloud model"),
        p.muted(),
    ));
    rows.push(skill_actions(ui, p, &slug));
    card(p, &format!("skill-{slug}"), rows)
}

/// One agent: what it holds, and the click that changes it.
fn agent_row(p: Paint, slug: &str, agent: &AgentRow) -> El {
    let key = format!("agent-{}", agent.agent.key());
    let name = agent.agent.name();
    let (label, controls): (&str, Vec<El>) = match (agent.found, agent.state) {
        (_, Installed::Linked) => (tr("Linked elsewhere"), Vec::new()),
        (_, Installed::Foreign) => (tr("Not Apprentice's"), Vec::new()),
        (false, Installed::Missing) => (tr("Not found"), Vec::new()),
        (true, Installed::Missing) => (
            tr("Not installed"),
            vec![action(p, "install", tr("Install"), slug, agent.agent, put)],
        ),
        (_, Installed::Current) => (
            tr("Installed"),
            vec![action(
                p,
                "remove",
                tr("Remove"),
                slug,
                agent.agent,
                take_out,
            )],
        ),
        (_, Installed::Older) => (
            tr("Older version installed"),
            vec![
                action(p, "update", tr("Update"), slug, agent.agent, put),
                action(p, "remove", tr("Remove"), slug, agent.agent, take_out),
            ],
        ),
    };
    setting_row(
        p,
        &key,
        name,
        Some(label),
        row().key("controls").gap(8.0).children(controls),
    )
}

fn action(
    p: Paint,
    key: &str,
    label: &str,
    slug: &str,
    agent: Agent,
    run: fn(&mut Ui, &str, Agent),
) -> El {
    let slug = slug.to_owned();
    small(p, key, label, true, move |ui: &mut Ui, _| {
        run(ui, &slug, agent)
    })
}

/// Edit and Delete skill, or Delete skill's confirmation.
fn skill_actions(ui: &Ui, p: Paint, slug: &str) -> El {
    let armed = ui.skills.armed.as_ref() == Some(&Armed::Skill(slug.to_owned()));
    let edit_slug = slug.to_owned();
    let delete_slug = slug.to_owned();
    let buttons = row()
        .key("skill-actions")
        .px(METRICS.row_px)
        .py(14.0)
        .gap(8.0)
        .center_y();
    if armed {
        return buttons
            .child(button(
                p,
                "confirm-delete-skill",
                tr("Delete skill"),
                ButtonKind::Danger,
                move |ui: &mut Ui, _| delete(ui, &delete_slug),
            ))
            .child(button(
                p,
                "cancel-delete-skill",
                tr("Cancel"),
                ButtonKind::Secondary,
                |ui: &mut Ui, _| ui.skills.armed = None,
            ));
    }
    buttons
        .child(button(
            p,
            "edit-skill",
            tr("Edit"),
            ButtonKind::Secondary,
            move |ui: &mut Ui, _| super::draft_view::open_skill(ui, &edit_slug),
        ))
        .child(button(
            p,
            "delete-skill",
            tr("Delete skill…"),
            ButtonKind::Secondary,
            move |ui: &mut Ui, _| delete(ui, &delete_slug),
        ))
}
