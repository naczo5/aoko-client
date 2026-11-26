//! The colour picker's pixels: `Aoko/ColorPickerDialog.xaml` laid out over
//! [`ColorPickerState`].
//!
//! **Not mounted in Phase 2** (section 12 decision 7). The managed dialog is opened from
//! two places — the palette editor's swatch (`MainWindow.xaml.cs:1075-1085`) and the Block
//! ESP colour row (`:2030-2036`) — and neither has a Rust call site yet, so this view is
//! reachable only from its own test. Mounting it needs a host: an overlay child of the
//! shell (`overlay::overlay` is the shape to copy) plus the two call sites, which is the
//! shell's wiring and not this file's.
//!
//! # What is wired, and what is not
//!
//! * **Wired.** The three RGB sliders write their channel through
//!   [`ColorPickerState::set_channel`]; the hex field takes typing and commits on Enter
//!   (`HexBox_KeyDown`, `ColorPickerDialog.xaml.cs:170-178`) and on leaving the box
//!   (`HexBox_LostFocus`, `:179`), both through one `CommitHex` (`:181-190`); OK and Cancel
//!   report a [`PickResult`] with the state machine's own
//!   [`accept`](ColorPickerState::accept) / [`cancel`](ColorPickerState::cancel), so the
//!   caller's colour is untouched on cancel.
//! * **Wired: pressing and dragging the square and the hue bar.** [`crate::picker`]'s two
//!   painters take the window and app on their drag and release callbacks, so a press
//!   writes [`ColorPickerState::drag_sv`] / [`drag_hue`](ColorPickerState::drag_hue) and
//!   notifies this entity; the thumb and the preview follow on the next frame. The
//!   press-versus-move distinction is the state machine's own drag flag, so a move that
//!   began outside the widget is ignored rather than applied.
//! * **The blur listener is registered once**, in the first `render` that has a window, and
//!   held in a `Subscription`. `HexBox_LostFocus` needs a `&mut Window`, which
//!   [`ColorPickerView::new`] does not have and `Render::render` does; registering per frame
//!   would leak a listener per frame.
//! * **One deviation, named: a refused entry says so.** `CommitHex` reverts in silence —
//!   `:189` is `RefreshAll()`, with no message box and no status line — and on this surface a
//!   box that quietly rewrites what you typed reads as a lost keystroke. So a refusal draws
//!   one 11 px line under the field: `Not a hex colour; kept #3366CC.` in
//!   [`crate::theme::fixed`]'s critical tone. That line is **new copy**, not an element of
//!   `ColorPickerDialog.xaml`. The rule it enforces is the managed one: `TryParseHex`
//!   (`:262-291`) accepts only three or six hex digits, so an empty or partial entry cannot
//!   commit, and [`parse_hex`](crate::dialogs::color_picker::parse_hex) is that same rule.
//!   The line is about one entry, so the next entry and every other edit of the colour (the
//!   square, the bar, a slider) drop it.
//! * **Not wired: nothing.** Everything the managed dialog does with the field, the square,
//!   the bar and the two buttons is reachable here. The one thing this view cannot do is
//!   read back a rasterised glyph, which is the pin's limit rather than a gap.
//! * **The hex field is a child view**, per `components::text_input`'s integration rule: it
//!   is created with a `Context` and drawn inside this view's tree, never as a window root.
//!
//! Colours here are content, not chrome: the swatch, the square and the hue bar paint the
//! colour under edit (through [`crate::picker`] and `Color::rgb`), while every surface,
//! edge and label colour comes from [`Theme`].

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use gpui::prelude::*;
use gpui::{
    div, px, AnyElement, App, Context, Entity, KeyDownEvent, Render, SharedString, Subscription,
    Window,
};

use crate::components::{
    button_sized, slider, text_input::TextInput, ButtonVariant, SliderGeometry,
};
use crate::dialogs::color_picker::{Channel, ColorPickerState, Hsv, PickResult, Rgb};
use crate::theme::{metrics, Theme};

/// `ColorPickerDialog.xaml` window width (`:5`).
pub const DIALOG_WIDTH: f32 = 360.0;
/// The dialog's `Grid Margin="16"` (`:10`).
pub const MARGIN: f32 = 16.0;
/// The R/G/B caption's width and the value's (`ColorPickerDialog.xaml:91-93`).
const CHANNEL_LABEL_WIDTH: f32 = 16.0;
const CHANNEL_VALUE_WIDTH: f32 = 34.0;
/// `Width="90"` on both buttons (`:108-110`).
const BUTTON_WIDTH: f32 = 90.0;

