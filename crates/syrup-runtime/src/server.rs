//! Syrup as a server, for the phone app.
//!
//! A phone can't run Syrup next to a game, but it can show Syrup its screen
//! and say what Syrup says. The iPhone app has two halves: its screen
//! broadcast (the eyes) posts a frame every half second or so, and the app
//! itself (the mouth) keeps asking what to say and says it, from the
//! background. Everything in between (seeing, recognising the game, learning
//! it, remembering, researching, deciding what is worth saying) runs here,
//! one [`Runtime`] per phone, exactly as it runs on a PC.
//!
//! | request | answer |
//! |---|---|
//! | `POST /v1/frame?device=D[&t=MS]`, a JPEG or PNG | `{"say": [...], "mouth": bool, "next": {"interval_ms", "max_side", "quality"}}` |
//! | `GET /v1/say?device=D&after=N` | up to 25 s later: `{"lines": [{"seq", "text", "end"}], "last", "watching"}`; without `after`, at once |
//! | `POST /v1/game?device=D`, `{"title": "..."}` | which game the player says this is |
//! | `POST /v1/end?device=D` | the broadcast stopped: the session ends, and its summary is said |
//! | `GET /healthz` | `ok` |
//!
//! `t` is the phone's own clock (milliseconds since its broadcast started);
//! without it, the time the frame arrived. `mouth` says whether the phone's
//! app is listening, so the eyes can show a line some other way when it
//! isn't. `next` is how the phone should capture: the server decides.
//!
//! A session also ends after `idle` without frames (the broadcast was cut,
//! the phone lost its connection). Frames are analysed and dropped: nothing
//! of them is kept unless the runtime is set to record.
//!
//! Who may use it: with a token set (a hosted server), every `/v1/` request
//! needs `Authorization: Bearer <token>`. Without one (a server on the
//! player's own computer), the server belongs to the first phone that talks
//! to it, and to no other until it is told to pair one more
//! ([`ServerConfig::pairing`]).

use std::collections::{HashMap, VecDeque};
use std::io::{self, Cursor};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::json;
use syrup_core::{Frame, SourceInfo, SourceKind};

use crate::http::{Request, error, json, read_request, respond};
use crate::{Command, Runtime, RuntimeConfig, game_for_title};

const MAX_FRAME_BYTES: usize = 6 << 20;
const MAX_SIDE: u32 = 4096;
const MAX_PHONES: usize = 32;
const KEEP_LINES: usize = 64;
const POLL: Duration = Duration::from_secs(25);
/// A mouth that asked this recently is listening.
const LISTENING: Duration = Duration::from_secs(40);
/// A phone with no session that nobody has heard from this long is forgotten.
const FORGET_PHONE: Duration = Duration::from_secs(6 * 3600);

/// How the phone should capture. Sent back with every frame, so the server decides.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CaptureAdvice {
    /// At least this long between frames.
    pub interval_ms: u32,
    /// Scale frames down so their longer side is at most this.
    pub max_side: u32,
    /// JPEG quality, 0..1.
    pub quality: f32,
}

impl Default for CaptureAdvice {
    fn default() -> Self {
        CaptureAdvice { interval_ms: 500, max_side: 960, quality: 0.6 }
    }
}

/// The runtime settings for one phone, from its player id and the game the
/// player named (a game id and title), if any.
pub type MakeConfig = Arc<dyn Fn(&str, Option<(String, String)>) -> RuntimeConfig + Send + Sync>;

pub struct ServerConfig {
    pub bind: String,
    pub port: u16,
    pub token: Option<String>,
    /// Without a token: where the phones this server belongs to are kept
    /// (the first phone to talk to it pairs itself). `None`: anyone may use it.
    pub pairing: Option<Pairing>,
    /// A session with no frame for this long ends.
    pub idle: Duration,
    pub capture: CaptureAdvice,
    pub make: MakeConfig,
}

/// The phones a server belongs to.
#[derive(Debug, Clone)]
pub struct Pairing {
    /// A JSON list of device ids.
    pub file: std::path::PathBuf,
    /// Let one more phone pair while this server runs.
    pub one_more: bool,
}

