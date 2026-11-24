//! Resumable range download from the object store (story 115.3): streams to
//! `<dest>.part`, resumes by `Range`, renews an expired URL, verifies SHA-256
//! streaming, and renames to `dest`.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use keeper_ports::transfer::{Downloaded, Progress};
use keeper_ports::ReplicaError;
use sha2::{Digest, Sha256};

use super::StoreTransfer;

const CHUNK_BYTES: usize = 1024 * 1024;
const MAX_GRANTS: u32 = 3;
const MAX_STALLED_ATTEMPTS: u32 = 5;

/// Download `content_id` (`size` bytes) to `dest` via presigned store URLs.
pub(super) fn run(
    t: &StoreTransfer,
    content_id: &str,
    size: u64,
    dest: &Path,
    progress: &mut dyn FnMut(Progress),
) -> Result<Downloaded, ReplicaError> {
    let expected_hex = parse_content_id(content_id)?;
    let temp_path = part_path(dest)?;

    if let Some(parent) = dest.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent).map_err(io_client_err)?;
    }

    let mut file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&temp_path)
        .map_err(io_client_err)?;

    let mut from = file.metadata().map_err(io_client_err)?.len();
    if from > size {
        truncate_to_zero(&mut file)?;
        from = 0;
    } else if from > 0 {
        file.seek(SeekFrom::End(0)).map_err(io_client_err)?;
    }

    let grant = t.grants.grant_download(content_id)?;
    let mut grants: u32 = 1;
    let mut url = t.checked(&grant.url)?;

    let mut bytes_fetched: u64 = 0;
    let mut stalled_attempts: u32 = 0;
    let mut buf = vec![0u8; CHUNK_BYTES];

    while from < size {
        let mut req = t.agent.get(url.as_str());
        if from > 0 && from < size {
            req = req.header("Range", format!("bytes={from}-"));
        }

        let mut response = match req.call() {
            Ok(response) => response,
            Err(error) => {
                stalled_attempts += 1;
                tracing::debug!(
                    %content_id,
                    from,
                    stalled_attempts,
                    %error,
                    "download request transport error"
                );
                if stalled_attempts >= MAX_STALLED_ATTEMPTS {
                    return Err(transport_failed(format!(
                        "download request failed: {error}"
                    )));
                }
                continue;
            }
        };

        let status = response.status().as_u16();
        match status {
            403 => {
                if grants >= MAX_GRANTS {
                    return Err(download_refused(
                        status,
                        "presigned download URL expired after maximum grant renewals".to_owned(),
                    ));
                }
                tracing::debug!(%content_id, grants, "presigned download URL expired; renewing");
                let fresh = t.grants.grant_download(content_id)?;
                grants += 1;
                url = t.checked(&fresh.url)?;
                continue;
            }
            416 => {
                if from == size {
                    break;
                }
                return Err(download_refused(
                    status,
                    format!("store refused range from {from} of {size} (HTTP 416)"),
                ));
            }
            206 => {
                let valid_range = response
                    .headers()
                    .get("content-range")
                    .and_then(|v| v.to_str().ok())
                    .and_then(parse_content_range)
                    .is_some_and(|range| {
                        range.start == from && range.end < size && range.total == size
                    });
                if !valid_range {
                    truncate_to_zero(&mut file)?;
                    from = 0;
                    stalled_attempts += 1;
                    if stalled_attempts >= MAX_STALLED_ATTEMPTS {
                        return Err(download_refused(
                            status,
                            "store returned invalid Content-Range on HTTP 206".to_owned(),
                        ));
                    }
                    continue;
                }
            }
            200 => {
                if from > 0 {
                    truncate_to_zero(&mut file)?;
                    from = 0;
                }
            }
            _ => {
                return Err(download_refused(
                    status,
                    format!("store refused download with HTTP {status}"),
                ));
            }
        }

        let start_from = from;
        let mut reader = response.body_mut().as_reader();
        let mut read_err: Option<io::Error> = None;

        loop {
            match read_chunk(&mut reader, &mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let n_u64 = n as u64;
                    let new_from = from.checked_add(n_u64).ok_or_else(|| {
                        download_refused(status, "download length overflow".to_owned())
                    })?;
                    if new_from > size {
                        let _ = file.flush();
                        return Err(download_refused(
                            status,
                            format!("store sent more bytes ({new_from}) than expected ({size})"),
                        ));
                    }
                    file.write_all(&buf[..n]).map_err(io_client_err)?;
                    from = new_from;
                    bytes_fetched = bytes_fetched.saturating_add(n_u64);
                    progress(Progress {
                        done: from,
                        total: size,
                    });
                }
                Err(error) => {
                    read_err = Some(error);
                    break;
                }
            }
        }

        file.flush().map_err(io_client_err)?;

        if from > start_from {
            stalled_attempts = 0;
        } else {
            stalled_attempts += 1;
        }

        if from < size {
            if stalled_attempts >= MAX_STALLED_ATTEMPTS {
                let message = match read_err {
                    Some(error) => format!("download stream failed: {error}"),
                    None => format!("download stream ended early at {from} of {size} bytes"),
                };
                return Err(transport_failed(message));
            }
            tracing::debug!(
                %content_id,
                from,
                size,
                stalled_attempts,
                "resuming interrupted download"
            );
        }
    }

    let actual_hex = hash_file_streaming(&mut file, &mut buf)?;
    drop(file);

    if actual_hex != expected_hex {
        let _ = fs::remove_file(&temp_path);
        return Err(ReplicaError::Remote {
            code: "files.hash_mismatch".to_owned(),
            message: format!("downloaded sha256:{actual_hex} does not match {content_id}"),
            retryable: false,
        });
    }

    fs::rename(&temp_path, dest).map_err(io_client_err)?;

    Ok(Downloaded {
        size,
        bytes_fetched,
        grants,
    })
}

