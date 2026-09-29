//! The devtools page: what Syrup sees, believes, knows and says, live.
//!
//! A small HTTP server on `127.0.0.1` only. It serves the page from
//! `apps/devtools/dist` (built into the program) and a JSON API:
//!
//! | request                     | answer                                             |
//! |-----------------------------|----------------------------------------------------|
//! | `GET /api/snapshot`         | the [`Snapshot`](crate::Snapshot)                  |
//! | `GET /api/events?since=N`   | bus events after #N (at most the last 400)         |
//! | `GET /api/frame.png`        | the frame, with everything perception found on it  |
//! | `GET /api/overlay.png`      | Syrup's overlay as the player sees it              |
//! | `POST /api/feedback`        | `{advice_id, topic, kind}`: 👍 👎 ❓ 🔇            |
//! | `POST /api/mode`            | `{mode}`: hidden, minimal, normal, analysis        |
//! | `POST /api/confirm`         | `{title, game_id?}`: "this game is …"              |
//! | `POST /api/correct`         | `{norm, kind, concept}`: "that element is …"       |
//! | `POST /api/research`        | `{topic}`: look this up                            |
//!
//! Requests whose `Host` is not this address are refused (so a web page
//! cannot reach the server through DNS rebinding), and changes must be sent
//! as JSON (so another site cannot post to it without the browser asking
//! first, which this server never allows).

use std::io;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use image::{ImageEncoder, RgbaImage};
use serde_json::Value;
use syrup_core::EventBus;

use crate::http::{error, json, read_request, respond};
use crate::{Command, Shared, game_for_title};

pub const INDEX_HTML: &str = include_str!("../../../apps/devtools/dist/index.html");
pub const APP_JS: &str = include_str!("../../../apps/devtools/dist/app.js");
pub const STYLE_CSS: &str = include_str!("../../../apps/devtools/dist/style.css");

const MAX_BODY: usize = 64 * 1024;
const MAX_EVENTS: usize = 400;

/// The running server. Dropping it stops it.
pub struct Devtools {
    pub addr: SocketAddr,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

#[derive(Clone)]
struct Ctx {
    shared: Arc<Mutex<Shared>>,
    bus: EventBus,
    commands: Sender<Command>,
    hosts: Vec<String>,
}

impl Devtools {
    /// Serves on `127.0.0.1:port` (0: any free port).
    pub fn start(
        port: u16,
        shared: Arc<Mutex<Shared>>,
        bus: EventBus,
        commands: Sender<Command>,
    ) -> io::Result<Devtools> {
        let listener = TcpListener::bind(("127.0.0.1", port))?;
        listener.set_nonblocking(true)?;
        let addr = listener.local_addr()?;
        let p = addr.port();
        let hosts = vec![format!("127.0.0.1:{p}"), format!("localhost:{p}"), format!("[::1]:{p}")];
        let stop = Arc::new(AtomicBool::new(false));
        let ctx = Ctx { shared, bus, commands, hosts };
        let flag = stop.clone();
        let thread = std::thread::Builder::new().name("syrup-devtools".into()).spawn(move || {
            while !flag.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let ctx = ctx.clone();
                        let _ = std::thread::Builder::new().name("syrup-devtools-conn".into()).spawn(move || {
                            let _ = serve(stream, &ctx);
                        });
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(15)),
                    Err(_) => std::thread::sleep(Duration::from_millis(50)),
                }
            }
        })?;
        Ok(Devtools { addr, stop, thread: Some(thread) })
    }

    pub fn url(&self) -> String {
        format!("http://{}/", self.addr)
    }
}

