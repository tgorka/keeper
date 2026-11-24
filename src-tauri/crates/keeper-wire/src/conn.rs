//! One connection: its reader, its writer and its calls (MA/AD-3 *In order*,
//! *Request ids and multiplexing*, *Cancellation*, *Deadlines*, *Streamed
//! answers*, *Limits*).
//!
//! Every frame the server writes on a connection goes through one queue and
//! one writer, terminal frames and refusals alike, so a call's terminal frame
//! is written before anything a later call with the same id says. A call's
//! terminal frame is decided once, under the connection's lock, by removing
//! its entry from the in-flight map and queueing the frame in the same step:
//! whatever comes for that call afterwards finds no entry and is ignored.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use keeper_runtime_api::{Door as RtDoor, ErrorCode, Kind, OpError, Reply as RtReply};
use keeper_wire_api::frame::{
    line, Close, DescribeOutput, Detail, Door, End, Event, Reply, ServerFrame, ServerHello,
    WireError, WIRE_V1,
};
use keeper_wire_api::{is_call_id, Code};
use serde_json::{Map, Value};
use tokio::io::{AsyncBufReadExt as _, AsyncReadExt as _, AsyncWriteExt as _, BufReader};
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::{watch, Notify};
use tokio::task::AbortHandle;
use tokio::time::Instant;

use crate::{CheckedPeer, Counter, Shared};

/// A call's place: one on its connection and one on the listener, given back
/// when the last holder drops it (the written terminal frame and, for a
/// started command, its task).
struct Place {
    conn: Arc<Counter>,
    listener: Arc<Counter>,
}

impl Place {
    fn take(conn: &Arc<Counter>, listener: &Arc<Counter>) -> Option<Self> {
        if !conn.try_take() {
            return None;
        }
        if !listener.try_take() {
            conn.give_back();
            return None;
        }
        Some(Self {
            conn: Arc::clone(conn),
            listener: Arc::clone(listener),
        })
    }
}

impl Drop for Place {
    fn drop(&mut self) {
        self.conn.give_back();
        self.listener.give_back();
    }
}

/// What one stream has waiting to be written.
#[derive(Default)]
struct Waiting {
    events: AtomicUsize,
    bytes: AtomicUsize,
}

/// A frame queued to the writer.
struct Out {
    bytes: Vec<u8>,
    queued_at: Instant,
    /// The stream event's call and generation, so a stream that fell behind
    /// can take its waiting events back.
    event_of: Option<u64>,
    /// Held until the frame is written, then given back by dropping.
    _place: Option<Arc<Place>>,
    waiting: Option<Arc<Waiting>>,
    unadmitted: bool,
    /// A `close`: the writer shuts the connection after it.
    last: bool,
}

/// A call in flight.
struct Entry {
    generation: u64,
    kind: Kind,
    /// Moved into the terminal frame when it is decided.
    place: Option<Arc<Place>>,
    task: Option<AbortHandle>,
    started: bool,
    waiting: Option<Arc<Waiting>>,
    seq: u64,
}

#[derive(Default)]
struct State {
    closed: bool,
    /// Closed with nothing more to write: the writer stops mid-frame too.
    hard: bool,
    queue: VecDeque<Out>,
    in_flight: HashMap<String, Entry>,
    unadmitted: usize,
    generation: u64,
}

struct Conn {
    state: Mutex<State>,
    writer_wake: Notify,
    reader_wake: Notify,
    closed_wake: Notify,
    shared: Arc<Shared>,
    places: Arc<Counter>,
}

/// What the reader does after a line.
enum Next {
    Read,
    /// Close with this error (a `close` frame).
    Close(WireError),
}

fn pointer(field: &str) -> String {
    if field.starts_with('/') {
        field.to_owned()
    } else if field.is_empty() {
        "/".to_owned()
    } else {
        format!("/{}", field.replace('.', "/"))
    }
}

