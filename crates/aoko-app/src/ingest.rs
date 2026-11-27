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
//! What this deliberately does not do: no command frames, no injection and no module execution.
//! A dial that never reaches the bridge retries for the window `DIAL_ATTEMPTS` describes and then
//! leaves the link down, and the reason is published into the runtime's `LinkReport` as well as
//! logged, because the loader panel is what a user can act on and `connected` alone cannot say why
//! it is false.
//!
//! # The dial window
//!
//! A dial is retried inside the reader, because one attempt is not what the managed host did and
//! not what the transport allows. The bridge's listener is bound by the DLL *after* `LoadLibrary`
//! returns, so a dial issued by the press that injected it loses the race unless it is repeated:
//! `GameStateClient.ConnectAsync(maxAttempts: 30)` retried every 500 ms on the injection path
//! (`GameStateClient.cs:379-385`, `:486`) and the loader painted the wait as "Waiting for bridge
//! startup". A reader that dialled once left the app connected to nothing for the rest of the
//! session - the state store stayed empty, so every module gate that reads game state (aim assist,
//! chest stealer, the HUD layout) saw a dead game rather than a slow one. See
//! `dial_with_retries` for the numbers and `DIAL_ATTEMPTS` for the bound.
//!
//! What is still out of scope: the managed host also *re*connected after a live link dropped
//! (`ScheduleAutoReconnectAsync`, `GameStateClient.cs:626-660`) and tore its socket down on close.
//! Neither is here. The reader thread is detached when `crate::ingest::Ingest` is dropped, and a
//! blocking `recv_line` on a live socket cannot be interrupted without closing it - closing it is
//! the transport's job, not this module's. `Ingest`'s stop flag therefore ends a reader that is
//! *waiting to dial*, which is the case a replaced reader is always in.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use aoko_core::app::{AppController, ConfigFrame};
use aoko_platform::TcpBridgeTransport;
use aoko_protocol::capabilities::BridgeCapabilities;
use aoko_protocol::codec::{Decoder, Inbound, Pump, TransportClient};
use aoko_protocol::diagnostics::ManagedTransportDiagnostics;
use aoko_protocol::hud::HudLayout;
use aoko_protocol::profile::Profile;
use aoko_protocol::transport::{LineBuf, RecvOutcome, Transport, TransportError};

/// Ticks per second the transport diagnostics are counted against.
///
/// **`1_000`, i.e. one tick per millisecond**, because the ticks this module feeds the counters are
/// `Instant` elapsed milliseconds. The object converts its counters with
/// `1000.0 / ticks_per_second`, so a rate that does not match the clock it is handed scales every
/// `ms/s` column by the wrong factor; the value used to be `20`, which no clock in this crate
/// produces and which would have misreported `parse=` and `configSerializeCpu=` five-fold.
///
/// The counters are off unless `AOKO_PERF_DIAGNOSTICS` is set; the object is required by
/// `aoko_protocol::codec::TransportClient` whether or not it is enabled.
const DIAGNOSTICS_TICKS_PER_SECOND: u64 = 1_000;

/// How many dials one reader makes before it reports the failure.
///
/// `GameStateClient.ConnectAsync(maxAttempts: 30)` on the injection path
/// (`GameStateClient.cs:379-385`). Thirty attempts half a second apart is a fifteen second
/// window, which is the time the bridge needs between `LoadLibrary` returning and its listener
/// accepting: the DLL starts a thread that does its JNI setup before `bind`, and the injector's
/// `WaitForSingleObject` only waits for the module's entry point.
const DIAL_ATTEMPTS: u32 = 30;

/// Delay between dial attempts: the managed `await Task.Delay(500, token)`
/// (`GameStateClient.cs:486`).
const DIAL_RETRY: Duration = Duration::from_millis(500);

