//! In-app replay of a learned skill's bounded plan. The plan comes from the
//! lesson's structured interactions in `skill.json`; the saved `SKILL.md`
//! is prose for an agent and is never read here. A person sees the plan and
//! confirms before any step runs, and every step can be stopped.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use scrawl_capture::model::{InteractionEvent, ScrawlBundle};
use serde::{Deserialize, Serialize};

use super::events::{self, ApprenticeAction};
use crate::desktop::i18n::{tr, tr_fill};

/// No learned skill may perform more than this many steps in one run.
pub const MAX_STEPS: usize = 40;
/// A short pause between steps, so a run is visible and can be stopped.
const STEP_GAP: Duration = Duration::from_millis(180);
/// Matches the evidence reader: quick repeats read as one action, and an
/// unnamed short drag says nothing worth replaying.
const REPEAT_MS: u64 = 3_000;
const MIN_DRAG: f32 = 0.15;

/// The display a lesson recorded, in global top-left points.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanFrame {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl PartialEq for PlanFrame {
    fn eq(&self, other: &Self) -> bool {
        [self.x, self.y, self.width, self.height].map(f64::to_bits)
            == [other.x, other.y, other.width, other.height].map(f64::to_bits)
    }
}

impl Eq for PlanFrame {}

/// One action Apprentice can perform, with positions normalized to
/// [`PlanFrame`] exactly as the lesson recorded them.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlanStep {
    Activate {
        /// The app's localized name. Never a window title or page address.
        app: String,
    },
    Click {
        x: f32,
        y: f32,
    },
    Key {
        shortcut: String,
    },
    Scroll {
        x: f32,
        y: f32,
        dx: f32,
        dy: f32,
    },
    Drag {
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
    },
}

impl PartialEq for PlanStep {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Activate { app: a }, Self::Activate { app: b }) => a == b,
            (Self::Click { x: x1, y: y1 }, Self::Click { x: x2, y: y2 }) => {
                x1.to_bits() == x2.to_bits() && y1.to_bits() == y2.to_bits()
            }
            (Self::Key { shortcut: a }, Self::Key { shortcut: b }) => a == b,
            (
                Self::Scroll {
                    x: x1,
                    y: y1,
                    dx: dx1,
                    dy: dy1,
                },
                Self::Scroll {
                    x: x2,
                    y: y2,
                    dx: dx2,
                    dy: dy2,
                },
            ) => [x1, y1, dx1, dy1].map(|v| v.to_bits()) == [x2, y2, dx2, dy2].map(|v| v.to_bits()),
            (
                Self::Drag {
                    x0: a0,
                    y0: b0,
                    x1: a1,
                    y1: b1,
                },
                Self::Drag {
                    x0: c0,
                    y0: d0,
                    x1: c1,
                    y1: d1,
                },
            ) => [a0, b0, a1, b1].map(|v| v.to_bits()) == [c0, d0, c1, d1].map(|v| v.to_bits()),
            _ => false,
        }
    }
}

impl Eq for PlanStep {}

impl PlanStep {
    /// What the person sees in the plan. This is UI copy, never event text.
    pub fn label(&self) -> String {
        match self {
            Self::Activate { app } => tr_fill("Open {}", app),
            Self::Click { .. } => tr("Click").to_owned(),
            Self::Key { shortcut } => tr_fill("Press {}", shortcut),
            Self::Scroll { .. } => tr("Scroll").to_owned(),
            Self::Drag { .. } => tr("Drag").to_owned(),
        }
    }
}

/// The ordered actions a skill may replay, stored only in `skill.json`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunPlan {
    pub frame: PlanFrame,
    pub steps: Vec<PlanStep>,
}

impl RunPlan {
    /// A normalized point in the recorded display's global points.
    pub fn point(&self, x: f32, y: f32) -> (f64, f64) {
        (
            self.frame.x + x.clamp(0.0, 1.0) as f64 * self.frame.width,
            self.frame.y + y.clamp(0.0, 1.0) as f64 * self.frame.height,
        )
    }
}

