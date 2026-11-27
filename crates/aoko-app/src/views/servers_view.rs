//! The Servers tab's pixels: `Aoko/Views/ServersView.xaml` ported one block at a
//! time, reading [`ServersState`] and calling back into the shell for every press.
//!
//! The card is a button-like row (the managed template is a `Button`), which is why
//! the whole card takes the press and the chip takes its own.

use gpui::prelude::*;
use gpui::{div, px, AnyElement, Entity, MouseButton, SharedString};

use crate::components::{self, ButtonVariant, ChipTone};
use crate::shell::Shell;
use crate::theme::metrics;
use crate::theme::motion::Motion;
use crate::theme::Theme;
use crate::views::accounts_state::StatusTone;
use crate::views::forms::FormFields;
use crate::views::servers_logic::{ServerRowState, ServerStatusTone};
use crate::views::servers_state::ServersState;

/// The tab's own copy, verbatim from `ServersView.xaml`.
pub mod copy {
    /// `:222`.
    pub const JOIN: &str = "Join";
    /// `:336`.
    pub const ADD_A_SERVER: &str = "Add a server";
    /// `:380`.
    pub const ADD: &str = "Add";
    /// `:388`, `:392`.
    pub const CANCEL: &str = "Cancel";
    /// `:433`.
    pub const JOIN_SELECTED: &str = "Join selected";
    /// `:438`.
    pub const ADD_SERVER: &str = "Add server";
    /// `:445`.
    pub const PING_ALL: &str = "Ping all";
    /// `:451`.
    pub const DELETE: &str = "Delete";
    /// `:191`, the tab's subtitle above the direct-join row.
    pub const SUBTITLE: &str = "Join a server without leaving aoko.";
    /// `:365`, the add form's name caption.
    ///
    /// The port passes the two captions to `TextInput` as the box's hint rather than painting a
    /// caption above it, because its form has one row per field and no caption line.
    pub const NAME_LABEL: &str = "Name";
    /// `:372`, the add form's address caption.
    ///
    /// `Host or host:port`, not "Address": the markup's caption is the pair of accepted forms,
    /// and the box takes either. Folded into the box's hint for the same reason as
    /// [`NAME_LABEL`].
    pub const ADDRESS_LABEL: &str = "Host or host:port";
    /// `:368`, the name box's tooltip.
    ///
    /// Managed had a caption above the box (`:365`) and this sentence on hover; the port folds the
    /// caption into the hint (see [`NAME_LABEL`]), so the tooltip is the sentence that tells the
    /// user the label is optional and the address is what gets used. Wired in `shell.rs`'s
    /// `ensure_form_fields`, which is not this file's.
    pub const NAME_HINT: &str = "Optional label. The address is used when this is empty.";
    /// `:219`, the direct-connect box's placeholder.
    ///
    /// Distinct from the add form's caption: this one names a concrete example. Nothing paints it
    /// yet — `shell.rs`'s direct-connect field has no placeholder — so it is the one entry in this
    /// module with no reader.
    pub const DIRECT_PLACEHOLDER: &str =
        "Server address, for example play.example.net or play.example.net:25566";
    /// `:333`, the empty state's title.
    pub const EMPTY_TITLE: &str = "No saved servers";
    /// `:334`, the empty state's body.
    pub const EMPTY_BODY: &str = "Join a server above, or add one to ping and join it later.";
    /// The delete confirmation's caption, `ServersView.xaml.cs:457-458`.
    ///
    /// From the code-behind, not the markup: managed asked the question in a `MessageBox` whose
    /// title argument is this string, so there is no XAML line to cite. The port draws the same
    /// question in a form of its own and keeps the caption managed would have shown.
    pub const DELETE_TITLE: &str = "Delete server";
}

/// The chip tone for a ping state.
fn chip_tone(tone: ServerStatusTone) -> ChipTone {
    match tone {
        ServerStatusTone::Neutral => ChipTone::Neutral,
        ServerStatusTone::Accent => ChipTone::Accent,
        ServerStatusTone::Warn => ChipTone::Warn,
        ServerStatusTone::Critical => ChipTone::Critical,
    }
}

