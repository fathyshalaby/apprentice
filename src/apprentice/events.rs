//! What Apprentice did, for `live.ndjson`: an action, an identifier, and
//! sometimes a count. Never a skill name, step text, narration, an answer, a
//! path, or an address.

use std::sync::OnceLock;

pub use crate::desktop::events::ApprenticeAction;
use crate::desktop::events::{EventLog, VoiceEvent, now_ms};

static LOG: OnceLock<EventLog> = OnceLock::new();

/// Hands over the app's event log. Only the first call counts.
pub fn set_log(log: EventLog) {
    let _ = LOG.set(log);
}

/// Records `action` for `id` (a lesson, draft, skill slug, or model name) and
/// an optional count. Nothing happens before [`set_log`]; a failed write is
/// only logged at debug.
pub fn emit(action: ApprenticeAction, id: &str, count: Option<u32>) {
    let Some(log) = LOG.get() else {
        return;
    };
    if let Err(error) = log.emit(&event(action, id, count, now_ms())) {
        tracing::debug!(%error, "could not record an Apprentice event");
    }
}

fn event(action: ApprenticeAction, id: &str, count: Option<u32>, timestamp_ms: u64) -> VoiceEvent {
    VoiceEvent::Apprentice {
        timestamp_ms,
        action,
        id: id.to_owned(),
        count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktop::events::{EventLog, VoiceEvent};

    #[test]
    fn emitting_writes_only_the_action_id_and_count() {
        assert!(event(ApprenticeAction::DraftStarted, "x", None, 1).kind() == "apprentice");
        // Before `set_log`, emitting does nothing and does not panic.
        emit(ApprenticeAction::DraftStarted, "early", None);
        let path = std::env::temp_dir().join(format!(
            "hex-apprentice-events-{}.ndjson",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let log = EventLog::create(&path).unwrap();
        set_log(log.clone());
        emit(ApprenticeAction::SkillEdited, "send-invoice", Some(3));
        emit(ApprenticeAction::SkillRemoved, "send-invoice", None);
        log.flush().unwrap();
        let written = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = written.lines().collect();
        assert_eq!(lines.len(), 2, "{written}");
        let first: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        let mut keys: Vec<&str> = first
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort();
        assert_eq!(keys, ["action", "count", "id", "kind", "timestamp_ms"]);
        assert_eq!(first["action"], "skill_edited");
        let second: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
        assert!(second.get("count").is_none(), "{second}");
        assert!(matches!(
            serde_json::from_str::<VoiceEvent>(lines[1]).unwrap(),
            VoiceEvent::Apprentice { .. }
        ));
        std::fs::remove_file(path).unwrap();
    }
}