/// The runnable actions in a lesson, in time order. Only structured events
/// become steps: narration, window titles, and page addresses never do. A
/// lesson with no such actions leaves the skill without a run plan.
pub fn plan_from_bundle(bundle: &ScrawlBundle) -> Option<RunPlan> {
    let frame = bundle.target.frame;
    let mut events = bundle.events.iter().collect::<Vec<_>>();
    events.sort_by_key(|event| event.timestamp_ms());
    let mut steps = Vec::new();
    let mut app = String::new();
    let mut last_key: Option<(String, u64)> = None;
    for event in events {
        let finished = steps.len() >= MAX_STEPS;
        if finished {
            break;
        }
        match event {
            InteractionEvent::AppChange { app: name, .. } => {
                let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
                if !name.is_empty() && name != app {
                    steps.push(PlanStep::Activate { app: name.clone() });
                    app = name;
                }
            }
            InteractionEvent::Click { x, y, .. } => steps.push(PlanStep::Click { x: *x, y: *y }),
            InteractionEvent::Key { t, shortcut } => {
                let shortcut = shortcut.split_whitespace().collect::<String>();
                if shortcut.is_empty()
                    || last_key.as_ref().is_some_and(|(previous, at)| {
                        previous == &shortcut && t.saturating_sub(*at) <= REPEAT_MS
                    })
                {
                    continue;
                }
                last_key = Some((shortcut.clone(), *t));
                steps.push(PlanStep::Key { shortcut });
            }
            InteractionEvent::Scroll { x, y, dx, dy, .. } => {
                if *dx != 0.0 || *dy != 0.0 {
                    steps.push(PlanStep::Scroll {
                        x: *x,
                        y: *y,
                        dx: *dx,
                        dy: *dy,
                    });
                }
            }
            InteractionEvent::Drag {
                x0,
                y0,
                x1,
                y1,
                label,
                ..
            } => {
                let named = label.as_deref().is_some_and(|name| {
                    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
                    !name.is_empty() && !name.starts_with("long press")
                });
                let (dx, dy) = (*x1 - *x0, *y1 - *y0);
                if named || dx.hypot(dy) >= MIN_DRAG {
                    steps.push(PlanStep::Drag {
                        x0: *x0,
                        y0: *y0,
                        x1: *x1,
                        y1: *y1,
                    });
                }
            }
            InteractionEvent::Point { .. } | InteractionEvent::AppEvent { .. } => {}
        }
    }
    (!steps.is_empty()).then_some(RunPlan {
        frame: PlanFrame {
            x: frame.x,
            y: frame.y,
            width: frame.width,
            height: frame.height,
        },
        steps,
    })
}

/// What one run reports, in order; the last is `Done`, `Failed`, or
/// `Cancelled`. No step's text, coordinates, or app name is ever reported.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Progress {
    Step { n: usize, of: usize },
    Done,
    Failed(String),
    Cancelled,
}

/// Performs one action. Tests inject a recording driver; the app uses
/// [`RealDriver`], which can be refused where the seams don't exist.
pub trait Driver: Send + Sync {
    fn perform(&self, step: &PlanStep, plan: &RunPlan) -> Result<(), String>;
}

/// Checks that a plan is safe to perform. A dry run uses this too: it shows
/// the same refusal as a real run without performing a step.
pub fn validate(plan: &RunPlan) -> Result<(), String> {
    if plan.steps.is_empty() {
        return Err(tr("This skill has no runnable steps").to_owned());
    }
    if plan.steps.len() > MAX_STEPS {
        return Err(tr("This skill has too many steps to run safely").to_owned());
    }
    Ok(())
}

/// Runs `plan` on a thread of its own, checking `cancel` before every step.
/// An empty or overlarge plan ends without starting a thread.
pub fn execute(
    slug: &str,
    plan: RunPlan,
    driver: Arc<dyn Driver>,
    cancel: Arc<AtomicBool>,
) -> Receiver<Progress> {
    let (send, received) = mpsc::channel();
    if let Err(error) = validate(&plan) {
        let _ = send.send(Progress::Failed(error));
        return received;
    }
    let slug = slug.to_owned();
    let running = send.clone();
    let spawned = thread::Builder::new()
        .name("apprentice-run".into())
        .spawn(move || job(&slug, &plan, driver.as_ref(), &cancel, &running));
    if let Err(error) = spawned {
        tracing::warn!(%error, "could not start a skill run");
        let _ = send.send(Progress::Failed(
            tr("Apprentice couldn't start the skill").to_owned(),
        ));
    }
    received
}