/// One saved server (`ServersView.xaml:36-300`).
fn render_row(
    row: &ServerRowState,
    theme: &Theme,
    index: usize,
    entity: &Entity<Shell>,
) -> AnyElement {
    let border = if row.is_selected {
        theme.accent
    } else {
        theme.card_border()
    };

    let mut card = components::card(theme)
        .id(crate::reuse::indexed("server-card", index))
        .border_1()
        .border_color(border)
        .cursor_pointer()
        .on_mouse_down(MouseButton::Left, {
            let handle = entity.clone();
            move |_, _, cx| handle.update(cx, |shell, cx| shell.select_server(index, cx))
        })
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .w_full()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .child(
                            div()
                                .text_size(px(metrics::text::BUTTON))
                                .text_color(theme.text)
                                .child(SharedString::from(row.name.clone())),
                        )
                        .child(
                            div()
                                .text_size(px(metrics::text::CAPTION))
                                .text_color(theme.dim_text)
                                .child(SharedString::from(row.address.clone())),
                        ),
                )
                .child(
                    // The chip is also the row's ping button (`LatencyChip_Click`).
                    //
                    // The press stops here rather than reaching the card's own listener. The
                    // markup nests a `ServerChipButton` (`ServersView.xaml:83-140`) inside the
                    // card's `Button`, and a WPF `ButtonBase` marks its mouse event handled, so
                    // the card's `ServerCard_Click` never ran for a chip press. Without this the
                    // row is selected *and* pinged, which quietly moves what "Join selected" and
                    // "Delete" would act on.
                    div()
                        .id(crate::reuse::indexed("server-chip", index))
                        .cursor_pointer()
                        .on_mouse_down(MouseButton::Left, {
                            let id = row.id.clone();
                            let handle = entity.clone();
                            move |_, _, cx| {
                                cx.stop_propagation();
                                let id = id.clone();
                                handle.update(cx, |shell, cx| shell.ping_server(id, cx));
                            }
                        })
                        .child(components::chip(
                            theme,
                            chip_tone(row.status_tone),
                            row.status_text.clone(),
                        )),
                ),
        );

    if row.has_motd() {
        card = card.child(
            div()
                .text_size(px(metrics::text::CAPTION))
                .text_color(theme.dim_text)
                .child(SharedString::from(row.motd.clone())),
        );
    }

    card = card.child(
        div()
            .flex()
            .flex_row()
            .justify_between()
            .w_full()
            .text_size(px(metrics::text::CAPTION))
            .text_color(theme.dim_text)
            .child(SharedString::from(row.last_joined_label.clone()))
            .child(SharedString::from(row.players_label.clone())),
    );

    card.into_any_element()
}