/// One thing Syrup said to a phone.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SaidLine {
    pub seq: u64,
    pub text: String,
    /// The session's last line (its summary).
    pub end: bool,
}

/// The running server. Dropping it stops it.
pub struct Server {
    pub addr: SocketAddr,
    hub: Arc<Hub>,
    threads: Vec<JoinHandle<()>>,
}

struct Session {
    rt: Runtime,
    source: Arc<SourceInfo>,
    frames: u64,
    started: Instant,
    last_t: u64,
    last_frame: Instant,
}

#[derive(Default)]
struct Said {
    next: u64,
    lines: VecDeque<SaidLine>,
    watching: bool,
}

impl Said {
    fn last(&self) -> u64 {
        self.next
    }
}

struct Phone {
    id: String,
    session: Mutex<Option<Session>>,
    game: Mutex<Option<String>>,
    said: Mutex<Said>,
    ready: Condvar,
    last_poll: Mutex<Option<Instant>>,
    last_seen: Mutex<Instant>,
}

struct Hub {
    cfg: ServerConfig,
    phones: Mutex<HashMap<String, Arc<Phone>>>,
    /// The paired phones, and whether one more may pair.
    paired: Mutex<(Vec<String>, bool)>,
    stop: AtomicBool,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Equal, in time that does not depend on where they differ.
fn same(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn valid_device(d: &str) -> bool {
    (1..=64).contains(&d.len()) && d.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// The player model a phone uses: one per phone.
pub fn player_for(device: &str) -> String {
    let short: String = device.chars().filter(|c| c.is_ascii_alphanumeric()).take(8).collect();
    format!("phone-{}", short.to_ascii_lowercase())
}

impl Server {
    pub fn start(cfg: ServerConfig) -> io::Result<Server> {
        let listener = TcpListener::bind((cfg.bind.as_str(), cfg.port))?;
        listener.set_nonblocking(true)?;
        let addr = listener.local_addr()?;
        let paired = match &cfg.pairing {
            Some(p) => {
                let known: Vec<String> =
                    std::fs::read(&p.file).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
                let one_more = p.one_more || known.is_empty();
                (known, one_more)
            }
            None => (Vec::new(), false),
        };
        let hub = Arc::new(Hub {
            cfg,
            phones: Mutex::new(HashMap::new()),
            paired: Mutex::new(paired),
            stop: AtomicBool::new(false),
        });
        let accept = {
            let hub = hub.clone();
            std::thread::Builder::new().name("syrup-server".into()).spawn(move || {
                while !hub.stop.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            let hub = hub.clone();
                            let _ = std::thread::Builder::new()
                                .name("syrup-server-conn".into())
                                .stack_size(16 << 20)
                                .spawn(move || {
                                    let _ = hub.serve(stream);
                                });
                        }
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(10))
                        }
                        Err(_) => std::thread::sleep(Duration::from_millis(50)),
                    }
                }
            })?
        };
        let idle = {
            let hub = hub.clone();
            std::thread::Builder::new().name("syrup-server-idle".into()).stack_size(16 << 20).spawn(move || {
                while !hub.stop.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(250));
                    hub.end_idle();
                }
            })?
        };
        Ok(Server { addr, hub, threads: vec![accept, idle] })
    }

    pub fn url(&self) -> String {
        format!("http://{}/", self.addr)
    }

    /// Ends every session (saving what was learned) and stops.
    pub fn shutdown(mut self) {
        self.stop_all();
    }

    fn stop_all(&mut self) {
        if self.hub.stop.swap(true, Ordering::Relaxed) {
            return;
        }
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
        let phones: Vec<_> = lock(&self.hub.phones).values().cloned().collect();
        for p in phones {
            self.hub.end(&p);
            p.ready.notify_all();
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop_all();
    }
}