fn code(code: ErrorCode) -> Code {
    match code {
        ErrorCode::NotFound => Code::NotFound,
        ErrorCode::InvalidArgument => Code::InvalidArgument,
        ErrorCode::Refused => Code::Refused,
        ErrorCode::Unavailable => Code::Unavailable,
        ErrorCode::Conflict => Code::Conflict,
        ErrorCode::DeadlineExceeded => Code::DeadlineExceeded,
        ErrorCode::Cancelled => Code::Cancelled,
        ErrorCode::ResourceExhausted => Code::ResourceExhausted,
        ErrorCode::Internal => Code::Internal,
    }
}

/// A module's error as a wire caller sees it: keeper's fixed sentence for the
/// code and the error kind. The module's own text is logged, never sent (m1).
fn wire_error(operation: &str, error: &OpError) -> WireError {
    tracing::info!(
        operation,
        code = error.code.as_str(),
        reason = %error.reason,
        detail = %error.detail,
        "a wire call answered an error"
    );
    let wire_code = code(error.code);
    let mut answer = WireError::fixed(wire_code, &error.reason).retryable(error.retryable);
    if wire_code == Code::InvalidArgument {
        answer = answer.with_field(pointer(&error.reason));
    }
    answer
}

fn reply_frame(call_id: &str, answer: Result<Value, WireError>) -> ServerFrame {
    match answer {
        Ok(output) => ServerFrame::Reply(Reply {
            call_id: call_id.to_owned(),
            output: Some(output),
            error: None,
        }),
        Err(error) => ServerFrame::Reply(Reply {
            call_id: call_id.to_owned(),
            output: None,
            error: Some(error),
        }),
    }
}

fn terminal(kind: Kind, call_id: &str, error: WireError) -> ServerFrame {
    match kind {
        Kind::Stream => ServerFrame::End(End {
            call_id: call_id.to_owned(),
            error: Some(error),
        }),
        Kind::Query | Kind::Command => reply_frame(call_id, Err(error)),
    }
}

fn close_frame(error: WireError) -> Vec<u8> {
    line(&ServerFrame::Close(Close {
        error,
        wire: vec![WIRE_V1.to_owned()],
    }))
}

impl Conn {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn limits(&self) -> &crate::Limits {
        &self.shared.limits
    }

    /// A frame's line; one longer than a line is the call's
    /// `RESOURCE_EXHAUSTED` instead (MA/AD-3 *Framing*).
    fn bytes_of(&self, frame: &ServerFrame) -> Vec<u8> {
        let bytes = line(frame);
        if bytes.len() <= self.limits().max_line_bytes {
            return bytes;
        }
        let too_large = WireError::fixed(Code::ResourceExhausted, "too_large").retryable(false);
        let replaced = match frame {
            ServerFrame::Reply(reply) => reply_frame(&reply.call_id, Err(too_large)),
            ServerFrame::End(end) => terminal(Kind::Stream, &end.call_id, too_large),
            ServerFrame::Event(event) => terminal(Kind::Stream, &event.call_id, too_large),
            ServerFrame::Hello(_) | ServerFrame::Close(_) => return bytes,
        };
        line(&replaced)
    }

    fn push(&self, state: &mut State, out: Out) {
        state.queue.push_back(out);
        self.writer_wake.notify_one();
    }

    /// An answer to a frame that took no place.
    fn refuse(&self, frame: &ServerFrame) {
        let bytes = self.bytes_of(frame);
        let mut state = self.lock();
        if state.closed {
            return;
        }
        state.unadmitted += 1;
        self.push(
            &mut state,
            Out {
                bytes,
                queued_at: Instant::now(),
                event_of: None,
                _place: None,
                waiting: None,
                unadmitted: true,
                last: false,
            },
        );
    }

