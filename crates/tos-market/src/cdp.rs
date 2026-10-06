//! Just enough of the Chrome DevTools Protocol to watch a login happen.
//!
//! CDP is JSON messages over a websocket, and this crate already needs a
//! websocket client for the gateway, so speaking it directly costs nothing a
//! driver crate would have saved. What the login capture asks of a browser is
//! small and fixed: launch it, find its pages, turn the network domain on,
//! listen for two websocket events, read the cookies once, navigate once, and
//! evaluate one expression. That is this file.
//!
//! One socket carries all of it. Pages are attached with `flatten: true`, so
//! every page's traffic arrives on the same connection tagged with a session
//! id, rather than one socket per tab. Everything is blocking, on the thread
//! that asked: a reply is waited for by reading frames until it turns up, and
//! the events that arrive in the meantime are kept for the caller rather than
//! dropped — which is the whole trick, because the frames worth capturing are
//! events that land while a command is in flight.

use std::collections::HashMap;
use std::io::ErrorKind;
use std::net::TcpStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

use crate::error::{Error, Result};

/// How long a read waits before the loop gets a turn. Short enough that a
/// caller polling for traffic is not held up by a quiet browser.
const READ_TIMEOUT: Duration = Duration::from_millis(200);

/// How long any one command waits for its reply. Generous: the browser is on
/// loopback, but a page mid-navigation can take its time answering.
const CALL_TIMEOUT: Duration = Duration::from_secs(30);

/// A frame the browser sent that nobody asked for: `method` names the event,
/// `session` the page it belongs to when it belongs to one.
#[derive(Debug, Clone)]
pub struct Event {
    pub method: String,
    pub params: Value,
    pub session: Option<String>,
}

/// One page, attached, with the network domain enabled.
#[derive(Debug, Clone)]
pub struct Page {
    pub target: String,
    pub session: String,
    pub url: String,
}

/// A browser this process launched, and the DevTools socket into it.
pub struct Browser {
    child: Child,
    socket: WebSocket<MaybeTlsStream<TcpStream>>,
    next_id: i64,
    /// Events read while waiting for a reply, waiting for [`Browser::events`].
    kept: Vec<Event>,
    /// Every target the browser has told us about, by target id.
    targets: HashMap<String, Target>,
}

#[derive(Debug, Clone, Default)]
struct Target {
    kind: String,
    url: String,
    session: Option<String>,
}

