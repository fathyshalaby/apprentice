//! One drafting job: a lesson's evidence through the model the person chose,
//! part by part, then one merge, read into a skill.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::{Duration, Instant};

use super::draft::{ParseError, instructions, merge_prompt, parse, schema, segment_prompt};
use super::events::{self, ApprenticeAction};
use super::evidence::{self, Limits, Segment};
use super::lessons::Lesson;
use super::ollama::{self, ModelError};
use super::skill::Skill;
use crate::app_settings::ModelChoice;
use crate::apple_intelligence;
use crate::desktop::i18n::tr;
use crate::meeting::generator::GenerationError;

/// At most this many part calls; longer lessons join neighbouring parts.
const MAX_PARTS: usize = 8;
/// The longest one model call may take.
const CALL_TIMEOUT: Duration = Duration::from_secs(180);
/// The longest a whole draft may take.
const DEADLINE: Duration = Duration::from_secs(10 * 60);
/// Apple Intelligence's sampling temperature for drafts.
const TEMPERATURE: f64 = 0.2;

/// What a drafting job reports, in order; the last is `Done`, `Failed`, or
/// `Unparsed`.
// Read by the Skills pane (Task 10).
#[allow(dead_code)]
#[derive(Debug)]
pub enum Progress {
    /// Asking about part `n` of `of`.
    Segment {
        n: usize,
        of: usize,
    },
    /// Joining the parts into one skill.
    Merging,
    Done(Skill),
    /// Why it stopped, for the person.
    Failed(String),
    /// The model answered, but not with a usable draft.
    Unparsed(ParseError),
}

/// One call to the chosen model: Ollama or Apple Intelligence, or a fake in
/// tests.
pub trait Model {
    fn call(
        &self,
        system: &str,
        prompt: &str,
        images: &[Vec<u8>],
        timeout: Duration,
    ) -> Result<String, ModelError>;
}

/// Drafts a skill from `lesson` with exactly `choice`, on a thread of its own
/// (`apprentice-draft`). The receiver ends after one `Done`, `Failed`, or
/// `Unparsed`. Any model error ends the draft: nothing retries or switches to
/// another model. Video and audio are never sent; Ollama gets each part's key
/// frames, Apple Intelligence text alone.
// Started by the Skills pane's Draft button (Task 10).
#[allow(dead_code)]
pub fn draft(
    lesson: &Lesson,
    choice: &ModelChoice,
    url: &str,
    cancel: Arc<AtomicBool>,
) -> mpsc::Receiver<Progress> {
    let (events, received) = mpsc::channel();
    if !crate::labs::watch_and_learn() {
        let _ = events.send(Progress::Failed(tr("Watch and learn is off").to_owned()));
        return received;
    }
    let deadline = Instant::now() + DEADLINE;
    let lesson = lesson.clone();
    let choice = choice.clone();
    let url = url.to_owned();
    let failed = events.clone();
    let spawned = thread::Builder::new()
        .name("apprentice-draft".into())
        .spawn(move || {
            events::emit(ApprenticeAction::DraftStarted, &lesson.id, None);
            let (model, limits): (Box<dyn Model>, Limits) = match choice {
                ModelChoice::Ollama { model } => {
                    (Box::new(Ollama { url, model }), Limits::for_vision())
                }
                ModelChoice::AppleIntelligence => (
                    Box::new(AppleIntelligence),
                    Limits::for_text(apple_intelligence::context_tokens()),
                ),
            };
            match evidence::build(&lesson, &limits) {
                Ok(evidence) => drive(
                    evidence.segments,
                    model.as_ref(),
                    limits.frames_per_segment,
                    &lesson.id,
                    &cancel,
                    deadline,
                    &events,
                ),
                // The one way a build fails: the lesson's bundle is gone.
                Err(reason) => {
                    tracing::warn!(lesson = %lesson.id, %reason, "could not read a lesson");
                    events::emit(ApprenticeAction::DraftFailed, &lesson.id, None);
                    let _ = events.send(Progress::Failed(
                        tr("This lesson's notes are gone, so there is nothing to learn from")
                            .to_owned(),
                    ));
                }
            }
        });
    if let Err(error) = spawned {
        tracing::warn!(%error, "could not start drafting");
        let _ = failed.send(Progress::Failed(
            tr("Apprentice couldn't start drafting").to_owned(),
        ));
    }
    received
}

