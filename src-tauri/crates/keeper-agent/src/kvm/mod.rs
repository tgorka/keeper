//! The classic NanoKVM, spoken (96.5, AD-409): one frame of its screen over
//! `/api/stream/mjpeg`, inputs over `/api/ws`, a paste and a power press
//! over HTTP — every connection under [`tls::pinned`], so the device's
//! self-signed certificate is accepted only by its configured fingerprint.
//! Each use logs in afresh with the credential the host's secret store
//! holds; nothing of a KVM is ever read from a drive.
//!
//! What to send is keeper-core's ([`keeper_core::agents::kvm::steps`]) and
//! how it is encoded keeper-ported's ([`keeper_ported::nanokvm`]); this
//! module only carries it.

pub mod tls;

use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use keeper_core::agents::kvm::{KvmEntry, KvmKind, Step};
use keeper_ported::nanokvm;
use reqwest::header::{CONTENT_TYPE, COOKIE, SET_COOKIE};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;

/// How long a KVM may take to answer one request, or to send the next
/// bytes of a frame.
pub const READ_WITHIN: Duration = Duration::from_secs(15);
/// How long one frame may take to arrive whole, however its bytes trickle
/// in (R96K1-08): a device that sends a byte just inside [`READ_WITHIN`]
/// forever does not hold the turn.
pub const FRAME_WITHIN: Duration = READ_WITHIN;
/// The most bytes of the MJPEG stream read for one frame.
pub const FRAME_MAX: usize = 16 * 1024 * 1024;
/// The widest and tallest frame keeper decodes: a JPEG whose header claims
/// more is refused before a pixel of it is allocated (R96K1-08).
pub const FRAME_SIDE_MAX: u32 = 8192;

/// One frame of a KVM's screen: the device's JPEG decoded once and kept
/// as the PNG a preview is (the previews' one image kind), that PNG's
/// SHA-256, and its pixels in grey for the region hash.
#[derive(Debug, Clone)]
pub struct Frame {
    pub png: Vec<u8>,
    pub sha256: String,
    pub grey: crate::previews::Grey,
}

/// `jpeg` decoded: its PNG and its grey pixels; `Err` for a frame wider or
/// taller than [`FRAME_SIDE_MAX`], refused by its header.
fn decoded(jpeg: &[u8]) -> Result<(Vec<u8>, crate::previews::Grey), String> {
    use image::ImageEncoder;
    let mut reader = image::ImageReader::new(std::io::Cursor::new(jpeg))
        .with_guessed_format()
        .map_err(|error| error.to_string())?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(FRAME_SIDE_MAX);
    limits.max_image_height = Some(FRAME_SIDE_MAX);
    reader.limits(limits);
    let image = reader.decode().map_err(|error| error.to_string())?;
    let (width, height) = (image.width(), image.height());
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new_with_quality(
        &mut png,
        image::codecs::png::CompressionType::Fast,
        image::codecs::png::FilterType::NoFilter,
    )
    .write_image(
        image.to_rgb8().as_raw(),
        width,
        height,
        image::ExtendedColorType::Rgb8,
    )
    .map_err(|error| error.to_string())?;
    let grey = crate::previews::Grey {
        pixels: image.to_luma8().into_raw(),
        width,
        height,
    };
    Ok((png, grey))
}

/// The first whole JPEG of the MJPEG stream `chunks` carries from `id`:
/// read up to [`FRAME_MAX`] bytes, each scanned once, all within `within`.
async fn first_frame<B: AsRef<[u8]>>(
    id: &str,
    chunks: impl futures_util::Stream<Item = Result<B, String>>,
    within: Duration,
) -> Result<Vec<u8>, String> {
    let reading = async {
        let mut chunks = std::pin::pin!(chunks);
        let mut stream: Vec<u8> = Vec::new();
        let mut cut = nanokvm::JpegCut::default();
        loop {
            if let Some(jpeg) = cut.find(&stream) {
                stream.truncate(jpeg.end);
                stream.drain(..jpeg.start);
                return Ok(stream);
            }
            if stream.len() > FRAME_MAX {
                return Err(format!(
                    "`{id}` sent more than {FRAME_MAX} bytes without one whole frame"
                ));
            }
            match chunks.next().await {
                Some(Ok(chunk)) => stream.extend_from_slice(chunk.as_ref()),
                Some(Err(error)) => return Err(failed(id, "snapshot", error)),
                None => return Err(format!("`{id}` ended its stream before one whole frame")),
            }
        }
    };
    tokio::time::timeout(within, reading).await.map_err(|_| {
        format!(
            "`{id}` did not send one whole frame within {} seconds",
            within.as_secs()
        )
    })?
}

