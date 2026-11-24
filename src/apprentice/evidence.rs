//! One lesson as bounded evidence for a local model: time-ordered text lines
//! in segments, plus downscaled key-frame PNGs. Video and audio never leave
//! the bundle; frames are for vision models only.

use std::io::Cursor;
use std::path::{Component, Path};

use scrawl_capture::agent_context::mark_text;
use scrawl_capture::model::{InteractionEvent, ScrawlBundle};
use scrawl_capture::render::{decode_png, downscale, encode_png};
use scrawl_capture::sketch::direction;

use super::execution::{self, RunPlan};
use super::lessons::Lesson;
use crate::capture_host::load_capture;

/// Words further apart than this start a new `said:` line.
const NARRATION_GAP_MS: u64 = 2_000;
/// An unnamed drag shorter than this says nothing the pictures don't.
const MIN_DRAG: f32 = 0.15;
/// Repeats of one action closer together than this read as one line.
const REPEAT_MS: u64 = 3_000;
/// Context for a model whose size is unknown.
const UNKNOWN_CONTEXT_TOKENS: usize = 4_000;

/// What a model is sent for one lesson.
#[derive(Debug)]
pub struct Evidence {
    pub segments: Vec<Segment>,
    pub thin: Thin,
    /// Bounded actions for in-app replay, derived from structured events.
    pub plan: Option<RunPlan>,
}

/// A stretch of the lesson: its lines and the key frames inside it.
#[derive(Debug)]
pub struct Segment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
    pub frames: Vec<Frame>,
}

