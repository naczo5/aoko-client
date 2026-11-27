//! The command-search overlay view and its rows.
//!
//! **Mounted** (section 12 decision 7): `a8479a7` wired the shell's `render_search`, which
//! installs the rail button, Ctrl+K and type-to-open, and paints this view over the tab
//! body. The state half is [`crate::views::command_search`]; this module is the painting
//! half, plus [`card_pulse_ring`], the accent ring a jump leaves on its destination card.
//!
//! Split by interface, as `UI_INVENTORY.md` §7 requires: **this module draws**, and
//! [`crate::views::command_search`] owns open/close, Ctrl+K, the query, ranking,
//! selection, the row actions and the pulse's clock. The overlay itself holds no state:
//! it is a function of `&CommandSearchState`, the query field entity the shell owns, and a
//! click callback.
//!
//! Geometry comes from `views::command_search` (`card_width`, `card_top`,
//! `results_viewport_height`), which is the C# `LayoutCard` port, so the card's clamp
//! and its top ratio have one home rather than two.
//!
//! Three things are visible here and should not be "improved" back:
//!
//! - **Opaque card over a dark scrim** (UI decision 2). The C# card was `Panel` at 89 %
//!   alpha over a blurred tab control; GPUI cannot blur behind an element, so the card
//!   is the panel colour at full alpha and the blur is gone. `Theme::search_glass`
//!   keeps the C# value for the record; it is not what the card paints.
//! - **Two-stop scrim only.** The C# scrim is a two-stop vertical gradient
//!   (`CommandSearchOverlay.xaml:117-124`), which `Background::linear_gradient`
//!   reproduces exactly. The 760 px radial "moon haze" (`:132-140`) is dropped rather
//!   than faked with concentric divs, which bands.
//! - **Row washes are opaque blends.** The C# layered `TextBrush` at 8 % / 14 % over the
//!   card; over an opaque card that is exactly `blend(card, text, 0.08 / 0.14)`, so no
//!   extra element is needed and a light palette still darkens the row instead of
//!   washing it white. Both layers are painted: a selected row under the pointer is the two
//!   alphas stacked, not the hover wash replacing the selected one.
//!
//! # The scrim is a layer, and it takes the press
//!
//! The C# overlay was one `Grid` over the whole window, scrim behind and card in front
//! (`CommandSearchOverlay.xaml:114`), so the card's top ratio and the scrim's own box were both
//! measured from the window. [`overlay`] therefore returns an **absolute** layer: as a flow child
//! of the shell's column it was laid out under the title bar, which painted the card one title bar
//! below `card_top` and collapsed the tab body behind it.
//!
//! The dimmed layer is also the close target: `Scrim_MouseLeftButtonDown`
//! (`CommandSearchOverlay.xaml.cs:488-492`) set `Handled` and called `Close`. A backdrop element
//! carries that here, and it `occlude`s: without that the hit test still reaches the rail and the
//! module switches painted under the scrim, so a click on the dimmed area pressed one of them.
//!
//! Copy is verbatim from the XAML: `up/down navigate`, `enter jump`, `f2 toggle` and
//! `UNAVAILABLE`. The keybind chip keeps `Consolas`'s role by using the bundled
//! monospace family through [`crate::assets::MONO_FAMILY`]. The row caption is
//! [`Row::hint_text`], which is `BuildHint` (`CommandSearchOverlay.xaml.cs:642-652`) — not
//! the card's element name.
//!
//! # The query row is a field, not a label
//!
//! `QueryBox` (`CommandSearchOverlay.xaml:215-227`) is a real `TextBox` in the C#: the
//! overlay opens with the caret in it and typing narrows the list. An earlier revision
//! of this file painted [`crate::components::field::field`], which cannot receive focus or
//! keystrokes, so the mount would have put an inert box on screen. [`overlay`] now paints
//! the owner's [`TextInput`] entity in its place, chosen with
//! [`FieldVariant::Query`].
//!
//! An entity cannot be created during a render, so the field arrives as a parameter: see
//! [`OverlayQuery`] for the two shapes and what the shell has to do to supply the live one.
//!
//! # Motion
//!
//! Decision 3 makes one toggle govern every animated site, and this view now reads it for
//! its own entrance the way the managed overlay did. `AnimateIn`
//! (`CommandSearchOverlay.xaml.cs:141-167`) ran three animations: the scrim from 0 to 1
//! over 180 ms on a `QuadraticEase EaseOut` (`:153`), the card's own opacity over 140 ms on
//! a `QuarticEase EaseOut` (`:158`), and `CardSlide.Y` from 8 to 0 over the same 140 ms
//! (`:163`). All three are here: [`overlay`] animates the scrim root and the card's absolute
//! box, which is the same pair of objects the C# animated.
//!
//! Two of the managed four are deliberately still missing, and both need the shell:
//!
//! - **Close** (`:104-130`): 100 ms back out to 0 opacity and `CardSlide.Y = 6`. An exited
//!   element cannot animate, so this needs the mount to hold the overlay for 100 ms after
//!   `search.is_open()` goes false and to notify when it finishes. Until then the close is
//!   a cut, which is what a user with animations off sees anyway.
//! - **Re-card** (`:221`, `OVERLAY_RECARD`): re-running the card's entrance when the result
//!   count changes. The card is one element with one id, so re-running means keying that id
//!   on the row count — a decision about churn on every keystroke, not a paint.
//!
//! [`card_pulse_ring`] **is** animated, and it reads the gate: it returns `None` under
//! `Motion::STILL`, and `begin_pulse` refuses to start a pulse at all in that case, so the
//! ring cannot outlive the setting. It advances through `with_animation`, which is what
//! makes GPUI redraw while the ring is up; the shadow clock in the state is not needed for
//! that and no `cx.notify` loop belongs in the shell for it.
//!
//! `with_animation` is a second gate, and the two are kept deliberately. It consults
//! [`App::reduce_motion`] itself: with that set, the animator runs once at the oneshot's
//! **end** state and no frames are scheduled (`gpui/src/elements/animation.rs:326-333` at
//! the pin). [`App::reduce_motion`] and [`Motion`] are not the same flag and never were —
//! `Motion` is the managed `ClientAreaAnimation` the Settings card owns, while
//! `reduce_motion` is GPUI's platform-level setting, which nothing in this tree sets
//! except the capture harness (`capture.rs:394`, for deterministic frames). So the double
//! gate is not redundant, and it is not interchangeable: the [`Motion`] check upstream
//! returns `None`, so no ring element exists at all — which is what a test can assert and
//! what the managed early return does — whereas `reduce_motion` alone would still mount the
//! element and paint it at opacity 0. Under capture the ring is therefore present and
//! invisible, which is why a screenshot settles the timeline and a window test does not.
//!
//! `the_overlay_entrance_is_gated_and_starts_on_its_slide` is what catches an entrance that
//! forgets the gate: the entrance starts `CardSlide.Y` (8 px) below the settled card, so an
//! ungated slide moves even the `Motion::STILL` frame and the test's comparison fails.
//!
//! [`row_element`] takes no `Motion` at all: rows never animated in the managed overlay,
//! and a parameter nothing reads is worse than a signature the mount has to extend.

