//! What a capture is for, and Labs › Watch and learn's lessons: an Annotate
//! recording of the whole screen with Accessibility names under clicks on
//! for that recording alone, never copied or routed, marked with
//! [`LESSON_MARK`] once written and handed to [`on_lesson_written`]'s hook.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::audio::CaptureInstant;
use crate::control::ControlReply;

/// The file that makes a written capture a lesson: `{"version":1,
/// "startedAtMs":<unix ms>}`, owner-only.
pub const LESSON_MARK: &str = ".apprentice-lesson.json";

/// What a capture is for, from its start to its outcome.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Purpose {
    /// An Annotate capture: copied, given Paste Last provenance, and routed.
    #[default]
    Capture,
    /// Labs › Screenshots' recording ([`super::CaptureCommand::Clip`]).
    Clip,
    /// Labs › Watch and learn's lesson ([`super::CaptureCommand::Lesson`]).
    Lesson,
}

impl Purpose {
    /// Only an Annotate capture is copied to the clipboard and routed.
    pub fn handoff(self) -> bool {
        self == Self::Capture
    }

    /// Only a Screenshots recording folds the capsule.
    pub fn folds_capsule(self) -> bool {
        self == Self::Clip
    }

    /// A lesson records the names under clicks; anything else follows the
    /// engine's own setting.
    pub(super) fn agent_context(self) -> Option<bool> {
        (self == Self::Lesson).then_some(true)
    }
}

/// Why a capture of `purpose` can't start now: a lesson needs Labs › Watch
/// and learn and Annotate on.
pub(super) fn refusal(purpose: Purpose) -> Option<ControlReply> {
    (purpose == Purpose::Lesson && !allowed())
        .then(|| ControlReply::error("feature-off", "Watch and learn is off"))
}

fn allowed() -> bool {
    #[cfg(test)]
    if let Some(allowed) = TEST_ALLOWED.get() {
        return allowed;
    }
    crate::labs::watch_and_learn() && crate::app_settings::capture_on()
}

#[cfg(test)]
thread_local! {
    static TEST_ALLOWED: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// Pins [`refusal`]'s gate on this test's thread; `None` reads the lab.
#[cfg(test)]
pub(super) fn allow_in_tests(allowed: Option<bool>) {
    TEST_ALLOWED.set(allowed);
}

/// Who learns of each written lesson, on the `capture-finish` thread: it
/// must only hand the bundle on.
static LESSON_WRITTEN: OnceLock<fn(PathBuf)> = OnceLock::new();

/// Sends each written lesson to `written`; the first caller wins. The
/// macOS app registers it.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn on_lesson_written(written: fn(PathBuf)) {
    let _ = LESSON_WRITTEN.set(written);
}

/// A written lesson (started at `since`): marks `bundle` as one before it
/// settles, and gives the hook to hand it to once it has.
pub(super) fn written(
    bundle: &Path,
    since: Option<CaptureInstant>,
) -> Option<&'static fn(PathBuf)> {
    let started = started_at_ms(since, CaptureInstant::now(), crate::history::now_ms());
    if let Err(error) = mark(bundle, started) {
        tracing::warn!(%error, bundle = %bundle.display(), "could not mark a lesson");
    }
    LESSON_WRITTEN.get()
}

/// When a capture that started at `since` began, in Unix ms, counted back
/// from `now` (which is `now_ms`); `now_ms` when the start is unknown.
fn started_at_ms(since: Option<CaptureInstant>, now: CaptureInstant, now_ms: u64) -> u64 {
    let ago = since.map_or(0, |since| now.as_nanos().saturating_sub(since.as_nanos()));
    now_ms.saturating_sub(ago / 1_000_000)
}

fn mark(bundle: &Path, started_at_ms: u64) -> std::io::Result<()> {
    let mark = serde_json::json!({ "version": 1, "startedAtMs": started_at_ms });
    crate::durable::write_private(&bundle.join(LESSON_MARK), mark.to_string().as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_capture_hands_off_and_only_a_clip_folds() {
        let all = [Purpose::Capture, Purpose::Clip, Purpose::Lesson];
        let handoff = all.map(Purpose::handoff);
        assert_eq!(handoff, [true, false, false]);
        assert_eq!(all.map(Purpose::folds_capsule), [false, true, false]);
        assert_eq!(all.map(Purpose::agent_context), [None, None, Some(true)]);
    }

    #[test]
    fn only_a_lesson_is_gated() {
        allow_in_tests(Some(false));
        assert!(refusal(Purpose::Capture).is_none());
        assert!(refusal(Purpose::Clip).is_none());
        assert!(matches!(
            refusal(Purpose::Lesson),
            Some(ControlReply::Error { code, .. }) if code == "feature-off"
        ));
        allow_in_tests(Some(true));
        assert!(refusal(Purpose::Lesson).is_none());
    }

    #[test]
    fn a_lesson_s_start_counts_back_from_now() {
        let now = CaptureInstant::from_nanos(50_000_000_000);
        let since = CaptureInstant::from_nanos(47_500_000_000);
        assert_eq!(started_at_ms(Some(since), now, 1_000_000), 997_500);
        assert_eq!(started_at_ms(None, now, 1_000_000), 1_000_000);
    }

    #[test]
    fn a_lesson_mark_is_owner_only_json() {
        let dir = std::env::temp_dir().join(format!(
            "hex-lesson-mark-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        mark(&dir, 1_234).unwrap();
        let bytes = std::fs::read(dir.join(LESSON_MARK)).unwrap();
        let read: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            read,
            serde_json::json!({ "version": 1, "startedAtMs": 1_234 })
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let meta = std::fs::metadata(dir.join(LESSON_MARK)).unwrap();
            assert_eq!(meta.permissions().mode() & 0o777, 0o600);
        }
        std::fs::remove_dir_all(dir).ok();
    }
}