/// One RGB slider row.
struct ChannelRow {
    channel: Channel,
    caption: &'static str,
    value: u8,
}

/// The dialog's result sink.
///
/// One callback for both outcomes: the [`PickResult`] carries `accepted`, which is what
/// `TryPick(owner, initial, out picked)` modelled with a `bool` return.
pub type ResultCallback = dyn Fn(PickResult, &mut Window, &mut App);

/// The dialog, as a GPUI entity.
pub struct ColorPickerView {
    state: ColorPickerState,
    theme: Theme,
    /// The hex box, per `HexBox` (`ColorPickerDialog.xaml:83`).
    hex: Entity<TextInput>,
    /// What the field last reported, drained at the start of the next render. The field's
    /// callback has no `Context<Self>`, so this is the seam that keeps typing out of a
    /// borrow of the view.
    pending_hex: Rc<RefCell<Option<String>>>,
    /// The one line under the field that says a refused entry was refused, and what was kept
    /// instead. Cleared by the next report and by an accepted commit.
    hex_notice: Option<SharedString>,
    /// The field's blur listener, registered once on the first render — that is the one place
    /// this view has a `&mut Window` to register it against. Registering per frame would leak
    /// a subscription per frame.
    hex_blur: Option<Subscription>,
    on_result: Option<Arc<ResultCallback>>,
}

impl ColorPickerView {
    /// A picker opened on `initial`, as `TryPick(owner, initial, out picked)` does.
    pub fn new(cx: &mut Context<Self>, theme: Theme, initial: Rgb) -> Self {
        let pending = Rc::new(RefCell::new(None));
        let sink = pending.clone();
        let field_theme = theme.clone();
        let hex = cx.new(|cx| {
            TextInput::new(cx, field_theme, "").on_change(move |text, _, _| {
                *sink.borrow_mut() = Some(text.to_string());
            })
        });

        Self {
            state: ColorPickerState::new(initial),
            theme,
            hex,
            pending_hex: pending,
            hex_notice: None,
            hex_blur: None,
            on_result: None,
        }
    }

    /// Called with the outcome of OK or Cancel.
    #[must_use]
    pub fn on_result(
        mut self,
        handler: impl Fn(PickResult, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_result = Some(Arc::new(handler));
        self
    }

    /// The state under the view, for a host that needs the current colour.
    #[must_use]
    pub fn state(&self) -> &ColorPickerState {
        &self.state
    }

    /// The colour the swatch shows.
    #[must_use]
    pub fn selected(&self) -> Rgb {
        self.state.selected()
    }

    /// The HSV the square and the hue bar paint.
    #[must_use]
    pub fn hsv(&self) -> Hsv {
        self.state.hsv()
    }

    /// Drains the field's report into the state, then writes the state's value back into
    /// the field when the two disagree (the sliders moved the colour).
    fn sync_hex(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = self.pending_hex.borrow_mut().take() {
            self.state.set_hex_text(&text);
            // The user is editing again, so the last refusal is stale.
            self.hex_notice = None;
        }

        let current = self.state.hex_text().to_owned();
        if self.hex.read(cx).text() != current.as_str() {
            self.hex
                .update(cx, |field, cx| field.set_text(&current, cx));
        }
    }

    /// `CommitHex` (`ColorPickerDialog.xaml.cs:181-190`): a parsable entry is adopted and
    /// anything else reverts the field to the last valid colour.
    ///
    /// Returns whether the entry was accepted. A refusal also says so in [`Self::hex_notice`]:
    /// the managed handler reverts in silence (`:189`, `RefreshAll()`), and a box that quietly
    /// rewrites what you typed reads as a lost keystroke. The notice is this view's one
    /// visible addition to the managed dialog, and the module docs name it.
    ///
    /// No re-entrancy guard is needed for the managed `_updating` (`:183`): writes into the
    /// field go through [`TextInput::set_text`], which deliberately does not fire `on_change`,
    /// so a programmatic rewrite cannot come back as another commit.
    fn commit_hex(&mut self, cx: &mut Context<Self>) -> bool {
        let accepted = self.state.commit_hex();
        let current = self.state.hex_text().to_owned();
        self.hex
            .update(cx, |field, cx| field.set_text(&current, cx));
        self.hex_notice = if accepted {
            None
        } else {
            Some(SharedString::from(format!(
                "Not a hex colour; kept {current}."
            )))
        };
        cx.notify();
        accepted
    }

    fn finish(&mut self, accepted: bool, window: &mut Window, cx: &mut Context<Self>) {
        let result = if accepted {
            self.state.accept()
        } else {
            self.state.cancel()
        };
        if let Some(handler) = self.on_result.clone() {
            handler(result, window, cx);
        }
    }

    fn channel_rows(&self) -> [ChannelRow; 3] {
        let color = self.state.selected();
        [
            ChannelRow {
                channel: Channel::Red,
                caption: "R",
                value: color.r,
            },
            ChannelRow {
                channel: Channel::Green,
                caption: "G",
                value: color.g,
            },
            ChannelRow {
                channel: Channel::Blue,
                caption: "B",
                value: color.b,
            },
        ]
    }

    fn render_channel(&self, row: ChannelRow, theme: &Theme, entity: &Entity<Self>) -> AnyElement {
        let channel = row.channel;
        let handle = entity.clone();
        let geometry = SliderGeometry::new(0.0, 255.0, 1.0);
        let caption = div()
            .w(px(CHANNEL_LABEL_WIDTH))
            .text_size(px(metrics::text::SECTION_TITLE))
            .text_color(theme.dim_text)
            .child(row.caption);
        let value = div()
            .w(px(CHANNEL_VALUE_WIDTH))
            .text_size(px(metrics::text::SECTION_TITLE))
            .text_color(theme.dim_text)
            .child(SharedString::from(row.value.to_string()));

        let mut slider_row = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.0))
            .w_full()
            .child(caption)
            .child(div().flex_1().min_w(px(0.0)).child(slider(
                SharedString::from(format!("color-channel-{caption}", caption = row.caption)),
                theme,
                geometry,
                f64::from(row.value),
                move |next, _window, cx| {
                    handle.update(cx, |view, cx| {
                        // A new edit, so the last refusal is stale.
                        view.hex_notice = None;
                        view.state.set_channel(channel, channel_byte(next));
                        cx.notify();
                    });
                },
            )))
            .child(value);

