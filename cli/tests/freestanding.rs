//! M26's closing marker: Skuld with no runtime and no libc under it.
//!
//! Two programs, each the shape the milestone names. The first is a static
//! Linux executable that makes `write` and `exit` syscalls and links nothing
//! at all — the assembly stub that starts it is written here, because the
//! `syscall` instruction is the one thing the language deliberately cannot
//! spell. The second is a bootable multiboot kernel that writes to the VGA
//! text buffer, reads it back, and asks the machine to stop.
//!
//! The kernel is built here always and booted only where `qemu-system-i386`
//! exists, the way the rest of the suite treats a tool it needs but cannot
//! assume.
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

struct Scratch {
    directory: PathBuf,
}

impl Scratch {
    fn new(name: &str) -> Self {
        let directory =
            env::temp_dir().join(format!("skuld-freestanding-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).expect("scratch directory");
        Self { directory }
    }
    fn write(&self, name: &str, contents: &str) -> PathBuf {
        let path = self.directory.join(name);
        fs::write(&path, contents).expect("scratch file");
        path
    }
    fn path(&self, name: &str) -> PathBuf {
        self.directory.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

fn available(tool: &str, flag: &str) -> bool {
    Command::new(tool)
        .arg(flag)
        .output()
        .is_ok_and(|output| output.status.success())
}

fn run(program: &str, arguments: &[&str], what: &str) {
    let output = Command::new(program)
        .args(arguments)
        .output()
        .unwrap_or_else(|error| panic!("cannot launch {program}: {error}"));
    assert!(
        output.status.success(),
        "{what} failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn skuld(arguments: &[&str], what: &str) {
    let output = Command::new(env!("CARGO_BIN_EXE_skuld"))
        .args(arguments)
        .output()
        .expect("run skuld");
    assert!(
        output.status.success(),
        "{what} failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A program with no `main`: something else starts it and calls what it
/// exports.
const PROGRAM: &str = r#"
unsafe extern "C" {
    func sys_write(fd: i64, buffer: *void, length: usize) -> i64
    func sys_exit(code: i64) -> void
}

static written: int = 0

static message: [13]u8 = [0; 13]

func put(index: int, byte: u8) {
    message[index] = byte
    written = written + 1
}

pub func skuld_entry() {
    let letters: [13]u8 = [102, 114, 101, 101, 115, 116, 97, 110, 100, 105, 110, 103, 10]
    var index = 0
    while index < 13 {
        put(index, letters[index])
        index = index + 1
    }
    let sent = sys_write(1, ptr(message), usize(written))
    sys_exit(0)
}
"#;

/// `_start`, and the two syscalls it wraps. The `syscall` instruction is out
/// of the language's scope on purpose, so it is here.
const START: &str = r#"
    .text
    .globl _start
_start:
    call    skuld_entry
    mov     $60, %eax
    xor     %edi, %edi
    syscall

    .globl sys_write
sys_write:
    mov     $1, %eax
    syscall
    ret

    .globl sys_exit
sys_exit:
    mov     $60, %eax
    syscall
    ret
"#;

#[test]
fn a_freestanding_program_runs_with_no_libc_at_all() {
    if !available("clang", "--version") {
        eprintln!("skipping: clang is not on PATH");
        return;
    }
    if !available("ld", "--version") {
        eprintln!("skipping: no `ld` to link with");
        return;
    }
    let scratch = Scratch::new("program");
    let source = scratch.write("program.skuld", PROGRAM);
    let object = scratch.path("program.o");
    skuld(
        &[
            "build",
            "--freestanding",
            source.to_str().expect("path"),
            "-o",
            object.to_str().expect("path"),
        ],
        "the freestanding build",
    );

    let start = scratch.write("start.S", START);
    let start_object = scratch.path("start.o");
    run(
        "clang",
        &[
            "-c",
            start.to_str().expect("path"),
            "-o",
            start_object.to_str().expect("path"),
        ],
        "assembling the entry stub",
    );
    let program = scratch.path("program");
    run(
        "ld",
        &[
            "-o",
            program.to_str().expect("path"),
            start_object.to_str().expect("path"),
            object.to_str().expect("path"),
        ],
        "linking",
    );

    let output = Command::new(&program).output().expect("run the program");
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "freestanding\n");

    // Nothing was linked, which is the claim worth checking rather than
    // assuming: a dynamic executable would name an interpreter here.
    let inspected = Command::new("file")
        .arg(&program)
        .output()
        .expect("inspect the program");
    let description = String::from_utf8_lossy(&inspected.stdout);
    assert!(
        description.contains("statically linked"),
        "expected a static executable, got {description}"
    );
}

/// A kernel: the VGA text buffer is memory-mapped at 0xB8000, two bytes per
/// cell. Writing there is what printing means before there is anything to
/// print to.
const KERNEL: &str = r#"
unsafe extern "C" {
    func serial_write(byte: u8) -> void
    func machine_exit(code: u32) -> void
}

const VGA_BUFFER: usize = 753664
const VGA_COLOUR: u16 = 3872
static column: int = 0

func put(character: u8) {
    unsafe {
        let cell: *u16 = ptr_from(VGA_BUFFER + usize(column * 2))
        volatile_store(cell, u16(character) | VGA_COLOUR)
    }
    serial_write(character)
    column = column + 1
}

func reads_back(character: u8, at: int) -> bool {
    unsafe {
        let cell: *u16 = ptr_from(VGA_BUFFER + usize(at * 2))
        return volatile_load(cell) == (u16(character) | VGA_COLOUR)
    }
}

pub func kernel_main() {
    let text: [6]u8 = [83, 107, 117, 108, 100, 33]
    var index = 0
    while index < 6 {
        put(text[index])
        index = index + 1
    }
    var ok = true
    var check = 0
    while check < 6 {
        if !reads_back(text[check], check) {
            ok = false
        }
        check = check + 1
    }
    if ok {
        machine_exit(0)
    }
    machine_exit(1)
}
"#;

const BOOT: &str = r#"
    .set MAGIC,    0x1BADB002
    .set FLAGS,    0
    .set CHECKSUM, -(MAGIC + FLAGS)

    .section .multiboot, "a"
    .align 4
    .long MAGIC
    .long FLAGS
    .long CHECKSUM

    .section .bss
    .align 16
stack_bottom:
    .skip 16384
stack_top:

    .section .text
    .globl _start
_start:
    mov $stack_top, %esp
    call kernel_main
hang:
    cli
    hlt
    jmp hang

    /* COM1, so that what the kernel writes leaves the machine. */
    .globl serial_write
serial_write:
    mov 4(%esp), %al
    mov $0x3F8, %dx
    outb %al, %dx
    ret

    /* QEMU's isa-debug-exit device: the status is (code << 1) | 1. */
    .globl machine_exit
machine_exit:
    mov 4(%esp), %eax
    mov $0xF4, %dx
    outl %eax, %dx
    ret
"#;

const LINK: &str = r#"ENTRY(_start)
SECTIONS {
    . = 1M;
    .text BLOCK(4K) : ALIGN(4K) { *(.multiboot) *(.text) }
    .rodata BLOCK(4K) : ALIGN(4K) { *(.rodata) }
    .data BLOCK(4K) : ALIGN(4K) { *(.data) }
    .bss BLOCK(4K) : ALIGN(4K) { *(COMMON) *(.bss) }
}
"#;

#[test]
fn a_bootable_kernel_is_built_and_booted() {
    if !available("clang", "--version") {
        eprintln!("skipping: clang is not on PATH");
        return;
    }
    if !available("ld", "--version") {
        eprintln!("skipping: no `ld` to link with");
        return;
    }
    let scratch = Scratch::new("kernel");
    let source = scratch.write("kernel.skuld", KERNEL);
    let generated = scratch.path("kernel.c");
    // A kernel is 32-bit here, and the target is the C compiler's business:
    // `emit-c` hands over the C, exactly as the 32-bit portability suite does.
    let output = Command::new(env!("CARGO_BIN_EXE_skuld"))
        .args(["emit-c", "--freestanding"])
        .arg(&source)
        .output()
        .expect("run skuld");
    assert!(
        output.status.success(),
        "emitting the kernel failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    fs::write(&generated, &output.stdout).expect("write the generated C");

    let kernel_object = scratch.path("kernel.o");
    let compiled = Command::new("clang")
        .args([
            "-m32",
            "-std=c11",
            "-O2",
            "-ffreestanding",
            "-fno-builtin",
            "-fno-stack-protector",
            "-fno-asynchronous-unwind-tables",
            "-c",
        ])
        .arg(&generated)
        .arg("-o")
        .arg(&kernel_object)
        .output()
        .expect("run clang");
    if !compiled.status.success() {
        let complaint = String::from_utf8_lossy(&compiled.stderr).to_string();
        // A machine with no 32-bit support is the same case the portability
        // suite skips on.
        if complaint.contains("unsupported") || complaint.contains("not found") {
            eprintln!("skipping: no 32-bit target on this machine");
            return;
        }
        panic!("compiling the kernel failed: {complaint}");
    }

    let boot = scratch.write("boot.S", BOOT);
    let boot_object = scratch.path("boot.o");
    run(
        "clang",
        &[
            "-m32",
            "-c",
            boot.to_str().expect("path"),
            "-o",
            boot_object.to_str().expect("path"),
        ],
        "assembling the boot stub",
    );
    let script = scratch.write("link.ld", LINK);
    let image = scratch.path("kernel.elf");
    run(
        "ld",
        &[
            "-m",
            "elf_i386",
            "-T",
            script.to_str().expect("path"),
            "-o",
            image.to_str().expect("path"),
            boot_object.to_str().expect("path"),
            kernel_object.to_str().expect("path"),
        ],
        "linking the kernel",
    );
    assert_multiboot(&image);

    if !available("qemu-system-i386", "--version") {
        eprintln!("skipping the boot itself: qemu-system-i386 is not on PATH");
        return;
    }
    let booted = Command::new("qemu-system-i386")
        .args([
            "-kernel",
            image.to_str().expect("path"),
            "-display",
            "none",
            "-serial",
            "stdio",
            "-no-reboot",
            "-device",
            "isa-debug-exit,iobase=0xf4,iosize=0x04",
        ])
        .output()
        .expect("run qemu");
    // The kernel wrote to the VGA buffer, read it back and stopped the
    // machine: `isa-debug-exit` turns a code of 0 into a status of 1.
    assert_eq!(
        booted.status.code(),
        Some(1),
        "the kernel did not report success: {}{}",
        String::from_utf8_lossy(&booted.stdout),
        String::from_utf8_lossy(&booted.stderr)
    );
    assert!(
        String::from_utf8_lossy(&booted.stdout).contains("Skuld!"),
        "the serial port did not carry what the kernel wrote: {}",
        String::from_utf8_lossy(&booted.stdout)
    );
}

/// A multiboot header is what makes an ELF a thing a bootloader will start:
/// the magic, and three words that sum to zero.
fn assert_multiboot(image: &Path) {
    let bytes = fs::read(image).expect("read the kernel image");
    let magic = 0x1BADB002_u32.to_le_bytes();
    let at = bytes
        .windows(4)
        .position(|window| window == magic)
        .expect("the image carries a multiboot header");
    let word = |offset: usize| {
        u32::from_le_bytes(
            bytes[at + offset..at + offset + 4]
                .try_into()
                .expect("four bytes"),
        )
    };
    let (magic, flags, checksum) = (word(0), word(4), word(8));
    assert_eq!(
        magic.wrapping_add(flags).wrapping_add(checksum),
        0,
        "the multiboot header does not check out"
    );
}
