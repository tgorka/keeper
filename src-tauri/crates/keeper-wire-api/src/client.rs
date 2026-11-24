//! The client of `keeper.wire.v1` (MA/AD-3, MA/AD-4).
//!
//! It checks the data directory and the path as the server does, refuses a
//! listener whose kernel credentials are not its own user's, says `hello`,
//! carries many calls on one connection, and sends `cancel` when its caller
//! drops a call it is waiting for. A call whose connection ends before its
//! terminal frame is reported `UNAVAILABLE`, retryable, with its `call_id` as
//! `request_id`, whatever a `close` frame said.

use std::collections::HashMap;
use std::fmt;
use std::io;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use serde_json::Value;
use tokio::io::{AsyncBufReadExt as _, AsyncReadExt as _, AsyncWriteExt as _, BufReader};
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::UnixStream;
use tokio::sync::{mpsc, oneshot};

use crate::frame::{
    line, Call, Cancel, ClientFrame, ClientHello, Describe, DescribeOutput, Detail, Door,
    ServerFrame, ServerHello, Watch, WireError, MAX_LINE_BYTES, WIRE_V1,
};
use crate::place::{check_for_dial, effective_uid, PlaceError};
use crate::{new_call_id, Code};

/// How a call is sent.
#[derive(Clone, Debug, Default)]
pub struct CallOptions {
    /// The id to send; a fresh ULID when `None`.
    pub call_id: Option<String>,
    pub deadline_ms: Option<u64>,
    pub door: Option<Door>,
    pub detail: Option<Detail>,
}

/// Why a connection was not made.
#[derive(Debug)]
pub enum ClientError {
    Place(PlaceError),
    Connect(io::Error),
    /// The listener is another user's.
    Foreign {
        uid: u32,
    },
    /// The server answered the `hello` with `close`.
    Refused(WireError),
    Protocol(String),
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Place(error) => error.fmt(f),
            Self::Connect(error) => write!(f, "keeper's socket did not answer: {error}"),
            Self::Foreign { uid } => {
                write!(f, "the listener belongs to uid {uid}, not to you; refused")
            }
            Self::Refused(error) => write!(f, "{}: {}", error.code.as_str(), error.message),
            Self::Protocol(reason) => write!(f, "keeper.wire.v1: {reason}"),
        }
    }
}

impl std::error::Error for ClientError {}

enum Waiter {
    Call(oneshot::Sender<Result<Value, WireError>>),
    Watch(mpsc::UnboundedSender<Result<Value, WireError>>),
}

#[derive(Default)]
struct Pending {
    ended: bool,
    waiters: HashMap<String, Waiter>,
}

struct Shared {
    pending: Mutex<Pending>,
    writer: mpsc::UnboundedSender<Vec<u8>>,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Pending> {
        self.pending.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The connection ended: every call in flight is `UNAVAILABLE` with its id.
    fn end(&self) {
        let waiters: Vec<(String, Waiter)> = {
            let mut pending = self.lock();
            pending.ended = true;
            pending.waiters.drain().collect()
        };
        for (call_id, waiter) in waiters {
            let error = WireError::connection_ended(&call_id);
            match waiter {
                Waiter::Call(tx) => {
                    let _ = tx.send(Err(error));
                }
                Waiter::Watch(tx) => {
                    let _ = tx.send(Err(error));
                }
            }
        }
    }
}

/// One connection to keeper's socket.
pub struct Client {
    shared: Arc<Shared>,
    hello: ServerHello,
    reader: tokio::task::JoinHandle<()>,
    writer: tokio::task::JoinHandle<()>,
}

impl Drop for Client {
    fn drop(&mut self) {
        self.reader.abort();
        self.writer.abort();
    }
}

/// Sends `cancel` for its call when dropped before the terminal frame.
struct CancelOnDrop {
    shared: Arc<Shared>,
    call_id: String,
    armed: bool,
}

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let was_waiting = self.shared.lock().waiters.remove(&self.call_id).is_some();
        if was_waiting {
            let frame = ClientFrame::Cancel(Cancel {
                call_id: self.call_id.clone(),
            });
            let _ = self.shared.writer.send(line(&frame));
        }
    }
}

