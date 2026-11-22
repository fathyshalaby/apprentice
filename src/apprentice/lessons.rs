//! Labs › Watch and learn's lessons on disk: the Annotate bundles in the
//! captures folder that carry [`LESSON_MARK`], newest first.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::SyncSender;

use crate::capture_host::{LESSON_MARK, load_capture};
use crate::macos_rune::status_item::StatusItemAction;

const MEDIA_CLEANED: &str = ".media-cleaned";

/// Whether a lesson's movie and voice are still on disk.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Media {
    /// `recording.mp4` is there.
    Present,
    /// Retention removed the movie and audio; frames and notes stay.
    Cleaned,
    /// No `scrawl.json` any more: only the mark is left.
    Expired,
}

/// One lesson bundle.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Lesson {
    pub bundle: PathBuf,
    pub id: String,
    pub created_ms: u64,
    pub duration_ms: u64,
    pub media: Media,
    /// Key frames still in `frames/`.
    pub frames: usize,
}

/// The capture-finish hook ([`crate::capture_host::on_lesson_written`]):
/// records that a lesson was written and asks the app to show the Skills
/// pane. It runs on the `capture-finish` thread, so it only hands on.
pub fn written(bundle: PathBuf) {
    match crate::macos_rune::status_item::actions() {
        Some(open) => announce(&bundle, open),
        None => super::events::emit(
            super::events::ApprenticeAction::LessonWritten,
            &lesson_id(&bundle),
            None,
        ),
    }
}

fn announce(bundle: &Path, open: &SyncSender<StatusItemAction>) {
    super::events::emit(
        super::events::ApprenticeAction::LessonWritten,
        &lesson_id(bundle),
        None,
    );
    // Never blocks: a full queue already has the app's attention.
    let _ = open.try_send(StatusItemAction::OpenSkills);
}

