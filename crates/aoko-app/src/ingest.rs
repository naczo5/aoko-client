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
//! # The config frame
//!
//! The link is not read-only any more: on connect, and whenever the controller's frame
//! changes, the reader writes the managed `config` frame. The frame is the controller's
//! (`AppController::config_frame`), both halves of it: `fightStatus` and every `keybind*` come
//! from the profile, and the layout half is `Profile::hud_layout` parsed into a `HudLayout`, so
//! a HUD the user moved is the HUD that ships. `ConfigFrame::to_line` is the protocol's encoder
//! — this crate assembles neither half.
//!
//! The compare is on the frame, so moving one HUD element is a change even though the payload
//! is identical, and it happens through a read guard the controller lends rather than a clone.
//! That guard is dropped before the socket write, which is not stylistic: holding a read lock
//! on the frame cache across a stalled write would block every profile write on the UI thread
//! behind it.
//!
//! **One writer, and it is the reader.** `Transport` gives a single `&mut self` for both
//! directions, so a second thread would have to share the transport behind a lock — and the
//! socket's read blocks with no timeout (`aoko-platform/src/tcp.rs:87`), which would starve
//! that lock for as long as the bridge is quiet. Instead the read loop syncs before every
//! read: the transport is owned by one thread, a write can never land inside a read, and a
//! torn frame is not something a lock has to prevent. The cost is latency, not correctness —
//! a change goes out on the next inbound frame, and the bridge streams state at tens of
//! hertz.
//!
//! The managed host runs a separate `ConfigSenderLoop` with a 2 s heartbeat
//! (`GameStateClient.cs:1265-1345`). That shape needs a writer that can make progress while a
//! read is blocked, which this transport cannot offer; a heartbeat here could only piggyback
//! on traffic that already arrived, so this slice sends on change only. [`crate::ingest`]'s
//! report names what a real sender would need.
//!
//! # The capability frame
//!
//! A `capabilities` frame is the one message besides state that changes something: the reader
//! copies the decoder's table into the cell `Runtime::module_supported` reads, which is what the
//! command-search overlay's `supported` column shows. It is published once on entry as well, so a
//! window opened before the handshake asks the codec's question rather than reading an empty list.
//!
//! What this deliberately does not do: no reconnect, no command frames, no injection and no
//! module execution. A failed dial leaves the link down, and the reason is published into the
//! runtime's `LinkReport` as well as logged, because the loader panel is what a user can act on
//! and `connected` alone cannot say why it is false. Retry, backoff and socket teardown belong to
//! a later slice. The reader thread is detached when `crate::ingest::Ingest` is dropped: a
//! blocking `recv_line` on a live socket cannot be interrupted without closing it, and closing it
//! is not this slice's job.

use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use aoko_core::app::{AppController, ConfigFrame};
use aoko_platform::TcpBridgeTransport;
use aoko_protocol::capabilities::BridgeCapabilities;
use aoko_protocol::codec::{Decoder, Inbound, Pump, TransportClient};
use aoko_protocol::diagnostics::ManagedTransportDiagnostics;
use aoko_protocol::transport::{Transport, TransportError};

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

/// Why the link is not up, in the two shapes the loader panel names.
///
/// `IngestStatus::connected` says *whether* the link is up; this says why it is not, which is what
/// a user can act on. The managed host carried the same two facts in one `StatusMessage` channel —
/// `"ERROR: Could not connect to agent on port " + _port` (`GameStateClient.cs:493`) when the dial
/// failed, `"Disconnected from agent."` (`:615`) when a live link went down — and this is the split
/// that lets the panel paint them as states instead of as one mutating string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkFailure {
    /// The dial never reached the bridge, so nothing was ever injected into.
    ///
    /// The port is carried rather than looked up by the painter: it is the endpoint the dial
    /// used, and the sentence that names it is a template.
    ConnectFailed {
        /// The bridge endpoint the reader dialled, [`aoko_protocol::BRIDGE_PORT`].
        port: u16,
    },
    /// The link was up and went down, which is the transition `connected` alone cannot express.
    Disconnected,
}