/// Dials until `dial` answers, the window closes, or `stop` is raised.
///
/// `GameStateClient.ConnectAsync`'s retry loop (`GameStateClient.cs:464-489`) with the same
/// numbers: [`DIAL_ATTEMPTS`] tries, [`DIAL_RETRY`] apart. It is a free function over a closure
/// rather than a loop inside [`Ingest::spawn`] so the retry policy is exercised without a socket
/// and without waiting: the tests drive it with a zero delay and a dial that refuses twice.
///
/// `None` means the link never came up inside the window, or `stop` was raised while waiting. The
/// caller distinguishes the two by reading `stop`, because only one of them is a failure worth
/// publishing: a reader whose handle was dropped is being *replaced*, and the reader that replaced
/// it owns the next answer to "why is the link down".
fn dial_with_retries<T>(
    mut dial: impl FnMut() -> Result<T, TransportError>,
    stop: &AtomicBool,
    attempts: u32,
    retry: Duration,
) -> Option<T> {
    for attempt in 1..=attempts {
        if stop.load(Ordering::SeqCst) {
            return None;
        }
        match dial() {
            Ok(connected) => return Some(connected),
            Err(error) => {
                // Debug, not warn: a refused dial while the bridge is still starting is the
                // expected case, and the outcome is logged by the caller. A warning per attempt
                // would put thirty lines in the shipped log for one slow bridge.
                tracing::debug!(%error, attempt = attempt, "bridge dial failed");
                if attempt < attempts {
                    thread::sleep(retry);
                }
            }
        }
    }
    None
}

/// A transport that counts the bytes it reads, for the transport diagnostics.
///
/// `TransportClient::pump` decodes one line and hands back a frame; it does not report how many
/// characters that line held, and that count is the one number
/// `ManagedTransportDiagnostics::record_inbound` records. The client keeps its line buffer private,
/// so the count has to be taken one layer down, here, where every frame already passes through.
///
/// The counter is shared (`Arc<AtomicU64>`) rather than a field because the client owns the
/// transport: the reader keeps the other handle and swaps the counter to zero after each read. One
/// thread touches both, so the atomic is about ownership, not contention.
///
/// **Bytes, not characters.** The managed host counted `line.Length`, UTF-16 units. A frame is
/// ASCII-dominant JSON, so the two agree except for a payload carrying a non-ASCII name; the
/// `chars=` column of the summary is therefore bytes on this side. Counting characters would mean
/// copying and decoding every frame a second time to make a diagnostic one percent more precise.
struct Counted<T> {
    inner: T,
    bytes_read: Arc<AtomicU64>,
}

impl<T: Transport> Transport for Counted<T> {
    fn recv_line(&mut self, out: &mut LineBuf) -> Result<RecvOutcome, TransportError> {
        // `recv_line` appends to `out`, so the frame's length is the growth.
        let before = out.len();
        let outcome = self.inner.recv_line(out)?;
        let read = out.len().saturating_sub(before);
        self.bytes_read
            .fetch_add(u64::try_from(read).unwrap_or(u64::MAX), Ordering::Relaxed);
        Ok(outcome)
    }

    fn send_line(&mut self, line: &[u8]) -> Result<(), TransportError> {
        self.inner.send_line(line)
    }
}

/// The byte counter a [`Counted`] transport fills, from the reader's side.
#[derive(Clone, Default)]
struct ByteCount(Arc<AtomicU64>);

impl ByteCount {
    /// Takes the bytes read since the last call, leaving the counter at zero.
    ///
    /// A swap rather than a load-and-subtract: the reader is the only consumer, so this cannot
    /// lose a count even if the transport gained another byte in between.
    fn take(&self) -> u64 {
        self.0.swap(0, Ordering::Relaxed)
    }
}

/// A decoder over one transport, its byte counter, and diagnostics taken from the environment.
///
/// `TransportClient` clears its line buffer before every read (`codec.rs:250-256`), which
/// is the half of the `Transport` contract `4f50b00` restored; the reader is that client
/// and nothing of our own.
fn client<T: Transport>(transport: T) -> (TransportClient<Counted<T>>, ByteCount) {
    let count = ByteCount::default();
    let counted = Counted {
        inner: transport,
        bytes_read: Arc::clone(&count.0),
    };
    let client = TransportClient::new(
        counted,
        Decoder::new(),
        ManagedTransportDiagnostics::from_environment(DIAGNOSTICS_TICKS_PER_SECOND, 0),
    );
    (client, count)
}

