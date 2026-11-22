//! A small client for a local Ollama: what is installed, one chat turn, and
//! downloading a model. Only loopback addresses are ever reached.

use std::io::{BufRead, BufReader};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use ureq::http::Request;

use crate::app_settings::is_loopback_http_url;
use crate::apprentice::events::{self, ApprenticeAction};
use crate::desktop::base64;
use crate::desktop::i18n::{tr, tr_fill};

/// Ollama's sampling temperature for drafts: steady, but not greedy.
const TEMPERATURE: f64 = 0.2;
/// Listing models and checking one's capabilities.
const LIST_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a download may wait for Ollama to start answering.
const PULL_RESPONSE_TIMEOUT: Duration = Duration::from_secs(60);
/// The longest a download may run (a slow line, a large model).
const PULL_BODY_TIMEOUT: Duration = Duration::from_secs(12 * 60 * 60);
/// The largest answer read: a draft, a model list, a model's details.
const MAX_ANSWER_BYTES: u64 = 8 * 1024 * 1024;
/// The largest error body read.
const MAX_ERROR_BYTES: u64 = 64 * 1024;

/// A model Ollama has on disk.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Installed {
    pub name: String,
    /// Bytes on disk.
    pub size: u64,
    /// Whether it reads images (`/api/show` lists `vision`).
    pub vision: bool,
}

/// One line of a download's progress.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PullProgress {
    pub status: String,
    pub completed: u64,
    pub total: u64,
}

/// Why a local model call failed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ModelError {
    /// Nothing answers at Ollama's address.
    NotRunning,
    /// Ollama doesn't have this model.
    NotDownloaded(String),
    /// The address is not on this computer; nothing was sent.
    NotLoopback,
    /// The model refused the lesson.
    Refused,
    /// No answer in time.
    Timeout,
    /// The person stopped it (a download's Cancel).
    Cancelled,
    Failed(String),
}

impl ModelError {
    /// What the person reads.
    pub fn message(&self) -> String {
        match self {
            Self::NotRunning => tr("Ollama isn't running").to_owned(),
            Self::NotDownloaded(model) => tr_fill("{} isn't downloaded", model),
            Self::NotLoopback => tr("Ollama's address must be on this computer").to_owned(),
            Self::Refused => tr("The model refused this lesson").to_owned(),
            Self::Timeout => tr("The model took too long to answer").to_owned(),
            Self::Cancelled => tr("Cancelled").to_owned(),
            Self::Failed(reason) => tr_fill("The model failed: {}", reason),
        }
    }
}

/// The models Ollama at `url` has, each checked for vision.
// Listed by the Skills pane (Task 10).
#[allow(dead_code)]
pub fn installed(url: &str) -> Result<Vec<Installed>, ModelError> {
    let base = base(url)?;
    let agent = agent(Some(LIST_TIMEOUT));
    let request = Request::get(format!("{base}/api/tags"))
        .body(Vec::new())
        .map_err(invalid)?;
    let tags: Tags = read_json(&agent, request, None)?;
    Ok(tags
        .models
        .into_iter()
        .map(|model| Installed {
            // A model Ollama can't describe reads as text-only, not as a
            // failed list.
            vision: sees_images(&agent, &base, &model.name),
            name: model.name,
            size: model.size,
        })
        .collect())
}

/// Whether `/api/show` lists `vision` for `model`; false when it can't say.
fn sees_images(agent: &ureq::Agent, base: &str, model: &str) -> bool {
    let request = post(base, "/api/show", &json!({ "model": model }));
    let Ok(request) = request else {
        return false;
    };
    read_json::<Show>(agent, request, None)
        .map(|show| {
            show.capabilities
                .iter()
                .any(|capability| capability == "vision")
        })
        .unwrap_or(false)
}