impl Drop for Devtools {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// PNG bytes (large frames are halved first: the page shows them small anyway).
pub fn png(img: &RgbaImage) -> Vec<u8> {
    let small;
    let img = if img.width() > 1280 {
        let h = (img.height() as u64 * 1280 / img.width() as u64).max(1) as u32;
        small = image::imageops::resize(img, 1280, h, image::imageops::FilterType::Triangle);
        &small
    } else {
        img
    };
    let mut out = Vec::new();
    let _ = image::codecs::png::PngEncoder::new(&mut out).write_image(
        img.as_raw(),
        img.width(),
        img.height(),
        image::ColorType::Rgba8,
    );
    out
}

fn serve(stream: TcpStream, ctx: &Ctx) -> io::Result<()> {
    // Accepted sockets may inherit the listener's non-blocking mode (Windows does).
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let req = match read_request(&stream, MAX_BODY) {
        Ok(r) => r,
        Err(_) => return error(&stream, "400 Bad Request", "could not read the request"),
    };
    let host = req.headers.get("host").map(|h| h.to_ascii_lowercase()).unwrap_or_default();
    if !ctx.hosts.contains(&host) {
        return error(&stream, "403 Forbidden", "only this computer can use Syrup's devtools");
    }
    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/" | "/index.html") => respond(&stream, "200 OK", "text/html; charset=utf-8", INDEX_HTML.as_bytes()),
        ("GET", "/app.js") => respond(&stream, "200 OK", "text/javascript; charset=utf-8", APP_JS.as_bytes()),
        ("GET", "/style.css") => respond(&stream, "200 OK", "text/css; charset=utf-8", STYLE_CSS.as_bytes()),
        ("GET", "/api/snapshot") => {
            let snap = ctx.shared.lock().map(|s| s.snapshot.clone()).unwrap_or_default();
            json(&stream, &snap)
        }
        ("GET", "/api/events") => {
            let since = req.query.get("since").and_then(|s| s.parse().ok()).unwrap_or(0);
            let mut events = ctx.bus.since(since);
            if events.len() > MAX_EVENTS {
                events.drain(..events.len() - MAX_EVENTS);
            }
            json(&stream, &events)
        }
        ("GET", "/api/frame.png") => {
            let (frame, obs) = match ctx.shared.lock() {
                Ok(s) => (s.frame.clone(), s.observation.clone()),
                Err(_) => (None, None),
            };
            match frame {
                Some(frame) => {
                    let img = match &obs {
                        Some(o) => syrup_perception::annotate(&frame, o),
                        None => (*frame).clone(),
                    };
                    respond(&stream, "200 OK", "image/png", &png(&img))
                }
                None => error(&stream, "404 Not Found", "no frame yet"),
            }
        }
        ("GET", "/api/overlay.png") => {
            let (view, t) = match ctx.shared.lock() {
                Ok(s) => (s.snapshot.view.clone(), s.snapshot.t_ms),
                Err(_) => (None, 0),
            };
            let view = view.unwrap_or_default();
            let painted = syrup_ui::paint(&view, t);
            respond(&stream, "200 OK", "image/png", &png(&painted.image))
        }
        ("POST", path) if path.starts_with("/api/") => {
            let is_json =
                req.headers.get("content-type").is_some_and(|c| c.to_ascii_lowercase().starts_with("application/json"));
            if !is_json {
                return error(&stream, "415 Unsupported Media Type", "send JSON");
            }
            let mut value: Value = match serde_json::from_slice(&req.body) {
                Ok(v @ Value::Object(_)) => v,
                _ => return error(&stream, "400 Bad Request", "the body must be a JSON object"),
            };
            let name = path.trim_start_matches("/api/");
            if name == "confirm" {
                let title = value.get("title").and_then(|t| t.as_str()).unwrap_or("").trim().to_string();
                if title.is_empty() {
                    return error(&stream, "400 Bad Request", "which game is it? send its title");
                }
                let (id, title) = match value.get("game_id").and_then(|g| g.as_str()).filter(|g| !g.trim().is_empty()) {
                    Some(id) => (id.to_string(), title),
                    None => game_for_title(&title),
                };
                value = serde_json::json!({ "game_id": id, "title": title });
            }
            if name != "command" {
                value["command"] = Value::String(name.to_string());
            }
            match serde_json::from_value::<Command>(value) {
                Ok(c) => {
                    let _ = ctx.commands.send(c);
                    json(&stream, &serde_json::json!({ "ok": true }))
                }
                Err(e) => error(&stream, "400 Bad Request", &format!("not a command Syrup knows: {e}")),
            }
        }
        _ => error(&stream, "404 Not Found", "nothing here"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Snapshot;
    use std::io::{Read, Write};
    use std::sync::mpsc::channel;

    fn request(addr: SocketAddr, raw: &str) -> (String, Vec<u8>) {
        let mut s = TcpStream::connect(addr).unwrap();
        s.write_all(raw.as_bytes()).unwrap();
        let mut out = Vec::new();
        s.read_to_end(&mut out).unwrap();
        let split = out.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        (String::from_utf8_lossy(&out[..split]).to_string(), out[split + 4..].to_vec())
    }

    #[test]
    fn serves_the_page_the_api_and_takes_commands() {
        let shared = Arc::new(Mutex::new(Shared::default()));
        shared.lock().unwrap().snapshot = Snapshot { session: "s1".into(), t_ms: 1234, ..Default::default() };
        let bus = EventBus::default();
        bus.publish(syrup_core::Event::Note { ts_ms: 1, message: "hello".into() });
        let (tx, rx) = channel();
        let d = Devtools::start(0, shared.clone(), bus.clone(), tx).unwrap();
        let host = format!("127.0.0.1:{}", d.addr.port());

        let (head, body) = request(d.addr, &format!("GET / HTTP/1.1\r\nHost: {host}\r\n\r\n"));
        assert!(head.starts_with("HTTP/1.1 200"), "{head}");
        assert!(String::from_utf8_lossy(&body).contains("Syrup devtools"));

        let (_, body) = request(d.addr, &format!("GET /api/snapshot HTTP/1.1\r\nHost: {host}\r\n\r\n"));
        let snap: Snapshot = serde_json::from_slice(&body).unwrap();
        assert_eq!(snap.t_ms, 1234);

        let (_, body) = request(d.addr, &format!("GET /api/events?since=0 HTTP/1.1\r\nHost: {host}\r\n\r\n"));
        assert!(String::from_utf8_lossy(&body).contains("hello"));

        // No frame yet; then one.
        let (head, _) = request(d.addr, &format!("GET /api/frame.png HTTP/1.1\r\nHost: {host}\r\n\r\n"));
        assert!(head.starts_with("HTTP/1.1 404"), "{head}");
        shared.lock().unwrap().frame = Some(Arc::new(RgbaImage::from_pixel(64, 36, image::Rgba([10, 20, 30, 255]))));
        let (head, body) = request(d.addr, &format!("GET /api/frame.png HTTP/1.1\r\nHost: {host}\r\n\r\n"));
        assert!(head.contains("image/png"), "{head}");
        assert_eq!(&body[1..4], b"PNG");
        let (head, body) = request(d.addr, &format!("GET /api/overlay.png HTTP/1.1\r\nHost: {host}\r\n\r\n"));
        assert!(head.contains("image/png"), "{head}");
        assert!(image::load_from_memory(&body).unwrap().width() > 100);

        let fb = r#"{"advice_id":7,"topic":"health","kind":"useful"}"#;
        let (head, _) = request(
            d.addr,
            &format!(
                "POST /api/feedback HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{fb}",
                fb.len()
            ),
        );
        assert!(head.starts_with("HTTP/1.1 200"), "{head}");
        let c = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(
            c,
            Command::Feedback { advice_id: 7, topic: "health".into(), kind: syrup_core::FeedbackKind::Useful }
        );

        let body = r#"{"title":"maplestory"}"#;
        request(
            d.addr,
            &format!(
                "POST /api/confirm HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            ),
        );
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(2)).unwrap(),
            Command::Confirm { game_id: "maplestory".into(), title: "MapleStory".into() }
        );

        let body = r#"{"mode":"analysis"}"#;
        request(
            d.addr,
            &format!(
                "POST /api/mode HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            ),
        );
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(2)).unwrap(),
            Command::Mode { mode: syrup_ui::OverlayMode::Analysis }
        );

        // Another site's page cannot use it.
        let (head, _) = request(d.addr, "GET /api/snapshot HTTP/1.1\r\nHost: evil.example:80\r\n\r\n");
        assert!(head.starts_with("HTTP/1.1 403"), "{head}");
        let (head, _) = request(
            d.addr,
            &format!(
                "POST /api/feedback HTTP/1.1\r\nHost: {host}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{fb}",
                fb.len()
            ),
        );
        assert!(head.starts_with("HTTP/1.1 415"), "{head}");
        let bad = r#"{"advice_id":"x"}"#;
        let (head, _) = request(
            d.addr,
            &format!(
                "POST /api/feedback HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{bad}",
                bad.len()
            ),
        );
        assert!(head.starts_with("HTTP/1.1 400"), "{head}");
        assert!(rx.try_recv().is_err());
    }
}
