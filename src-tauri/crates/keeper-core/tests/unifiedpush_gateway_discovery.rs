//! UnifiedPush gateway discovery against a real socket (Epic 98, story 98.4
//! #2; the epic's Q11, D-4).
//!
//! keeper asks the server behind a UnifiedPush endpoint whether it is a
//! Matrix push gateway, and registers it only when it answers so. The risk is
//! in the request as much as in the answer — which address is asked, whether
//! a redirect is followed to a server the person never named, what an
//! unreachable server becomes — so every case here goes through `reqwest`, a
//! TLS connection and a real HTTP response. A gateway is `https://` only, so
//! the fixture speaks TLS under a certificate made for this run and trusted by
//! this test's client alone; the client is otherwise discovery's own
//! (`unifiedpush::discovery_client_from`). The server records every request
//! line it answers, which is how "asked once, at the push path, nothing of
//! the endpoint's path or query" is checked rather than assumed.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use keeper_core::agents::unifiedpush::{
    discover_gateway, discovery_client_from, DiscoveryRefusal, Endpoint,
};
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};

/// The host the endpoint names; the client resolves it to the fixture.
const HOST: &str = "ntfy.test";

/// What the fixture answers to every request.
#[derive(Clone)]
struct Answer {
    status: u16,
    headers: &'static str,
    body: &'static str,
}

fn answer(status: u16, body: &'static str) -> Answer {
    Answer {
        status,
        headers: "Content-Type: application/json\r\n",
        body,
    }
}

/// A TLS server on loopback that answers every request with one [`Answer`]
/// and records each request line.
struct Server {
    addr: SocketAddr,
    certificate: Vec<u8>,
    seen: Arc<Mutex<Vec<String>>>,
}

impl Server {
    async fn start(answer: Answer) -> Server {
        let made =
            rcgen::generate_simple_self_signed(vec![HOST.to_owned()]).expect("a certificate");
        let der = made.cert.der().clone();
        let key = rustls::pki_types::PrivateKeyDer::Pkcs8(made.signing_key.serialize_der().into());
        let config = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::aws_lc_rs::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .expect("versions")
        .with_no_client_auth()
        .with_single_cert(vec![der.clone()], key)
        .expect("a server configuration");
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind loopback");
        let addr = listener.local_addr().expect("the bound address");
        let seen = Arc::new(Mutex::new(Vec::new()));
        let recorder = Arc::clone(&seen);
        tokio::spawn(async move {
            loop {
                let Ok((tcp, _)) = listener.accept().await else {
                    return;
                };
                let acceptor = acceptor.clone();
                let recorder = Arc::clone(&recorder);
                let answer = answer.clone();
                tokio::spawn(async move {
                    let Ok(tls) = acceptor.accept(tcp).await else {
                        return;
                    };
                    let mut reader = BufReader::new(tls);
                    let mut request = String::new();
                    if reader.read_line(&mut request).await.is_err() {
                        return;
                    }
                    loop {
                        let mut line = String::new();
                        match reader.read_line(&mut line).await {
                            Ok(0) | Err(_) => break,
                            Ok(_) if line.trim().is_empty() => break,
                            Ok(_) => {}
                        }
                    }
                    let mut words = request.split_whitespace();
                    let method = words.next().unwrap_or_default();
                    let path = words.next().unwrap_or_default();
                    recorder
                        .lock()
                        .expect("the recorder")
                        .push(format!("{method} {path}"));
                    let response = format!(
                        "HTTP/1.1 {} Answer\r\n{}Content-Length: {}\r\nConnection: close\r\n\r\n{}",
                        answer.status,
                        answer.headers,
                        answer.body.len(),
                        answer.body
                    );
                    let mut tls = reader.into_inner();
                    let _ = tls.write_all(response.as_bytes()).await;
                    let _ = tls.shutdown().await;
                });
            }
        });
        Server {
            addr,
            certificate: der.to_vec(),
            seen,
        }
    }

