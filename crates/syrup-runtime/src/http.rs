//! The little HTTP the devtools page and the phone server speak: one request
//! per connection, a body only with a length, and the answer closes it.

use std::collections::BTreeMap;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::TcpStream;

pub(crate) struct Request {
    pub method: String,
    pub path: String,
    pub query: BTreeMap<String, String>,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

pub(crate) fn read_request(stream: &TcpStream, max_body: usize) -> io::Result<Request> {
    let mut reader = BufReader::new(stream);
    let mut first = String::new();
    reader.read_line(&mut first)?;
    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("/").to_string();
    let mut headers = BTreeMap::new();
    for _ in 0..100 {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }
    let len: usize = headers.get("content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
    if len > max_body {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "body too large"));
    }
    let mut body = vec![0; len];
    reader.read_exact(&mut body)?;
    let (path, q) = target.split_once('?').unwrap_or((&target, ""));
    let query = q
        .split('&')
        .filter_map(|kv| {
            let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
            (!k.is_empty()).then(|| (k.to_string(), v.to_string()))
        })
        .collect();
    Ok(Request { method, path: path.to_string(), query, headers, body })
}

pub(crate) fn respond(mut stream: &TcpStream, status: &str, content_type: &str, body: &[u8]) -> io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)?;
    stream.flush()
}

pub(crate) fn json(stream: &TcpStream, value: &impl serde::Serialize) -> io::Result<()> {
    let body = serde_json::to_vec(value).unwrap_or_else(|_| b"null".to_vec());
    respond(stream, "200 OK", "application/json", &body)
}

pub(crate) fn error(stream: &TcpStream, status: &str, message: &str) -> io::Result<()> {
    let body = serde_json::json!({ "error": message }).to_string();
    respond(stream, status, "application/json", body.as_bytes())
}