fn job(
    slug: &str,
    plan: &RunPlan,
    driver: &dyn Driver,
    cancel: &AtomicBool,
    send: &Sender<Progress>,
) {
    let of = plan.steps.len();
    let count = u32::try_from(of).unwrap_or(u32::MAX);
    events::emit(ApprenticeAction::SkillRunStarted, slug, Some(count));
    for (index, step) in plan.steps.iter().enumerate() {
        if cancel.load(Ordering::Acquire) {
            events::emit(ApprenticeAction::SkillRunCancelled, slug, Some(count));
            let _ = send.send(Progress::Cancelled);
            return;
        }
        let n = index + 1;
        let _ = send.send(Progress::Step { n, of });
        if let Err(error) = driver.perform(step, plan) {
            tracing::warn!(step = n, "a skill run stopped");
            events::emit(ApprenticeAction::SkillRunFailed, slug, Some(count));
            let _ = send.send(Progress::Failed(error));
            return;
        }
        events::emit(
            ApprenticeAction::SkillRunStep,
            slug,
            Some(u32::try_from(n).unwrap_or(0)),
        );
        if index + 1 < of {
            thread::sleep(STEP_GAP);
        }
    }
    events::emit(ApprenticeAction::SkillRunFinished, slug, Some(count));
    let _ = send.send(Progress::Done);
}

/// The app's real actions. On macOS these are synthesized through the same
/// CoreGraphics and AppKit seams the capture engine uses; elsewhere a run is
/// refused instead of pretending to have acted.
pub struct RealDriver;

impl Driver for RealDriver {
    fn perform(&self, step: &PlanStep, plan: &RunPlan) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        {
            macos::perform(step, plan)
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (step, plan);
            Err(tr("Running learned skills is available on macOS only").to_owned())
        }
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::c_void;
    use std::ptr;
    use std::thread;
    use std::time::Duration;

    use objc2::rc::autoreleasepool;
    use objc2_app_kit::NSWorkspace;

    use super::{PlanStep, RunPlan};
    use crate::desktop::i18n::{tr, tr_fill};
    use crate::keyboard;

    #[derive(Clone, Copy)]
    #[repr(C)]
    struct CGPoint {
        x: f64,
        y: f64,
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn CGEventCreateMouseEvent(
            source: *mut c_void,
            kind: u32,
            point: CGPoint,
            button: u32,
        ) -> *mut c_void;
        fn CGEventCreateScrollWheelEvent(
            source: *mut c_void,
            unit: u32,
            count: u32,
            wheel1: i32,
            wheel2: i32,
            wheel3: i32,
        ) -> *mut c_void;
        fn CGEventPost(tap: u32, event: *mut c_void);
        fn CGWarpMouseCursorPosition(point: CGPoint) -> i32;
        fn CFRelease(value: *const c_void);
    }

    const HID_TAP: u32 = 0;
    const LEFT_DOWN: u32 = 1;
    const LEFT_UP: u32 = 2;
    const PIXEL_UNIT: u32 = 1;
    const MOUSE_GAP: Duration = Duration::from_millis(12);

    pub(super) fn perform(step: &PlanStep, plan: &RunPlan) -> Result<(), String> {
        match step {
            PlanStep::Activate { app } => activate(app),
            PlanStep::Click { x, y } => {
                let point = point(plan, *x, *y);
                scrawl_capture::capture::post_synthetic_click(point.x, point.y);
                Ok(())
            }
            PlanStep::Key { shortcut } => key(shortcut),
            PlanStep::Scroll { x, y, dx, dy } => scroll(plan, *x, *y, *dx, *dy),
            PlanStep::Drag { x0, y0, x1, y1 } => drag(plan, *x0, *y0, *x1, *y1),
        }
    }

    fn point(plan: &RunPlan, x: f32, y: f32) -> CGPoint {
        let (x, y) = plan.point(x, y);
        CGPoint { x, y }
    }

