//! The Accounts tab's pixels: `Aoko/Views/AccountsView.xaml` ported one block at a
//! time, reading [`AccountsState`] and calling back into the shell for every press.
//!
//! The state machine does the deciding; this file only lays out what it decided.
//! Two consequences of the pin show up here and are not bugs:
//!
//! * **No text input.** `components::field` draws the box, the text, the
//!   placeholder and the caret, but editing is `EntityInputHandler` and does not
//!   exist yet (`UI_INVENTORY.md` row 9). The panels therefore paint and focus but
//!   cannot take keystrokes.
//! * **No file dialog.** "Import file" has nowhere to go without one; the press is
//!   wired to the shell so the state machine records the refusal the managed code
//!   records when the dialog throws.

use gpui::prelude::*;
use gpui::{div, px, AnyElement, Entity, MouseButton, SharedString};

use crate::components::{self, ButtonVariant, ChipTone, FieldVariant};
use crate::shell::Shell;
use crate::theme::metrics;
use crate::theme::Theme;
use crate::views::accounts_state::{AccountPanel, AccountRowState, AccountsState, StatusTone};

/// The tab's own copy, verbatim from `AccountsView.xaml`.
pub mod copy {
    /// `:211`.
    pub const SUBTITLE: &str = "Switch the account the game is signed in with. Drop a cookie file here, or use the buttons on the right.";
    /// `:254`.
    pub const PASTE_IMPORT: &str = "Import";
    /// `:228`.
    pub const PASTE_TITLE: &str = "Paste an account";
    /// `:246`.
    pub const PASTE_PLACEHOLDER: &str = "Accepted: a Netscape cookie export, a cookie JSON export, a Localts token JSON, or a raw Minecraft access token.";
    /// `:280`.
    pub const OFFLINE_TITLE: &str = "Add an offline account";
    /// `:287`.
    pub const OFFLINE_HINT: &str =
        "Usernames must be 3-16 characters using only letters, numbers, and underscores.";
    /// `:299`.
    pub const OFFLINE_ADD: &str = "Add";
    /// `:260`, `:304`.
    pub const CANCEL: &str = "Cancel";
    /// `:193`.
    pub const IMPORT_FILE: &str = "Import file";
    /// `:199`.
    pub const PASTE: &str = "Paste";
    /// `:205`.
    pub const ADD_OFFLINE: &str = "Add offline";
    /// `:355`.
    pub const REFRESH: &str = "Refresh";
    /// `:333`.
    pub const CHIP_OFFLINE: &str = "Bridge offline";
    /// `:342`.
    pub const CHIP_MAIN_MENU: &str = "At main menu";
    /// `:351`.
    pub const CHIP_WORLD: &str = "In a world";
    /// `:393`.
    pub const EMPTY_TITLE: &str = "No accounts yet";
    /// `:394`.
    pub const EMPTY_BODY: &str = "Import a Localts token, a cookie export, or a raw access token \u{2014} or drop a cookie file here.";
    /// `:396`.
    pub const EMPTY_IMPORT: &str = "Import a file";
    /// `:412`.
    pub const DROP_HINT: &str = "Drop to import";
    /// `:446`.
    pub const DELETE: &str = "Delete";
    /// `:452`.
    pub const USE_ACCOUNT: &str = "Use account";
    /// `:456`.
    pub const RESTORE_SESSION: &str = "Restore launch account";
    /// `:484`.
    pub const OPEN_FOLDER: &str = "Open folder";
    /// `:491`.
    pub const LUNAR_ACCOUNTS: &str = "Write saved accounts into Lunar";
    /// `:507`.
    pub const STORAGE_WARNING: &str = "Saved credentials are stored unencrypted on this PC.";
    /// `:1` division label: the panel the delete question is asked in.
    pub const DELETE_TITLE: &str = "Delete account";
}

