//! The bridge ingest: one reader thread that turns wire lines into game state.
//!
//! Until this module, nothing connected the app to the bridge.
//! `aoko_platform::TcpBridgeTransport::connect_bridge` had no caller and
//! `aoko_core::app::StateStore::publish` was only reached from tests, so the window
//! rendered a shell over a state nothing ever received. `crate::ingest::Ingest` is the
//! reader: a background thread that owns an optional connection, decodes each line
//! through `aoko_protocol::codec::Decoder`, and publishes the decoded `GameState` into the
//! controller's store.
//!
//! Two entry points, and the split is the test seam:
//!
//! * `crate::ingest::Ingest::spawn` dials the constant loopback endpoint **on the worker
//!   thread**, so a bridge that is not running costs a warning line instead of blocking
//!   the window's first frame.
//! * `crate::ingest::Ingest::attach` takes a transport that is already connected. Every
//!   test uses it with `aoko_protocol::transport::memory::duplex`, which is why no test
//!   opens a socket.
//!
//! # Reading through `TransportClient`
//!
//! `aoko_protocol::codec::TransportClient` owns the transport, the decoder and the line
//! buffer, and its `pump` clears the buffer before every read (`codec.rs:250-256`), which
//! the `Transport` contract requires because `recv_line` appends. That clear was missing
//! until `4f50b00`; this module carried a fifteen-line reader while it was absent and now
//! uses the client directly. `memory::MemoryTransport` appends as well, so the seam tests
//! exercise the same contract the socket does.
//!
//! What this deliberately does not do: no reconnect, no command sending, no injection and
//! no module execution. A failed dial leaves the link down with the reason in the log, and
//! the connection lifecycle (retry, backoff, teardown) belongs to a later slice. The
//! reader thread is detached when `crate::ingest::Ingest` is dropped: a blocking
//! `recv_line` on a live socket cannot be interrupted without closing it, and closing it
//! is not this slice's job.

use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use aoko_core::app::AppController;
use aoko_platform::TcpBridgeTransport;
use aoko_protocol::codec::{Decoder, Inbound, Pump, TransportClient};
use aoko_protocol::diagnostics::ManagedTransportDiagnostics;
use aoko_protocol::transport::Transport;

/// Ticks per second the transport diagnostics are counted against.
///
/// The managed host measures its read loop against the same rate. The counters are off
/// unless `AOKO_PERF_DIAGNOSTICS` is set; the object is required by
/// `aoko_protocol::codec::TransportClient`.
const DIAGNOSTICS_TICKS_PER_SECOND: u64 = 20;

/// A decoder over one transport, with diagnostics taken from the environment.
///
/// `TransportClient` clears its line buffer before every read (`codec.rs:250-256`), which
/// is the half of the `Transport` contract `4f50b00` restored; the reader is that client
/// and nothing of our own.
fn client<T: Transport>(transport: T) -> TransportClient<T> {
    TransportClient::new(
        transport,
        Decoder::new(),
        ManagedTransportDiagnostics::from_environment(DIAGNOSTICS_TICKS_PER_SECOND, 0),
    )
}

/// What the bridge link currently reports.
///
/// Read from the controller's store, so it is the same truth every consumer sees: no
/// second copy of "are we connected" exists in this crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IngestStatus {
    /// Whether the reader has a live connection.
    pub connected: bool,
    /// How long ago the newest state arrived, `None` before the first frame.
    pub state_age: Option<Duration>,
    /// Whether that age is inside `aoko_core::chest_stealer::FRESH_STATE_MS`.
    pub fresh: bool,
}

/// A bridge reader running on its own thread.
pub struct Ingest {
    /// `None` only when the thread could not be started; the reason is logged.
    worker: Option<JoinHandle<()>>,
}

