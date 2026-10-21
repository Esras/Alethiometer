# Setting up Alethiometer

This gets you from a fresh clone to the app running on sample data, then onto your own accounts. It takes about 15 minutes, most of it the first Rust build.

## 1. Prerequisites

**With Nix (macOS and Linux).** Install [Nix](https://nixos.org/download) with flakes and [direnv](https://direnv.net) (2.30 or later), then run `direnv allow` once at the repo root. Rust, Node 22 and the system libraries then come from the repo's devshell (`flake.nix`, `devshell.toml`), not your system.

**Without Nix**, install:

- Rust, stable, through [rustup](https://rustup.rs).
- Node.js 22 or later.
- macOS: the Xcode command line tools (`xcode-select --install`).
- Linux (Debian/Ubuntu names): `libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev librsvg2-dev libayatana-appindicator3-dev`. Only the desktop app needs these; browser mode (step 2) doesn't.
- Windows: see `app/README.md` › Desktop. Windows has never been run, so expect to hit problems (`app/docs/windows.md` is the checklist).

Then install the front-end packages:

```sh
cd app
npm ci
```

## 2. Look around on sample data (browser mode)

The quickest way to see the app. The Rust core runs as a small local server and the UI runs in your browser.

```sh
cd app
ALETHIOMETER_FAKE_NOW=10:52 npm run devserver   # terminal 1: builds, then serves on 127.0.0.1:1421
npm run dev                                      # terminal 2: UI on http://localhost:1420
```

Open <http://localhost:1420>. The first `devserver` build takes several minutes; later ones are quick. `ALETHIOMETER_FAKE_NOW` pins the clock to 10:52 so the day strip matches the sample day; leave it out to use the real time. The sample data lives in `app/.alethiometer-dev/`; delete that folder to start over.

Things to try:

- `⌘1`–`⌘7` (`Ctrl` on Linux and Windows) switch tabs: Incoming, Priorities, Topics, Todos, Notes, Reflect, Agents.
- In **Incoming**, `j`/`k` move between signals, `c` confirms the suggested topic, `t` moves one to another topic.
- `⌘K` or `/` searches everything. `⌘,` opens Settings. Each tab's footer shows its main keys.

The full key list is in `app/README.md` › Keys.

## 3. Run the desktop app

```sh
cd app
npx tauri dev
```

This opens the real window and rebuilds when you save a file. By default it uses the **same data directory as an installed copy** (step 4), so while you're developing keep it apart:

```sh
ALETHIOMETER_DATA_DIR=~/.alethiometer-dev npx tauri dev
```

To build an app you can keep in `/Applications` on a Mac:

```sh
cd app
npm run build:mac   # → src-tauri/target/release/bundle/macos/Alethiometer.app
```

The build is ad-hoc signed. To keep Keychain and Calendar approvals across rebuilds, set `bundle.macOS.signingIdentity` in `app/src-tauri/tauri.conf.json` to your Apple Development certificate (a free Apple ID works). No notarization is needed. On Linux and Windows, `npx tauri build` makes the platform's installers; neither has been used day to day.

## 4. Switch to your own data

The app fills an **empty** database with sample data while `demo_data` is on, which is the default. Turning it off later doesn't remove sample data that's already there. So before the first real launch, create the settings file with sample data off:

| OS | Data directory |
|---|---|
| macOS | `~/Library/Application Support/dev.alethiometer.app` |
| Linux | `~/.local/share/dev.alethiometer.app` |
| Windows | `%APPDATA%\dev.alethiometer.app` |

```sh
# macOS; use the path from the table on Linux
mkdir -p ~/Library/Application\ Support/dev.alethiometer.app
echo '{ "demo_data": false }' > ~/Library/Application\ Support/dev.alethiometer.app/settings.json
```

The app fills in every other setting with its default on first launch. If you've already launched it on sample data, quit, delete `alethiometer.sqlite` in that directory, set `"demo_data": false` in `settings.json`, and start it again.

## 5. Connect your accounts

Open Settings (`⌘,`) › **Sources** › **Add source**. Each source has its own guide, because most need you to create an app or token on the service's side first:

| Source | Guide | What you'll need |
|---|---|---|
| Slack | `app/docs/sources/slack.md` | A Slack app you create from the manifest in the guide; its user token |
| Gmail | `app/docs/sources/gmail.md` | A Google Cloud OAuth client; then **Connect** |
| Google Calendar | `app/docs/sources/gcal.md` | The same OAuth client; then **Connect** |
| Microsoft Teams | `app/docs/sources/teams.md` | An Entra app registration; channels need admin approval |
| Jira Cloud | `app/docs/sources/jira.md` | An API token |
| GitHub | `app/docs/sources/github.md` | The `gh` CLI signed in, or a fine-grained token |
| Obsidian | `app/docs/sources/obsidian.md` | Your vault path (Settings › General › Notes); the Local REST API plugin for writes |
| Proton Calendar | `app/docs/sources/protoncal.md` | "Limited view" share links; shows as busy Life blocks only |
| Glean | `app/docs/sources/glean.md` | A Glean API token; search only |

Secrets you paste go to `secrets.json` in the data directory (mode 0600) and are never shown again. The sync status in the tab bar shows each source's last sync and any error; a red dot in Settings marks a missing secret or a failed sync.

Microsoft Teams, Proton Calendar and Glean have only been tested against recorded API responses. If a source misbehaves, check `ISSUES.md` first and add what you find there.

## 6. Optional extras

- **Work AI** (topic suggestions, the assistant pane, Reflect's ask): off until you pick models. It runs on Amazon Bedrock with your AWS profile and only calls providers listed in `ai.allowed_providers`, which is a hand edit in `settings.json`. See `app/docs/ai.md`.
- **Local search embeddings**: downloaded from Hugging Face on first run (needs to reach `huggingface.co` once). If that fails, retry from Settings › LLM Providers.
- **Agents tab**: runs Claude Code through `npx -y @agentclientprotocol/claude-agent-acp`, and Pi through `pi-acp` (also needs `pi` on your PATH). Each agent signs in its own way; Alethiometer passes no credentials. See `app/docs/agents.md`.

## Where things are

Everything lives in the data directory from step 4 (or `ALETHIOMETER_DATA_DIR`):

- `alethiometer.sqlite`: the database.
- `settings.json`: every setting. Edits in Settings or by hand apply live (`app/docs/settings.md`).
- `secrets.json`: tokens. `secrets.meta.json` keeps only names and when each was set.
- `models/`: the embedding model.
- `vault/`: a stand-in Obsidian vault until you set `obsidian_vault`.

Back up the whole directory to back up Alethiometer.
