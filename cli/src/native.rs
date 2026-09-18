//! External-tool orchestration; the compiler library never launches processes.
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    process::{Command, ExitStatus, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
struct TempDir {
    path: PathBuf,
}
impl TempDir {
    fn create() -> io::Result<Self> {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        for _ in 0..100 {
            let serial = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("skuld-{}-{stamp}-{serial}", std::process::id()));
            // Only the `cfg(unix)` arm below mutates the builder, so anywhere
            // else the `mut` is dead and `-D warnings` rejects it.
            #[cfg_attr(not(unix), allow(unused_mut))]
            let mut builder = fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            match builder.create(&path) {
                Ok(()) => return Ok(Self { path }),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not create a unique temporary directory",
        ))
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// Write the generated C into `directory` and compile it to `executable`.
fn emit_and_compile(
    c_source: &str,
    directory: &Path,
    executable: &Path,
    link_flags: &[String],
) -> Result<(), String> {
    let source = directory.join("generated.c");
    write_new(&source, c_source, "generated C")?;
    // The platform layer is its own translation unit, so that the headers it
    // needs for the system's own flags and widths do not reach the program
    // and collide with what it declares for itself.
    let platform = directory.join("skuld_platform.c");
    write_new(
        &platform,
        skuld_compiler::codegen_c::platform_source(),
        "platform layer",
    )?;
    compile(&[&source, &platform], executable, link_flags)
}

fn write_new(path: &Path, contents: &str, what: &str) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| format!("cannot create {what}: {error}"))?;
    file.write_all(contents.as_bytes())
        .map_err(|error| format!("cannot write {what}: {error}"))?;
    Ok(())
}

/// Compile to a persistent executable. Only the C stays in the temporary
/// directory; the executable is the one artifact the user keeps.
pub fn build(c_source: &str, executable: &Path, link_flags: &[String]) -> Result<(), String> {
    let temp = TempDir::create()
        .map_err(|error| format!("cannot create temporary build directory: {error}"))?;
    emit_and_compile(c_source, &temp.path, executable, link_flags)
}

/// Compile to an object file, for a program that has no runtime, no libc and
/// no entry point of its own.
///
/// The flags are fixed and few on purpose: this stops at the object file, and
/// what happens next — the linker script, the target, the assembly stub that
/// starts it — belongs to whoever is building the thing this is a part of.
pub fn build_object(c_source: &str, object: &Path) -> Result<(), String> {
    let temp = TempDir::create()
        .map_err(|error| format!("cannot create temporary build directory: {error}"))?;
    let source = temp.path.join("generated.c");
    std::fs::write(&source, c_source)
        .map_err(|error| format!("cannot write generated C: {error}"))?;
    let output = Command::new("clang")
        .args([
            "-std=c11",
            "-O2",
            "-fno-fast-math",
            "-c",
            "-ffreestanding",
            "-fno-builtin",
            "-fno-stack-protector",
            "-fno-asynchronous-unwind-tables",
        ])
        .arg(&source)
        .arg("-o")
        .arg(object)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                "clang was not found; install clang and make it available on PATH to build (checking does not need clang)".into()
            } else {
                format!("cannot launch clang: {error}")
            }
        })?;
    if !output.status.success() {
        return Err(format!(
            "clang failed ({}):\n{}{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(())
}

/// `arguments` are the program's own, from `--args`; the compiler passes them
/// through without reading them.
pub fn run(
    c_source: &str,
    link_flags: &[String],
    arguments: &[std::ffi::OsString],
) -> Result<u8, String> {
    let temp = TempDir::create()
        .map_err(|error| format!("cannot create temporary build directory: {error}"))?;
    let executable = temp.path.join(if cfg!(windows) {
        "program.exe"
    } else {
        "program"
    });
    emit_and_compile(c_source, &temp.path, &executable, link_flags)?;
    let status = Command::new(&executable)
        .args(arguments)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|error| format!("cannot execute compiled program: {error}"))?;
    Ok(exit_code(status))
}
/// Compile and run, capturing what the program printed instead of letting it
/// through. `skuld test` needs the output as data: the markers in it are how
/// the runner knows which tests ran.
pub fn capture(c_source: &str, link_flags: &[String]) -> Result<(u8, String, String), String> {
    let temp = TempDir::create()
        .map_err(|error| format!("cannot create temporary build directory: {error}"))?;
    let executable = temp.path.join(if cfg!(windows) {
        "program.exe"
    } else {
        "program"
    });
    emit_and_compile(c_source, &temp.path, &executable, link_flags)?;
    let output = Command::new(&executable)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("cannot execute compiled program: {error}"))?;
    Ok((
        exit_code(output.status),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    ))
}

/// What the platform layer itself needs linked, as opposed to what a program
/// asks for.
///
/// On Windows the sockets live in `ws2_32` rather than in the C library, so
/// the layer does not link without it. It is added unconditionally rather
/// than when a program imports `std/net`, because nothing today connects an
/// import to a link flag and inventing that is a compiler concept of its own;
/// the cost of always asking is an import library for a DLL every Windows
/// process has mapped already. Elsewhere the sockets are in libc, which clang
/// links without being asked.
const PLATFORM_LIBRARIES: &[&str] = if cfg!(windows) { &["-lws2_32"] } else { &[] };

/// `link_flags` carries `-l`/`-L` arguments for libraries an `extern "C"`
/// declaration needs; libc is linked by clang without asking.
fn compile(sources: &[&Path], executable: &Path, link_flags: &[String]) -> Result<(), String> {
    let output = Command::new("clang").arg("-std=c11").arg("-O2").arg("-fno-fast-math").args(sources).arg("-o").arg(executable).args(link_flags).args(PLATFORM_LIBRARIES).stdin(Stdio::null()).output().map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound { "clang was not found; install clang and make it available on PATH to use `skuld run` (checking does not need clang)".into() }
        else { format!("cannot launch clang: {error}") }
    })?;
    if !output.status.success() {
        return Err(format!(
            "clang failed ({}):\n{}{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(())
}
fn exit_code(status: ExitStatus) -> u8 {
    if let Some(code) = status.code() {
        return u8::try_from(code).unwrap_or(1);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return u8::try_from(128 + signal).unwrap_or(1);
        }
    }
    1
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn temporary_build_directories_are_unique_and_cleaned() {
        let first = TempDir::create().expect("first temp");
        let second = TempDir::create().expect("second temp");
        assert_ne!(first.path, second.path);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&first.path)
                    .expect("metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
        }
        let path = first.path.clone();
        fs::write(path.join("artifact"), "temporary").expect("artifact");
        drop(first);
        assert!(!path.exists());
        assert!(second.path.exists());
    }
}

#[cfg(all(test, unix))]
mod unix_tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;
    #[test]
    fn child_status_is_propagated() {
        assert_eq!(exit_code(ExitStatus::from_raw(37 << 8)), 37);
        assert_eq!(exit_code(ExitStatus::from_raw(15)), 143);
    }
    #[test]
    fn driver_forwards_real_child_exit_code() {
        assert_eq!(run("int main(void) { return 37; }", &[], &[]), Ok(37));
    }
    #[test]
    fn a_program_receives_the_arguments_it_was_given() {
        // The count includes the program itself, so two arguments make three.
        let program = "int main(int argc, char **argv) { (void)argv; return argc; }";
        let arguments = [
            std::ffi::OsString::from("one"),
            std::ffi::OsString::from("two"),
        ];
        assert_eq!(run(program, &[], &arguments), Ok(3));
    }
}