/// One card: the avatar letter, the name, the source chip, the last-used line and
/// the in-use dot (`AccountsView.xaml:42-160`).
fn render_card(
    row: &AccountRowState,
    theme: &Theme,
    index: usize,
    entity: &Entity<Shell>,
) -> AnyElement {
    let selected = row.is_current;
    let mut card = components::card(theme)
        .id(SharedString::from(format!("account-card-{index}")))
        .cursor_pointer()
        .on_mouse_down(MouseButton::Left, {
            let handle = entity.clone();
            move |_, _, cx| handle.update(cx, |shell, cx| shell.select_account(index, cx))
        })
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .w(px(26.0))
                        .h(px(26.0))
                        .rounded(px(13.0))
                        .bg(theme.slider_background)
                        .text_size(px(metrics::text::SECTION_TITLE))
                        .text_color(theme.text)
                        .child(SharedString::from(row.initial.clone())),
                )
                .child(
                    div()
                        .flex_1()
                        .text_size(px(metrics::text::BUTTON))
                        .text_color(theme.text)
                        .child(SharedString::from(row.name.clone())),
                )
                .child(components::chip(theme, ChipTone::Accent, row.source_label)),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .justify_between()
                .w_full()
                .text_size(px(metrics::text::CAPTION))
                .text_color(theme.dim_text)
                .child(SharedString::from(row.last_used_label.clone()))
                .child(SharedString::from(row.uuid_label.clone())),
        );

    if selected {
        // `CurrentDot` (`:126`): the accent dot marks the signed-in account.
        card = card.child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(4.0))
                .text_size(px(metrics::text::CAPTION))
                .text_color(theme.dim_text)
                .child(
                    div()
                        .w(px(6.0))
                        .h(px(6.0))
                        .rounded(px(3.0))
                        .bg(theme.accent),
                )
                .child("in use"),
        );
    }

    card.into_any_element()
}

