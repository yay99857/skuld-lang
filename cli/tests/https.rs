//! `std/tls` and `std/https`, end to end, against a TLS server this test runs
//! with a certificate authority it creates.
//!
//! Nothing here reaches the public network, and no test disables
//! verification — the point of the milestone is that a certificate is
//! checked, so every case here is about which certificates are accepted and
//! which are refused.
//!
//! The suite needs `openssl` on PATH for the server and the test CA, and
//! `clang` for the program; without either it skips, the way the rest of the
//! native tests do.
use std::{
    env, fs,
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
};

struct Scratch {
    directory: PathBuf,
}

impl Scratch {
    fn new(name: &str) -> Self {
        let directory = env::temp_dir().join(format!("skuld-tls-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).expect("scratch directory");
        Self { directory }
    }
    fn path(&self, name: &str) -> PathBuf {
        self.directory.join(name)
    }
    fn program(&self, source: &str) -> PathBuf {
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

/// A server process that is killed when the test ends, however it ends.
struct Server {
    child: Child,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn tool_available(name: &str) -> bool {
    Command::new(name)
        .arg("version")
        .output()
        .is_ok_and(|output| output.status.success())
}

fn clang_available() -> bool {
    Command::new("clang")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
}

fn openssl(arguments: &[&str]) {
    let output = Command::new("openssl")
        .args(arguments)
        .output()
        .expect("run openssl");
    assert!(
        output.status.success(),
        "openssl {arguments:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A certificate authority, and a certificate for `localhost` signed by it.
///
/// `validity` is the pair of timestamps the certificate carries, so the same
/// function produces the good certificate and the expired one.
fn certificates(
    scratch: &Scratch,
    name: &str,
    validity: Option<(&str, &str)>,
) -> (PathBuf, PathBuf) {
    let ca_key = scratch.path("ca.key");
    let ca = scratch.path("ca.pem");
    openssl(&[
        "req",
        "-x509",
        "-newkey",
        "rsa:2048",
        "-nodes",
        "-keyout",
        ca_key.to_str().unwrap(),
        "-out",
        ca.to_str().unwrap(),
        "-subj",
        "/CN=Skuld Test CA",
        "-days",
        "2",
    ]);

    let key = scratch.path(&format!("{name}.key"));
    let request = scratch.path(&format!("{name}.csr"));
    openssl(&[
        "req",
        "-newkey",
        "rsa:2048",
        "-nodes",
        "-keyout",
        key.to_str().unwrap(),
        "-out",
        request.to_str().unwrap(),
        "-subj",
        "/CN=localhost",
    ]);

    let extensions = scratch.path("ext.cnf");
    fs::write(&extensions, "subjectAltName=DNS:localhost\n").expect("extensions");
    let certificate = scratch.path(&format!("{name}.pem"));
    match validity {
        // `x509 -not_before/-not_after` says this in one option each, but both
        // are recent enough that the openssl on the CI runner rejects them, so
        // this file had never once passed there. `ca` has taken `-startdate`
        // and `-enddate` for far longer, and the price is the small database
        // it insists on keeping, written here beside the certificate itself.
        Some((from, to)) => {
            let database = scratch.path("index.txt");
            let serial = scratch.path("serial");
            let configuration = scratch.path("ca.cnf");
            fs::write(&database, "").expect("certificate database");
            fs::write(&serial, "01\n").expect("serial");
            // The config parser reads a backslash as an escape, so a path goes
            // in with forward slashes. Everywhere but Windows that is what it
            // already had.
            let readable = |path: &Path| path.display().to_string().replace('\\', "/");
            fs::write(
                &configuration,
                format!(
                    "[ca]\ndefault_ca = CA_default\n\
                     [CA_default]\ndatabase = {}\nserial = {}\nnew_certs_dir = {}\n\
                     default_md = sha256\npolicy = policy_any\n\
                     email_in_dn = no\nrand_serial = no\nunique_subject = no\n\
                     [policy_any]\ncommonName = supplied\n",
                    readable(&database),
                    readable(&serial),
                    readable(&scratch.directory),
                ),
            )
            .expect("ca configuration");
            openssl(&[
                "ca",
                "-batch",
                "-notext",
                "-config",
                configuration.to_str().unwrap(),
                "-cert",
                ca.to_str().unwrap(),
                "-keyfile",
                ca_key.to_str().unwrap(),
                "-in",
                request.to_str().unwrap(),
                "-out",
                certificate.to_str().unwrap(),
                "-extfile",
                extensions.to_str().unwrap(),
                "-startdate",
                from,
                "-enddate",
                to,
            ]);
        }
        None => openssl(&[
            "x509",
            "-req",
            "-in",
            request.to_str().unwrap(),
            "-CA",
            ca.to_str().unwrap(),
            "-CAkey",
            ca_key.to_str().unwrap(),
            "-CAcreateserial",
            "-out",
            certificate.to_str().unwrap(),
            "-extfile",
            extensions.to_str().unwrap(),
            "-days",
            "2",
        ]),
    }
    (ca, certificate)
}

/// Start `openssl s_server` on a free port and wait for it to listen.
fn serve(scratch: &Scratch, certificate: &Path, name: &str) -> (Server, u16) {
    let port = {
        let listener = TcpListener::bind("127.0.0.1:0").expect("an ephemeral port");
        listener.local_addr().expect("address").port()
    };
    let key = scratch.path(&format!("{name}.key"));
    let child = Command::new("openssl")
        .args([
            "s_server",
            "-accept",
            &format!("127.0.0.1:{port}"),
            "-cert",
            certificate.to_str().unwrap(),
            "-key",
            key.to_str().unwrap(),
            "-www",
            "-quiet",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start s_server");
    // Wait for the port to answer rather than sleeping a fixed time.
    for _ in 0..200 {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    (Server { child }, port)
}

/// Build and run a program with the test CA as the trust store.
fn run(program: &Path, trust: Option<&Path>) -> (String, bool) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_skuld"));
    command.arg("run").arg(program).arg("-lssl").arg("-lcrypto");
    match trust {
        // OpenSSL reads this when the default verify paths are loaded, so the
        // test's own CA is the whole trust store for that program.
        Some(ca) => command.env("SSL_CERT_FILE", ca),
        None => command.env("SSL_CERT_FILE", "/nonexistent/trust.pem"),
    };
    let output = command.output().expect("run skuld");
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        output.status.success(),
    )
}

fn fetch_program(port: u16) -> String {
    format!(
        r#"import "std/https"

func main() {{
    match https.get("https://localhost:{port}/") {{
        Ok(response): print("status ${{response.status}}")
        Err(problem): print(https.describe(problem))
    }}
}}
"#
    )
}

fn skip() -> bool {
    // TLS is deliberately outside M27, and the reason is not effort. Windows
    // has no Unix trust store, so `SSL_CTX_set_default_verify_paths` — which
    // `std/tls` calls to decide what to believe — finds nothing there, and a
    // verified connection on a clean machine needs the CryptoAPI root store
    // or a CA bundle shipped with the program. Choosing between those is a
    // `std/tls` design with its own milestone, and doing it badly here would
    // mean a program that appears to verify and does not.
    //
    // So these skip rather than fail: the library is unported, not broken,
    // and the difference should be visible in the output.
    if !cfg!(unix) {
        eprintln!("skipping: TLS has no trust store on this system yet (M28)");
        return true;
    }
    if !clang_available() {
        eprintln!("skipping: clang is not on PATH");
        return true;
    }
    if !tool_available("openssl") {
        eprintln!("skipping: openssl is not on PATH");
        return true;
    }
    false
}

#[test]
fn a_program_fetches_over_a_verified_connection() {
    if skip() {
        return;
    }
    let scratch = Scratch::new("good");
    let (ca, certificate) = certificates(&scratch, "server", None);
    let (_server, port) = serve(&scratch, &certificate, "server");
    let program = scratch.program(&fetch_program(port));
    let (out, ok) = run(&program, Some(&ca));
    assert!(ok, "{out}");
    assert_eq!(out, "status 200\n");
}

#[test]
fn a_certificate_nothing_trusts_is_refused() {
    if skip() {
        return;
    }
    let scratch = Scratch::new("untrusted");
    let (_ca, certificate) = certificates(&scratch, "server", None);
    let (_server, port) = serve(&scratch, &certificate, "server");
    let program = scratch.program(&fetch_program(port));
    // The trust store has nothing in it, so the chain cannot be built.
    let (out, ok) = run(&program, None);
    assert!(ok, "a refusal is a value, not a crash");
    assert!(out.contains("the certificate was rejected"), "{out}");
}

#[test]
fn a_certificate_for_another_name_is_refused() {
    if skip() {
        return;
    }
    let scratch = Scratch::new("wrongname");
    let (ca, certificate) = certificates(&scratch, "server", None);
    let (_server, port) = serve(&scratch, &certificate, "server");
    // The certificate is for `localhost`; this asks for a different name at
    // the same address, which is exactly what verification is for.
    let program = scratch.program(&format!(
        r#"import "std/tls"

func main() {{
    match tls.connect("127.0.0.1", {port}, "elsewhere.test") {{
        Ok(connection): {{
            connection.close()
            print("unexpectedly accepted")
        }}
        Err(problem): print(tls.describe(problem))
    }}
}}
"#
    ));
    let (out, ok) = run(&program, Some(&ca));
    assert!(ok, "{out}");
    assert!(out.contains("it is for a different host name"), "{out}");
}

#[test]
fn an_expired_certificate_is_refused() {
    if skip() {
        return;
    }
    let scratch = Scratch::new("expired");
    let (ca, certificate) = certificates(
        &scratch,
        "server",
        Some(("20200101000000Z", "20200102000000Z")),
    );
    let (_server, port) = serve(&scratch, &certificate, "server");
    let program = scratch.program(&fetch_program(port));
    let (out, ok) = run(&program, Some(&ca));
    assert!(ok, "{out}");
    assert!(out.contains("it has expired"), "{out}");
}

#[test]
fn a_plain_http_url_belongs_to_the_other_module() {
    if skip() {
        return;
    }
    let scratch = Scratch::new("scheme");
    let program = scratch.program(
        r#"import "std/https"

func main() {
    match https.get("http://localhost/") {
        Ok(response): print("unexpected")
        Err(problem): print(https.describe(problem))
    }
    match https.get("ftp://localhost/") {
        Ok(response): print("unexpected")
        Err(problem): print(https.describe(problem))
    }
}
"#,
    );
    let (out, ok) = run(&program, None);
    assert!(ok, "{out}");
    assert_eq!(
        out,
        "`http://localhost/` is plain http; use `std/http` for it\n\
         `ftp://localhost/` is not an https URL\n"
    );
}

#[test]
fn json_is_fetched_and_parsed_over_the_verified_connection() {
    if skip() {
        return;
    }
    // The milestone's marker: a document fetched over TLS and decoded, with
    // verification on the whole way.
    let scratch = Scratch::new("json");
    let (ca, certificate) = certificates(&scratch, "server", None);
    fs::write(
        scratch.path("data.json"),
        r#"{"name": "skuld", "tags": ["native", "verified"]}"#,
    )
    .expect("the document");

    // `-WWW` serves files from the working directory with an HTTP head on
    // them, so the server is started in the directory the document lives in.
    // (`-HTTP` sends the file with no headers at all, which is not HTTP.)
    let port = {
        let listener = TcpListener::bind("127.0.0.1:0").expect("an ephemeral port");
        listener.local_addr().expect("address").port()
    };
    let child = Command::new("openssl")
        .current_dir(&scratch.directory)
        .args([
            "s_server",
            "-accept",
            &format!("127.0.0.1:{port}"),
            "-cert",
            certificate.to_str().unwrap(),
            "-key",
            scratch.path("server.key").to_str().unwrap(),
            "-WWW",
            "-quiet",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start s_server");
    let _server = Server { child };
    for _ in 0..200 {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }

    let program = scratch.program(&format!(
        r#"import "std/https"
import "std/json"

func main() {{
    let response = https.get("https://localhost:{port}/data.json") else problem {{
        print(https.describe(problem))
        return
    }}
    let document = json.parse(response.body) else problem {{
        print(json.describe(problem))
        return
    }}
    if let name = json.lookup(document, "name") {{
        print(json.render(name))
    }}
    if let tags = json.lookup(document, "tags") {{
        print(json.render(tags))
    }}
}}
"#
    ));
    let (out, ok) = run(&program, Some(&ca));
    assert!(ok, "{out}");
    assert_eq!(out, "\"skuld\"\n[\"native\",\"verified\"]\n");
}
