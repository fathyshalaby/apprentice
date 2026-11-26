//! The Skills pane's Model section: the model drafts use, and the picker
//! that lists Ollama's models (marking which see images), the recommended
//! ones not downloaded yet with Download, Apple Intelligence (text only),
//! and Ollama's address, which must be on this computer. Nothing is chosen
//! or downloaded for the person: each is its own click.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};

use rune::{El, TextEdit, row};
use rune_kit::TextInput;

use super::super::Ui;
use super::super::controls::{self, Paint, described_row, note, setting_row};
use super::super::lists::small_button;
use super::{allowed, small};
use crate::app_settings::{ApprenticeSettings, ModelChoice, is_loopback_http_url};
use crate::apple_intelligence::Availability;
use crate::apprentice::models::RECOMMENDED;
use crate::apprentice::ollama::{self, Installed, ModelError, PullProgress};
use crate::desktop::i18n::{tr, tr_fill};

/// What a download's thread reports.
enum PullEvent {
    Progress(PullProgress),
    Done(Result<(), ModelError>),
}

/// A download under way.
pub(in crate::rune_ui) struct Pull {
    pub(in crate::rune_ui) model: String,
    pub(in crate::rune_ui) completed: u64,
    pub(in crate::rune_ui) total: u64,
    /// `None` in scenes.
    rx: Option<Receiver<PullEvent>>,
    cancel: Arc<AtomicBool>,
}

#[derive(Default)]
pub(in crate::rune_ui) struct Picker {
    pub(in crate::rune_ui) open: bool,
    /// Ollama's models, or why they couldn't be listed; `None` while
    /// listing or before the first look.
    pub(in crate::rune_ui) installed: Option<Result<Vec<Installed>, String>>,
    listing: Option<Receiver<Result<Vec<Installed>, ModelError>>>,
    /// Apple Intelligence on this Mac, read when the picker opens.
    pub(in crate::rune_ui) apple: Option<Availability>,
    pub(in crate::rune_ui) url: TextEdit,
    pub(in crate::rune_ui) url_error: Option<String>,
    /// The address the last list and download used.
    listed_url: String,
    pub(in crate::rune_ui) pull: Option<Pull>,
    /// The model whose download failed, and why.
    pub(in crate::rune_ui) pull_error: Option<(String, String)>,
}

impl Picker {
    /// Follows the list and a download.
    pub(super) fn poll(&mut self) -> bool {
        let mut changed = false;
        if let Some(listing) = &self.listing {
            match listing.try_recv() {
                Ok(result) => {
                    self.installed = Some(result.map_err(|error| error.message()));
                    self.listing = None;
                    changed = true;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    self.installed = Some(Err(tr("Ollama isn't running").to_owned()));
                    self.listing = None;
                    changed = true;
                }
            }
        }
        let Some(pull) = &mut self.pull else {
            return changed;
        };
        let Some(rx) = &pull.rx else {
            return changed;
        };
        let done = loop {
            match rx.try_recv() {
                Ok(PullEvent::Progress(progress)) => {
                    pull.completed = progress.completed;
                    pull.total = progress.total;
                    changed = true;
                }
                Ok(PullEvent::Done(result)) => break result,
                Err(TryRecvError::Empty) => return changed,
                Err(TryRecvError::Disconnected) => {
                    break Err(ModelError::Failed(tr("The download stopped").to_owned()));
                }
            }
        };
        let model = pull.model.clone();
        self.pull = None;
        match done {
            Ok(()) => self.list(),
            // A cancelled download just returns to its row.
            Err(ModelError::Cancelled) => {}
            Err(error) => self.pull_error = Some((model, error.message())),
        }
        true
    }

    /// Whether Ollama's `model` sees images, once its models are listed.
    pub(super) fn sees_images(&self, model: &str) -> Option<bool> {
        let installed = self.installed.as_ref()?.as_ref().ok()?;
        installed.iter().find(|m| m.name == model).map(|m| m.vision)
    }