use gpui::prelude::*;
use gpui::{
    div, px, Animation, AnimationExt, AnyElement, App, ClickEvent, ElementId, Entity, Hsla,
    IntoElement, Pixels, ScrollHandle, SharedString, Size, Window,
};

use crate::assets::MONO_FAMILY;
use crate::components::field::FieldVariant;
use crate::components::text_input::TextInput;
use crate::components::{self, field, glyph, highlight_text, scroll, switch::switch_static};
use crate::theme::motion::{self, Motion};
use crate::theme::{fixed, metrics, Color, Theme};
use crate::views::command_search::{self, CommandSearchState, Row};
use crate::views::copy;

/// Header (accent line, query row, divider, footer) height taken off the results area.
const CHROME: f32 = 108.0;

/// The pulse adorner's halo: 3.2 px at 28 % (`MainWindow.xaml.cs:1846-1848`).
const PULSE_HALO_WIDTH: f32 = 3.2;
/// The halo is the accent at this alpha, before the pulse's own opacity.
const PULSE_HALO_ALPHA: f32 = 0.28;
/// The ring itself: 1.6 px at the pulse's opacity (`:1849-1853`).
const PULSE_RING_WIDTH: f32 = 1.6;
/// How far the halo sits outside the card's box (`:1846`: `Rect(-1.5, -1.5, width + 3, …)`).
const PULSE_HALO_INSET: f32 = 1.5;
/// The ring's inset inside the card's box (`:1852`: `Rect(0.5, 0.5, width - 1, …)`).
const PULSE_RING_INSET: f32 = 0.5;
/// The selected row's accent rule: 2 px wide (`CommandSearchOverlay.xaml:304-316`).
const ROW_RULE_WIDTH: f32 = 2.0;
/// The rule's height, centred on the row's box.
const ROW_RULE_HEIGHT: f32 = 24.0;

/// The query row's source: the live field entity, and the viewport the card is sized from.
///
/// The two shapes are deliberate. `From<Size<Pixels>>` is the **un-wired** path: it paints
/// the static query line, which is what the mounted overlay does until the shell hands its
/// field over. `From<(&Entity<TextInput>, Size<Pixels>)>` is the live one, and switching is
/// one expression at the call site:
///
/// ```ignore
/// crate::overlay::overlay(
///     theme, &self.search, motion, (&self.search_query, window.viewport_size()),
///     ResultsScroll::new(&self.search_scroll, None), on_row, on_close,
/// )
/// ```
///
/// The entity itself has to be created and focused where a `Context` and a `&mut Window`
/// exist (the shell's `ensure_form_fields` / open path), because an entity cannot be born
/// inside a render.
pub struct OverlayQuery<'a> {
    entity: Option<&'a Entity<TextInput>>,
    viewport: Size<Pixels>,
}

impl<'a> From<Size<Pixels>> for OverlayQuery<'a> {
    fn from(viewport: Size<Pixels>) -> Self {
        Self {
            entity: None,
            viewport,
        }
    }
}

impl<'a> From<(&'a Entity<TextInput>, Size<Pixels>)> for OverlayQuery<'a> {
    fn from((entity, viewport): (&'a Entity<TextInput>, Size<Pixels>)) -> Self {
        Self {
            entity: Some(entity),
            viewport,
        }
    }
}

/// Where the results list is scrolled: the handle the card tracks, and the row to show.
///
/// The handle outlives the frame — it holds the offset and every row's bounds between paints —
/// so the shell owns it and lends it here, and the overlay never invents one per frame. `to` is
/// the row a query-field key just moved the selection to: the state records it as
/// `CommandSearchState::scroll_request` because only the paint can act on it, and
/// `ScrollHandle::scroll_to_item` (`crates/gpui/src/elements/div.rs:4419`) is how a paint moves a
/// row into view — called before the list is built, so the prepaint that follows acts on it.
pub struct ResultsScroll<'a> {
    handle: &'a ScrollHandle,
    to: Option<usize>,
}

impl<'a> ResultsScroll<'a> {
    /// The handle, and the row a key moved to.
    #[must_use]
    pub fn new(handle: &'a ScrollHandle, to: Option<usize>) -> Self {
        Self { handle, to }
    }
}

/// Seeds the query field, puts the caret at the end of the seed and focuses it.
///
/// This is the **open path**, not a render: it needs a `&mut Window` and a `&mut App`, and
/// the field entity must not be leased, so it belongs in a key handler or a click callback.
/// It is `CommandSearchOverlay.Open` (`CommandSearchOverlay.xaml.cs:69-88`) with the three
/// steps that matter kept together: the seed (`:76`), the caret at its end (`:77`), the
/// focus (`:87`). The seed itself is the character that opened the overlay
/// (`MainWindow.xaml.cs:1637`) or empty for Ctrl+K (`:1647`).
///
/// # Panics
/// Panics if `field` is already being updated, which is what calling this from inside that
/// entity's own `render` would do.
pub fn focus_query(field: &Entity<TextInput>, seed: &str, window: &mut Window, cx: &mut App) {
    field.update(cx, |input, cx| input.set_text(seed, cx));
    let handle = field.read(cx).focus_handle().clone();
    window.focus(&handle, cx);
}