/// One non-streaming chat turn; returns the answer's text. `images` go with
/// the person's message as base64; `format` is the answer's JSON schema.
pub fn chat(
    url: &str,
    model: &str,
    system: &str,
    prompt: &str,
    images: &[Vec<u8>],
    format: &Value,
    timeout: Duration,
) -> Result<String, ModelError> {
    let base = base(url)?;
    let mut user = json!({ "role": "user", "content": prompt });
    if !images.is_empty() {
        user["images"] = images
            .iter()
            .map(|image| Value::String(base64::encode(image)))
            .collect();
    }
    let body = json!({
        "model": model,
        "messages": [{ "role": "system", "content": system }, user],
        "stream": false,
        "format": format,
        "options": { "temperature": TEMPERATURE },
    });
    let request = post(&base, "/api/chat", &body)?;
    let answer: Chat = read_json(&agent(Some(timeout)), request, Some(model))?;
    Ok(answer.message.content)
}

/// Downloads `model`, reporting each progress line until Ollama says
/// `success`. `cancel` is read between lines; a cancelled download returns
/// [`ModelError::Cancelled`], and Ollama keeps what it has for the next pull.
// Started by the Skills pane's Download button (Task 10).
#[allow(dead_code)]
pub fn pull(
    url: &str,
    model: &str,
    progress: &mut dyn FnMut(PullProgress),
    cancel: &AtomicBool,
) -> Result<(), ModelError> {
    let base = base(url)?;
    let request = post(
        &base,
        "/api/pull",
        &json!({ "model": model, "stream": true }),
    )?;
    let agent = crate::net::loopback_agent(|config| {
        config
            .http_status_as_error(false)
            .timeout_recv_response(Some(PULL_RESPONSE_TIMEOUT))
            .timeout_recv_body(Some(PULL_BODY_TIMEOUT))
    });
    let mut response = agent.run(request).map_err(from_ureq)?;
    if !response.status().is_success() {
        return Err(refusal(&mut response, Some(model)));
    }
    let reader = response.body_mut().with_config().limit(u64::MAX).reader();
    for line in BufReader::new(reader).lines() {
        if cancel.load(Ordering::Acquire) {
            return Err(ModelError::Cancelled);
        }
        let line = line.map_err(|error| {
            tracing::warn!(%error, "an Ollama download stopped");
            ModelError::Failed(tr("The download stopped").to_owned())
        })?;
        if line.trim().is_empty() {
            continue;
        }
        let line: PullLine = serde_json::from_str(&line)
            .map_err(|_| ModelError::Failed(tr("Ollama sent an unreadable line").to_owned()))?;
        if let Some(error) = line.error {
            return Err(ModelError::Failed(error));
        }
        if line.status == "success" {
            events::emit(ApprenticeAction::ModelDownloaded, model, None);
            return Ok(());
        }
        progress(PullProgress {
            status: line.status,
            completed: line.completed,
            total: line.total,
        });
    }
    Err(ModelError::Failed(
        tr("The download ended early").to_owned(),
    ))
}

/// `url` without a trailing slash, once it is known to be on this computer.
fn base(url: &str) -> Result<String, ModelError> {
    if !is_loopback_http_url(url) {
        return Err(ModelError::NotLoopback);
    }
    Ok(url.trim().trim_end_matches('/').to_owned())
}

/// A loopback-only agent that reads error statuses as answers.
fn agent(timeout: Option<Duration>) -> ureq::Agent {
    crate::net::loopback_agent(|config| config.http_status_as_error(false).timeout_global(timeout))
}

fn post(base: &str, path: &str, body: &Value) -> Result<Request<Vec<u8>>, ModelError> {
    Request::post(format!("{base}{path}"))
        .header("Content-Type", "application/json")
        .body(body.to_string().into_bytes())
        .map_err(invalid)
}

fn invalid(error: ureq::http::Error) -> ModelError {
    ModelError::Failed(tr_fill("The request was invalid: {}", &error.to_string()))
}

