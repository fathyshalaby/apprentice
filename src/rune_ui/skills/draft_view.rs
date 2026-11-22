//! The Skills pane's draft view: a drafted skill's fields to read and edit,
//! its why? questions one at a time, and Approve skill; or a saved skill
//! reopened with Edit, whose Save changes is its own approval. Discarding
//! a draft takes a second click in another place; a name already taken
//! asks Replace or Keep both. What those buttons do is in `approval`.

use rune::{El, TextEdit, col, row, text};
use rune_kit::{TextArea, TextInput};

use super::super::Ui;
use super::super::controls::{
    ButtonKind, DESCRIPTION, METRICS, Paint, button, note, section_with, setting_row,
};
use super::super::lists::{Tone, tone_tag};
use super::approval::{answer, approve, discard, save_edit, skip};
use super::small;
use crate::apprentice::skill::{Skill, Step};
use crate::apprentice::store::{Clash, Draft};
use crate::desktop::i18n::{tr, tr_fill, tr_named};

/// Where the open skill comes from and goes back to.
#[derive(Clone, Debug)]
pub(in crate::rune_ui) enum Source {
    /// A draft, approved into a skill.
    Draft {
        id: String,
        lesson_id: String,
        evidence_text: String,
    },
    /// A saved skill, saved again with its version bumped.
    Skill,
}

/// One step's two fields.
pub(in crate::rune_ui) struct StepEdit {
    pub(in crate::rune_ui) title: TextEdit,
    pub(in crate::rune_ui) detail: TextEdit,
}

/// The open skill: its fields as typed, over the skill they came from.
pub(in crate::rune_ui) struct Editor {
    pub(in crate::rune_ui) source: Source,
    /// The skill as opened, with the why? answers given since.
    pub(in crate::rune_ui) base: Skill,
    /// The skill as last saved, to tell unsaved typing apart.
    pub(super) saved: Skill,
    pub(in crate::rune_ui) name: TextEdit,
    pub(in crate::rune_ui) description: TextEdit,
    pub(in crate::rune_ui) when_to_use: TextEdit,
    /// One input per line.
    pub(in crate::rune_ui) inputs: TextEdit,
    pub(in crate::rune_ui) steps: Vec<StepEdit>,
    pub(in crate::rune_ui) verification: TextEdit,
    pub(in crate::rune_ui) edge_cases: TextEdit,
    pub(in crate::rune_ui) answer: TextEdit,
    /// Questions skipped while the draft is open.
    pub(in crate::rune_ui) skipped: Vec<usize>,
    /// The slug a skill already has, while the person picks Replace or
    /// Keep both.
    pub(in crate::rune_ui) clash: Option<String>,
    pub(in crate::rune_ui) discard_armed: bool,
    /// Review or Edit pressed over unsaved typing: what it would open.
    pub(in crate::rune_ui) switching: Option<Opening>,
    pub(in crate::rune_ui) error: Option<String>,
}

impl Editor {
    fn new(source: Source, skill: &Skill) -> Self {
        let mut editor = Self {
            source,
            base: skill.clone(),
            saved: skill.clone(),
            name: TextEdit::new(skill.name.clone()),
            description: TextEdit::new(skill.description.clone()),
            when_to_use: TextEdit::new(skill.when_to_use.clone()),
            inputs: TextEdit::new(skill.inputs.join("\n")),
            steps: skill
                .steps
                .iter()
                .map(|step| StepEdit {
                    title: TextEdit::new(step.title.clone()),
                    detail: TextEdit::new(step.detail.clone()),
                })
                .collect(),
            verification: TextEdit::new(skill.verification.join("\n")),
            edge_cases: TextEdit::new(skill.edge_cases.join("\n")),
            answer: TextEdit::default(),
            skipped: Vec::new(),
            clash: None,
            discard_armed: false,
            switching: None,
            error: None,
        };
        // As the fields read it back, so untouched trimming isn't a change.
        editor.saved = editor.skill();
        editor
    }