/// Why [`Classic::send`] stopped: keeper's sentence, and whether an input
/// may have reached the target before it did — then what was sent may have
/// taken effect, and the call is never a plain refusal (R96K1-07).
#[derive(Debug)]
pub struct SendFailed {
    pub reason: String,
    pub reached: bool,
}

/// The KVMs a host names, each with its credential as the host's secret
/// store resolved it (`None`: not resolved, so not reachable).
pub struct Kvms {
    kvms: Vec<(KvmEntry, Option<String>)>,
}

impl Kvms {
    pub fn new(kvms: Vec<(KvmEntry, Option<String>)>) -> Kvms {
        Kvms { kvms }
    }

    /// The `[[kvm]]` entry named `id`.
    pub fn entry(&self, id: &str) -> Option<&KvmEntry> {
        self.kvms
            .iter()
            .map(|(entry, _)| entry)
            .find(|entry| entry.id == id)
    }

    /// The ids of the KVMs of `kind` this host names.
    pub fn ids(&self, kind: KvmKind) -> impl Iterator<Item = &str> {
        self.kvms
            .iter()
            .filter(move |(entry, _)| entry.kind == kind)
            .map(|(entry, _)| entry.id.as_str())
    }

    /// The classic NanoKVM named `id`, ready to be spoken to.
    pub fn classic(&self, id: &str) -> Result<Classic, String> {
        let Some((entry, password)) = self.kvms.iter().find(|(entry, _)| entry.id == id) else {
            return Err(format!("this host names no KVM `{id}`"));
        };
        if entry.kind != KvmKind::NanoKvm {
            return Err(format!(
                "`{id}` is a NanoKVM-Go: it is reached through the MCP server whose role is \"kvm:{id}\", not through kvm_snapshot or kvm_act"
            ));
        }
        let password = password.clone().ok_or_else(|| {
            format!("the credential of `{id}` is not in this host's secret store")
        })?;
        Ok(Classic {
            entry: entry.clone(),
            password,
        })
    }
}

/// A classic NanoKVM and its password.
#[derive(Clone)]
pub struct Classic {
    pub entry: KvmEntry,
    password: String,
}

/// What a request to the KVM came to, in keeper's words: never the
/// device's body, which is outside content.
fn failed(id: &str, what: &str, error: impl std::fmt::Display) -> String {
    format!("`{id}` could not be reached ({what}): {error}")
}

impl Classic {
    fn url(&self, path: &str) -> Result<url::Url, String> {
        url::Url::parse(&self.entry.url)
            .and_then(|base| base.join(path))
            .map_err(|_| format!("the `url` of `{}` does not take a path", self.entry.id))
    }

    /// A pinned client, and the error sentence a refused certificate gets.
    fn client(&self) -> Result<(reqwest::Client, Arc<tls::Pinned>), String> {
        tls::client(&self.entry.fingerprint, READ_WITHIN)
    }

    fn explained(&self, pinned: &tls::Pinned, what: &str, error: impl std::fmt::Display) -> String {
        match pinned.refused() {
            Some(refusal) => format!("`{}` was not trusted: {refusal}", self.entry.id),
            None => failed(&self.entry.id, what, error),
        }
    }