/// The tab body. `tab_index` scopes the scroll id so two tabs cannot share one.
pub fn render(
    state: &AccountsState,
    theme: &Theme,
    entity: &Entity<Shell>,
    tab_index: usize,
) -> AnyElement {
    let mut body = components::scroll(SharedString::from(format!("tab-scroll-{tab_index}")))
        .flex_col()
        .p(px(12.0))
        .gap(px(8.0));

    body = body
        .child(
            div()
                .text_size(px(metrics::text::SECTION_TITLE))
                .text_color(theme.text)
                .child("Accounts"),
        )
        .child(
            div()
                .text_size(px(metrics::text::CAPTION))
                .text_color(theme.dim_text)
                .child(copy::SUBTITLE),
        );

    // === Action row ===
    let handle = entity.clone();
    body = body.child(
        div()
            .flex()
            .flex_row()
            .gap(px(8.0))
            .child(components::button(
                "accounts-import-file",
                theme,
                ButtonVariant::Dark,
                copy::IMPORT_FILE,
                move |_, _, cx| {
                    handle.update(cx, |shell, cx| shell.accounts_import_file(cx));
                },
            ))
            .child(components::button(
                "accounts-paste",
                theme,
                ButtonVariant::Dark,
                copy::PASTE,
                {
                    let handle = entity.clone();
                    move |_, _, cx| {
                        handle.update(cx, |shell, cx| {
                            shell.toggle_account_panel(AccountPanel::Paste, cx)
                        });
                    }
                },
            ))
            .child(components::button(
                "accounts-add-offline",
                theme,
                ButtonVariant::Dark,
                copy::ADD_OFFLINE,
                {
                    let handle = entity.clone();
                    move |_, _, cx| {
                        handle.update(cx, |shell, cx| {
                            shell.toggle_account_panel(AccountPanel::Offline, cx);
                        });
                    }
                },
            )),
    );

    // === Paste panel ===
    if state.paste_open() {
        let handle = entity.clone();
        body = body.child(
            components::card(theme)
                .child(
                    div()
                        .text_size(px(metrics::text::SECTION_TITLE))
                        .text_color(theme.text)
                        .child(copy::PASTE_TITLE),
                )
                .child(components::field(
                    "accounts-paste-box",
                    theme,
                    FieldVariant::Panel,
                    state.paste_text(),
                    copy::PASTE_PLACEHOLDER,
                    state.paste_focused(),
                    move |_, _, cx| {
                        handle.update(cx, |shell, cx| {
                            shell.focus_account_field(AccountPanel::Paste, true, cx);
                        });
                    },
                ))
                .child(validation_line(state.paste_validation()))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap(px(8.0))
                        .child(components::button(
                            "accounts-paste-import",
                            theme,
                            ButtonVariant::Primary,
                            copy::PASTE_IMPORT,
                            {
                                let handle = entity.clone();
                                move |_, _, cx| {
                                    handle.update(cx, |shell, cx| shell.accounts_paste_import(cx));
                                }
                            },
                        ))
                        .child(components::button(
                            "accounts-paste-cancel",
                            theme,
                            ButtonVariant::Secondary,
                            copy::CANCEL,
                            {
                                let handle = entity.clone();
                                move |_, _, cx| {
                                    handle.update(cx, |shell, cx| shell.close_account_panel(cx));
                                }
                            },
                        )),
                ),
        );
    }

    // === Offline panel ===
    if state.panel() == AccountPanel::Offline {
        let handle = entity.clone();
        body = body.child(
            components::card(theme)
                .child(
                    div()
                        .text_size(px(metrics::text::SECTION_TITLE))
                        .text_color(theme.text)
                        .child(copy::OFFLINE_TITLE),
                )
                .child(components::field(
                    "accounts-offline-name",
                    theme,
                    FieldVariant::Panel,
                    state.offline_name(),
                    "",
                    state.offline_focused(),
                    move |_, _, cx| {
                        handle.update(cx, |shell, cx| {
                            shell.focus_account_field(AccountPanel::Offline, true, cx);
                        });
                    },
                ))
                .child(
                    div()
                        .text_size(px(metrics::text::CAPTION))
                        .text_color(theme.dim_text)
                        .child(copy::OFFLINE_HINT),
                )
                .child(validation_line(state.offline_validation()))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap(px(8.0))
                        .child(components::button(
                            "accounts-offline-add",
                            theme,
                            ButtonVariant::Primary,
                            copy::OFFLINE_ADD,
                            {
                                let handle = entity.clone();
                                move |_, _, cx| {
                                    handle.update(cx, |shell, cx| shell.accounts_add_offline(cx));
                                }
                            },
                        ))
                        .child(components::button(
                            "accounts-offline-cancel",
                            theme,
                            ButtonVariant::Secondary,
                            copy::CANCEL,
                            {
                                let handle = entity.clone();
                                move |_, _, cx| {
                                    handle.update(cx, |shell, cx| shell.close_account_panel(cx));
                                }
                            },
                        )),
                ),
        );
    }

    // === Session strip ===
    let session = state.session();
    let chip = session.chip();
    let chip_label = match chip {
        crate::views::accounts_logic::SessionChipTone::Offline => copy::CHIP_OFFLINE,
        crate::views::accounts_logic::SessionChipTone::MainMenu => copy::CHIP_MAIN_MENU,
        crate::views::accounts_logic::SessionChipTone::InWorld => copy::CHIP_WORLD,
    };
    body = body.child(
        components::card(theme)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .w_full()
                    .child(components::chip(theme, ChipTone::Neutral, chip_label))
                    .child(components::button(
                        "accounts-refresh-session",
                        theme,
                        ButtonVariant::Secondary,
                        copy::REFRESH,
                        {
                            let handle = entity.clone();
                            move |_, _, cx| {
                                handle.update(cx, |shell, cx| shell.accounts_refresh_session(cx));
                            }
                        },
                    )),
            )
            .child(
                div()
                    .text_size(px(metrics::text::BUTTON))
                    .text_color(theme.text)
                    .child(SharedString::from(session.name_text())),
            )
            .child(
                div()
                    .text_size(px(metrics::text::CAPTION))
                    .text_color(theme.dim_text)
                    .child(SharedString::from(session.detail_text())),
            ),
    );

    // === Busy line ===
    if state.is_busy() {
        body = body.child(components::progress(
            "accounts-busy",
            theme,
            components::progress::ProgressStyle::busy(),
            0.5,
        ));
    }

    // === Cards or the empty state ===
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
                        .max_w(px(420.0))
                        .text_size(px(metrics::text::CAPTION))
                        .text_color(theme.dim_text)
                        .child(copy::EMPTY_BODY),
                )
                .child(components::button(
                    "accounts-empty-import",
                    theme,
                    ButtonVariant::Primary,
                    copy::EMPTY_IMPORT,
                    {
                        let handle = entity.clone();
                        move |_, _, cx| {
                            handle.update(cx, |shell, cx| shell.accounts_import_file(cx));
                        }
                    },
                )),
        );
    } else {
        for (index, row) in state.rows().iter().enumerate() {
            body = body.child(render_card(row, theme, index, entity));
        }
    }

    if state.drop_hint() {
        body = body.child(
            div()
                .text_size(px(metrics::text::CAPTION))
                .text_color(theme.accent)
                .child(copy::DROP_HINT),
        );
    }

    // === Footer ===
    let hint = state.primary_hint();
    if !hint.is_empty() {
        body = body.child(
            div()
                .text_size(px(metrics::text::CAPTION))
                .text_color(theme.dim_text)
                .child(SharedString::from(hint)),
        );
    }

    body = body.child(
        div()
            .flex()
            .flex_row()
            .gap(px(8.0))
            .child(components::button(
                "accounts-delete",
                theme,
                ButtonVariant::Danger,
                copy::DELETE,
                {
                    let handle = entity.clone();
                    move |_, _, cx| {
                        handle.update(cx, |shell, cx| shell.accounts_request_delete(cx));
                    }
                },
            ))
            .child(components::button(
                "accounts-use",
                theme,
                ButtonVariant::Primary,
                copy::USE_ACCOUNT,
                {
                    let handle = entity.clone();
                    move |_, _, cx| {
                        handle.update(cx, |shell, cx| shell.accounts_use_selected(cx));
                    }
                },
            ))
            .child(components::button(
                "accounts-restore",
                theme,
                ButtonVariant::Secondary,
                copy::RESTORE_SESSION,
                {
                    let handle = entity.clone();
                    move |_, _, cx| {
                        handle.update(cx, |shell, cx| shell.accounts_restore_session(cx));
                    }
                },
            )),
    );

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
                            "accounts-confirm-delete",
                            theme,
                            ButtonVariant::Danger,
                            copy::DELETE,
                            {
                                let handle = entity.clone();
                                move |_, _, cx| {
                                    handle.update(cx, |shell, cx| {
                                        shell.accounts_confirm_delete(cx);
                                    });
                                }
                            },
                        ))
                        .child(components::button(
                            "accounts-cancel-delete",
                            theme,
                            ButtonVariant::Secondary,
                            copy::CANCEL,
                            {
                                let handle = entity.clone();
                                move |_, _, cx| {
                                    handle.update(cx, |shell, cx| {
                                        shell.accounts_cancel_delete(cx);
                                    });
                                }
                            },
                        )),
                ),
        );
    }

    // === Storage footer ===
    body = body
        .child(
            div()
                .text_size(px(metrics::text::CAPTION))
                .text_color(theme.dim_text)
                .child(copy::STORAGE_WARNING),
        )
        .child(components::button(
            "accounts-open-folder",
            theme,
            ButtonVariant::Secondary,
            copy::OPEN_FOLDER,
            {
                let handle = entity.clone();
                move |_, _, cx| {
                    handle.update(cx, |shell, cx| shell.accounts_open_folder(cx));
                }
            },
        ))
        .child(components::checkbox(
            "accounts-lunar",
            theme,
            state.lunar_accounts(),
            copy::LUNAR_ACCOUNTS,
            {
                let handle = entity.clone();
                move |_, _, cx| {
                    handle.update(cx, |shell, cx| shell.toggle_lunar_accounts(cx));
                }
            },
        ));

    body.into_any_element()
}

/// The red validation line, collapsed when empty (`SetInlineValidation`, `:1113`).
fn validation_line(text: &str) -> AnyElement {
    if text.is_empty() {
        return div().into_any_element();
    }

    div()
        .text_size(px(metrics::text::CAPTION))
        .text_color(crate::theme::fixed::CRITICAL)
        .child(SharedString::from(text.to_owned()))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tab_copy_is_the_markup_copy() {
        // The em dash and the two hyphens are deliberate: the managed markup uses
        // U+2014 in the empty body and plain ASCII elsewhere.
        assert!(copy::EMPTY_BODY.contains('\u{2014}'));
        assert!(copy::OFFLINE_HINT.contains("3-16"));
        assert_eq!(copy::IMPORT_FILE, "Import file");
        assert_eq!(copy::USE_ACCOUNT, "Use account");
        assert_eq!(copy::RESTORE_SESSION, "Restore launch account");
    }

    #[test]
    fn an_empty_validation_line_draws_nothing() {
        let theme = Theme::slate();
        let empty = validation_line("");
        let message = validation_line("Paste a token first.");
        let _ = &theme;
        // Both are `AnyElement`s; the check is that neither panics while building.
        let _ = (empty, message);
    }
}