/// The whole overlay: scrim, card, query row, results and legend.
///
/// `query` carries the viewport (`Window::viewport_size()`) and, when the shell has one,
/// its field entity; see [`OverlayQuery`]. Card width, card top and the results cap all
/// come from [`crate::views::command_search`], the C# `LayoutCard` port, so the clamp
/// has one home.
///
/// `results` is the list's scroll position; see [`ResultsScroll`]. It is a parameter rather
/// than something the overlay builds because the handle has to outlive the frame, and it is
/// how a key that moved the selection gets the new row onto the screen.
///
/// `on_row` receives the clicked row index; the caller owns what a click means (select,
/// jump, or toggle), and whether the overlay closes.
///
/// `on_close` is the one close path the pointer takes: the Esc chip's click (`CloseChip_Click`,
/// `CommandSearchOverlay.xaml.cs:245-250`) and a click on the dimmed layer
/// (`Scrim_MouseLeftButtonDown`, `:488-492`) both called the `Close` the Escape key did, so the
/// caller supplies it once rather than the overlay inventing a second one.
///
/// `motion` is the animation gate for the entrance the module docs describe: `Motion::STILL`
/// paints the settled scrim and the settled card in the first frame, which is the managed
/// `!ClientAreaAnimation` early return (`CommandSearchOverlay.xaml.cs:143-147`), and only an
/// enabled gate mounts the two animations. [`card_pulse_ring`] takes its own copy so the
/// ring cannot outlive the setting independently of the entrance.
pub fn overlay<'a>(
    theme: &Theme,
    state: &CommandSearchState,
    motion: Motion,
    query: impl Into<OverlayQuery<'a>>,
    results: ResultsScroll<'_>,
    on_row: impl Fn(usize, &ClickEvent, &mut Window, &mut App) + 'static,
    on_close: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let query = query.into();
    let viewport = query.viewport;
    let viewport_width = f32::from(viewport.width);
    let viewport_height = f32::from(viewport.height);
    let card_width = command_search::card_width(viewport_width);
    let card_top = command_search::card_top(viewport_height);
    let results_height = command_search::results_viewport_height(viewport_height, CHROME);

    // A key moved the selection: ask for that row before the list is built, so the prepaint that
    // follows reads the request.
    if let Some(index) = results.to {
        results.handle.scroll_to_item(index);
    }

    let on_row = std::rc::Rc::new(on_row);
    // One close path with two triggers: the esc chip's press and a press on the dimmed layer
    // (`CloseChip_Click`, `CommandSearchOverlay.xaml.cs:245-250`, and `Scrim_MouseLeftButtonDown`,
    // `:488-492`). `Rc` because both are separate `'static` closures and neither can borrow the
    // other's copy.
    let on_close = std::rc::Rc::new(on_close);
    let selected = state.selected_index();

    let mut results_list = scroll("command-search-results")
        .max_h(px(results_height))
        .track_scroll(results.handle);
    for (index, row) in state.rows().iter().enumerate() {
        let callback = on_row.clone();
        results_list = results_list.child(row_element(
            theme,
            row,
            index,
            selected == Some(index),
            move |event, window, cx| callback(index, event, window, cx),
        ));
    }

    if state.rows().is_empty() {
        results_list = results_list.child(
            div()
                .px(px(22.0))
                .py(px(20.0))
                .text_size(px(12.0))
                .text_color(theme.dim_text)
                .child(SharedString::from(state.empty_message())),
        );
    }

    let card = div()
        .flex()
        .flex_col()
        .w(px(card_width))
        // The one stable handle on the card's box: the window test compares it under both
        // motion settings.
        .debug_selector(|| String::from("command-search-card"))
        .rounded(px(metrics::radius::OVERLAY_CARD))
        .border_1()
        .border_color(fixed::GLASS_HAIRLINE)
        // Decision 2: opaque card. `search_glass()` exists for the record only.
        .bg(theme.panel)
        .child(accent_line(theme))
        .child(query_row(theme, state, query.entity, {
            let close = std::rc::Rc::clone(&on_close);
            move |event, window, cx| (*close)(event, window, cx)
        }))
        .child(
            div()
                .h(px(metrics::HAIRLINE))
                .w_full()
                .bg(theme.slider_background),
        )
        .child(results_list)
        .child(legend(theme, state));

    // The card's own box, so the entrance can move it without restyling the card or
    // displacing its `debug_selector`.
    //
    // `occlude` is what keeps a press on the card's own blank space from reaching the scrim below
    // it: the card is an opaque surface in managed, so its edge is neither a close nor a press on
    // whatever the tab body paints there.
    let card_box = div()
        .absolute()
        .top(px(card_top))
        .left(px((viewport_width - card_width) / 2.0))
        .occlude()
        .child(card);

    // Scrim: the C# two-stop vertical gradient, darker at the bottom. It is a **layer** over the
    // window, not a row in the shell's column: `card_top` and `card_width` are measured from the
    // window, and a flow child is laid out under the title bar instead — which painted the card a
    // title bar below its own box and collapsed the tab body behind it.
    let scrim_bg = gpui::linear_gradient(
        180.0,
        gpui::linear_color_stop(Hsla::from(fixed::SCRIM_TOP), 0.0),
        gpui::linear_color_stop(Hsla::from(fixed::SCRIM_BOTTOM), 1.0),
    );
    let scrim = div()
        .absolute()
        .top(px(0.0))
        .left(px(0.0))
        .size_full()
        .bg(scrim_bg);

    // The dimmed layer's pointer behaviour: `Scrim_MouseLeftButtonDown` set `Handled` and
    // closed, so a press that misses the card closes the overlay and stops here. `occlude` is the
    // "stops here" half — without it the hit test still reaches the rail and the module switches
    // painted under the scrim, so a click on the dimmed area pressed one of them.
    let backdrop = div()
        .id("command-search-scrim")
        .absolute()
        .top(px(0.0))
        .left(px(0.0))
        .size_full()
        .occlude()
        .on_click({
            let close = std::rc::Rc::clone(&on_close);
            move |event, window, cx| (*close)(event, window, cx)
        });

    if !motion.enabled() {
        return scrim.child(backdrop).child(card_box).into_any_element();
    }

    // `AnimateIn` (`CommandSearchOverlay.xaml.cs:141-167`): the scrim over 180 ms on a
    // quadratic ease-out, the card and its slide over 140 ms on a quartic one.
    let scrim_in = Animation::new(motion.duration(motion::OVERLAY_SCRIM))
        .with_easing(motion::quadratic_ease_out);
    let card_in = Animation::new(motion.duration(motion::OVERLAY_CARD_IN))
        .with_easing(motion::quartic_ease_out);

    let card_in = card_box.with_animation(
        // Both ids are constants rather than per-open names: a fresh id would restart the
        // entrance on every frame the shell rebuilds, which is most keystrokes.
        ElementId::Name("command-search-card-in".into()),
        card_in,
        move |element, t| {
            element
                .top(px(card_top + motion::OVERLAY_CARD_SLIDE * (1.0 - t)))
                .opacity(t)
        },
    );

    scrim
        .with_animation(
            ElementId::Name("command-search-scrim-in".into()),
            scrim_in,
            |element, t| element.opacity(t),
        )
        .child(backdrop)
        .child(card_in)
        .into_any_element()
}