    /// Decide `call_id`'s terminal frame, if generation `generation` is still
    /// in flight: free its id and queue the frame in one step. Returns the
    /// entry it decided.
    fn decide(&self, call_id: &str, generation: u64, frame: &ServerFrame) -> Option<Entry> {
        let bytes = self.bytes_of(frame);
        let mut state = self.lock();
        if state.in_flight.get(call_id)?.generation != generation {
            return None;
        }
        let mut entry = state.in_flight.remove(call_id)?;
        if !state.closed {
            let out = Out {
                bytes,
                queued_at: Instant::now(),
                event_of: None,
                _place: entry.place.take(),
                waiting: None,
                unadmitted: false,
                last: false,
            };
            self.push(&mut state, out);
        }
        Some(entry)
    }

    /// Queue one stream event, or end the stream `RESOURCE_EXHAUSTED` when it
    /// finds no room. `false`: the stream is over, drop its handler.
    fn event(&self, call_id: &str, generation: u64, output: Value) -> bool {
        let mut state = self.lock();
        if state.closed {
            return false;
        }
        let Some(entry) = state.in_flight.get_mut(call_id) else {
            return false;
        };
        if entry.generation != generation {
            return false;
        }
        entry.seq += 1;
        let frame = ServerFrame::Event(Event {
            call_id: call_id.to_owned(),
            seq: entry.seq,
            output,
        });
        let bytes = line(&frame);
        let waiting = entry.waiting.clone().unwrap_or_default();
        let limits = &self.shared.limits;
        let full = waiting.events.load(Ordering::Acquire) >= limits.stream_events
            || waiting.bytes.load(Ordering::Acquire) + bytes.len() > limits.stream_bytes;
        if full || bytes.len() > limits.max_line_bytes {
            let kind = if full { "fell_behind" } else { "too_large" };
            // Its waiting events are discarded: `end` is the next frame the
            // client reads for it.
            if full {
                state.queue.retain(|out| out.event_of != Some(generation));
            }
            drop(state);
            let error = WireError::fixed(Code::ResourceExhausted, kind).retryable(full);
            self.decide(call_id, generation, &terminal(Kind::Stream, call_id, error));
            return false;
        }
        waiting.events.fetch_add(1, Ordering::AcqRel);
        waiting.bytes.fetch_add(bytes.len(), Ordering::AcqRel);
        let out = Out {
            bytes,
            queued_at: Instant::now(),
            event_of: Some(generation),
            _place: None,
            waiting: Some(waiting),
            unadmitted: false,
            last: false,
        };
        self.push(&mut state, out);
        true
    }

    /// End the connection. Queries and streams are dropped, a command not
    /// yet started never starts, a started one runs to its end holding its
    /// listener place. With `close`, that frame is the last one written after
    /// what was queued; without, nothing more is written.
    fn end(&self, close: Option<WireError>) {
        let mut state = self.lock();
        if state.closed {
            if close.is_none() && !state.hard {
                state.hard = true;
                state.queue.clear();
                drop(state);
                self.writer_wake.notify_one();
                self.closed_wake.notify_waiters();
            }
            return;
        }
        state.closed = true;
        for (_, entry) in state.in_flight.drain() {
            if !(entry.kind == Kind::Command && entry.started) {
                if let Some(task) = entry.task {
                    task.abort();
                }
            }
        }
        match close {
            Some(error) => {
                let out = Out {
                    bytes: close_frame(error),
                    queued_at: Instant::now(),
                    event_of: None,
                    _place: None,
                    waiting: None,
                    unadmitted: false,
                    last: true,
                };
                state.queue.push_back(out);
            }
            None => {
                state.hard = true;
                state.queue.clear();
            }
        }
        drop(state);
        self.writer_wake.notify_one();
        self.reader_wake.notify_one();
        self.closed_wake.notify_waiters();
    }

    async fn closed(&self, hard: bool) {
        loop {
            let notified = self.closed_wake.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            {
                let state = self.lock();
                if state.closed && (state.hard || !hard) {
                    return;
                }
            }
            notified.await;
        }
    }
}

