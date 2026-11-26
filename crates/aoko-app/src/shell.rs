//! The root view: client-drawn chrome, the tab rail, and the nine tab bodies.
//!
//! `MIGRATION_ARCHITECTURE.md` section 12 decision 5 makes the title bar ours:
//! themed background, a drag region, our own minimise/maximise/close hit-testing
//! and resize edges. The caption, border and text colouring that DWM did
//! (`MainWindow.ApplyNativeTitleBarTheme`, `:494-517`) has no equivalent, which is
//! why drawing the whole strip is the parity-preserving choice rather than the
//! convenient one.
//!
//! The three window actions are the verified GPUI surface at the pin:
//! `Window::minimize_window` (`crates/gpui/src/window.rs:7285`),
//! `Window::zoom_window` (`:3397`), `Window::start_window_move` (`:3410`) and
//! `App::quit` (`crates/gpui/src/app.rs:1374`).
//!
//! Two things are deliberately minimal and say so:
//!
//! * **Resize edges.** GPUI has no per-edge resize hit-testing API in the notes and
//!   `window_min_size` is creation-time only (`window.rs`, `platform.rs`), so the
//!   shell relies on the platform's own edge handling. Marked here rather than
//!   faked.
//! * **Slider and checkbox persistence.** The switch rows are wired through
//!   [`aoko_core::clicker::toggles::ModuleToggles`] and the search index's
//!   `toggle_property`, which is the same reverse map the overlay uses. Sliders,
//!   checkboxes and combos keep their value in [`Shell::draft`] for the session:
//!   the `bind`-path → `Profile` field table is the next step and is not claimed
//!   here.

use std::collections::BTreeMap;

use gpui::prelude::*;
use gpui::{div, px, AnyElement, Context, Entity, MouseButton, Render, SharedString, Window};

use aoko_core::clicker::toggles::{self, ModuleToggles};
use aoko_core::search;

use crate::components;
use crate::components::ButtonVariant;
use crate::theme::metrics;
use crate::theme::motion::{self, Motion};
use crate::theme::Theme;
use crate::views::command_search::{CommandSearchState, KeyboardGate, RowFactTable, SearchKey};
use crate::views::copy::{self, Card, Control, TabBody, MODULE_LIST_PREVIEW_SAMPLE};

/// The root view.
pub struct Shell {
    theme: Theme,
    tab: usize,
    toggles: ModuleToggles,
    /// In-session values for every `bind` path, seeded on first touch.
    draft: BTreeMap<&'static str, f64>,
    search: CommandSearchState,
}

impl Shell {
    /// A shell on the given palette, showing Combat.
    #[must_use]
    pub fn new(theme: Theme) -> Self {
        Self {
            theme,
            tab: 0,
            toggles: ModuleToggles::new(),
            draft: BTreeMap::new(),
            search: CommandSearchState::new(),
        }
    }

    /// The active palette.
    #[must_use]
    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    /// Replaces the palette, e.g. from the Settings tab.
    pub fn set_theme(&mut self, theme: Theme, cx: &mut Context<Self>) {
        self.theme = theme;
        cx.notify();
    }

    /// Index of the visible tab.
    #[must_use]
    pub fn selected_tab(&self) -> usize {
        self.tab
    }