/// The 2 px warm line at the top of the card: a 1 px specular highlight over a 1 px
/// accent line at 0.8 opacity (`CommandSearchOverlay.xaml:179-190`).
fn accent_line(theme: &Theme) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .child(div().h(px(1.0)).w_full().bg(fixed::GLASS_SPECULAR))
        .child(div().h(px(1.0)).w_full().bg(theme.accent).opacity(0.8))
        .into_any_element()
}

/// The query row: the owner's field entity when one was handed over, otherwise the
/// static line that cannot take focus.
///
/// The static branch exists only so a caller with no entity still paints the row (a
/// capture, a test that is not about typing); the mounted overlay passes the entity, and
/// `command-search-query-field` is the handle a window test uses to tell which branch
/// painted.
fn query_row(
    theme: &Theme,
    state: &CommandSearchState,
    input: Option<&Entity<TextInput>>,
    on_close: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let placeholder = if state.placeholder_visible() {
        copy::SEARCH_QUERY_NAME
    } else {
        ""
    };

    // The hint is managed's `AutomationProperties.HelpText` (`:228`) shown on hover instead. The
    // field itself is an entity the shell creates, so the tooltip goes on the wrapper that holds
    // it — hover reaches a parent as well as the child under the cursor.
    let hint = || components::tooltip(theme, copy::SEARCH_QUERY_HINT);

    let field_area = match input {
        Some(input) => div()
            .id("command-search-query-hint")
            .flex_1()
            .debug_selector(|| String::from("command-search-query-field"))
            .tooltip(hint())
            .child(input.clone())
            .into_any_element(),
        None => div()
            .id("command-search-query-hint")
            .flex_1()
            .tooltip(hint())
            .child(field(
                "command-search-query",
                theme,
                FieldVariant::Query,
                state.query(),
                placeholder,
                true,
                |_, _, _| {},
            ))
            .into_any_element(),
    };

    div()
        .flex()
        .flex_row()
        .items_center()
        .px(px(16.0))
        .py(px(14.0))
        .child(field_area)
        .child(
            // `EscChipStyle`: 4 px radius, slider-background fill, slider-foreground edge
            // that turns accent on hover, 10 px monospace cap.
            div()
                .id("command-search-close-chip")
                .debug_selector(|| String::from("command-search-close-chip"))
                .ml(px(10.0))
                .rounded(px(metrics::radius::CHIP))
                .border_1()
                .border_color(theme.slider_foreground)
                .bg(theme.slider_background)
                .px(px(7.0))
                .py(px(2.0))
                .text_size(px(metrics::text::ESC))
                .font_family(MONO_FAMILY)
                .text_color(theme.search_subtle_text())
                .cursor_pointer()
                // `CloseChip_Click` (`CommandSearchOverlay.xaml.cs:245-250`): a `Button` whose
                // handler called the same `Close` the Escape key did. The callback is the
                // caller's, so both routes end in one close path rather than two.
                .on_click(move |event, window, cx| on_close(event, window, cx))
                .hover(move |style| style.border_color(theme.accent))
                .tooltip(components::tooltip(theme, copy::SEARCH_CLOSE_TOOLTIP))
                .child(glyph::ESC),
        )
        .into_any_element()
}