/// The tab body. `motion` is the animation gate the busy line's sweep reads
/// (`ServersView.xaml:158-164`).
pub fn render(
    state: &ServersState,
    fields: &FormFields,
    theme: &Theme,
    entity: &Entity<Shell>,
    tab_index: usize,
    motion: Motion,
) -> AnyElement {
    let mut body = components::scroll(crate::reuse::indexed("tab-scroll", tab_index))
        .flex_col()
        .p(px(12.0))
        .gap(px(8.0));

    body = body
        .child(
            div()
                .text_size(px(metrics::text::SECTION_TITLE))
                .text_color(theme.text)
                .child("Servers"),
        )
        .child(
            div()
                .text_size(px(metrics::text::CAPTION))
                .text_color(theme.dim_text)
                .child(copy::SUBTITLE),
        );

    // === Direct join row ===
    body = body.child(
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.0))
            .w_full()
            .child(
                div()
                    .flex_1()
                    .child(FormFields::element(&fields.direct_address)),
            )
            .child(button_with_hint(
                "servers-direct-join",
                theme,
                ButtonVariant::Primary,
                copy::JOIN,
                state.direct_join_tooltip(),
                {
                    let handle = entity.clone();
                    move |_, _, cx| {
                        handle.update(cx, |shell, cx| shell.join_direct(cx));
                    }
                },
            )),
    );

    if !state.direct_validation().is_empty() {
        body = body.child(
            div()
                .text_size(px(metrics::text::CAPTION))
                .text_color(crate::theme::fixed::CRITICAL)
                .child(SharedString::from(state.direct_validation().to_owned())),
        );
    }

    // === Add form ===
    if state.add_open() {
        body = body.child(
            components::card(theme)
                .child(FormFields::element(&fields.server_name))
                .child(FormFields::element(&fields.server_address))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap(px(8.0))
                        .child(components::button(
                            "servers-add-save",
                            theme,
                            ButtonVariant::Primary,
                            copy::ADD,
                            {
                                let handle = entity.clone();
                                move |_, _, cx| {
                                    handle.update(cx, |shell, cx| shell.add_server(cx));
                                }
                            },
                        ))
                        .child(components::button(
                            "servers-add-cancel",
                            theme,
                            ButtonVariant::Secondary,
                            copy::CANCEL,
                            {
                                let handle = entity.clone();
                                move |_, _, cx| {
                                    handle.update(cx, |shell, cx| shell.close_add_form(cx));
                                }
                            },
                        )),
                ),
        );

        if !state.add_validation().is_empty() {
            body = body.child(
                div()
                    .text_size(px(metrics::text::CAPTION))
                    .text_color(crate::theme::fixed::CRITICAL)
                    .child(SharedString::from(state.add_validation().to_owned())),
            );
        }
    }

    // === Rows or the empty state ===
    if state.is_empty() {
        body = body.child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(8.0))
                .py(px(24.0))
                .child(
                    div()
                        .text_size(px(metrics::text::SECTION_TITLE))
                        .text_color(theme.text)
                        .child(copy::EMPTY_TITLE),
                )
                .child(
                    div()
                        .text_size(px(metrics::text::CAPTION))
                        .text_color(theme.dim_text)
                        .child(copy::EMPTY_BODY),
                )
                .child(components::button(
                    "servers-empty-add",
                    theme,
                    ButtonVariant::Primary,
                    copy::ADD_A_SERVER,
                    {
                        let handle = entity.clone();
                        move |_, _, cx| {
                            handle.update(cx, |shell, cx| {
                                shell.open_add_form(cx);
                            });
                        }
                    },
                )),
        );
    } else {
        for (index, row) in state.rows().iter().enumerate() {
            body = body.child(render_row(row, theme, index, entity));
        }
    }

    // === Busy and status ===
    //
    // The re-templated bar at `ServersView.xaml:158-164` is `IsIndeterminate="True"` with a looping
    // 2.2 s storyboard, so the painter is the sweeping one; a determinate half-fill froze in the
    // middle and read as progress that had stopped.
    if state.is_busy() {
        body = body.child(components::progress_indeterminate(
            "servers-busy",
            theme,
            components::progress::ProgressStyle::busy(),
            motion,
        ));
    }

    if let Some(status) = state.status() {
        let colour = match status.tone {
            StatusTone::Neutral => theme.text,
            StatusTone::Success => theme.accent,
            StatusTone::Critical => crate::theme::fixed::CRITICAL,
        };
        body = body.child(
            div()
                .text_size(px(metrics::text::CAPTION))
                .text_color(colour)
                .child(SharedString::from(status.short_text())),
        );
    }

    if let Some(notice) = state.recovery_notice() {
        body = body.child(
            div()
                .text_size(px(metrics::text::CAPTION))
                .text_color(theme.dim_text)
                .child(SharedString::from(notice.to_owned())),
        );
    }

    let hint = state.primary_hint();
    if !hint.is_empty() {
        body = body.child(
            div()
                .text_size(px(metrics::text::CAPTION))
                .text_color(theme.dim_text)
                .child(SharedString::from(hint)),
        );
    }

    // === Action row ===
    body = body.child(
        div()
            .flex()
            .flex_row()
            .gap(px(8.0))
            .child(components::button(
                "servers-join-selected",
                theme,
                ButtonVariant::Primary,
                copy::JOIN_SELECTED,
                {
                    let handle = entity.clone();
                    move |_, _, cx| {
                        handle.update(cx, |shell, cx| shell.join_selected(cx));
                    }
                },
            ))
            .child(components::button(
                "servers-add",
                theme,
                ButtonVariant::Secondary,
                copy::ADD_SERVER,
                {
                    let handle = entity.clone();
                    move |_, _, cx| {
                        handle.update(cx, |shell, cx| shell.open_add_form(cx));
                    }
                },
            ))
            .child(button_with_hint(
                "servers-ping-all",
                theme,
                ButtonVariant::Secondary,
                copy::PING_ALL,
                state.ping_all_tooltip(),
                {
                    let handle = entity.clone();
                    move |_, _, cx| {
                        handle.update(cx, |shell, cx| shell.ping_all(cx));
                    }
                },
            ))
            .child(button_with_hint(
                "servers-delete",
                theme,
                ButtonVariant::Danger,
                copy::DELETE,
                state.delete_tooltip(),
                {
                    let handle = entity.clone();
                    move |_, _, cx| {
                        handle.update(cx, |shell, cx| shell.servers_request_delete(cx));
                    }
                },
            )),
    );

    // === Delete confirmation ===
    if let Some((_name, question)) = state.delete_confirmation() {
        body = body.child(
            components::card(theme)
                .border_1()
                .border_color(crate::theme::fixed::CRITICAL)
                .child(
                    div()
                        .text_size(px(metrics::text::SECTION_TITLE))
                        .text_color(theme.text)
                        .child(copy::DELETE_TITLE),
                )
                .child(
                    div()
                        .text_size(px(metrics::text::CAPTION))
                        .text_color(theme.dim_text)
                        .child(SharedString::from(question)),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap(px(8.0))
                        .child(components::button(
                            "servers-confirm-delete",
                            theme,
                            ButtonVariant::Danger,
                            copy::DELETE,
                            {
                                let handle = entity.clone();
                                move |_, _, cx| {
                                    handle.update(cx, |shell, cx| {
                                        shell.servers_confirm_delete(cx);
                                    });
                                }
                            },
                        ))
                        .child(components::button(
                            "servers-cancel-delete",
                            theme,
                            ButtonVariant::Secondary,
                            copy::CANCEL,
                            {
                                let handle = entity.clone();
                                move |_, _, cx| {
                                    handle.update(cx, |shell, cx| {
                                        shell.servers_cancel_delete(cx);
                                    });
                                }
                            },
                        )),
                ),
        );
    }

    body.into_any_element()
}

