//! The long-range target, end to end: a Skuld program that makes an HTTP
//! request over a socket and decodes the JSON it gets back.
//!
//! The server is this test. It binds an ephemeral port on the loopback
//! interface, answers exactly one request and stops, so the suite never
//! touches the network and never depends on a port being free.
use std::{
    env, fs,
    io::{self, Read, Write},
    net::TcpListener,
    process::Command,
    thread,
    time::{Duration, Instant},
};

/// How long the fixture waits for the program under test to show up. A client
/// that never connects is a failure of the test, and it has to be reported as
/// one: `accept` on its own would wait for ever, and a hung test tells the
/// reader nothing and stops every package behind it from running at all.
const PATIENCE: Duration = Duration::from_secs(30);

/// A canned HTTP/1.1 response, and the request the client sent, once.
fn serve_once(listener: TcpListener, response: &'static [u8]) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        listener
            .set_nonblocking(true)
            .expect("a listener that can time out");
        let deadline = Instant::now() + PATIENCE;
        let (mut stream, _) = loop {
            match listener.accept() {
                Ok(accepted) => break accepted,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    assert!(
                        Instant::now() < deadline,
                        "no client connected within {PATIENCE:?}: the program under \
                         test never reached the server"
                    );
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("accept: {error}"),
            }
        };
        // The accepted stream inherits the listener's non-blocking mode on some
        // platforms and not on others; say which one this wants either way.
        stream
            .set_nonblocking(false)
            .expect("a blocking accepted stream");
        stream
            .set_read_timeout(Some(PATIENCE))
            .expect("a read that can time out");
        // The client sends `Connection: close` and a request with no body, so
        // the head is everything there is to read.
        let mut request = Vec::new();
        let mut buffer = [0u8; 512];
        loop {
            let read = stream.read(&mut buffer).expect("read the request");
            request.extend_from_slice(&buffer[..read]);
            if read == 0 || request.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }
        stream.write_all(response).expect("write the response");
        stream.flush().expect("flush");
        drop(stream);
        request
    })
}

struct Scratch {
    directory: std::path::PathBuf,
}

impl Scratch {
    fn new(name: &str) -> Self {
        let directory = env::temp_dir().join(format!("skuld-net-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).expect("scratch directory");
        Self { directory }
    }
    fn program(&self, source: &str) -> std::path::PathBuf {
        let path = self.directory.join("main.skuld");
        fs::write(&path, source).expect("program source");
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

fn clang_available() -> bool {
    Command::new("clang")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
}

#[test]
fn a_skuld_program_fetches_and_decodes_json_over_http() {
    if !clang_available() {
        eprintln!("skipping: clang is not on PATH");
        return;
    }
    let listener = TcpListener::bind("127.0.0.1:0").expect("an ephemeral port");
    let port = listener.local_addr().expect("address").port();
    let server = serve_once(
        listener,
        b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 46\r\n\r\n\
          {\"name\":\"skuld\",\"port\":8080,\"tags\":[\"native\"]}\n",
    );

    let scratch = Scratch::new("json");
    let program = scratch.program(&format!(
        r#"import "std/http"
import "std/json"

func main() {{
    match http.get("http://127.0.0.1:{port}/data.json") {{
        Ok(response): {{
            print("status ${{response.status}}")
            if let kind = response.header("Content-Type") {{
                print(kind)
            }}
            match json.parse(response.body) {{
                Ok(document): {{
                    if let name = json.lookup(document, "name") {{
                        print(json.render(name))
                    }}
                    if let tags = json.lookup(document, "tags") {{
                        print(json.render(tags))
                    }}
                }}
                Err(reason): print(json.describe(reason))
            }}
        }}
        Err(reason): print(http.describe(reason))
    }}
}}
"#
    ));

    let output = Command::new(env!("CARGO_BIN_EXE_skuld"))
        .arg("run")
        .arg(&program)
        .output()
        .expect("run skuld");
    let request = server.join().expect("the server thread");
    let request = String::from_utf8_lossy(&request);

    assert!(
        output.status.success(),
        "the program should run cleanly:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "status 200\napplication/json\n\"skuld\"\n[\"native\"]\n"
    );
    // The request the client actually produced, which is the other half of
    // the contract: a server the test does not control has to understand it.
    assert!(
        request.starts_with("GET /data.json HTTP/1.1\r\n"),
        "unexpected request line:\n{request}"
    );
    assert!(
        request.contains(&format!("Host: 127.0.0.1:{port}\r\n")),
        "a request must carry a Host header:\n{request}"
    );
    assert!(
        request.contains("Connection: close\r\n"),
        "this client opens one connection per request:\n{request}"
    );
}

#[test]
fn a_refused_connection_is_an_error_and_not_a_crash() {
    if !clang_available() {
        eprintln!("skipping: clang is not on PATH");
        return;
    }
    // Binding and dropping gives a port nothing is listening on.
    let port = {
        let listener = TcpListener::bind("127.0.0.1:0").expect("an ephemeral port");
        listener.local_addr().expect("address").port()
    };
    let scratch = Scratch::new("refused");
    let program = scratch.program(&format!(
        r#"import "std/http"

func main() {{
    match http.get("http://127.0.0.1:{port}/") {{
        Ok(response): print("unexpected ${{response.status}}")
        Err(reason): print(http.describe(reason))
    }}
}}
"#
    ));
    let output = Command::new(env!("CARGO_BIN_EXE_skuld"))
        .arg("run")
        .arg(&program)
        .output()
        .expect("run skuld");
    assert!(output.status.success(), "a refusal is a value, not a crash");
    // The failure now says why, not just which step failed.
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!("could not connect to 127.0.0.1:{port}: Connection refused\n")
    );
}