impl Hub {
    fn authorized(&self, req: &Request) -> bool {
        let Some(token) = self.cfg.token.as_deref().filter(|t| !t.is_empty()) else {
            return true;
        };
        req.headers
            .get("authorization")
            .and_then(|h| h.strip_prefix("Bearer ").or_else(|| h.strip_prefix("bearer ")))
            .is_some_and(|t| same(t.trim(), token))
    }

    /// Whether this phone may use the server (pairing it, if one more may pair).
    fn belongs_to(&self, device: &str) -> bool {
        let Some(pairing) = self.cfg.pairing.as_ref().filter(|_| self.cfg.token.is_none()) else {
            return true;
        };
        let mut paired = lock(&self.paired);
        if paired.0.iter().any(|d| d == device) {
            return true;
        }
        if !paired.1 {
            return false;
        }
        paired.0.push(device.to_string());
        paired.1 = false;
        if let Some(dir) = pairing.file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = pairing.file.with_extension("json.tmp");
        if std::fs::write(&tmp, serde_json::to_vec_pretty(&paired.0).unwrap_or_default()).is_ok() {
            let _ = std::fs::rename(&tmp, &pairing.file);
        }
        eprintln!("Syrup: paired with the phone {device}");
        true
    }

    fn phone(&self, id: &str) -> Option<Arc<Phone>> {
        let mut phones = lock(&self.phones);
        if let Some(p) = phones.get(id) {
            *lock(&p.last_seen) = Instant::now();
            return Some(p.clone());
        }
        if phones.len() >= MAX_PHONES {
            let now = Instant::now();
            phones.retain(|_, p| lock(&p.session).is_some() || now.duration_since(*lock(&p.last_seen)) < FORGET_PHONE);
            if phones.len() >= MAX_PHONES {
                return None;
            }
        }
        let p = Arc::new(Phone {
            id: id.to_string(),
            session: Mutex::new(None),
            game: Mutex::new(None),
            said: Mutex::new(Said::default()),
            ready: Condvar::new(),
            last_poll: Mutex::new(None),
            last_seen: Mutex::new(Instant::now()),
        });
        phones.insert(id.to_string(), p.clone());
        Some(p)
    }

    fn serve(&self, stream: TcpStream) -> io::Result<()> {
        // Accepted sockets may inherit the listener's non-blocking mode (Windows does).
        stream.set_nonblocking(false)?;
        stream.set_read_timeout(Some(Duration::from_secs(20)))?;
        stream.set_write_timeout(Some(Duration::from_secs(20)))?;
        let req = match read_request(&stream, MAX_FRAME_BYTES) {
            Ok(r) => r,
            Err(_) => return error(&stream, "400 Bad Request", "could not read the request"),
        };
        match (req.method.as_str(), req.path.as_str()) {
            ("GET", "/healthz") => return respond(&stream, "200 OK", "text/plain", b"ok"),
            ("GET", "/") => {
                return respond(
                    &stream,
                    "200 OK",
                    "text/plain; charset=utf-8",
                    b"Syrup learns the game with you. This is its server; the phone app talks to /v1/.\n",
                );
            }
            (_, p) if p.starts_with("/v1/") => {}
            _ => return error(&stream, "404 Not Found", "nothing here"),
        }
        if !self.authorized(&req) {
            return error(&stream, "401 Unauthorized", "wrong or missing token");
        }
        let Some(device) = req.query.get("device").filter(|d| valid_device(d)) else {
            return error(&stream, "400 Bad Request", "which phone? add ?device=<its id>");
        };
        if !self.belongs_to(device) {
            return error(
                &stream,
                "403 Forbidden",
                "this Syrup server belongs to another phone. To add this one, start it with: syrup serve --pair",
            );
        }
        let Some(phone) = self.phone(device) else {
            return error(&stream, "503 Service Unavailable", "too many phones at once");
        };
        match (req.method.as_str(), req.path.as_str()) {
            ("POST", "/v1/frame") => self.frame(&stream, &phone, &req),
            ("GET", "/v1/say") => self.say(&stream, &phone, req.query.get("after").and_then(|a| a.parse().ok())),
            ("POST", "/v1/game") => self.game(&stream, &phone, &req.body),
            ("POST", "/v1/end") => {
                self.end(&phone);
                json(&stream, &json!({ "ok": true }))
            }
            _ => error(&stream, "404 Not Found", "nothing here"),
        }
    }

