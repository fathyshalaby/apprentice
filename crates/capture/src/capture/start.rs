//! What a capture asks the engine to record.

use std::path::PathBuf;

use crate::model::{CaptureMode, TargetInfo};

use super::{AudioIn, MovieQuality, PortalToken, Target, boot_time_ns};

#[derive(Clone, Debug)]
pub struct Start {
    /// The capture's start on the boot-time nanosecond clock that audio and input timestamps share.
    pub t0_boot_ns: u64,
    pub target: Target,
    pub bundle_dir: Option<PathBuf>,
    pub audio: AudioIn,
    pub window_log: bool,
    pub note: Option<String>,
    pub movie: MovieQuality,
    /// Where a screen-cast portal keeps the screen's restore token; `None` keeps none.
    pub portal_token: PortalToken,
    /// Whether to record Accessibility names under clicks for this capture
    /// alone; `None` follows the engine's `agentContext` setting.
    pub agent_context: Option<bool>,
}

impl Default for Start {
    fn default() -> Self {
        Self {
            t0_boot_ns: boot_time_ns(),
            target: Target::ActiveDisplay,
            bundle_dir: None,
            audio: AudioIn::External,
            window_log: true,
            note: None,
            movie: MovieQuality::Compact,
            portal_token: PortalToken::None,
            agent_context: None,
        }
    }
}

impl Start {
    /// Whether this capture records Accessibility names under clicks: its
    /// own [`Self::agent_context`], else the engine's `agentContext`.
    pub fn records_agent_context(&self, engine: &crate::handoff::EngineConfig) -> bool {
        self.agent_context
            .unwrap_or_else(|| engine.experiment(crate::handoff::AGENT_CONTEXT))
    }

    pub fn screen(target: TargetInfo) -> Self {
        Self {
            window_log: target.mode == CaptureMode::Display,
            target: Target::Screen(target),
            ..Self::default()
        }
    }

    pub fn screen_with_mic(target: TargetInfo) -> Self {
        Self::screen(target).with_audio(AudioIn::Microphone)
    }

    pub const fn with_audio(mut self, audio: AudioIn) -> Self {
        self.audio = audio;
        self
    }

    pub fn phone(device: Option<String>, name: Option<String>) -> Self {
        Self {
            target: Target::Phone { device, name },
            audio: AudioIn::None,
            window_log: false,
            ..Self::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handoff::{AGENT_CONTEXT, EngineConfig};

    #[test]
    fn an_explicit_agent_context_overrides_the_engine_setting() {
        let mut off = EngineConfig::default();
        off.experiments.insert(AGENT_CONTEXT.into(), false);
        let mut on = EngineConfig::default();
        on.experiments.insert(AGENT_CONTEXT.into(), true);
        let follows = Start::default();
        assert!(!follows.records_agent_context(&off));
        assert!(follows.records_agent_context(&on));
        let names = Start {
            agent_context: Some(true),
            ..Start::default()
        };
        assert!(names.records_agent_context(&off));
        let none = Start {
            agent_context: Some(false),
            ..Start::default()
        };
        assert!(!none.records_agent_context(&on));
    }
}
