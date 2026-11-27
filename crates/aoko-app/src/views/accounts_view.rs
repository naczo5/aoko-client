//! The Accounts tab's pixels: `Aoko/Views/AccountsView.xaml` ported one block at a
//! time, reading [`AccountsState`] and calling back into the shell for every press.
//!
//! The state machine does the deciding; this file only lays out what it decided.
//! Two consequences of the pin show up here and are not bugs:
//!
//! * **The text fields are real.** `components::text_input` is attached to the paste
//!   box and the offline-name box, so both take keystrokes; the widget's own limits
//!   (no selection, no IME composition underline, no click-to-caret) are documented
//!   on [`crate::components::text_input`].
//! * **No file dialog.** "Import file" has nowhere to go without one; the press is
//!   wired to the shell so the state machine records the refusal the managed code
//!   records when the dialog throws.

use gpui::prelude::*;
use gpui::{div, px, AnyElement, Entity, MouseButton, SharedString};

use crate::components::{self, ButtonVariant, ChipTone};
use crate::shell::Shell;
use crate::theme::metrics;
use crate::theme::Theme;
use crate::views::accounts_state::{AccountPanel, AccountRowState, AccountsState, StatusTone};
use crate::views::forms::FormFields;

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
    /// The delete confirmation's caption, `AccountsView.xaml.cs:899-900`.
    ///
    /// From the code-behind, not the markup: managed asked the question in a `MessageBox` whose
    /// title argument is this string, so there is no XAML line to cite. The port draws the same
    /// question in a form of its own and keeps the caption managed would have shown.
    pub const DELETE_TITLE: &str = "Delete account";

    // The markup's tooltips, one per control the tab paints. They are `ToolTip=` attributes in
    // `AccountsView.xaml`, not note rows, so each rides its control's wrapper rather than taking a
    // line of the card.

    /// `:197`, the Import file button — the sentence says what the picker accepts.
    pub const IMPORT_FILE_HINT: &str = "Choose one or more Localts token or cookie export files";
    /// `:203`, the Paste button.
    pub const PASTE_HINT: &str = "Paste a token or cookie export";
    /// `:209`, the Add offline button.
    pub const ADD_OFFLINE_HINT: &str = "Save a cracked username";
    /// `:258`, the paste panel's Import button.
    pub const PASTE_IMPORT_HINT: &str = "Import the pasted token or cookie export";
    /// `:265` and `:309`, the paste panel's and the offline panel's Cancel buttons — the markup
    /// repeats the sentence for both, so this is one constant with two cites.
    pub const CANCEL_HINT: &str = "Close the panel and keep what you typed";
    /// `:302`, the offline panel's Add button.
    pub const OFFLINE_ADD_HINT: &str = "Save this username as an offline account";
    /// `:360`, the session Refresh button.
    pub const REFRESH_HINT: &str =
        "Read the account signed in to the game. Needs the client injected and the main menu.";
    /// `:488`, the Open folder button.
    pub const OPEN_FOLDER_HINT: &str = "Open the folder that holds accounts.json";
    /// `:500`, the Lunar checkbox — two sentences in the markup, which is why it reads long.
    pub const LUNAR_HINT: &str = "Writes every saved Microsoft account into Lunar's accounts.json after an import or switch. Offline accounts are skipped, and Lunar must be relaunched to read the file.";
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
        .id(crate::reuse::indexed("account-card", index))
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
    fields: &FormFields,
    theme: &Theme,
    entity: &Entity<Shell>,
    tab_index: usize,
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
            .child(button_with_hint(
                "accounts-import-file",
                theme,
                ButtonVariant::Dark,
                copy::IMPORT_FILE,
                copy::IMPORT_FILE_HINT,
                move |_, _, cx| {
                    handle.update(cx, |shell, cx| shell.accounts_import_file(cx));
                },
            ))
            .child(button_with_hint(
                "accounts-paste",
                theme,
                ButtonVariant::Dark,
                copy::PASTE,
                copy::PASTE_HINT,
                {
                    let handle = entity.clone();
                    move |_, _, cx| {
                        handle.update(cx, |shell, cx| {
                            shell.toggle_account_panel(AccountPanel::Paste, cx)
                        });
                    }
                },
            ))
            .child(button_with_hint(
                "accounts-add-offline",
                theme,
                ButtonVariant::Dark,
                copy::ADD_OFFLINE,
                copy::ADD_OFFLINE_HINT,
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
        body = body.child(
            components::card(theme)
                .child(
                    div()
                        .text_size(px(metrics::text::SECTION_TITLE))
                        .text_color(theme.text)
                        .child(copy::PASTE_TITLE),
                )
                .child(div().w_full().child(FormFields::element(&fields.token)))
                .child(validation_line(state.paste_validation()))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap(px(8.0))
                        .child(button_with_hint(
                            "accounts-paste-import",
                            theme,
                            ButtonVariant::Primary,
                            copy::PASTE_IMPORT,
                            copy::PASTE_IMPORT_HINT,
                            {
                                let handle = entity.clone();
                                move |_, _, cx| {
                                    handle.update(cx, |shell, cx| shell.accounts_paste_import(cx));
                                }
                            },
                        ))
                        .child(button_with_hint(
                            "accounts-paste-cancel",
                            theme,
                            ButtonVariant::Secondary,
                            copy::CANCEL,
                            copy::CANCEL_HINT,
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
        body = body.child(
            components::card(theme)
                .child(
                    div()
                        .text_size(px(metrics::text::SECTION_TITLE))
                        .text_color(theme.text)
                        .child(copy::OFFLINE_TITLE),
                )
                .child(
                    div()
                        .w_full()
                        .child(FormFields::element(&fields.offline_name)),
                )
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
                        .child(button_with_hint(
                            "accounts-offline-add",
                            theme,
                            ButtonVariant::Primary,
                            copy::OFFLINE_ADD,
                            copy::OFFLINE_ADD_HINT,
                            {
                                let handle = entity.clone();
                                move |_, _, cx| {
                                    handle.update(cx, |shell, cx| shell.accounts_add_offline(cx));
                                }
                            },
                        ))
                        .child(button_with_hint(
                            "accounts-offline-cancel",
                            theme,
                            ButtonVariant::Secondary,
                            copy::CANCEL,
                            copy::CANCEL_HINT,
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
                    .child(button_with_hint(
                        "accounts-refresh-session",
                        theme,
                        ButtonVariant::Secondary,
                        copy::REFRESH,
                        copy::REFRESH_HINT,
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
        .child(button_with_hint(
            "accounts-open-folder",
            theme,
            ButtonVariant::Secondary,
            copy::OPEN_FOLDER,
            copy::OPEN_FOLDER_HINT,
            {
                let handle = entity.clone();
                move |_, _, cx| {
                    handle.update(cx, |shell, cx| shell.accounts_open_folder(cx));
                }
            },
        ))
        .child(
            // The Lunar toggle's tooltip rides a wrapper for the same reason the buttons' do:
            // `components::checkbox` takes no tooltip and this round owns no component.
            div()
                .id(crate::reuse::stitched("accounts-lunar", "-hint", ""))
                .tooltip(components::tooltip(theme, copy::LUNAR_HINT))
                .child(components::checkbox(
                    "accounts-lunar",
                    theme,
                    state.lunar_accounts(),
                    copy::LUNAR_ACCOUNTS,
                    None,
                    {
                        let handle = entity.clone();
                        move |_, _, cx| {
                            handle.update(cx, |shell, cx| shell.toggle_lunar_accounts(cx));
                        }
                    },
                )),
        );

    body.into_any_element()
}

/// The validation line's text, or `None` when there is nothing to paint.
///
/// Split out of [`validation_line`] so the absence is a value a test can assert. The line is an
/// `AnyElement`, and an element with no children is not something a test can tell from an
/// element with a child; the decision has to be a function that answers, not a branch inside a
/// painter.
fn validation_copy(text: &str) -> Option<&str> {
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

/// One button with the markup's tooltip on a wrapper around it.
///
/// `components::button` takes no tooltip and this round owns no component, so the sentence rides an
/// id'd wrapper — the shape `components::block_esp` gives its colour swatch and `views::stats_view`
/// gives a histogram bar. The wrapper's id is the button's plus `-hint`, because two elements
/// cannot share one id and the button keeps the name its tests and selectors use.
fn button_with_hint(
    id: &'static str,
    theme: &Theme,
    variant: ButtonVariant,
    label: &'static str,
    hint: &'static str,
    on_click: impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> AnyElement {
    div()
        .id(crate::reuse::stitched(id, "-hint", ""))
        .tooltip(components::tooltip(theme, hint))
        .child(components::button(id, theme, variant, label, on_click))
        .into_any_element()
}

/// The red validation line, collapsed when empty (`SetInlineValidation`, `:1113`).
fn validation_line(text: &str) -> AnyElement {
    let Some(text) = validation_copy(text) else {
        return div().into_any_element();
    };

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
        // The property in the name, as a value: an empty line has nothing to paint, and any
        // other line is painted as itself. A banner rendered for `""` fails the first
        // assertion; a painter that dropped a real message fails the second.
        assert_eq!(
            validation_copy(""),
            None,
            "an empty line must paint nothing"
        );
        assert_eq!(
            validation_copy("Paste a token first."),
            Some("Paste a token first."),
            "a message must reach the painter unchanged"
        );
    }
}