/// Serve one checked connection to its end.
pub(crate) async fn serve(
    peer: CheckedPeer,
    shared: Arc<Shared>,
    mut shutdown: watch::Receiver<bool>,
) {
    let (read, mut write) = peer.stream.into_split();
    // The listener's connection place, before the `hello` is read.
    if !shared.connections.try_take() {
        let bytes = close_frame(WireError::fixed(Code::ResourceExhausted, "connections"));
        let _ = tokio::time::timeout(shared.limits.stall, write.write_all(&bytes)).await;
        return;
    }
    struct ConnectionPlace(Arc<Counter>);
    impl Drop for ConnectionPlace {
        fn drop(&mut self) {
            self.0.give_back();
        }
    }
    let _connection = ConnectionPlace(Arc::clone(&shared.connections));
    let conn = Arc::new(Conn {
        state: Mutex::new(State::default()),
        writer_wake: Notify::new(),
        reader_wake: Notify::new(),
        closed_wake: Notify::new(),
        places: Arc::new(Counter::new(shared.limits.per_connection)),
        shared,
    });
    let writer = tokio::spawn(write_loop(Arc::clone(&conn), write));
    let mut reader = Reader {
        conn: Arc::clone(&conn),
        read: BufReader::with_capacity(64 * 1024, read),
        hello: false,
    };
    tokio::select! {
        () = reader.run() => {}
        () = conn.closed(false) => {}
        _ = shutdown.wait_for(|stop| *stop) => {
            conn.end(Some(WireError::fixed(Code::Unavailable, "switched_off")));
        }
    }
    // A reader that stopped at the end of its input: the client went away,
    // and nothing more is written. A reader that closed the connection with
    // a `close` frame (or a stop) left it queued for the writer.
    if !conn.lock().closed {
        conn.end(None);
    }
    let _ = writer.await;
}

async fn write_loop(conn: Arc<Conn>, mut write: OwnedWriteHalf) {
    loop {
        let out = loop {
            let notified = conn.writer_wake.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let closed = {
                let mut state = conn.lock();
                if let Some(out) = state.queue.pop_front() {
                    break out;
                }
                state.closed
            };
            if closed {
                let _ = write.shutdown().await;
                return;
            }
            notified.await;
        };
        // A frame's age counts, not the time since a byte last moved.
        let deadline = out.queued_at + conn.limits().stall;
        // A terminal frame gives its place back before its `\n` is written:
        // the client has the frame only once the `\n` arrives, so a client
        // that read it finds the place free, and a frame stuck behind a
        // reader that stopped still holds its place.
        let mut out = out;
        let (body, newline) = out.bytes.split_at(out.bytes.len().saturating_sub(1));
        let place = out._place.take();
        let written = tokio::select! {
            written = tokio::time::timeout_at(deadline, async {
                write.write_all(body).await?;
                drop(place);
                write.write_all(newline).await
            }) => written,
            () = conn.closed(true) => return,
        };
        if !matches!(written, Ok(Ok(()))) {
            if written.is_err() {
                tracing::warn!("keeper's socket closed a connection whose reader stalled");
            }
            conn.end(None);
            let _ = write.shutdown().await;
            return;
        }
        if let Some(waiting) = &out.waiting {
            waiting.events.fetch_sub(1, Ordering::AcqRel);
            waiting.bytes.fetch_sub(out.bytes.len(), Ordering::AcqRel);
        }
        if out.unadmitted {
            conn.lock().unadmitted -= 1;
            conn.reader_wake.notify_one();
        }
        if out.last {
            let _ = write.shutdown().await;
            return;
        }
        // `out` drops here: a terminal frame gives its place back once written.
    }
}

struct Reader {
    conn: Arc<Conn>,
    read: BufReader<OwnedReadHalf>,
    hello: bool,
}