    /// Log in and return the session's token (the reference's cookie).
    async fn login(
        &self,
        client: &reqwest::Client,
        pinned: &tls::Pinned,
    ) -> Result<String, String> {
        let iv = rand_iv()?;
        let body = nanokvm::login_body(
            self.entry
                .username
                .as_deref()
                .unwrap_or(keeper_core::agents::kvm::DEFAULT_USERNAME),
            &nanokvm::login_password(&self.password, iv),
        );
        let answer = client
            .post(self.url(nanokvm::LOGIN_PATH)?)
            .header(CONTENT_TYPE, "application/json")
            .body(body)
            .send()
            .await
            .map_err(|error| self.explained(pinned, "login", error))?;
        if !answer.status().is_success() {
            return Err(failed(&self.entry.id, "login", answer.status()));
        }
        let token = answer
            .headers()
            .get_all(SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .find_map(|cookie| {
                cookie
                    .split(';')
                    .next()?
                    .trim()
                    .strip_prefix(nanokvm::TOKEN_COOKIE)?
                    .strip_prefix('=')
                    .map(str::to_owned)
            });
        token
            .filter(|token| !token.is_empty())
            .ok_or_else(|| format!("`{}` refused the login", self.entry.id))
    }

    fn cookie(token: &str) -> String {
        format!("{}={token}", nanokvm::TOKEN_COOKIE)
    }

    /// One frame of the screen: the first whole JPEG of the MJPEG stream,
    /// read up to [`FRAME_MAX`] within [`FRAME_WITHIN`], decoded to grey.
    pub async fn snapshot(&self) -> Result<Frame, String> {
        let (client, pinned) = self.client()?;
        let token = self.login(&client, &pinned).await?;
        let answer = client
            .get(self.url(nanokvm::MJPEG_PATH)?)
            .header(COOKIE, Self::cookie(&token))
            .send()
            .await
            .map_err(|error| self.explained(&pinned, "snapshot", error))?;
        if !answer.status().is_success() {
            return Err(failed(&self.entry.id, "snapshot", answer.status()));
        }
        let chunks = futures_util::stream::unfold(answer, |mut answer| async move {
            match answer.chunk().await {
                Ok(Some(chunk)) => Some((Ok(chunk), answer)),
                Ok(None) => None,
                Err(error) => Some((Err(error.to_string()), answer)),
            }
        });
        let jpeg = first_frame(&self.entry.id, chunks, FRAME_WITHIN).await?;
        let (png, grey) = decoded(&jpeg).map_err(|why| {
            format!(
                "`{}` sent a frame that does not decode: {why}",
                self.entry.id
            )
        })?;
        Ok(Frame {
            sha256: keeper_core::agents::approval::sha256_hex(&png),
            png,
            grey,
        })
    }

    /// Send `steps` in order: frames over one WebSocket, a paste or a power
    /// press over HTTP between them, the socket closed at the end. `Err`
    /// says whether any input may have reached the target: once an input
    /// was put on the wire, it may have.
    pub async fn send(&self, steps: &[Step]) -> Result<(), SendFailed> {
        let unsent = |reason: String| SendFailed {
            reason,
            reached: false,
        };
        let (client, pinned) = self.client().map_err(unsent)?;
        let token = self.login(&client, &pinned).await.map_err(unsent)?;
        let id = &self.entry.id;
        let mut socket = None;
        // Before step `sent` was put on the wire.
        let before = |sent: usize, reason: String| SendFailed {
            reason,
            reached: sent > 0,
        };
        // While step `sent` was on its way.
        let during = |reason: String| SendFailed {
            reason,
            reached: true,
        };
        for (sent, step) in steps.iter().enumerate() {
            match step {
                Step::Frame(frame) => {
                    if socket.is_none() {
                        socket = Some(
                            self.open_socket(&token)
                                .await
                                .map_err(|error| before(sent, error))?,
                        );
                    }
                    if let Some(ws) = socket.as_mut() {
                        ws.send(Message::text(frame.clone()))
                            .await
                            .map_err(|error| during(failed(id, "input", error)))?;
                    }
                }
                Step::Paste(body) | Step::Power(body) => {
                    let path = if matches!(step, Step::Paste(_)) {
                        nanokvm::PASTE_PATH
                    } else {
                        nanokvm::GPIO_PATH
                    };
                    let url = self.url(path).map_err(|error| before(sent, error))?;
                    let answer = client
                        .post(url)
                        .header(COOKIE, Self::cookie(&token))
                        .header(CONTENT_TYPE, "application/json")
                        .body(body.clone())
                        .send()
                        .await
                        .map_err(|error| during(self.explained(&pinned, "input", error)))?;
                    if !answer.status().is_success() {
                        return Err(during(failed(id, "input", answer.status())));
                    }
                }
            }
        }
        if let Some(mut ws) = socket {
            // The frames are on the wire; a close that fails takes nothing
            // back.
            let _ = ws.close(None).await;
            while let Ok(Some(_)) =
                tokio::time::timeout(Duration::from_millis(200), ws.next()).await
            {}
        }
        Ok(())
    }

    async fn open_socket(
        &self,
        token: &str,
    ) -> Result<
        tokio_tungstenite::WebSocketStream<tokio_rustls::client::TlsStream<tokio::net::TcpStream>>,
        String,
    > {
        let id = &self.entry.id;
        let mut url = self.url(nanokvm::WS_PATH)?;
        let _ = url.set_scheme("wss");
        let host = url
            .host_str()
            .ok_or_else(|| format!("the `url` of `{id}` names no host"))?
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_owned();
        let port = url.port_or_known_default().unwrap_or(443);
        let (config, pinned) = tls::pinned(&self.entry.fingerprint)?;
        let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
        let name = rustls::pki_types::ServerName::try_from(host.clone())
            .map_err(|_| format!("the host of `{id}` is not a name TLS takes"))?;
        let tcp = tokio::time::timeout(
            READ_WITHIN,
            tokio::net::TcpStream::connect((host.as_str(), port)),
        )
        .await
        .map_err(|_| failed(id, "input", "it did not answer in time"))?
        .map_err(|error| failed(id, "input", error))?;
        let stream = tokio::time::timeout(READ_WITHIN, connector.connect(name, tcp))
            .await
            .map_err(|_| failed(id, "input", "it did not answer in time"))?
            .map_err(|error| match pinned.refused() {
                Some(refusal) => format!("`{id}` was not trusted: {refusal}"),
                None => failed(id, "input", error),
            })?;
        let mut request = url
            .as_str()
            .into_client_request()
            .map_err(|error| failed(id, "input", error))?;
        request.headers_mut().insert(
            COOKIE,
            Self::cookie(token)
                .parse()
                .map_err(|_| format!("`{id}` gave a token no header can carry"))?,
        );
        let (ws, _) = tokio::time::timeout(
            READ_WITHIN,
            tokio_tungstenite::client_async(request, stream),
        )
        .await
        .map_err(|_| failed(id, "input", "it did not answer in time"))?
        .map_err(|error| failed(id, "input", error))?;
        Ok(ws)
    }
}

/// A fresh IV for the login's obfuscation, from the OS's generator.
fn rand_iv() -> Result<[u8; 16], String> {
    let mut iv = [0u8; 16];
    getrandom::fill(&mut iv).map_err(|error| format!("no random IV for the login: {error}"))?;
    Ok(iv)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A grey JPEG `width` × `height`.
    fn jpeg(width: u32, height: u32) -> Vec<u8> {
        let mut out = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 80)
            .encode(
                &vec![128u8; (width * height) as usize],
                width,
                height,
                image::ExtendedColorType::L8,
            )
            .expect("a JPEG");
        out
    }

