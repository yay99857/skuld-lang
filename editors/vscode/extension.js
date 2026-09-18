// Start `skuld-lsp` and let Visual Studio Code talk to it.
//
// The grammar beside this file colours a document on its own, without any
// process running. Everything else an editor can do about Skuld — completion,
// hover, go to definition, rename, the outline, signature help, inlay hints,
// the quick fixes the compiler attaches to its diagnostics — comes from the
// server, and none of it happened here until this file existed.
//
// The Neovim configuration in `../nvim/lsp/skuld.lua` finds the server the
// same way and for the same reasons; the two should not drift.

const vscode = require("vscode");
const { LanguageClient, TransportKind } = require("vscode-languageclient/node");
const { locate } = require("./server.js");

let client;

function activate(context) {
  const configured = vscode.workspace
    .getConfiguration("skuld")
    .get("server.path");
  // The open folders too: someone working on the language itself has the
  // server built in the checkout they are looking at, and the extension may
  // have been copied rather than linked, in which case its own location says
  // nothing about where the repository is.
  const workspaces = (vscode.workspace.workspaceFolders ?? []).map(
    (folder) => folder.uri.fsPath,
  );
  const command = locate(configured, { workspaces });
  if (command === undefined) {
    // Said once, with what to do about it. Highlighting still works, so the
    // extension is not broken — it is doing half of its job, and silence
    // would read as the other half being unsupported.
    vscode.window.showWarningMessage(
      "Skuld: `skuld-lsp` was not found, so completion, hover and go-to-definition " +
        "are off. Build it with `cargo build --release`, put it on PATH, or set " +
        "`skuld.server.path`. Syntax highlighting is unaffected.",
    );
    return;
  }

  client = new LanguageClient(
    "skuld",
    "Skuld Language Server",
    {
      run: { command, transport: TransportKind.stdio },
      debug: { command, transport: TransportKind.stdio },
    },
    {
      // Both extensions the language answers to. A Skuld program has no
      // manifest, so there is no project file to look for and no workspace
      // shape to require: the server reads each open document as the entry
      // file of its own program, which is how the compiler resolves an import
      // too.
      documentSelector: [
        { scheme: "file", language: "skuld" },
        { scheme: "untitled", language: "skuld" },
      ],
    },
  );

  context.subscriptions.push(client);
  client.start();
}

function deactivate() {
  if (client === undefined) {
    return undefined;
  }
  return client.stop();
}

module.exports = { activate, deactivate };
