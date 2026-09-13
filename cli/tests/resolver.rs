//! `std/dns`, end to end: the resolver is a Skuld program talking to a DNS
//! server that this test runs on a loopback port of its own.
//!
//! Nothing here touches the network or the machine's real nameserver: every
//! query goes to a socket this file binds, and every answer is one it wrote.
use std::{
    env, fs,
    net::UdpSocket,
    process::Command,
    thread::{self, JoinHandle},
};

struct Scratch {
    directory: std::path::PathBuf,
}

impl Scratch {
    fn new(name: &str) -> Self {
        let directory = env::temp_dir().join(format!("skuld-dns-{name}-{}", std::process::id()));
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

/// Answer exactly one query and stop.
///
/// `address` is the A record to hand back; `rcode` other than 0 answers with
/// no records at all, which is how a server says the name does not exist.
fn answer_once(socket: UdpSocket, address: [u8; 4], rcode: u8) -> JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut query = [0u8; 512];
        let (read, from) = socket.recv_from(&mut query).expect("one query");
        let query = &query[..read];

        let mut reply = Vec::new();
        // The id of the query, echoed: an answer that does not match is
        // ignored by the resolver.
        reply.extend_from_slice(&query[0..2]);
        // Response, recursion desired and available, plus the response code.
        reply.push(0x81);
        reply.push(0x80 | rcode);
        // One question, and one answer unless this is a failure.
        reply.extend_from_slice(&[0, 1]);
        reply.extend_from_slice(&[0, if rcode == 0 { 1 } else { 0 }]);
        reply.extend_from_slice(&[0, 0, 0, 0]);
        // The question, copied back verbatim.
        reply.extend_from_slice(&query[12..]);
        if rcode == 0 {
            // A pointer to the name at offset 12, which is where the question
            // put it: the resolver has to step over a compressed name.
            reply.extend_from_slice(&[0xC0, 0x0C]);
            // Type A, class IN, a minute of TTL, four bytes of address.
            reply.extend_from_slice(&[0, 1, 0, 1, 0, 0, 0, 60, 0, 4]);
            reply.extend_from_slice(&address);
        }
        socket.send_to(&reply, from).expect("send the answer");
        query.to_vec()
    })
}

fn run(program: &std::path::Path) -> (String, bool) {
    let output = Command::new(env!("CARGO_BIN_EXE_skuld"))
        .arg("run")
        .arg(program)
        .output()
        .expect("run skuld");
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        output.status.success(),
    )
}

/// A program that asks one server for one name and prints what came back.
fn resolver_program(port: u16, name: &str) -> String {
    format!(
        r#"import "std/dns"

func main() {{
    match dns.resolve_with("127.0.0.1", {port}, "{name}") {{
        Ok(address): print(address)
        Err(problem): print(dns.describe(problem))
    }}
}}
"#
    )
}

#[test]
fn a_name_resolves_against_a_server_this_test_runs() {
    if !clang_available() {
        eprintln!("skipping: clang is not on PATH");
        return;
    }
    let socket = UdpSocket::bind("127.0.0.1:0").expect("an ephemeral port");
    let port = socket.local_addr().expect("address").port();
    let server = answer_once(socket, [93, 184, 216, 34], 0);

    let scratch = Scratch::new("found");
    let program = scratch.program(&resolver_program(port, "example.test"));
    let (out, ok) = run(&program);
    assert!(ok);
    assert_eq!(out, "93.184.216.34\n");

    // The query was a well-formed question for that name.
    let query = server.join().expect("the server thread");
    assert_eq!(&query[4..6], &[0, 1], "one question: {query:?}");
    let labels = &query[12..];
    assert_eq!(labels[0], 7, "`example` is seven bytes");
    assert_eq!(&labels[1..8], b"example");
    assert_eq!(labels[8], 4, "`test` is four bytes");
    assert_eq!(&labels[9..13], b"test");
    assert_eq!(labels[13], 0, "the name ends with a root label");
    // QTYPE A, QCLASS IN.
    assert_eq!(&labels[14..18], &[0, 1, 0, 1]);
}

