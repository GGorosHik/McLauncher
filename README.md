# MC Launcher

Compact Minecraft launcher built with **Tauri 2 + Rust**. It installs a prebuilt client
from a GitHub release, checks for updates on its own, lets you pick the install folder and
how much RAM the game may use, then starts the client.

The whole app is ~16 MB on disk with no bundled Chromium — Windows uses the WebView2
runtime that is already installed on the system.

## Features

- **GitHub update checks.** Reads `https://api.github.com/repos/<owner>/<repo>/releases`,
  compares the release tag with the tag recorded after the last install and reports whether
  an update is available. Leave the tag field empty to track `latest`.
- **One-click install.** Downloads the release `.zip` with live progress, verifies the
  entry paths (no absolute paths, no `..`, no drive letters), extracts it and copies the
  contents into the folder you picked. Archives that wrap everything in a single folder are
  unwrapped automatically.
- **Folder picker.** Native dialog for the install location and for `java.exe`.
- **Memory sliders.** Minimum and maximum heap in whole gigabytes, clamped to the detected
  system memory and passed to the game as `-Xms` / `-Xmx`.
- **Two launch modes.** If the installed folder contains `start.bat` or `start-local.bat`,
  that script runs. Otherwise the launcher assembles a classpath from `libraries/` and
  `versions/<id>/<id>.jar` and starts the Fabric client directly.
- **Java detection.** Scans `JAVA_HOME`, `PATH`, `Program Files\Java`,
  `Eclipse Adoptium`, `Microsoft`, `Zulu` and `BellSoft`, then prefers a `jre/` folder
  bundled inside the client and finally the newest Java 17+ it can find.

## Requirements

- Windows 10/11 with WebView2 (preinstalled on modern builds).
- Java 17+ for a classpath launch. A bundled `jre/` inside the client is used when present.
- Rust 1.77+ and Node.js 20+ to build from source.

## Run locally

```bash
cd src-tauri
cargo build
cargo run
```

The frontend is plain HTML/CSS/JS served from `frontend/`, so no bundler step is needed.
Tauri points at it through `build.frontendDist`.

## Build the installer

```bash
npm install --no-save @tauri-apps/cli@^2
npx tauri build
```

Output lands in `src-tauri/target/release/bundle/`:

| Target | File |
| --- | --- |
| NSIS | `nsis/MC Launcher_0.1.0_x64-setup.exe` |
| MSI | `msi/MC Launcher_0.1.0_x64_en-US.msi` |

## Prepare a client bundle

Zip the client folder so the archive root contains the game files:

```text
start.bat          (or start-local.bat)
jre/
libraries/
versions/
assets/
game/
```

Attach the ZIP to a GitHub release of any repository:

```bash
gh release create v1.0.0 ./mc-client-v1.0.0.zip --title "Client 1.0.0"
```

Or upload it through **Releases → Draft a new release → attach file → Publish**.
The asset name must end in `.zip`; when several exist, the first `.zip` is used.

Then in the launcher:

1. Set **Update source** to `owner/repo` (the repo holding the bundle release).
2. Press **Check for updates**.
3. Press **Download client** — it installs into the chosen folder and stores the tag.
4. Press **Play**.

## Publishing this launcher

```bash
git remote add origin https://github.com/<owner>/<repo>.git
git push -u origin main
```

`.github/workflows/build.yml` builds Windows and Linux installers on every push, and
publishes a GitHub release with the installers attached when you push a `v*` tag:

```bash
git tag v0.1.0
git push origin v0.1.0
```

## Project layout

```text
frontend/
  index.html          window markup
  styles.css          dark theme
  app.js              IPC calls, sliders, progress, polling
src-tauri/
  src/main.rs         Tauri commands and wiring
  src/config.rs       settings persistence, client layout detection, RAM probe
  src/update.rs       GitHub release lookup, download, ZIP extraction
  src/java.rs         Java discovery and version probing
  src/game.rs         launch plan (script or classpath) and process control
  tauri.conf.json     window, bundle and CSP configuration
```

## Environment variables

| Variable | Purpose |
| --- | --- |
| `LAUNCHER_REPO` | Default `owner/repo` for update checks. |
| `LAUNCHER_USERNAME` | Offline username used for a classpath launch (default `Player`). |

## Notes

- The installer is unsigned. Windows SmartScreen may warn on first run; code signing is a
  separate release step.
- Offline launches use `--accessToken 0`; premium servers will reject that, exactly like any
  offline launcher.