/// Flushes the transport window when it is due, as `LogTransportDiagnosticsIfReady` did
/// (`GameStateClient.cs:1612-1619`).
///
/// A no-op unless `AOKO_PERF_DIAGNOSTICS` is set: a disabled instance records nothing and
/// `try_take_snapshot` returns `None`, so a shipped session pays one atomic load per frame. `info!`
/// rather than `debug!` because the environment variable is itself the opt-in - a user who set it
/// wants the line with the default filter.
fn log_transport_diagnostics<T: Transport>(client: &TransportClient<T>, epoch: Instant) {
    let now = i64::try_from(epoch.elapsed().as_millis()).unwrap_or(i64::MAX);
    if let Some(window) = client.diagnostics().try_take_snapshot(now) {
        tracing::info!("{}", window.to_log_message(DIAGNOSTICS_TICKS_PER_SECOND));
    }
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
    /// Raised when this handle is dropped, so a reader that is *waiting to dial again* stops
    /// instead of racing the reader that replaced it. Shared with the worker, which reads it
    /// before every attempt and between every frame.
    stop: Arc<AtomicBool>,
}

impl Ingest {
    /// Dials the constant bridge endpoint on a worker thread and publishes what it reads.
    ///
    /// The dial happens inside the thread: a resolved-but-silent address can block for the
    /// OS SYN timeout, and the startup path must not wait for it. It is also *retried* inside the
    /// thread, up to `DIAL_ATTEMPTS` times `DIAL_RETRY` apart, because the press that injected
    /// the bridge races the bridge's own listener: see the module header and `dial_with_retries`.
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
        let stop = Arc::new(AtomicBool::new(false));
        if cfg!(test) {
            return Self { worker: None, stop };
        }

        let reading = {
            let stop = Arc::clone(&stop);
            move || {
                match dial_with_retries(
                    TcpBridgeTransport::connect_bridge,
                    stop.as_ref(),
                    DIAL_ATTEMPTS,
                    DIAL_RETRY,
                ) {
                    Some(transport) => {
                        // A reason from an earlier attempt must not outlive the link that makes it
                        // false: `Runtime::link_failure` reads this cell first, so a stale
                        // `ConnectFailed` paints over a session that is already connected.
                        report.clear();
                        tracing::info!("bridge connected on the loopback port");
                        let (reader, counts) = client(transport);
                        pump(
                            reader,
                            counts,
                            &controller,
                            &advertised,
                            &report,
                            stop.as_ref(),
                        );
                        tracing::info!("bridge link closed");
                    }
                    None => {
                        // A raised stop is this reader being replaced, not a failed dial. Publishing
                        // here would overwrite the answer of the reader that replaced it.
                        if stop.load(Ordering::SeqCst) {
                            tracing::debug!("bridge dial abandoned: the reader was replaced");
                            return;
                        }
                        // Not fatal: the window is usable without a bridge. The reason is published
                        // as well as logged, because the loader panel is what a user can act on and
                        // `connected` alone cannot say why it is false.
                        tracing::warn!(
                            "no bridge on the loopback port after {} dial attempts",
                            DIAL_ATTEMPTS
                        );
                        report.publish(LinkFailure::ConnectFailed {
                            port: aoko_protocol::BRIDGE_PORT,
                        });
                    }
                }
            }
        };

        Self::start("aoko-bridge-dial", stop, reading)
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
        let stop = Arc::new(AtomicBool::new(false));
        let reading = {
            let stop = Arc::clone(&stop);
            move || {
                let (reader, counts) = client(transport);
                pump(
                    reader,
                    counts,
                    &controller,
                    &advertised,
                    &report,
                    stop.as_ref(),
                )
            }
        };
        Self::start("aoko-bridge-read", stop, reading)
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
    fn start<F>(name: &str, stop: Arc<AtomicBool>, body: F) -> Self
    where
        F: FnOnce() + Send + 'static,
    {
        match thread::Builder::new().name(name.to_owned()).spawn(body) {
            Ok(worker) => Self {
                worker: Some(worker),
                stop,
            },
            Err(error) => {
                tracing::warn!("cannot start the bridge reader thread: {error}");
                Self { worker: None, stop }
            }
        }
    }
}

