//! Labs › Notice patterns' offer (macOS): a capsule at the top of the screen
//! that offers to learn a routine Apprentice noticed. Show me starts a
//! lesson, Stop asking stops offering that routine, Not now only closes, and
//! after [`TIMEOUT`] it closes by itself, doing nothing. It never takes
//! focus, takes the pointer only where it draws, and is left out of screen
//! recordings. It stays clear of the meeting recording capsule's corner.

use std::sync::Arc;
use std::time::{Duration, Instant};

use rune::{
    Align, Anim, Cursor, Cx, El, KeyPolicy, OverlayHandle, OverlayInput, OverlayOptions,
    OverlaySpec, ScreenEdge, col, el, row, text,
};
use rune_kit::{Button, ButtonVariant};

use crate::app_settings::MeetingIndicatorCorner;
use crate::desktop::i18n::tr;
use crate::rune_ui::controls::{Paint, PushSize, status_dot};

/// An unanswered offer closes by itself after this long, doing nothing.
pub(crate) const TIMEOUT: Duration = Duration::from_secs(30);
/// The overlay window: the capsule and the few points around it.
const WINDOW: (f32, f32) = (440.0, 128.0);
/// Below the menu bar, where the meeting capsules sit.
const MARGIN: f32 = 40.0;
/// From the screen's side edge.
const INSET: f32 = 12.0;

/// What the capsule answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Answer {
    NotNow,
    StopAsking,
    ShowMe,
}

/// The routine on offer.
struct Offer {
    id: String,
    /// "New PDF in Downloads".
    title: String,
    /// "You usually file these (3 days). Show me how?"
    line: String,
    /// When it closes by itself.
    until: Instant,
}