/// The accent ring a jump leaves on its destination card, or `None` when this card is not
/// the one the live pulse is ringing.
///
/// `card_id` is the card's XAML `x:Name` — the same string as
/// `SearchEntry::card_name`, which [`command_search::pulse_card`] resolves from the
/// pulse's catalogue id. The ring is **outside the overlay**: managed attached an
/// `Adorner` to the card's border in the tab body, and the overlay had already closed
/// when it ran (`MainWindow.xaml.cs:1694-1720` closes, `:1780-1862` draws). So the caller
/// wraps the card in a `relative()` box and adds this as the **last** child, which is why
/// this lives here rather than in a card builder: the ring is positioned against the
/// card's box and must not take part in its layout.
///
/// Geometry is the adorner's: a 3.2 px halo at 28 % just outside the card
/// (`:1846-1848`) and a 1.6 px ring on its edge (`:1849-1853`), both at the pulse's
/// opacity and both rounded to the card radius plus two.
///
/// Frames: `with_animation` is what makes GPUI redraw while the ring is up, so no
/// `cx.notify` loop belongs in the shell for it. The opacity still comes from
/// [`command_search::CardPulse::opacity_at`], the one curve shared with the keyframes, and
/// the gate is read first — `Motion::STILL` paints no ring at all, which is the managed
/// `!ClientAreaAnimation` early return (`:1786-1788`). `with_animation` gates on
/// [`App::reduce_motion`] as well; that is a different flag from [`Motion`] and the two are
/// kept on purpose — see the module's motion section for why one is not the other.
pub fn card_pulse_ring(
    theme: &Theme,
    state: &CommandSearchState,
    motion: Motion,
    card_id: &str,
) -> Option<AnyElement> {
    if !motion.enabled() {
        return None;
    }
    if !command_search::pulse_card(state)?.eq_ignore_ascii_case(card_id) {
        return None;
    }
    let pulse = state.pulse()?.clone();
    let total_ms = i64::try_from(motion::PULSE_TOTAL.as_millis()).unwrap_or(i64::MAX);
    let started = pulse.started_ms;
    let radius = metrics::radius::CARD + 2.0;

    // One animation per outline: `with_animation` wraps its element, so each outline takes
    // its own id and its own frame callback instead of sharing one opacity.
    let fade = |opacity_scale: f32, id: &'static str| {
        let pulse = pulse.clone();
        let animation = Animation::new(motion.duration(motion::PULSE_TOTAL));
        move |element: gpui::Div| {
            element.with_animation(id, animation.clone(), move |element, t| {
                let now = started + (t * total_ms as f32) as i64;
                element.opacity(pulse.opacity_at(now) * opacity_scale)
            })
        }
    };

    let halo = div()
        .absolute()
        .top(px(-PULSE_HALO_INSET))
        .left(px(-PULSE_HALO_INSET))
        .right(px(-PULSE_HALO_INSET))
        .bottom(px(-PULSE_HALO_INSET))
        .rounded(px(radius + 1.0))
        .border(px(PULSE_HALO_WIDTH))
        .border_color(theme.accent);
    let halo = fade(PULSE_HALO_ALPHA, "card-pulse-halo")(halo);

    let ring = div()
        .absolute()
        .top(px(PULSE_RING_INSET))
        .left(px(PULSE_RING_INSET))
        .right(px(PULSE_RING_INSET))
        .bottom(px(PULSE_RING_INSET))
        .rounded(px(radius))
        .border(px(PULSE_RING_WIDTH))
        .border_color(theme.accent);
    let ring = fade(1.0, "card-pulse-ring")(ring);

    Some(
        div()
            // The handle a window test reads: the ring is a paint this harness can only
            // observe as geometry, so its box is the evidence it was drawn.
            .debug_selector(|| String::from("card-pulse-ring"))
            .absolute()
            .top(px(0.0))
            .left(px(0.0))
            .w_full()
            .h_full()
            .child(halo)
            .child(ring)
            .into_any_element(),
    )
}

/// The fills a result row can paint, all of them the card colour blended towards the palette's
/// text so a light palette darkens the row instead of washing it white.
struct RowWashes {
    /// The row at rest: the card itself.
    card: Color,
    /// A row under the pointer, unselected (`CommandSearchOverlay.xaml:281-289`).
    hover: Color,
    /// The selected row (`:291-300`).
    selected: Color,
    /// The selected row under the pointer. The managed row stacked the hover layer *over* the
    /// selected one, so the fill is both alphas composited; painting the hover wash alone shows
    /// the weaker of the two and the row reads as deselected under the cursor.
    selected_hover: Color,
}

impl RowWashes {
    /// The four fills for this palette.
    fn of(theme: &Theme) -> Self {
        let card = theme.panel;
        let hover_wash = metrics::overlay::ROW_HOVER_WASH;
        let selected_wash = metrics::overlay::ROW_SELECTED_WASH;
        let hover = card.blend(theme.text, hover_wash);
        let selected = card.blend(theme.text, selected_wash);
        // Two layers of one colour over an opaque card are one blend at the combined alpha.
        let stacked = 1.0 - (1.0 - selected_wash) * (1.0 - hover_wash);

        Self {
            card,
            hover,
            selected,
            selected_hover: card.blend(theme.text, stacked),
        }
    }

    /// The fill the row paints at rest, and the fill it paints under the pointer.
    fn fills(&self, selected: bool) -> (Color, Color) {
        if selected {
            (self.selected, self.selected_hover)
        } else {
            (self.card, self.hover)
        }
    }
}