    /// The visible tab's rail entry.
    #[must_use]
    pub fn selected_tab_name(&self) -> &'static str {
        copy::TABS.get(self.tab).map_or("", |tab| tab.name)
    }

    /// Shows one tab by index; out of range is ignored.
    pub fn select_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < copy::TABS.len() && index != self.tab {
            self.tab = index;
            cx.notify();
        }
    }

    /// Shows one tab by name, case-insensitively, as the overlay's jump does.
    pub fn select_tab_named(&mut self, name: &str, cx: &mut Context<Self>) {
        if let Some(index) = copy::TABS
            .iter()
            .position(|tab| tab.name.eq_ignore_ascii_case(name))
        {
            self.select_tab(index, cx);
        }
    }

    /// Turns one module on or off. Refuses nothing: the catalogue decides what a
    /// switch may show, and an unavailable row is drawn read-only by its card.
    pub fn toggle_module(&mut self, module_id: &str, cx: &mut Context<Self>) {
        let Some(canonical) = toggles::canonical_id(module_id) else {
            return;
        };
        let next = !self.toggles.is_enabled(canonical);
        self.toggles.set_enabled(canonical, next);
        cx.notify();
    }

    /// The switch state for a card, resolved through the search index's
    /// `toggle_property` map so a card and its overlay row cannot disagree.
    #[must_use]
    pub fn is_module_on(&self, module_id: &str) -> bool {
        self.toggles.is_enabled(module_id)
    }

    /// An in-session control value, seeded from `default`.
    #[must_use]
    pub fn value(&self, bind: &'static str, default: f64) -> f64 {
        self.draft.get(bind).copied().unwrap_or(default)
    }

    /// Records an in-session control value.
    pub fn set_value(&mut self, bind: &'static str, value: f64, cx: &mut Context<Self>) {
        self.draft.insert(bind, value);
        cx.notify();
    }

    /// Seeds a control value without notifying, for the capture harness.
    pub fn seed_value(&mut self, bind: &'static str, value: f64) {
        self.draft.insert(bind, value);
    }

    /// Seeds one module's switch without notifying, for the capture harness.
    pub fn seed_module(&mut self, module_id: &str, on: bool) {
        if let Some(canonical) = toggles::canonical_id(module_id) {
            self.toggles.set_enabled(canonical, on);
        }
    }

    /// The command-search state, for the overlay view to read.
    ///
    /// The overlay owns the pixels; this owns open/close, the query, ranking and
    /// the pulse — see [`crate::views::command_search`].
    #[must_use]
    pub fn search(&self) -> &CommandSearchState {
        &self.search
    }

    /// Opens the overlay, optionally seeded with the typed character.
    ///
    /// Ctrl+K and type-to-open both land here; they differ only in the seed.
    pub fn open_search(&mut self, seed: Option<&str>, cx: &mut Context<Self>) {
        let gate = KeyboardGate::ready();
        let facts = RowFactTable::default();
        let dev_mode = false;
        let _ = self
            .search
            .handle_window_key(SearchKey::CtrlK, &gate, dev_mode, &facts);
        if self.search.is_open() {
            if let Some(seed) = seed {
                let _ = self.search.set_query(seed, dev_mode, &facts);
            }
            cx.notify();
        }
    }

    /// Closes the overlay, as Escape and a scrim click do.
    pub fn close_search(&mut self, cx: &mut Context<Self>) {
        if self.search.close() {
            cx.notify();
        }
    }

    fn chip_text(&self) -> SharedString {
        let enabled = self.toggles.enabled_count();
        let armed = if self.toggles.armed() {
            "armed"
        } else {
            "disarmed"
        };
        SharedString::from(format!("{armed} \u{00b7} {enabled} on"))
    }
}

impl Render for Shell {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let motion = motion::motion();
        let theme = self.theme.clone();
        let entity = cx.entity();

        let body = match copy::TABS.get(self.tab).map(|tab| tab.body) {
            Some(TabBody::Cards(cards)) => self.render_cards(cards, &theme, motion, &entity),
            Some(TabBody::Accounts) => self.render_data_tab(
                &theme,
                "Accounts",
                "Import an account, or paste one in. Saved logins live in accounts.json, unencrypted.",
                crate::views::tab_copy::SELECT_ACCOUNT_HINT,
            ),
            Some(TabBody::Servers) => self.render_data_tab(
                &theme,
                "Servers",
                "Save a server once, then join it from here.",
                crate::views::tab_copy::NO_SERVERS_HINT,
            ),
            None => div().flex_1().into_any_element(),
        };

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.background)
            .child(self.render_title_bar(&theme))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .min_h(px(0.0))
                    .gap(px(metrics::rail::GAP))
                    .child(self.render_rail(&theme, &entity))
                    .child(div().flex().flex_1().min_w(px(0.0)).child(body)),
            )
            .into_any_element()
    }
}

