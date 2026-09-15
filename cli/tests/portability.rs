//! The language fixtures on a second native target.
//!
//! The target is **i686-linux-gnu**: the same machine, the same libc, and
//! pointers half the width. It was chosen because it is the second target this
//! project can actually run rather than only build for — an ARM or a macOS
//! claim with no machine behind it would be a claim, not evidence — and
//! because a 32-bit pointer is what finds the places where the runtime or the
//! generated C assumed a 64-bit one.
//!
//! It needs `clang` and a 32-bit libc (`glibc-devel.i686`, `gcc-multilib` or
//! the distribution's equivalent); without them the test skips, the way the
//! rest of the native suite does without clang.
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

/// Fixtures that are deliberately not portable, and why.
///
/// With M21 providing `usize` and `isize`, `extern_c_ffi` uses `usize`/`isize`
/// matching C `size_t`/`ssize_t`, so every pass fixture runs identically on i686.
const TARGET_SPECIFIC: &[&str] = &[];

fn workspace(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .join(relative)
}

fn scratch() -> PathBuf {
    let directory = env::temp_dir().join(format!("skuld-m32-{}", std::process::id()));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).expect("scratch directory");
    directory
}

/// Whether clang can build and run a 32-bit program here at all.
fn thirty_two_bit_available(directory: &Path) -> bool {
    let source = directory.join("probe.c");
    fs::write(
        &source,
        "#include <stdio.h>\nint main(void){printf(\"%zu\\n\", sizeof(void*));return 0;}\n",
    )
    .expect("probe source");
    let binary = directory.join("probe");
    let built = Command::new("clang")
        .args(["-m32", "-O0"])
        .arg(&source)
        .arg("-o")
        .arg(&binary)
        .output();
    if !built.is_ok_and(|output| output.status.success()) {
        return false;
    }
    Command::new(&binary)
        .output()
        .is_ok_and(|output| String::from_utf8_lossy(&output.stdout).trim() == "4")
}

#[test]
fn every_pass_fixture_runs_the_same_on_a_32_bit_target() {
    let directory = scratch();
    if !thirty_two_bit_available(&directory) {
        eprintln!("skipping: no clang with a 32-bit libc on this machine");
        let _ = fs::remove_dir_all(&directory);
        return;
    }

    let pass = workspace("tests/pass");
    let mut sources: Vec<PathBuf> = fs::read_dir(&pass)
        .expect("read tests/pass")
        .map(|entry| entry.expect("entry").path())
        .filter(|path| path.extension().is_some_and(|e| e == "skuld"))
        .collect();
    sources.sort();
    assert!(!sources.is_empty(), "no fixtures to run");

    let mut ran = 0;
    for source in &sources {
        let name = source
            .file_stem()
            .expect("a stem")
            .to_string_lossy()
            .into_owned();
        if TARGET_SPECIFIC.contains(&name.as_str()) {
            continue;
        }
        let expected = fs::read_to_string(pass.join(format!("{name}.out")))
            .unwrap_or_else(|error| panic!("{name}.out: {error}"));

        // The compiler is the same one; only the C compiler's target changes.
        let generated = Command::new(env!("CARGO_BIN_EXE_skuld"))
            .arg("emit-c")
            .arg(source)
            .output()
            .expect("emit C");
        assert!(
            generated.status.success(),
            "{name}: emit-c failed: {}",
            String::from_utf8_lossy(&generated.stderr)
        );
        let c_file = directory.join(format!("{name}.c"));
        fs::write(&c_file, &generated.stdout).expect("write generated C");

        let binary = directory.join(&name);
        let built = Command::new("clang")
            .args(["-m32", "-std=c11", "-O2", "-fno-fast-math"])
            .arg(&c_file)
            .arg("-o")
            .arg(&binary)
            .output()
            .expect("run clang");
        assert!(
            built.status.success(),
            "{name}: 32-bit build failed: {}",
            String::from_utf8_lossy(&built.stderr)
        );

        let run = Command::new(&binary).output().expect("run the program");
        assert!(run.status.success(), "{name}: exited with {}", run.status);
        assert_eq!(
            String::from_utf8_lossy(&run.stdout),
            expected,
            "{name}: different output on i686"
        );
        ran += 1;
    }

    assert!(ran > 40, "only {ran} fixtures ran; the list looks wrong");
    let _ = fs::remove_dir_all(&directory);
}