/// What a line was, framing-wise.
enum Line {
    Frame(Vec<u8>),
    /// The connection ended; a torn last line is discarded unread.
    Eof,
    TooLong,
}

impl Reader {
    async fn run(&mut self) {
        loop {
            // While refusals fill their queue, no further line is read.
            loop {
                let notified = self.conn.reader_wake.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                {
                    let state = self.conn.lock();
                    if state.closed {
                        return;
                    }
                    if state.unadmitted < self.conn.limits().unadmitted {
                        break;
                    }
                }
                notified.await;
            }
            let next = match self.read_line().await {
                Line::Eof => return,
                Line::TooLong => {
                    Next::Close(WireError::fixed(Code::ResourceExhausted, "line_too_long"))
                }
                Line::Frame(bytes) => self.handle(&bytes),
            };
            if let Next::Close(error) = next {
                self.conn.end(Some(error));
                return;
            }
        }
    }

    async fn read_line(&mut self) -> Line {
        let limit = self.conn.limits().max_line_bytes;
        let mut buf = Vec::new();
        let mut take = (&mut self.read).take(limit as u64);
        let result = take.read_until(b'\n', &mut buf).await;
        self.conn
            .shared
            .longest_line_held
            .fetch_max(buf.len(), Ordering::AcqRel);
        match result {
            Err(_) | Ok(0) => Line::Eof,
            Ok(_) if buf.last() == Some(&b'\n') => Line::Frame(buf),
            Ok(_) if buf.len() >= limit => Line::TooLong,
            Ok(_) => Line::Eof,
        }
    }