        if row.channel == Channel::Red {
            slider_row = slider_row.debug_selector(|| String::from("color-picker-red-row"));
        }

        slider_row.into_any_element()
    }
}

/// A slider position as a channel byte: the geometry is `0..=255` with a 1 step, so this is
/// a re-clamp rather than a conversion of arbitrary input.
fn channel_byte(value: f64) -> u8 {
    let clamped = value.round().clamp(0.0, 255.0);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    {
        clamped as u8
    }
}

impl Render for ColorPickerView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_hex(cx);

        // `HexBox_LostFocus` (`ColorPickerDialog.xaml.cs:179`) runs the same commit as Enter,
        // so leaving the box is enough to apply the entry. Registered once, on the first
        // frame that has a window; a per-frame registration would leak a subscription per
        // frame, which is why this is a stored `Option<Subscription>` and not a call here.
        if self.hex_blur.is_none() {
            let handle = self.hex.read(cx).focus_handle().clone();
            self.hex_blur = Some(cx.on_blur(&handle, window, |view, _window, cx| {
                view.commit_hex(cx);
            }));
        }

        let theme = self.theme.clone();
        let hsv = self.state.hsv();
        let selected = self.state.selected();
        let entity = cx.entity();
        let hex = self.hex.clone();

        // Pressing the square or the bar writes this entity's state and repaints: the
        // painters take the window and app so a press becomes a value. `drag_sv`/`drag_hue`
        // set the state machine's own drag flag, which is what keeps a move that started
        // elsewhere from being applied.
        let square = {
            let handle = entity.clone();
            let released = entity.clone();
            crate::picker::sv_square(
                "color-picker-sv",
                hsv,
                move |x, y, _window, cx| {
                    handle.update(cx, |view, cx| {
                        // A new edit, so the last refusal is stale.
                        view.hex_notice = None;
                        view.state.drag_sv(x, y);
                        cx.notify();
                    });
                },
                move |_window, cx| {
                    released.update(cx, |view, cx| {
                        view.state.end_sv();
                        cx.notify();
                    });
                },
            )
        };
        let hue = {
            let handle = entity.clone();
            let released = entity.clone();
            crate::picker::hue_bar(
                "color-picker-hue",
                hsv,
                move |y, _window, cx| {
                    handle.update(cx, |view, cx| {
                        // A new edit, so the last refusal is stale.
                        view.hex_notice = None;
                        view.state.drag_hue(y);
                        cx.notify();
                    });
                },
                move |_window, cx| {
                    released.update(cx, |view, cx| {
                        view.state.end_hue();
                        cx.notify();
                    });
                },
            )
        };
        let swatch = crate::picker::preview_swatch(&theme, selected);

        let commit = entity.clone();
        let hex_row = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.0))
            .debug_selector(|| String::from("color-picker-hex-row"))
            .child(swatch)
            .child(
                div()
                    .text_size(px(metrics::text::SECTION_TITLE))
                    .text_color(theme.dim_text)
                    .child("Hex"),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    // `HexBox_KeyDown`: Enter commits, Escape does not (the dialog's Cancel
                    // owns Escape).
                    .on_key_down(move |event: &KeyDownEvent, _window, cx| {
                        if event.keystroke.key.eq_ignore_ascii_case("enter") {
                            commit.update(cx, |view, cx| {
                                view.commit_hex(cx);
                            });
                            cx.stop_propagation();
                        }
                    })
                    .child(hex),
            );

        let channels = div().flex().flex_col().gap(px(2.0)).w_full().children(
            self.channel_rows()
                .map(|row| self.render_channel(row, &theme, &entity)),
        );

        let cancel = {
            let handle = entity.clone();
            div()
                .debug_selector(|| String::from("color-picker-cancel"))
                .child(button_sized(
                    "color-picker-cancel",
                    &theme,
                    ButtonVariant::Dark,
                    SharedString::from("Cancel"),
                    BUTTON_WIDTH,
                    move |_, window, cx| {
                        handle.update(cx, |view, cx| view.finish(false, window, cx));
                    },
                ))
        };
        let ok = {
            let handle = entity.clone();
            div()
                .debug_selector(|| String::from("color-picker-ok"))
                .child(button_sized(
                    "color-picker-ok",
                    &theme,
                    ButtonVariant::Dark,
                    SharedString::from("OK"),
                    BUTTON_WIDTH,
                    move |_, window, cx| {
                        handle.update(cx, |view, cx| view.finish(true, window, cx));
                    },
                ))
        };

        // The one visible addition to the managed dialog: a refused hex entry says what it
        // refused and what it kept instead of silently rewriting the box.
        let hex_notice = self.hex_notice.clone().map(|notice| {
            div()
                .debug_selector(|| String::from("color-picker-hex-notice"))
                .text_size(px(metrics::text::CAPTION))
                .text_color(crate::theme::fixed::CRITICAL)
                .child(notice)
        });

        div()
            .id("color-picker")
            .debug_selector(|| String::from("color-picker"))
            .flex()
            .flex_col()
            .gap(px(12.0))
            .w(px(DIALOG_WIDTH))
            .p(px(MARGIN))
            .bg(theme.background)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap(px(10.0))
                    .child(
                        div()
                            .debug_selector(|| String::from("color-picker-sv"))
                            .child(square),
                    )
                    .child(
                        div()
                            .debug_selector(|| String::from("color-picker-hue"))
                            .child(hue),
                    ),
            )
            .child(hex_row)
            .children(hex_notice)
            .child(channels)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_end()
                    .gap(px(8.0))
                    .child(cancel)
                    .child(ok),
            )
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use gpui::{Modifiers, TestAppContext, VisualTestContext, WindowOptions};

    use super::*;

    /// A window whose content is the dialog, plus every outcome it reported.
    struct PickerProbe {
        view: Entity<ColorPickerView>,
    }

    impl Render for PickerProbe {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(self.view.clone())
        }
    }

    struct Probe {
        visual: VisualTestContext,
        view: Entity<ColorPickerView>,
        results: Arc<Mutex<Vec<PickResult>>>,
    }

    impl Probe {
        fn click(&mut self, selector: &'static str) {
            let bounds = self
                .visual
                .debug_bounds(selector)
                .unwrap_or_else(|| panic!("{selector} renders"));
            self.visual
                .simulate_click(bounds.center(), Modifiers::default());
        }

        fn selected(&self) -> Rgb {
            self.visual.cx.read(|cx| self.view.read(cx).selected())
        }

        fn hex_text(&self) -> String {
            self.visual
                .cx
                .read(|cx| self.view.read(cx).state().hex_text().to_owned())
        }

        fn type_hex(&mut self, text: &'static str) {
            let field = self.visual.cx.read(|cx| self.view.read(cx).hex.clone());
            self.visual.cx.update(|cx| {
                field.update(cx, |field, cx| field.set_text(text, cx));
            });
        }

        /// One typed entry, as the field's `on_change` hands it over.
        ///
        /// The widget's own typing path is covered by `components::text_input`; what this
        /// reproduces is the hand-off — a report that `sync_hex` drains at the start of the
        /// next frame — so the commit under test sees exactly what a keystroke would have
        /// produced, without depending on where a glyph landed. The notify is part of the
        /// hand-off: without it the view is not re-rendered, so no frame drains the report.
        fn report_hex(&mut self, text: &'static str) {
            let view = self.view.clone();
            self.visual.cx.update(|cx| {
                let _ = view
                    .read(cx)
                    .pending_hex
                    .borrow_mut()
                    .replace(text.to_owned());
                view.update(cx, |_, cx| cx.notify());
            });
            self.draw();
        }

        /// Focuses the hex box and then takes focus away, which is `HexBox_LostFocus`.
        ///
        /// Three frames, because focus listeners are dispatched inside `Window::draw`
        /// (`crates/gpui/src/window.rs:3938-3993`): `Window::blur` only clears the focus and
        /// marks the frame dirty (`:2785-2800`), so the draw after it is the one that
        /// delivers the blur, and the draw after that paints what the commit wrote. The
        /// focus move needs its own frame for the same reason — the blur listener compares
        /// the focus path of the frame before with the one being drawn.
        fn blur_hex(&mut self) {
            let handle = self
                .visual
                .cx
                .read(|cx| self.view.read(cx).hex.read(cx).focus_handle().clone());
            self.visual.update(|window, cx| {
                window.focus(&handle, cx);
            });
            self.draw();
            self.visual.update(|window, cx| {
                window.blur(cx);
            });
            self.draw();
            self.draw();
        }

        fn draw(&mut self) {
            self.visual.update(|window, cx| {
                let _ = window.draw(cx);
            });
        }

        fn notice(&self) -> Option<String> {
            self.visual.cx.read(|cx| {
                self.view
                    .read(cx)
                    .hex_notice
                    .as_ref()
                    .map(|notice| notice.as_str().to_owned())
            })
        }

        fn results(&self) -> Vec<PickResult> {
            self.results.lock().expect("results lock").clone()
        }
    }

    fn probe(cx: &mut TestAppContext) -> Probe {
        let results: Arc<Mutex<Vec<PickResult>>> = Arc::new(Mutex::new(Vec::new()));
        let recorded = results.clone();
        let (probe, view) = cx.update(|cx| {
            let view = cx.new(|cx| {
                ColorPickerView::new(cx, Theme::slate(), Rgb::new(255, 0, 0)).on_result(
                    move |result, _, _| {
                        recorded.lock().expect("results lock").push(result);
                    },
                )
            });
            let probe = cx.new(|_| PickerProbe { view: view.clone() });
            (probe, view)
        });
        let window = cx.update(|cx| {
            cx.open_window(WindowOptions::default(), |_, _| probe.clone())
                .expect("test window")
        });
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        Probe {
            visual,
            view,
            results,
        }
    }

    #[gpui::test]
    fn every_part_of_the_dialog_has_a_place_in_the_window(cx: &mut TestAppContext) {
        let mut probe = probe(cx);

        for selector in [
            "color-picker",
            "color-picker-sv",
            "color-picker-hue",
            "color-picker-hex-row",
            "color-picker-red-row",
        ] {
            let bounds = probe
                .visual
                .debug_bounds(selector)
                .unwrap_or_else(|| panic!("{selector} is laid out"));
            assert!(
                f32::from(bounds.size.width) > 0.0 && f32::from(bounds.size.height) > 0.0,
                "{selector} has a non-empty box"
            );
        }

        assert_eq!(probe.selected(), Rgb::new(255, 0, 0));
        assert_eq!(
            probe.visual.cx.read(|cx| probe.view.read(cx).hsv().hue),
            0.0
        );
    }

    #[gpui::test]
    fn ok_and_cancel_report_the_state_machines_own_result(cx: &mut TestAppContext) {
        let mut probe = probe(cx);

        probe.click("color-picker-ok");
        probe.click("color-picker-cancel");

        assert_eq!(
            probe.results(),
            [
                PickResult {
                    color: Rgb::new(255, 0, 0),
                    accepted: true,
                },
                PickResult {
                    color: Rgb::new(255, 0, 0),
                    accepted: false,
                },
            ],
            "OK accepts the selected colour and Cancel returns the initial one"
        );
        assert_eq!(probe.selected(), Rgb::new(255, 0, 0));
    }

    #[gpui::test]
    fn the_hex_field_shows_the_state_and_a_bad_entry_reverts(cx: &mut TestAppContext) {
        let mut probe = probe(cx);

        assert_eq!(probe.hex_text(), "#FF0000");

        // An unparsable entry reverts to the last valid colour rather than being swallowed.
        probe.type_hex("not a colour");
        probe.click("color-picker-ok");

        assert_eq!(
            probe.hex_text(),
            "#FF0000",
            "the commit reverted the field to the selected colour"
        );
    }

    #[gpui::test]
    fn a_press_on_the_square_writes_the_colour_and_the_hex_box_follows(cx: &mut TestAppContext) {
        let mut probe = probe(cx);

        // The centre of the square: half saturation, half value, which is neither the red
        // the dialog opened on nor a colour any other control wrote.
        probe.click("color-picker-sv");
        // `sync_hex` runs at the start of a frame, so paint before reading the field.
        probe.visual.update(|window, cx| {
            let _ = window.draw(cx);
        });

        let selected = probe.selected();
        assert_ne!(selected, Rgb::new(255, 0, 0), "the press wrote a colour");
        let field = probe
            .visual
            .cx
            .read(|cx| probe.view.read(cx).hex.read(cx).text().to_owned());
        assert_eq!(
            field,
            selected.to_hex(),
            "the hex box follows the state, which is what `sync_hex` exists for"
        );
    }

    #[gpui::test]
    fn losing_focus_commits_a_good_entry_and_reports_a_bad_one(cx: &mut TestAppContext) {
        let mut probe = probe(cx);

        // A typed entry, then leaving the box: `HexBox_LostFocus` runs the same commit as
        // Enter, so the colour is adopted without a keypress.
        probe.report_hex("#3366CC");
        assert_eq!(probe.hex_text(), "#3366CC", "the report reached the state");
        probe.blur_hex();

        assert_eq!(
            probe.selected(),
            Rgb::new(0x33, 0x66, 0xCC),
            "leaving the box committed the entry"
        );
        assert_eq!(probe.notice(), None, "an accepted entry needs no notice");
        assert!(
            probe
                .visual
                .debug_bounds("color-picker-hex-notice")
                .is_none(),
            "and draws none"
        );

        // A bad entry: the colour is the one before, the field goes back to it, and the
        // dialog says what it kept rather than rewriting the box in silence.
        probe.report_hex("nonsense");
        assert_eq!(probe.hex_text(), "nonsense");
        probe.blur_hex();

        assert_eq!(
            probe.selected(),
            Rgb::new(0x33, 0x66, 0xCC),
            "a refused entry commits nothing"
        );
        assert_eq!(
            probe.hex_text(),
            "#3366CC",
            "the field is back at the last valid colour"
        );
        assert_eq!(
            probe.notice().as_deref(),
            Some("Not a hex colour; kept #3366CC."),
            "the refusal is reported"
        );
        assert!(
            probe
                .visual
                .debug_bounds("color-picker-hex-notice")
                .is_some(),
            "and the notice is on screen"
        );

        // A partial entry is a refusal too, not a commit of the digits it has.
        probe.report_hex("#33");
        probe.blur_hex();
        assert_eq!(
            probe.selected(),
            Rgb::new(0x33, 0x66, 0xCC),
            "two digits are not a colour"
        );

        // Typing again clears the stale complaint.
        probe.report_hex("#00FF00");
        assert_eq!(probe.notice(), None, "editing dismisses the notice");

        // So does touching the colour any other way: the line is about the entry the field
        // refused, so it cannot outlive it.
        probe.report_hex("#12345");
        probe.blur_hex();
        assert!(probe.notice().is_some(), "five digits are refused too");
        probe.click("color-picker-sv");
        probe.draw();
        assert_eq!(
            probe.notice(),
            None,
            "a press on the square makes the refusal stale"
        );
        assert!(
            probe
                .visual
                .debug_bounds("color-picker-hex-notice")
                .is_none(),
            "and the line goes with it"
        );
    }
}
