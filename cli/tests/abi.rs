//! An `extern struct` crossing the boundary by value, against a C object file
//! this test compiles.
//!
//! It lives here rather than in `tests/pass` because the golden suite links
//! nothing but libc: a struct passed by value can only be observed by another
//! object file that reads it, so the test has to build one. What it proves is
//! the half of M24 no fixture can — that the layout Skuld emits is the layout
//! the platform's C compiler expects, in both directions.
use std::{env, fs, path::PathBuf, process::Command};

struct Scratch {
    directory: PathBuf,
}

impl Scratch {
    fn new(name: &str) -> Self {
        let directory = env::temp_dir().join(format!("skuld-abi-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).expect("scratch directory");
        Self { directory }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

/// Whether the clang on PATH builds for the MSVC ABI, which decides how a
/// static library has to be named for `-l` to find it.
fn targets_msvc() -> bool {
    Command::new("clang")
        .arg("-print-target-triple")
        .output()
        .is_ok_and(|output| String::from_utf8_lossy(&output.stdout).contains("msvc"))
}

fn clang_available() -> bool {
    Command::new("clang")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
}

const HELPER: &str = r#"
#include <stdint.h>

typedef struct { int32_t x; int32_t y; } Point;
typedef struct __attribute__((packed)) { uint8_t tag; uint32_t value; } Tagged;

int32_t point_sum(Point p) { return p.x + p.y; }
Point point_scaled(Point p, int32_t by) { Point out = { p.x * by, p.y * by }; return out; }
uint32_t tagged_value(Tagged t) { return t.value + (uint32_t)t.tag; }
int32_t point_x_through_pointer(const Point *p) { return p->x; }
"#;

const PROGRAM: &str = r#"
extern struct Point {
    x: i32,
    y: i32,
}

extern struct Tagged packed {
    tag: u8,
    value: u32,
}

unsafe extern "C" {
    func point_sum(p: Point) -> i32
    func point_scaled(p: Point, by: i32) -> Point
    func tagged_value(t: Tagged) -> u32
    func point_x_through_pointer(p: *void) -> i32
}

func main() {
    let point = Point { x: 20, y: 22 }
    print(point_sum(point))

    let scaled = point_scaled(point, 3)
    print(scaled.x)
    print(scaled.y)

    // A returned struct is an ordinary value: it goes straight back in.
    print(point_sum(point_scaled(scaled, 2)))

    // Packed, so C reads a field Skuld wrote at an offset neither of them
    // padded.
    print(int(tagged_value(Tagged { tag: u8(6), value: u32(36) })))

    // And the same type by address rather than by value.
    print(point_x_through_pointer(ptr(point)))
}
"#;

#[test]
fn an_extern_struct_crosses_the_boundary_by_value_in_both_directions() {
    if !clang_available() {
        eprintln!("skipping: clang is not on PATH");
        return;
    }
    let scratch = Scratch::new("byvalue");
    let helper_source = scratch.directory.join("helper.c");
    let helper_object = scratch.directory.join("helper.o");
    fs::write(&helper_source, HELPER).expect("helper source");
    let compiled = Command::new("clang")
        // `-fPIC` describes how an ELF object is relocated, and the Windows
        // target refuses it rather than ignoring it: a PE image is relocated
        // whether or not anyone asks.
        .args(if cfg!(unix) {
            &["-c", "-fPIC", "-std=c11"][..]
        } else {
            &["-c", "-std=c11"][..]
        })
        .arg(&helper_source)
        .arg("-o")
        .arg(&helper_object)
        .output()
        .expect("run clang");
    assert!(
        compiled.status.success(),
        "clang failed: {}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    // A static library, because `-l` is the only way a program names something
    // to link and the CLI forwards nothing else.
    // `-l<name>` is spelled out by the target, not by the host: a GNU driver
    // looks for `libskuldabi.a` and an MSVC one for `skuldabi.lib`, and
    // clang on Windows is usually the second. Asking which it is beats
    // guessing from the operating system, since both exist there.
    let archive = scratch.directory.join(if targets_msvc() {
        "skuldabi.lib"
    } else {
        "libskuldabi.a"
    });
    // `llvm-ar` ships with the clang this test already needs, and is the one
    // archiver present on both systems; GNU `ar` is not on a stock Windows
    // machine. Both accept the same arguments here.
    let archiver = if Command::new("llvm-ar").arg("--version").output().is_ok() {
        "llvm-ar"
    } else {
        "ar"
    };
    let archived = Command::new(archiver)
        .arg("rcs")
        .arg(&archive)
        .arg(&helper_object)
        .output()
        .expect("run ar");
    assert!(
        archived.status.success(),
        "ar failed: {}",
        String::from_utf8_lossy(&archived.stderr)
    );

    let program = scratch.directory.join("main.skuld");
    fs::write(&program, PROGRAM).expect("program source");
    let output = Command::new(env!("CARGO_BIN_EXE_skuld"))
        .arg("run")
        .arg(&program)
        .arg(format!("-L{}", scratch.directory.display()))
        .arg("-lskuldabi")
        .output()
        .expect("run the program");
    assert!(
        output.status.success(),
        "the program failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "42\n60\n66\n252\n42\n20\n"
    );
}
