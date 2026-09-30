# Skuld documentation website

Static English documentation, built from Markdown with TypeScript. The site has no client framework, hydration, server backend, database or execution service.

## Develop

Requires Node.js 22+ and npm.

```sh
npm ci
npm run build
npm run dev
```

Open http://127.0.0.1:4173. After editing content, rebuild; the server reads the new output on the next request. `dist/` is disposable generated output. A production static host must serve directory indexes and `404.html` for missing paths.

## Validate

```sh
npm run typecheck
npm run lint
npm run build
npm test
```

The validation script checks internal routes, heading fragments, metadata, supported highlighter languages and complete Skuld programs. It uses `../target/debug/skuld.exe` on Windows or `../target/debug/skuld` elsewhere; build the compiler with `cargo build -p skuld-cli` first, or set `SKULD_BIN` to another current executable. Browser verification is `npx playwright test` with the local server running; install Chromium with `npx playwright install chromium` when necessary.

## Content and components

- `content/docs/`: Markdown articles, metadata and feature status.
- `content/examples.md`, `playground.md`, `changelog.md`: standalone routes.
- `src/config.ts`: single navigation order, current version and canonical origin.
- `src/highlight.ts`: dedicated `skuld` grammar and shared code-block rendering; highlight.js also provides Bash, JSON, TOML, C, C++, Rust, JavaScript and TypeScript.
- `scripts/build.ts`: static layout, navigation, breadcrumbs, TOC, pagination, metadata and local search index generation.
- `src/client.ts`: theme, search, copy, tabs, mobile drawer, scroll tracking and the unconnected playground shell.
- `src/site.css`: shared dark/light tokens and responsive styling.

Use `skuld filename.skuld numbers highlight=1,3` as a fenced code block's info string to enable a filename, line numbers and highlighted lines. Omit `numbers` for short snippets. Callouts use `> [!Note]`, `Tip`, `Warning`, `Important` or `Experimental`. Use semantic HTML for tabs and API parameter tables within Markdown. Raw HTML is trusted repository content, never user input.

The compiler's version is currently 0.1.0. The selector deliberately contains only that version. Add version content trees and route prefixes when a second version is actually maintained, rather than inventing historical documentation.

Set `SITE_URL` when building for another domain. No unverified installer, package manager or remote code runner is advertised. The playground downloads source and reports execution as unavailable; its controls do not simulate compiler results.

## Content review

Consult `../LANGUAGE.md`, `../README.md`, `../test.skuld`, executable fixtures and current `../std/` sources. Older passages in the repository can predate later implemented milestones. Check examples with the current compiler and mark unimplemented features as Planned. Do not change the user's root syntax sketches.