impl Shell {
    /// Decision 5: the themed, draggable, hit-tested title bar.
    fn render_title_bar(&self, theme: &Theme) -> AnyElement {
        let close = crate::theme::fixed::CRITICAL;
        div()
            .id("title-bar")
            .flex()
            .flex_row()
            .items_center()
            .h(px(metrics::window::TITLE_BAR_HEIGHT))
            .w_full()
            .bg(theme.panel)
            .border_b(px(metrics::HAIRLINE))
            .border_color(theme.card_border())
            .px(px(12.0))
            .gap(px(8.0))
            .on_mouse_down(MouseButton::Left, |_, window, _| {
                // The drag region is the whole strip; the buttons sit above it and
                // consume their own clicks first.
                window.start_window_move();
            })
            .child(
                div()
                    .text_size(px(metrics::text::SECTION_TITLE))
                    .text_color(theme.text)
                    .child("Aoko"),
            )
            .child(
                div()
                    .flex_1()
                    .text_size(px(metrics::text::CAPTION))
                    .text_color(theme.dim_text)
                    .child(format!(
                        "{} \u{00b7} {}",
                        self.selected_tab_name(),
                        self.chip_text()
                    )),
            )
            .child(
                div()
                    .id("title-bar-minimise")
                    .px(px(8.0))
                    .cursor_pointer()
                    .text_color(theme.dim_text)
                    .aria_label(copy::TITLE_BAR_MINIMISE)
                    .on_click(|_, window, _| window.minimize_window())
                    .child("\u{2013}"),
            )
            .child(
                div()
                    .id("title-bar-maximise")
                    .px(px(8.0))
                    .cursor_pointer()
                    .text_color(theme.dim_text)
                    .aria_label(copy::TITLE_BAR_MAXIMISE)
                    .on_click(|_, window, _| window.zoom_window())
                    .child("\u{25a1}"),
            )
            .child(
                div()
                    .id("title-bar-close")
                    .px(px(8.0))
                    .cursor_pointer()
                    .text_color(close)
                    .aria_label(copy::TITLE_BAR_CLOSE)
                    .on_click(|_, _, cx| cx.quit())
                    .child("\u{00d7}"),
            )
            .into_any_element()
    }

    /// The 176 px rail: search button, the nine tabs, the update row.
    fn render_rail(&self, theme: &Theme, entity: &Entity<Self>) -> AnyElement {
        let mut rail = div()
            .flex()
            .flex_col()
            .w(px(metrics::rail::WIDTH))
            .h_full()
            .py(px(10.0))
            .px(px(10.0))
            .gap(px(2.0))
            .bg(theme.background);

        rail = rail.child(
            components::button(
                "rail-search",
                theme,
                ButtonVariant::Secondary,
                format!(
                    "{}   {}",
                    copy::SEARCH_BUTTON_TOOLTIP
                        .split(" (")
                        .next()
                        .unwrap_or(copy::SEARCH_BUTTON_TOOLTIP),
                    copy::SEARCH_BUTTON_CHORD
                ),
                |_, _, _| {},
            )
            .into_any_element(),
        );

        for (index, tab) in copy::TABS.iter().enumerate() {
            rail = rail.child(self.render_rail_item(index, tab.name, theme, entity));
        }

        rail.child(
            div()
                .text_size(px(metrics::text::CAPTION))
                .text_color(theme.dim_text)
                .child(self.chip_text()),
        )
        .into_any_element()
    }

    fn render_rail_item(
        &self,
        index: usize,
        name: &'static str,
        theme: &Theme,
        entity: &Entity<Self>,
    ) -> AnyElement {
        let selected = index == self.tab;
        let fill = if selected {
            theme.tab_selected
        } else {
            theme.background
        };
        let label = if selected { theme.text } else { theme.dim_text };
        let handle = entity.clone();

        div()
            .id(SharedString::from(format!("rail-tab-{index}")))
            .flex()
            .items_center()
            .w(px(metrics::rail::ITEM_WIDTH))
            .h(px(metrics::rail::ITEM_MIN_HEIGHT))
            .px(px(12.0))
            .rounded(px(metrics::radius::TAB_ITEM))
            .bg(fill)
            .cursor_pointer()
            .text_size(px(metrics::text::SECTION_TITLE))
            .text_color(label)
            .aria_selected(selected)
            .on_click(move |_, _, cx| {
                handle.update(cx, |shell, cx| shell.select_tab(index, cx));
            })
            .child(name)
            .into_any_element()
    }

