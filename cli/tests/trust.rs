//! The trust anchors the platform layer reads out of the machine.
//!
//! This is checked here rather than in `tests/pass` because the right answer
//! differs by system — Windows has a certificate store to enumerate and the
//! others keep a bundle where OpenSSL already looks — and a golden fixture
//! has one expected output for every platform.
//!
//! It touches no network and installs nothing. The program under test asks
//! the platform layer for the anchors and describes what it got; what is
//! asserted is the shape of that answer, not any particular certificate,
//! since which authorities a machine trusts is the machine's business.
use std::{env, fs, path::PathBuf, process::Command};

fn clang_available() -> bool {
    Command::new("clang")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
}

/// A program that asks the layer twice, as its two-call protocol wants: once
/// to learn the length, then again with somewhere to put it.
const PROGRAM: &str = r#"import "std/strings"

unsafe extern "C" {
    func sk_trust_anchors_pem(out: *u8, capacity: u64) -> i64
}

func no_bytes() -> *u8 {
    unsafe {
        return ptr_from(usize(0))
    }
}

func main() {
    let needed = sk_trust_anchors_pem(no_bytes(), u64(0))
    if needed < 0 {
        print("refused")
        return
    }
    if needed == 0 {
        print("none")
        return
    }
    var bundle: []u8 = []
    var filled = 0
    while filled < int(needed) {
        bundle.push(0)
        filled += 1
    }
    let got = sk_trust_anchors_pem(ptr(bundle), u64(needed))
    if got != needed {
        print("short")
        return
    }
    let text = bytes_to_string(bundle) else problem {
        print("not text")
        return
    }
    if !strings.starts_with(text, "-----BEGIN CERTIFICATE-----") {
        print("not pem")
        return
    }
    var anchors = 0
    var at = 0
    loop {
        let rest = text[at..text.len()]
        let found = strings.index_of(rest, "-----BEGIN CERTIFICATE-----") else {
            break
        }
        anchors += 1
        at = at + found + 27
    }
    print("anchors ${anchors}")
}
"#;

#[test]
fn the_platform_layer_answers_for_this_machines_trust_anchors() {
    if !clang_available() {
        eprintln!("skipping: clang is not on PATH");
        return;
    }
    let directory = env::temp_dir().join(format!("skuld-trust-{}", std::process::id()));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).expect("scratch directory");
    let source = directory.join("anchors.skuld");
    fs::write(&source, PROGRAM).expect("program source");

    let output = Command::new(env!("CARGO_BIN_EXE_skuld"))
        .arg("run")
        .arg(&source)
        .output()
        .expect("run skuld");
    let out = String::from_utf8_lossy(&output.stdout).into_owned();
    let err = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(output.status.success(), "{out}{err}[{}]", output.status);

    if cfg!(windows) {
        // The store is the machine's, so the count is not fixed — but a
        // Windows install that trusts nothing at all cannot reach any public
        // site, and an answer of `none` here would mean the enumeration
        // silently found nothing, which is the failure this exists to catch.
        let anchors = out
            .trim()
            .strip_prefix("anchors ")
            .unwrap_or_else(|| panic!("expected a count of anchors, got: {out}{err}"))
            .parse::<u32>()
            .expect("a number of anchors");
        assert!(
            anchors > 0,
            "the Windows certificate store enumerated to nothing"
        );
    } else {
        // Nothing to add: the default verify paths already are the store.
        assert_eq!(out, "none\n", "{err}");
    }
    let _ = fs::remove_dir_all(&directory);
}

/// The layer must be safe to ask with no buffer, since that is how a caller
/// learns the length. Asking twice must also answer the same thing, or the
/// two-call protocol has a race in it that a caller cannot work around.
#[test]
fn asking_for_the_length_twice_answers_the_same() {
    if !clang_available() {
        eprintln!("skipping: clang is not on PATH");
        return;
    }
    let directory = env::temp_dir().join(format!("skuld-trust-twice-{}", std::process::id()));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).expect("scratch directory");
    let source: PathBuf = directory.join("twice.skuld");
    fs::write(
        &source,
        r#"unsafe extern "C" {
    func sk_trust_anchors_pem(out: *u8, capacity: u64) -> i64
}

func no_bytes() -> *u8 {
    unsafe {
        return ptr_from(usize(0))
    }
}

func main() {
    let first = sk_trust_anchors_pem(no_bytes(), u64(0))
    let second = sk_trust_anchors_pem(no_bytes(), u64(0))
    if first == second {
        print("stable")
    } else {
        print("${first} then ${second}")
    }
}
"#,
    )
    .expect("program source");

    let output = Command::new(env!("CARGO_BIN_EXE_skuld"))
        .arg("run")
        .arg(&source)
        .output()
        .expect("run skuld");
    let out = String::from_utf8_lossy(&output.stdout).into_owned();
    let err = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(output.status.success(), "{out}{err}[{}]", output.status);
    assert_eq!(out, "stable\n", "{err}");
    let _ = fs::remove_dir_all(&directory);
}
