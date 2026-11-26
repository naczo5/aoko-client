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
//!   (`HexBox_KeyDown`); OK and Cancel report a [`PickResult`] with the state machine's own
//!   [`accept`](ColorPickerState::accept) / [`cancel`](ColorPickerState::cancel), so the
//!   caller's colour is untouched on cancel.
//! * **Wired: pressing and dragging the square and the hue bar.** [`crate::picker`]'s two
//!   painters take the window and app on their drag and release callbacks, so a press
//!   writes [`ColorPickerState::drag_sv`] / [`drag_hue`](ColorPickerState::drag_hue) and
//!   notifies this entity; the thumb and the preview follow on the next frame. The
//!   press-versus-move distinction is the state machine's own drag flag, so a move that
//!   began outside the widget is ignored rather than applied.
//! * **Not wired: the commit on focus loss.** `HexBox_LostFocus` commits the same way
//!   Enter does. The state machine's commit is reachable and tested; the trigger needs a
//!   blur subscription on the field's focus handle, which the view cannot register in
//!   `Render` without leaking one subscription per frame. Enter is the wired trigger.
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
use gpui::{div, px, AnyElement, App, Context, Entity, KeyDownEvent, Render, SharedString, Window};

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
    on_result: Option<Arc<dyn Fn(PickResult, &mut Window, &mut App)>>,
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
        }

        let current = self.state.hex_text().to_owned();
        if self.hex.read(cx).text() != current.as_str() {
            self.hex
                .update(cx, |field, cx| field.set_text(&current, cx));
        }
    }

    /// `CommitHex`: adopts a parsable entry, otherwise reverts to the last valid colour.
    /// Returns whether the entry was accepted, which the view shows in the field it already
    /// rewrote.
    fn commit_hex(&mut self, cx: &mut Context<Self>) -> bool {
        let accepted = self.state.commit_hex();
        let current = self.state.hex_text().to_owned();
        self.hex
            .update(cx, |field, cx| field.set_text(&current, cx));
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_hex(cx);

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
}
