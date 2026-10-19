//! A minimal, defensive HTTP/1.1 server on the standard library: one request
//! per connection (`Connection: close`), `Content-Length` bodies only, hard
//! limits on header and body size, socket timeouts and a cap on concurrent
//! connections. Enough for a local dashboard backend; put a real reverse
//! proxy (TLS, rate limiting) in front of it before exposing it publicly.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Largest accepted request body (the dashboard sends portfolio CSVs up to 10 MB).
pub const MAX_BODY: usize = 12 * 1024 * 1024;
/// Largest accepted request head.
pub const MAX_HEAD: usize = 64 * 1024;
/// Concurrent connections served; further connections get 503.
pub const MAX_CONNECTIONS: usize = 32;

/// A parsed request.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Request {
    /// Upper-case method.
    pub method: String,
    /// Path without the query string.
    pub path: String,
    /// Raw query string (after `?`), possibly empty.
    pub query: String,
    /// Headers with lower-case names.
    pub headers: Vec<(String, String)>,
    /// Body bytes.
    pub body: Vec<u8>,
}

impl Request {
    /// A header value by (case-insensitive) name.
    pub fn header(&self, name: &str) -> Option<&str> {
        let name = name.to_ascii_lowercase();
        self.headers.iter().find(|(k, _)| *k == name).map(|(_, v)| v.as_str())
    }
}

/// A response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    /// Status code.
    pub status: u16,
    /// Extra headers.
    pub headers: Vec<(String, String)>,
    /// Body.
    pub body: Vec<u8>,
}

impl Response {
    /// A JSON response.
    pub fn json(status: u16, body: impl Into<String>) -> Self {
        Self {
            status,
            headers: vec![("Content-Type".into(), "application/json".into())],
            body: body.into().into_bytes(),
        }
    }

    /// An error in the FastAPI shape the dashboard reads: `{"detail": "..."}`.
    pub fn error(status: u16, detail: &str) -> Self {
        Self::json(
            status,
            spectra_core::Json::object().with("detail", spectra_core::Json::string(detail)).dump(),
        )
    }
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        411 => "Length Required",
        413 => "Payload Too Large",
        422 => "Unprocessable Entity",
        431 => "Request Header Fields Too Large",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        503 => "Service Unavailable",
        _ => "Unknown",
    }
}

/// Why a request could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadError {
    /// The peer closed the connection before sending a request.
    Closed,
    /// A malformed or over-limit request; answer with this response.
    Respond(u16, &'static str),
}

/// Reads one request from `stream`.
///
/// # Errors
/// [`ReadError`] for a closed, malformed or over-limit request.
pub fn read_request(stream: &mut impl Read) -> Result<Request, ReadError> {
    let mut reader = BufReader::new(stream);
    let mut head_bytes = 0usize;
    let mut line = String::new();
    let read_line = |reader: &mut BufReader<_>,
                     line: &mut String,
                     head: &mut usize|
     -> Result<(), ReadError> {
        line.clear();
        let mut limited = reader.by_ref().take((MAX_HEAD - *head) as u64 + 1);
        match limited.read_line(line) {
            Ok(0) => Err(ReadError::Closed),
            Ok(n) => {
                *head += n;
                if *head > MAX_HEAD {
                    return Err(ReadError::Respond(431, "Request head too large."));
                }
                Ok(())
            }
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {
                Err(ReadError::Respond(408, "Request timed out."))
            }
            Err(_) => Err(ReadError::Respond(400, "Malformed request.")),
        }
    };
    read_line(&mut reader, &mut line, &mut head_bytes)?;
    let mut parts = line.trim_end().split(' ');
    let (Some(method), Some(target), Some(version)) = (parts.next(), parts.next(), parts.next()) else {
        return Err(ReadError::Respond(400, "Malformed request line."));
    };
    if !version.starts_with("HTTP/1.") {
        return Err(ReadError::Respond(400, "Only HTTP/1.x is supported."));
    }
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let mut req = Request {
        method: method.to_ascii_uppercase(),
        path: path.to_string(),
        query: query.to_string(),
        ..Request::default()
    };
    loop {
        read_line(&mut reader, &mut line, &mut head_bytes).map_err(|e| match e {
            ReadError::Closed => ReadError::Respond(400, "Truncated request head."),
            other @ ReadError::Respond(..) => other,
        })?;
        let l = line.trim_end_matches(['\r', '\n']);
        if l.is_empty() {
            break;
        }
        let Some((k, v)) = l.split_once(':') else {
            return Err(ReadError::Respond(400, "Malformed header."));
        };
        req.headers.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
    }
    if req.header("transfer-encoding").is_some() {
        return Err(ReadError::Respond(411, "Chunked bodies are not supported; send Content-Length."));
    }
    let len = match req.header("content-length") {
        None => 0,
        Some(v) => v.parse::<usize>().map_err(|_| ReadError::Respond(400, "Invalid Content-Length."))?,
    };
    if len > MAX_BODY {
        return Err(ReadError::Respond(413, "Request body exceeds the 12 MB limit."));
    }
    req.body = vec![0; len];
    reader.read_exact(&mut req.body).map_err(|e| match e.kind() {
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut => {
            ReadError::Respond(408, "Request timed out.")
        }
        _ => ReadError::Respond(400, "Truncated request body."),
    })?;
    Ok(req)
}

/// Serializes a response.
///
/// # Errors
/// Any socket write error.
pub fn write_response(stream: &mut impl Write, resp: &Response) -> std::io::Result<()> {
    let mut head = format!("HTTP/1.1 {} {}\r\n", resp.status, reason(resp.status));
    for (k, v) in &resp.headers {
        head += &format!("{k}: {v}\r\n");
    }
    head += &format!(
        "Content-Length: {}\r\nConnection: close\r\nX-Content-Type-Options: nosniff\r\nCache-Control: no-store\r\n\r\n",
        resp.body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(&resp.body)?;
    stream.flush()
}

fn serve_one(mut stream: TcpStream, handler: &(dyn Fn(&Request) -> Response + Sync)) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(60)));
    let resp = match read_request(&mut stream) {
        Ok(req) => handler(&req),
        Err(ReadError::Closed) => return,
        Err(ReadError::Respond(code, msg)) => Response::error(code, msg),
    };
    let _ = write_response(&mut stream, &resp);
    let _ = stream.shutdown(std::net::Shutdown::Both);
}

/// Accepts connections forever, one thread per connection up to
/// [`MAX_CONNECTIONS`].
#[allow(clippy::needless_pass_by_value)] // the server owns its handler for its whole life
pub fn serve(listener: &TcpListener, handler: Arc<dyn Fn(&Request) -> Response + Send + Sync>) {
    let active = Arc::new(AtomicUsize::new(0));
    for conn in listener.incoming() {
        let Ok(mut stream) = conn else { continue };
        if active.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
            active.fetch_sub(1, Ordering::SeqCst);
            let _ = write_response(&mut stream, &Response::error(503, "Server busy; try again."));
            continue;
        }
        let (counter, handler) = (Arc::clone(&active), Arc::clone(&handler));
        let spawned = std::thread::Builder::new().name("spectra-api-conn".into()).spawn(move || {
            serve_one(stream, handler.as_ref());
            counter.fetch_sub(1, Ordering::SeqCst);
        });
        if spawned.is_err() {
            // The thread never started, so its decrement never runs: release the slot here.
            active.fetch_sub(1, Ordering::SeqCst);
        }
    }
}