    /// Brings a running app forward by its localized name, the name the
    /// lesson recorded. Nothing is launched behind the person's back: an app
    /// that isn't running asks them to open it first.
    fn activate(name: &str) -> Result<(), String> {
        let wanted = name.trim();
        if wanted.is_empty() {
            return Ok(());
        }
        let found = autoreleasepool(|_| {
            NSWorkspace::sharedWorkspace()
                .runningApplications()
                .iter()
                .find(|application| {
                    application
                        .localizedName()
                        .is_some_and(|found| found.to_string() == wanted)
                })
                .is_some()
        });
        if !found {
            return Err(tr_fill("Open {} first, then run the skill again", wanted));
        }
        let status = std::process::Command::new("open")
            .arg("-a")
            .arg(wanted)
            .status();
        match status {
            Ok(status) if status.success() => Ok(()),
            Ok(_) => Err(tr_fill("Couldn't open {}", wanted)),
            Err(_) => Err(tr_fill("Couldn't open {}", wanted)),
        }
    }

    /// Shortcut labels from the capture are `⌃⌥⇧⌘K`, names such as `Return`,
    /// or one bare character. Only a single key plus modifiers is runnable.
    fn key(shortcut: &str) -> Result<(), String> {
        let (key, control, option, shift, command) = parse_shortcut(shortcut)?;
        keyboard::post_shortcut_key(key, control, option, shift, command)
            .map_err(|error| error.to_string())
    }

    fn parse_shortcut(shortcut: &str) -> Result<(u16, bool, bool, bool, bool), String> {
        let mut control = false;
        let mut option = false;
        let mut shift = false;
        let mut command = false;
        let mut key = String::new();
        for character in shortcut.trim().chars() {
            match character {
                '⌃' => control = true,
                '⌥' => option = true,
                '⇧' => shift = true,
                '⌘' => command = true,
                c if c.is_control() || c.is_whitespace() => {}
                c => key.push(c),
            }
        }
        let key = match key.chars().count() {
            1 => named_key(key.chars().next().unwrap_or(' '), command)?,
            0 => return Err(unpressable()),
            _ => named_key_word(&key)?,
        };
        Ok((key, control, option, shift, command))
    }

    fn unpressable() -> String {
        tr("This skill has a shortcut Apprentice can't press").to_owned()
    }

    fn named_key(character: char, command: bool) -> Result<u16, String> {
        let code = match character {
            '↵' | '↩' => keyboard::RETURN_KEY_CODE,
            '⇥' => keyboard::TAB_KEY_CODE,
            '⌫' => keyboard::DELETE_KEY_CODE,
            '⎋' => keyboard::ESCAPE_KEY_CODE,
            '←' => keyboard::LEFT_KEY_CODE,
            '→' => keyboard::RIGHT_KEY_CODE,
            '↑' => keyboard::UP_KEY_CODE,
            '↓' => keyboard::DOWN_KEY_CODE,
            ' ' => keyboard::SPACE_KEY_CODE,
            _ => keyboard::shortcut_key_code(character, command)
                .map_err(|error| error.to_string())?,
        };
        Ok(code)
    }

    fn named_key_word(name: &str) -> Result<u16, String> {
        let code = match name.trim().to_ascii_lowercase().as_str() {
            "return" | "enter" => keyboard::RETURN_KEY_CODE,
            "tab" => keyboard::TAB_KEY_CODE,
            "delete" | "backspace" => keyboard::DELETE_KEY_CODE,
            "escape" | "esc" => keyboard::ESCAPE_KEY_CODE,
            "space" => keyboard::SPACE_KEY_CODE,
            "left" => keyboard::LEFT_KEY_CODE,
            "right" => keyboard::RIGHT_KEY_CODE,
            "up" => keyboard::UP_KEY_CODE,
            "down" => keyboard::DOWN_KEY_CODE,
            _ => return Err(tr("This skill has a shortcut Apprentice can't press").to_owned()),
        };
        Ok(code)
    }

