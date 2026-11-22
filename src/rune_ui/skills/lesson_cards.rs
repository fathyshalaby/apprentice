//! The Skills pane's new lessons: one card each with its length and age,
//! what drafting reads and with which model, and Draft skill, or the
//! running draft with Cancel, or why the last one stopped with Try again.
//! An expired lesson says so and can only be discarded.

use rune::{El, col, row, text};

use super::super::Ui;
use super::super::controls::{ButtonKind, DESCRIPTION, METRICS, Paint, button, card};
use super::{Armed, Failure, Primary, Stage, cancel_draft, discard_lesson, draft};
use crate::app_settings::ModelChoice;
use crate::apprentice::lessons::{Lesson, Media};
use crate::desktop::i18n::{event_age, tr, tr_fill, tr_named};

/// The tallest the model's unread answer shows before it scrolls.
const RAW_HEIGHT: f32 = 160.0;

/// The New lessons section, while any lesson waits.
pub(super) fn section(ui: &Ui, p: Paint, primary: &Primary) -> Option<El> {
    let pane = &ui.skills;
    let cards: Vec<El> = pane
        .new_lessons()
        .map(|lesson| lesson_card(ui, p, lesson, primary))
        .collect();
    if cards.is_empty() {
        return None;
    }
    Some(
        col()
            .key("lessons")
            .gap(METRICS.label_gap)
            .child(p.section_label(tr("New lessons")).mx(METRICS.row_px))
            .child(col().key("cards").gap(12.0).children(cards)),
    )
}

/// "New lesson · 2:14 · 5m ago".
fn title(lesson: &Lesson) -> String {
    let length =
        super::super::screenshot_thumbnail::editor::clip::duration_label(lesson.duration_ms);
    if lesson.duration_ms == 0 {
        return format!("{} · {}", tr("New lesson"), event_age(lesson.created_ms));
    }
    format!(
        "{} · {length} · {}",
        tr("New lesson"),
        event_age(lesson.created_ms)
    )
}

/// What drafting reads, and with which model. An Ollama model reads
/// screenshots only once the picker knows it sees images (`vision`).
fn reads(choice: Option<&ModelChoice>, vision: Option<bool>) -> String {
    let text = tr("Reads: narration, names of what you click, window titles");
    let screen = tr("Reads: narration, names of what you click, window titles, screenshots");
    match (choice, vision) {
        (Some(ModelChoice::AppleIntelligence), _) => {
            format!("{text} · Apple Intelligence · {}", tr("text only"))
        }
        (Some(ModelChoice::Ollama { model }), Some(true)) => format!("{screen} · {model}"),
        (Some(ModelChoice::Ollama { model }), Some(false)) => {
            format!("{text} · {model} · {}", tr("text only"))
        }
        (Some(ModelChoice::Ollama { model }), None) => format!("{text} · {model}"),
        (None, _) => screen.to_owned(),
    }
}

/// What the lesson left too little of, once its notes are read.
fn thin_note(ui: &Ui, lesson: &Lesson) -> Option<String> {
    let thin = ui.skills.notes(&lesson.id)?.thin;
    let mut missing = Vec::new();
    if thin.no_narration {
        missing.push(tr("no narration"));
    }
    if thin.no_click_names {
        missing.push(tr("no click names"));
    }
    (!missing.is_empty()).then(|| tr_fill("Little to learn from: {}", &missing.join(", ")))
}

fn line(key: &str, copy: impl Into<String>, color: rune::Color) -> El {
    text(copy.into())
        .key(key)
        .font(DESCRIPTION)
        .color(color)
        .wrap()
}

