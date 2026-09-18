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

/// Build and run a program, as standard output and whether it succeeded.
fn run(program: &std::path::Path) -> (String, bool) {
    let output = Command::new(env!("CARGO_BIN_EXE_skuld"))
        .arg("run")
        .arg(program)
        .output()
        .expect("run skuld");
    let mut reported = String::from_utf8_lossy(&output.stdout).into_owned();
    if !output.status.success() {
        reported.push_str(&String::from_utf8_lossy(&output.stderr));
    }
    (reported, output.status.success())
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
    // The failure says why, and the why is the system's own words: POSIX
    // answers "Connection refused" and Windows "No connection could be made
    // because the target machine actively refused it". Asserting one system's
    // sentence would be asserting that the other is wrong, so what is checked
    // is that the peer is named and a reason was appended at all.
    let printed = String::from_utf8_lossy(&output.stdout);
    let prefix = format!("could not connect to 127.0.0.1:{port}: ");
    assert!(printed.starts_with(&prefix), "{printed}");
    assert!(
        printed.len() > prefix.len() + 1,
        "the failure named no reason: {printed}"
    );
}

/// Like `serve_once`, but reads the body a request announces rather than
/// stopping at the end of the head.
fn serve_once_with_body(
    listener: TcpListener,
    response: &'static [u8],
) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        listener
            .set_nonblocking(true)
            .expect("a listener that can time out");
        let deadline = Instant::now() + PATIENCE;
        let (mut stream, _) = loop {
            match listener.accept() {
                Ok(accepted) => break accepted,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline, "no client connected");
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("accept: {error}"),
            }
        };
        stream.set_nonblocking(false).expect("a blocking stream");
        stream
            .set_read_timeout(Some(PATIENCE))
            .expect("a read that can time out");
        let mut request = Vec::new();
        let mut buffer = [0u8; 512];
        // The head first, then exactly what `Content-Length` announced. A
        // reader that stopped at the blank line would report the body missing
        // when it was only unread, which is the wrong thing to fail on.
        let mut head_end = None;
        loop {
            let read = stream.read(&mut buffer).expect("read the request");
            request.extend_from_slice(&buffer[..read]);
            if head_end.is_none() {
                head_end = request
                    .windows(4)
                    .position(|window| window == b"\r\n\r\n")
                    .map(|at| at + 4);
            }
            if let Some(head) = head_end {
                let text = String::from_utf8_lossy(&request[..head]).to_lowercase();
                let announced = text
                    .split("content-length:")
                    .nth(1)
                    .and_then(|rest| rest.split("\r\n").next())
                    .and_then(|value| value.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                if request.len() >= head + announced {
                    break;
                }
            }
            if read == 0 {
                break;
            }
        }
        stream.write_all(response).expect("write the response");
        stream.flush().expect("flush");
        drop(stream);
        request
    })
}

/// Headers the caller wrote reach the server, in the order they gave, after
/// the two the client sets for itself.
#[test]
fn a_request_carries_the_headers_it_was_given() {
    if !clang_available() {
        eprintln!("skipping: clang is not on PATH");
        return;
    }
    let listener = TcpListener::bind("127.0.0.1:0").expect("an ephemeral port");
    let port = listener.local_addr().expect("address").port();
    let server = serve_once(
        listener,
        b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n",
    );

    let scratch = Scratch::new("headers");
    let program = scratch.program(&format!(
        r#"import "std/http"

func main() {{
    var head: []http.Header = []
    head.push(new http.Header(name: "User-Agent", value: "skuld/1"))
    head.push(new http.Header(name: "Authorization", value: "Bearer token"))
    let ask = new http.Request(url: "http://127.0.0.1:{port}/x", headers: head)
    match http.send(ask) {{
        Ok(response): print("status ${{response.status}}")
        Err(problem): print(http.describe(problem))
    }}
}}
"#
    ));
    let (out, ok) = run(&program);
    assert!(ok, "{out}");
    assert_eq!(out, "status 204\n");

    let request = String::from_utf8_lossy(&server.join().expect("the server thread")).into_owned();
    assert!(request.starts_with("GET /x HTTP/1.1\r\n"), "{request}");
    assert!(request.contains("\r\nUser-Agent: skuld/1\r\n"), "{request}");
    assert!(
        request.contains("\r\nAuthorization: Bearer token\r\n"),
        "{request}"
    );
    // The client's own two are still there, and still first.
    assert!(
        request.find("Host:") < request.find("User-Agent:"),
        "{request}"
    );
}

/// A method and a body reach the server, with the length measured in bytes.
#[test]
fn a_post_carries_its_body_and_a_length_in_bytes() {
    if !clang_available() {
        eprintln!("skipping: clang is not on PATH");
        return;
    }
    let listener = TcpListener::bind("127.0.0.1:0").expect("an ephemeral port");
    let port = listener.local_addr().expect("address").port();
    let server = serve_once_with_body(listener, b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");

    let scratch = Scratch::new("post");
    // `ação` is six bytes and four characters, so a length counted wrong is
    // visible rather than merely possible.
    let program = scratch.program(&format!(
        r#"import "std/http"

func main() {{
    let ask = new http.Request(
        url: "http://127.0.0.1:{port}/submit",
        method: "POST",
        body: "ação",
    )
    match http.send(ask) {{
        Ok(response): print("status ${{response.status}} ${{response.body}}")
        Err(problem): print(http.describe(problem))
    }}
}}
"#
    ));
    let (out, ok) = run(&program);
    assert!(ok, "{out}");
    assert_eq!(out, "status 200 ok\n");

    let request = String::from_utf8_lossy(&server.join().expect("the server thread")).into_owned();
    assert!(
        request.starts_with("POST /submit HTTP/1.1\r\n"),
        "{request}"
    );
    assert!(request.contains("\r\nContent-Length: 6\r\n"), "{request}");
    assert!(request.ends_with("\r\n\r\nação"), "{request}");
}

/// A header value carrying a newline would end the header and start a second
/// request the caller never wrote. It is refused before a socket is opened.
#[test]
fn a_header_that_would_split_the_request_is_refused() {
    if !clang_available() {
        eprintln!("skipping: clang is not on PATH");
        return;
    }
    let scratch = Scratch::new("split");
    let program = scratch.program(
        r#"import "std/http"

func main() {
    var head: []http.Header = []
    head.push(new http.Header(name: "X-Note", value: "fine\r\nX-Injected: not fine"))
    let ask = new http.Request(url: "http://127.0.0.1:1/x", headers: head)
    match http.send(ask) {
        Ok(response): print("unexpectedly sent")
        Err(problem): print(http.describe(problem))
    }
}
"#,
    );
    let (out, ok) = run(&program);
    assert!(ok, "{out}");
    assert!(out.contains("is not a usable header"), "{out}");
    // Refused before the connection, so nothing listening on port 1 matters.
    assert!(!out.contains("unexpectedly sent"), "{out}");
}