    fn scroll(plan: &RunPlan, x: f32, y: f32, dx: f32, dy: f32) -> Result<(), String> {
        let point = point(plan, x, y);
        let (x, y) = (clamp_wheel(dx), clamp_wheel(-dy));
        if x == 0 && y == 0 {
            return Ok(());
        }
        unsafe {
            if CGWarpMouseCursorPosition(point) != 0 {
                return Err(tr("Couldn't move the pointer for a scroll").to_owned());
            }
            let event = CGEventCreateScrollWheelEvent(ptr::null_mut(), PIXEL_UNIT, 2, x, y, 0);
            if event.is_null() {
                return Err(tr("Couldn't scroll for this skill").to_owned());
            }
            CGEventPost(HID_TAP, event);
            CFRelease(event.cast());
        }
        Ok(())
    }

    fn drag(plan: &RunPlan, x0: f32, y0: f32, x1: f32, y1: f32) -> Result<(), String> {
        let start = point(plan, x0, y0);
        let end = point(plan, x1, y1);
        unsafe {
            let down = CGEventCreateMouseEvent(ptr::null_mut(), LEFT_DOWN, start, 0);
            let up = CGEventCreateMouseEvent(ptr::null_mut(), LEFT_UP, end, 0);
            if down.is_null() || up.is_null() {
                if !down.is_null() {
                    CFRelease(down.cast());
                }
                if !up.is_null() {
                    CFRelease(up.cast());
                }
                return Err(tr("Couldn't drag for this skill").to_owned());
            }
            CGEventPost(HID_TAP, down);
            for step in 1..=8 {
                let t = step as f64 / 8.0;
                let at = CGPoint {
                    x: start.x + (end.x - start.x) * t,
                    y: start.y + (end.y - start.y) * t,
                };
                CGWarpMouseCursorPosition(at);
                thread::sleep(MOUSE_GAP);
            }
            CGEventPost(HID_TAP, up);
            CFRelease(down.cast());
            CFRelease(up.cast());
        }
        Ok(())
    }

