//! One lesson as bounded evidence for a local model: time-ordered text lines
//! in segments, plus downscaled key-frame PNGs. Video and audio never leave
//! the bundle; frames are for vision models only.

use std::io::Cursor;
use std::path::{Component, Path};

use scrawl_capture::model::{InteractionEvent, ScrawlBundle};
use scrawl_capture::render::{decode_png, downscale, encode_png};

use super::lessons::Lesson;
use crate::capture_host::load_capture;

/// Words further apart than this start a new `said:` line.
const NARRATION_GAP_MS: u64 = 2_000;
/// Context for a model whose size is unknown.
const UNKNOWN_CONTEXT_TOKENS: usize = 4_000;

/// What a model is sent for one lesson.
#[derive(Debug)]
pub struct Evidence {
    pub segments: Vec<Segment>,
    pub thin: Thin,
}

// Read by the drafting steps (Task 6).
#[allow(dead_code)]
/// A stretch of the lesson: its lines and the key frames inside it.
#[derive(Debug)]
pub struct Segment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
    pub frames: Vec<Frame>,
}

// Read by the drafting steps (Task 6).
#[allow(dead_code)]
/// A downscaled key frame as PNG bytes.
#[derive(Debug)]
pub struct Frame {
    pub t_ms: u64,
    pub png: Vec<u8>,
}

/// What the lesson left too little of to learn from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Thin {
    /// Nothing was said.
    pub no_narration: bool,
    /// No click carried an element name.
    pub no_click_names: bool,
}

/// How much evidence one model call can take.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Limits {
    pub max_segment_chars: usize,
    pub frames_per_segment: usize,
    pub max_frames: usize,
    pub frame_width: u32,
}

// Read by the drafting steps (Task 6).
#[allow(dead_code)]
impl Limits {
    /// Vision models: four frames a call, twelve a draft, 1024 px wide.
    pub fn for_vision() -> Self {
        Self {
            max_segment_chars: 12_000,
            frames_per_segment: 4,
            max_frames: 12,
            frame_width: 1024,
        }
    }

    /// Text-only models: no frames, text sized to the context (0 = unknown).
    pub fn for_text(context_tokens: usize) -> Self {
        let tokens = if context_tokens == 0 {
            UNKNOWN_CONTEXT_TOKENS
        } else {
            context_tokens
        };
        Self {
            max_segment_chars: tokens * 3 / 2,
            frames_per_segment: 0,
            max_frames: 0,
            frame_width: 0,
        }
    }
}

struct Item {
    t: u64,
    line: String,
    /// The app changed here, a place a segment may break.
    app_change: bool,
    /// A new phrase after a narration gap, a place a segment may break.
    after_gap: bool,
}

// Read by the drafting steps (Task 6).
#[allow(dead_code)]
/// The lesson's evidence. Fails only when its bundle can't be loaded.
pub fn build(lesson: &Lesson, limits: &Limits) -> Result<Evidence, String> {
    let bundle = load_capture(&lesson.bundle).ok_or_else(|| {
        "This lesson's notes are gone, so there is nothing to learn from".to_string()
    })?;
    let (items, thin) = items(&bundle);
    let mut segments = segments(&items, limits.max_segment_chars, bundle.duration_ms);
    pick_frames(&mut segments, &bundle, &lesson.bundle, limits);
    Ok(Evidence { segments, thin })
}

fn stamp(t_ms: u64) -> String {
    let secs = t_ms / 1000;
    format!("[{:02}:{:02}]", secs / 60, secs % 60)
}