/// Asks about each part (at most [`MAX_PARTS`]), then merges several parts'
/// answers in one more call, and reads the last answer into a skill.
fn drive(
    segments: Vec<Segment>,
    model: &dyn Model,
    frames_per_call: usize,
    lesson_id: &str,
    cancel: &AtomicBool,
    deadline: Instant,
    events: &Sender<Progress>,
) {
    let fail = |message: &str| {
        events::emit(ApprenticeAction::DraftFailed, lesson_id, None);
        let _ = events.send(Progress::Failed(message.to_owned()));
    };
    let segments = fit(segments, MAX_PARTS, frames_per_call);
    if segments.is_empty() {
        fail(tr("This lesson has nothing to learn from"));
        return;
    }
    // Checked before every call: the call's timeout, or why the draft stops.
    let next_timeout = || {
        if cancel.load(Ordering::Acquire) {
            return Err(tr("Cancelled"));
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(tr("Drafting took too long"));
        }
        Ok(left.min(CALL_TIMEOUT))
    };
    let ask = |prompt: &str, images: &[Vec<u8>]| -> Result<String, String> {
        let timeout = next_timeout()?;
        model
            .call(instructions(), prompt, images, timeout)
            .map_err(|error| {
                tracing::warn!(?error, "drafting stopped");
                if error == ModelError::Timeout && Instant::now() >= deadline {
                    tr("Drafting took too long").to_owned()
                } else {
                    error.message()
                }
            })
    };
    let of = segments.len();
    let mut answers = Vec::with_capacity(of);
    for (index, segment) in segments.iter().enumerate() {
        if let Err(message) = next_timeout() {
            fail(message);
            return;
        }
        let n = index + 1;
        let _ = events.send(Progress::Segment { n, of });
        let images: Vec<Vec<u8>> = segment
            .frames
            .iter()
            .map(|frame| frame.png.clone())
            .collect();
        match ask(&segment_prompt(segment, n, of), &images) {
            Ok(answer) => answers.push(answer),
            Err(message) => return fail(&message),
        }
    }
    let raw = if of == 1 {
        answers.remove(0)
    } else {
        if let Err(message) = next_timeout() {
            fail(message);
            return;
        }
        let _ = events.send(Progress::Merging);
        match ask(&merge_prompt(&answers), &[]) {
            Ok(answer) => answer,
            Err(message) => return fail(&message),
        }
    };
    let finished = match parse(&raw, lesson_id, crate::history::now_ms()) {
        Ok(skill) => {
            let steps = u32::try_from(skill.steps.len()).unwrap_or(u32::MAX);
            events::emit(ApprenticeAction::DraftFinished, lesson_id, Some(steps));
            Progress::Done(skill)
        }
        Err(error) => {
            events::emit(ApprenticeAction::DraftFailed, lesson_id, None);
            Progress::Unparsed(error)
        }
    };
    let _ = events.send(finished);
}