    /// Discovery's client, trusting only this server's certificate and
    /// resolving [`HOST`] to it.
    fn client(&self) -> reqwest::Client {
        discovery_client_from(
            reqwest::Client::builder()
                .tls_certs_only([
                    reqwest::Certificate::from_der(&self.certificate).expect("the certificate")
                ])
                .resolve(HOST, self.addr),
        )
        .expect("the discovery client builds")
    }

    /// The endpoint a distributor on this server would give: a path and a
    /// query that are its capability.
    fn endpoint(&self) -> Endpoint {
        Endpoint::parse(&format!("https://{HOST}:{}/upAbc?up=1", self.addr.port()))
            .expect("an endpoint")
    }

    fn host(&self) -> String {
        format!("{HOST}:{}", self.addr.port())
    }

    fn requests(&self) -> Vec<String> {
        self.seen.lock().expect("the recorder").clone()
    }
}

const PUSH_PATH: &str = "GET /_matrix/push/v1/notify";

/// The refusal a served answer must come to, given the server's host.
type Refusal = fn(String) -> DiscoveryRefusal;

/// 98.4 #2: a server that answers as a Matrix UnifiedPush gateway is the
/// gateway — its own push path, asked once, with nothing of the endpoint's
/// path or query; every other answer refuses with a sentence naming the
/// host, and no gateway comes back in its place.
#[tokio::test]
async fn unifiedpush_gateway_discovery() {
    let matrix = Server::start(answer(200, r#"{"unifiedpush":{"gateway":"matrix"}}"#)).await;
    let gateway = discover_gateway(&matrix.client(), &matrix.endpoint())
        .await
        .expect("a Matrix gateway");
    assert_eq!(
        gateway.as_str(),
        format!("https://{}/_matrix/push/v1/notify", matrix.host())
    );
    assert_eq!(matrix.requests(), [PUSH_PATH]);

    let cases: [(Answer, Refusal); 3] = [
        (answer(404, r#"{"error":"not found"}"#), |host| {
            DiscoveryRefusal::Status { host, status: 404 }
        }),
        (
            Answer {
                status: 200,
                headers: "Content-Type: text/html\r\n",
                body: "<html>ntfy</html>",
            },
            |host| DiscoveryRefusal::NotMatrix { host },
        ),
        (
            answer(200, r#"{"unifiedpush":{"gateway":"webpush"}}"#),
            |host| DiscoveryRefusal::OtherGateway {
                host,
                kind: "webpush".to_owned(),
            },
        ),
    ];
    for (served, expected) in cases {
        let server = Server::start(served).await;
        let refused = discover_gateway(&server.client(), &server.endpoint())
            .await
            .expect_err("not a Matrix gateway");
        assert_eq!(refused, expected(server.host()));
        assert!(refused.to_string().contains(&server.host()), "{refused}");
        assert_eq!(server.requests(), [PUSH_PATH]);
    }
}

/// D-4: a redirect is the answer, not a pointer to follow — a server the
/// person never named is never asked, let alone registered.
#[tokio::test]
async fn a_redirect_to_another_gateway_is_refused_not_followed() {
    let redirect = Server::start(Answer {
        status: 302,
        headers: "Location: https://gateway.elsewhere.example/_matrix/push/v1/notify\r\n",
        body: "",
    })
    .await;
    assert_eq!(
        discover_gateway(&redirect.client(), &redirect.endpoint()).await,
        Err(DiscoveryRefusal::Status {
            host: redirect.host(),
            status: 302
        })
    );
    assert_eq!(redirect.requests(), [PUSH_PATH]);
}

/// A server nobody answers at is unreachable, named by host.
#[tokio::test]
async fn a_server_that_does_not_answer_is_unreachable_by_name() {
    let closed = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind loopback");
    let port = closed.local_addr().expect("the bound address").port();
    drop(closed);
    let endpoint = Endpoint::parse(&format!("https://{HOST}:{port}/upAbc")).expect("an endpoint");
    let client = discovery_client_from(
        reqwest::Client::builder().resolve(HOST, SocketAddr::from(([127, 0, 0, 1], port))),
    )
    .expect("the discovery client builds");
    assert_eq!(
        discover_gateway(&client, &endpoint).await,
        Err(DiscoveryRefusal::Unreachable {
            host: format!("{HOST}:{port}")
        })
    );
}
