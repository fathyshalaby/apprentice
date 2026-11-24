//! The Skills pane's approved skills: one card each, with the bounded plan
//! Apprentice recorded from its lesson. A person reads the plan, confirms it,
//! and only then may Apprentice perform it. Dry run checks the same bounds
//! without acting, and Stop can end a run before its next step.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use rune::{El, col, row, text};

use super::super::Ui;
use super::super::controls::{
    ButtonKind, DESCRIPTION, METRICS, Paint, button, button_enabled, card, note, setting_row,
};
use super::{Armed, RunCommand, RunOutcome, RunStage, Running, small};
use crate::apprentice::execution::{self, RealDriver};
use crate::apprentice::skill::Skill;
use crate::desktop::i18n::{tr, tr_fill, tr_named};

/// Delete skill: the first click asks; the second removes the skill.
fn delete(ui: &mut Ui, slug: &str) {
    let pane = &mut ui.skills;
    if pane.armed.as_ref() != Some(&Armed::Skill(slug.to_owned())) {
        pane.armed = Some(Armed::Skill(slug.to_owned()));
        return;
    }
    pane.armed = None;
    // Nothing is removed while the skill itself can't be.
    if let Err(message) = pane.ready_store() {
        pane.error = Some(message);
        return;
    }
    if let Some(store) = &pane.store
        && let Err(error) = store.delete_skill(slug)
    {
        pane.error = Some(tr_fill("Couldn't delete the skill: {}", &error.to_string()));
        return;
    }
    pane.skills.retain(|s| s.slug != slug);
    pane.outcomes.remove(slug);
    if pane.run_confirm.as_deref() == Some(slug) {
        pane.run_confirm = None;
    }
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
    pane.run_confirm = None;
}

/// Run is two clicks: the first shows exactly what would happen, the second
/// starts it. A scene records the click and shows a run with no real driver.
fn confirm(ui: &mut Ui, slug: &str, dry: bool) {
    if !super::allowed(ui) || ui.skills.running.is_some() {
        return;
    }
    let Some(skill) = ui.skills.skills.iter().find(|s| s.slug == slug).cloned() else {
        return;
    };
    let Some(plan) = skill.run_plan.clone() else {
        return;
    };
    let pane = &mut ui.skills;
    pane.error = None;
    pane.armed = None;
    pane.run_confirm = None;
    pane.outcomes.remove(slug);
    let of = plan.steps.len();
    if dry {
        if let Err(message) = execution::validate(&plan) {
            pane.outcomes
                .insert(slug.to_owned(), RunOutcome::Failed(message));
            pane.changed();
            return;
        }
        if let Some(recorded) = &mut pane.fixture {
            recorded.runs.push(RunCommand::Dry(slug.to_owned()));
        }
        pane.outcomes
            .insert(slug.to_owned(), RunOutcome::Dry { of });
        pane.changed();
        return;
    }
    if let Err(message) = execution::validate(&plan) {
        pane.outcomes
            .insert(slug.to_owned(), RunOutcome::Failed(message));
        pane.changed();
        return;
    }
    let cancel = Arc::new(AtomicBool::new(false));
    let rx = match &mut pane.fixture {
        Some(recorded) => {
            recorded.runs.push(RunCommand::Run(slug.to_owned()));
            None
        }
        None => Some(execution::execute(
            slug,
            plan,
            Arc::new(RealDriver),
            cancel.clone(),
        )),
    };
    pane.running = Some(Running {
        slug: slug.to_owned(),
        stage: RunStage::Starting,
        rx,
        cancel,
    });
    pane.changed();
}

fn ask(ui: &mut Ui, slug: &str) {
    if !super::allowed(ui) || ui.skills.running.is_some() {
        return;
    }
    let has_plan = ui
        .skills
        .skills
        .iter()
        .find(|s| s.slug == slug)
        .is_some_and(|s| s.run_plan.is_some());
    if !has_plan {
        return;
    }
    ui.skills.armed = None;
    ui.skills.run_confirm = Some(slug.to_owned());
}