    /// R96K1-08: a frame as wide or as tall as [`FRAME_SIDE_MAX`] decodes;
    /// one pixel more on either side is refused by its header.
    #[test]
    fn kvm_a_frame_past_the_side_bound_is_refused() {
        for (width, height) in [(FRAME_SIDE_MAX, 8), (8, FRAME_SIDE_MAX)] {
            let (_, grey) = decoded(&jpeg(width, height)).expect("decodes");
            assert_eq!((grey.width, grey.height), (width, height));
        }
        for (width, height) in [(FRAME_SIDE_MAX + 1, 8), (8, FRAME_SIDE_MAX + 1)] {
            assert!(decoded(&jpeg(width, height)).is_err(), "{width}×{height}");
        }
    }

    /// R96K1-08: one deadline for the whole frame. A stream whose bytes
    /// each arrive in time but whose frame is whole only after the
    /// deadline is refused; the same stream inside it is read, its frame
    /// cut out whole.
    #[tokio::test]
    async fn kvm_a_frame_trickled_past_its_deadline_is_refused() {
        let mut bytes = b"--frame\r\n\r\n".to_vec();
        let frame = jpeg(16, 8);
        bytes.extend_from_slice(&frame);
        bytes.extend_from_slice(b"\r\n");
        let trickle = |bytes: Vec<u8>, gap: Duration| {
            futures_util::stream::iter(bytes.chunks(64).map(<[u8]>::to_vec).collect::<Vec<_>>())
                .then(move |chunk| async move {
                    tokio::time::sleep(gap).await;
                    Ok::<_, String>(chunk)
                })
        };
        let chunks = bytes.len().div_ceil(64) as u32;
        let gap = Duration::from_millis(40);
        assert!(chunks >= 6, "{chunks} chunks");
        let late = first_frame("desk", trickle(bytes.clone(), gap), gap * 3).await;
        assert!(late.is_err());
        let read = first_frame("desk", trickle(bytes, gap), gap * (chunks + 20)).await;
        assert_eq!(read, Ok(frame));
    }

    /// 96.5 #4: no path in keeper-agent switches certificate verification
    /// off — not reqwest's switch, not rustls' accept-anything verifier —
    /// so a KVM is trusted only through [`super::tls::pinned`].
    #[test]
    fn kvm_never_disables_tls_verification() {
        let banned = [
            concat!("danger_accept_", "invalid_certs"),
            concat!("danger_accept_", "invalid_hostnames"),
            concat!("NoServer", "Verification"),
            concat!("tls_danger_accept_", "invalid"),
        ];
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut found = Vec::new();
        let mut dirs = vec![root.join("src"), root.join("tests")];
        let mut read = 0usize;
        while let Some(dir) = dirs.pop() {
            for entry in std::fs::read_dir(&dir).expect("readable").flatten() {
                let path = entry.path();
                if path.is_dir() {
                    dirs.push(path);
                } else if path.extension().is_some_and(|ext| ext == "rs") {
                    read += 1;
                    let text = std::fs::read_to_string(&path).expect("readable");
                    for word in banned {
                        if text.contains(word) {
                            found.push(format!("{}: {word}", path.display()));
                        }
                    }
                }
            }
        }
        assert!(read > 20, "the scan read keeper-agent's sources");
        assert_eq!(found, Vec::<String>::new());
    }
}