/// Where the reader publishes why the link went down, one attempt at a time.
///
/// The reader is the only writer, because it is the thread that dials and the thread the link
/// closes on. The runtime holds the handle, clears it when a new attempt starts, and reads it to
/// answer [`crate::runtime::Runtime::link_failure`] — so the panel's reason is the reader's report
/// rather than a second guess about the same socket.
///
/// `Mutex` rather than `RwLock`: one writer, one reader, and the value is a `Copy` enum, so the
/// lock is held for a load and a store and never across a paint.
#[derive(Debug, Clone, Default)]
pub struct LinkReport(Arc<Mutex<Option<LinkFailure>>>);

impl LinkReport {
    /// An empty report: no attempt has answered yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records why the link is down. Called by the reader thread only.
    pub fn publish(&self, failure: LinkFailure) {
        if let Ok(mut cell) = self.0.lock() {
            *cell = Some(failure);
        }
    }

    /// Forgets the last answer, which is what a new dial does before it starts.
    pub fn clear(&self) {
        if let Ok(mut cell) = self.0.lock() {
            *cell = None;
        }
    }

    /// The last answer, `None` while an attempt is still unanswered.
    #[must_use]
    pub fn read(&self) -> Option<LinkFailure> {
        self.0.lock().ok().and_then(|cell| *cell)
    }
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
    ///
    /// A test build returns a reader with no worker instead of dialling. That is the rule this
    /// module's header states — no test opens a socket, because a bridge genuinely listening on the
    /// machine running the suite would flip the controller's `connected` under an assertion — and
    /// it lives here rather than behind a `cfg` on the caller so the shipped path is one function
    /// rather than two. The press that reaches it still proves itself by the counter in
    /// `Shell::dial`; a test that needs a live link uses [`Ingest::attach`] over a memory duplex.
    #[must_use]
    pub fn spawn(
        controller: Arc<AppController>,
        advertised: Arc<RwLock<BridgeCapabilities>>,
        report: LinkReport,
    ) -> Self {
        if cfg!(test) {
            return Self { worker: None };
        }

        Self::start("aoko-bridge-dial", move || {
            match TcpBridgeTransport::connect_bridge() {
                Ok(transport) => {
                    tracing::info!("bridge connected on the loopback port");
                    pump(client(transport), &controller, &advertised, &report);
                    tracing::info!("bridge link closed");
                }
                Err(error) => {
                    // Not fatal: the window is usable without a bridge. The reason is published
                    // as well as logged, because the loader panel is what a user can act on and
                    // `connected` alone cannot say why it is false.
                    tracing::warn!("no bridge on the loopback port: {error}");
                    report.publish(LinkFailure::ConnectFailed {
                        port: aoko_protocol::BRIDGE_PORT,
                    });
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
    pub fn attach<T: Transport + 'static>(
        transport: T,
        controller: Arc<AppController>,
        advertised: Arc<RwLock<BridgeCapabilities>>,
        report: LinkReport,
    ) -> Self {
        Self::start("aoko-bridge-read", move || {
            pump(client(transport), &controller, &advertised, &report);
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

/// The read loop: publish every state frame until the peer closes or the read fails, and
/// keep the link's `config` frame in step with the controller.
///
/// Connected is set on entry and cleared on every exit path, so a closed link, a read
/// error and a panicking frame handler all leave the store saying "disconnected" rather
/// than "connected, last seen a while ago".
///
/// The exit also publishes [`LinkFailure::Disconnected`], and it does so **before** the store flips:
/// a reader that was connected and is now going away is a drop, not a dial that never answered, and
/// the panel's next frame has to see the reason before it sees `connected == false`. Reversed, a
/// frame landing between the two writes would read an unanswered-looking attempt.
///
/// `advertised` is the cell [`crate::runtime::Runtime::module_supported`] reads. This thread
/// is its only writer, because it is the thread that receives the capability frame.
fn pump<T: Transport>(
    mut client: TransportClient<T>,
    controller: &AppController,
    advertised: &RwLock<BridgeCapabilities>,
    report: &LinkReport,
) {
    let store = controller.store();
    store.set_connected(true);

    // What the link has been told. It lives on this thread, which is the only writer: see the
    // module docs. The frame carries the HUD layout as well as the payload, because a layout
    // the controller hands back is the one the user moved.
    let mut sent: Option<ConfigFrame> = None;

    // The codec's own table, published once on entry: a window that opens before the handshake
    // asks the same question the decoder would answer, rather than reading an empty list.
    if let Ok(mut cell) = advertised.write() {
        *cell = client.decoder().capabilities().clone();
    }

    loop {
        // Before every read, so a config write can never land inside one. A profile edit made
        // on the UI thread is picked up here on the next iteration; while nothing changes
        // this is one guard on the controller's cached frame and a compare, not an encode.
        if let Err(error) = sync_config(&mut client, controller, &mut sent) {
            tracing::warn!("bridge config write failed: {error}");
            break;
        }

        match client.pump() {
            Ok(Pump::Frame(Inbound::State { state, .. })) => {
                // `publish_shared`, not `publish`: the frame already arrives as an `Arc`
                // (`codec.rs:51-54`), and handing it over unchanged is the zero-copy path
                // `d8e23a4` added. `publish(*state, ..)` would be a move out of the `Arc`.
                store.publish_shared(state, Instant::now());
            }
            // Command results are decoded and dropped; the command request queue is a later
            // slice. A capability frame is not: the codec has already applied it to its own
            // table, so the shared cell is one clone away from being current, and the overlay's
            // `supported` column reads it through `Runtime::module_supported`.
            Ok(Pump::Frame(Inbound::Capabilities(_))) => {
                if let Ok(mut cell) = advertised.write() {
                    *cell = client.decoder().capabilities().clone();
                }
            }
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

    // The reason first, then the flag: see this function's doc for why the order is a
    // correctness requirement rather than style.
    report.publish(LinkFailure::Disconnected);
    store.set_connected(false);
}

/// Writes the `config` frame when the controller's frame differs from the one on the wire.
///
/// The frame is the controller's (`AppController::config_frame`), both halves of it: the
/// payload is the profile's `fightStatus` and every `keybind*`, and the layout is
/// `Profile::hud_layout` parsed into a `HudLayout`. Nothing here assembles either half, which
/// also closes the gap this function used to have — it encoded a customised HUD against
/// `HudLayout::default()` because nothing converted the profile's blob.
///
/// The compare is on the `ConfigFrame` value rather than the encoded line: `PartialEq` spans
/// both halves, so a HUD-only edit is a change and an idle tick costs one guard on the
/// controller's cached frame plus a compare, where encoding costs a ~4 KB `String`. The clone
/// happens only on the send path, so the compare path stays allocation-free.
///
/// The guard is dropped before `send_line`, deliberately. It is a read lock on the controller's
/// frame cache, and holding it across a stalled socket write would block every profile write on
/// the UI thread behind it: a UI freeze caused by a config send. The ordering is a correctness
/// requirement, not style.
fn sync_config<T: Transport>(
    client: &mut TransportClient<T>,
    controller: &AppController,
    sent: &mut Option<ConfigFrame>,
) -> Result<(), TransportError> {
    let frame = controller.config_frame();
    if sent.as_ref().is_some_and(|sent| sent == &*frame) {
        return Ok(());
    }

    let line = frame.to_line();
    let keep = (*frame).clone();
    drop(frame);
    client.send_line(&line)?;
    tracing::debug!(bytes = line.len(), "bridge config frame sent");
    *sent = Some(keep);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aoko_protocol::state::GameState;
    use aoko_protocol::transport::{memory, LineBuf, RecvOutcome, TransportError};
    use std::sync::atomic::{AtomicBool, Ordering};
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

        let report = LinkReport::new();
        let ingest = Ingest::attach(
            HeldTransport {
                inner: transport,
                gate: Arc::clone(&gate),
            },
            Arc::clone(&controller),
            advertised_cell(),
            report.clone(),
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
            report.read(),
            Some(LinkFailure::Disconnected),
            "and the reader published why, which is the half `connected` cannot say"
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

    // ---- the config frame ------------------------------------------------------------

    /// A link the test drives: the memory transport for the wire, plus the one property it
    /// lacks — a read that waits instead of reporting end of stream.
    ///
    /// `MemoryTransport` answers `EndOfStream` as soon as its queue is empty, which a socket
    /// never does, so a reader on it would leave at the first quiet moment and no later frame
    /// could be observed. The wait is a 1 ms poll on a flag: a real socket blocks in the
    /// kernel, and nothing asserted below depends on how long the fake waits.
    struct Driven {
        inner: memory::MemoryTransport,
        closed: Arc<AtomicBool>,
    }

    impl Transport for Driven {
        fn recv_line(&mut self, out: &mut LineBuf) -> Result<RecvOutcome, TransportError> {
            loop {
                match self.inner.recv_line(out)? {
                    RecvOutcome::EndOfStream if !self.closed.load(Ordering::SeqCst) => {
                        thread::sleep(Duration::from_millis(1));
                    }
                    other => return Ok(other),
                }
            }
        }

        fn send_line(&mut self, line: &[u8]) -> Result<(), TransportError> {
            self.inner.send_line(line)
        }
    }

    /// A reader on a [`Driven`] link, plus the peer that feeds it and records what it writes.
    fn connect(controller: &Arc<AppController>) -> (memory::MemoryPeer, Arc<AtomicBool>, Ingest) {
        let (peer, closed, ingest, _advertised) = connect_with_cell(controller);
        (peer, closed, ingest)
    }

    /// The same reader, handing back the advertised cell it fills.
    ///
    /// The cell is not reachable from `Ingest` — it belongs to the caller that passed it in —
    /// so a test that wants to read what the reader wrote has to keep its own handle.
    fn connect_with_cell(
        controller: &Arc<AppController>,
    ) -> (
        memory::MemoryPeer,
        Arc<AtomicBool>,
        Ingest,
        Arc<RwLock<BridgeCapabilities>>,
    ) {
        let closed = Arc::new(AtomicBool::new(false));
        let (transport, peer) = memory::duplex();
        let advertised = advertised_cell();
        let ingest = Ingest::attach(
            Driven {
                inner: transport,
                closed: Arc::clone(&closed),
            },
            Arc::clone(controller),
            Arc::clone(&advertised),
            LinkReport::new(),
        );
        (peer, closed, ingest, advertised)
    }

    /// The advertised-module cell a reader fills, seeded the way `Runtime` seeds its own.
    fn advertised_cell() -> Arc<RwLock<BridgeCapabilities>> {
        Arc::new(RwLock::new(BridgeCapabilities::for_version_fallback(None)))
    }

    /// Waits until the reader has written a cell the predicate accepts.
    ///
    /// The reader is on its own thread, so this is a poll with a deadline rather than a fixed
    /// sleep: when it returns, the frame has been taken and the cell is what the next read of
    /// `Runtime::module_supported` sees.
    fn wait_for_advertised(
        advertised: &Arc<RwLock<BridgeCapabilities>>,
        predicate: impl Fn(&BridgeCapabilities) -> bool,
    ) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if advertised.read().is_ok_and(|cell| predicate(&cell)) {
                return;
            }
            if Instant::now() >= deadline {
                panic!("the reader never published the capability frame");
            }
            thread::sleep(Duration::from_millis(2));
        }
    }

    /// Waits until the client has written `count` lines, then hands back everything it wrote.
    fn wait_for_lines(peer: &memory::MemoryPeer, count: usize) -> Vec<String> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let lines = peer.sent_lines();
            if lines.len() >= count {
                return lines;
            }
            if Instant::now() >= deadline {
                panic!(
                    "the client wrote {} lines, expected at least {count}: {lines:?}",
                    lines.len()
                );
            }
            thread::sleep(Duration::from_millis(2));
        }
    }

    /// One written line as the bridge would read it.
    fn json(line: &str) -> serde_json::Value {
        serde_json::from_str(line).unwrap_or_else(|error| panic!("{line}: {error}"))
    }

    /// Feeds one state frame and waits until the reader published it.
    ///
    /// The wait is what makes a tick a fact rather than a timing assumption: when this
    /// returns, the reader has taken the frame, so its next config sync is the one that runs
    /// before the *next* read.
    fn tick(controller: &AppController, peer: &memory::MemoryPeer, health: f32) {
        peer.feed(
            format!("{{\"screenName\":\"Arena\",\"health\":{health},\"inWorld\":true}}\n")
                .as_bytes(),
        );
        wait_for_state(controller, |state| state.health == health);
    }

    /// On connect the client writes exactly one `config` frame, and it is the controller's:
    /// `fightStatus` and `keybindAutoclicker` are the profile's values, not the payload's
    /// defaults.
    #[test]
    fn connecting_writes_the_controllers_config_frame_once() {
        let controller = Arc::new(AppController::default());
        controller.update_profile(|profile| {
            profile.fight_status_enabled = true;
            profile
                .module_keys
                .insert(String::from("autoclicker"), 0x70);
            true
        });

        let (peer, closed, ingest) = connect(&controller);

        let lines = wait_for_lines(&peer, 1);
        assert_eq!(lines.len(), 1, "one frame on connect: {lines:?}");

        let frame = json(&lines[0]);
        assert_eq!(frame["type"].as_str(), Some("config"));
        assert_eq!(
            frame["fightStatus"].as_bool(),
            Some(true),
            "`fightStatus` is `Profile::fight_status_enabled`; `false` would be a default payload"
        );
        assert_eq!(
            frame["keybindAutoclicker"].as_i64(),
            Some(0x70),
            "and every keybind comes from `Profile::module_keys`"
        );
        assert!(
            frame["hudLayout"].is_object(),
            "the layout field is always present in the managed frame"
        );

        closed.store(true, Ordering::SeqCst);
        ingest.join();
    }

    /// A profile change reaches the wire on the live link: no reconnect, no second reader.
    #[test]
    fn a_fight_status_change_is_written_on_the_live_link() {
        let controller = Arc::new(AppController::default());
        let (peer, closed, ingest) = connect(&controller);

        let lines = wait_for_lines(&peer, 1);
        assert_eq!(
            json(&lines[0])["fightStatus"].as_bool(),
            Some(false),
            "the default profile is off, and that is what connect sent"
        );

        // The flip is what a switch press does, from another thread.
        controller.update_profile(|profile| {
            profile.fight_status_enabled = true;
            true
        });

        // One inbound frame is the tick the reader syncs on. The bridge streams these at tens
        // of hertz, so the change goes out within a frame rather than at the next connect.
        tick(&controller, &peer, 20.0);

        let lines = wait_for_lines(&peer, 2);
        assert_eq!(
            json(&lines[1])["fightStatus"].as_bool(),
            Some(true),
            "the new value, on the same link"
        );
        assert!(
            controller.store().is_connected(),
            "the link never dropped, so this is not a reconnect"
        );

        closed.store(true, Ordering::SeqCst);
        ingest.join();
    }

    /// An unchanged tick writes nothing: the compare is on the payload, so idle frames leave
    /// the wire at two lines rather than resending the same one.
    #[test]
    fn an_unchanged_tick_writes_no_third_frame() {
        let controller = Arc::new(AppController::default());
        let (peer, closed, ingest) = connect(&controller);
        wait_for_lines(&peer, 1);

        // One change, so there are two frames on the wire.
        controller.update_profile(|profile| {
            profile.fight_status_enabled = true;
            true
        });
        tick(&controller, &peer, 20.0);
        tick(&controller, &peer, 19.0);
        wait_for_lines(&peer, 2);

        // Two more ticks. Each one syncs before its read and finds the frame it already
        // sent; by the time `tick` returns, that sync has run.
        tick(&controller, &peer, 18.0);
        tick(&controller, &peer, 17.0);

        assert_eq!(
            peer.sent_lines().len(),
            2,
            "the connect frame and the one change, and nothing on an idle tick"
        );

        closed.store(true, Ordering::SeqCst);
        ingest.join();
    }

    /// A HUD-only edit is written, which a payload-only compare cannot see.
    ///
    /// The layout is the frame's other half: `Profile::hud_layout` is in no field of
    /// `ConfigPayload`, so a sender comparing payloads sends nothing when a user moves one HUD
    /// element and the customised layout never reaches the bridge. The payload is asserted
    /// unchanged on the second frame, so the send can only be the layout's.
    #[test]
    fn a_hud_only_edit_is_written() {
        use aoko_protocol::hud::{element_id, HudLayout};
        use aoko_protocol::profile::Profile;

        let controller = Arc::new(AppController::default());
        let (peer, closed, ingest) = connect(&controller);
        wait_for_lines(&peer, 1);

        // One element moved, stored the way the profile stores it (`Profile::hud_layout`).
        let mut stored = Profile::hud_layout_map(&HudLayout::new());
        let moved = stored
            .get_mut(element_id::MODULE_LIST)
            .expect("the layout map carries every known element");
        moved.x = 0.42;
        controller.update_profile(|profile| {
            profile.hud_layout = Some(stored);
            true
        });

        tick(&controller, &peer, 20.0);
        let lines = wait_for_lines(&peer, 2);

        let frame = json(&lines[1]);
        assert_eq!(frame["type"].as_str(), Some("config"));
        assert_eq!(
            frame["fightStatus"].as_bool(),
            Some(false),
            "the payload half is what connect already sent, so the payload did not change"
        );
        assert_eq!(
            frame["hudLayout"][element_id::MODULE_LIST]["x"].as_f64(),
            Some(0.42),
            "the layout half carries the element the user moved"
        );

        closed.store(true, Ordering::SeqCst);
        ingest.join();
    }

    /// The advertised cell follows the bridge, and it is the reader that writes it.
    ///
    /// `Runtime::module_supported` is what the overlay's `supported` column reads, so once a
    /// `capabilities` frame arrives the cell has to be that frame. It names one module and
    /// leaves the other three sets empty, which this also pins: `from_payload` inherits an
    /// empty set from the fallback, so the settings, state fields and actions survive while the
    /// module list is replaced outright rather than merged into
    /// (`capabilities.rs:613-633`).
    ///
    /// The probe module is `probe-only-module` rather than a real one, and that is the whole
    /// point of this test. The legacy fallback already advertises 21 modules including
    /// `autoclicker` and `killaura` (`capabilities.rs:275-295`), so waiting on either of those
    /// returned the moment the reader published its *fallback* — the wait passed before the frame
    /// was even decoded, and the "replaced, not merged" assertion then failed against a cell the
    /// frame had never touched. A name in neither fallback is what makes the wait wait.
    #[test]
    fn a_capability_frame_replaces_the_advertised_cell() {
        let controller = Arc::new(AppController::default());
        let (peer, _closed, _ingest, advertised) = connect_with_cell(&controller);

        const PROBE: &str = "probe-only-module";
        peer.feed(b"{\"type\":\"capabilities\",\"modules\":[\"probe-only-module\"]}\n");

        wait_for_advertised(&advertised, |capabilities| {
            capabilities.supports_module(PROBE)
        });
        let capabilities = advertised.read().expect("cell").clone();
        assert!(
            capabilities.supports_module(PROBE),
            "the module the frame names is advertised"
        );
        assert!(
            !capabilities.supports_module("autoclicker"),
            "and the codec's fallback module list is replaced, not merged into"
        );
        assert!(
            capabilities.setting_count() > 0,
            "the sets the frame left empty came from the fallback"
        );
    }
}