    fn frame(&self, stream: &TcpStream, phone: &Phone, req: &Request) -> io::Result<()> {
        let img = match decode(&req.body) {
            Ok(img) => img,
            Err(why) => return error(stream, "415 Unsupported Media Type", &why),
        };
        let t = req.query.get("t").and_then(|t| t.parse::<u64>().ok());
        let said = {
            let mut slot = lock(&phone.session);
            if slot.is_none() {
                let game = lock(&phone.game).clone();
                let cfg = (self.cfg.make)(&player_for(&phone.id), game.as_deref().map(game_for_title));
                let rt = match Runtime::new(cfg) {
                    Ok(rt) => rt,
                    Err(e) => return error(stream, "500 Internal Server Error", &format!("could not start: {e}")),
                };
                let mut source = SourceInfo::new(SourceKind::Screen);
                source.path = Some(format!("phone:{}", phone.id));
                let now = Instant::now();
                *slot =
                    Some(Session { rt, source: Arc::new(source), frames: 0, started: now, last_t: 0, last_frame: now });
                let mut said = lock(&phone.said);
                said.watching = true;
                phone.ready.notify_all();
            }
            let Some(s) = slot.as_mut() else {
                return error(stream, "500 Internal Server Error", "no session");
            };
            let now = Instant::now();
            let t = t.unwrap_or_else(|| now.duration_since(s.started).as_millis() as u64);
            // Time only moves forward.
            let t = if s.frames == 0 { t } else { t.max(s.last_t + 1) };
            let frame = Frame::new(s.frames, t, img, s.source.clone());
            s.frames += 1;
            s.last_t = t;
            s.last_frame = now;
            let step = s.rt.on_frame(&frame);
            step.shown.into_iter().map(|a| a.text).collect::<Vec<_>>()
        };
        for text in &said {
            self.push(phone, text, false);
        }
        json(stream, &json!({ "ok": true, "say": said, "mouth": listening(phone), "next": self.cfg.capture }))
    }

    fn say(&self, stream: &TcpStream, phone: &Phone, after: Option<u64>) -> io::Result<()> {
        *lock(&phone.last_poll) = Some(Instant::now());
        let mut said = lock(&phone.said);
        let lines = match after {
            // A mouth that just started only wants what comes next.
            None => Vec::new(),
            Some(after) => {
                let deadline = Instant::now() + POLL;
                loop {
                    let fresh: Vec<SaidLine> = said.lines.iter().filter(|l| l.seq > after).cloned().collect();
                    let left = deadline.saturating_duration_since(Instant::now());
                    if !fresh.is_empty() || left.is_zero() || self.stop.load(Ordering::Relaxed) {
                        break fresh;
                    }
                    said = phone
                        .ready
                        .wait_timeout(said, left.min(Duration::from_secs(1)))
                        .unwrap_or_else(|e| e.into_inner())
                        .0;
                }
            }
        };
        let answer = json!({ "lines": lines, "last": said.last(), "watching": said.watching });
        drop(said);
        *lock(&phone.last_poll) = Some(Instant::now());
        json(stream, &answer)
    }

    fn game(&self, stream: &TcpStream, phone: &Phone, body: &[u8]) -> io::Result<()> {
        let title = serde_json::from_slice::<serde_json::Value>(body)
            .ok()
            .and_then(|v| v.get("title").and_then(|t| t.as_str()).map(|t| t.trim().to_string()))
            .unwrap_or_default();
        let title: String = title.chars().take(80).collect();
        *lock(&phone.game) = (!title.is_empty()).then(|| title.clone());
        if !title.is_empty()
            && let Some(s) = lock(&phone.session).as_ref()
        {
            let (game_id, title) = game_for_title(&title);
            let _ = s.rt.commands().send(Command::Confirm { game_id, title });
        }
        json(stream, &json!({ "ok": true, "game": (!title.is_empty()).then_some(title) }))
    }