/// Sends `request` and reads a JSON answer; `model` names a model that a
/// 404 means is missing.
fn read_json<T: DeserializeOwned>(
    agent: &ureq::Agent,
    request: Request<Vec<u8>>,
    model: Option<&str>,
) -> Result<T, ModelError> {
    let mut response = agent.run(request).map_err(from_ureq)?;
    if !response.status().is_success() {
        return Err(refusal(&mut response, model));
    }
    let body = response
        .body_mut()
        .with_config()
        .limit(MAX_ANSWER_BYTES)
        .read_to_vec()
        .map_err(from_ureq)?;
    serde_json::from_slice(&body)
        .map_err(|_| ModelError::Failed(tr("Ollama sent an unreadable answer").to_owned()))
}

/// An error status as a [`ModelError`], from Ollama's `{"error": …}` body.
fn refusal(response: &mut ureq::http::Response<ureq::Body>, model: Option<&str>) -> ModelError {
    let status = response.status().as_u16();
    let error = response
        .body_mut()
        .with_config()
        .limit(MAX_ERROR_BYTES)
        .read_to_vec()
        .ok()
        .and_then(|body| serde_json::from_slice::<OllamaError>(&body).ok())
        .map(|body| body.error);
    match (model, error) {
        (Some(model), Some(error)) if status == 404 && error.contains("not found") => {
            ModelError::NotDownloaded(model.to_owned())
        }
        (_, Some(error)) => ModelError::Failed(error),
        (_, None) => ModelError::Failed(tr_fill("Ollama answered HTTP {}", &status.to_string())),
    }
}

fn from_ureq(error: ureq::Error) -> ModelError {
    match &error {
        ureq::Error::Timeout(_) => ModelError::Timeout,
        ureq::Error::Io(io) if io.to_string().contains(crate::net::NOT_LOOPBACK) => {
            ModelError::NotLoopback
        }
        ureq::Error::Io(io) if io.kind() == std::io::ErrorKind::TimedOut => ModelError::Timeout,
        ureq::Error::Io(io)
            if matches!(
                io.kind(),
                std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::ConnectionReset
            ) =>
        {
            ModelError::NotRunning
        }
        ureq::Error::ConnectionFailed | ureq::Error::HostNotFound => ModelError::NotRunning,
        _ => ModelError::Failed(crate::net::reason(&error)),
    }
}

#[derive(Deserialize)]
struct Tags {
    #[serde(default)]
    models: Vec<Tag>,
}

#[derive(Deserialize)]
struct Tag {
    name: String,
    #[serde(default)]
    size: u64,
}

#[derive(Deserialize)]
struct Show {
    #[serde(default)]
    capabilities: Vec<String>,
}

#[derive(Deserialize)]
struct Chat {
    message: ChatMessage,
}

#[derive(Deserialize)]
struct ChatMessage {
    #[serde(default)]
    content: String,
}

#[derive(Deserialize)]
struct PullLine {
    #[serde(default)]
    status: String,
    #[serde(default)]
    completed: u64,
    #[serde(default)]
    total: u64,
    error: Option<String>,
}

#[derive(Deserialize)]
struct OllamaError {
    error: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktop::opencode_service::tests::serve;

    fn body(received: &(String, Vec<u8>)) -> Value {
        serde_json::from_slice(&received.1).unwrap()
    }