#[test]
fn a_name_the_server_refuses_is_reported_as_that() {
    if !clang_available() {
        eprintln!("skipping: clang is not on PATH");
        return;
    }
    let socket = UdpSocket::bind("127.0.0.1:0").expect("an ephemeral port");
    let port = socket.local_addr().expect("address").port();
    let server = answer_once(socket, [0, 0, 0, 0], 3);

    let scratch = Scratch::new("nxdomain");
    let program = scratch.program(&resolver_program(port, "nothing.test"));
    let (out, ok) = run(&program);
    assert!(ok, "a refusal is a value, not a crash");
    assert_eq!(out, "the nameserver answered with code 3 (no such name)\n");
    server.join().expect("the server thread");
}

#[test]
fn an_address_needs_no_server_at_all() {
    if !clang_available() {
        eprintln!("skipping: clang is not on PATH");
        return;
    }
    // Port 0 would fail if it were used; resolving a literal never asks.
    let scratch = Scratch::new("literal");
    let program = scratch.program(&resolver_program(1, "127.0.0.1"));
    let (out, ok) = run(&program);
    assert!(ok);
    assert_eq!(out, "127.0.0.1\n");
}

#[test]
fn a_server_that_never_answers_times_out_instead_of_hanging() {
    if !clang_available() {
        eprintln!("skipping: clang is not on PATH");
        return;
    }
    // Bound and never read from: the query arrives and nothing comes back.
    let socket = UdpSocket::bind("127.0.0.1:0").expect("an ephemeral port");
    let port = socket.local_addr().expect("address").port();

    let scratch = Scratch::new("timeout");
    let program = scratch.program(&resolver_program(port, "silent.test"));
    let (out, ok) = run(&program);
    assert!(ok, "a timeout is a value, not a crash");
    assert!(out.starts_with("dns query failed:"), "{out}");
    drop(socket);
}

#[test]
fn a_label_longer_than_the_protocol_allows_is_refused_before_any_socket() {
    if !clang_available() {
        eprintln!("skipping: clang is not on PATH");
        return;
    }
    let scratch = Scratch::new("badname");
    let long = "x".repeat(64);
    let program = scratch.program(&resolver_program(1, &format!("{long}.test")));
    let (out, ok) = run(&program);
    assert!(ok);
    assert!(out.contains("is not a usable host name"), "{out}");
}

/// A one-shot HTTP server, so the fetch below has something to answer it.
fn serve_once(listener: std::net::TcpListener) -> JoinHandle<()> {
    thread::spawn(move || {
        use std::io::{Read, Write};
        let (mut stream, _) = listener.accept().expect("one connection");
        let mut request = Vec::new();
        let mut buffer = [0u8; 512];
        loop {
            let read = stream.read(&mut buffer).expect("read the request");
            request.extend_from_slice(&buffer[..read]);
            if read == 0 || request.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello")
            .expect("write the response");
        stream.flush().expect("flush");
    })
}

#[test]
fn a_resolved_name_is_what_the_request_then_goes_to() {
    if !clang_available() {
        eprintln!("skipping: clang is not on PATH");
        return;
    }
    // The milestone's marker, with both halves under this test's control: a
    // name resolves against a server it runs, and the address that comes back
    // is what the HTTP request is made to.
    let dns_socket = UdpSocket::bind("127.0.0.1:0").expect("an ephemeral port");
    let dns_port = dns_socket.local_addr().expect("address").port();
    let dns = answer_once(dns_socket, [127, 0, 0, 1], 0);

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("an ephemeral port");
    let http_port = listener.local_addr().expect("address").port();
    let http = serve_once(listener);

    let scratch = Scratch::new("fetch");
    let program = scratch.program(&format!(
        r#"import "std/dns"
import "std/http"

func main() {{
    let address = dns.resolve_with("127.0.0.1", {dns_port}, "service.test") else problem {{
        print(dns.describe(problem))
        return
    }}
    print("resolved ${{address}}")
    match http.get("http://${{address}}:{http_port}/greeting") {{
        Ok(response): print("${{response.status}} ${{response.body}}")
        Err(reason): print(http.describe(reason))
    }}
}}
"#
    ));
    let (out, ok) = run(&program);
    assert!(ok, "{out}");
    assert_eq!(out, "resolved 127.0.0.1\n200 hello\n");
    dns.join().expect("the dns thread");
    http.join().expect("the http thread");
}
