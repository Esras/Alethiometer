# Windows

The app builds for Windows (x86_64, MSVC) as far as a type-check from Linux can tell; it hasn't been built or run on Windows yet. This is the checklist for the first run. What's known to be missing is in ISSUES.md ("Windows").

## Set up

- Rust with the MSVC toolchain: `rustup default stable-x86_64-pc-windows-msvc`.
- Visual Studio Build Tools, workload "Desktop development with C++" (C compiler, `ml64` and the Windows SDK, for SQLite, sqlite-vec, tree-sitter and tract).
- Node 22+. WebView2 is already on Windows 10 and 11.
- Git with `core.autocrlf` left as it is: `.gitattributes` keeps the checkout LF.
- Windows 10 1803 or later (the agent host uses AF_UNIX sockets).

## Build and test

In PowerShell, from `app`:

```powershell
npm ci
cd agentd; cargo test; cd ..
cd core; cargo test; cd ..
cd settings; cargo test; cd ..
cd platform; cargo test; cd ..
cd src-tauri; cargo test --no-default-features; cd ..
npm run check
npm test
npx tauri build
```

Worth watching in the test run:

- `platform`: `concurrent_writers_never_leave_a_mix` (eight threads renaming over one file; a sharing violation would show here first).
- `agentd`: `kill_ends_the_agents_job_and_nothing_else` (Windows only: ending an agent ends what it started, and nothing else).
- `src-tauri/tests/agents_restart.rs` (needs `node` on the PATH): agents outliving the app, through the agent host's socket.
- `obsidian::tests::refuses_paths_outside_the_vault` (the vault path without its `\\?\` prefix).
- `which_finds_a_cmd_shim_on_the_path` (`npx` found as `npx.cmd`).

`npx tauri build` should leave an MSI and an NSIS installer under `src-tauri/target/release/bundle/`, each with `alethiometer-agentd.exe` next to `Alethiometer.exe`. Neither is signed, so SmartScreen warns.

## By hand, in the installed app

1. It opens with one title bar (Windows' own), no console windows flashing up while it syncs.
2. Links open in the default browser: "open in GitHub", Settings › Open settings.json, Google's Connect.
3. Keys: Ctrl+K, Ctrl+, and the rest work; on a keyboard layout with AltGr (German, French), typing `[` `]` `{` `}` `@` in Notes types them instead of changing page.
4. Obsidian: set `obsidian_vault` to a vault on an NTFS drive; save a topic note; "open in Obsidian" opens the note.
5. Agents: New agent → Local, Claude Code (Claude Code on Windows needs Git Bash) or Pi. Check that it starts, asks for an approval, survives closing and reopening the app, and that Stop ends it with its child processes (Task Manager). Rebuild the agent host (`npm run agentd`) while nothing runs: the next start swaps in the new build.
6. Data is in `%APPDATA%\dev.alethiometer.app`; `secrets.json` is there too.
7. Ctrl+R and F5 when nothing failed: they may reload the page (WebView2's own keys). Note what happens.

Report what fails with the output; the tests name the behaviour they check.
