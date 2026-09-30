<p class="eyebrow">WRITE / INSPECT / TAKE IT LOCAL</p>

# Skuld playground

Explore the shape of a Skuld program. Edit the source here, then download it to run with the native compiler.

> [!Experimental]
> Playground execution is not available yet. There is no browser compiler or remote execution service connected to this editor.

<div class="playground-grid">
<div class="editor-panel"><div class="panel-label"><label for="editor">main.skuld</label></div><textarea id="editor" spellcheck="false" aria-describedby="editor-help">func main() {
    let name = "Skuld"
    print("Hello, ${name}!")
}</textarea></div>
<div><div class="panel-label">Output</div><div id="playground-output" class="playground-output" role="status">Playground execution is not available yet.</div></div>
</div>
<div class="playground-actions"><button id="run" class="button primary">Run locally ↗</button><button id="reset" class="button">Reset</button><button id="download" class="button">Download source</button><span id="editor-help">Ctrl / ⌘ + Enter shows local run instructions.</span></div>

## Run on your machine

Download the source, then run it with your [installed compiler](/docs/getting-started/installation/).

```bash
skuld run main.skuld
```

Source stays in this page unless you download it. No code is sent to a server for execution.