/// The overlay's state: the offer it shows, if any, and what its buttons
/// hand the answer to.
pub(crate) struct OfferState {
    offer: Option<Offer>,
    /// Show me: [`crate::apprentice::notice::watch::accept`].
    accept: fn(&str),
    /// Stop asking: [`crate::apprentice::notice::watch::ignore`].
    ignore: fn(&str),
    /// Hides the overlay's window; none offscreen.
    hide: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl OfferState {
    pub(crate) fn new(accept: fn(&str), ignore: fn(&str)) -> Self {
        Self {
            offer: None,
            accept,
            ignore,
            hide: None,
        }
    }

    /// Shows routine `id`'s offer in place of any other, for [`TIMEOUT`]
    /// from `now`.
    pub(crate) fn show(&mut self, id: String, title: String, line: String, now: Instant) {
        self.offer = Some(Offer {
            id,
            title,
            line,
            until: now + TIMEOUT,
        });
    }

    /// Closes the offer once its time is up, or at once when the gate is
    /// closed, doing nothing else; true when it closed now.
    pub(crate) fn tick(&mut self, now: Instant, open: bool) -> bool {
        let due = self
            .offer
            .as_ref()
            .is_some_and(|offer| !open || now >= offer.until);
        if due {
            self.close();
        }
        due
    }

    #[cfg(test)]
    fn visible(&self) -> bool {
        self.offer.is_some()
    }

    fn close(&mut self) {
        self.offer = None;
        if let Some(hide) = &self.hide {
            hide();
        }
    }

    /// The drawn offer's buttons answer only that offer, even when a press
    /// lands after a replacement. Accepting and ignoring take the notice
    /// store's lock and may sync to disk, so they run on a thread of their
    /// own, never the main thread's.
    fn answer(&mut self, id: &str, answer: Answer) {
        if self.offer.as_ref().is_none_or(|offer| offer.id != id) {
            return;
        }
        self.close();
        let act = match answer {
            Answer::NotNow => return,
            Answer::StopAsking => self.ignore,
            Answer::ShowMe => self.accept,
        };
        let id = id.to_owned();
        if let Err(error) = std::thread::Builder::new()
            .name("apprentice-offer".into())
            .spawn(move || act(&id))
        {
            tracing::warn!(%error, "could not answer the routine offer");
        }
    }
}

/// The capsule: the routine's title and line, and its three buttons.
pub(in crate::rune_ui) fn view(state: &OfferState, p: Paint) -> El {
    let Some(offer) = &state.offer else {
        return row().key("routine-offer-empty");
    };
    let c = &p.k.c;
    let button = |key: &str, label: &str, variant, answer| {
        let id = offer.id.clone();
        Button::new(label)
            .variant(variant)
            .build_push(p.k)
            .key(key)
            .cursor(Cursor::Pointer)
            .on_press(move |s: &mut OfferState| s.answer(&id, answer))
    };
    let lines = col()
        .key("lines")
        .grow(1.0)
        .min_w(0.0)
        .gap(2.0)
        .child(
            text(offer.title.clone())
                .key("title")
                .truncate()
                .font(p.k.ty.ui_medium)
                .color(p.text())
                .a11y_hidden(),
        )
        .child(
            text(offer.line.clone())
                .key("line")
                .wrap()
                .max_lines(2)
                .font(p.k.ty.small)
                .color(p.muted()),
        );
    // A status named by its title, announced when an offer shows; only the
    // drawn capsule takes the pointer, not the window's margin around it.
    let body = col()
        .key("routine-offer")
        .role("status")
        .label(&offer.title)
        .w_full()
        .gap(10.0)
        .pt(12.0)
        .pb(8.0)
        .pl(16.0)
        .pr(8.0)
        .radius(p.k.radius.md + 8.0)
        .fill(c.bg_base)
        .border(p.hair, c.border_base)
        .hit()
        .child(
            row()
                .key("heading")
                .w_full()
                .gap(12.0)
                .center_y()
                .child(status_dot(p.accent()))
                .child(lines),
        )
        .child(
            row()
                .key("actions")
                .w_full()
                .gap(8.0)
                .child(el().grow(1.0))
                .child(button(
                    "not-now",
                    tr("Not now"),
                    ButtonVariant::Ghost,
                    Answer::NotNow,
                ))
                .child(button(
                    "stop-asking",
                    tr("Stop asking"),
                    ButtonVariant::Neutral,
                    Answer::StopAsking,
                ))
                .child(button(
                    "show-me",
                    tr("Show me"),
                    ButtonVariant::Contrast,
                    Answer::ShowMe,
                )),
        );
    row().key("capsule").w_full().px(4.0).py(4.0).child(body)
}

/// Top right, unless the meeting recording capsule sits there.
fn align_for(corner: MeetingIndicatorCorner) -> Align {
    if corner == MeetingIndicatorCorner::TopRight {
        Align::Start
    } else {
        Align::End
    }
}

/// The offer capsule's overlay, fed from the pump on the main thread.
pub(crate) struct RoutineOffer {
    handle: OverlayHandle<OfferState>,
    /// When the shown offer closes by itself; none while nothing shows.
    until: Option<Instant>,
}

impl RoutineOffer {
    pub(crate) fn install<S: 'static>(app: &mut rune::App<S>) -> Self {
        use crate::apprentice::notice::watch;
        let spec = OverlaySpec {
            name: "Apprentice routine offer",
            size: WINDOW,
            edge: ScreenEdge::Top,
            margin: MARGIN,
            align: Align::End,
            inset: INSET,
            interactive: true,
            view: |state: &OfferState, cx: &mut Cx<OfferState>| {
                view(state, Paint::new(cx.system_dark(), cx.env().scale))
                    .hover_motion_within(Anim::INSTANT, Anim::INSTANT)
            },
        }
        .options(OverlayOptions {
            input: OverlayInput::Regions,
            keys: KeyPolicy::None,
            exclude_from_capture: true,
            ..Default::default()
        })
        .release_gpu_when_hidden();
        let handle = app.overlay(spec, OfferState::new(watch::accept, watch::ignore));
        let hide = handle.clone();
        handle.update(move |state, _| state.hide = Some(Arc::new(move || hide.hide())));
        Self {
            handle,
            until: None,
        }
    }

    /// Shows the watcher's offer in place of any other, while the gate is
    /// open.
    pub(crate) fn show(&mut self, id: String, title: String, line: String, now: Instant) {
        if !crate::labs::notice_patterns() {
            return;
        }
        let align = align_for(crate::app_settings::meeting_indicator_corner());
        self.handle.set_align(align, INSET);
        self.until = Some(now + TIMEOUT);
        let shown = self.handle.clone();
        self.handle.update(move |state, tx| {
            state.show(id, title, line, now);
            tx.instant();
            shown.show();
        });
    }