/// A bundle's lesson id: its folder's stem.
fn lesson_id(bundle: &Path) -> String {
    bundle
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Whole-screen lesson of the display under the pointer, handed to the
/// listener without waiting (the Skills pane's and the menu's Watch this).
pub fn start() -> color_eyre::Result<()> {
    use crate::capture_host::{CaptureCommand, CaptureControl, CaptureTarget};
    let at = scrawl_capture::screens::pointer().unwrap_or_default();
    let command = CaptureCommand::Lesson(CaptureTarget::Display { at });
    crate::capture_host::send_to_listener(CaptureControl::Command(command))
}

/// [`start`], but waits at most `wait` for the listener's answer, so `Ok`
/// means the lesson started recording; a refusal or no answer in time is
/// the reason it didn't. For a caller off the UI thread (Notice patterns'
/// Show me).
pub fn start_confirmed(wait: std::time::Duration) -> Result<(), String> {
    use crate::capture_host::{CaptureCommand, CaptureControl, CaptureTarget};
    let at = scrawl_capture::screens::pointer().unwrap_or_default();
    let command = CaptureCommand::Lesson(CaptureTarget::Display { at });
    answered(crate::capture_host::ask_listener(
        CaptureControl::Command(command),
        wait,
    ))
}

/// The listener's answer to a lesson: started, or why not.
fn answered(reply: color_eyre::Result<crate::control::ControlReply>) -> Result<(), String> {
    match reply {
        Ok(crate::control::ControlReply::Error { message, .. }) => Err(message),
        Ok(_) => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

/// Every lesson under `captures`, newest first.
pub fn list(captures: &Path) -> Vec<Lesson> {
    let Ok(entries) = fs::read_dir(captures) else {
        return Vec::new();
    };
    let mut lessons: Vec<Lesson> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|dir| dir.extension().is_some_and(|ext| ext == "scrawl"))
        .filter_map(|dir| read(&dir))
        .collect();
    lessons.sort_by(|a, b| b.created_ms.cmp(&a.created_ms).then(b.id.cmp(&a.id)));
    lessons
}

fn read(dir: &Path) -> Option<Lesson> {
    let mark = fs::read_to_string(dir.join(LESSON_MARK)).ok()?;
    let started_ms = serde_json::from_str::<serde_json::Value>(&mark)
        .ok()
        .and_then(|value| value.get("startedAtMs")?.as_u64())
        .unwrap_or(0);
    let frames = fs::read_dir(dir.join("frames"))
        .map(|files| files.flatten().filter(|f| f.path().is_file()).count())
        .unwrap_or(0);
    let (id, created_ms, duration_ms, media) = match load_capture(dir) {
        Some(bundle) => {
            let media = if dir.join(MEDIA_CLEANED).exists() || !dir.join("recording.mp4").exists() {
                Media::Cleaned
            } else {
                Media::Present
            };
            (bundle.id, bundle.created_at_ms, bundle.duration_ms, media)
        }
        None => {
            let id = dir
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default();
            (id, started_ms, 0, Media::Expired)
        }
    };
    Some(Lesson {
        bundle: dir.to_path_buf(),
        id,
        created_ms,
        duration_ms,
        media,
        frames,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "hex-lessons-{name}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    /// A bundle with a manifest, one frame, and (when `lesson`) the mark.
    fn bundle(root: &Path, ms: u64, lesson: bool) -> PathBuf {
        let dir = root.join(format!("{ms}.scrawl"));
        fs::create_dir_all(dir.join("frames")).unwrap();
        let frame = r#"{"x":0,"y":0,"width":1,"height":1,"scale":2}"#;
        let manifest = format!(
            r#"{{"version":1,"id":"id-{ms}","createdAtMs":{ms},"durationMs":4000,
            "target":{{"mode":"display","displayId":1,"frame":{frame}}},
            "transcript":{{"source":"parakeet","text":"","segments":[],"words":[]}},
            "strokes":[],"marks":[],"events":[],"keyframes":[],"contactSheet":"sheet.jpg"}}"#
        );
        fs::write(dir.join("scrawl.json"), manifest).unwrap();
        fs::write(dir.join("frames/frame-01.png"), b"x").unwrap();
        fs::write(dir.join("recording.mp4"), b"x").unwrap();
        if lesson {
            mark(&dir, ms);
        }
        dir
    }

    fn mark(dir: &Path, ms: u64) {
        fs::write(
            dir.join(LESSON_MARK),
            format!(r#"{{"version":1,"startedAtMs":{ms}}}"#),
        )
        .unwrap();
    }

    #[test]
    fn a_written_lesson_opens_the_skills_pane() {
        use crate::macos_rune::status_item::StatusItemAction;
        let (open, requests) = std::sync::mpsc::sync_channel(1);
        let bundle = PathBuf::from("/captures/2026-11-22-lesson.scrawl");
        // No lesson mark and no manifest: the pane lists what it finds.
        announce(&bundle, &open);
        assert_eq!(requests.try_recv(), Ok(StatusItemAction::OpenSkills));
        // A full queue never blocks the capture-finish thread.
        announce(&bundle, &open);
        announce(&bundle, &open);
        assert_eq!(requests.try_recv(), Ok(StatusItemAction::OpenSkills));
        assert!(requests.try_recv().is_err());
        assert_eq!(lesson_id(&bundle), "2026-11-22-lesson");
    }

    #[test]
    fn only_the_listeners_yes_counts_as_started() {
        use crate::control::ControlReply;
        let started = ControlReply::Done {
            message: "recording".into(),
            changed: true,
        };
        assert_eq!(answered(Ok(started)), Ok(()));
        let refused = ControlReply::error("feature-off", "captures are off");
        assert_eq!(answered(Ok(refused)), Err("captures are off".to_owned()));
        let silent = Err(color_eyre::eyre::eyre!("Apprentice didn't answer in time"));
        assert_eq!(
            answered(silent),
            Err("Apprentice didn't answer in time".to_owned())
        );
    }

    #[test]
    fn only_marked_bundles_are_lessons() {
        let root = root("marked");
        bundle(&root, 1000, true);
        bundle(&root, 2000, false);
        let lessons = list(&root);
        assert_eq!(lessons.len(), 1);
        assert_eq!(lessons[0].id, "id-1000");
        assert_eq!(lessons[0].media, Media::Present);
        assert_eq!(lessons[0].frames, 1);
        assert_eq!(lessons[0].duration_ms, 4000);
    }

    #[test]
    fn lessons_list_newest_first() {
        let root = root("order");
        for ms in [1000, 3000, 2000] {
            bundle(&root, ms, true);
        }
        let ids: Vec<_> = list(&root).into_iter().map(|l| l.created_ms).collect();
        assert_eq!(ids, [3000, 2000, 1000]);
    }

    #[test]
    fn cleaned_media_still_lists_with_frames() {
        let root = root("cleaned");
        let dir = bundle(&root, 1000, true);
        fs::remove_file(dir.join("recording.mp4")).unwrap();
        fs::write(dir.join(MEDIA_CLEANED), b"").unwrap();
        let lessons = list(&root);
        assert_eq!(lessons[0].media, Media::Cleaned);
        assert_eq!(lessons[0].frames, 1);
    }

    #[test]
    fn a_lesson_without_scrawl_json_reads_expired() {
        let root = root("expired");
        let dir = root.join("5000.scrawl");
        fs::create_dir_all(&dir).unwrap();
        mark(&dir, 5000);
        let lessons = list(&root);
        assert_eq!(lessons.len(), 1);
        assert_eq!(lessons[0].media, Media::Expired);
        assert_eq!(lessons[0].id, "5000");
        assert_eq!(lessons[0].created_ms, 5000);
        assert_eq!(lessons[0].duration_ms, 0);
    }
}
