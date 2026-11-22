//! What a local model is asked to write from a lesson, and reading its answer.

use serde::Deserialize;
use serde_json::{Value, json};

use super::evidence::Segment;
use super::skill::{Autonomy, Skill, Step, slug};

/// At most this many steps keep a question for the person.
const MAX_QUESTIONS: usize = 5;

/// A model answer that is not a usable draft.
#[derive(Debug)]
pub struct ParseError {
    pub raw: String,
    pub reason: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Draft {
    name: Option<String>,
    #[serde(default)]
    description: String,
    #[serde(default)]
    when_to_use: String,
    #[serde(default)]
    inputs: Vec<String>,
    #[serde(default)]
    steps: Vec<DraftStep>,
    #[serde(default)]
    verification: Vec<String>,
    #[serde(default)]
    edge_cases: Vec<String>,
}

#[derive(Deserialize)]
struct DraftStep {
    #[serde(default)]
    title: String,
    #[serde(default)]
    detail: String,
    question: Option<String>,
}

/// The system prompt for every drafting call.
pub fn instructions() -> &'static str {
    "You turn a recorded lesson into a skill that an AI coding agent can follow later.\n\
The evidence is a time-ordered list of lines. `[mm:ss] said:` is what the person said aloud. \
`[mm:ss] window:` is the app and window in front. `[mm:ss] clicked ...` is a click, with the \
name of the element when it is known. Screenshots of key moments may be attached.\n\
Write the steps an AI coding agent could follow to do the same task: short titles, concrete \
details, in order. Use only what the evidence supports and never invent steps. \
Also give a one-sentence description, when to use the skill, the inputs it needs, how to \
verify it worked, and edge cases the person mentioned. Use an empty list where there are none.\n\
Mark at most 5 steps that look like decisions (the person chose between options) by giving \
them a short, plain question to the person, such as \"Why did you pick that folder?\". Ask \
about choices the person did not already explain. Every other step's `question` is an empty \
string.\n\
Answer with JSON in the given schema only, with no other text."
}

/// The JSON schema a model's answer must follow (Ollama's `format`).
pub fn schema() -> Value {
    let strings = json!({ "type": "array", "items": { "type": "string" } });
    json!({
        "type": "object",
        "properties": {
            "name": { "type": "string" },
            "description": { "type": "string" },
            "whenToUse": { "type": "string" },
            "inputs": strings,
            "steps": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "title": { "type": "string" },
                        "detail": { "type": "string" },
                        "question": { "type": "string" }
                    },
                    "required": ["title", "detail", "question"]
                }
            },
            "verification": strings,
            "edgeCases": strings
        },
        // Every field: Ollama lets a model skip optional ones, and real
        // models then fold them into other fields.
        "required": [
            "name", "description", "whenToUse", "inputs", "steps", "verification", "edgeCases"
        ]
    })
}

/// The user prompt for one part of a long lesson.
pub fn segment_prompt(seg: &Segment, n: usize, of: usize) -> String {
    format!(
        "Part {n} of {of} of the lesson. List the steps shown in this part, in the schema.\n\n{}",
        seg.text
    )
}

/// The user prompt that joins per-part step lists into one skill.
pub fn merge_prompt(step_lists: &[String]) -> String {
    let mut out = String::from(
        "These are the steps found in each part of one lesson. Combine them into one complete \
skill in the schema, removing repeats and keeping the order.\n",
    );
    for (i, list) in step_lists.iter().enumerate() {
        out.push_str(&format!("\nPart {}:\n{}\n", i + 1, list));
    }
    out
}

/// Reads a model answer into a new `Observe` skill.
pub fn parse(raw: &str, lesson_id: &str, now_ms: u64) -> Result<Skill, ParseError> {
    let fail = |reason: &str| ParseError {
        raw: raw.to_string(),
        reason: reason.to_string(),
    };
    let json = outermost_object(raw).ok_or_else(|| fail("not JSON"))?;
    let draft: Draft = serde_json::from_str(json).map_err(|_| fail("not JSON"))?;
    if draft.steps.is_empty() {
        return Err(fail("no steps"));
    }
    let name = draft
        .name
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty());
    let name = name.ok_or_else(|| fail("no name"))?;
    let mut questions = 0;
    let steps = draft
        .steps
        .into_iter()
        .map(|s| {
            let question = s
                .question
                .map(|q| q.trim().to_string())
                .filter(|q| !q.is_empty());
            let question = question.filter(|_| {
                questions += 1;
                questions <= MAX_QUESTIONS
            });
            Step {
                title: s.title,
                detail: s.detail,
                question,
                why: None,
            }
        })
        .collect();
    Ok(Skill {
        slug: slug(&name),
        name,
        description: draft.description,
        when_to_use: draft.when_to_use,
        inputs: draft.inputs,
        steps,
        verification: draft.verification,
        edge_cases: draft.edge_cases,
        autonomy: Autonomy::Observe,
        version: 1,
        created_ms: now_ms,
        updated_ms: now_ms,
        lesson_id: lesson_id.to_string(),
    })
}