    pub(in crate::rune_ui) fn for_draft(draft: &Draft) -> Self {
        Self::new(
            Source::Draft {
                id: draft.id.clone(),
                lesson_id: draft.lesson_id.clone(),
                evidence_text: draft.evidence_text.clone(),
            },
            &draft.skill,
        )
    }

    pub(in crate::rune_ui) fn for_skill(skill: &Skill) -> Self {
        Self::new(Source::Skill, skill)
    }

    /// The skill as edited.
    pub(in crate::rune_ui) fn skill(&self) -> Skill {
        let mut skill = self.base.clone();
        let name = self.name.text.trim();
        if !name.is_empty() {
            skill.name = name.to_owned();
        }
        skill.description = self.description.text.trim().to_owned();
        skill.when_to_use = self.when_to_use.text.trim().to_owned();
        skill.inputs = lines(&self.inputs);
        skill.steps = self
            .base
            .steps
            .iter()
            .zip(&self.steps)
            .map(|(step, edit)| Step {
                title: edit.title.text.trim().to_owned(),
                detail: edit.detail.text.trim().to_owned(),
                ..step.clone()
            })
            .collect();
        skill.verification = lines(&self.verification);
        skill.edge_cases = lines(&self.edge_cases);
        skill
    }

    /// Whether anything typed here isn't saved yet.
    pub(in crate::rune_ui) fn unsaved(&self) -> bool {
        self.skill() != self.saved || !self.answer.text.trim().is_empty()
    }

    /// The draft as it would be saved now.
    pub(super) fn draft(&self) -> Option<Draft> {
        let Source::Draft {
            id,
            lesson_id,
            evidence_text,
        } = &self.source
        else {
            return None;
        };
        Some(Draft {
            id: id.clone(),
            lesson_id: lesson_id.clone(),
            skill: self.skill(),
            evidence_text: evidence_text.clone(),
        })
    }

    /// The step whose why? question shows: the first unanswered and not
    /// skipped.
    pub(in crate::rune_ui) fn question(&self) -> Option<usize> {
        self.base
            .steps
            .iter()
            .enumerate()
            .position(|(index, step)| {
                step.question.is_some() && step.why.is_none() && !self.skipped.contains(&index)
            })
    }

    /// Answered and asked why? questions.
    fn answered(&self) -> (usize, usize) {
        let asked = self.base.steps.iter().filter(|s| s.question.is_some());
        let answered = asked.clone().filter(|s| s.why.is_some()).count();
        (answered, asked.count())
    }
}

/// A list field's lines, trimmed, empty ones dropped.
fn lines(edit: &TextEdit) -> Vec<String> {
    edit.text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The open editor's field `pick`, or a spare when none is open, so a late
/// keystroke never lands anywhere that matters.
fn field(ui: &mut Ui, pick: impl Fn(&mut Editor) -> &mut TextEdit) -> &mut TextEdit {
    let pane = &mut ui.skills;
    match &mut pane.editor {
        Some(editor) => pick(editor),
        None => &mut pane.scratch,
    }
}

/// Step `index`'s title or detail, or the spare.
fn step_field(ui: &mut Ui, index: usize, detail: bool) -> &mut TextEdit {
    let pane = &mut ui.skills;
    match &mut pane.editor {
        Some(editor) if index < editor.steps.len() => {
            let step = &mut editor.steps[index];
            if detail {
                &mut step.detail
            } else {
                &mut step.title
            }
        }
        _ => &mut pane.scratch,
    }
}

/// What Review or Edit asks to open in place of the open editor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::rune_ui) enum Opening {
    Draft(String),
    Skill(String),
}