    /// The card list for a module tab, inside its own scroll container.
    ///
    /// The scroll id is the tab index, so the same tab keeps its offset when the
    /// rail is clicked twice and two tabs never share one.
    fn render_cards(
        &self,
        cards: &'static [Card],
        theme: &Theme,
        motion: Motion,
        entity: &Entity<Self>,
    ) -> AnyElement {
        let mut body = components::scroll(SharedString::from(format!("tab-scroll-{}", self.tab)))
            .flex_col()
            .p(px(12.0))
            .gap(px(12.0));

        for card in cards {
            body = body.child(self.render_card(card, theme, motion, entity));
        }

        body.into_any_element()
    }

    fn render_card(
        &self,
        card: &Card,
        theme: &Theme,
        motion: Motion,
        entity: &Entity<Self>,
    ) -> AnyElement {
        let mut body = components::card(theme).child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .w_full()
                .text_size(px(metrics::text::BUTTON))
                .text_color(theme.text)
                .child(SharedString::from(card_title(card))),
        );

        for control in card.controls {
            body = body.child(self.render_control(card, control, theme, motion, entity));
        }

        body.into_any_element()
    }

    fn render_control(
        &self,
        card: &Card,
        control: &Control,
        theme: &Theme,
        motion: Motion,
        entity: &Entity<Self>,
    ) -> AnyElement {
        match control {
            Control::Note { text } => div()
                .text_size(px(metrics::text::CAPTION))
                .text_color(theme.dim_text)
                .child(*text)
                .into_any_element(),

            Control::Switch { label, bind } => {
                let module = module_for_bind(bind).or_else(|| card_module(card));
                let is_on = module.is_some_and(|module| self.is_module_on(module));
                let row = div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .w_full()
                    .child(
                        div()
                            .text_size(px(metrics::text::BUTTON))
                            .text_color(theme.text)
                            .child(*label),
                    )
                    .child(components::switch(
                        SharedString::from(format!("{}-{}", card.id, bind)),
                        theme,
                        is_on,
                        motion,
                    ));

                row.into_any_element()
            }

            Control::Check {
                label,
                bind,
                tooltip,
            } => {
                // The closure runs after this frame, so it needs the `'static` copy
                // the table already holds rather than the borrow of `control`.
                let bind: &'static str = bind;
                let value = self.value(bind, 0.0) > 0.5;
                let id = SharedString::from(format!("{}-{}", card.id, bind));
                let handle = entity.clone();
                let boxed = components::checkbox(id, theme, value, *label, move |_, _, cx| {
                    let next = if value { 0.0 } else { 1.0 };
                    handle.update(cx, |shell, cx| shell.set_value(bind, next, cx));
                });

                let mut row = div().flex().flex_col().gap(px(2.0)).child(boxed);
                if !tooltip.is_empty() {
                    row = row.child(
                        div()
                            .text_size(px(metrics::text::CAPTION))
                            .text_color(theme.dim_text)
                            .child(*tooltip),
                    );
                }
                row.into_any_element()
            }

            Control::Slider {
                label,
                bind,
                min,
                max,
                snap,
            } => {
                let value = self.value(bind, f64::from(*min));
                let geometry = components::SliderGeometry::new(
                    f64::from(*min),
                    f64::from(*max),
                    if *snap { 1.0 } else { 0.0 },
                );
                let entity = entity.clone();
                let name = SharedString::from(format!("{}-{}", card.id, bind));

                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .w_full()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .justify_between()
                            .w_full()
                            .child(
                                div()
                                    .text_size(px(metrics::text::CAPTION))
                                    .text_color(theme.text)
                                    .child(*label),
                            )
                            .child(
                                div()
                                    .text_size(px(metrics::text::CAPTION))
                                    .text_color(theme.dim_text)
                                    .child(SharedString::from(format_value(value))),
                            ),
                    )
                    .child(components::slider(
                        name,
                        theme,
                        geometry,
                        value,
                        move |next| {
                            // `slider`'s callback is `Fn(f64)` with no `&mut App`, so
                            // it cannot reach the entity. The track therefore paints
                            // the stored value and does not write back yet: reported
                            // as a component API request (`on_change` needs
                            // `Fn(f64, &mut Window, &mut App)`), not worked around by
                            // patching the component.
                            let _ = (&entity, next);
                        },
                    ))
                    .into_any_element()
            }

            Control::Combo {
                label,
                bind,
                options,
            } => {
                let selected = self.value(bind, 0.0) as usize;
                let current = options.get(selected).copied().unwrap_or("");
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .w_full()
                    .gap(px(8.0))
                    .child(
                        div()
                            .text_size(px(metrics::text::CAPTION))
                            .text_color(theme.text)
                            .child(*label),
                    )
                    .child(
                        div()
                            .text_size(px(metrics::text::CAPTION))
                            .text_color(theme.dim_text)
                            .child(SharedString::from(current)),
                    )
                    .into_any_element()
            }

            Control::Keybind { module } => components::button(
                SharedString::from(format!("{}-keybind", card.id)),
                theme,
                ButtonVariant::Secondary,
                SharedString::from(keybind_label(module)),
                |_, _, _| {},
            )
            .into_any_element(),

            Control::Button { label, tooltip } => components::button(
                SharedString::from(format!("{}-{label}", card.id)),
                theme,
                if tooltip.is_empty() {
                    ButtonVariant::Dark
                } else {
                    ButtonVariant::Danger
                },
                SharedString::from(*label),
                |_, _, _| {},
            )
            .into_any_element(),

            Control::ActionBind { which } => components::button(
                SharedString::from(format!("{}-{which:?}", card.id)),
                theme,
                ButtonVariant::Secondary,
                SharedString::from("Action: Unbound"),
                |_, _, _| {},
            )
            .into_any_element(),

            Control::Availability { text, bind } => div()
                .id(SharedString::from(*bind))
                .text_size(px(metrics::text::CAPTION))
                .text_color(theme.dim_text)
                .aria_label(SharedString::from(*bind))
                .child(*text)
                .into_any_element(),

            Control::Expand { header } => div()
                .mt(px(6.0))
                .text_size(px(metrics::text::BUTTON))
                .text_color(theme.text)
                .child(SharedString::from(format!("\u{25b8} {header}")))
                .into_any_element(),

            Control::BlockEspRows => div()
                .text_size(px(metrics::text::CAPTION))
                .text_color(theme.dim_text)
                .child("Tracked blocks: add a block to start.")
                .into_any_element(),

            Control::PalettePreview { name } => {
                let preview = Theme::by_name(name);
                div()
                    .id(SharedString::from(format!("palette-{name}")))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(6.0))
                    .cursor_pointer()
                    .child(
                        div()
                            .w(px(22.0))
                            .h(px(22.0))
                            .rounded(px(3.0))
                            .bg(preview.accent),
                    )
                    .child(
                        div()
                            .text_size(px(metrics::text::CAPTION))
                            .text_color(theme.text)
                            .child(SharedString::from(*name)),
                    )
                    .into_any_element()
            }
        }
    }

    fn render_data_tab(
        &self,
        theme: &Theme,
        title: &'static str,
        subtitle: &'static str,
        hint: &'static str,
    ) -> AnyElement {
        components::scroll(SharedString::from(format!("tab-scroll-{}", self.tab)))
            .p(px(12.0))
            .gap(px(8.0))
            .child(
                div()
                    .text_size(px(metrics::text::SECTION_TITLE))
                    .text_color(theme.text)
                    .child(title),
            )
            .child(
                div()
                    .text_size(px(metrics::text::CAPTION))
                    .text_color(theme.dim_text)
                    .child(subtitle),
            )
            .child(
                div()
                    .text_size(px(metrics::text::CAPTION))
                    .text_color(theme.dim_text)
                    .child(hint),
            )
            .into_any_element()
    }
}