/// Read one line of at most `limit` bytes, its `\n` included. `Ok(None)` at
/// the end of the stream (a torn last line is discarded).
async fn read_line(
    reader: &mut BufReader<OwnedReadHalf>,
    limit: usize,
) -> io::Result<Option<Vec<u8>>> {
    let mut buf = Vec::new();
    let mut take = reader.take(limit as u64);
    let n = take.read_until(b'\n', &mut buf).await?;
    if n == 0 || buf.last() != Some(&b'\n') {
        if buf.len() >= limit {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "a line longer than the limit",
            ));
        }
        return Ok(None);
    }
    Ok(Some(buf))
}

fn parse_server(bytes: &[u8]) -> Result<ServerFrame, String> {
    serde_json::from_slice(bytes).map_err(|e| format!("an invalid frame from the server: {e}"))
}

impl Client {
    /// Check `data_dir` and its `wire/` as the server does, then connect.
    pub async fn connect(data_dir: &Path, name: &str) -> Result<Self, ClientError> {
        let socket = check_for_dial(data_dir).map_err(ClientError::Place)?;
        Self::connect_socket(&socket, name).await
    }

    /// Connect to `socket` itself (no directory check; the peer check stays).
    pub async fn connect_socket(socket: &Path, name: &str) -> Result<Self, ClientError> {
        let stream = UnixStream::connect(socket)
            .await
            .map_err(ClientError::Connect)?;
        let cred = stream.peer_cred().map_err(ClientError::Connect)?;
        if cred.uid() != effective_uid() {
            return Err(ClientError::Foreign { uid: cred.uid() });
        }
        let (read, mut write) = stream.into_split();
        let mut reader = BufReader::new(read);
        let hello = ClientFrame::Hello(ClientHello {
            wire: vec![WIRE_V1.to_owned()],
            client: Some(name.to_owned()),
        });
        write
            .write_all(&line(&hello))
            .await
            .map_err(ClientError::Connect)?;
        let first = read_line(&mut reader, MAX_LINE_BYTES)
            .await
            .map_err(ClientError::Connect)?
            .ok_or_else(|| ClientError::Protocol("the server closed before its hello".into()))?;
        let hello = match parse_server(&first).map_err(ClientError::Protocol)? {
            ServerFrame::Hello(hello) => hello,
            ServerFrame::Close(close) => return Err(ClientError::Refused(close.error)),
            _ => return Err(ClientError::Protocol("the first line was not hello".into())),
        };
        let limit = usize::try_from(hello.max_line_bytes).unwrap_or(MAX_LINE_BYTES);
        let (tx, rx) = mpsc::unbounded_channel();
        let shared = Arc::new(Shared {
            pending: Mutex::new(Pending::default()),
            writer: tx,
        });
        let writer = tokio::spawn(write_loop(write, rx));
        let reader = tokio::spawn(read_loop(reader, limit, Arc::clone(&shared)));
        Ok(Self {
            shared,
            hello,
            reader,
            writer,
        })
    }

    /// The server's `hello`.
    #[must_use]
    pub fn server_hello(&self) -> &ServerHello {
        &self.hello
    }

    fn register(&self, call_id: &str, waiter: Waiter) -> Result<CancelOnDrop, WireError> {
        let mut pending = self.shared.lock();
        if pending.ended {
            return Err(WireError::connection_ended(call_id));
        }
        pending.waiters.insert(call_id.to_owned(), waiter);
        Ok(CancelOnDrop {
            shared: Arc::clone(&self.shared),
            call_id: call_id.to_owned(),
            armed: true,
        })
    }

    fn send(&self, call_id: &str, frame: &ClientFrame) -> Result<(), WireError> {
        self.shared
            .writer
            .send(line(frame))
            .map_err(|_| WireError::connection_ended(call_id))
    }

    async fn one_answer(&self, call_id: String, frame: ClientFrame) -> Result<Value, WireError> {
        let (tx, rx) = oneshot::channel();
        let mut guard = self.register(&call_id, Waiter::Call(tx))?;
        self.send(&call_id, &frame)?;
        let answer = rx
            .await
            .unwrap_or_else(|_| Err(WireError::connection_ended(&call_id)));
        guard.armed = false;
        answer
    }

    /// One query or command: its `reply`'s output or error.
    pub async fn call(
        &self,
        operation: &str,
        input: Value,
        options: CallOptions,
    ) -> Result<Value, WireError> {
        let call_id = options.call_id.unwrap_or_else(new_call_id);
        let frame = ClientFrame::Call(Call {
            call_id: call_id.clone(),
            operation: operation.to_owned(),
            input,
            deadline_ms: options.deadline_ms,
            door: options.door,
            detail: options.detail,
        });
        self.one_answer(call_id, frame).await
    }