impl Browser {
    /// Launches `executable` on `user_data_dir` with `flags` (each written as
    /// `--flag`) and connects to its DevTools endpoint.
    ///
    /// The debugging port is 0, which asks the browser to pick one and write
    /// it — with the path of the browser-wide endpoint — into
    /// `DevToolsActivePort` in the profile. Reading that is how the port is
    /// learned without parsing a log line, and deleting it first is what
    /// keeps a previous run's port from being read as this one's.
    pub fn launch(
        executable: &Path,
        user_data_dir: &Path,
        flags: &[&str],
        timeout: Duration,
    ) -> Result<Browser> {
        let port_file = user_data_dir.join("DevToolsActivePort");
        let _ = std::fs::remove_file(&port_file);
        let mut command = Command::new(executable);
        command
            .arg("--remote-debugging-port=0")
            .arg(format!("--user-data-dir={}", user_data_dir.display()));
        for flag in flags {
            command.arg(format!("--{flag}"));
        }
        let mut child = command
            .arg("about:blank")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| Error::Other(format!("launch browser: {e}")))?;
        // A browser left running on this profile would refuse the next login
        // for as long as it lived, so a failure to reach it kills it.
        let socket = match endpoint(&port_file, timeout).and_then(|url| connect(&url)) {
            Ok(socket) => socket,
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(e);
            }
        };
        let mut browser = Browser {
            child,
            socket,
            next_id: 0,
            kept: Vec::new(),
            targets: HashMap::new(),
        };
        // Every page the browser has, and every one it opens later.
        browser.call("Target.setDiscoverTargets", json!({"discover": true}), None)?;
        Ok(browser)
    }

    /// Sends one command and waits for the reply carrying its id.
    ///
    /// Every other frame read on the way is an event, and it is kept rather
    /// than discarded: a `webSocketFrameReceived` that arrives while a
    /// navigation is being acknowledged is exactly the frame the login
    /// capture is here for.
    pub fn call(&mut self, method: &str, params: Value, session: Option<&str>) -> Result<Value> {
        self.next_id += 1;
        let id = self.next_id;
        let mut frame = json!({"id": id, "method": method, "params": params});
        if let Some(session) = session {
            frame["sessionId"] = Value::String(session.to_string());
        }
        self.socket
            .send(Message::text(frame.to_string()))
            .map_err(|e| Error::Other(format!("devtools {method}: {e}")))?;
        let deadline = Instant::now() + CALL_TIMEOUT;
        loop {
            if let Some(value) = self.read()? {
                if value.get("id").and_then(Value::as_i64) == Some(id) {
                    if let Some(error) = value.get("error") {
                        return Err(Error::Other(format!("devtools {method}: {error}")));
                    }
                    return Ok(value.get("result").cloned().unwrap_or(Value::Null));
                }
                self.keep(value);
            }
            if Instant::now() >= deadline {
                return Err(Error::Timeout(format!("devtools {method}")));
            }
        }
    }

    /// Reads whatever the browser has to say for up to `window`, keeping the
    /// events for [`Browser::events`].
    pub fn pump(&mut self, window: Duration) -> Result<()> {
        let deadline = Instant::now() + window;
        while Instant::now() < deadline {
            if let Some(value) = self.read()? {
                self.keep(value);
            }
        }
        Ok(())
    }

    /// The events read since the last call, in the order they arrived.
    pub fn events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.kept)
    }

    /// Attaches every page that is not attached yet, enabling the network
    /// domain on each, and returns all the pages there are.
    ///
    /// `Network.enable` is part of attaching here because watching network
    /// traffic is the only reason this crate attaches to anything.
    pub fn pages(&mut self) -> Result<Vec<Page>> {
        let fresh: Vec<String> = self
            .targets
            .iter()
            .filter(|(_, t)| t.kind == "page" && t.session.is_none())
            .map(|(id, _)| id.clone())
            .collect();
        for target in fresh {
            let attached = self.call(
                "Target.attachToTarget",
                json!({"targetId": target, "flatten": true}),
                None,
            )?;
            let Some(session) = attached.get("sessionId").and_then(Value::as_str) else {
                continue;
            };
            let session = session.to_string();
            self.call("Network.enable", json!({}), Some(&session))?;
            if let Some(entry) = self.targets.get_mut(&target) {
                entry.session = Some(session);
            }
        }
        Ok(self
            .targets
            .iter()
            .filter_map(|(id, t)| {
                t.session.as_ref().filter(|_| t.kind == "page").map(|s| Page {
                    target: id.clone(),
                    session: s.clone(),
                    url: t.url.clone(),
                })
            })
            .collect())
    }

    /// Points a page at `url`, without waiting for it to finish loading.
    pub fn navigate(&mut self, page: &Page, url: &str) -> Result<()> {
        self.call("Page.navigate", json!({"url": url}), Some(&page.session))?;
        Ok(())
    }

    /// Evaluates `expression` in a page and returns what it produced.
    pub fn evaluate(&mut self, page: &Page, expression: &str) -> Result<Value> {
        let result = self.call(
            "Runtime.evaluate",
            json!({"expression": expression, "returnByValue": true}),
            Some(&page.session),
        )?;
        Ok(result["result"]["value"].clone())
    }

    /// The cookies the browser would send to `url`, HttpOnly included — which
    /// is the whole reason this goes through DevTools rather than the page.
    pub fn cookies(&mut self, page: &Page, url: &str) -> Result<Vec<(String, String)>> {
        let result = self.call(
            "Network.getCookies",
            json!({"urls": [url]}),
            Some(&page.session),
        )?;
        Ok(result["cookies"]
            .as_array()
            .map(|cookies| {
                cookies
                    .iter()
                    .filter_map(|c| {
                        Some((
                            c["name"].as_str()?.to_string(),
                            c["value"].as_str()?.to_string(),
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    /// Asks the browser to shut down, then makes sure it did.
    ///
    /// `Browser.close` is sent without waiting for a reply, because a browser
    /// that obeys it closes the socket instead of answering on it.
    pub fn close(mut self) {
        let frame = json!({"id": self.next_id + 1, "method": "Browser.close"});
        let _ = self.socket.send(Message::text(frame.to_string()));
        let _ = self.socket.close(None);
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => return,
                Err(_) => break,
                Ok(None) if Instant::now() >= deadline => break,
                Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            }
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    /// One frame, decoded, or `None` when the browser had nothing to say
    /// before the read timed out.
    fn read(&mut self) -> Result<Option<Value>> {
        match self.socket.read() {
            Ok(Message::Text(text)) => Ok(serde_json::from_str(&text).ok()),
            Ok(Message::Close(_)) => Err(Error::Other("devtools socket closed".into())),
            Ok(_) => Ok(None),
            Err(tungstenite::Error::Io(e))
                if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
            {
                Ok(None)
            }
            Err(e) => Err(Error::Other(format!("devtools socket: {e}"))),
        }
    }

    /// Files an event: the target bookkeeping is kept here so that a caller
    /// never has to understand `Target.*` to ask which pages exist.
    fn keep(&mut self, value: Value) {
        let Some(method) = value.get("method").and_then(Value::as_str) else {
            return;
        };
        let params = value.get("params").cloned().unwrap_or(Value::Null);
        match method {
            "Target.targetCreated" | "Target.targetInfoChanged" => {
                let info = &params["targetInfo"];
                if let Some(id) = info["targetId"].as_str() {
                    let entry = self.targets.entry(id.to_string()).or_default();
                    entry.kind = info["type"].as_str().unwrap_or_default().to_string();
                    entry.url = info["url"].as_str().unwrap_or_default().to_string();
                }
            }
            "Target.targetDestroyed" | "Target.detachedFromTarget" => {
                if let Some(id) = params["targetId"].as_str() {
                    self.targets.remove(id);
                }
            }
            _ => {}
        }
        self.kept.push(Event {
            method: method.to_string(),
            params,
            session: value
                .get("sessionId")
                .and_then(Value::as_str)
                .map(str::to_string),
        });
    }
}

/// The browser-wide websocket endpoint, once the browser has written it.
///
/// `DevToolsActivePort` is two lines: the port it settled on, and the path of
/// the endpoint. A browser that never writes it either failed to start or is
/// refusing the profile, and either way there is nothing to connect to.
fn endpoint(port_file: &Path, timeout: Duration) -> Result<String> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok(text) = std::fs::read_to_string(port_file) {
            let mut lines = text.lines();
            if let (Some(port), Some(path)) = (lines.next(), lines.next()) {
                let port = port.trim();
                if !port.is_empty() && !path.trim().is_empty() {
                    return Ok(format!("ws://127.0.0.1:{port}{}", path.trim()));
                }
            }
        }
        if Instant::now() >= deadline {
            return Err(Error::Timeout(format!(
                "browser never wrote {}",
                port_file.display()
            )));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Connects, and gives the socket a read timeout so that a caller waiting for
/// traffic gets its turn back on a quiet browser instead of blocking.
fn connect(url: &str) -> Result<WebSocket<MaybeTlsStream<TcpStream>>> {
    let (socket, _) = tungstenite::connect(url)
        .map_err(|e| Error::Other(format!("devtools connect {url}: {e}")))?;
    if let MaybeTlsStream::Plain(stream) = socket.get_ref() {
        stream
            .set_read_timeout(Some(READ_TIMEOUT))
            .map_err(|e| Error::Other(format!("devtools socket timeout: {e}")))?;
    }
    Ok(socket)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_endpoint_is_built_from_the_two_lines_the_browser_writes() {
        let dir = std::env::temp_dir().join(format!("tos-cdp-port-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("DevToolsActivePort");
        // Nothing written yet, and a half-written file, both wait.
        assert!(endpoint(&file, Duration::from_millis(1)).is_err());
        std::fs::write(&file, "45321\n").unwrap();
        assert!(endpoint(&file, Duration::from_millis(1)).is_err());
        std::fs::write(&file, "45321\n/devtools/browser/abc-123\n").unwrap();
        assert_eq!(
            endpoint(&file, Duration::from_millis(1)).unwrap(),
            "ws://127.0.0.1:45321/devtools/browser/abc-123"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Everything the login capture asks of a browser, against a real one.
    ///
    /// The page is a local file that opens a websocket to a server this test
    /// starts, so the whole exchange stays on loopback: no brokerage, no
    /// account, and nothing on the network. Run it with
    /// `cargo test -p tos-market -- --ignored --nocapture`; it is ignored by
    /// default because it needs a browser installed and takes seconds.
    #[test]
    #[ignore]
    fn a_real_browser_answers_the_protocol_this_speaks() {
        use std::net::TcpListener;

        let executable = ["/usr/bin/chromium", "/usr/bin/google-chrome-stable"]
            .iter()
            .map(Path::new)
            .find(|p| p.is_file())
            .expect("no chromium or chrome on this machine");

        // A websocket server that greets whoever connects, so that the page
        // has a frame to receive rather than only one to send.
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            if let Ok((stream, _)) = listener.accept() {
                if let Ok(mut ws) = tungstenite::accept(stream) {
                    let _ = ws.send(Message::text("hello from the test server"));
                    let _ = ws.read();
                }
            }
        });

        let dir = std::env::temp_dir().join(format!("tos-cdp-live-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let profile = dir.join("profile");
        std::fs::create_dir_all(&profile).unwrap();
        let html = dir.join("page.html");
        std::fs::write(
            &html,
            format!(
                "<!doctype html><title>socket</title><script>\
                 new WebSocket('ws://127.0.0.1:{port}/feed')\
                   .onmessage = e => {{ document.title = e.data; }};\
                 </script>"
            ),
        )
        .unwrap();

        // Headless here only because a test must not need a display; every
        // other flag is the login's own list.
        let flags = ["headless=new", "no-first-run", "no-default-browser-check"];
        let mut browser = Browser::launch(executable, &profile, &flags, Duration::from_secs(30))
            .expect("launch");

        let deadline = Instant::now() + Duration::from_secs(30);
        let page = loop {
            if let Some(page) = browser.pages().expect("pages").into_iter().next() {
                break page;
            }
            assert!(Instant::now() < deadline, "the browser opened no page");
            browser.pump(Duration::from_millis(200)).unwrap();
        };

        // One Network.getCookies, the call the login makes exactly once. An
        // empty jar is the right answer here; that it replies is the point.
        let cookies = browser
            .cookies(&page, "http://127.0.0.1/")
            .expect("Network.getCookies replied");

        browser
            .navigate(&page, &format!("file://{}", html.display()))
            .expect("navigate");

        let (mut created, mut received) = (false, false);
        while Instant::now() < deadline && !(created && received) {
            browser.pump(Duration::from_millis(250)).unwrap();
            for event in browser.events() {
                match event.method.as_str() {
                    "Network.webSocketCreated" => {
                        created |= event.params["url"]
                            .as_str()
                            .is_some_and(|url| url.ends_with("/feed"));
                    }
                    "Network.webSocketFrameReceived" => {
                        received |= event.params["response"]["payloadData"]
                            .as_str()
                            .is_some_and(|payload| payload.contains("hello from the test server"));
                    }
                    _ => {}
                }
            }
        }

        // Runtime.evaluate, the last call the login makes, reading what the
        // frame did to the page.
        let title = browser
            .evaluate(&page, "document.title")
            .expect("Runtime.evaluate");

        browser.close();
        let _ = server.join();
        let _ = std::fs::remove_dir_all(&dir);

        eprintln!("cookies: {}, title after the frame: {title}", cookies.len());
        assert!(created, "no Network.webSocketCreated for the page's socket");
        assert!(received, "no Network.webSocketFrameReceived carrying the frame");
        assert_eq!(title.as_str(), Some("hello from the test server"));
    }
}