impl Ingest {
    /// Dials the constant bridge endpoint on a worker thread and publishes what it reads.
    ///
    /// The dial happens inside the thread: a resolved-but-silent address can block for the
    /// OS SYN timeout, and the startup path must not wait for it.
    #[must_use]
    pub fn spawn(controller: Arc<AppController>) -> Self {
        Self::start("aoko-bridge-dial", move || {
            match TcpBridgeTransport::connect_bridge() {
                Ok(transport) => {
                    tracing::info!("bridge connected on the loopback port");
                    pump(client(transport), &controller);
                    tracing::info!("bridge link closed");
                }
                Err(error) => {
                    // Not fatal: the window is usable without a bridge, and the next
                    // launch tries again.
                    tracing::warn!("no bridge on the loopback port: {error}");
                }
            }
        })
    }

    /// Attaches an already-connected transport and starts the reader.
    ///
    /// This is the seam a test uses: `aoko_protocol::transport::memory::duplex` gives a
    /// connected pair without a socket, so the decode-and-publish path is exercised for
    /// real and nothing touches the network.
    #[must_use]
    pub fn attach<T: Transport + 'static>(transport: T, controller: Arc<AppController>) -> Self {
        Self::start("aoko-bridge-read", move || {
            pump(client(transport), &controller);
        })
    }

    /// Waits for the reader to finish.
    ///
    /// Only a transport that ends on its own can finish: a real socket blocks in
    /// `recv_line` until the peer closes. Tests call this to prove the disconnect path.
    pub fn join(mut self) {
        if let Some(worker) = self.worker.take() {
            if worker.join().is_err() {
                tracing::warn!("the bridge reader thread panicked");
            }
        }
    }

    /// Starts the worker, logging rather than panicking when the OS refuses a thread.
    fn start<F>(name: &str, body: F) -> Self
    where
        F: FnOnce() + Send + 'static,
    {
        match thread::Builder::new().name(name.to_owned()).spawn(body) {
            Ok(worker) => Self {
                worker: Some(worker),
            },
            Err(error) => {
                tracing::warn!("cannot start the bridge reader thread: {error}");
                Self { worker: None }
            }
        }
    }
}

/// The read loop: publish every state frame until the peer closes or the read fails.
///
/// Connected is set on entry and cleared on every exit path, so a closed link, a read
/// error and a panicking frame handler all leave the store saying "disconnected" rather
/// than "connected, last seen a while ago".
fn pump<T: Transport>(mut client: TransportClient<T>, controller: &AppController) {
    let store = controller.store();
    store.set_connected(true);

    loop {
        match client.pump() {
            Ok(Pump::Frame(Inbound::State { state, .. })) => {
                // `publish_shared`, not `publish`: the frame already arrives as an `Arc`
                // (`codec.rs:51-54`), and handing it over unchanged is the zero-copy path
                // `d8e23a4` added. `publish(*state, ..)` would be a move out of the `Arc`.
                store.publish_shared(state, Instant::now());
            }
            // Capabilities and command results are decoded but not consumed yet: the
            // command request queue and the capability-driven UI are later slices.
            Ok(Pump::Frame(_)) => {}
            Ok(Pump::TooLong) => {
                tracing::warn!("bridge line exceeded the 1 MiB inbound cap; discarded");
            }
            Ok(Pump::Closed) => break,
            Err(error) => {
                tracing::warn!("bridge read failed: {error}");
                break;
            }
        }
    }

    store.set_connected(false);
}

#[cfg(test)]
mod tests {
    use super::*;
    use aoko_protocol::state::GameState;
    use aoko_protocol::transport::{memory, LineBuf, RecvOutcome, TransportError};
    use std::sync::{Condvar, Mutex};

    /// A gate the test opens when it is done watching the link.
    ///
    /// `memory::MemoryTransport` reports end of stream as soon as its queue is empty,
    /// which would race every assertion about "connected": the reader could finish before
    /// the test looks. This holds the reader at end of stream until the test releases it,
    /// so "connected while the peer is there" and "disconnected after the peer closes"
    /// are both facts rather than timing.
    struct Gate {
        open: Mutex<bool>,
        signal: Condvar,
    }