/// `segments` joined down to at most `max` parts, keeping every line: the
/// neighbours whose key frames still fit one call together first, then those
/// with the least text. Only a part left with more than `frames_per_call`
/// frames (no call sends more) is thinned, evenly, and the log says so.
fn fit(mut segments: Vec<Segment>, max: usize, frames_per_call: usize) -> Vec<Segment> {
    while segments.len() > max.max(1) {
        let at = (0..segments.len() - 1)
            .min_by_key(|&i| {
                let (left, right) = (&segments[i], &segments[i + 1]);
                (
                    left.frames.len() + right.frames.len() > frames_per_call,
                    left.text.len() + right.text.len(),
                )
            })
            .unwrap_or(0);
        let next = segments.remove(at + 1);
        let joined = &mut segments[at];
        joined.end_ms = next.end_ms;
        joined.text.push('\n');
        joined.text.push_str(&next.text);
        joined.frames.extend(next.frames);
    }
    for segment in &mut segments {
        let count = segment.frames.len();
        if count > frames_per_call {
            tracing::warn!(
                frames = count,
                kept = frames_per_call,
                "a joined lesson part had more key frames than one call sends"
            );
            let frames = std::mem::take(&mut segment.frames);
            segment.frames = frames
                .into_iter()
                .enumerate()
                .filter(|(i, _)| keep(*i, count, frames_per_call))
                .map(|(_, frame)| frame)
                .collect();
        }
    }
    segments
}

/// Whether frame `i` of `count` is one of `k` evenly spread picks.
fn keep(i: usize, count: usize, k: usize) -> bool {
    (0..k).any(|pick| pick * count / k == i)
}

/// A model on this computer's Ollama.
struct Ollama {
    url: String,
    model: String,
}

impl Model for Ollama {
    fn call(
        &self,
        system: &str,
        prompt: &str,
        images: &[Vec<u8>],
        timeout: Duration,
    ) -> Result<String, ModelError> {
        ollama::chat(
            &self.url,
            &self.model,
            system,
            prompt,
            images,
            &schema(),
            timeout,
        )
    }
}

/// Apple Intelligence on this Mac: text only, the schema in the instructions.
struct AppleIntelligence;