/// Validated `Content-Range: bytes <start>-<end>/<total>` header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ContentRange {
    start: u64,
    end: u64,
    total: u64,
}

fn parse_content_range(header: &str) -> Option<ContentRange> {
    let rest = header.trim().strip_prefix("bytes ")?;
    let (range_part, total_part) = rest.split_once('/')?;
    let (start_part, end_part) = range_part.split_once('-')?;

    let start = parse_ascii_u64(start_part)?;
    let end = parse_ascii_u64(end_part)?;
    let total = parse_ascii_u64(total_part)?;

    if start <= end && end < total {
        Some(ContentRange { start, end, total })
    } else {
        None
    }
}

fn parse_ascii_u64(s: &str) -> Option<u64> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

fn parse_content_id(content_id: &str) -> Result<&str, ReplicaError> {
    let Some(hex) = content_id.strip_prefix("sha256:") else {
        return Err(ReplicaError::Client(format!(
            "invalid content id: {content_id}"
        )));
    };
    if hex.len() != 64 || !hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        return Err(ReplicaError::Client(format!(
            "invalid content id: {content_id}"
        )));
    }
    Ok(hex)
}

fn part_path(dest: &Path) -> Result<PathBuf, ReplicaError> {
    let Some(name) = dest.file_name() else {
        return Err(ReplicaError::Client(
            "download destination has no file name".to_owned(),
        ));
    };
    let mut part_name = name.to_os_string();
    part_name.push(".part");
    Ok(dest.with_file_name(part_name))
}

fn truncate_to_zero(file: &mut File) -> Result<(), ReplicaError> {
    file.set_len(0).map_err(io_client_err)?;
    file.seek(SeekFrom::Start(0)).map_err(io_client_err)?;
    Ok(())
}

fn hash_file_streaming(file: &mut File, buf: &mut [u8]) -> Result<String, ReplicaError> {
    file.seek(SeekFrom::Start(0)).map_err(io_client_err)?;
    let mut hasher = Sha256::new();
    loop {
        let n = read_chunk(file, buf).map_err(io_client_err)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn read_chunk(reader: &mut impl Read, buf: &mut [u8]) -> io::Result<usize> {
    loop {
        match reader.read(buf) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            other => return other,
        }
    }
}

fn download_refused(status: u16, message: String) -> ReplicaError {
    ReplicaError::Remote {
        code: "files.download_refused".to_owned(),
        message,
        retryable: status >= 500,
    }
}

fn transport_failed(message: String) -> ReplicaError {
    ReplicaError::Unheard {
        code: "transport.failed".to_owned(),
        message,
        retryable: true,
    }
}

fn io_client_err(error: io::Error) -> ReplicaError {
    ReplicaError::Client(format!("download file io: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_content_ranges() {
        assert_eq!(
            parse_content_range("bytes 0-99/100"),
            Some(ContentRange {
                start: 0,
                end: 99,
                total: 100,
            })
        );
        assert_eq!(
            parse_content_range("bytes 50-99/100"),
            Some(ContentRange {
                start: 50,
                end: 99,
                total: 100,
            })
        );
        assert_eq!(
            parse_content_range("  bytes 0-0/1  "),
            Some(ContentRange {
                start: 0,
                end: 0,
                total: 1,
            })
        );
    }

    #[test]
    fn rejects_invalid_content_ranges() {
        for invalid in [
            "",
            "items 0-99/100",
            "bytes */100",
            "bytes 0-99/*",
            "bytes 50-40/100",
            "bytes 0-100/100",
            "bytes 0-0/0",
            "bytes +0-99/100",
            "bytes 0-99",
            "bytes 0/100",
            "bytes a-99/100",
        ] {
            assert_eq!(
                parse_content_range(invalid),
                None,
                "expected None for {invalid:?}"
            );
        }
    }

    #[test]
    fn validates_content_ids() {
        let hex = "0123456789abcdef".repeat(4);
        let id = format!("sha256:{hex}");
        assert_eq!(parse_content_id(&id).expect("valid id"), hex);

        assert!(parse_content_id("sha256:short").is_err());
        assert!(parse_content_id(&format!("sha256:{}", "A".repeat(64))).is_err());
        assert!(parse_content_id(&format!("md5:{hex}")).is_err());
    }
}