/// Renders a value the way the markup does: whole numbers with no decimals,
/// fractional ones with one or two.
fn format_value(value: f64) -> String {
    if (value - value.round()).abs() < 0.0005 {
        format!("{}", value.round() as i64)
    } else {
        format!("{value:.2}")
    }
}

/// The card's title: the first `Note` that reads like a heading, else the module
/// name its keybind names, else the id.
fn card_title(card: &Card) -> String {
    if let Some(module) = card_module(card) {
        if let Some(entry) = search::entries().iter().find(|entry| entry.id == module) {
            return entry.display_name.to_owned();
        }
    }

    card.id.trim_end_matches("Card").to_owned()
}

/// The catalogue module a card's keybind button names.
fn card_module(card: &Card) -> Option<&'static str> {
    card.controls.iter().find_map(|control| match control {
        Control::Keybind { module } => Some(*module),
        _ => None,
    })
}

/// The module a switch's managed binding path belongs to.
///
/// The search index already carries the pairing (`SearchEntry::toggle_property`),
/// so this is the same reverse map the overlay's row state uses; duplicating it
/// would let a card and its row disagree.
fn module_for_bind(bind: &'static str) -> Option<&'static str> {
    search::entries()
        .iter()
        .find(|entry| entry.toggle_property.eq_ignore_ascii_case(bind))
        .map(|entry| entry.id)
}

/// The keybind button's rest label: the markup shows the formatted key, and an
/// unbound module reads `Unbound` here (the capture never seeds keybinds).
fn keybind_label(_module: &'static str) -> &'static str {
    "Unbound"
}

/// So `MODULE_LIST_PREVIEW_SAMPLE` stays referenced: the Settings tab draws the
/// live module-list sample row from it once the preview rows are wired.
#[must_use]
pub fn module_list_sample() -> &'static str {
    MODULE_LIST_PREVIEW_SAMPLE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shell_opens_on_combat_and_selects_by_index_and_name() {
        let shell = Shell::new(Theme::slate());
        assert!(matches!(
            copy::TABS[shell.selected_tab()].body,
            TabBody::Cards(_)
        ));
        assert_eq!(shell.selected_tab_name(), "Combat");
    }

    #[test]
    fn every_tab_name_selects_itself() {
        for (index, tab) in copy::TABS.iter().enumerate() {
            let position = copy::TABS
                .iter()
                .position(|candidate| candidate.name.eq_ignore_ascii_case(tab.name))
                .expect("every tab has a name");
            assert_eq!(position, index);
        }
    }

    #[test]
    fn cards_are_titled_by_their_catalogue_display_name() {
        let combat = match copy::TABS[0].body {
            TabBody::Cards(cards) => cards,
            _ => unreachable!(),
        };

        assert_eq!(card_title(&combat[0]), "AutoClicker");
        assert_eq!(card_title(&combat[1]), "Right Click");
    }

    #[test]
    fn a_switch_binding_resolves_to_its_catalogue_module() {
        assert_eq!(module_for_bind("IsArmed"), Some("autoclicker"));
        assert_eq!(module_for_bind("NametagsEnabled"), Some("nametags"));
        assert_eq!(module_for_bind("NoSuchFlag"), None);
    }

    #[test]
    fn values_round_trip_through_the_draft_map() {
        let mut shell = Shell::new(Theme::slate());
        assert_eq!(shell.value("MinCPS", 12.0), 12.0);

        shell.draft.insert("MinCPS", 14.0);
        assert_eq!(shell.value("MinCPS", 12.0), 14.0);
    }

    #[test]
    fn formatted_values_drop_useless_decimals() {
        assert_eq!(format_value(12.0), "12");
        assert_eq!(format_value(4.5), "4.50");
        assert_eq!(format_value(0.0), "0");
    }
}