/// Opens a saved draft or skill in the view. Over unsaved typing it asks
/// first, in the open editor; `confirmed` is the answer.
fn open(ui: &mut Ui, opening: Opening, confirmed: bool) {
    let pane = &mut ui.skills;
    if !confirmed
        && let Some(editor) = &mut pane.editor
        && editor.unsaved()
    {
        editor.switching = Some(opening);
        return;
    }
    let editor = match &opening {
        Opening::Draft(id) => pane
            .drafts
            .iter()
            .find(|d| &d.id == id)
            .map(Editor::for_draft),
        Opening::Skill(slug) => pane
            .skills
            .iter()
            .find(|s| &s.slug == slug)
            .map(Editor::for_skill),
    };
    if editor.is_some() {
        pane.editor = editor;
    }
}

pub(super) fn open_draft(ui: &mut Ui, id: &str) {
    open(ui, Opening::Draft(id.to_owned()), false);
}

pub(super) fn open_skill(ui: &mut Ui, slug: &str) {
    open(ui, Opening::Skill(slug.to_owned()), false);
}

/// The question Review or Edit asked over unsaved typing, at the top of
/// the open editor.
fn switching_rows(p: Paint, editor: &Editor) -> Option<El> {
    let opening = editor.switching.clone()?;
    Some(
        col()
            .key("switching")
            .px(METRICS.row_px)
            .py(12.0)
            .gap(8.0)
            .child(
                text(tr_fill(
                    "Discard your changes to {}?",
                    editor.name.text.trim(),
                ))
                .key("question")
                .font(p.k.ty.ui_medium)
                .color(p.text())
                .wrap(),
            )
            .child(
                row()
                    .key("choices")
                    .gap(8.0)
                    .child(button(
                        p,
                        "confirm-switch",
                        tr("Discard changes"),
                        ButtonKind::Danger,
                        move |ui: &mut Ui, _| open(ui, opening.clone(), true),
                    ))
                    .child(button(
                        p,
                        "keep-editing",
                        tr("Keep editing"),
                        ButtonKind::Secondary,
                        |ui: &mut Ui, _| {
                            if let Some(editor) = &mut ui.skills.editor {
                                editor.switching = None;
                            }
                        },
                    )),
            ),
    )
}

/// A labelled field: the label over its input.
fn labelled(p: Paint, key: &str, label: &str, input: El) -> El {
    col()
        .key(key)
        .px(METRICS.row_px)
        .py(10.0)
        .gap(6.0)
        .child(
            text(label)
                .key("label")
                .font(p.k.ty.ui)
                .color(p.text())
                .a11y_hidden(),
        )
        .child(input)
}

fn area(
    p: Paint,
    edit: &TextEdit,
    label: &str,
    rows: usize,
    on_edit: impl for<'a> Fn(&'a mut Ui) -> &'a mut TextEdit + 'static,
) -> El {
    TextArea::new(edit)
        .label(label)
        .rows(rows)
        .width(None)
        .on_edit(on_edit)
        .build(p.k)
        .key("input")
}