/// One button with a tooltip on a wrapper around it.
///
/// `components::button` takes no tooltip and this round owns no component, so the sentence rides
/// an id'd wrapper — the shape [`crate::views::accounts_view`] gives its buttons. The wrapper's id
/// is the button's plus `-hint`, because two elements cannot share one id and the button keeps the
/// name its selectors use.
///
/// The sentences are the state's, not literals: `DirectJoinButton.ToolTip`, `PingAllButton.ToolTip`
/// and `DeleteButton.ToolTip` are set from the capability gate and the selection in
/// `ServersView.xaml.cs:145-166`, which is the only place a user is told *why* a press will not go
/// through. The accessors were written and tested but never read, so those three sentences were
/// dead code and the buttons explained nothing.
fn button_with_hint(
    id: &'static str,
    theme: &Theme,
    variant: ButtonVariant,
    label: &'static str,
    hint: impl Into<SharedString>,
    on_click: impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> AnyElement {
    div()
        .id(crate::reuse::stitched(id, "-hint", ""))
        .tooltip(components::tooltip(theme, hint))
        .child(components::button(id, theme, variant, label, on_click))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tab_copy_is_the_markup_copy() {
        assert_eq!(copy::JOIN_SELECTED, "Join selected");
        assert_eq!(copy::PING_ALL, "Ping all");
        assert_eq!(copy::ADD_A_SERVER, "Add a server");
    }

    #[test]
    fn every_chip_tone_maps_to_the_matching_chip() {
        assert_eq!(chip_tone(ServerStatusTone::Neutral), ChipTone::Neutral);
        assert_eq!(chip_tone(ServerStatusTone::Accent), ChipTone::Accent);
        assert_eq!(chip_tone(ServerStatusTone::Warn), ChipTone::Warn);
        assert_eq!(chip_tone(ServerStatusTone::Critical), ChipTone::Critical);
    }
}
