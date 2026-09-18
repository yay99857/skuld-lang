// Where `skuld-lsp` is.
//
// Its own file, and taking the configured path as an argument rather than
// reading it, so that it can be run outside Visual Studio Code. This is the
// part most likely to be wrong on a machine that is not the one it was written
// on, and a function that can only run inside an editor is a function nothing
// tests.
//
// `../nvim/lsp/skuld.lua` answers the same question the same way; the two
// should not drift.

const { execFileSync } = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");

/// The command to run, or nothing when there is none to run.
///
/// A configured path wins, because someone who has said where it is has said
/// so for a reason. Then an installed `skuld-lsp`. Then this checkout's own
/// build — release first, since that is what someone who ran
/// `cargo build --release` expects to be used, and debug after, so the server
/// works during development with no install step at all.
function locate(configured, options) {
  const settings = options ?? {};
  const platform = settings.platform ?? process.platform;
  const exists = settings.exists ?? fs.existsSync;
  const installed = settings.installed ?? onPath;

  if (typeof configured === "string" && configured.trim().length > 0) {
    return configured.trim();
  }
  const name = platform === "win32" ? "skuld-lsp.exe" : "skuld-lsp";
  if (installed(name, platform)) {
    return name;
  }
  // Two checkouts to consider, and they are rarely the same one. The
  // extension's own, which is the repository when the folder is linked and is
  // `~/.vscode` when it was copied instead — a copy is what a machine without
  // permission to make links ends up with, and the search has to survive it.
  // And the open folder, which is the repository whenever someone is working
  // on the language itself.
  const roots = settings.roots ?? [
    path.join(__dirname, "..", ".."),
    ...(settings.workspaces ?? []),
  ];
  for (const root of roots) {
    for (const profile of ["release", "debug"]) {
      const built = path.join(root, "target", profile, name);
      if (exists(built)) {
        return built;
      }
    }
  }
  return undefined;
}

/// Whether a name resolves to something runnable, asked of the system rather
/// than worked out from `PATH` — what counts as executable differs between
/// systems, and this way neither rule has to be restated here.
function onPath(name, platform) {
  const finder = (platform ?? process.platform) === "win32" ? "where" : "which";
  try {
    execFileSync(finder, [name], { stdio: "ignore" });
    return true;
  } catch {
    return false;
  }
}

module.exports = { locate, onPath };