    fn push(&self, phone: &Phone, text: &str, end: bool) {
        let mut said = lock(&phone.said);
        said.next += 1;
        let seq = said.next;
        said.lines.push_back(SaidLine { seq, text: text.to_string(), end });
        while said.lines.len() > KEEP_LINES {
            said.lines.pop_front();
        }
        if end {
            said.watching = false;
        }
        phone.ready.notify_all();
    }

    /// Ends the phone's session, if it has one: everything saved, and the summary said.
    fn end(&self, phone: &Phone) {
        let Some(mut s) = lock(&phone.session).take() else {
            return;
        };
        s.rt.finish();
        let text =
            s.rt.advice_log()
                .last()
                .filter(|r| r.shown)
                .map(|r| r.advice.text.clone())
                .unwrap_or_else(|| "That's it for now.".into());
        self.push(phone, &text, true);
    }

    fn end_idle(&self) {
        let phones: Vec<_> = lock(&self.phones).values().cloned().collect();
        for p in phones {
            let idle = lock(&p.session).as_ref().is_some_and(|s| s.last_frame.elapsed() >= self.cfg.idle);
            if idle {
                self.end(&p);
            }
        }
    }
}

fn listening(phone: &Phone) -> bool {
    lock(&phone.last_poll).is_some_and(|t| t.elapsed() < LISTENING)
}