fn lesson_card(ui: &Ui, p: Paint, lesson: &Lesson, primary: &Primary) -> El {
    let pane = &ui.skills;
    let id = lesson.id.clone();
    let choice = ui.settings.apprentice.choice();
    let vision = match &choice {
        Some(ModelChoice::Ollama { model }) => pane.picker.sees_images(model),
        _ => None,
    };
    let drafting = pane.drafting.as_ref().filter(|d| d.lesson == lesson.id);
    let failed = pane
        .failed
        .as_ref()
        .filter(|(failed, _)| *failed == lesson.id)
        .map(|(_, failure)| failure);
    let mut block = col()
        .key("block")
        .px(METRICS.row_px)
        .py(14.0)
        .gap(6.0)
        .child(
            text(title(lesson))
                .key("title")
                .font(p.k.ty.ui_medium)
                .color(p.text()),
        );
    let mut buttons = row().key("buttons").gap(8.0).pt(6.0).center_y();
    let armed = pane.armed.as_ref() == Some(&Armed::Lesson(lesson.id.clone()));
    if lesson.media == Media::Expired {
        block = block.child(line("expired", tr("This lesson expired"), p.muted()));
    } else if let Some(drafting) = drafting {
        let stage = match &drafting.stage {
            Stage::Starting => tr("Drafting…").to_owned(),
            Stage::Part { n, of } => tr_named(
                "Drafting · part {n} of {m}",
                &[("n", &n.to_string()), ("m", &of.to_string())],
            ),
            Stage::Merging => tr("Putting it together…").to_owned(),
            Stage::Stopping => tr("Stopping…").to_owned(),
        };
        block = block
            .child(line("reads", reads(choice.as_ref(), vision), p.muted()))
            .child(line("stage", stage, p.text()));
        let stopping = drafting.stage == Stage::Stopping;
        buttons = buttons.child(super::super::controls::button_enabled(
            p,
            "cancel-draft",
            tr("Cancel"),
            ButtonKind::Secondary,
            !stopping,
            |ui: &mut Ui, _| cancel_draft(ui),
        ));
        return card(p, &format!("lesson-{id}"), vec![block.child(buttons)]);
    } else {
        block = block.child(line("reads", reads(choice.as_ref(), vision), p.muted()));
        if let Some(note) = thin_note(ui, lesson) {
            block = block.child(line("thin", note, p.warning()));
        }
        match failed {
            Some(Failure::Message(message)) => {
                block = block.child(line("failed", message.clone(), p.danger()));
            }
            Some(Failure::Unparsed(raw)) => {
                block = block
                    .child(line(
                        "failed",
                        tr("The model's answer couldn't be read"),
                        p.danger(),
                    ))
                    .child(raw_answer(p, raw));
            }
            None => {}
        }
        let kind = if *primary == Primary::Draft(lesson.id.clone()) {
            ButtonKind::Primary
        } else {
            ButtonKind::Secondary
        };
        let draft_id = id.clone();
        buttons = buttons.child(match (&choice, failed) {
            (None, _) => button(
                p,
                "choose-model-first",
                tr("Choose a model first"),
                kind,
                |ui: &mut Ui, _| super::model_picker::open(ui),
            ),
            (Some(_), Some(_)) => button(
                p,
                "try-again",
                tr("Try again"),
                kind,
                move |ui: &mut Ui, _| draft(ui, &draft_id),
            ),
            (Some(_), None) => button(
                p,
                "draft-skill",
                tr("Draft skill"),
                kind,
                move |ui: &mut Ui, _| draft(ui, &draft_id),
            ),
        });
    }
    let discard_id = id.clone();
    // Cancel takes Discard's place and Move to Trash comes after it, so a
    // double click on Discard never discards.
    buttons = if armed {
        buttons
            .child(button(
                p,
                "cancel-discard-lesson",
                tr("Cancel"),
                ButtonKind::Secondary,
                |ui: &mut Ui, _| ui.skills.armed = None,
            ))
            .child(button(
                p,
                "confirm-discard-lesson",
                tr("Move to Trash"),
                ButtonKind::Danger,
                move |ui: &mut Ui, _| discard_lesson(ui, &discard_id),
            ))
    } else {
        buttons.child(button(
            p,
            "discard-lesson",
            tr("Discard"),
            ButtonKind::Secondary,
            move |ui: &mut Ui, _| discard_lesson(ui, &discard_id),
        ))
    };
    card(p, &format!("lesson-{id}"), vec![block.child(buttons)])
}

/// The model's answer that couldn't be read, as it came, in a box that
/// scrolls.
fn raw_answer(p: Paint, raw: &str) -> El {
    let body = text(raw.to_owned())
        .key("raw-text")
        .font(p.k.ty.mono)
        .color(p.muted())
        .wrap()
        .selectable(true);
    col()
        .key("raw")
        .max_h(RAW_HEIGHT)
        .radius(p.k.radius.md)
        .fill(p.k.c.layer_01)
        .border(p.hair, p.k.c.border_weak)
        .clip_soft(0.0)
        .child(
            col()
                .key("raw-scroll")
                .grow(1.0)
                .shrink()
                .min_h(0.0)
                .scroll(rune::ScrollSpec {
                    stick_bottom: false,
                })
                .child(col().key("raw-inset").padding(10.0).child(body)),
        )
}
