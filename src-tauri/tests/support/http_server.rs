// A small real HTTP server for tests of the network broker. It listens on a
// loopback port, answers with whatever the test's handler says, and remembers
// every request it received, so a test can prove what did and did not reach it.
//
// Shared by the unit tests under `src/plugins/external` (through `include!`)
// and by the integration tests.

use std::io::{Read as _, Write as _};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::{Arc as StdArc, Mutex as StdMutex};
use std::time::Duration as StdDuration;

/// One request as the server received it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Received {
    pub method: String,
    /// Path and query, as written on the request line.
    pub target: String,
    /// Lower-case names.
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Received {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

/// What the server sends back.
pub enum Reply {
    Response {
        status: u16,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
        /// Wait this long before answering.
        delay: StdDuration,
    },
    /// Bytes written as they are, for answers that are not HTTP.
    Raw(Vec<u8>),
}

impl Reply {
    pub fn text(status: u16, body: &str) -> Reply {
        Reply::Response {
            status,
            headers: vec![("content-type".into(), "text/plain".into())],
            body: body.as_bytes().to_vec(),
            delay: StdDuration::ZERO,
        }
    }

    pub fn json(status: u16, body: &str) -> Reply {
        Reply::Response {
            status,
            headers: vec![("content-type".into(), "application/json".into())],
            body: body.as_bytes().to_vec(),
            delay: StdDuration::ZERO,
        }
    }

    pub fn redirect(status: u16, location: &str) -> Reply {
        Reply::Response {
            status,
            headers: vec![("location".into(), location.into())],
            body: Vec::new(),
            delay: StdDuration::ZERO,
        }
    }

    pub fn after(self, wait: StdDuration) -> Reply {
        match self {
            Reply::Response {
                status,
                headers,
                body,
                ..
            } => Reply::Response {
                status,
                headers,
                body,
                delay: wait,
            },
            raw => raw,
        }
    }
}

type Handler = dyn Fn(&Received) -> Reply + Send + Sync;

pub struct TestServer {
    pub addr: SocketAddr,
    received: StdArc<StdMutex<Vec<Received>>>,
    stop: StdArc<AtomicBool>,
}

fn read_request(stream: &mut TcpStream) -> Option<Received> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        if let Some(at) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break at;
        }
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return None,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
        if buf.len() > 1024 * 1024 {
            return None;
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let mut lines = head.split("\r\n");
    let mut first = lines.next()?.split(' ');
    let (method, target) = (first.next()?.to_string(), first.next()?.to_string());
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(n, v)| (n.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect();
    let length = headers
        .iter()
        .find(|(n, _)| n == "content-length")
        .and_then(|(_, v)| v.parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < length {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => body.extend_from_slice(&chunk[..n]),
        }
    }
    Some(Received {
        method,
        target,
        headers,
        body: String::from_utf8_lossy(&body).to_string(),
    })
}

impl TestServer {
    pub fn start(handler: impl Fn(&Received) -> Reply + Send + Sync + 'static) -> TestServer {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let addr = listener.local_addr().expect("its address");
        let received = StdArc::new(StdMutex::new(Vec::new()));
        let stop = StdArc::new(AtomicBool::new(false));
        let handler: StdArc<Handler> = StdArc::new(handler);
        let (log, flag) = (received.clone(), stop.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                if flag.load(AtomicOrdering::SeqCst) {
                    break;
                }
                let Ok(mut stream) = stream else { continue };
                let (log, handler) = (log.clone(), handler.clone());
                std::thread::spawn(move || {
                    let _ = stream.set_read_timeout(Some(StdDuration::from_secs(5)));
                    let Some(request) = read_request(&mut stream) else {
                        return;
                    };
                    log.lock().unwrap().push(request.clone());
                    match handler(&request) {
                        Reply::Raw(bytes) => {
                            let _ = stream.write_all(&bytes);
                        }
                        Reply::Response {
                            status,
                            headers,
                            body,
                            delay,
                        } => {
                            if !delay.is_zero() {
                                std::thread::sleep(delay);
                            }
                            let mut head = format!("HTTP/1.1 {status} Test\r\ncontent-length: {}\r\nconnection: close\r\n", body.len());
                            for (name, value) in headers {
                                head.push_str(&format!("{name}: {value}\r\n"));
                            }
                            head.push_str("\r\n");
                            let _ = stream.write_all(head.as_bytes());
                            let _ = stream.write_all(&body);
                        }
                    }
                    let _ = stream.flush();
                });
            }
        });
        TestServer {
            addr,
            received,
            stop,
        }
    }

    /// `http://127.0.0.1:<port>`: the origin a grant has to name.
    pub fn origin(&self) -> String {
        format!("http://{}", self.addr)
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.origin())
    }

    /// Everything that reached the server, in order.
    pub fn received(&self) -> Vec<Received> {
        self.received.lock().unwrap().clone()
    }

    pub fn hits(&self) -> usize {
        self.received.lock().unwrap().len()
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.stop.store(true, AtomicOrdering::SeqCst);
        // Wake the accept loop so the thread ends.
        let _ = TcpStream::connect(self.addr);
    }
}
