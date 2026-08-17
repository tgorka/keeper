//! git's pkt-line framing, as much of it as a filter process needs (DW-140).
//!
//! Normative source: `git/Documentation/gitprotocol-common.txt`, section
//! "pkt-line Format".
//!
//! A packet is a four-digit lower-case hexadecimal length followed by that many
//! bytes *including the four length digits themselves*. `0000` is the flush
//! packet: a zero-length delimiter that ends a list. So the shortest data packet
//! is `0005x` and the longest carries [`MAX_DATA`] bytes.
//!
//! # Why this lives in keeper rather than being borrowed
//!
//! gitoxide has `gix-packetline`, and it is a good implementation — but it is
//! built around the *transport* dialect: sidebands, delimiter packets, response
//! ends, and a reader that wants to own the stream. The filter dialect uses
//! none of that, and the one thing it does need — treating "the pipe closed
//! cleanly between packets" as the ordinary way a conversation ends rather than
//! as a truncation error — is exactly the case a transport parser is right to
//! reject. Sixty lines that say only what the filter protocol says beats
//! bending a transport reader into a shape it was not meant to take.

use std::io::{Read, Write};

use crate::error::{Result, SyncError};

/// The largest payload one packet may carry.
///
/// 65520 is the largest length the four hex digits can express in the dialect
/// git actually uses (`ffff` is reserved), and four of those bytes are the
/// length prefix itself.
pub const MAX_DATA: usize = 65516;

/// The four bytes that end a list.
const FLUSH: &[u8; 4] = b"0000";

/// One thing read off the wire.
#[derive(Debug, PartialEq, Eq)]
pub enum Packet {
    /// A payload. Not necessarily text: content packets carry raw bytes.
    Data(Vec<u8>),
    /// `0000` — the end of the current list.
    Flush,
    /// The peer closed the pipe between packets.
    ///
    /// A distinct answer rather than an error because it is how git says "no
    /// more files, shut down" — the protocol has no goodbye packet. Mid-packet
    /// EOF stays an error, because that one *is* a truncation.
    Eof,
}

/// Read one packet.
pub fn read(input: &mut impl Read) -> Result<Packet> {
    let mut header = [0u8; 4];
    match read_exact_or_eof(input, &mut header)? {
        // Nothing at all where a length was due: the ordinary end.
        0 => return Ok(Packet::Eof),
        4 => {}
        got => {
            return Err(SyncError::Git(format!(
                "truncated pkt-line header: {got} of 4 bytes"
            )))
        }
    }
    if &header == FLUSH {
        return Ok(Packet::Flush);
    }
    let text = std::str::from_utf8(&header)
        .map_err(|_| SyncError::Git("pkt-line length is not ASCII".into()))?;
    let length = usize::from_str_radix(text, 16)
        .map_err(|_| SyncError::Git(format!("pkt-line length {text:?} is not hexadecimal")))?;
    if length < 4 {
        // 0001..0003 are delimiter packets from the transport dialect, which
        // the filter protocol never sends. Refusing beats guessing.
        return Err(SyncError::Git(format!("unusable pkt-line length {length}")));
    }
    let mut payload = vec![0u8; length - 4];
    let got = read_exact_or_eof(input, &mut payload)?;
    if got != payload.len() {
        return Err(SyncError::Git(format!(
            "truncated pkt-line payload: {got} of {} bytes",
            payload.len()
        )));
    }
    Ok(Packet::Data(payload))
}

/// Read every packet up to the next flush, as text.
///
/// The metadata lists git sends — the handshake, a command's keys — are always
/// short and always `key=value\n`, so collecting them whole is bounded. Content
/// never comes through here; it streams.
pub fn read_text_list(input: &mut impl Read) -> Result<Option<Vec<String>>> {
    let mut lines = Vec::new();
    loop {
        match read(input)? {
            Packet::Flush => return Ok(Some(lines)),
            // EOF before the list closed is only legal when the list had not
            // started, which is how git shuts a filter down.
            Packet::Eof if lines.is_empty() => return Ok(None),
            Packet::Eof => return Err(SyncError::Git("pkt-line list ended mid-way".into())),
            Packet::Data(bytes) => lines.push(
                String::from_utf8(bytes)
                    .map_err(|_| SyncError::Git("pkt-line metadata is not UTF-8".into()))?
                    .trim_end_matches('\n')
                    .to_owned(),
            ),
        }
    }
}

