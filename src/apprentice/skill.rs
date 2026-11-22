//! The skill record Apprentice learns from a lesson, and its `SKILL.md`.

use serde::{Deserialize, Serialize};

/// How much a skill may do on its own. A new skill only observes.
// Later levels are chosen in the skills pane (Task 7).
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Autonomy {
    Observe,
    Suggest,
    Assist,
    AutonomousReview,
}

/// One step of a skill.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    pub title: String,
    pub detail: String,
    /// Set when the step looks like a decision: a plain question to the person.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub question: Option<String>,
    /// The person's answer to `question`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

/// A skill as stored in `skill.json`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Skill {
    pub slug: String,
    pub name: String,
    pub description: String,
    pub when_to_use: String,
    pub inputs: Vec<String>,
    pub steps: Vec<Step>,
    pub verification: Vec<String>,
    pub edge_cases: Vec<String>,
    pub autonomy: Autonomy,
    pub version: u32,
    pub created_ms: u64,
    pub updated_ms: u64,
    pub lesson_id: String,
}

const MAX_SLUG: usize = 48;

/// A folder-safe name: lowercase ASCII words joined by `-`, at most 48
/// characters, never empty.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    let mut pending_dash = false;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            if pending_dash && !out.is_empty() {
                out.push('-');
            }
            pending_dash = false;
            out.push(c.to_ascii_lowercase());
        } else {
            pending_dash = true;
        }
    }
    out.truncate(MAX_SLUG);
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() { "skill".into() } else { out }
}

/// The `SKILL.md` an agent reads.
pub fn markdown(skill: &Skill) -> String {
    let mut out = format!(
        "---\nname: {}\ndescription: {}\n---\n\n# {}\n",
        skill.slug,
        front_matter_value(&skill.description),
        skill.name
    );
    if !skill.when_to_use.trim().is_empty() {
        out.push_str(&format!(
            "\n## When to use\n\n{}\n",
            skill.when_to_use.trim()
        ));
    }
    bullets(&mut out, "Inputs", &skill.inputs);
    out.push_str("\n## Steps\n\n");
    for (i, step) in skill.steps.iter().enumerate() {
        out.push_str(&format!("{}. **{}**\n", i + 1, step.title));
        if !step.detail.trim().is_empty() {
            out.push_str(&format!("   {}\n", step.detail.trim()));
        }
    }
    let decisions: Vec<&Step> = skill
        .steps
        .iter()
        .filter(|s| s.question.is_some())
        .collect();
    if !decisions.is_empty() {
        out.push_str("\n## Decisions\n\n");
        for step in decisions {
            let question = step.question.as_deref().unwrap_or_default();
            let why = step
                .why
                .as_deref()
                .filter(|w| !w.trim().is_empty())
                .unwrap_or("not answered");
            out.push_str(&format!(
                "**{}** — {}\nWhy: {}\n\n",
                step.title, question, why
            ));
        }
        out.pop();
    }
    bullets(&mut out, "Verification", &skill.verification);
    bullets(&mut out, "Edge cases", &skill.edge_cases);
    out.push_str(&format!(
        "\n## Evidence\n\nLearned from lesson {}, recorded {}. Autonomy: observe.\n",
        skill.lesson_id,
        date(skill.created_ms)
    ));
    out
}

fn bullets(out: &mut String, heading: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }
    out.push_str(&format!("\n## {heading}\n\n"));
    for item in items {
        out.push_str(&format!("- {item}\n"));
    }
}

/// One line, quoted when plain YAML would misread it.
fn front_matter_value(text: &str) -> String {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let risky = text.contains('\n') || line.contains([':', '"', '#', '\\']) || line.is_empty();
    if risky {
        format!("\"{}\"", line.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        line
    }
}

/// UTC `yyyy-mm-dd` of a Unix time in milliseconds.
fn date(ms: u64) -> String {
    let days = (ms / 86_400_000) as i64;
    // Civil-from-days (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_are_safe() {
        assert_eq!(slug("File the March invoice!"), "file-the-march-invoice");
        assert_eq!(slug("   "), "skill");
        assert!(slug(&"word ".repeat(16)).len() <= 48);
        assert!(!slug(&"word ".repeat(16)).ends_with('-'));
    }

    fn fixture() -> Skill {
        Skill {
            slug: "file-invoice".into(),
            name: "File the invoice".into(),
            description: "Files a supplier invoice: renames and moves it.".into(),
            when_to_use: "An invoice PDF arrives.".into(),
            inputs: vec!["The PDF".into()],
            steps: vec![
                Step {
                    title: "Open the PDF".into(),
                    detail: "Double-click it in Downloads.".into(),
                    question: None,
                    why: None,
                },
                Step {
                    title: "Pick the folder".into(),
                    detail: "Choose by supplier.".into(),
                    question: Some("Why did you choose that folder?".into()),
                    why: Some("Suppliers are filed alphabetically.".into()),
                },
                Step {
                    title: "Rename".into(),
                    detail: "Add the date.".into(),
                    question: Some("Why add the date?".into()),
                    why: None,
                },
            ],
            verification: vec!["The file is in the folder.".into()],
            edge_cases: vec![],
            autonomy: Autonomy::Observe,
            version: 1,
            created_ms: 1_774_000_000_000,
            updated_ms: 1_774_000_000_000,
            lesson_id: "lesson-1".into(),
        }
    }

    #[test]
    fn markdown_matches_the_agent_skill_format() {
        let expected = "---\n\
name: file-invoice\n\
description: \"Files a supplier invoice: renames and moves it.\"\n\
---\n\
\n\
# File the invoice\n\
\n\
## When to use\n\
\n\
An invoice PDF arrives.\n\
\n\
## Inputs\n\
\n\
- The PDF\n\
\n\
## Steps\n\
\n\
1. **Open the PDF**\n   Double-click it in Downloads.\n\
2. **Pick the folder**\n   Choose by supplier.\n\
3. **Rename**\n   Add the date.\n\
\n\
## Decisions\n\
\n\
**Pick the folder** — Why did you choose that folder?\n\
Why: Suppliers are filed alphabetically.\n\
\n\
**Rename** — Why add the date?\n\
Why: not answered\n\
\n\
## Verification\n\
\n\
- The file is in the folder.\n\
\n\
## Evidence\n\
\n\
Learned from lesson lesson-1, recorded 2026-03-20. Autonomy: observe.\n";
        assert_eq!(markdown(&fixture()), expected);
    }

    #[test]
    fn skill_json_is_camel_case() {
        let json = serde_json::to_value(fixture()).unwrap();
        assert!(json.get("whenToUse").is_some() && json.get("lessonId").is_some());
        assert_eq!(json["autonomy"], "observe");
    }
}