    #[test]
    fn installed_marks_vision_models() {
        let (url, server) = serve(vec![
            (
                "GET /api/tags".into(),
                200,
                json!({ "models": [
                    { "name": "gemma4:26b", "size": 17_000_000_000_u64 },
                    { "name": "llama3:8b", "size": 4_700_000_000_u64 },
                    { "name": "broken:1b", "size": 1 },
                ] })
                .to_string(),
            ),
            (
                "POST /api/show".into(),
                200,
                json!({ "capabilities": ["completion", "vision"] }).to_string(),
            ),
            (
                "POST /api/show".into(),
                200,
                json!({ "capabilities": ["completion"] }).to_string(),
            ),
            (
                "POST /api/show".into(),
                500,
                json!({ "error": "boom" }).to_string(),
            ),
        ]);
        let models = installed(&url).unwrap();
        assert_eq!(
            models,
            [
                Installed {
                    name: "gemma4:26b".into(),
                    size: 17_000_000_000,
                    vision: true,
                },
                Installed {
                    name: "llama3:8b".into(),
                    size: 4_700_000_000,
                    vision: false,
                },
                Installed {
                    name: "broken:1b".into(),
                    size: 1,
                    vision: false,
                },
            ]
        );
        let received = server.join().unwrap();
        assert_eq!(body(&received[1]), json!({ "model": "gemma4:26b" }));
        assert_eq!(body(&received[2]), json!({ "model": "llama3:8b" }));
    }

