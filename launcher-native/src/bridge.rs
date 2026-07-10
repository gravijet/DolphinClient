//! Local dashboard bridge.
//!
//! The launcher runs a tiny HTTP server on `127.0.0.1` that exposes its live
//! state as JSON. The website dashboard (served over HTTPS) fetches it directly
//! — loopback origins are "potentially trustworthy", so this is not blocked as
//! mixed content, and we answer the CORS + Private-Network-Access preflight so
//! it works from a public site to a private (loopback) address.
//!
//! No web framework, no extra dependencies — just `std::net`. The app thread
//! keeps `status_json` fresh via [`Bridge::set`]; the server just serves
//! whatever is currently there.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;

/// Fixed loopback port the web dashboard probes for a running launcher.
pub const PORT: u16 = 47654;

pub struct Bridge {
    status_json: Mutex<String>,
}

impl Bridge {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            status_json: Mutex::new("{\"connected\":true}".to_string()),
        })
    }

    /// Replace the served status JSON (called by the UI thread every frame).
    pub fn set(&self, json: String) {
        if let Ok(mut s) = self.status_json.lock() {
            *s = json;
        }
    }

    fn snapshot(&self) -> String {
        self.status_json
            .lock()
            .map(|s| s.clone())
            .unwrap_or_else(|_| "{}".to_string())
    }
}

/// Start the server in a background thread. Silently no-ops if the port is
/// already in use (e.g. a second launcher instance).
pub fn start(bridge: Arc<Bridge>) {
    thread::spawn(move || {
        let Ok(listener) = TcpListener::bind(("127.0.0.1", PORT)) else {
            return;
        };
        for stream in listener.incoming().flatten() {
            let b = bridge.clone();
            thread::spawn(move || {
                let _ = handle(stream, &b);
            });
        }
    });
}

const CORS: &str = "Access-Control-Allow-Origin: *\r\n\
     Access-Control-Allow-Methods: GET, OPTIONS\r\n\
     Access-Control-Allow-Headers: *\r\n\
     Access-Control-Allow-Private-Network: true\r\n";

fn handle(mut stream: TcpStream, bridge: &Bridge) -> std::io::Result<()> {
    let mut buf = [0u8; 2048];
    let n = stream.read(&mut buf)?;
    let req = String::from_utf8_lossy(&buf[..n]);
    let mut first = req.lines().next().unwrap_or("").split_whitespace();
    let method = first.next().unwrap_or("");
    let path = first.next().unwrap_or("/");

    if method == "OPTIONS" {
        let resp = format!("HTTP/1.1 204 No Content\r\n{CORS}Content-Length: 0\r\nConnection: close\r\n\r\n");
        return stream.write_all(resp.as_bytes());
    }

    let (status, body) = if path.starts_with("/status") {
        ("200 OK", bridge.snapshot())
    } else if path.starts_with("/ping") {
        ("200 OK", "{\"ok\":true}".to_string())
    } else {
        ("404 Not Found", "{\"error\":\"not found\"}".to_string())
    };

    let resp = format!(
        "HTTP/1.1 {status}\r\n{CORS}Content-Type: application/json; charset=utf-8\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.as_bytes().len()
    );
    stream.write_all(resp.as_bytes())
}
