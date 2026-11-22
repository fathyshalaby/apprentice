//! Apprentice's own persisted settings: which local model drafts a skill
//! from a lesson, and where Ollama listens (`crate::labs` Watch and learn).

use super::*;

/// The default Ollama address: loopback only, never a remote host.
pub const DEFAULT_OLLAMA_URL: &str = "http://127.0.0.1:11434";

/// The `apprentice.model` string for Apple Intelligence.
const APPLE_INTELLIGENCE: &str = "apple-intelligence";
/// The prefix of an Ollama `apprentice.model` string: `ollama:<name>`.
const OLLAMA_PREFIX: &str = "ollama:";

/// A model the person chose to draft skills with. Nothing picks or switches
/// it for them.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ModelChoice {
    AppleIntelligence,
    Ollama { model: String },
}

/// Settings for Watch and learn (`apprentice` in `settings.json`).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct ApprenticeSettings {
    /// `None` until a model is chosen, else `"apple-intelligence"` or
    /// `"ollama:<name>"`. Omitted from the file while unchosen.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Ollama's address; it must be a loopback `http` URL.
    pub ollama_url: String,
}

impl Default for ApprenticeSettings {
    fn default() -> Self {
        Self {
            model: None,
            ollama_url: DEFAULT_OLLAMA_URL.to_owned(),
        }
    }
}

impl ApprenticeSettings {
    /// The chosen model, or `None` when none is chosen or the saved string
    /// names something this build does not know.
    pub fn choice(&self) -> Option<ModelChoice> {
        let model = self.model.as_deref()?;
        if model == APPLE_INTELLIGENCE {
            return Some(ModelChoice::AppleIntelligence);
        }
        let name = model.strip_prefix(OLLAMA_PREFIX)?;
        (!name.trim().is_empty()).then(|| ModelChoice::Ollama {
            model: name.to_owned(),
        })
    }

    /// The `apprentice.model` string for `choice`.
    pub fn model_string(choice: &ModelChoice) -> String {
        match choice {
            ModelChoice::AppleIntelligence => APPLE_INTELLIGENCE.to_owned(),
            ModelChoice::Ollama { model } => format!("{OLLAMA_PREFIX}{model}"),
        }
    }
}

/// Whether `url` is an `http` address on this computer alone (`localhost`, a
/// loopback IP) with no sign-in, path, or query: what Ollama's URL may be.
/// The request's own resolver still refuses anything that resolves elsewhere.
pub fn is_loopback_http_url(url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(url.trim()) else {
        return false;
    };
    let on_this_computer = match parsed.host() {
        Some(url::Host::Domain(name)) => name.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(address)) => address.is_loopback(),
        Some(url::Host::Ipv6(address)) => address.is_loopback(),
        None => false,
    };
    parsed.scheme() == "http"
        && on_this_computer
        && parsed.username().is_empty()
        && parsed.password().is_none()
        && matches!(parsed.path(), "" | "/")
        && parsed.query().is_none()
        && parsed.fragment().is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apprentice_settings_default_to_no_model_and_local_ollama() {
        let settings = ApprenticeSettings::default();
        assert_eq!(settings.choice(), None);
        assert_eq!(settings.ollama_url, "http://127.0.0.1:11434");
        // An unchosen model is omitted, so older builds read the file.
        let json = serde_json::to_value(&settings).unwrap();
        assert!(json.get("model").is_none());
        assert_eq!(AppSettings::default().apprentice, settings);
    }

    #[test]
    fn model_strings_parse() {
        let parse = |model: &str| {
            ApprenticeSettings {
                model: Some(model.to_owned()),
                ..ApprenticeSettings::default()
            }
            .choice()
        };
        assert_eq!(
            parse("ollama:gemma4:26b"),
            Some(ModelChoice::Ollama {
                model: "gemma4:26b".to_owned()
            })
        );
        assert_eq!(
            parse("apple-intelligence"),
            Some(ModelChoice::AppleIntelligence)
        );
        assert_eq!(parse("cloud:x"), None);
        assert_eq!(parse("ollama:"), None);
        let choice = ModelChoice::Ollama {
            model: "qwen3-vl:8b".to_owned(),
        };
        assert_eq!(
            parse(&ApprenticeSettings::model_string(&choice)),
            Some(choice)
        );
    }

    #[test]
    fn only_loopback_http_urls_are_allowed() {
        for ok in [
            "http://127.0.0.1:11434",
            "http://localhost:11434/",
            "http://[::1]:11434",
        ] {
            assert!(is_loopback_http_url(ok), "{ok}");
        }
        for bad in [
            "https://127.0.0.1:11434",
            "http://example.com:11434",
            "http://192.168.1.5:11434",
            "http://127.0.0.1:11434/api",
            "http://user@127.0.0.1:11434",
            "127.0.0.1:11434",
            "",
        ] {
            assert!(!is_loopback_http_url(bad), "{bad}");
        }
    }

    #[test]
    fn a_saved_file_without_the_section_loads_the_defaults() {
        let loaded: AppSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(loaded.apprentice, ApprenticeSettings::default());
    }
}
