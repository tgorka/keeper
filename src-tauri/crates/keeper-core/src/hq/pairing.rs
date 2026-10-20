//! Pairing a device (AD-422): a signed-in device asks its server for a
//! one-time code, and the new device redeems it for a token of its own.
//!
//! The code is 24 characters of Crockford's base32 alphabet (no `I`, `L`,
//! `O`, `U`). A person reads it off one screen and types it on another, so the
//! grammar forgives what typing does — case, spaces, the dashes keeper shows
//! it in, and the letters a person reads as digits — and refuses the rest with
//! one sentence. The QR keeper draws is a [`pairing_link`], which carries the
//! server's address too, so a scanned code needs no address typed first.

use crate::hq::server::ServerUrl;

/// How many characters a code has.
pub const CODE_LEN: usize = 24;

/// The alphabet a code is drawn from.
const ALPHABET: &str = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// The scheme and path of the link the QR carries.
const LINK_PREFIX: &str = "keeper://agents/pair";

/// A code keeper will redeem, canonical (upper case, no separators).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingCode(String);

impl PairingCode {
    /// The code as the server issued it.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The code in groups of four, as a screen shows it.
    pub fn grouped(&self) -> String {
        self.0
            .as_bytes()
            .chunks(4)
            .map(|chunk| String::from_utf8_lossy(chunk).into_owned())
            .collect::<Vec<_>>()
            .join("-")
    }

    fn parse_code(raw: &str) -> Result<PairingCode, CodeRefusal> {
        let mut code = String::with_capacity(CODE_LEN);
        for c in raw.chars() {
            if c.is_whitespace() || c == '-' {
                continue;
            }
            let c = match c.to_ascii_uppercase() {
                'O' => '0',
                'I' | 'L' => '1',
                other => other,
            };
            if !ALPHABET.contains(c) {
                return Err(CodeRefusal::NotACode);
            }
            code.push(c);
        }
        if code.chars().count() != CODE_LEN {
            return Err(CodeRefusal::Length {
                got: code.chars().count(),
            });
        }
        Ok(PairingCode(code))
    }
}

/// What a typed or scanned code says: the code, and the server when it came
/// from a [`pairing_link`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redeemable {
    pub code: PairingCode,
    pub server: Option<ServerUrl>,
}

/// Read what the person typed or scanned: a code, or the link a QR carries.
pub fn parse(raw: &str) -> Result<Redeemable, CodeRefusal> {
    let trimmed = raw.trim();
    if let Some(rest) = trimmed.strip_prefix(LINK_PREFIX) {
        let query = rest.strip_prefix('?').ok_or(CodeRefusal::NotACode)?;
        let link = url::form_urlencoded::parse(query.as_bytes());
        let (mut code, mut server) = (None, None);
        for (key, value) in link {
            match key.as_ref() {
                "code" => code = Some(PairingCode::parse_code(&value)?),
                "server" => {
                    server = ServerUrl::parse(&value).map_err(|_| CodeRefusal::NotACode)?;
                }
                _ => {}
            }
        }
        return Ok(Redeemable {
            code: code.ok_or(CodeRefusal::NotACode)?,
            server: Some(server.ok_or(CodeRefusal::NotACode)?),
        });
    }
    Ok(Redeemable {
        code: PairingCode::parse_code(trimmed)?,
        server: None,
    })
}

/// The link a QR carries: the server's address and the code.
pub fn pairing_link(server: &ServerUrl, code: &PairingCode) -> String {
    let query: String = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("server", server.as_str())
        .append_pair("code", code.as_str())
        .finish();
    format!("{LINK_PREFIX}?{query}")
}

/// Why what was typed is not a code. The `Display` text is the sentence the
/// field shows.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CodeRefusal {
    /// A character no code holds, or a link that is not keeper's.
    #[error(
        "That is not a pairing code. Open Settings › Agents › Add a device on a device that \
         is paired, and type the code it shows."
    )]
    NotACode,
    /// The wrong number of characters.
    #[error("A pairing code has 24 characters; this has {got}.")]
    Length {
        /// How many code characters were typed.
        got: usize,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    const CODE: &str = "0123456789ABCDEFGHJKMNPQ";

    #[test]
    fn a_typed_code_forgives_case_separators_and_look_alikes() {
        let typed = "0i23-4567 89ab-cdef-ghjk-mnpq";
        let read = parse(typed).expect("a code");
        assert_eq!(read.code.as_str(), CODE);
        assert_eq!(read.server, None);
        assert_eq!(read.code.grouped(), "0123-4567-89AB-CDEF-GHJK-MNPQ");
        assert_eq!(
            parse(&read.code.grouped()).expect("round trip").code,
            read.code
        );
        // `O` is read as zero, `L` as one.
        assert_eq!(
            parse("O123456789ABCDEFGHJKMNPQ")
                .expect("a code")
                .code
                .as_str(),
            CODE
        );
    }

    #[test]
    fn each_refusal_says_why() {
        assert_eq!(parse("0123"), Err(CodeRefusal::Length { got: 4 }));
        assert_eq!(
            parse(&format!("{CODE}0")),
            Err(CodeRefusal::Length { got: 25 })
        );
        // `U` is not in the alphabet.
        assert_eq!(
            parse("U123456789ABCDEFGHJKMNPQ"),
            Err(CodeRefusal::NotACode)
        );
        assert_eq!(parse("keeper://agents/pair"), Err(CodeRefusal::NotACode));
        assert_eq!(
            parse(&format!("keeper://agents/pair?code={CODE}")),
            Err(CodeRefusal::NotACode),
            "a link names its server"
        );
    }

    #[test]
    fn the_qr_link_carries_the_server_and_the_code() {
        let server = ServerUrl::parse("http://100.64.0.7:8790")
            .expect("parses")
            .expect("an address");
        let code = parse(CODE).expect("a code").code;
        let link = pairing_link(&server, &code);
        assert!(link.starts_with("keeper://agents/pair?"), "{link}");
        let read = parse(&link).expect("the link");
        assert_eq!(read.code, code);
        assert_eq!(read.server, Some(server));
    }
}
