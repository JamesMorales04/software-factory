use std::{
    fmt,
    io::{ErrorKind, Read, Write},
    net::TcpStream,
    time::Duration,
};

use crate::core_url::CoreUrl;

const MAX_RESPONSE_BYTES: usize = 64 * 1024;
const MAX_ADDRESSES: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HealthResponse {
    pub(crate) status: u16,
    pub(crate) body: String,
}

#[derive(Debug)]
pub(crate) struct HttpError(String);

impl HttpError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for HttpError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for HttpError {}

pub(crate) fn get_health(url: &CoreUrl, timeout: Duration) -> Result<HealthResponse, HttpError> {
    let mut stream = connect_loopback(url, timeout)?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|error| HttpError::new(format!("cannot set read timeout: {error}")))?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|error| HttpError::new(format!("cannot set write timeout: {error}")))?;

    let request = format!(
        "GET /health HTTP/1.1\r\nHost: {}\r\nAccept: application/json\r\nUser-Agent: factory-cli/{}\r\nConnection: close\r\n\r\n",
        url.host_header(),
        env!("CARGO_PKG_VERSION")
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|error| HttpError::new(format!("cannot send /health request: {error}")))?;
    stream
        .flush()
        .map_err(|error| HttpError::new(format!("cannot flush /health request: {error}")))?;

    let mut response = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => {
                if response.len() + read > MAX_RESPONSE_BYTES {
                    return Err(HttpError::new(format!(
                        "response exceeded {MAX_RESPONSE_BYTES} bytes"
                    )));
                }
                response.extend_from_slice(&buffer[..read]);
            }
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                return Err(HttpError::new(format!(
                    "timed out after {} ms waiting for /health",
                    timeout.as_millis()
                )));
            }
            Err(error) => {
                return Err(HttpError::new(format!(
                    "cannot read /health response: {error}"
                )));
            }
        }
    }
    parse_response(&response)
}

fn connect_loopback(url: &CoreUrl, timeout: Duration) -> Result<TcpStream, HttpError> {
    let addresses = url
        .socket_addrs()
        .map_err(|error| HttpError::new(format!("cannot resolve Core host: {error}")))?;
    let mut attempted_loopback = false;
    let mut last_error = None;
    for address in addresses.into_iter().take(MAX_ADDRESSES) {
        if !address.ip().is_loopback() {
            continue;
        }
        attempted_loopback = true;
        match TcpStream::connect_timeout(&address, timeout) {
            Ok(stream) => return Ok(stream),
            Err(error) => last_error = Some(error),
        }
    }
    if !attempted_loopback {
        return Err(HttpError::new(
            "Core host did not resolve to a loopback address",
        ));
    }
    match last_error {
        Some(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
            Err(HttpError::new(format!(
                "cannot connect to Core within {} ms: {error}",
                timeout.as_millis()
            )))
        }
        Some(error) => Err(HttpError::new(format!("cannot connect to Core: {error}"))),
        None => Err(HttpError::new(
            "cannot connect to Core: no address available",
        )),
    }
}

fn parse_response(raw: &[u8]) -> Result<HealthResponse, HttpError> {
    let separator = find_bytes(raw, b"\r\n\r\n")
        .ok_or_else(|| HttpError::new("malformed HTTP response: missing header terminator"))?;
    let head = std::str::from_utf8(&raw[..separator])
        .map_err(|_| HttpError::new("malformed HTTP response: headers are not UTF-8"))?;
    let body_bytes = &raw[separator + 4..];

    let mut lines = head.split("\r\n");
    let status_line = lines
        .next()
        .ok_or_else(|| HttpError::new("malformed HTTP response: missing status line"))?;
    let mut fields = status_line.split_whitespace();
    let version = fields
        .next()
        .ok_or_else(|| HttpError::new("malformed HTTP response: missing HTTP version"))?;
    if !version.starts_with("HTTP/") {
        return Err(HttpError::new(format!(
            "malformed HTTP response: unexpected version {version:?}"
        )));
    }
    let status = fields
        .next()
        .ok_or_else(|| HttpError::new("malformed HTTP response: missing status code"))?
        .parse::<u16>()
        .map_err(|_| HttpError::new("malformed HTTP response: invalid status code"))?;

    let mut content_length = None;
    let mut chunked = false;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("content-length") {
            content_length = value.trim().parse::<usize>().ok();
        } else if name.eq_ignore_ascii_case("transfer-encoding")
            && value.to_ascii_lowercase().contains("chunked")
        {
            chunked = true;
        }
    }

    let body = if chunked {
        decode_chunked(body_bytes)?
    } else if let Some(length) = content_length {
        if length > MAX_RESPONSE_BYTES {
            return Err(HttpError::new(format!(
                "response declared {length} bytes, exceeding {MAX_RESPONSE_BYTES}"
            )));
        }
        if body_bytes.len() < length {
            return Err(HttpError::new(
                "malformed HTTP response: body shorter than Content-Length",
            ));
        }
        body_bytes[..length].to_vec()
    } else {
        body_bytes.to_vec()
    };

    Ok(HealthResponse {
        status,
        body: String::from_utf8_lossy(&body).trim().to_owned(),
    })
}

fn decode_chunked(mut data: &[u8]) -> Result<Vec<u8>, HttpError> {
    let mut decoded = Vec::new();
    loop {
        let line_end = find_bytes(data, b"\r\n")
            .ok_or_else(|| HttpError::new("malformed chunked body: missing size line"))?;
        let size_line = std::str::from_utf8(&data[..line_end])
            .map_err(|_| HttpError::new("malformed chunked body: size line is not UTF-8"))?;
        let size_text = size_line.split(';').next().unwrap_or_default().trim();
        let size = usize::from_str_radix(size_text, 16)
            .map_err(|_| HttpError::new("malformed chunked body: invalid chunk size"))?;
        data = &data[line_end + 2..];
        if size == 0 {
            return Ok(decoded);
        }
        if data.len() < size + 2 {
            return Err(HttpError::new("malformed chunked body: truncated chunk"));
        }
        if &data[size..size + 2] != b"\r\n" {
            return Err(HttpError::new(
                "malformed chunked body: missing chunk terminator",
            ));
        }
        if decoded.len() + size > MAX_RESPONSE_BYTES {
            return Err(HttpError::new(format!(
                "response exceeded {MAX_RESPONSE_BYTES} bytes"
            )));
        }
        decoded.extend_from_slice(&data[..size]);
        data = &data[size + 2..];
    }
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::{HealthResponse, parse_response};

    #[test]
    fn parses_health_with_content_length() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 34\r\n\r\n{\"healthy\":true,\"version\":\"0.1.0\"}";
        assert_eq!(
            parse_response(raw).expect("response"),
            HealthResponse {
                status: 200,
                body: "{\"healthy\":true,\"version\":\"0.1.0\"}".to_owned(),
            }
        );
    }

    #[test]
    fn parses_chunked_health() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nA\r\n{\"healthy\"\r\n6\r\n:true}\r\n0\r\n\r\n";
        assert_eq!(
            parse_response(raw).expect("response"),
            HealthResponse {
                status: 200,
                body: "{\"healthy\":true}".to_owned(),
            }
        );
    }

    #[test]
    fn rejects_malformed_responses() {
        assert!(parse_response(b"not http").is_err());
        assert!(parse_response(b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nshort").is_err());
        assert!(parse_response(b"HTTP/1.1 nope OK\r\n\r\n").is_err());
    }
}