/// The open draft or skill.
pub(super) fn section(ui: &Ui, p: Paint) -> Option<El> {
    let editor = ui.skills.editor.as_ref()?;
    let drafting = matches!(editor.source, Source::Draft { .. });
    let mut rows: Vec<El> = switching_rows(p, editor).into_iter().collect();
    rows.extend(why_rows(p, editor));
    rows.extend([
        labelled(
            p,
            "name",
            tr("Name"),
            TextInput::new(&editor.name)
                .label(tr("Name"))
                .width(None)
                .on_edit(|ui: &mut Ui| field(ui, |e| &mut e.name))
                .build(p.k)
                .key("input"),
        ),
        labelled(
            p,
            "description",
            tr("Description"),
            area(p, &editor.description, tr("Description"), 2, |ui| {
                field(ui, |e| &mut e.description)
            }),
        ),
        labelled(
            p,
            "when-to-use",
            tr("When to use"),
            area(p, &editor.when_to_use, tr("When to use"), 2, |ui| {
                field(ui, |e| &mut e.when_to_use)
            }),
        ),
    ]);
    rows.push(labelled(
        p,
        "inputs",
        tr("Inputs (one per line)"),
        area(p, &editor.inputs, tr("Inputs (one per line)"), 2, |ui| {
            field(ui, |e| &mut e.inputs)
        }),
    ));
    for (index, edit) in editor.steps.iter().enumerate() {
        rows.push(step_rows(p, editor, index, edit));
    }
    rows.push(labelled(
        p,
        "verification",
        tr("How to check it worked (one per line)"),
        area(
            p,
            &editor.verification,
            tr("How to check it worked (one per line)"),
            3,
            |ui| field(ui, |e| &mut e.verification),
        ),
    ));
    rows.push(labelled(
        p,
        "edge-cases",
        tr("Edge cases (one per line)"),
        area(
            p,
            &editor.edge_cases,
            tr("Edge cases (one per line)"),
            3,
            |ui| field(ui, |e| &mut e.edge_cases),
        ),
    ));
    if let Some(slug) = &editor.clash {
        rows.push(clash_rows(p, slug));
    }
    if let Some(error) = &editor.error {
        rows.push(note("draft-error", error, p.danger()));
    }
    rows.push(actions(p, editor, drafting));
    let (answered, asked) = editor.answered();
    let counter = if asked == 0 {
        row().key("why-count")
    } else {
        tone_tag(
            p,
            "why-count",
            &tr_named(
                "{n} of {m} why? answered",
                &[("n", &answered.to_string()), ("m", &asked.to_string())],
            ),
            if answered == asked {
                Tone::Success
            } else {
                Tone::Neutral
            },
        )
    };
    let title = if drafting {
        tr_fill("Draft · {}", &editor.base.name)
    } else {
        tr_fill("Editing · {}", &editor.base.name)
    };
    Some(section_with(p, "draft", &title, counter, rows))
}

/// Step `index`: its title and what to do, and its decision's answer.
fn step_rows(p: Paint, editor: &Editor, index: usize, edit: &StepEdit) -> El {
    let label = tr_fill("Step {}", &(index + 1).to_string());
    let mut block = col()
        .key(format!("step{index}"))
        .px(METRICS.row_px)
        .py(10.0)
        .gap(6.0)
        .child(
            text(label.clone())
                .key("label")
                .font(p.k.ty.ui)
                .color(p.text())
                .a11y_hidden(),
        )
        .child(
            TextInput::new(&edit.title)
                .label(label.clone())
                .width(None)
                .on_edit(move |ui: &mut Ui| step_field(ui, index, false))
                .build(p.k)
                .key("title"),
        )
        .child(
            TextArea::new(&edit.detail)
                .label(tr_fill("Step {} details", &(index + 1).to_string()))
                .rows(2)
                .width(None)
                .on_edit(move |ui: &mut Ui| step_field(ui, index, true))
                .build(p.k)
                .key("detail"),
        );
    let step = &editor.base.steps[index];
    if let (Some(question), Some(why)) = (&step.question, &step.why) {
        block = block.child(
            text(format!("{question} {why}"))
                .key("why")
                .font(DESCRIPTION)
                .color(p.muted())
                .wrap(),
        );
    }
    block
}

/// The why? question showing now, with its answer field, Answer, and Skip.
fn why_rows(p: Paint, editor: &Editor) -> Option<El> {
    let index = editor.question()?;
    let question = editor.base.steps[index].question.clone()?;
    let field = TextInput::new(&editor.answer)
        .label(question.clone())
        .placeholder(tr("Your answer…"))
        .width(None)
        .on_edit(|ui: &mut Ui| field(ui, |e| &mut e.answer))
        .on_submit(|ui: &mut Ui, _| answer(ui))
        .build(p.k)
        .key("answer")
        .grow(1.0);
    Some(
        col()
            .key("why")
            .px(METRICS.row_px)
            .py(12.0)
            .gap(8.0)
            .child(
                text(format!(
                    "{} · {}",
                    tr_fill("Step {}", &(index + 1).to_string()),
                    editor.steps[index].title.text.trim()
                ))
                .key("about")
                .font(DESCRIPTION)
                .color(p.muted()),
            )
            .child(
                text(question)
                    .key("question")
                    .font(p.k.ty.ui_medium)
                    .color(p.text())
                    .wrap(),
            )
            .child(
                row()
                    .key("answer-row")
                    .gap(8.0)
                    .center_y()
                    .child(field)
                    .child(small(
                        p,
                        "answer-why",
                        tr("Answer"),
                        true,
                        |ui: &mut Ui, _| answer(ui),
                    ))
                    .child(small(p, "skip-why", tr("Skip"), true, |ui: &mut Ui, _| {
                        skip(ui)
                    })),
            ),
    )
}