    /// Lists Ollama's models at `url` once, so a lesson can say whether the
    /// chosen one reads screenshots before the picker is ever opened.
    pub(super) fn list_once(&mut self, url: &str) {
        if self.installed.is_none() && self.listing.is_none() {
            self.listed_url = url.to_owned();
            self.list();
        }
    }

    /// Asks Ollama for its models off the window's thread.
    fn list(&mut self) {
        self.installed = None;
        let url = self.listed_url.clone();
        let (send, listing) = mpsc::sync_channel(1);
        let spawned = std::thread::Builder::new()
            .name("apprentice-models".into())
            .spawn(move || {
                let _ = send.send(ollama::installed(&url));
            });
        match spawned {
            Ok(_) => self.listing = Some(listing),
            Err(error) => {
                tracing::warn!(%error, "could not list Ollama's models");
                self.installed = Some(Err(tr("Ollama isn't running").to_owned()));
            }
        }
    }
}

/// Opens the picker (or closes it): the address field shows the saved one,
/// and Ollama and Apple Intelligence are looked at afresh.
pub(super) fn open(ui: &mut Ui) {
    let fixture = ui.skills.fixture.is_some();
    let url = ui.settings.apprentice.ollama_url.clone();
    let picker = &mut ui.skills.picker;
    if picker.open {
        picker.open = false;
        return;
    }
    picker.open = true;
    picker.url = TextEdit::new(url.clone());
    picker.url_error = None;
    if fixture {
        return;
    }
    picker.apple = Some(crate::apple_intelligence::availability());
    picker.listed_url = url;
    picker.list();
}

fn retry(ui: &mut Ui) {
    if ui.skills.fixture.is_none() {
        ui.skills.picker.list();
    }
}

/// Picking a row saves it as the model drafts use; nothing else runs.
fn choose(ui: &mut Ui, choice: &ModelChoice) {
    let model = ApprenticeSettings::model_string(choice);
    if ui.update(|settings| settings.apprentice.model = Some(model)) {
        ui.skills.picker.open = false;
    }
}

/// Clearing the choice returns to no model: Draft skill then asks for one.
fn choose_none(ui: &mut Ui) {
    if ui.update(|settings| settings.apprentice.model = None) {
        ui.skills.picker.open = false;
    }
}

/// Download: the approval for this one pull. One at a time.
fn download(ui: &mut Ui, model: &str) {
    if !allowed(ui) || ui.skills.picker.pull.is_some() {
        return;
    }
    let cancel = Arc::new(AtomicBool::new(false));
    let picker = &mut ui.skills.picker;
    picker.pull_error = None;
    let rx = match &mut ui.skills.fixture {
        Some(recorded) => {
            recorded.pulls.push(model.to_owned());
            None
        }
        None => {
            let (send, rx) = mpsc::channel();
            let url = ui.settings.apprentice.ollama_url.clone();
            picker.listed_url = url.clone();
            let model = model.to_owned();
            let stop = cancel.clone();
            let spawned = std::thread::Builder::new()
                .name("apprentice-pull".into())
                .spawn(move || {
                    let progress = send.clone();
                    let result = ollama::pull(
                        &url,
                        &model,
                        &mut |line| {
                            let _ = progress.send(PullEvent::Progress(line));
                        },
                        &stop,
                    );
                    let _ = send.send(PullEvent::Done(result));
                });
            if let Err(error) = spawned {
                tracing::warn!(%error, "could not start a download");
                return;
            }
            Some(rx)
        }
    };
    picker.pull = Some(Pull {
        model: model.to_owned(),
        completed: 0,
        total: 0,
        rx,
        cancel,
    });
}

fn cancel_download(ui: &mut Ui) {
    let picker = &mut ui.skills.picker;
    if let Some(pull) = &picker.pull {
        pull.cancel.store(true, Ordering::Release);
        if pull.rx.is_none() {
            picker.pull = None;
        }
    }
}