    /// One line, in MA/AD-3's order: framing and `hello`; `cancel`; an id in
    /// flight; the frame's own fields; the place; the dispatcher.
    fn handle(&mut self, bytes: &[u8]) -> Next {
        let invalid = |kind: &str, field: &str| {
            Next::Close(WireError::fixed(Code::InvalidArgument, kind).with_field(field))
        };
        // 1. Framing.
        let Ok(text) = std::str::from_utf8(bytes) else {
            return invalid("framing", "/");
        };
        let Ok(Value::Object(frame)) = serde_json::from_str::<Value>(text) else {
            return invalid("framing", "/");
        };
        let kind = frame.get("type").and_then(Value::as_str);
        if kind == Some("hello") {
            if self.hello {
                return invalid("second_hello", "/type");
            }
            return self.hello(&frame);
        }
        if !self.hello {
            return invalid("not_hello", "/type");
        }
        let Some(call_id) = frame.get("call_id").and_then(Value::as_str) else {
            return invalid("framing", "/call_id");
        };
        if !is_call_id(call_id) {
            return invalid("framing", "/call_id");
        }
        let call_id = call_id.to_owned();
        // 2. Cancel.
        if kind == Some("cancel") {
            self.cancel(&call_id);
            return Next::Read;
        }
        // 3. An id in flight closes the connection, whatever else is wrong.
        if self.conn.lock().in_flight.contains_key(&call_id) {
            return invalid("call_id_in_flight", "/call_id");
        }
        // 4. The frame's own fields.
        let refuse = |field: &str| {
            self.conn.refuse(&reply_frame(
                &call_id,
                Err(WireError::fixed(Code::InvalidArgument, "framing").with_field(field)),
            ));
            Next::Read
        };
        let frame_kind = match kind {
            Some("call") => Kind::Query,
            Some("watch") => Kind::Stream,
            Some("describe") => Kind::Query,
            _ => return refuse("/type"),
        };
        let is_describe = kind == Some("describe");
        let operation = match frame.get("operation") {
            Some(Value::String(op)) => Some(op.clone()),
            None | Some(Value::Null) if is_describe => None,
            _ => return refuse("/operation"),
        };
        let door = match frame.get("door") {
            None | Some(Value::Null) => Door::Cli,
            Some(Value::String(door)) => match Door::parse(door) {
                Some(door) => door,
                None => return refuse("/door"),
            },
            Some(_) => return refuse("/door"),
        };
        let detail = match frame.get("detail") {
            None | Some(Value::Null) => Detail::Full,
            Some(Value::String(detail)) => match Detail::parse(detail) {
                Some(detail) => detail,
                None => return refuse("/detail"),
            },
            Some(_) => return refuse("/detail"),
        };
        let deadline_ms = if frame_kind == Kind::Stream {
            None
        } else {
            match frame.get("deadline_ms") {
                None | Some(Value::Null) => None,
                Some(value) => match value.as_u64() {
                    Some(ms) => Some(ms),
                    None => return refuse("/deadline_ms"),
                },
            }
        };
        // 5. The place.
        let Some(place) = Place::take(&self.conn.places, &self.conn.shared.calls) else {
            self.conn.refuse(&terminal(
                frame_kind,
                &call_id,
                WireError::fixed(Code::ResourceExhausted, "places"),
            ));
            return Next::Read;
        };
        // 6. The dispatcher.
        let input = frame
            .get("input")
            .cloned()
            .unwrap_or(Value::Object(Map::new()));
        let rt_door = match door {
            Door::Cli => RtDoor::Cli,
            Door::Mcp => RtDoor::Mcp,
        };
        if is_describe {
            let conn = Arc::clone(&self.conn);
            let work = Box::pin(async move {
                let dispatch = &conn.shared.dispatch;
                let operations = dispatch
                    .describe(rt_door, detail)
                    .into_iter()
                    .filter(|entry| operation.as_deref().is_none_or(|op| entry.id == op))
                    .collect();
                let output = DescribeOutput {
                    envelope: WIRE_V1.to_owned(),
                    app_version: dispatch.app_version(),
                    catalogue_digest: dispatch.catalogue_digest(),
                    operations,
                    next_page_token: None,
                };
                serde_json::to_value(output)
                    .map_err(|e| OpError::new(ErrorCode::Internal, "describe", e.to_string()))
            });
            self.spawn_query(call_id, place, deadline_ms, "describe".to_owned(), work);
            return Next::Read;
        }
        let operation = operation.unwrap_or_default();
        let dispatch = Arc::clone(&self.conn.shared.dispatch);
        let Some(op_kind) = dispatch.kind(rt_door, &operation) else {
            let error = wire_error(
                &operation,
                &OpError::new(ErrorCode::NotFound, "unknown_operation", &operation),
            );
            self.answer_now(frame_kind, &call_id, place, error);
            return Next::Read;
        };
        if (frame_kind == Kind::Stream) != (op_kind == Kind::Stream) {
            let error = WireError::fixed(Code::InvalidArgument, "framing").with_field("/type");
            self.answer_now(frame_kind, &call_id, place, error);
            return Next::Read;
        }
        match op_kind {
            Kind::Query => {
                let work = dispatch.call(rt_door, &operation, input);
                self.spawn_query(call_id, place, deadline_ms, operation, work);
            }
            Kind::Command => {
                self.spawn_command(call_id, place, deadline_ms, rt_door, operation, input)
            }
            Kind::Stream => {
                let stream = dispatch.watch(rt_door, &operation, input);
                self.spawn_stream(call_id, place, operation, stream);
            }
        }
        Next::Read
    }