fn clash_rows(p: Paint, slug: &str) -> El {
    setting_row(
        p,
        "clash",
        &tr_fill("A skill named {} already exists", slug),
        None,
        row()
            .key("clash-choices")
            .gap(8.0)
            .child(small(
                p,
                "replace-skill",
                tr("Replace"),
                true,
                |ui: &mut Ui, _| approve(ui, Some(Clash::Replace)),
            ))
            .child(small(
                p,
                "keep-both",
                tr("Keep both"),
                true,
                |ui: &mut Ui, _| approve(ui, Some(Clash::KeepBoth)),
            )),
    )
}

fn actions(p: Paint, editor: &Editor, drafting: bool) -> El {
    let mut buttons = row()
        .key("draft-actions")
        .px(METRICS.row_px)
        .py(14.0)
        .gap(8.0)
        .center_y();
    if !drafting {
        return buttons
            .child(button(
                p,
                "save-skill",
                tr("Save changes"),
                ButtonKind::Primary,
                |ui: &mut Ui, _| save_edit(ui),
            ))
            .child(button(
                p,
                "close-skill",
                tr("Cancel"),
                ButtonKind::Secondary,
                |ui: &mut Ui, _| ui.skills.editor = None,
            ));
    }
    buttons = buttons.child(button(
        p,
        "approve-skill",
        tr("Approve skill"),
        ButtonKind::Primary,
        |ui: &mut Ui, _| approve(ui, None),
    ));
    // Cancel takes Discard's place and the confirmation comes after it, so
    // a double click on Discard never discards.
    if editor.discard_armed {
        buttons
            .child(button(
                p,
                "cancel-discard-draft",
                tr("Cancel"),
                ButtonKind::Secondary,
                |ui: &mut Ui, _| {
                    if let Some(editor) = &mut ui.skills.editor {
                        editor.discard_armed = false;
                    }
                },
            ))
            .child(button(
                p,
                "confirm-discard-draft",
                tr("Discard draft"),
                ButtonKind::Danger,
                |ui: &mut Ui, _| discard(ui),
            ))
    } else {
        buttons.child(button(
            p,
            "discard-draft",
            tr("Discard"),
            ButtonKind::Secondary,
            |ui: &mut Ui, _| discard(ui),
        ))
    }
}

/// Drafts not open in the view, each with Review.
pub(super) fn other_drafts(ui: &Ui, p: Paint) -> Option<El> {
    let pane = &ui.skills;
    let open = match pane.editor.as_ref().map(|e| &e.source) {
        Some(Source::Draft { id, .. }) => Some(id.as_str()),
        _ => None,
    };
    let rows: Vec<El> = pane
        .drafts
        .iter()
        .filter(|draft| Some(draft.id.as_str()) != open)
        .map(|draft| {
            let id = draft.id.clone();
            setting_row(
                p,
                &format!("draft-{}", draft.id),
                &draft.skill.name,
                Some(&tr_fill("{} steps", &draft.skill.steps.len().to_string())),
                small(
                    p,
                    "review-draft",
                    tr("Review"),
                    true,
                    move |ui: &mut Ui, _| open_draft(ui, &id),
                ),
            )
        })
        .collect();
    (!rows.is_empty())
        .then(|| super::super::controls::section(p, "drafts", tr("Drafts waiting"), rows))
}