    /// Closes an unanswered offer after [`TIMEOUT`], and at once when the
    /// gate closes.
    pub(crate) fn tick(&mut self, now: Instant) {
        let Some(until) = self.until else {
            return;
        };
        let open = crate::labs::notice_patterns();
        if open && now < until {
            return;
        }
        self.until = None;
        self.handle.update_if(move |state, _| state.tick(now, open));
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use rune::Runtime;
    use rune::text::TextSystem;
    use serde_json::Value;

    use super::*;

    /// Every answer handed on, by routine id. Each test uses its own ids.
    static CALLS: Mutex<Vec<(&str, String)>> = Mutex::new(Vec::new());

    fn accept(id: &str) {
        CALLS.lock().unwrap().push(("accept", id.to_owned()));
    }

    fn ignore(id: &str) {
        CALLS.lock().unwrap().push(("ignore", id.to_owned()));
    }

    fn calls(id: &str) -> Vec<&'static str> {
        CALLS
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, called)| called == id)
            .map(|(what, _)| *what)
            .collect()
    }

    /// The answers for `id` once `count` arrived from their threads.
    fn wait_for(id: &str, count: usize) -> Vec<&'static str> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let seen = calls(id);
            if seen.len() >= count || Instant::now() > deadline {
                return seen;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn offered(id: &str, now: Instant) -> OfferState {
        let mut state = OfferState::new(accept, ignore);
        state.show(
            id.into(),
            "New PDF in Downloads".into(),
            "You usually file these (3 days). Show me how?".into(),
            now,
        );
        state
    }

    fn overlay(state: &OfferState, cx: &mut Cx<OfferState>) -> El {
        view(state, Paint::new(cx.system_dark(), cx.env().scale))
    }

    fn runtime(state: OfferState) -> Runtime<OfferState> {
        themed(state, true)
    }

    fn themed(state: OfferState, dark: bool) -> Runtime<OfferState> {
        let text = TextSystem::new(rune_theme_opencode::FONTS, rune_theme_opencode::FAMILIES);
        let env = rune::Env {
            system_dark: dark,
            scale: 2.0,
        };
        let mut rt = crate::rune_ui::offscreen::runtime(state, overlay, WINDOW, env, text);
        rt.frame(0.0);
        rt
    }

    fn tap(rt: &mut Runtime<OfferState>, key: &str) {
        let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(1.0 / 60.0);
    }

    fn named(tree: &Value, out: &mut Vec<(String, String)>) {
        let role = tree["role"].as_str().unwrap_or_default().to_owned();
        let name = tree["name"].as_str().unwrap_or_default().trim().to_owned();
        out.push((role, name));
        for child in tree["children"].as_array().into_iter().flatten() {
            named(child, out);
        }
    }

    #[test]
    fn the_offer_names_every_button() {
        let rt = runtime(offered("names", Instant::now()));
        let mut nodes = Vec::new();
        named(&rt.accessibility_json(), &mut nodes);
        for label in ["Show me", "Not now", "Stop asking"] {
            assert!(
                nodes
                    .iter()
                    .any(|(role, name)| role == "button" && name == label),
                "no button named {label:?} in {nodes:?}"
            );
        }
        let texts: Vec<_> = rt.elements().into_iter().filter_map(|e| e.text).collect();
        assert!(
            texts.contains(&"New PDF in Downloads".to_owned()),
            "{texts:?}"
        );
        assert!(
            texts.contains(&"You usually file these (3 days). Show me how?".to_owned()),
            "{texts:?}"
        );
    }

    #[test]
    fn the_offer_closes_after_thirty_seconds_without_acting() {
        let start = Instant::now();
        let mut state = offered("timeout", start);
        assert!(!state.tick(start + Duration::from_secs(29), true));
        assert!(state.visible());
        assert!(state.tick(start + Duration::from_secs(31), true));
        assert!(!state.visible());
        assert!(calls("timeout").is_empty());
        // Nothing draws once it closed.
        let rt = runtime(state);
        assert!(rt.find("show-me").is_none());
    }

    #[test]
    fn the_offer_hides_when_the_gate_closes() {
        let start = Instant::now();
        let mut state = offered("gate", start);
        assert!(state.tick(start + Duration::from_secs(1), false));
        assert!(!state.visible());
        assert!(calls("gate").is_empty());
    }

    #[test]
    fn a_new_offer_replaces_the_old_one() {
        let start = Instant::now();
        let mut state = offered("old", start);
        let later = start + Duration::from_secs(20);
        state.show(
            "new".into(),
            "New PNG in Desktop".into(),
            "You usually file these (4 days). Show me how?".into(),
            later,
        );
        // The replacement keeps its own 30 s.
        assert!(!state.tick(start + Duration::from_secs(31), true));
        assert!(state.visible());
        let mut rt = runtime(state);
        let texts: Vec<_> = rt.elements().into_iter().filter_map(|e| e.text).collect();
        assert!(
            texts.contains(&"New PNG in Desktop".to_owned()),
            "{texts:?}"
        );
        assert!(
            !texts.contains(&"New PDF in Downloads".to_owned()),
            "{texts:?}"
        );
        tap(&mut rt, "show-me");
        assert_eq!(wait_for("new", 1), ["accept"]);
        assert!(calls("old").is_empty());
        assert!(!rt.state.visible());
    }

    #[test]
    fn each_button_hands_on_its_answer_and_closes() {
        let mut rt = runtime(offered("show", Instant::now()));
        tap(&mut rt, "show-me");
        assert!(!rt.state.visible());
        assert_eq!(wait_for("show", 1), ["accept"]);

        let mut rt = runtime(offered("stop", Instant::now()));
        tap(&mut rt, "stop-asking");
        assert!(!rt.state.visible());
        assert_eq!(wait_for("stop", 1), ["ignore"]);

        let mut rt = runtime(offered("later", Instant::now()));
        tap(&mut rt, "not-now");
        assert!(!rt.state.visible());
        std::thread::sleep(Duration::from_millis(50));
        assert!(calls("later").is_empty());
    }

    #[test]
    fn a_press_meant_for_a_replaced_offer_does_nothing() {
        let mut state = offered("first", Instant::now());
        state.show(
            "second".into(),
            "New PNG in Desktop".into(),
            "You usually file these (4 days). Show me how?".into(),
            Instant::now(),
        );
        state.answer("first", Answer::ShowMe);
        assert!(state.visible());
        std::thread::sleep(Duration::from_millis(50));
        assert!(calls("first").is_empty());
        assert!(calls("second").is_empty());
    }

    /// No `--snap` scene draws an overlay, so this renders the capsule to
    /// PNGs in `HEX_CAPSULE_SNAPS` for review, as the meeting capsules do;
    /// without it, or a GPU, it skips.
    #[test]
    fn capsule_snapshots() {
        use rune::render::Gpu;
        let Some(folder) = std::env::var_os("HEX_CAPSULE_SNAPS") else {
            return;
        };
        let Ok(gpu) = Gpu::try_new(Gpu::instance(), None) else {
            eprintln!("skipping capsule snapshots: no GPU adapter");
            return;
        };
        let folder = std::path::PathBuf::from(folder);
        std::fs::create_dir_all(&folder).unwrap();
        let (w, h) = ((WINDOW.0 * 2.0) as u32, (WINDOW.1 * 2.0) as u32);
        for dark in [false, true] {
            let mut rt = themed(offered("snap", Instant::now()), dark);
            let pixels = crate::rune_ui::offscreen::draw(&gpu, &mut rt, w, h);
            let theme = if dark { "dark" } else { "light" };
            let path = folder.join(format!("capsule-routine-offer-{theme}.png"));
            rune::render::write_png(&path, w, h, &pixels).unwrap();
        }
    }

    #[test]
    fn the_offer_avoids_the_meeting_capsules_corner() {
        assert_eq!(align_for(MeetingIndicatorCorner::TopRight), Align::Start);
        for corner in [
            MeetingIndicatorCorner::TopLeft,
            MeetingIndicatorCorner::BottomRight,
            MeetingIndicatorCorner::BottomLeft,
        ] {
            assert_eq!(align_for(corner), Align::End);
        }
    }
}