/// A downscaled key frame as PNG bytes.
#[derive(Debug)]
pub struct Frame {
    /// When it was taken, so the draft's prompt can place it in the lesson.
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

/// The lesson's evidence. Fails only when its bundle can't be loaded.
pub fn build(lesson: &Lesson, limits: &Limits) -> Result<Evidence, String> {
    let bundle = load_capture(&lesson.bundle).ok_or_else(|| {
        "This lesson's notes are gone, so there is nothing to learn from".to_string()
    })?;
    let (items, thin) = items(&bundle);
    let mut segments = segments(&items, limits.max_segment_chars, bundle.duration_ms);
    pick_frames(&mut segments, &bundle, &lesson.bundle, limits);
    let plan = execution::plan_from_bundle(&bundle);
    Ok(Evidence {
        segments,
        thin,
        plan,
    })
}

fn stamp(t_ms: u64) -> String {
    let secs = t_ms / 1000;
    format!("[{:02}:{:02}]", secs / 60, secs % 60)
}

fn clean(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A label that already quotes its own parts isn't wrapped again.
fn quote(label: &str) -> String {
    let label = clean(label);
    if label.contains('“') || label.starts_with('<') {
        label
    } else {
        format!("\"{label}\"")
    }
}

/// The frontmost window under a normalized point at `t`, from the last snapshot before it.
fn window_at(bundle: &ScrawlBundle, t: u64, x: f32, y: f32) -> Option<String> {
    let snapshot = bundle
        .windows
        .iter()
        .filter(|snapshot| snapshot.t <= t)
        .max_by_key(|snapshot| snapshot.t)?;
    let (px, py) = (
        x * bundle.target.frame.width as f32,
        y * bundle.target.frame.height as f32,
    );
    let window = snapshot
        .windows
        .iter()
        .filter(|window| {
            let [x0, y0, width, height] = window.bounds;
            px >= x0 as f32
                && px < (x0 + width) as f32
                && py >= y0 as f32
                && py < (y0 + height) as f32
        })
        // The frontmost window wins; otherwise the smaller one is likely the panel on top.
        .min_by_key(|window| {
            (
                !window.frontmost,
                window.bounds[2].saturating_mul(window.bounds[3]),
            )
        })?;
    let app = clean(&window.app);
    let title = clean(&window.title);
    Some(if title.is_empty() {
        app
    } else {
        format!("{app} — {title}")
    })
}

fn app_at(timeline: &[(u64, String, String)], t: u64) -> Option<&str> {
    timeline
        .partition_point(|entry| entry.0 <= t)
        .checked_sub(1)
        .map(|i| timeline[i].1.as_str())
        .filter(|app| !app.is_empty())
}

/// Interaction lines, with quick repeats of one action folded into one.
/// The flag says whether a line names something worth learning from.
fn event_lines(
    bundle: &ScrawlBundle,
    timeline: &[(u64, String, String)],
) -> Vec<(u64, String, bool)> {
    let mut events: Vec<&InteractionEvent> = bundle.events.iter().collect();
    events.sort_by_key(|event| event.timestamp_ms());
    let mut out = Vec::new();
    let mut i = 0;
    while i < events.len() {
        match events[i] {
            InteractionEvent::Key { t, shortcut } => {
                let mut count = 1usize;
                while let Some(InteractionEvent::Key {
                    t: next,
                    shortcut: other,
                    ..
                }) = events.get(i + 1).copied()
                {
                    if other == shortcut && next.saturating_sub(*t) <= REPEAT_MS {
                        count += 1;
                        i += 1;
                    } else {
                        break;
                    }
                }
                let shortcut = clean(shortcut);
                let line = if count > 1 {
                    format!("{} pressed {shortcut} ×{count}", stamp(*t))
                } else {
                    format!("{} pressed {shortcut}", stamp(*t))
                };
                out.push((*t, line, false));
            }
            InteractionEvent::Scroll {
                t, dx, dy, label, ..
            } => {
                let (mut sum_x, mut sum_y) = (*dx, *dy);
                let mut end = *t;
                while let Some(InteractionEvent::Scroll {
                    t: next,
                    dx,
                    dy,
                    label: other,
                    ..
                }) = events.get(i + 1).copied()
                {
                    if other == label && next.saturating_sub(end) <= REPEAT_MS {
                        sum_x += dx;
                        sum_y += dy;
                        end = *next;
                        i += 1;
                    } else {
                        break;
                    }
                }
                let mut line = format!("{} scrolled", stamp(*t));
                let named = label.as_deref().is_some_and(|l| !clean(l).is_empty());
                if let Some(label) = label.as_deref()
                    && named
                {
                    line.push_str(&format!(" {}", quote(label)));
                }
                line.push_str(&format!(" {}", direction(sum_x, sum_y, 0.0)));
                out.push((*t, line, named));
            }
            InteractionEvent::Click {
                t,
                x,
                y,
                label,
                role,
                ..
            } => {
                let app = app_at(timeline, *t);
                let label = label.as_deref().map(clean).filter(|l| !l.is_empty());
                let mut line = format!("{} clicked", stamp(*t));
                match &label {
                    Some(label) => match role.as_deref().map(clean).filter(|r| !r.is_empty()) {
                        Some(role) => line.push_str(&format!(" {role} {}", quote(label))),
                        None => line.push_str(&format!(" {}", quote(label))),
                    },
                    None => line.push_str(&format!(" at {x:.2},{y:.2}")),
                }
                if let Some(app) = app {
                    line.push_str(&format!(" in {app}"));
                }
                out.push((*t, line, label.is_some()));
            }
            InteractionEvent::Drag {
                t,
                x0,
                y0,
                x1,
                y1,
                label,
                button,
                ..
            } => {
                let name = label
                    .as_deref()
                    .map(clean)
                    .filter(|name| !name.is_empty() && !name.starts_with("long press"));
                let (dx, dy) = (x1 - x0, y1 - y0);
                if name.is_none() && dx.hypot(dy) < MIN_DRAG {
                    i += 1;
                    continue;
                }
                let scrub = name.as_deref().is_some_and(|name| {
                    ["slider", "scroll bar", "scrubber", "timeline", "progress"]
                        .iter()
                        .any(|word| name.to_lowercase().contains(word))
                });
                let verb = if button.as_deref() == Some("touch") {
                    "swiped"
                } else if scrub {
                    "scrubbed"
                } else {
                    "dragged"
                };
                let mut line = format!("{} {verb}", stamp(*t));
                if let Some(name) = &name {
                    line.push_str(&format!(" {}", quote(name)));
                }
                let way = direction(dx, dy, 0.04);
                if verb != "scrubbed" && way != "a little" {
                    line.push_str(&format!(" {way}"));
                }
                if let Some(window) = window_at(bundle, *t, *x1, *y1) {
                    line.push_str(&format!(", let go over {window}"));
                }
                out.push((*t, line, name.is_some()));
            }
            _ => {}
        }
        i += 1;
    }
    out
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

    // Word timings are best; a transcript that only kept segments still reads as narration.
    let speech: Vec<(u64, u64, String)> = if bundle.transcript.words.is_empty() {
        bundle
            .transcript
            .segments
            .iter()
            .map(|segment| (segment.t0, segment.t1, clean(&segment.text)))
            .filter(|(_, _, text)| !text.is_empty())
            .collect()
    } else {
        bundle
            .transcript
            .words
            .iter()
            .map(|word| (word.t0, word.t1, word.text.trim().to_owned()))
            .filter(|(_, _, text)| !text.is_empty())
            .collect()
    };
    let mut phrase: Option<(u64, u64, Vec<String>)> = None;
    let mut phrases: Vec<(u64, Vec<String>)> = Vec::new();
    for (t0, t1, text) in speech {
        match &mut phrase {
            Some((_, end, words)) if t0.saturating_sub(*end) <= NARRATION_GAP_MS => {
                *end = (*end).max(t1);
                words.push(text);
            }
            _ => {
                if let Some((start, _, words)) = phrase.take() {
                    phrases.push((start, words));
                }
                phrase = Some((t0, t1, vec![text]));
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

    let mut named = false;
    for (t, line, has_name) in event_lines(bundle, &timeline) {
        named |= has_name;
        all.push((
            t,
            2,
            Item {
                t,
                line,
                app_change: false,
                after_gap: false,
            },
        ));
    }

    for mark in &bundle.marks {
        let t = mark.t0;
        named |= mark.on.as_deref().is_some_and(|on| !clean(on).is_empty());
        all.push((
            t,
            3,
            Item {
                t,
                line: format!("{} drew: {}", stamp(t), mark_text(mark)),
                app_change: false,
                after_gap: false,
            },
        ));
    }

    all.sort_by_key(|(t, order, _)| (*t, *order));
    let thin = Thin {
        no_narration,
        no_click_names: !named,
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
pub(super) mod tests {
    use super::*;
    use crate::apprentice::lessons::Media;
    use scrawl_capture::model::RgbaFrame;
    use serde_json::{Value, json};
    use std::path::PathBuf;

    /// A lesson's folder, removed when dropped.
    pub(in crate::apprentice) struct Fixture {
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
    pub(in crate::apprentice) fn lesson(
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

    /// Rewrites the lesson's `scrawl.json` through `change`.
    fn patch(lesson: &Lesson, change: impl FnOnce(&mut Value)) {
        let path = lesson.bundle.join("scrawl.json");
        let mut manifest: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        change(&mut manifest);
        std::fs::write(path, manifest.to_string()).unwrap();
    }

    fn drag(t: u64, from: [f32; 2], to: [f32; 2], label: Option<&str>) -> Value {
        let mut event = json!({"kind": "drag", "t": t, "t1": t + 500,
            "x0": from[0], "y0": from[1], "x1": to[0], "y1": to[1]});
        if let Some(label) = label {
            event["label"] = json!(label);
        }
        event
    }

    fn text(lesson: &Lesson) -> String {
        let evidence = build(lesson, &Limits::for_vision()).unwrap();
        evidence
            .segments
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn a_drag_names_what_moved_and_the_window_it_was_let_go_over() {
        let (_fixture, lesson) = lesson(
            "drag",
            &[],
            vec![drag(
                3000,
                [0.1, 0.1],
                [0.8, 0.6],
                Some("cell “Rechnung.pdf”"),
            )],
            &[],
            &[],
        );
        patch(&lesson, |manifest| {
            manifest["target"]["frame"] =
                json!({"x": 0, "y": 0, "width": 100, "height": 100, "scale": 2});
            manifest["windows"] = json!([{"t": 0, "windows": [
                {"id": 1, "app": "Finder", "title": "Downloads",
                 "bounds": [0, 0, 50, 100], "layer": 0, "frontmost": true},
                {"id": 2, "app": "Finder", "title": "Invoices 2026-11",
                 "bounds": [50, 0, 50, 100], "layer": 0}
            ]}]);
        });
        assert_eq!(
            text(&lesson),
            "[00:00] window: Finder — Downloads\n\
             [00:03] dragged cell “Rechnung.pdf” down and right, let go over Finder — Invoices 2026-11"
        );
    }

    #[test]
    fn a_short_drag_nothing_names_is_left_out() {
        let (_fixture, lesson) = lesson(
            "short-drag",
            &[],
            vec![
                drag(1000, [0.5, 0.5], [0.55, 0.5], None),
                drag(5000, [0.1, 0.5], [0.9, 0.5], None),
            ],
            &[],
            &[],
        );
        assert_eq!(text(&lesson), "[00:05] dragged right");
    }

    #[test]
    fn a_drawing_reads_as_its_shape_and_what_it_is_on() {
        let (_fixture, lesson) = lesson("mark", &[], vec![], &[], &[]);
        patch(&lesson, |manifest| {
            manifest["strokes"] =
                json!([{"t0": 4000, "t1": 4500, "fw": 100, "fh": 100, "pts": []}]);
            manifest["marks"] = json!([{"id": "m1", "strokes": [0], "t0": 4000, "t1": 4500,
                "boxNorm": [0.1, 0.1, 0.2, 0.2], "shape": "circle",
                "on": "button “Save”", "description": "circled region"}]);
        });
        assert_eq!(text(&lesson), "[00:04] drew: circled button “Save”");
    }

    #[test]
    fn scrolls_and_shortcuts_are_lines_and_repeats_fold() {
        let key = |t: u64| json!({"kind": "key", "t": t, "shortcut": "⌘T"});
        let scroll = |t: u64, dy: f32| json!({"kind": "scroll", "t": t, "x": 0.5, "y": 0.5, "dx": 0.0, "dy": dy});
        let (_fixture, lesson) = lesson(
            "keys",
            &[],
            vec![
                key(1000),
                key(1500),
                key(2000),
                scroll(5000, 0.2),
                scroll(5400, 0.3),
            ],
            &[],
            &[],
        );
        assert_eq!(
            text(&lesson),
            "[00:01] pressed ⌘T ×3\n[00:05] scrolled down"
        );
    }

    #[test]
    fn a_label_that_quotes_itself_is_not_quoted_again() {
        let (_fixture, lesson) = lesson(
            "quoted",
            &[],
            vec![click(1000, Some("“Save” #save"), Some("button"))],
            &[],
            &[],
        );
        assert_eq!(text(&lesson), "[00:01] clicked button “Save” #save");
    }

    #[test]
    fn narration_without_word_times_reads_its_segments() {
        let (_fixture, lesson) = lesson("segments", &[], vec![], &[], &[]);
        patch(&lesson, |manifest| {
            manifest["transcript"]["segments"] =
                json!([{"t0": 1000, "t1": 2000, "text": "open  settings"}]);
        });
        let evidence = build(&lesson, &Limits::for_vision()).unwrap();
        assert_eq!(evidence.segments[0].text, "[00:01] said: open settings");
        assert!(!evidence.thin.no_narration);
    }

    #[test]
    fn a_named_drag_is_a_name_to_learn_from() {
        let (_fixture, lesson) = lesson(
            "named-drag",
            &[],
            vec![drag(1000, [0.1, 0.1], [0.8, 0.6], Some("cell “a.pdf”"))],
            &[],
            &[],
        );
        let evidence = build(&lesson, &Limits::for_vision()).unwrap();
        assert!(!evidence.thin.no_click_names);
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