/// A frame from its JPEG or PNG bytes, refusing absurd sizes.
fn decode(bytes: &[u8]) -> Result<image::RgbaImage, String> {
    let mut reader = image::io::Reader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| "send a JPEG or PNG".to_string())?;
    let mut limits = image::io::Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    reader.limits(limits);
    let img = reader.decode().map_err(|_| "send a JPEG or PNG".to_string())?.to_rgba8();
    if img.width() < 32 || img.height() < 32 {
        return Err("that frame is too small".into());
    }
    Ok(img)
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::path::Path;

    use syrup_capture::{Capture, FrameSource};
    use syrup_perception::NoOcr;
    use syrup_testgames::{GameKind, Session as Game};

    use super::*;

    const TOKEN: &str = "s3cret";

    fn server(dir: &Path, idle: Duration) -> Server {
        let dir = dir.to_path_buf();
        Server::start(ServerConfig {
            bind: "127.0.0.1".into(),
            port: 0,
            token: Some(TOKEN.into()),
            pairing: None,
            idle,
            capture: CaptureAdvice::default(),
            make: Arc::new(move |player, confirm| {
                let mut cfg = RuntimeConfig::new(&dir, Arc::new(NoOcr));
                cfg.player_id = player.to_string();
                cfg.confirm = confirm;
                cfg
            }),
        })
        .unwrap()
    }

    fn call(addr: SocketAddr, method: &str, path: &str, token: Option<&str>, body: &[u8]) -> (u16, serde_json::Value) {
        let mut s = TcpStream::connect(addr).unwrap();
        let auth = token.map(|t| format!("Authorization: Bearer {t}\r\n")).unwrap_or_default();
        write!(s, "{method} {path} HTTP/1.1\r\nHost: syrup\r\n{auth}Content-Length: {}\r\n\r\n", body.len()).unwrap();
        s.write_all(body).unwrap();
        let mut out = Vec::new();
        s.read_to_end(&mut out).unwrap();
        let split = out.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        let head = String::from_utf8_lossy(&out[..split]).to_string();
        let status = head.split_whitespace().nth(1).unwrap().parse().unwrap();
        let body = serde_json::from_slice(&out[split + 4..]).unwrap_or(serde_json::Value::Null);
        (status, body)
    }

    fn jpeg(img: &image::RgbaImage) -> Vec<u8> {
        let rgb = image::DynamicImage::ImageRgba8(img.clone()).to_rgb8();
        let mut out = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 70).encode_image(&rgb).unwrap();
        out
    }

    /// The phone app's mouth: asks what to say until the session's last line.
    fn mouth(addr: SocketAddr, device: &str) -> JoinHandle<Vec<SaidLine>> {
        let device = device.to_string();
        std::thread::spawn(move || {
            let (_, first) = call(addr, "GET", &format!("/v1/say?device={device}"), Some(TOKEN), b"");
            let mut after = first["last"].as_u64().unwrap();
            let mut heard = Vec::new();
            let until = Instant::now() + Duration::from_secs(120);
            while Instant::now() < until {
                let (status, answer) =
                    call(addr, "GET", &format!("/v1/say?device={device}&after={after}"), Some(TOKEN), b"");
                assert_eq!(status, 200);
                after = answer["last"].as_u64().unwrap();
                let lines: Vec<SaidLine> = serde_json::from_value(answer["lines"].clone()).unwrap();
                let done = lines.iter().any(|l| l.end);
                heard.extend(lines);
                if done {
                    break;
                }
            }
            heard
        })
    }

    #[test]
    fn a_phone_shows_its_screen_and_hears_syrup() {
        let dir = tempfile::tempdir().unwrap();
        let srv = server(dir.path(), Duration::from_secs(60));
        let device = "4F2C9A10-7B3E-4D2A-9C1B-0E5F6A7B8C9D";
        let listen = mouth(srv.addr, device);
        std::thread::sleep(Duration::from_millis(200));

        let (status, answer) =
            call(srv.addr, "POST", &format!("/v1/game?device={device}"), Some(TOKEN), br#"{"title":"Dungeon 3D"}"#);
        assert_eq!((status, answer["game"].as_str()), (200, Some("Dungeon 3D")));

        let mut game = Game::of(GameKind::Dungeon, 5, 4.0, Some(30.0));
        let mut frames = 0;
        let mut said = Vec::new();
        while let Ok(Capture::Frame(f)) = game.next() {
            let (status, answer) = call(
                srv.addr,
                "POST",
                &format!("/v1/frame?device={device}&t={}", f.timestamp_ms),
                Some(TOKEN),
                &jpeg(&f.image),
            );
            assert_eq!(status, 200, "{answer}");
            assert_eq!(answer["next"]["interval_ms"], 500);
            assert_eq!(answer["mouth"], true, "the mouth is listening");
            said.extend(answer["say"].as_array().unwrap().iter().map(|s| s.as_str().unwrap().to_string()));
            frames += 1;
        }
        assert!(frames >= 100, "{frames} frames");
        assert!(!said.is_empty(), "Syrup said nothing");

        let (status, _) = call(srv.addr, "POST", &format!("/v1/end?device={device}"), Some(TOKEN), b"");
        assert_eq!(status, 200);
        let heard = listen.join().unwrap();
        for l in &heard {
            eprintln!("phone heard #{}{}: {}", l.seq, if l.end { " (end)" } else { "" }, l.text);
        }
        // The mouth heard everything the eyes were told, in order, then the summary.
        let texts: Vec<_> = heard.iter().filter(|l| !l.end).map(|l| l.text.clone()).collect();
        assert_eq!(texts, said);
        let last = heard.last().unwrap();
        assert!(last.end, "{heard:?}");
        assert!(heard.windows(2).all(|w| w[0].seq < w[1].seq));

        // The game was the one the player named, and what Syrup learned was kept for this phone.
        let store = syrup_memory::MemoryStore::open(dir.path()).unwrap();
        let profile = store.load_profile("dungeon-3d").or_else(|| store.profiles().into_iter().next()).unwrap();
        assert_eq!(profile.title, "Dungeon 3D");
        assert!(!profile.known_ui_elements.is_empty());
        assert!(store.load_player::<syrup_player::PlayerModel>(&player_for(device)).is_some());
    }

    #[test]
    fn a_broadcast_that_stops_ends_on_its_own() {
        let dir = tempfile::tempdir().unwrap();
        let srv = server(dir.path(), Duration::from_millis(800));
        let device = "idle-phone";
        let listen = mouth(srv.addr, device);
        let mut game = Game::of(GameKind::Scroller, 2, 4.0, Some(3.0));
        while let Ok(Capture::Frame(f)) = game.next() {
            let (status, _) =
                call(srv.addr, "POST", &format!("/v1/frame?device={device}"), Some(TOKEN), &jpeg(&f.image));
            assert_eq!(status, 200);
        }
        let heard = listen.join().unwrap();
        assert!(heard.last().is_some_and(|l| l.end), "{heard:?}");
        let (_, answer) = call(srv.addr, "GET", &format!("/v1/say?device={device}"), Some(TOKEN), b"");
        assert_eq!(answer["watching"], false);
    }

    #[test]
    fn strangers_and_nonsense_are_turned_away() {
        let dir = tempfile::tempdir().unwrap();
        let srv = server(dir.path(), Duration::from_secs(60));
        assert_eq!(call(srv.addr, "GET", "/healthz", None, b"").0, 200);
        assert_eq!(call(srv.addr, "GET", "/v1/say?device=abc", None, b"").0, 401);
        assert_eq!(call(srv.addr, "GET", "/v1/say?device=abc", Some("wrong"), b"").0, 401);
        assert_eq!(call(srv.addr, "GET", "/v1/say", Some(TOKEN), b"").0, 400);
        assert_eq!(call(srv.addr, "GET", "/v1/say?device=a%20b", Some(TOKEN), b"").0, 400);
        assert_eq!(call(srv.addr, "POST", "/v1/frame?device=abc", Some(TOKEN), b"not a picture").0, 415);
        assert_eq!(call(srv.addr, "POST", "/v1/nothing?device=abc", Some(TOKEN), b"").0, 404);
        assert_eq!(call(srv.addr, "GET", "/v1/say?device=abc", Some(TOKEN), b"").0, 200);
        let tiny = jpeg(&image::RgbaImage::from_pixel(8, 8, image::Rgba([1, 2, 3, 255])));
        assert_eq!(call(srv.addr, "POST", "/v1/frame?device=abc", Some(TOKEN), &tiny).0, 415);
    }

    #[test]
    fn without_a_token_the_server_belongs_to_the_first_phone() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("phones.json");
        let start = |one_more: bool| {
            let data = dir.path().to_path_buf();
            Server::start(ServerConfig {
                bind: "127.0.0.1".into(),
                port: 0,
                token: None,
                pairing: Some(Pairing { file: file.clone(), one_more }),
                idle: Duration::from_secs(60),
                capture: CaptureAdvice::default(),
                make: Arc::new(move |player, _| {
                    let mut cfg = RuntimeConfig::new(&data, Arc::new(NoOcr));
                    cfg.player_id = player.to_string();
                    cfg
                }),
            })
            .unwrap()
        };
        let srv = start(false);
        let say = |srv: &Server, d: &str| call(srv.addr, "GET", &format!("/v1/say?device={d}"), None, b"").0;
        assert_eq!(say(&srv, "first-phone"), 200);
        assert_eq!(say(&srv, "first-phone"), 200);
        assert_eq!(say(&srv, "second-phone"), 403);
        drop(srv);
        // Remembered after a restart; one more only when asked.
        let srv = start(false);
        assert_eq!((say(&srv, "first-phone"), say(&srv, "second-phone")), (200, 403));
        drop(srv);
        let srv = start(true);
        assert_eq!((say(&srv, "second-phone"), say(&srv, "third-phone")), (200, 403));
        assert_eq!(say(&srv, "first-phone"), 200);
        let kept: Vec<String> = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(kept, ["first-phone", "second-phone"]);
    }

    #[test]
    fn a_phone_id_becomes_a_player() {
        assert_eq!(player_for("4F2C9A10-7B3E-4D2A"), "phone-4f2c9a10");
        assert!(valid_device("4F2C9A10-7B3E_x") && !valid_device("") && !valid_device("a/b"));
        assert!(same("abc", "abc") && !same("abc", "abd") && !same("abc", "abcd"));
    }
}