    fn hello(&mut self, frame: &Map<String, Value>) -> Next {
        let offered = frame.get("wire").and_then(Value::as_array);
        let speaks = offered.is_some_and(|versions| {
            versions
                .iter()
                .any(|version| version.as_str() == Some(WIRE_V1))
        });
        if !speaks {
            return Next::Close(
                WireError::fixed(Code::InvalidArgument, "version").with_field("/wire"),
            );
        }
        if let Some(client) = frame.get("client").and_then(Value::as_str) {
            tracing::info!(client, "keeper's socket: hello");
        }
        self.hello = true;
        let dispatch = &self.conn.shared.dispatch;
        let limits = self.conn.limits();
        let hello = ServerFrame::Hello(ServerHello {
            wire: WIRE_V1.to_owned(),
            app_version: dispatch.app_version(),
            catalogue_digest: dispatch.catalogue_digest(),
            max_line_bytes: limits.max_line_bytes as u64,
            max_in_flight: u32::try_from(limits.per_connection).unwrap_or(u32::MAX),
        });
        let bytes = line(&hello);
        let mut state = self.conn.lock();
        let out = Out {
            bytes,
            queued_at: Instant::now(),
            event_of: None,
            _place: None,
            waiting: None,
            unadmitted: false,
            last: false,
        };
        self.conn.push(&mut state, out);
        Next::Read
    }

    /// Enter a call in flight; its task is attached by the caller.
    fn enter(&self, call_id: &str, kind: Kind, place: Place) -> (u64, Arc<Place>) {
        let place = Arc::new(place);
        let mut state = self.conn.lock();
        state.generation += 1;
        let generation = state.generation;
        state.in_flight.insert(
            call_id.to_owned(),
            Entry {
                generation,
                kind,
                place: Some(Arc::clone(&place)),
                task: None,
                started: false,
                waiting: (kind == Kind::Stream).then(Arc::default),
                seq: 0,
            },
        );
        (generation, place)
    }

    fn attach(&self, call_id: &str, generation: u64, task: AbortHandle) {
        let mut state = self.conn.lock();
        match state.in_flight.get_mut(call_id) {
            Some(entry) if entry.generation == generation => entry.task = Some(task),
            // Already decided (or the connection ended) before the task was
            // attached: a query or a stream has nothing left to do.
            _ => {}
        }
    }

    /// A call answered at once, holding its place until the answer is written.
    fn answer_now(&self, kind: Kind, call_id: &str, place: Place, error: WireError) {
        let (generation, _) = self.enter(call_id, kind, place);
        self.conn
            .decide(call_id, generation, &terminal(kind, call_id, error));
    }

    fn deadline(&self, kind: Kind, deadline_ms: Option<u64>) -> std::time::Duration {
        let limits = self.conn.limits();
        let ceiling = match kind {
            Kind::Command => limits.command_ceiling,
            Kind::Query | Kind::Stream => limits.query_ceiling,
        };
        match deadline_ms {
            None | Some(0) => ceiling,
            Some(ms) => ceiling.min(std::time::Duration::from_millis(ms)),
        }
    }

    fn spawn_query(
        &self,
        call_id: String,
        place: Place,
        deadline_ms: Option<u64>,
        operation: String,
        work: crate::dispatch::BoxFuture<'static, RtReply>,
    ) {
        let deadline = self.deadline(Kind::Query, deadline_ms);
        let (generation, place) = self.enter(&call_id, Kind::Query, place);
        drop(place);
        let conn = Arc::clone(&self.conn);
        let id = call_id.clone();
        let task = tokio::spawn(async move {
            let answer = match tokio::time::timeout(deadline, work).await {
                Ok(Ok(output)) => Ok(output),
                Ok(Err(error)) => Err(wire_error(&operation, &error)),
                Err(_) => Err(WireError::fixed(Code::DeadlineExceeded, "query")),
            };
            conn.decide(&id, generation, &reply_frame(&id, answer));
        });
        self.attach(&call_id, generation, task.abort_handle());
    }