    /// The catalogue this caller can see through its door.
    pub async fn describe(&self, mut request: Describe) -> Result<DescribeOutput, WireError> {
        if request.call_id.is_empty() {
            request.call_id = new_call_id();
        }
        let call_id = request.call_id.clone();
        let output = self
            .one_answer(call_id, ClientFrame::Describe(request))
            .await?;
        serde_json::from_value(output)
            .map_err(|_| WireError::fixed(Code::Internal, "describe").retryable(false))
    }

    /// Open a stream.
    pub async fn watch(&self, operation: &str, input: Value, options: CallOptions) -> Watching {
        let call_id = options.call_id.unwrap_or_else(new_call_id);
        let (tx, rx) = mpsc::unbounded_channel();
        let frame = ClientFrame::Watch(Watch {
            call_id: call_id.clone(),
            operation: operation.to_owned(),
            input,
            door: options.door,
            detail: options.detail,
        });
        let opened = self
            .register(&call_id, Waiter::Watch(tx))
            .and_then(|guard| self.send(&call_id, &frame).map(|()| guard));
        match opened {
            Ok(guard) => Watching {
                rx: Some(rx),
                failed: None,
                guard: Some(guard),
            },
            Err(error) => Watching {
                rx: None,
                failed: Some(error),
                guard: None,
            },
        }
    }
}

/// A stream in flight. Dropping it before its `end` sends `cancel`.
pub struct Watching {
    rx: Option<mpsc::UnboundedReceiver<Result<Value, WireError>>>,
    failed: Option<WireError>,
    guard: Option<CancelOnDrop>,
}

impl Watching {
    /// The next event; `None` after `end`. An error is given once, then `None`.
    pub async fn next(&mut self) -> Option<Result<Value, WireError>> {
        if let Some(error) = self.failed.take() {
            return Some(Err(error));
        }
        let rx = self.rx.as_mut()?;
        match rx.recv().await {
            Some(Ok(event)) => Some(Ok(event)),
            Some(Err(error)) => {
                self.finish();
                Some(Err(error))
            }
            None => {
                self.finish();
                None
            }
        }
    }

    fn finish(&mut self) {
        self.rx = None;
        if let Some(guard) = self.guard.as_mut() {
            guard.armed = false;
        }
    }
}

async fn write_loop(mut write: OwnedWriteHalf, mut rx: mpsc::UnboundedReceiver<Vec<u8>>) {
    while let Some(bytes) = rx.recv().await {
        if write.write_all(&bytes).await.is_err() {
            return;
        }
    }
}

async fn read_loop(mut reader: BufReader<OwnedReadHalf>, limit: usize, shared: Arc<Shared>) {
    loop {
        let bytes = match read_line(&mut reader, limit).await {
            Ok(Some(bytes)) => bytes,
            Ok(None) | Err(_) => break,
        };
        let Ok(frame) = parse_server(&bytes) else {
            break;
        };
        match frame {
            ServerFrame::Reply(reply) => {
                let waiter = shared.lock().waiters.remove(&reply.call_id);
                if let Some(Waiter::Call(tx)) = waiter {
                    let answer = match reply.error {
                        Some(error) => Err(error),
                        None => Ok(reply.output.unwrap_or(Value::Null)),
                    };
                    let _ = tx.send(answer);
                }
            }
            ServerFrame::Event(event) => {
                let pending = shared.lock();
                if let Some(Waiter::Watch(tx)) = pending.waiters.get(&event.call_id) {
                    let _ = tx.send(Ok(event.output));
                }
            }
            ServerFrame::End(end) => {
                let waiter = shared.lock().waiters.remove(&end.call_id);
                if let Some(Waiter::Watch(tx)) = waiter {
                    if let Some(error) = end.error {
                        let _ = tx.send(Err(error));
                    }
                    // Dropping the sender ends the stream.
                }
            }
            // A close's error is the connection's; the calls in flight are
            // told the connection ended (MA/AD-3 *Errors*).
            ServerFrame::Close(_) => break,
            ServerFrame::Hello(_) => break,
        }
    }
    shared.end();
}
