//! A KVM's TLS (AD-409): its certificate is self-signed, so no authority
//! vouches for it; keeper accepts it only when the SHA-256 of the
//! certificate the server presents is the `fingerprint` the host's
//! configuration pins, and the handshake's signature is that certificate's
//! key's. Anything else is refused with a sentence naming both
//! fingerprints. Verification is never switched off: this verifier is the
//! only one, written with rustls' `dangerous()` builder because that is how
//! a custom verifier is installed.

use std::sync::{Arc, Mutex};

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{verify_tls12_signature, verify_tls13_signature, CryptoProvider};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, SignatureScheme};

/// What a refused certificate is told: the one it presented and the one
/// the configuration pins.
pub fn mismatch(presented: &str, pinned: &str) -> String {
    format!(
        "the server presented the certificate {presented}, and the configured fingerprint is {pinned}; keeper does not connect to it"
    )
}

/// The verifier: the pinned fingerprint, and the one the server last
/// presented, so a refusal can name both.
#[derive(Debug)]
pub struct Pinned {
    pinned: String,
    presented: Mutex<Option<String>>,
    provider: Arc<CryptoProvider>,
}

impl Pinned {
    /// Why the last handshake was refused, when a certificate was presented
    /// and was not the pinned one.
    pub fn refused(&self) -> Option<String> {
        self.presented
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .filter(|presented| **presented != self.pinned)
            .map(|presented| mismatch(presented, &self.pinned))
    }
}

impl ServerCertVerifier for Pinned {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let presented = format!(
            "sha256:{}",
            keeper_core::agents::approval::sha256_hex(end_entity.as_ref())
        );
        let held = presented == self.pinned;
        let refusal = mismatch(&presented, &self.pinned);
        *self.presented.lock().unwrap_or_else(|p| p.into_inner()) = Some(presented);
        if held {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::General(refusal))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

/// A TLS client configuration that accepts exactly the certificate whose
/// SHA-256 is `fingerprint` (`sha256:` and 64 hex digits, any case), and
/// its verifier, which says why a handshake was refused.
pub fn pinned(fingerprint: &str) -> Result<(ClientConfig, Arc<Pinned>), String> {
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let verifier = Arc::new(Pinned {
        pinned: fingerprint.to_ascii_lowercase(),
        presented: Mutex::new(None),
        provider: Arc::clone(&provider),
    });
    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|error| format!("keeper's TLS could not be set up: {error}"))?
        .dangerous()
        .with_custom_certificate_verifier(Arc::clone(&verifier) as Arc<dyn ServerCertVerifier>)
        .with_no_client_auth();
    Ok((config, verifier))
}

/// An HTTP client over [`pinned`] TLS, as keeper's other clients are built:
/// a connect and a read bound, no redirect followed — a 302 would carry the
/// credential to a host nobody pinned — and `https` only: a pin over plain
/// HTTP would pin nothing.
pub fn client(
    fingerprint: &str,
    read_timeout: std::time::Duration,
) -> Result<(reqwest::Client, Arc<Pinned>), String> {
    let (config, verifier) = pinned(fingerprint)?;
    let client = reqwest::Client::builder()
        .tls_backend_preconfigured(config)
        .connect_timeout(std::time::Duration::from_secs(10))
        .read_timeout(read_timeout)
        .redirect(reqwest::redirect::Policy::none())
        .https_only(true)
        .build()
        .map_err(|error| format!("keeper's HTTP client could not be built: {error}"))?;
    Ok((client, verifier))
}