/// One result row: 48 px, radius 7, inset 10×1, a hover wash and a stronger selected
/// wash, and an accent rule on the selected row.
///
/// No `Motion`: the managed overlay animated the card and the scrim, never a row, so this
/// painter is static by construction rather than by a gate that always says no.
pub fn row_element(
    theme: &Theme,
    row: &Row,
    index: usize,
    selected: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let washes = RowWashes::of(theme);
    let (rest_fill, hover_fill) = washes.fills(selected);
    let ranges: Vec<std::ops::Range<usize>> = row
        .name_ranges
        .iter()
        .map(|range| range.start..range.start + range.length)
        .collect();

    let keybind = row.state.keybind_text.clone();
    let unavailable = !row.is_available();

    let content = div()
        .flex()
        .flex_col()
        .flex_1()
        .overflow_hidden()
        // `TextTrimming="CharacterEllipsis"` on the name (`:337`) and on the caption (`:350`): a
        // long module name ellipsises inside the row instead of wrapping onto a second line and
        // pushing the caption out of the 48 px box.
        .child(
            div()
                .truncate()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(highlight_text(theme, row.display_name(), &ranges, 14.0)),
        )
        .child(
            div()
                .text_size(px(metrics::text::CAPTION))
                .text_color(theme.search_subtle_text())
                .truncate()
                // `BuildHint`, not the card's `x:Name`: the caption is the destination,
                // the unavailable reason, or the alias that matched.
                .child(SharedString::from(row.hint_text())),
        );

    let mut element = div()
        // A name rather than a formatted string: the id is built on every frame of every row, and
        // `NamedInteger` is the same identity without the allocation.
        .id(gpui::ElementId::named_usize("command-search-row", index))
        .relative()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .h(px(metrics::overlay::ROW_HEIGHT))
        .mx(px(metrics::overlay::ROW_INSET_X))
        .my(px(metrics::overlay::ROW_INSET_Y))
        .px(px(12.0))
        .rounded(px(metrics::radius::OVERLAY_ROW))
        .cursor_pointer()
        .bg(rest_fill)
        // The managed hover layer paints *over* the selected one (`:281-300`), so a selected row
        // under the pointer keeps its stronger wash; the hover wash alone would read as deselected.
        .hover(move |style| style.bg(hover_fill))
        .on_click(on_click);

    if selected {
        // Absolute, as the managed rule is: it is a layer over the row's left edge, not a column.
        // In flow its 2 px and the row's 8 px gap pushed the whole row 10 px right, so moving the
        // selection moved every name, caption and chip sideways.
        element = element.child(
            div()
                .absolute()
                .top(px((metrics::overlay::ROW_HEIGHT - ROW_RULE_HEIGHT) / 2.0))
                .left(px(0.0))
                .w(px(ROW_RULE_WIDTH))
                .h(px(ROW_RULE_HEIGHT))
                .rounded(px(1.0))
                .bg(theme.accent),
        );
    }

    element = element.child(if unavailable {
        div().flex_1().opacity(0.72).child(content)
    } else {
        div().flex_1().child(content)
    });

    if !unavailable && !keybind.is_empty() {
        element = element.child(
            div()
                .rounded(px(metrics::radius::CHIP))
                .border_1()
                .border_color(theme.slider_foreground)
                .bg(theme.slider_background)
                .px(px(6.0))
                .py(px(2.0))
                .text_size(px(metrics::text::ESC))
                .font_family(MONO_FAMILY)
                .text_color(theme.search_subtle_text())
                .child(SharedString::from(keybind)),
        );
    }

    if unavailable {
        element = element.child(
            div()
                .rounded(px(metrics::radius::CHIP))
                .border_1()
                .border_color(theme.slider_foreground)
                .px(px(6.0))
                .py(px(2.0))
                .text_size(px(9.0))
                // `RowSecondaryTextStyle` (`:83-92`): the palette's secondary tone, not a fixed
                // red — the managed pill carried no colour of its own, and a literal here was one
                // palette the HUD ignored.
                .text_color(theme.search_subtle_text())
                .child(copy::SEARCH_ROW_UNAVAILABLE),
        );
    }

    if row.has_toggle() {
        element = element.child(
            div()
                .w(px(metrics::switch::TRACK_WIDTH))
                .flex()
                .justify_center()
                .child(switch_static(theme, row.is_on())),
        );
    }

    element.into_any_element()
}