impl Model for AppleIntelligence {
    fn call(
        &self,
        system: &str,
        prompt: &str,
        _images: &[Vec<u8>],
        timeout: Duration,
    ) -> Result<String, ModelError> {
        let availability = apple_intelligence::availability();
        if !availability.is_available() {
            return Err(ModelError::Failed(
                tr(availability.description()).to_owned(),
            ));
        }
        let system = format!("{system}\n\nThe JSON schema:\n{}", schema());
        apple_intelligence::generate_text(&system, prompt, TEMPERATURE, timeout).map_err(|error| {
            match error {
                GenerationError::Refused => ModelError::Refused,
                GenerationError::ContextWindow => ModelError::Failed(
                    tr("This part of the lesson is too long for Apple Intelligence").to_owned(),
                ),
                GenerationError::Failed(reason) => ModelError::Failed(reason),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apprentice::evidence::Frame;
    use crate::apprentice::lessons::Media;
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::sync::atomic::Ordering;

    const DRAFT: &str = r#"{"name":"Rename files","description":"d","whenToUse":"w",
        "steps":[{"title":"Open Finder","detail":"Click Finder in the Dock"}]}"#;

    /// A model that answers from a queue and remembers each call.
    struct Fake {
        replies: RefCell<VecDeque<Result<String, ModelError>>>,
        calls: RefCell<Vec<(String, Vec<Vec<u8>>)>>,
        cancel_after: Option<(usize, Arc<AtomicBool>)>,
    }

    impl Fake {
        fn new(replies: Vec<Result<String, ModelError>>) -> Self {
            Self {
                replies: RefCell::new(replies.into()),
                calls: RefCell::new(Vec::new()),
                cancel_after: None,
            }
        }
    }

    impl Model for Fake {
        fn call(
            &self,
            system: &str,
            prompt: &str,
            images: &[Vec<u8>],
            timeout: Duration,
        ) -> Result<String, ModelError> {
            assert_eq!(system, crate::apprentice::draft::instructions());
            assert!(timeout <= Duration::from_secs(180));
            self.calls
                .borrow_mut()
                .push((prompt.to_owned(), images.to_vec()));
            if let Some((after, cancel)) = &self.cancel_after
                && self.calls.borrow().len() == *after
            {
                cancel.store(true, Ordering::Release);
            }
            self.replies
                .borrow_mut()
                .pop_front()
                .unwrap_or_else(|| Ok(DRAFT.to_owned()))
        }
    }

    fn segment(text: &str, frames: usize) -> Segment {
        Segment {
            start_ms: 0,
            end_ms: 1_000,
            text: text.to_owned(),
            frames: (0..frames)
                .map(|i| Frame {
                    t_ms: i as u64,
                    png: vec![i as u8],
                })
                .collect(),
        }
    }

    fn run(segments: Vec<Segment>, model: &Fake, deadline: Instant) -> Vec<Progress> {
        let (events, received) = mpsc::channel();
        drive(
            segments,
            model,
            4,
            "lesson-1",
            &AtomicBool::new(false),
            deadline,
            &events,
        );
        drop(events);
        received.into_iter().collect()
    }

    fn soon() -> Instant {
        Instant::now() + Duration::from_secs(600)
    }

    #[test]
    fn one_segment_skips_the_merge() {
        let model = Fake::new(vec![Ok(DRAFT.to_owned())]);
        let progress = run(
            vec![segment("[00:01] said: open finder", 2)],
            &model,
            soon(),
        );
        assert!(
            matches!(progress[0], Progress::Segment { n: 1, of: 1 }),
            "{progress:?}"
        );
        let Progress::Done(skill) = &progress[1] else {
            panic!("{progress:?}");
        };
        assert_eq!(skill.name, "Rename files");
        assert_eq!(skill.lesson_id, "lesson-1");
        assert_eq!(progress.len(), 2, "{progress:?}");
        let calls = model.calls.borrow();
        assert_eq!(calls.len(), 1);
        assert!(calls[0].0.contains("open finder"));
        assert_eq!(calls[0].1.len(), 2, "the part's frames are attached");
    }

    #[test]
    fn several_segments_end_with_one_merge() {
        let model = Fake::new(vec![
            Ok("steps of part one".into()),
            Ok("steps of part two".into()),
            Ok(DRAFT.to_owned()),
        ]);
        let progress = run(vec![segment("one", 1), segment("two", 1)], &model, soon());
        assert!(matches!(progress[0], Progress::Segment { n: 1, of: 2 }));
        assert!(matches!(progress[1], Progress::Segment { n: 2, of: 2 }));
        assert!(matches!(progress[2], Progress::Merging));
        assert!(matches!(progress[3], Progress::Done(_)), "{progress:?}");
        let calls = model.calls.borrow();
        assert_eq!(calls.len(), 3);
        assert!(calls[2].0.contains("steps of part one"));
        assert!(calls[2].0.contains("steps of part two"));
        assert_eq!(calls[2].1.len(), 0, "the merge sends no frames");
    }

    #[test]
    fn a_long_lesson_merges_parts_down_to_eight_calls() {
        let model = Fake::new(Vec::new());
        let segments = (0..11).map(|i| segment(&format!("part-{i}"), 2)).collect();
        let progress = run(segments, &model, soon());
        let parts = progress
            .iter()
            .filter(|event| matches!(event, Progress::Segment { of: 8, .. }))
            .count();
        assert_eq!(parts, 8, "{progress:?}");
        let calls = model.calls.borrow();
        assert_eq!(calls.len(), 9, "eight parts and one merge");
        for i in 0..11 {
            let text = format!("part-{i}");
            assert!(
                calls[..8].iter().any(|(prompt, _)| prompt.contains(&text)),
                "{text} was dropped"
            );
        }
        assert!(calls[..8].iter().all(|(_, images)| images.len() <= 4));
    }

    #[test]
    fn joining_parts_keeps_every_frame_that_fits() {
        let model = Fake::new(Vec::new());
        let segments = (0..11)
            .map(|i| {
                let mut part = segment(&format!("part-{i}"), 0);
                if i < 3 {
                    part.frames = (0..4)
                        .map(|f| Frame {
                            t_ms: f,
                            png: vec![i as u8, f as u8],
                        })
                        .collect();
                }
                part
            })
            .collect();
        run(segments, &model, soon());
        let calls = model.calls.borrow();
        assert_eq!(calls.len(), 9, "eight parts and one merge");
        let mut sent: Vec<Vec<u8>> = calls[..8]
            .iter()
            .flat_map(|(_, images)| images.clone())
            .collect();
        sent.sort();
        let expected: Vec<Vec<u8>> = (0..3u8)
            .flat_map(|i| (0..4u8).map(move |f| vec![i, f]))
            .collect();
        assert_eq!(sent, expected, "every frame is sent");
        assert!(calls[..8].iter().all(|(_, images)| images.len() <= 4));
    }

    #[test]
    fn a_missing_model_fails_without_switching() {
        let model = Fake::new(vec![Err(ModelError::NotDownloaded("gemma4:26b".into()))]);
        let progress = run(vec![segment("one", 0), segment("two", 0)], &model, soon());
        let failures: Vec<&String> = progress
            .iter()
            .filter_map(|event| match event {
                Progress::Failed(message) => Some(message),
                _ => None,
            })
            .collect();
        assert_eq!(
            failures,
            [&ModelError::NotDownloaded("gemma4:26b".into()).message()]
        );
        assert!(matches!(progress.last(), Some(Progress::Failed(_))));
        assert_eq!(
            model.calls.borrow().len(),
            1,
            "no second call, no other model"
        );
    }

    #[test]
    fn an_unusable_answer_is_unparsed() {
        let model = Fake::new(vec![Ok("I can't help with that".into())]);
        let progress = run(vec![segment("one", 0)], &model, soon());
        let Some(Progress::Unparsed(error)) = progress.last() else {
            panic!("{progress:?}");
        };
        assert_eq!(error.raw, "I can't help with that");
    }

    #[test]
    fn cancelling_stops_between_calls() {
        let cancel = Arc::new(AtomicBool::new(false));
        let mut model = Fake::new(Vec::new());
        model.cancel_after = Some((1, cancel.clone()));
        let (events, received) = mpsc::channel();
        drive(
            vec![segment("one", 0), segment("two", 0)],
            &model,
            4,
            "lesson-1",
            &cancel,
            soon(),
            &events,
        );
        drop(events);
        let progress: Vec<Progress> = received.into_iter().collect();
        assert_eq!(model.calls.borrow().len(), 1);
        let Some(Progress::Failed(message)) = progress.last() else {
            panic!("{progress:?}");
        };
        assert_eq!(message, tr("Cancelled"));
    }

    #[test]
    fn a_passed_deadline_stops_the_draft() {
        let model = Fake::new(Vec::new());
        let progress = run(vec![segment("one", 0)], &model, Instant::now());
        let Some(Progress::Failed(message)) = progress.last() else {
            panic!("{progress:?}");
        };
        assert_eq!(message, tr("Drafting took too long"));
        assert!(model.calls.borrow().is_empty());
    }

    #[test]
    fn drafting_refuses_with_the_lab_off() {
        crate::labs::apply(&crate::app_settings::LabsSettings::default());
        let lesson = Lesson {
            bundle: std::path::PathBuf::from("/nonexistent/lesson"),
            id: "lesson-1".into(),
            created_ms: 0,
            duration_ms: 0,
            media: Media::Present,
            frames: 0,
        };
        let progress: Vec<Progress> = draft(
            &lesson,
            &ModelChoice::Ollama {
                model: "gemma4:26b".into(),
            },
            "http://127.0.0.1:9",
            Arc::new(AtomicBool::new(false)),
        )
        .into_iter()
        .collect();
        assert_eq!(progress.len(), 1, "{progress:?}");
        let Progress::Failed(message) = &progress[0] else {
            panic!("{progress:?}");
        };
        assert_eq!(message, tr("Watch and learn is off"));
    }
}
