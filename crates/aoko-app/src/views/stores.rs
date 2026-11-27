//! The seam between the two data tabs' state machines and whatever owns their
//! persistence.
//!
//! `aoko-core` has no account or server store yet: the managed side had
//! `AccountStore` and `ServerStore` reading `accounts.json` and `servers.json`
//! under `%AppData%\Aoko`, and no Rust port exists in `aoko-core` or
//! `aoko-platform`. Rather than reach into those crates from `aoko-app`, the tabs
//! take this trait: the state machines own every decision, and a store only has to
//! answer six questions.
//!
//! [`NoStore`] is the honest default until a real one lands: it reports no rows and
//! refuses every write, so a press that needs a store reaches the managed failure
//! line (`That account was already removed.`) instead of pretending to succeed.

use aoko_protocol::accounts::StoredAccount;

use crate::views::servers_logic::ServerRowSource;

/// Everything the Accounts and Servers tabs need from persistence.
///
/// # These answers are the write, which is why they are not queued
///
/// [`crate::views::file_store`] runs a queued write on a background thread and reports it through
/// the submitter's mailbox; that is what the config and palette saves use. This seam cannot: each
/// method below answers with the write's own outcome — the saved row, or `None`/`false` — and both
/// tabs paint that answer as their failure line. An implementation that queued the bytes would
/// have to answer before the rename, which is exactly the success this port removes, or answer
/// `None` and move the failure out of the tab's reach.
///
/// So [`FileStore`](crate::views::file_store::FileStore) writes on the calling thread, and the two
/// tabs keep the managed rule: what they paint is what the file holds. Making these two files
/// asynchronous needs a seam that hands a ticket back and a tab that polls it, which is the tabs'
/// half of the code — a signature here alone cannot do it.
pub trait StoreBridge {
    /// Every saved account, in store order.
    fn accounts(&self) -> Vec<StoredAccount>;
    /// Saves a new offline account and returns it, or `None` when the write fails.
    fn add_offline_account(&mut self, name: &str, uuid: &str) -> Option<StoredAccount>;
    /// Removes one account; `false` when it was already gone.
    fn remove_account(&mut self, id: &str) -> bool;

    /// Every saved server, unsorted: the tab sorts for display.
    fn servers(&self) -> Vec<ServerRowSource>;
    /// Saves a server; `name` is `None` when the user left it blank.
    fn add_server(&mut self, name: Option<&str>, address: &str) -> Option<ServerRowSource>;
    /// Removes one server; `false` when it was already gone.
    fn remove_server(&mut self, id: &str) -> bool;
}

/// A bridge with nothing behind it.
///
/// Every read is empty and every write fails, which is exactly what the app has
/// while `aoko-core` has no store: the tabs still open, still select, still arm
/// their buttons, and a write reports the managed failure line.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoStore;

impl StoreBridge for NoStore {
    fn accounts(&self) -> Vec<StoredAccount> {
        Vec::new()
    }

    fn add_offline_account(&mut self, _name: &str, _uuid: &str) -> Option<StoredAccount> {
        None
    }

    fn remove_account(&mut self, _id: &str) -> bool {
        false
    }

    fn servers(&self) -> Vec<ServerRowSource> {
        Vec::new()
    }

    fn add_server(&mut self, _name: Option<&str>, _address: &str) -> Option<ServerRowSource> {
        None
    }

    fn remove_server(&mut self, _id: &str) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_absent_store_reports_nothing_and_refuses_writes() {
        let mut store = NoStore;

        assert!(store.accounts().is_empty());
        assert!(store.servers().is_empty());
        assert!(store.add_offline_account("Notch", "uuid").is_none());
        assert!(!store.remove_account("a"));
        assert!(store.add_server(None, "mc.example.net").is_none());
        assert!(!store.remove_server("s"));
    }
}