/// Stop refuses another step. The run's own last word lands through
/// `poll_running`, so the card shows a typed outcome rather than a guess.
fn stop(ui: &mut Ui, slug: &str) {
    let Some(running) = &mut ui.skills.running else {
        return;
    };
    if running.slug != slug {
        return;
    }
    running.cancel.store(true, Ordering::Release);
    running.stage = RunStage::Stopping;
    if let Some(recorded) = &mut ui.skills.fixture {
        recorded.runs.push(RunCommand::Stop(slug.to_owned()));
        ui.skills.running = None;
        ui.skills
            .outcomes
            .insert(slug.to_owned(), RunOutcome::Cancelled);
    }
    ui.skills.changed();
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
    let open = pane.expanded.as_deref() == Some(skill.slug.as_str());
    let toggle_slug = slug.clone();
    let mut rows = vec![setting_row(
        p,
        "skill",
        &skill.name,
        Some(&summary(ui, skill)),
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
    if let Some(plan) = &skill.run_plan {
        rows.push(note(
            "plan-note",
            tr("Only these steps will run."),
            p.muted(),
        ));
        for (index, step) in plan.steps.iter().take(execution::MAX_STEPS).enumerate() {
            rows.push(
                text(step.label())
                    .key(format!("plan-step-{index}"))
                    .font(DESCRIPTION)
                    .color(p.text())
                    .wrap()
                    .mx(METRICS.row_px),
            );
        }
        rows.push(run_controls(ui, p, skill));
    } else {
        rows.push(note(
            "no-plan-note",
            tr("This skill has no steps Apprentice can run."),
            p.muted(),
        ));
    }
    rows.push(skill_actions(ui, p, &slug));
    card(p, &format!("skill-{slug}"), rows)
}

fn summary(ui: &Ui, skill: &Skill) -> String {
    let pane = &ui.skills;
    if pane.running.as_ref().is_some_and(|r| r.slug == skill.slug) {
        return stage(pane.running.as_ref().unwrap());
    }
    if let Some(outcome) = pane.outcomes.get(&skill.slug) {
        return outcome_text(outcome);
    }
    let Some(plan) = &skill.run_plan else {
        return tr("No runnable steps recorded").to_owned();
    };
    if plan.steps.is_empty() {
        return tr("No runnable steps recorded").to_owned();
    }
    if plan.steps.len() > execution::MAX_STEPS {
        return tr("Too many steps to run safely").to_owned();
    }
    tr_fill("{} steps ready", &plan.steps.len().to_string())
}

fn stage(running: &Running) -> String {
    match &running.stage {
        RunStage::Starting => tr("Starting…").to_owned(),
        RunStage::Step { n, of } => tr_named(
            "Step {n} of {of}",
            &[("n", &n.to_string()), ("of", &of.to_string())],
        ),
        RunStage::Stopping => tr("Stopping…").to_owned(),
    }
}

fn outcome_text(outcome: &RunOutcome) -> String {
    match outcome {
        RunOutcome::Dry { of } => tr_fill("Dry run checked {} steps", &of.to_string()),
        RunOutcome::Done => tr("Finished").to_owned(),
        RunOutcome::Failed(message) => tr_fill("Couldn't run: {}", message),
        RunOutcome::Cancelled => tr("Stopped").to_owned(),
    }
}

fn run_controls(ui: &Ui, p: Paint, skill: &Skill) -> El {
    let pane = &ui.skills;
    let slug = skill.slug.clone();
    let of = skill.run_plan.as_ref().map_or(0, |plan| plan.steps.len());
    let running = pane.running.as_ref();
    let mine = running.is_some_and(|r| r.slug == slug);
    let busy = running.is_some();
    let confirming = pane.run_confirm.as_deref() == Some(slug.as_str());
    let controls = row()
        .key("run-controls")
        .px(METRICS.row_px)
        .py(14.0)
        .gap(8.0)
        .center_y();
    if mine {
        return controls.child(button(
            p,
            "stop-run-skill",
            tr("Stop"),
            ButtonKind::Secondary,
            move |ui: &mut Ui, _| stop(ui, &slug),
        ));
    }
    if confirming {
        let run_slug = slug.clone();
        let dry_slug = slug.clone();
        return col()
            .key("run-confirmation")
            .px(METRICS.row_px)
            .pb(14.0)
            .gap(METRICS.label_gap)
            .child(
                text(tr_fill("Run these {} steps?", &of.to_string()))
                    .key("run-question")
                    .font(DESCRIPTION)
                    .color(p.text()),
            )
            .child(
                controls
                    .child(button(
                        p,
                        "confirm-run-skill",
                        tr("Run skill"),
                        ButtonKind::Primary,
                        move |ui: &mut Ui, _| confirm(ui, &run_slug, false),
                    ))
                    .child(button(
                        p,
                        "dry-run-skill",
                        tr("Dry run"),
                        ButtonKind::Secondary,
                        move |ui: &mut Ui, _| confirm(ui, &dry_slug, true),
                    ))
                    .child(button(
                        p,
                        "cancel-run-skill",
                        tr("Not now"),
                        ButtonKind::Secondary,
                        move |ui: &mut Ui, _| {
                            ui.skills.run_confirm = None;
                        },
                    )),
            );
    }
    let ask_slug = slug.clone();
    button_enabled(
        p,
        "run-skill",
        tr("Run skill…"),
        ButtonKind::Secondary,
        !busy,
        move |ui: &mut Ui, _| ask(ui, &ask_slug),
    )
    .px(METRICS.row_px)
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