/// Write one data packet. Payload must be at most [`MAX_DATA`].
pub fn write_data(output: &mut impl Write, payload: &[u8]) -> Result<()> {
    debug_assert!(payload.len() <= MAX_DATA);
    let header = format!("{:04x}", payload.len() + 4);
    output
        .write_all(header.as_bytes())
        .and_then(|()| output.write_all(payload))
        .map_err(|err| SyncError::Git(format!("could not write a pkt-line: {err}")))
}

/// Write one `key=value` metadata packet, newline included.
pub fn write_line(output: &mut impl Write, line: &str) -> Result<()> {
    write_data(output, format!("{line}\n").as_bytes())
}

/// Write the flush packet that ends a list.
pub fn write_flush(output: &mut impl Write) -> Result<()> {
    output
        .write_all(FLUSH)
        .map_err(|err| SyncError::Git(format!("could not write a pkt-line flush: {err}")))
}

/// `read_exact` that reports a clean EOF instead of failing on one.
fn read_exact_or_eof(input: &mut impl Read, buffer: &mut [u8]) -> Result<usize> {
    let mut filled = 0;
    while filled < buffer.len() {
        match input.read(&mut buffer[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => {}
            Err(err) => return Err(SyncError::Git(format!("could not read a pkt-line: {err}"))),
        }
    }
    Ok(filled)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flush_is_told_from_a_payload() {
        let mut wire: &[u8] = b"0000";
        assert_eq!(read(&mut wire).expect("flush"), Packet::Flush);

        let mut wire: &[u8] = b"000ahello\n";
        assert_eq!(
            read(&mut wire).expect("data"),
            Packet::Data(b"hello\n".to_vec())
        );
    }

    /// The pipe closing between packets is how git ends the conversation, and
    /// treating it as an error would make every clean shutdown look like a
    /// crash — which is precisely the confusion that lets a filter failure be
    /// mistaken for a filter that was never configured.
    #[test]
    fn a_closed_pipe_between_packets_is_not_an_error() {
        let mut wire: &[u8] = b"";
        assert_eq!(read(&mut wire).expect("eof"), Packet::Eof);
        assert_eq!(read_text_list(&mut wire).expect("eof"), None);
    }

    /// Mid-packet EOF is the one that really is a truncation.
    #[test]
    fn a_pipe_that_dies_mid_packet_is_an_error() {
        let mut wire: &[u8] = b"0010short";
        assert!(read(&mut wire).is_err());
        let mut wire: &[u8] = b"00";
        assert!(read(&mut wire).is_err());
    }

    /// The reader must stop *exactly* at the flush. Reading one byte past it
    /// consumes the next command's header, and the conversation never recovers.
    #[test]
    fn a_metadata_list_stops_at_its_flush() {
        let mut wire = Vec::new();
        write_line(&mut wire, "command=smudge").expect("command");
        write_line(&mut wire, "pathname=a/b.mov").expect("pathname");
        write_flush(&mut wire).expect("flush");
        wire.extend_from_slice(b"after");

        let mut wire = wire.as_slice();
        assert_eq!(
            read_text_list(&mut wire).expect("list"),
            Some(vec![
                "command=smudge".to_owned(),
                "pathname=a/b.mov".to_owned()
            ])
        );
        let mut rest = Vec::new();
        wire.read_to_end(&mut rest).expect("rest");
        assert_eq!(rest, b"after");
    }

    #[test]
    fn what_is_written_reads_back() {
        let mut wire = Vec::new();
        write_line(&mut wire, "status=success").expect("line");
        write_flush(&mut wire).expect("flush");
        write_data(&mut wire, b"\x00\xffbinary").expect("data");
        write_flush(&mut wire).expect("flush");

        let mut wire = wire.as_slice();
        assert_eq!(
            read_text_list(&mut wire).expect("list"),
            Some(vec!["status=success".to_owned()])
        );
        assert_eq!(
            read(&mut wire).expect("data"),
            Packet::Data(b"\x00\xffbinary".to_vec())
        );
        assert_eq!(read(&mut wire).expect("flush"), Packet::Flush);
    }

    /// The lengths are the protocol. A four-digit header that says 0x000a must
    /// mean six bytes of payload, not ten.
    #[test]
    fn the_length_counts_its_own_header() {
        let mut wire = Vec::new();
        write_data(&mut wire, b"abcdef").expect("data");
        assert_eq!(&wire[..4], b"000a");
        assert_eq!(wire.len(), 10);
    }
}