    #[test]
    fn chat_sends_images_and_the_schema() {
        let (url, server) = serve(vec![(
            "POST /api/chat".into(),
            200,
            json!({ "message": { "role": "assistant", "content": "{\"name\":\"x\"}" } })
                .to_string(),
        )]);
        let schema = json!({ "type": "object" });
        let answer = chat(
            &url,
            "gemma4:26b",
            "system words",
            "user words",
            &[b"frame one".to_vec(), b"frame two".to_vec()],
            &schema,
            Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!(answer, "{\"name\":\"x\"}");
        let received = server.join().unwrap();
        let sent = body(&received[0]);
        assert_eq!(sent["model"], "gemma4:26b");
        assert_eq!(sent["stream"], false);
        assert_eq!(sent["format"], schema);
        assert_eq!(sent["options"]["temperature"], 0.2);
        assert_eq!(
            sent["messages"][0],
            json!({ "role": "system", "content": "system words" })
        );
        assert_eq!(sent["messages"][1]["role"], "user");
        assert_eq!(sent["messages"][1]["content"], "user words");
        assert_eq!(
            sent["messages"][1]["images"],
            json!([
                crate::desktop::base64::encode(b"frame one"),
                crate::desktop::base64::encode(b"frame two"),
            ])
        );
    }

    #[test]
    fn chat_without_images_sends_none() {
        let (url, server) = serve(vec![(
            "POST /api/chat".into(),
            200,
            json!({ "message": { "content": "ok" } }).to_string(),
        )]);
        let answer = chat(&url, "m", "s", "p", &[], &json!({}), Duration::from_secs(5));
        assert_eq!(answer.unwrap(), "ok");
        let sent = body(&server.join().unwrap()[0]);
        assert!(sent["messages"][1].get("images").is_none(), "{sent}");
    }

    #[test]
    fn a_missing_model_reads_not_downloaded() {
        let (url, server) = serve(vec![(
            "POST /api/chat".into(),
            404,
            json!({ "error": "model 'gemma4:26b' not found" }).to_string(),
        )]);
        let error = chat(
            &url,
            "gemma4:26b",
            "s",
            "p",
            &[],
            &json!({}),
            Duration::from_secs(5),
        )
        .unwrap_err();
        assert_eq!(error, ModelError::NotDownloaded("gemma4:26b".into()));
        assert_eq!(server.join().unwrap().len(), 1);
    }

    #[test]
    fn an_ollama_error_names_its_reason() {
        let (url, server) = serve(vec![(
            "POST /api/chat".into(),
            500,
            json!({ "error": "out of memory" }).to_string(),
        )]);
        let error = chat(&url, "m", "s", "p", &[], &json!({}), Duration::from_secs(5));
        assert_eq!(error, Err(ModelError::Failed("out of memory".into())));
        server.join().unwrap();
    }

    #[test]
    fn pull_reports_progress_lines() {
        let lines = [
            json!({ "status": "pulling manifest" }),
            json!({ "status": "pulling abc", "completed": 5, "total": 10 }),
            json!({ "status": "pulling abc", "completed": 10, "total": 10 }),
            json!({ "status": "success" }),
        ]
        .map(|line| line.to_string())
        .join("\n");
        let (url, server) = serve(vec![("POST /api/pull".into(), 200, lines)]);
        let mut seen = Vec::new();
        pull(
            &url,
            "qwen3-vl:8b",
            &mut |progress| seen.push(progress),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(
            seen,
            [
                PullProgress {
                    status: "pulling manifest".into(),
                    completed: 0,
                    total: 0,
                },
                PullProgress {
                    status: "pulling abc".into(),
                    completed: 5,
                    total: 10,
                },
                PullProgress {
                    status: "pulling abc".into(),
                    completed: 10,
                    total: 10,
                },
            ]
        );
        let sent = body(&server.join().unwrap()[0]);
        assert_eq!(sent, json!({ "model": "qwen3-vl:8b", "stream": true }));
    }

    #[test]
    fn pull_fails_on_an_error_line_or_an_early_end() {
        let lines = [
            json!({ "status": "pulling manifest" }).to_string(),
            json!({ "error": "pull model manifest: file does not exist" }).to_string(),
        ]
        .join("\n");
        let (url, server) = serve(vec![
            ("POST /api/pull".into(), 200, lines),
            (
                "POST /api/pull".into(),
                200,
                json!({ "status": "pulling manifest" }).to_string(),
            ),
        ]);
        let error = pull(&url, "nope", &mut |_| {}, &AtomicBool::new(false)).unwrap_err();
        assert_eq!(
            error,
            ModelError::Failed("pull model manifest: file does not exist".into())
        );
        let error = pull(&url, "nope", &mut |_| {}, &AtomicBool::new(false)).unwrap_err();
        assert!(matches!(error, ModelError::Failed(_)), "{error:?}");
        server.join().unwrap();
    }

    #[test]
    fn a_cancelled_pull_reads_cancelled() {
        let lines = [
            json!({ "status": "pulling manifest" }).to_string(),
            json!({ "status": "success" }).to_string(),
        ]
        .join("\n");
        let (url, server) = serve(vec![("POST /api/pull".into(), 200, lines)]);
        let mut seen = 0;
        let error = pull(&url, "m", &mut |_| seen += 1, &AtomicBool::new(true));
        assert_eq!(error, Err(ModelError::Cancelled));
        assert_eq!(seen, 0);
        assert_eq!(ModelError::Cancelled.message(), tr("Cancelled"));
        server.join().unwrap();
    }

    #[test]
    fn a_non_loopback_url_is_refused() {
        let url = "http://example.com:11434";
        assert_eq!(installed(url), Err(ModelError::NotLoopback));
        assert_eq!(
            chat(url, "m", "s", "p", &[], &json!({}), Duration::from_secs(1)),
            Err(ModelError::NotLoopback)
        );
        assert_eq!(
            pull(url, "m", &mut |_| {}, &AtomicBool::new(false)),
            Err(ModelError::NotLoopback)
        );
        assert_eq!(
            installed("https://127.0.0.1:11434"),
            Err(ModelError::NotLoopback)
        );
    }

    #[test]
    fn nothing_listening_reads_not_running() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);
        assert_eq!(installed(&url), Err(ModelError::NotRunning));
        assert_eq!(
            chat(&url, "m", "s", "p", &[], &json!({}), Duration::from_secs(5)),
            Err(ModelError::NotRunning)
        );
    }

    #[test]
    fn every_error_has_a_message() {
        for error in [
            ModelError::NotRunning,
            ModelError::NotDownloaded("gemma4:26b".into()),
            ModelError::NotLoopback,
            ModelError::Refused,
            ModelError::Timeout,
            ModelError::Cancelled,
            ModelError::Failed("boom".into()),
        ] {
            assert!(!error.message().is_empty(), "{error:?}");
        }
        assert!(
            ModelError::NotDownloaded("gemma4:26b".into())
                .message()
                .contains("gemma4:26b")
        );
    }
}