    fn clamp_wheel(value: f32) -> i32 {
        if !value.is_finite() {
            return 0;
        }
        value.clamp(i16::MIN as f32, i16::MAX as f32) as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Recorder {
        steps: Mutex<Vec<PlanStep>>,
        fail_at: Option<usize>,
    }

    impl Driver for Recorder {
        fn perform(&self, step: &PlanStep, _plan: &RunPlan) -> Result<(), String> {
            let n = {
                let mut steps = self.steps.lock().unwrap();
                steps.push(step.clone());
                steps.len()
            };
            if self.fail_at == Some(n) {
                Err("stopped".into())
            } else {
                Ok(())
            }
        }
    }

    fn last(rx: Receiver<Progress>) -> Progress {
        rx.iter().last().expect("a run always ends")
    }

    fn plan(count: usize) -> RunPlan {
        RunPlan {
            frame: PlanFrame {
                x: 0.0,
                y: 0.0,
                width: 1000.0,
                height: 800.0,
            },
            steps: (0..count)
                .map(|i| PlanStep::Click {
                    x: i as f32 / 100.0,
                    y: 0.5,
                })
                .collect(),
        }
    }

    #[test]
    fn every_step_runs_in_order() {
        let driver = Arc::new(Recorder::default());
        let rx = execute(
            "s",
            plan(3),
            driver.clone(),
            Arc::new(AtomicBool::new(false)),
        );
        let progress: Vec<_> = rx.iter().collect();
        assert_eq!(progress.first(), Some(&Progress::Step { n: 1, of: 3 }));
        assert_eq!(progress.last(), Some(&Progress::Done));
        assert_eq!(driver.steps.lock().unwrap().len(), 3);
    }

    #[test]
    fn a_stop_before_a_step_leaves_it_unperformed() {
        let driver = Arc::new(Recorder::default());
        let cancel = Arc::new(AtomicBool::new(false));
        let rx = execute("s", plan(2), driver.clone(), cancel.clone());
        assert!(matches!(rx.recv().unwrap(), Progress::Step { n: 1, .. }));
        cancel.store(true, Ordering::Release);
        assert_eq!(last(rx), Progress::Cancelled);
        assert_eq!(driver.steps.lock().unwrap().len(), 1);
    }

    #[test]
    fn a_failed_step_stops_the_run() {
        let driver = Arc::new(Recorder {
            fail_at: Some(2),
            ..Default::default()
        });
        let rx = execute(
            "s",
            plan(4),
            driver.clone(),
            Arc::new(AtomicBool::new(false)),
        );
        assert_eq!(last(rx), Progress::Failed("stopped".into()));
        assert_eq!(driver.steps.lock().unwrap().len(), 2);
    }

    #[test]
    fn empty_and_oversized_plans_refuse_before_running() {
        let driver = Arc::new(Recorder::default());
        assert_eq!(
            execute(
                "s",
                plan(0),
                driver.clone(),
                Arc::new(AtomicBool::new(false))
            )
            .recv()
            .unwrap(),
            Progress::Failed("This skill has no runnable steps".into())
        );
        assert_eq!(
            execute(
                "s",
                plan(MAX_STEPS + 1),
                driver.clone(),
                Arc::new(AtomicBool::new(false))
            )
            .recv()
            .unwrap(),
            Progress::Failed("This skill has too many steps to run safely".into())
        );
        assert!(driver.steps.lock().unwrap().is_empty());
    }

    #[test]
    fn a_plan_maps_normalized_points_to_its_frame() {
        let p = plan(1);
        assert_eq!(p.point(0.5, 0.25), (500.0, 200.0));
    }

    fn bundle(
        events: Vec<serde_json::Value>,
    ) -> (super::super::evidence::tests::Fixture, ScrawlBundle) {
        let (fixture, lesson) =
            super::super::evidence::tests::lesson("plan", &[], events, &[], &[]);
        let bundle = crate::capture_host::load_capture(&lesson.bundle).unwrap();
        (fixture, bundle)
    }

    #[test]
    fn only_structured_interactions_become_steps() {
        use serde_json::json;

        let (_fixture, bundle) = bundle(vec![
            json!({"kind": "app_change", "t": 1000, "app": "Mail", "windowTitle": "Private project"}),
            json!({"kind": "click", "t": 1100, "x": 0.25, "y": 0.5, "label": "Download", "role": "button"}),
            json!({"kind": "key", "t": 1200, "shortcut": "⌘ T"}),
            json!({"kind": "key", "t": 1500, "shortcut": "⌘T"}),
            json!({"kind": "scroll", "t": 1600, "x": 0.5, "y": 0.5, "dx": 0.0, "dy": -1.0}),
            json!({"kind": "drag", "t": 1700, "t1": 1800, "x0": 0.1, "y0": 0.2,
                   "x1": 0.4, "y1": 0.6, "label": "the invoice"}),
            json!({"kind": "drag", "t": 1900, "t1": 1950, "x0": 0.6, "y0": 0.6,
                   "x1": 0.62, "y1": 0.61}),
            json!({"kind": "point", "t": 2000, "x": 0.5, "y": 0.5, "label": "a pointed word"}),
            json!({"kind": "app_event", "t": 2100, "label": "said something private"}),
        ]);
        let plan = plan_from_bundle(&bundle).unwrap();
        assert_eq!(
            plan.steps,
            vec![
                PlanStep::Activate { app: "Mail".into() },
                PlanStep::Click { x: 0.25, y: 0.5 },
                PlanStep::Key {
                    shortcut: "⌘T".into()
                },
                PlanStep::Scroll {
                    x: 0.5,
                    y: 0.5,
                    dx: 0.0,
                    dy: -1.0
                },
                PlanStep::Drag {
                    x0: 0.1,
                    y0: 0.2,
                    x1: 0.4,
                    y1: 0.6
                },
            ]
        );
        assert!(
            !serde_json::to_string(&plan)
                .unwrap()
                .contains("Private project")
        );
    }

    #[test]
    fn a_plan_stops_at_the_safety_bound() {
        use serde_json::json;

        let events = (0..50)
            .map(|n| json!({"kind": "click", "t": n, "x": 0.5, "y": 0.5}))
            .collect();
        let (_fixture, bundle) = bundle(events);
        let plan = plan_from_bundle(&bundle).unwrap();
        assert_eq!(plan.steps.len(), MAX_STEPS);
        assert!(validate(&plan).is_ok());
    }
}
