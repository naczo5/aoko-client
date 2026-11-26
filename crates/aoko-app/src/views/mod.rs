//! Copy and logic shared by the tabs and the shell.
//!
//! The split inside this module is deliberate:
//!
//! * [`tab_copy`], [`relative_time`], [`store_recovery`], [`accounts_logic`] and
//!   [`servers_logic`] are direct ports of the `Aoko/Views/*.cs` helpers. Their
//!   wording is asserted by the managed tests, so a paraphrase is a parity bug,
//!   not a style choice.
//! * [`command_search`] is the state behind the overlay and
//!   [`accounts_state`]/[`servers_state`] are the state machines behind the two
//!   data tabs. They exist because GPUI has no data binding: where WPF repainted a
//!   bound target, these produce the value the tab paints and say whether it
//!   changed.
//! * [`copy`] holds the card, tab and settings copy lifted verbatim out of
//!   `MainWindow.xaml`, which is where 478 of the app's strings live.

pub mod accounts_logic;
pub mod command_search;
pub mod relative_time;
pub mod servers_logic;
pub mod store_recovery;
pub mod tab_copy;