/// The text from the first `{` to the last `}`: strips code fences and prose.
fn outermost_object(raw: &str) -> Option<&str> {
    let start = raw.find('{')?;
    let end = raw.rfind('}')?;
    (end > start).then(|| &raw[start..=end])
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"{"name":"File the invoice","description":"Files it.","whenToUse":"A PDF arrives.",
"inputs":["PDF"],"steps":[{"title":"Open","detail":"Open the PDF."},
{"title":"Pick","detail":"Choose folder.","question":"Why that folder?"}],
"verification":["Filed."],"edgeCases":["Duplicate."]}"#;

    #[test]
    fn a_draft_parses_from_schema_json() {
        let s = parse(GOOD, "lesson-9", 42).unwrap();
        assert_eq!(s.slug, "file-the-invoice");
        assert_eq!(s.when_to_use, "A PDF arrives.");
        assert_eq!(s.steps.len(), 2);
        assert_eq!(s.steps[1].question.as_deref(), Some("Why that folder?"));
        assert!(s.steps.iter().all(|st| st.why.is_none()));
        assert_eq!(
            (s.autonomy, s.version, s.created_ms, s.updated_ms),
            (Autonomy::Observe, 1, 42, 42)
        );
        assert_eq!(s.lesson_id, "lesson-9");
        assert_eq!(s.edge_cases, ["Duplicate."]);
    }

    #[test]
    fn more_than_five_questions_keep_the_first_five() {
        let steps: Vec<String> = (0..7)
            .map(|i| format!(r#"{{"title":"S{i}","detail":"d","question":"Why {i}?"}}"#))
            .collect();
        let raw = format!(r#"{{"name":"N","steps":[{}]}}"#, steps.join(","));
        let s = parse(&raw, "l", 0).unwrap();
        assert_eq!(s.steps.len(), 7);
        let asked: Vec<_> = s
            .steps
            .iter()
            .filter_map(|st| st.question.as_deref())
            .collect();
        assert_eq!(asked, ["Why 0?", "Why 1?", "Why 2?", "Why 3?", "Why 4?"]);
    }

    #[test]
    fn invalid_json_keeps_the_raw_text() {
        let err = parse("I could not do that.", "l", 0).unwrap_err();
        assert_eq!(err.raw, "I could not do that.");
        assert_eq!(err.reason, "not JSON");
    }

    #[test]
    fn a_draft_without_steps_is_rejected() {
        assert_eq!(
            parse(r#"{"name":"N","steps":[]}"#, "l", 0)
                .unwrap_err()
                .reason,
            "no steps"
        );
        assert_eq!(
            parse(r#"{"name":"N"}"#, "l", 0).unwrap_err().reason,
            "no steps"
        );
        assert_eq!(
            parse(r#"{"steps":[{"title":"a","detail":"b"}]}"#, "l", 0)
                .unwrap_err()
                .reason,
            "no name"
        );
    }

    #[test]
    fn json_inside_a_code_fence_still_parses() {
        let fenced = format!("Here you go:\n```json\n{GOOD}\n```\nDone.");
        assert!(parse(&fenced, "l", 0).is_ok());
    }

    #[test]
    fn prompts_carry_their_evidence() {
        assert!(instructions().len() < 2_000);
        let seg = Segment {
            start_ms: 0,
            end_ms: 1,
            text: "[00:01] said: hi".into(),
            frames: vec![],
        };
        let p = segment_prompt(&seg, 2, 3);
        assert!(p.contains("Part 2 of 3") && p.contains("said: hi"));
        assert!(merge_prompt(&["a".into(), "b".into()]).contains("Part 2:\nb"));
        assert!(schema()["properties"]["steps"].is_object());
    }

    /// Ollama writes to the schema token by token and lets a model skip
    /// optional fields: real models then put inputs, checks, edge cases, and
    /// their questions into other fields. Every field is required.
    #[test]
    fn the_schema_requires_every_field() {
        fn required(object: &Value) -> Vec<String> {
            let mut names: Vec<String> = object["required"]
                .as_array()
                .unwrap()
                .iter()
                .map(|name| name.as_str().unwrap().to_owned())
                .collect();
            names.sort();
            names
        }
        fn properties(object: &Value) -> Vec<String> {
            let mut names: Vec<String> = object["properties"]
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect();
            names.sort();
            names
        }
        let schema = schema();
        assert_eq!(required(&schema), properties(&schema));
        let step = &schema["properties"]["steps"]["items"];
        assert_eq!(required(step), properties(step));
        assert!(instructions().contains("empty string"));
    }

    #[test]
    fn an_empty_question_is_no_question() {
        let raw = r#"{"name":"N","description":"","whenToUse":"","inputs":[],
            "steps":[{"title":"a","detail":"b","question":""},
                     {"title":"c","detail":"d","question":"Why that folder?"}],
            "verification":[],"edgeCases":[]}"#;
        let skill = parse(raw, "l", 0).unwrap();
        assert_eq!(skill.steps[0].question, None);
        assert_eq!(skill.steps[1].question.as_deref(), Some("Why that folder?"));
    }
}