fn clean(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The frontmost app and window over time, one entry per change.
fn timeline(bundle: &ScrawlBundle) -> Vec<(u64, String, String)> {
    let mut raw: Vec<(u64, String, String)> = Vec::new();
    for snapshot in &bundle.windows {
        if let Some(front) = snapshot.windows.iter().find(|w| w.frontmost) {
            raw.push((snapshot.t, clean(&front.app), clean(&front.title)));
        }
    }
    for event in &bundle.events {
        if let InteractionEvent::AppChange {
            t,
            app,
            window_title,
        } = event
        {
            raw.push((*t, clean(app), clean(window_title.as_deref().unwrap_or(""))));
        }
    }
    raw.sort_by_key(|entry| entry.0);
    let mut out: Vec<(u64, String, String)> = Vec::new();
    for entry in raw {
        if out
            .last()
            .is_none_or(|last| last.1 != entry.1 || last.2 != entry.2)
        {
            out.push(entry);
        }
    }
    out
}

fn items(bundle: &ScrawlBundle) -> (Vec<Item>, Thin) {
    // (time, order at a tie, item)
    let mut all: Vec<(u64, u8, Item)> = Vec::new();
    let timeline = timeline(bundle);
    let mut previous_app: Option<&str> = None;
    for (t, app, title) in &timeline {
        let line = if title.is_empty() {
            format!("{} window: {app}", stamp(*t))
        } else {
            format!("{} window: {app} — {title}", stamp(*t))
        };
        let app_change = previous_app.is_some_and(|prev| prev != app);
        previous_app = Some(app);
        all.push((
            *t,
            0,
            Item {
                t: *t,
                line,
                app_change,
                after_gap: false,
            },
        ));
    }

    let mut phrase: Option<(u64, u64, Vec<&str>)> = None;
    let mut phrases: Vec<(u64, Vec<&str>)> = Vec::new();
    for word in &bundle.transcript.words {
        let text = word.text.trim();
        if text.is_empty() {
            continue;
        }
        match &mut phrase {
            Some((_, end, words)) if word.t0.saturating_sub(*end) <= NARRATION_GAP_MS => {
                *end = (*end).max(word.t1);
                words.push(text);
            }
            _ => {
                if let Some((start, _, words)) = phrase.take() {
                    phrases.push((start, words));
                }
                phrase = Some((word.t0, word.t1, vec![text]));
            }
        }
    }
    if let Some((start, _, words)) = phrase {
        phrases.push((start, words));
    }
    let no_narration = phrases.is_empty();
    for (index, (t, words)) in phrases.into_iter().enumerate() {
        all.push((
            t,
            1,
            Item {
                t,
                line: format!("{} said: {}", stamp(t), words.join(" ")),
                app_change: false,
                after_gap: index > 0,
            },
        ));
    }

    let mut named_click = false;
    for event in &bundle.events {
        let InteractionEvent::Click {
            t,
            x,
            y,
            label,
            role,
            ..
        } = event
        else {
            continue;
        };
        let app = timeline
            .partition_point(|entry| entry.0 <= *t)
            .checked_sub(1)
            .map(|i| timeline[i].1.as_str())
            .filter(|app| !app.is_empty());
        let label = label.as_deref().map(clean).filter(|l| !l.is_empty());
        let mut line = format!("{} clicked", stamp(*t));
        match label {
            Some(label) => {
                named_click = true;
                match role.as_deref().map(clean).filter(|r| !r.is_empty()) {
                    Some(role) => line.push_str(&format!(" {role} \"{label}\"")),
                    None => line.push_str(&format!(" \"{label}\"")),
                }
            }
            None => line.push_str(&format!(" at {x:.2},{y:.2}")),
        }
        if let Some(app) = app {
            line.push_str(&format!(" in {app}"));
        }
        all.push((
            *t,
            2,
            Item {
                t: *t,
                line,
                app_change: false,
                after_gap: false,
            },
        ));
    }

    all.sort_by_key(|(t, order, _)| (*t, *order));
    let thin = Thin {
        no_narration,
        no_click_names: !named_click,
    };
    (all.into_iter().map(|(_, _, item)| item).collect(), thin)
}

fn truncate(line: &str, max: usize) -> String {
    if line.chars().count() <= max {
        return line.to_string();
    }
    let mut out: String = line.chars().take(max.saturating_sub(1)).collect();
    if max > 0 {
        out.push('…');
    }
    out
}

fn segments(items: &[Item], max_chars: usize, duration_ms: u64) -> Vec<Segment> {
    let max_chars = max_chars.max(1);
    let mut out: Vec<Segment> = Vec::new();
    let mut text = String::new();
    let mut len = 0usize;
    let mut start = 0u64;
    for item in items {
        let line = truncate(&item.line, max_chars);
        let line_len = line.chars().count();
        let breaks = !text.is_empty()
            && (len + 1 + line_len > max_chars
                || ((item.app_change || item.after_gap) && len >= max_chars / 2));
        if breaks {
            out.push(Segment {
                start_ms: start,
                end_ms: item.t,
                text: std::mem::take(&mut text),
                frames: Vec::new(),
            });
            len = 0;
        }
        if text.is_empty() {
            start = item.t;
            len = line_len;
            text = line;
        } else {
            text.push('\n');
            text.push_str(&line);
            len += 1 + line_len;
        }
    }
    if !text.is_empty() {
        let last = items.last().map_or(start, |item| item.t);
        out.push(Segment {
            start_ms: start,
            end_ms: duration_ms.max(last),
            text,
            frames: Vec::new(),
        });
    }
    out
}

/// `k` evenly spaced indices out of `n`, first and last included.
fn spaced(n: usize, k: usize) -> Vec<usize> {
    if k == 0 || n == 0 {
        return Vec::new();
    }
    if k >= n {
        return (0..n).collect();
    }
    if k == 1 {
        return vec![0];
    }
    (0..k)
        .map(|i| (i * (n - 1) + (k - 1) / 2) / (k - 1))
        .collect()
}

fn pick_frames(segments: &mut Vec<Segment>, bundle: &ScrawlBundle, dir: &Path, limits: &Limits) {
    if limits.frames_per_segment == 0 || limits.max_frames == 0 || bundle.keyframes.is_empty() {
        return;
    }
    if segments.is_empty() {
        // Nothing to read, but the pictures may still say something.
        segments.push(Segment {
            start_ms: 0,
            end_ms: bundle.duration_ms,
            text: String::new(),
            frames: Vec::new(),
        });
    }
    let mut keyframes: Vec<_> = bundle.keyframes.iter().collect();
    keyframes.sort_by_key(|k| k.t);
    // Each keyframe belongs to the last segment that started at or before it;
    // the first segment also takes any before it.
    let mut groups: Vec<Vec<&scrawl_capture::model::Keyframe>> = vec![Vec::new(); segments.len()];
    for keyframe in keyframes {
        let index = segments
            .partition_point(|s| s.start_ms <= keyframe.t)
            .saturating_sub(1);
        groups[index].push(keyframe);
    }
    let mut counts: Vec<usize> = groups
        .iter()
        .map(|g| g.len().min(limits.frames_per_segment))
        .collect();
    // Over the draft's budget: thin each segment in proportion, keeping one.
    let total: usize = counts.iter().sum();
    if total > limits.max_frames {
        for count in &mut counts {
            if *count > 0 {
                *count = (*count * limits.max_frames / total).max(1);
            }
        }
        while counts.iter().sum::<usize>() > limits.max_frames {
            let Some(i) = (0..counts.len())
                .filter(|&i| counts[i] > 1)
                .max_by_key(|&i| counts[i])
            else {
                break;
            };
            counts[i] -= 1;
        }
        // More segments with pictures than the budget: keep an even spread.
        let with: Vec<usize> = (0..counts.len()).filter(|&i| counts[i] > 0).collect();
        if with.len() > limits.max_frames {
            let keep = spaced(with.len(), limits.max_frames);
            for (pos, &i) in with.iter().enumerate() {
                if !keep.contains(&pos) {
                    counts[i] = 0;
                }
            }
        }
    }
    for ((segment, group), count) in segments.iter_mut().zip(&groups).zip(counts) {
        for i in spaced(group.len(), count) {
            let keyframe = group[i];
            if let Some(png) = load_frame(dir, &keyframe.file, limits.frame_width) {
                segment.frames.push(Frame {
                    t_ms: keyframe.t,
                    png,
                });
            }
        }
    }
}

fn load_frame(dir: &Path, file: &str, width: u32) -> Option<Vec<u8>> {
    let relative = Path::new(file);
    if relative.is_absolute()
        || relative
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return None;
    }
    let bytes = std::fs::read(dir.join(relative)).ok()?;
    let frame = decode_png(Cursor::new(bytes)).ok()?;
    encode_png(&downscale(&frame, width)).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apprentice::lessons::Media;
    use scrawl_capture::model::RgbaFrame;
    use serde_json::{Value, json};
    use std::path::PathBuf;

    struct Fixture {
        root: PathBuf,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn png(width: u32, height: u32) -> Vec<u8> {
        let rgba = vec![200u8; (width * height * 4) as usize];
        encode_png(&RgbaFrame::new(0, width, height, rgba)).unwrap()
    }

    /// A lesson bundle on disk: words as `(t0, t1, text)`, extra events,
    /// window snapshots as `(t, app, title)`, keyframe times (2000 px wide).
    fn lesson(
        name: &str,
        words: &[(u64, u64, &str)],
        events: Vec<Value>,
        windows: &[(u64, &str, &str)],
        keyframes: &[u64],
    ) -> (Fixture, Lesson) {
        let root = std::env::temp_dir().join(format!(
            "hex-evidence-{name}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let dir = root.join("1000.scrawl");
        std::fs::create_dir_all(dir.join("frames")).unwrap();
        let words: Vec<Value> = words
            .iter()
            .map(|(t0, t1, text)| json!({"t0": t0, "t1": t1, "text": text}))
            .collect();
        let windows: Vec<Value> = windows
            .iter()
            .map(|(t, app, title)| {
                json!({"t": t, "windows": [{
                    "id": 1, "app": app, "title": title,
                    "bounds": [0, 0, 10, 10], "layer": 0, "frontmost": true
                }]})
            })
            .collect();
        let frames: Vec<Value> = keyframes
            .iter()
            .enumerate()
            .map(|(i, t)| {
                std::fs::write(dir.join(format!("frames/frame-{i:02}.png")), png(2000, 40))
                    .unwrap();
                json!({"index": i, "t": t, "reason": "click",
                       "file": format!("frames/frame-{i:02}.png"), "event": "click"})
            })
            .collect();
        let manifest = json!({
            "version": 1, "id": "id-1", "createdAtMs": 1000, "durationMs": 600_000,
            "target": {"mode": "display", "displayId": 1,
                       "frame": {"x": 0, "y": 0, "width": 1, "height": 1, "scale": 2}},
            "transcript": {"source": "parakeet", "text": "", "segments": [], "words": words},
            "strokes": [], "marks": [], "events": events, "windows": windows,
            "keyframes": frames, "contactSheet": "sheet.jpg"
        });
        std::fs::write(dir.join("scrawl.json"), manifest.to_string()).unwrap();
        let lesson = Lesson {
            bundle: dir,
            id: "id-1".into(),
            created_ms: 1000,
            duration_ms: 600_000,
            media: Media::Cleaned,
            frames: keyframes.len(),
        };
        (Fixture { root }, lesson)
    }

    fn click(t: u64, label: Option<&str>, role: Option<&str>) -> Value {
        let mut event = json!({"kind": "click", "t": t, "x": 0.5, "y": 0.25});
        if let Some(label) = label {
            event["label"] = json!(label);
        }
        if let Some(role) = role {
            event["role"] = json!(role);
        }
        event
    }

    #[test]
    fn evidence_lines_read_in_time_order() {
        let (_fixture, lesson) = lesson(
            "order",
            &[(1000, 1200, "open"), (1300, 1600, "settings")],
            vec![click(3000, Some("Save"), Some("button"))],
            &[(2000, "Safari", "Inbox")],
            &[],
        );
        let evidence = build(&lesson, &Limits::for_vision()).unwrap();
        assert_eq!(evidence.segments.len(), 1);
        assert_eq!(
            evidence.segments[0].text,
            "[00:01] said: open settings\n[00:02] window: Safari — Inbox\n[00:03] clicked button \"Save\" in Safari"
        );
        assert!(!evidence.thin.no_narration);
        assert!(!evidence.thin.no_click_names);
    }

    #[test]
    fn segments_never_exceed_the_limit() {
        let words: Vec<(u64, u64, &str)> = (0..40)
            .map(|i| (i * 5000, i * 5000 + 200, "word"))
            .collect();
        let (_fixture, lesson) = lesson("limit", &words, vec![], &[], &[]);
        let limits = Limits {
            max_segment_chars: 60,
            frames_per_segment: 0,
            max_frames: 0,
            frame_width: 0,
        };
        let evidence = build(&lesson, &limits).unwrap();
        assert!(evidence.segments.len() > 3);
        for segment in &evidence.segments {
            assert!(segment.text.chars().count() <= 60, "{}", segment.text);
        }
        let lines: usize = evidence
            .segments
            .iter()
            .map(|s| s.text.lines().count())
            .sum();
        assert_eq!(lines, 40);
    }

    #[test]
    fn one_long_line_is_truncated_not_dropped() {
        let long = "a".repeat(500);
        let (_fixture, lesson) = lesson("long", &[(0, 100, &long)], vec![], &[], &[]);
        let limits = Limits {
            max_segment_chars: 80,
            frames_per_segment: 0,
            max_frames: 0,
            frame_width: 0,
        };
        let evidence = build(&lesson, &limits).unwrap();
        assert_eq!(evidence.segments.len(), 1);
        assert!(evidence.segments[0].text.starts_with("[00:00] said: aaa"));
        assert!(evidence.segments[0].text.chars().count() <= 80);
    }

    #[test]
    fn frames_are_downscaled_and_capped() {
        // Each word starts a new segment of 60 chars; 16 frames, 4 a segment.
        let words: Vec<(u64, u64, &str)> = (0..4)
            .map(|i| {
                (
                    i * 10_000,
                    i * 10_000 + 200,
                    "a long enough spoken phrase to fill",
                )
            })
            .collect();
        let keyframes: Vec<u64> = (0..16).map(|i| i * 2_500).collect();
        let (_fixture, lesson) = lesson("frames", &words, vec![], &[], &keyframes);
        let limits = Limits {
            max_segment_chars: 60,
            frames_per_segment: 4,
            max_frames: 12,
            frame_width: 1024,
        };
        let evidence = build(&lesson, &limits).unwrap();
        assert_eq!(evidence.segments.len(), 4);
        let frames: Vec<&Frame> = evidence.segments.iter().flat_map(|s| &s.frames).collect();
        assert_eq!(frames.len(), 12);
        for segment in &evidence.segments {
            assert!(!segment.frames.is_empty());
        }
        for frame in frames {
            let decoded = decode_png(Cursor::new(&frame.png)).unwrap();
            assert!(decoded.width <= 1024 && decoded.width > 0);
        }
    }

    #[test]
    fn text_limits_carry_no_frames() {
        let (_fixture, lesson) = lesson("text", &[(0, 100, "hi")], vec![], &[], &[0, 1000, 2000]);
        let evidence = build(&lesson, &Limits::for_text(0)).unwrap();
        assert!(evidence.segments.iter().all(|s| s.frames.is_empty()));
        assert_eq!(Limits::for_text(0).max_segment_chars, 6_000);
        assert_eq!(Limits::for_text(8_000).max_segment_chars, 12_000);
    }

    #[test]
    fn thin_evidence_is_flagged() {
        let (_fixture, lesson) = lesson(
            "thin",
            &[],
            vec![click(1000, None, None), click(2000, None, None)],
            &[],
            &[],
        );
        let evidence = build(&lesson, &Limits::for_vision()).unwrap();
        assert!(evidence.thin.no_narration);
        assert!(evidence.thin.no_click_names);
    }

    #[test]
    fn secure_or_missing_labels_are_left_out() {
        let (_fixture, lesson) = lesson(
            "secure",
            &[],
            vec![click(1000, None, Some("textField"))],
            &[],
            &[],
        );
        let evidence = build(&lesson, &Limits::for_vision()).unwrap();
        assert_eq!(evidence.segments[0].text, "[00:01] clicked at 0.50,0.25");
        assert!(!evidence.segments[0].text.contains('"'));
    }

    #[test]
    fn a_missing_frame_is_skipped_and_an_expired_lesson_errors() {
        let (_fixture, lesson) = lesson("missing", &[(0, 100, "hi")], vec![], &[], &[0, 1000]);
        std::fs::remove_file(lesson.bundle.join("frames/frame-00.png")).unwrap();
        let evidence = build(&lesson, &Limits::for_vision()).unwrap();
        assert_eq!(evidence.segments[0].frames.len(), 1);
        std::fs::remove_file(lesson.bundle.join("scrawl.json")).unwrap();
        assert!(build(&lesson, &Limits::for_vision()).is_err());
    }
}
