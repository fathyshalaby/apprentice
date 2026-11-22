//! The local models Watch and learn suggests downloading, in the order the
//! Skills pane lists them. Nothing here picks one for the person.

/// One suggested Ollama model.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Recommended {
    /// Ollama's model name, as `ollama pull` takes it.
    pub name: &'static str,
    /// About how much it downloads, in gigabytes.
    pub size_gb: u32,
    /// The pane's line under the name, in English (translated where shown).
    pub note: &'static str,
}

/// The models the Skills pane suggests, best first.
// Listed by the Skills pane (Task 10).
#[allow(dead_code)]
pub const RECOMMENDED: [Recommended; 4] = [
    Recommended {
        name: "gemma4:26b",
        size_gb: 17,
        note: "Sees images · recommended",
    },
    Recommended {
        name: "qwen3.8:27b",
        size_gb: 18,
        note: "Sees images · strongest that fits",
    },
    Recommended {
        name: "gemma4:12b",
        size_gb: 8,
        note: "Sees images · lighter",
    },
    Recommended {
        name: "qwen3-vl:8b",
        size_gb: 6,
        note: "Sees images · lightest",
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recommended_models_are_the_four_from_the_spec() {
        let listed: Vec<(&str, u32, &str)> = RECOMMENDED
            .iter()
            .map(|model| (model.name, model.size_gb, model.note))
            .collect();
        assert_eq!(
            listed,
            [
                ("gemma4:26b", 17, "Sees images · recommended"),
                ("qwen3.8:27b", 18, "Sees images · strongest that fits"),
                ("gemma4:12b", 8, "Sees images · lighter"),
                ("qwen3-vl:8b", 6, "Sees images · lightest"),
            ]
        );
    }
}