    fn spawn_command(
        &self,
        call_id: String,
        place: Place,
        deadline_ms: Option<u64>,
        door: RtDoor,
        operation: String,
        input: Value,
    ) {
        let deadline = self.deadline(Kind::Command, deadline_ms);
        let (generation, place) = self.enter(&call_id, Kind::Command, place);
        // The deadline, decided by whoever comes first.
        let conn = Arc::clone(&self.conn);
        let id = call_id.clone();
        tokio::spawn(async move {
            tokio::time::sleep(deadline).await;
            let started = {
                let state = conn.lock();
                match state.in_flight.get(&id) {
                    Some(entry) if entry.generation == generation => entry.started,
                    _ => return,
                }
            };
            let error = deadline_error(&id, started);
            conn.decide(&id, generation, &reply_frame(&id, Err(error)));
        });
        let conn = Arc::clone(&self.conn);
        let id = call_id.clone();
        let task = tokio::spawn(async move {
            // The task's share of the place: a started command keeps it
            // until its handler has returned.
            let _place = place;
            conn.shared.hooks.before_start().await;
            // The start gate (MA/AD-5 step 9): start, or not, at one point.
            {
                let mut state = conn.lock();
                if state.closed {
                    return;
                }
                match state.in_flight.get_mut(&id) {
                    Some(entry) if entry.generation == generation => entry.started = true,
                    _ => return,
                }
            }
            let answer = conn.shared.dispatch.call(door, &operation, input).await;
            let answer = answer.map_err(|error| wire_error(&operation, &error));
            conn.decide(&id, generation, &reply_frame(&id, answer));
        });
        self.attach(&call_id, generation, task.abort_handle());
    }

    fn spawn_stream(
        &self,
        call_id: String,
        place: Place,
        operation: String,
        mut stream: crate::dispatch::Stream,
    ) {
        let (generation, place) = self.enter(&call_id, Kind::Stream, place);
        drop(place);
        let conn = Arc::clone(&self.conn);
        let id = call_id.clone();
        let task = tokio::spawn(async move {
            struct Open(Arc<Shared>);
            impl Drop for Open {
                fn drop(&mut self) {
                    self.0.open_streams.fetch_sub(1, Ordering::AcqRel);
                }
            }
            conn.shared.open_streams.fetch_add(1, Ordering::AcqRel);
            let _open = Open(Arc::clone(&conn.shared));
            loop {
                let item = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await;
                match item {
                    Some(Ok(output)) => {
                        if !conn.event(&id, generation, output) {
                            return;
                        }
                    }
                    Some(Err(error)) => {
                        let error = wire_error(&operation, &error);
                        conn.decide(&id, generation, &terminal(Kind::Stream, &id, error));
                        return;
                    }
                    None => {
                        let end = ServerFrame::End(End {
                            call_id: id.clone(),
                            error: None,
                        });
                        conn.decide(&id, generation, &end);
                        return;
                    }
                }
                // Let the writer and other calls run between events.
                tokio::task::yield_now().await;
            }
        });
        self.attach(&call_id, generation, task.abort_handle());
    }

    /// `cancel`: a query or a stream is dropped; a started command runs on
    /// and its caller is answered with its `request_id`; a command not yet
    /// started never starts. An id not in flight is ignored.
    fn cancel(&self, call_id: &str) {
        let (generation, kind, started) = {
            let state = self.conn.lock();
            let Some(entry) = state.in_flight.get(call_id) else {
                return;
            };
            (entry.generation, entry.kind, entry.started)
        };
        let mut error = WireError::fixed(
            Code::Cancelled,
            if started { "started" } else { "not_started" },
        );
        if kind == Kind::Command && started {
            error = error.with_request_id(call_id);
        }
        let Some(entry) = self
            .conn
            .decide(call_id, generation, &terminal(kind, call_id, error))
        else {
            return;
        };
        if !(kind == Kind::Command && entry.started) {
            if let Some(task) = entry.task {
                task.abort();
            }
        }
    }
}

fn deadline_error(call_id: &str, started: bool) -> WireError {
    if started {
        WireError::fixed(Code::DeadlineExceeded, "started")
            .retryable(true)
            .with_request_id(call_id)
    } else {
        WireError::fixed(Code::DeadlineExceeded, "not_started").retryable(true)
    }
}
