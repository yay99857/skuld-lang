---
title: Installation
description: Build the Skuld compiler from source and put the toolchain on your PATH.
status: Implemented
---
Skuld currently installs from its source repository. You need Git, Rust stable with Cargo, and clang for native builds. The `check` command does not need clang.

## Choose your platform

<div class="tabs">
<div class="tablist" role="tablist" aria-label="Installation platform">
<button role="tab" id="linux-tab" aria-controls="linux-panel" aria-selected="true">Linux</button>
<button role="tab" id="macos-tab" aria-controls="macos-panel" aria-selected="false" tabindex="-1">macOS</button>
<button role="tab" id="windows-tab" aria-controls="windows-panel" aria-selected="false" tabindex="-1">Windows</button>
</div>
<section role="tabpanel" id="linux-panel" aria-labelledby="linux-tab">
<p>Linux x86_64 is supported. Install Rust stable and clang using your distribution's development tools. The i686 Linux target is also covered by portability tests.</p>
</section>
<section role="tabpanel" id="macos-panel" aria-labelledby="macos-tab" hidden>
<p><strong>Unverified platform.</strong> macOS is not currently a supported target. The source-build commands below are provided for investigation, not as a verified installation path. Platform-dependent standard library modules may not build.</p>
</section>
<section role="tabpanel" id="windows-panel" aria-labelledby="windows-tab" hidden>
<p>Windows x86_64 is supported. Install Rust stable for the MSVC toolchain, LLVM clang, and Visual Studio C++ Build Tools with the Windows SDK. Ensure clang and the linker are available in your development shell. HTTPS needs OpenSSL installed and linked as <code>-llibssl -llibcrypto</code>; certificates are verified against the Windows certificate store.</p>
</section>
</div>

## Install from source

Run these commands in a terminal. They apply to the supported Linux and Windows toolchains.

```bash
git clone https://github.com/yay99857/skuld-lang.git
cd skuld-lang
cargo install --path cli
```

> [!Note]
> There is no published installer script documented here. Cargo builds the compiler locally; it does not install clang for you.

## Verify the installation

```bash
skuld --version
skuld --help
skuld run examples/hello.skuld
```

The last command prints `Hello from Skuld!` from the repository example. Continue with [your own first program](/docs/getting-started/hello-world/).

## Update or uninstall

From a clean checkout, fetch the latest source and rebuild the installed binary:

```bash
git pull --ff-only
cargo install --path cli --force
```

To remove the Cargo-installed CLI:

```bash
cargo uninstall skuld-cli
```

## Troubleshooting

| Symptom | What to check |
| --- | --- |
| `skuld` is not found | Cargo's binary directory must be on PATH. Restart your terminal after installing Rust. |
| Native build cannot find clang | Run `clang --version`. Add your LLVM installation to PATH. |
| Windows linker or SDK error | Run from a development shell with the MSVC tools and Windows SDK available. |
| An old version still runs | Check which executable your shell resolves, then reinstall with `--force`. |
| Only static checking is needed | Use `skuld check main.skuld`; clang is not invoked. |
