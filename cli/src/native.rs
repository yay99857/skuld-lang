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
fn emit_and_compile(c_source: &str, directory: &Path, executable: &Path) -> Result<(), String> {
    let source = directory.join("generated.c");
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&source)
        .map_err(|error| format!("cannot create generated C: {error}"))?;
    file.write_all(c_source.as_bytes())
        .map_err(|error| format!("cannot write generated C: {error}"))?;
    drop(file);
    compile(&source, executable)
}

/// Compile to a persistent executable. Only the C stays in the temporary
/// directory; the executable is the one artifact the user keeps.
pub fn build(c_source: &str, executable: &Path) -> Result<(), String> {
    let temp = TempDir::create()
        .map_err(|error| format!("cannot create temporary build directory: {error}"))?;
    emit_and_compile(c_source, &temp.path, executable)
}

pub fn run(c_source: &str) -> Result<u8, String> {
    let temp = TempDir::create()
        .map_err(|error| format!("cannot create temporary build directory: {error}"))?;
    let executable = temp.path.join(if cfg!(windows) {
        "program.exe"
    } else {
        "program"
    });
    emit_and_compile(c_source, &temp.path, &executable)?;
    let status = Command::new(&executable)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|error| format!("cannot execute compiled program: {error}"))?;
    Ok(exit_code(status))
}
fn compile(source: &Path, executable: &Path) -> Result<(), String> {
    let output = Command::new("clang").arg("-std=c11").arg("-O2").arg("-fno-fast-math").arg(source).arg("-o").arg(executable).stdin(Stdio::null()).output().map_err(|error| {
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
        assert_eq!(run("int main(void) { return 37; }"), Ok(37));
    }
}
