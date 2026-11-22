//! What the draft view's buttons do to the skills store: a why? answer
//! saved into the draft, Approve skill, Save changes on a saved skill, and
//! Discard, which moves a draft to the Trash. In the app, nothing changes
//! on screen while the store can't be opened; a scene's pane, which has
//! none, shows the change only.

use rune::TextEdit;

use super::super::Ui;
use super::draft_view::{Editor, Source};
use crate::apprentice::skill::Skill;
use crate::apprentice::store::{Clash, StoreError};
use crate::desktop::i18n::tr_fill;

/// Saves the open draft as edited; false, with the reason shown, when it
/// can't be. A scene's pane keeps it on screen.
fn persist(ui: &mut Ui) -> bool {
    let pane = &mut ui.skills;
    let opened = pane.ready_store();
    let Some(editor) = &mut pane.editor else {
        return true;
    };
    let Some(draft) = editor.draft() else {
        return true;
    };
    let store = match (opened, &pane.store) {
        (Ok(true), Some(store)) => store,
        (Err(message), _) => {
            editor.error = Some(message);
            return false;
        }
        _ => return true,
    };
    match store.save_draft(&draft) {
        Ok(()) => {
            editor.saved = draft.skill.clone();
            if let Some(shown) = pane.drafts.iter_mut().find(|d| d.id == draft.id) {
                *shown = draft;
            }
            true
        }
        Err(error) => {
            editor.error = Some(tr_fill("Couldn't save the draft: {}", &error.to_string()));
            false
        }
    }
}

/// Answer: the typed reason becomes the step's why.
pub(super) fn answer(ui: &mut Ui) {
    let Some(editor) = &mut ui.skills.editor else {
        return;
    };
    let Some(index) = editor.question() else {
        return;
    };
    let why = editor.answer.text.trim().to_owned();
    if why.is_empty() {
        return;
    }
    editor.base.steps[index].why = Some(why);
    editor.answer = TextEdit::default();
    persist(ui);
}

pub(super) fn skip(ui: &mut Ui) {
    if let Some(editor) = &mut ui.skills.editor
        && let Some(index) = editor.question()
    {
        editor.skipped.push(index);
        editor.answer = TextEdit::default();
    }
}

/// The reason the store can't be opened, shown in the open editor.
fn no_store(ui: &mut Ui, message: String) {
    if let Some(editor) = &mut ui.skills.editor {
        editor.error = Some(message);
    }
}

/// Approve skill: the draft as edited becomes a skill. A taken name asks
/// first; `clash` is the answer.
pub(super) fn approve(ui: &mut Ui, clash: Option<Clash>) {
    let pane = &mut ui.skills;
    let Some(draft) = pane.editor.as_ref().and_then(Editor::draft) else {
        return;
    };
    let opened = pane.ready_store();
    let Some(store) = &pane.store else {
        if let Err(message) = opened {
            return no_store(ui, message);
        }
        // A scene: the draft becomes a skill on screen only.
        pane.drafts.retain(|d| d.id != draft.id);
        pane.expanded = Some(draft.skill.slug.clone());
        pane.skills.retain(|s| s.slug != draft.skill.slug);
        pane.skills.push(draft.skill);
        pane.editor = None;
        pane.changed();
        return;
    };
    let approved = store
        .save_draft(&draft)
        .and_then(|()| store.approve(&draft.id, clash));
    match approved {
        Ok(approved) => {
            pane.drafts.retain(|d| d.id != draft.id);
            pane.skills.retain(|s| s.slug != approved.slug);
            pane.skills.push(approved.skill.clone());
            pane.skills.sort_by_key(|skill| skill.name.to_lowercase());
            pane.agents.insert(
                approved.slug.clone(),
                super::agents::rows(
                    crate::agent_install::Paths::current().as_ref(),
                    &approved.skill,
                ),
            );
            pane.expanded = Some(approved.slug);
            pane.editor = None;
            pane.changed();
        }
        Err(StoreError::Clash(slug)) => {
            if let Some(editor) = &mut pane.editor {
                editor.clash = Some(slug);
            }
        }
        Err(error) => {
            if let Some(editor) = &mut pane.editor {
                editor.error = Some(tr_fill("Couldn't save the skill: {}", &error.to_string()));
            }
        }
    }
}

/// Save changes on a saved skill: its own approval, with a new version.
pub(super) fn save_edit(ui: &mut Ui) {
    let pane = &mut ui.skills;
    let Some(editor) = &pane.editor else {
        return;
    };
    let skill = editor.skill();
    let opened = pane.ready_store();
    let saved = match (&pane.store, opened) {
        (Some(store), _) => store.save_edit(skill),
        (None, Err(message)) => return no_store(ui, message),
        // A scene: saved on screen only.
        (None, Ok(_)) => Ok(Skill {
            version: skill.version + 1,
            ..skill
        }),
    };
    match saved {
        Ok(saved) => {
            if let Some(shown) = pane.skills.iter_mut().find(|s| s.slug == saved.slug) {
                *shown = saved.clone();
            }
            if pane.fixture.is_none() {
                let paths = crate::agent_install::Paths::current();
                pane.agents.insert(
                    saved.slug.clone(),
                    super::agents::rows(paths.as_ref(), &saved),
                );
            }
            pane.editor = None;
            pane.changed();
        }
        Err(error) => {
            if let Some(editor) = &mut pane.editor {
                editor.error = Some(tr_fill("Couldn't save the skill: {}", &error.to_string()));
            }
        }
    }
}

/// Discard on a draft: the first click asks, the second moves it to the
/// Trash.
pub(super) fn discard(ui: &mut Ui) {
    let pane = &mut ui.skills;
    let opened = pane.ready_store();
    let Some(editor) = &mut pane.editor else {
        return;
    };
    if !editor.discard_armed {
        editor.discard_armed = true;
        return;
    }
    let Source::Draft { id, .. } = editor.source.clone() else {
        return;
    };
    if let Err(message) = opened {
        editor.discard_armed = false;
        editor.error = Some(message);
        return;
    }
    if let Some(store) = &pane.store
        && let Err(error) = store.discard_draft(&id)
        && error != StoreError::NotFound
    {
        editor.error = Some(tr_fill(
            "Couldn't discard the draft: {}",
            &error.to_string(),
        ));
        return;
    }
    pane.drafts.retain(|d| d.id != id);
    pane.editor = None;
    pane.changed();
}
