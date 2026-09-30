---
title: Hello World
description: Write, check and run your first native Skuld program.
status: Implemented
---
After [installing the toolchain](/docs/getting-started/installation/), create a text file named `main.skuld`.

## Write a program

```skuld main.skuld numbers
func main() {
    print("Hello, world!")
}
```

`func` declares a function. A hosted program starts in `main`, which takes no parameters and returns no value. `print` writes a value followed by a newline.

## Check and run

```bash
skuld check main.skuld
skuld run main.skuld
```

A successful check is silent. Running the program produces:

```text Output
Hello, world!
```

`run` compiles and executes in a private temporary directory, then removes the temporary files. It does not leave a binary in your project.

## Keep an executable

```bash
skuld build main.skuld
```

The output is `main` on Linux or `main.exe` on Windows. Use `-o` to choose a different output path. The compiler refuses to overwrite its own source.

## Make it your own

```skuld main.skuld
func main() {
    let language = "Skuld"
    print("Hello from ${language}!")
}
```

`${expression}` interpolates a value into a string. Learn about [bindings](/docs/language/variables/) and [functions](/docs/language/functions/) next.