    impl Gate {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                open: Mutex::new(false),
                signal: Condvar::new(),
            })
        }

        fn release(&self) {
            if let Ok(mut open) = self.open.lock() {
                *open = true;
            }
            self.signal.notify_all();
        }

        /// Blocks until `release`, then reports end of stream exactly once.
        fn wait_for_release(&self) {
            let Ok(mut open) = self.open.lock() else {
                return;
            };
            while !*open {
                let Ok(next) = self.signal.wait(open) else {
                    return;
                };
                open = next;
            }
        }
    }

    /// A memory transport whose end of stream waits for the gate.
    ///
    /// The only thing it adds is the gate; reading, framing and the appending contract are
    /// the fake's own, so this test goes through the same `TransportClient` a socket does.
    struct HeldTransport {
        inner: memory::MemoryTransport,
        gate: Arc<Gate>,
    }

    impl Transport for HeldTransport {
        fn recv_line(&mut self, out: &mut LineBuf) -> Result<RecvOutcome, TransportError> {
            let outcome = self.inner.recv_line(out)?;
            if outcome == RecvOutcome::EndOfStream {
                self.gate.wait_for_release();
            }
            Ok(outcome)
        }

        fn send_line(&mut self, line: &[u8]) -> Result<(), TransportError> {
            self.inner.send_line(line)
        }
    }

    /// Waits for the store to hold a state the predicate accepts, then hands it back.
    fn wait_for_state(
        controller: &AppController,
        predicate: impl Fn(&GameState) -> bool,
    ) -> Arc<GameState> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(snapshot) = controller.store().snapshot() {
                if predicate(&snapshot.state) {
                    return Arc::clone(&snapshot.state);
                }
            }
            if Instant::now() >= deadline {
                panic!("the ingest never published the expected state");
            }
            thread::sleep(Duration::from_millis(2));
        }
    }

    /// The proof this slice has to make: two lines fed by the peer arrive as one decoded
    /// state in the controller's store, and the link reads connected while it is up.
    ///
    /// The first line is a complete frame (screen name, health, world); the second is a
    /// patch carrying only health and the action bar. The final state therefore proves
    /// *both* lines were decoded and applied: dropping either one loses a field the
    /// assertions check.
    #[test]
    fn two_frames_reach_the_store_and_the_link_reads_connected() {
        let controller = Arc::new(AppController::default());
        let gate = Gate::new();
        let (transport, peer) = memory::duplex();

        peer.feed(b"{\"screenName\":\"Arena\",\"health\":20.0,\"inWorld\":true}\n");
        peer.feed(b"{\"partial\":true,\"health\":7.5,\"actionBar\":\"frame two\"}\n");

        let ingest = Ingest::attach(
            HeldTransport {
                inner: transport,
                gate: Arc::clone(&gate),
            },
            Arc::clone(&controller),
        );

        let state = wait_for_state(&controller, |state| state.health == 7.5);

        assert_eq!(
            state.screen_name, "Arena",
            "the complete frame's fields survive the patch"
        );
        assert_eq!(
            state.action_bar, "frame two",
            "the patch reached the decoder"
        );
        assert!(state.in_world, "the complete frame's flags survive");
        assert!(
            state.is_connected,
            "the decoder marks a state as arriving on a live link"
        );

        assert!(
            controller.store().is_connected(),
            "a live reader means connected"
        );
        assert!(
            controller.store().age(Instant::now()).is_some(),
            "the snapshot records when it arrived"
        );

        gate.release();
        ingest.join();

        assert!(
            !controller.store().is_connected(),
            "end of stream disconnects the link"
        );
        assert_eq!(
            controller
                .store()
                .snapshot()
                .map(|snapshot| snapshot.state.health),
            Some(7.5),
            "the last decoded state survives the disconnect"
        );
    }
}
