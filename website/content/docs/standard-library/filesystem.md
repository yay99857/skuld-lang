---
title: Filesystem
description: Read and write complete files or stream through an explicitly closed handle.
status: Implemented
---
Import `"std/fs"`. File operations return `Result` values with `FsError`; use `fs.describe(error)` to explain a failure.

## Whole-file operations

```skuld
func read_file(path: string) -> Result<[]u8, FsError>
func read_text(path: string) -> Result<string, FsError>
func write_file(path: string, bytes: []u8) -> Result<int, FsError>
func write_text(path: string, text: string) -> Result<int, FsError>
```

The write result reports the number of bytes written. Text reads validate UTF-8. Whole-file operations are convenient for documents that fit in memory.

```skuld main.skuld
import "std/fs"

func main() {
    match fs.read_text("notes.txt") {
        Ok(text): print(text)
        Err(error): print(fs.describe(error))
    }
}
```

## Streaming files

`fs.open(path) -> Result<File, FsError>` opens for reading. `fs.create(path)` opens for writing and replaces existing content. A `File` offers `read(limit)`, `write(bytes)` and `close()`.

The caller must close a streaming file. An empty read means end-of-file.

```skuld main.skuld
import "std/fs"

func main() {
    let file = fs.open("notes.txt") else error {
        print(fs.describe(error))
        return
    }
    defer file.close()
    match file.read(1024) {
        Ok(bytes): print(bytes.len())
        Err(error): print(fs.describe(error))
    }
}
```

## Availability

Implemented in the current source checkout on the supported hosted platforms. The class's memory is reference counted, but closing its OS handle remains the caller's responsibility.