/// The footer legend and the live-region result count summary.
fn legend(theme: &Theme, state: &CommandSearchState) -> AnyElement {
    let key = |text: &'static str| {
        div()
            .text_size(px(10.5))
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(theme.text)
            .child(text)
    };
    let rest = |text: &'static str| {
        div()
            .text_size(px(10.5))
            .text_color(theme.dim_text)
            .child(text)
    };

    div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .px(px(22.0))
        .py(px(10.0))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(16.0))
                .child(
                    div()
                        .flex()
                        .gap(px(4.0))
                        .child(key(copy::SEARCH_LEGEND_UP_DOWN))
                        .child(rest(copy::SEARCH_LEGEND_NAVIGATE)),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(4.0))
                        .child(key(copy::SEARCH_LEGEND_ENTER))
                        .child(rest(copy::SEARCH_LEGEND_JUMP)),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(4.0))
                        .child(key(copy::SEARCH_LEGEND_F2))
                        .child(rest(copy::SEARCH_LEGEND_TOGGLE)),
                ),
        )
        .child(
            div()
                .text_size(px(10.5))
                .text_color(theme.dim_text)
                .child(SharedString::from(state.results_summary())),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Context, Render, TestAppContext, VisualTestContext, WindowOptions};

    /// The overlay in a window, with the motion setting a test can flip.
    struct OverlayProbe {
        state: CommandSearchState,
        motion: Motion,
        /// The results list's scroll position, which has to outlive a frame.
        scroll: ScrollHandle,
    }

    impl Render for OverlayProbe {
        fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            overlay(
                &Theme::slate(),
                &self.state,
                self.motion,
                window.viewport_size(),
                ResultsScroll::new(&self.scroll, None),
                |_, _, _, _| {},
                |_, _, _| {},
            )
        }
    }

    #[gpui::test]
    fn the_overlay_entrance_is_gated_and_starts_on_its_slide(cx: &mut TestAppContext) {
        let probe = cx.update(|cx| {
            cx.new(|_| OverlayProbe {
                state: CommandSearchState::new(),
                motion: Motion::ANIMATED,
                scroll: ScrollHandle::new(),
            })
        });
        let window = cx.update(|cx| {
            cx.open_window(WindowOptions::default(), |_, _| probe.clone())
                .expect("test window")
        });
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        let animated = visual
            .debug_bounds("command-search-card")
            .expect("the card records its box in a debug build");
        assert!(
            f32::from(animated.size.width) > 0.0 && f32::from(animated.size.height) > 0.0,
            "the card has a box to compare"
        );

        // Flip the gate the way the settings toggle does. Under `Motion::STILL` no
        // animation element is mounted at all, so this frame *is* the settled card — the
        // managed early return (`CommandSearchOverlay.xaml.cs:143-147`).
        visual.update(|_, cx| {
            probe.update(cx, |probe, cx| {
                probe.motion = Motion::STILL;
                cx.notify();
            });
        });
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        let still = visual
            .debug_bounds("command-search-card")
            .expect("the card is still laid out");

        // The entrance is a fade and a slide, not a resize: the card's box moves down by
        // `CardSlide.Y` on the first animated frame and its size and left edge do not move
        // by a pixel. A gate that leaked would move the settled frame too, which is what the
        // comparison against `still` catches.
        assert_eq!(
            (
                f32::from(animated.origin.x),
                f32::from(animated.size.width),
                f32::from(animated.size.height),
            ),
            (
                f32::from(still.origin.x),
                f32::from(still.size.width),
                f32::from(still.size.height),
            ),
            "the entrance must not move the card sideways or resize it"
        );

        let slide = f32::from(animated.origin.y) - f32::from(still.origin.y);
        assert!(
            slide > 0.0 && slide <= motion::OVERLAY_CARD_SLIDE + 0.5,
            "the first animated frame starts {slide} px below the settled card; \
             `CardSlide.Y` runs 8 -> 0 (`CommandSearchOverlay.xaml.cs:163`), and 0 would \
             mean the entrance never ran"
        );
    }

    #[gpui::test]
    fn a_typed_character_reaches_the_query_field(cx: &mut TestAppContext) {
        use gpui::{KeyDownEvent, Keystroke, PlatformInput};

        let probe = cx.update(|cx| {
            let query = cx.new(|cx| {
                TextInput::new(cx, Theme::slate(), "Search modules and settings")
                    .variant(FieldVariant::Query)
            });
            let state = CommandSearchState::new();
            cx.new(|_| QueryProbe {
                state,
                query,
                scroll: ScrollHandle::new(),
            })
        });
        let window = cx.update(|cx| {
            cx.open_window(WindowOptions::default(), |_, _| probe.clone())
                .expect("test window")
        });
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });

        assert!(
            visual.debug_bounds("command-search-query-field").is_some(),
            "the overlay paints the field entity, not the static line"
        );

        // The shell's open path: seed, caret at the end, focus (`CommandSearchOverlay.Open`).
        let query = visual.read(|cx| probe.read(cx).query.clone());
        visual.update(|window, cx| focus_query(&query, "auto", window, cx));
        assert_eq!(
            visual.read(|cx| probe.read(cx).query.read(cx).edit().caret()),
            4,
            "the caret sits at the end of the seeded text"
        );

        let mut keystroke = Keystroke::parse("a").expect("keystroke");
        keystroke.key_char = Some("a".into());
        let result = visual.update(|window, cx| {
            window.dispatch_event(
                PlatformInput::KeyDown(KeyDownEvent {
                    keystroke,
                    is_held: false,
                    prefer_character_input: false,
                }),
                cx,
            )
        });
        assert!(
            !result.propagate,
            "the field's key listener consumed the character, which is how the two input \
             routes stay from double-inserting it"
        );

        assert_eq!(
            visual.read(|cx| probe.read(cx).query.read(cx).text().to_owned()),
            "autoa",
            "a typed character reaches the field through the overlay, at the caret"
        );
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        assert!(
            visual.debug_bounds("command-search-query-field").is_some(),
            "the field is still the painted one after the edit"
        );
    }

    /// A press on the dimmed layer closes the card; a press on the card does not.
    ///
    /// Both halves are new paint, so both halves are asserted: `Scrim_MouseLeftButtonDown`
    /// (`CommandSearchOverlay.xaml.cs:488-492`) closed on a press outside the card, and the card
    /// in managed was an opaque surface that swallowed a press on its own edge. `occlude` is what
    /// makes the second one hold — without it the scrim's backdrop is hovered through the card,
    /// and every press on the card closes the overlay.
    #[gpui::test]
    fn a_scrim_press_closes_and_a_card_press_does_not(cx: &mut TestAppContext) {
        use gpui::{Modifiers, Point};
        use std::cell::Cell;
        use std::rc::Rc;

        let mut state = CommandSearchState::new();
        assert!(
            state.open(Some("esp"), false, &supported_facts()),
            "the overlay opens with a seeded query"
        );

        let closes: Rc<Cell<usize>> = Rc::new(Cell::new(0));
        let probe = cx.update(|cx| {
            let closes = Rc::clone(&closes);
            cx.new(|_| ScrimProbe {
                state,
                scroll: ScrollHandle::new(),
                closes,
            })
        });
        let window = cx.update(|cx| {
            cx.open_window(WindowOptions::default(), |_, _| probe.clone())
                .expect("test window")
        });
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });

        let card = visual
            .debug_bounds("command-search-card")
            .expect("the card paints");
        visual.simulate_click(card.center(), Modifiers::default());
        assert_eq!(closes.get(), 0, "a card press is not a scrim press");

        // The card is centred, so the window's top-left corner is scrim and nothing else.
        visual.simulate_click(Point::new(px(4.0), px(4.0)), Modifiers::default());
        assert_eq!(closes.get(), 1, "a scrim press closes the card");
    }

    /// The ring is drawn while the pulse is live, gone once `prune_pulse` drops it, gone
    /// for another card, and gone under `Motion::STILL`.
    ///
    /// GPUI's test frames expose geometry, not opacity (`the_overlay_entrance_is_gated…`
    /// records the same limit), so this asserts that the ring is **painted** — its box
    /// exists — and the alpha comes from the keyframe test in `views::command_search`,
    /// which asserts `opacity_at` is positive while live and zero at the end.
    #[gpui::test]
    fn a_jump_rings_its_card_and_prune_takes_the_ring_away(cx: &mut TestAppContext) {
        let mut state = CommandSearchState::new();
        assert!(
            state.open(Some("autoclicker"), false, &supported_facts()),
            "the overlay opens with a seeded query"
        );
        let request = state.jump_selected().expect("a jump");
        state
            .begin_pulse(&request, 0, true)
            .expect("motion is on, so the pulse starts");

        let probe = cx.update(|cx| {
            cx.new(|_| RingProbe {
                state,
                motion: Motion::ANIMATED,
                card: "AutoClickerCard",
            })
        });
        let window = cx.update(|cx| {
            cx.open_window(WindowOptions::default(), |_, _| probe.clone())
                .expect("test window")
        });
        let mut visual = VisualTestContext::from_window(window.into(), cx);

        let draw = |visual: &mut VisualTestContext| {
            visual.update(|window, cx| {
                let _ = window.draw(cx);
            });
        };

        draw(&mut visual);
        assert!(
            visual.debug_bounds("card-pulse-ring").is_some(),
            "a live pulse paints the ring on its card"
        );

        // Another card is not the target.
        visual.update(|_, cx| {
            probe.update(cx, |probe, cx| {
                probe.card = "RightClickCard";
                cx.notify();
            });
        });
        draw(&mut visual);
        assert!(
            visual.debug_bounds("card-pulse-ring").is_none(),
            "only the jumped-to card is ringed"
        );

        // Back to the target: still live, so it paints again.
        visual.update(|_, cx| {
            probe.update(cx, |probe, cx| {
                probe.card = "AutoClickerCard";
                cx.notify();
            });
        });
        draw(&mut visual);
        assert!(visual.debug_bounds("card-pulse-ring").is_some());

        // The gate, while the pulse is **still live**: only `Motion::STILL` can explain a
        // missing ring here, so a painter that ignores the setting fails this line.
        visual.update(|_, cx| {
            probe.update(cx, |probe, cx| {
                probe.motion = Motion::STILL;
                cx.notify();
            });
        });
        draw(&mut visual);
        assert!(
            visual.debug_bounds("card-pulse-ring").is_none(),
            "Motion::STILL paints no ring even with a live pulse in the state"
        );
        assert!(
            visual.read(|cx| probe.read(cx).state.pulse().is_some()),
            "the pulse is still in the state, so the assertion above is about the gate"
        );

        // Motion back on, then run the pulse out: the ring goes because the clock did.
        visual.update(|_, cx| {
            probe.update(cx, |probe, cx| {
                probe.motion = Motion::ANIMATED;
                cx.notify();
            });
        });
        draw(&mut visual);
        assert!(visual.debug_bounds("card-pulse-ring").is_some());

        let total = i64::try_from(motion::PULSE_TOTAL.as_millis()).unwrap_or(i64::MAX);
        visual.update(|_, cx| {
            probe.update(cx, |probe, cx| {
                assert!(
                    !probe.state.prune_pulse(total),
                    "the ring has run out of motion"
                );
                cx.notify();
            });
        });
        draw(&mut visual);
        assert!(
            visual.debug_bounds("card-pulse-ring").is_none(),
            "a pruned pulse paints nothing: the settled card is unmarked"
        );
    }

    /// A state whose every catalogue entry is supported, so a jump has a card.
    fn supported_facts() -> command_search::RowFactTable {
        use aoko_core::search;

        let mut facts = command_search::RowFactTable::default();
        for entry in search::entries() {
            facts.set(
                entry.id,
                command_search::RowFacts {
                    supported: true,
                    is_on: false,
                    keybind_text: String::new(),
                },
            );
        }
        facts
    }

    /// A relative box standing in for a settings card, with the ring painter as its last
    /// child the way the shell will compose it.
    struct RingProbe {
        state: CommandSearchState,
        motion: Motion,
        card: &'static str,
    }

    impl Render for RingProbe {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            let theme = Theme::slate();
            let ring = card_pulse_ring(&theme, &self.state, self.motion, self.card);
            div()
                .relative()
                .w(px(320.0))
                .h(px(140.0))
                .child(div().size_full().bg(theme.panel))
                .children(ring)
        }
    }

    /// A window whose root is the overlay, with the close path counted.
    struct ScrimProbe {
        state: CommandSearchState,
        /// The results list's scroll position, which has to outlive a frame.
        scroll: ScrollHandle,
        /// How many times the overlay's `on_close` fired.
        closes: std::rc::Rc<std::cell::Cell<usize>>,
    }

    impl Render for ScrimProbe {
        fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            let closes = std::rc::Rc::clone(&self.closes);
            overlay(
                &Theme::slate(),
                &self.state,
                Motion::STILL,
                window.viewport_size(),
                ResultsScroll::new(&self.scroll, None),
                |_, _, _, _| {},
                move |_, _, _| closes.set(closes.get() + 1),
            )
        }
    }

    /// A window whose root is the overlay with the field wired in.
    struct QueryProbe {
        state: CommandSearchState,
        query: Entity<TextInput>,
        /// The results list's scroll position, which has to outlive a frame.
        scroll: ScrollHandle,
    }

    impl Render for QueryProbe {
        fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            overlay(
                &Theme::slate(),
                &self.state,
                Motion::STILL,
                (&self.query, window.viewport_size()),
                ResultsScroll::new(&self.scroll, None),
                |_, _, _, _| {},
                |_, _, _| {},
            )
        }
    }

    #[test]
    fn the_card_is_never_narrower_than_the_footer_needs() {
        // The card comes from the shell builder's LayoutCard port; this pins the numbers
        // the overlay actually asks for at the product's window size.
        let width = command_search::card_width(metrics::window::WIDTH);
        assert_eq!(width, 620.0);
        assert!(width >= 340.0);
        // At the 560 px floor the card is 416 wide, which still clears the 340 minimum.
        assert_eq!(command_search::card_width(560.0), 416.0);
    }

    #[test]
    fn the_results_area_is_capped_and_never_collapses() {
        let height = command_search::results_viewport_height(760.0, CHROME);
        assert!(height > 0.0);
        assert!(
            height <= 300.0,
            "results area is {height}, above the C# cap"
        );
        // A short window hits the floor rather than showing nothing.
        assert!(command_search::results_viewport_height(200.0, CHROME) >= 100.0);
    }

    #[test]
    fn washes_stay_ordered_on_every_palette() {
        for name in Theme::builtin_names() {
            let theme = Theme::by_name(name);
            let washes = RowWashes::of(&theme);
            assert_eq!(washes.card, theme.panel, "{name}: rest is the card");
            assert_ne!(washes.hover, washes.card, "{name}: hover is invisible");
            assert_ne!(washes.selected, washes.hover, "{name}: not stronger");
            // The selected row under the pointer is the stacked pair, so it is neither of the two:
            // a row that painted the hover wash while selected fails this on every palette.
            assert_ne!(washes.selected_hover, washes.selected, "{name}: wash lost");
            assert_ne!(washes.selected_hover, washes.hover, "{name}: not the stack");
        }
    }
}