/// Saves Ollama's address once it is checked to be on this computer.
fn save_url(ui: &mut Ui) {
    let url = ui.skills.picker.url.text.trim().to_owned();
    if !is_loopback_http_url(&url) {
        ui.skills.picker.url_error =
            Some(tr("Use an address on this computer, such as http://127.0.0.1:11434").to_owned());
        return;
    }
    ui.skills.picker.url_error = None;
    if ui.update(|settings| settings.apprentice.ollama_url = url.clone()) {
        ui.skills.picker.listed_url = url;
        retry(ui);
    }
}

/// The chosen model as a row's line: its name and what it reads.
pub(super) fn chosen_label(choice: Option<&ModelChoice>) -> String {
    match choice {
        None => tr("None chosen").to_owned(),
        Some(ModelChoice::AppleIntelligence) => {
            format!("Apple Intelligence · {}", tr("text only"))
        }
        Some(ModelChoice::Ollama { model }) => model.clone(),
    }
}

/// A key part made from a model's name: `qwen3-vl:8b` → `qwen3-vl-8b`.
fn key_part(model: &str) -> String {
    model
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

fn gigabytes(bytes: u64) -> String {
    format!("{:.1} GB", bytes as f64 / 1_000_000_000.0)
}

/// The Model section: the chosen model, and the picker while it is open.
pub(super) fn section(ui: &Ui, p: Paint) -> El {
    let picker = &ui.skills.picker;
    let choice = ui.settings.apprentice.choice();
    let mut rows = vec![setting_row(
        p,
        "drafting-model",
        tr("Model for drafting"),
        Some(&chosen_label(choice.as_ref())),
        small_button(
            p,
            "choose-model",
            if picker.open {
                tr("Close")
            } else {
                tr("Choose…")
            },
            |ui: &mut Ui, _| open(ui),
        ),
    )];
    if picker.open {
        rows.extend(picker_rows(ui, p, choice.as_ref()));
    }
    controls::section(p, "model", tr("Model"), rows)
}

/// The explicit no-model row: In use when nothing drafts, else Use to clear
/// the saved choice without changing anything else.
fn none_row(p: Paint, chosen: Option<&ModelChoice>) -> El {
    let control = if chosen.is_none() {
        super::super::lists::tone_tag(
            p,
            "in-use",
            tr("In use"),
            super::super::lists::Tone::Success,
        )
    } else {
        small(p, "use-none", tr("Use"), true, |ui: &mut Ui, _| {
            choose_none(ui)
        })
    };
    setting_row(
        p,
        "model-none",
        tr("None"),
        Some(tr("None chosen")),
        control,
    )
}

/// A model row's control: In use, or Use.
fn use_control(p: Paint, key: String, chosen: bool, enabled: bool, choice: ModelChoice) -> El {
    if chosen {
        return super::super::lists::tone_tag(
            p,
            "in-use",
            tr("In use"),
            super::super::lists::Tone::Success,
        );
    }
    small(p, &key, tr("Use"), enabled, move |ui: &mut Ui, _| {
        choose(ui, &choice)
    })
}

fn picker_rows(ui: &Ui, p: Paint, chosen: Option<&ModelChoice>) -> Vec<El> {
    let picker = &ui.skills.picker;
    let mut rows = vec![none_row(p, chosen)];
    let installed: &[Installed] = match &picker.installed {
        None => {
            rows.push(note(
                "models-loading",
                tr("Looking for Ollama's models…"),
                p.muted(),
            ));
            &[]
        }
        Some(Err(message)) => {
            rows.push(described_row(
                p,
                "ollama-error",
                "Ollama",
                Some((message.clone(), p.danger())),
                small_button(p, "retry-ollama", tr("Retry"), |ui: &mut Ui, _| retry(ui)),
            ));
            &[]
        }
        Some(Ok(models)) => models,
    };
    for model in installed {
        let choice = ModelChoice::Ollama {
            model: model.name.clone(),
        };
        let line = format!(
            "{} · {}",
            if model.vision {
                tr("Sees images")
            } else {
                tr("Text only")
            },
            gigabytes(model.size)
        );
        rows.push(setting_row(
            p,
            &format!("model-{}", key_part(&model.name)),
            &model.name,
            Some(&line),
            use_control(
                p,
                format!("use-{}", key_part(&model.name)),
                chosen == Some(&choice),
                true,
                choice,
            ),
        ));
    }
    for model in RECOMMENDED
        .iter()
        .filter(|model| !installed.iter().any(|i| i.name == model.name))
    {
        rows.push(recommended_row(
            ui,
            p,
            model.name,
            model.size_gb,
            model.note,
        ));
    }
    rows.push(apple_row(ui, p, chosen));
    rows.push(url_row(ui, p));
    if let Some(error) = &picker.url_error {
        rows.push(note("ollama-url-error", error, p.danger()));
    }
    rows
}

/// A recommended model not downloaded yet: its size and Download, or the
/// download's progress and Cancel.
fn recommended_row(ui: &Ui, p: Paint, name: &'static str, size_gb: u32, about: &str) -> El {
    let picker = &ui.skills.picker;
    let key = key_part(name);
    let pulling = picker.pull.as_ref().filter(|pull| pull.model == name);
    let failed = picker
        .pull_error
        .as_ref()
        .filter(|(model, _)| model == name)
        .map(|(_, message)| message.clone());
    let (line, color, control) = match pulling {
        Some(pull) => {
            let percent = (pull.completed * 100)
                .checked_div(pull.total)
                .unwrap_or(0)
                .min(100);
            (
                tr_fill("Downloading… {}%", &percent.to_string()),
                p.muted(),
                small_button(
                    p,
                    &format!("cancel-{key}"),
                    tr("Cancel"),
                    |ui: &mut Ui, _| cancel_download(ui),
                ),
            )
        }
        None => (
            failed
                .clone()
                .unwrap_or_else(|| format!("{} · {size_gb} GB", tr(about))),
            if failed.is_some() {
                p.danger()
            } else {
                p.muted()
            },
            small(
                p,
                &format!("download-{key}"),
                tr("Download"),
                picker.pull.is_none(),
                move |ui: &mut Ui, _| download(ui, name),
            ),
        ),
    };
    described_row(
        p,
        &format!("model-{key}"),
        name,
        Some((line, color)),
        control,
    )
}

fn apple_row(ui: &Ui, p: Paint, chosen: Option<&ModelChoice>) -> El {
    let availability = ui.skills.picker.apple.unwrap_or(Availability::Unknown);
    let available = availability.is_available();
    let line = if available {
        tr("On this Mac · text only").to_owned()
    } else {
        tr(availability.description()).to_owned()
    };
    setting_row(
        p,
        "model-apple-intelligence",
        "Apple Intelligence",
        Some(&line),
        use_control(
            p,
            "use-apple-intelligence".into(),
            chosen == Some(&ModelChoice::AppleIntelligence),
            available,
            ModelChoice::AppleIntelligence,
        ),
    )
}

fn url_row(ui: &Ui, p: Paint) -> El {
    let picker = &ui.skills.picker;
    let field = TextInput::new(&picker.url)
        .label(tr("Ollama's address"))
        .width(Some(220.0))
        .on_edit(|ui: &mut Ui| &mut ui.skills.picker.url)
        .on_submit(|ui: &mut Ui, _| save_url(ui))
        .build(p.k)
        .key("ollama-url");
    let control = row()
        .key("ollama-url-control")
        .gap(8.0)
        .center_y()
        .child(field)
        .child(small_button(
            p,
            "save-ollama-url",
            tr("Save"),
            |ui: &mut Ui, _| save_url(ui),
        ));
    setting_row(
        p,
        "ollama-address",
        tr("Ollama's address"),
        Some(tr("On this computer only")),
        control,
    )
}