impl Drop for Ingest {
    fn drop(&mut self) {
        // The one thing this handle can still do to a blocked worker: a reader that is between
        // dial attempts reads this and returns, so replacing the reader does not leave two of them
        // dialling the same port. A reader inside `recv_line` on a live socket is out of reach -
        // interrupting it means closing the socket, which the transport owns and this type does not.
        self.stop.store(true, Ordering::SeqCst);
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
///
/// `stop` is the owning [`Ingest`]'s stop flag. It cannot interrupt a blocked `recv_line`, so it is
/// read between frames: a reader whose handle was dropped leaves at the next frame instead of
/// decoding into a store nobody watches any more. Leaving that way writes nothing - not the reason,
/// not `connected`, not the capability cell - because the reader that replaced it owns all three.
/// A quiet socket holds the thread until the peer speaks or closes, which is the transport's limit
/// and not something this loop can fix.
///
/// `counts` is the byte counter of `client`'s own transport. `TransportClient::pump` does not report
/// how many bytes the frame it decoded held, so the reader takes that number from the layer the
/// bytes passed through and feeds `ManagedTransportDiagnostics::record_inbound` - the counter that
/// previously recorded nothing and left `AOKO_PERF_DIAGNOSTICS` silent.
fn pump<T: Transport>(
    mut client: TransportClient<Counted<T>>,
    counts: ByteCount,
    controller: &AppController,
    advertised: &RwLock<BridgeCapabilities>,
    report: &LinkReport,
    stop: &AtomicBool,
) {
    let store = controller.store();
    store.set_connected(true);

    // The clock the diagnostics window is measured against: `MonitorTransportDiagnostics` was
    // constructed with `Stopwatch.GetTimestamp()`, and this is the Rust equivalent for a window
    // that only ever needs elapsed time.
    let diagnostics_epoch = Instant::now();

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
        if stop.load(Ordering::SeqCst) {
            // An abandoned reader owns nothing on the way out. Its handle was dropped because
            // another reader replaced it, so that reader owns the store and the report cell now:
            // publishing `Disconnected` or clearing `connected` here would mark a link that is up
            // as down. Returning early is the whole rule.
            tracing::debug!("bridge reader stopped: its handle was dropped");
            return;
        }

        // Before every read, so a config write can never land inside one. A profile edit made
        // on the UI thread is picked up here on the next iteration; while nothing changes
        // this is one guard on the controller's cached frame and a compare, not an encode.
        if let Err(error) = sync_config(&mut client, controller, &mut sent) {
            tracing::warn!("bridge config write failed: {error}");
            break;
        }

        // One read, timed and counted. The managed loop timed the same span
        // (`Stopwatch.GetTimestamp() - parseStarted`, `GameStateClient.cs:595-600`) and recorded it
        // for every line it consumed, including one it discarded: the `inbound=` and `parse=`
        // columns are the read loop's own cost, not the decoded subset of it.
        let started = Instant::now();
        let outcome = client.pump();
        let elapsed = i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX);
        let read = counts.take();

        match outcome {
            Ok(Pump::Frame(Inbound::State { state, hud_layout })) => {
                // An in-game HUD drag arrives as the frame's own `hudLayout` echo, which the
                // managed read loop applied before it published the state
                // (`ApplyInboundHudLayout`, `GameStateClient.cs:573-589`, `:1624-1648`). The codec
                // hands the layout out beside the state precisely so this is one `Option`, and
                // dropping it - as this arm used to - made the HUD editor's work vanish on the
                // next config push, because the profile still held the pre-drag layout.
                if let Some(layout) = hud_layout {
                    adopt_inbound_hud_layout(controller, &layout);
                }
                // `publish_shared`, not `publish`: the frame already arrives as an `Arc`
                // (`codec.rs:51-54`), and handing it over unchanged is the zero-copy path
                // `d8e23a4` added. `publish(*state, ..)` would be a move out of the `Arc`.
                store.publish_shared(state, Instant::now());
                // The `stateNotify=` column: one per published state, which is what the managed
                // `RecordStateNotification` counted.
                client.diagnostics().record_state_notification();
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

        // Recorded for every line the loop consumed, discarded ones included, which is what the
        // managed `finally` did - then the window is flushed if its five seconds are up.
        client
            .diagnostics()
            .record_inbound(i64::try_from(read).unwrap_or(i64::MAX), elapsed);
        log_transport_diagnostics(&client, diagnostics_epoch);
    }

    // The reason first, then the flag: see this function's doc for why the order is a
    // correctness requirement rather than style.
    report.publish(LinkFailure::Disconnected);
    store.set_connected(false);

    // The capability half, which the managed read loop also reset on its way out:
    // `finally { Capabilities = BridgeCapabilities.ForVersionFallback(InjectedVersion); }`
    // (`GameStateClient.cs:609`). Without it a dead link keeps advertising the modules the bridge
    // that is gone claimed to support, which is worse than having no answer: every surface that
    // gates a control on `module_supported` keeps the control enabled for a bridge that cannot
    // answer. The fallback is the same table `Runtime::new` seeds the cell with, so "no link" reads
    // exactly like "not connected yet".
    if let Ok(mut cell) = advertised.write() {
        *cell = BridgeCapabilities::for_version_fallback(None);
    }
}

/// Stores an inbound `hudLayout` into the active profile, unless it is the echo of what is there.
///
/// `GameStateClient.ApplyInboundHudLayout` (`GameStateClient.cs:1624-1648`), in the three steps
/// that function takes and in the same order:
///
/// 1. **Compare.** `inbound.EqualsLayout(Clicker.Instance.HudLayout)` returned early, and
///    [`HudLayout::equals_layout`] is that comparison (per-axis float epsilon). The bridge repeats
///    the layout on frames it sends, so the equal case is the common one and has to cost a compare:
///    a profile write would invalidate the cached config frame and push the same bytes straight back
///    at the bridge.
/// 2. **Write.** `Clicker.Instance.HudLayout = inbound` is `Profile::hud_layout`, and
///    `Profile::hud_layout_map` is the same conversion the HUD editor and the config frame use, so
///    the drag survives a save and the next `config` frame carries the layout the user sees.
/// 3. **Persist.** The managed host called `ProfileManager.SaveProfile` here because it owned the
///    profile store. This reader does not: the shell is the only writer of the profile directory
///    (`Shell::save_profile`), so the change lands in the controller and the shell's next save puts
///    it on disk. That save is the half that only works if it takes `hud_layout` from the
///    controller - see this module's note on `profile_for_save`.
///
/// No echo suppression is needed on this side: the sender compares the frame it last wrote with the
/// controller's current frame, so a layout the bridge itself just pushed differs from `sent` once,
/// goes out once, and then compares equal. Managed suppressed its push because it re-sent on a 2 s
/// heartbeat rather than on change (`GameStateClient.cs:1632-1647`).
///
/// Returns whether the profile changed.
fn adopt_inbound_hud_layout(controller: &AppController, inbound: &HudLayout) -> bool {
    // `with_profile` holds the profile read lock for exactly this conversion; `update_profile`
    // takes the frame cache first and then the profile, so the compare must not be made while that
    // guard is alive or the two locks would be taken in the opposite order to every writer.
    let unchanged = controller
        .with_profile(|profile| profile.to_hud_layout())
        .equals_layout(inbound);
    if unchanged {
        return false;
    }

    let changed = controller.update_profile(|profile| {
        profile.hud_layout = Some(Profile::hud_layout_map(inbound));
        true
    });
    tracing::debug!(
        changed = changed,
        "in-game HUD layout adopted from the bridge"
    );
    changed
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

    let encoded_at = Instant::now();
    let line = frame.to_line();
    // The encode is timed on its own so a slow frame shows up separately from a slow socket: the
    // `configSerialize=` and `configSerializeCpu=` columns against `configSend=`.
    let encode_ticks = i64::try_from(encoded_at.elapsed().as_millis()).unwrap_or(i64::MAX);
    let keep = (*frame).clone();
    drop(frame);
    client
        .diagnostics()
        .record_config_serialization(i64::try_from(line.len()).unwrap_or(i64::MAX), encode_ticks);
    client.send_line(&line)?;
    client
        .diagnostics()
        .record_config_send(i64::try_from(line.len()).unwrap_or(i64::MAX));
    tracing::debug!(bytes = line.len(), "bridge config frame sent");
    *sent = Some(keep);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aoko_protocol::module_catalog::EnabledFlag;
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
    /// `fightStatus` is the module switch and `keybindAutoclicker` is the profile's key, so
    /// neither is the payload's default.
    #[test]
    fn connecting_writes_the_controllers_config_frame_once() {
        let controller = Arc::new(AppController::default());
        controller.set_module_enabled(EnabledFlag::FightStatus, true);
        controller.update_profile(|profile| {
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
            "`fightStatus` is the `FightStatus` switch; `false` would be a default payload"
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

        // The flip is what a switch press does, and it happens off the reader's thread.
        controller.set_module_enabled(EnabledFlag::FightStatus, true);

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

        // One change a frame can see — a switch the payload carries — so there are two frames on
        // the wire.
        controller.set_module_enabled(EnabledFlag::FightStatus, true);
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

    // ---- the dial window ---------------------------------------------------------------

    /// A bridge that is not listening yet is dialled again rather than given up on.
    ///
    /// This is the regression that mattered: one dial issued from the press that injected the
    /// bridge lost the race with the bridge's own `bind`, and nothing retried, so the app stayed
    /// attached to nothing for the rest of the session and every state-driven control (aim assist,
    /// chest stealer, the HUD layout send) behaved as if the game were dead.
    #[test]
    fn a_bridge_that_refuses_twice_is_still_dialled() {
        let stop = AtomicBool::new(false);
        let mut dials = 0u32;

        let connected = dial_with_retries(
            || {
                dials += 1;
                if dials < 3 {
                    Err(TransportError::Closed)
                } else {
                    Ok(dials)
                }
            },
            &stop,
            30,
            Duration::ZERO,
        );

        assert_eq!(
            connected,
            Some(3),
            "the attempt that answered is the one returned"
        );
        assert_eq!(dials, 3, "two refusals and then the success, in that order");
    }

    /// An exhausted window answers `None` after exactly `attempts` dials, so a bridge that is
    /// never coming costs a bounded number of syscalls and then one published failure.
    #[test]
    fn an_exhausted_dial_window_stops_at_the_last_attempt() {
        let stop = AtomicBool::new(false);
        let mut dials = 0u32;

        let connected: Option<u32> = dial_with_retries(
            || {
                dials += 1;
                Err(TransportError::Closed)
            },
            &stop,
            4,
            Duration::ZERO,
        );

        assert_eq!(connected, None);
        assert_eq!(dials, 4, "one dial per attempt and not one more");
    }

    /// A reader that was replaced dials nothing at all, which is what keeps the replacement's
    /// answer the only one published: `Runtime` shares one `LinkReport` across attempts.
    #[test]
    fn a_raised_stop_is_read_before_the_first_dial() {
        let stop = AtomicBool::new(true);
        let mut dials = 0u32;

        let connected: Option<u32> = dial_with_retries(
            || {
                dials += 1;
                Ok(dials)
            },
            &stop,
            30,
            Duration::ZERO,
        );

        assert_eq!(connected, None);
        assert_eq!(
            dials, 0,
            "a stopped reader must not race the one that replaced it"
        );
    }

    // ---- the transport diagnostics ------------------------------------------------------

    /// The counting transport reports the bytes of the frame it read, and a `take` drains them.
    ///
    /// This is the number `AOKO_PERF_DIAGNOSTICS` prints as `chars=`/`inbound=`, and before this
    /// wrapper it was never taken: `TransportClient::pump` returns a decoded frame and keeps its
    /// line buffer private, so nothing in the shipped path ever called `record_inbound` and the
    /// summary had no window to print.
    #[test]
    fn the_counted_transport_reports_the_bytes_it_read() {
        let (transport, peer) = memory::duplex();
        let count = ByteCount::default();
        let mut counted = Counted {
            inner: transport,
            bytes_read: Arc::clone(&count.0),
        };

        peer.feed(b"{\"a\":1}\n");
        let mut line = LineBuf::new();
        assert_eq!(
            counted.recv_line(&mut line).expect("read the frame"),
            RecvOutcome::Line
        );

        assert_eq!(line.as_str(), "{\"a\":1}", "the frame itself is unchanged");
        assert_eq!(count.take(), 7, "seven bytes, the terminator stripped");
        assert_eq!(count.take(), 0, "a take leaves the counter at zero");
    }

    // ---- the inbound HUD layout ---------------------------------------------------------

    /// The echo guard is what keeps a repeated layout from rewriting the profile every frame.
    ///
    /// `GameStateClient.ApplyInboundHudLayout` returned early on `EqualsLayout`
    /// (`GameStateClient.cs:1629-1630`). Without the guard every state frame would write the
    /// profile, each write would invalidate the cached config frame, and one bridge echo would
    /// become one push per frame for the rest of the session.
    #[test]
    fn an_inbound_hud_layout_is_adopted_once_and_the_echo_is_ignored() {
        use aoko_protocol::hud::{element_id, HudElementLayout, HudLayout};

        let controller = AppController::default();

        // A default profile already *is* the default layout, so the first echo changes nothing.
        // The assertion is on the absence of a write, not on the value: a write would leave
        // `Some(...)` where the profile started with `None`.
        assert!(!adopt_inbound_hud_layout(&controller, &HudLayout::new()));
        assert!(
            controller.profile().hud_layout.is_none(),
            "an equal layout must not touch the profile"
        );

        // One element dragged in game: adopted, and stored through the map the editor and the
        // config frame both use.
        let mut dragged = HudLayout::new();
        dragged.set(
            element_id::MODULE_LIST,
            HudElementLayout::new(0.42, 0.11, 1.0),
        );
        assert!(adopt_inbound_hud_layout(&controller, &dragged));

        let stored = controller
            .profile()
            .hud_layout
            .expect("the drag is in the profile");
        let element = stored.get(element_id::MODULE_LIST).expect("every known id");
        assert_eq!((element.x, element.y, element.scale), (0.42, 0.11, 1.0));

        // The echo of that same layout now equals the profile, so it is ignored too: one drag,
        // one write, however many frames carry it.
        assert!(!adopt_inbound_hud_layout(&controller, &dragged));
    }

    /// A HUD drag reported by the bridge reaches the profile *and* the next `config` frame.
    ///
    /// This is the path that was dead: the codec decoded the frame's `hudLayout`, handed it out
    /// beside the state, and the reader dropped it. The profile kept the pre-drag layout, so the
    /// next push reverted the drag the user had just made in game.
    #[test]
    fn a_hud_drag_in_game_reaches_the_profile_and_the_next_config_frame() {
        use aoko_protocol::hud::element_id;

        let controller = Arc::new(AppController::default());
        let (peer, closed, ingest) = connect(&controller);
        wait_for_lines(&peer, 1);

        // A state frame carrying the layout the bridge says the user dragged to.
        peer.feed(
            format!(
                "{{\"inWorld\":true,\"hudLayout\":{{\"{}\":{{\"x\":0.42,\"y\":0.25,\"scale\":1.0}}}}}}\n",
                element_id::MODULE_LIST
            )
            .as_bytes(),
        );

        // The profile is the store the HUD editor and the save path read, so the drag has to land
        // there and not only in the decoder.
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let moved = controller.with_profile(|profile| {
                profile
                    .hud_layout
                    .as_ref()
                    .and_then(|layout| layout.get(element_id::MODULE_LIST).copied())
                    .is_some_and(|element| (element.x - 0.42).abs() < 1e-12)
            });
            if moved {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "the in-game drag never reached the profile"
            );
            thread::sleep(Duration::from_millis(2));
        }

        // The adopted layout is a change, so the sender puts it on the live link in the next
        // frame: the bridge and the profile end up agreeing instead of fighting.
        let lines = wait_for_lines(&peer, 2);
        assert_eq!(
            lines.len(),
            2,
            "the connect frame and the adopted layout: {lines:?}"
        );
        let frame = json(&lines[1]);
        assert_eq!(frame["type"].as_str(), Some("config"));
        assert_eq!(
            frame["hudLayout"][element_id::MODULE_LIST]["x"].as_f64(),
            Some(0.42),
            "the config frame carries the layout the user dragged to"
        );

        closed.store(true, Ordering::SeqCst);
        ingest.join();
    }
}
