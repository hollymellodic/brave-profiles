# Brave Profile Manager

Windows desktop app for managing many Brave browser profiles at once. Create, rename, delete, launch, and sync settings across profiles from a single table view.

Built with Tauri 2 and Rust. The frontend is one static HTML file, no Node.js toolchain required.

## Features

- **Bulk create**: generate up to 200 profiles at a time as `Prefix + number` (for example `HM01` to `HM200`), with configurable start, digit width, and count.
- **Bulk rename**: re-prefix selected profiles with an optional numbering pattern (`{n}`), custom start, and step. Duplicates and in-use profiles are skipped.
- **Bulk delete**: remove selected profiles behind a confirmation dialog. The `Default` profile is protected.
- **Launch / stop**: open any profile in Brave with `--profile-directory=...`, or close its windows. `Stop all` asks Brave to close normally (WM_CLOSE) first, then force-kills as a fallback.
- **Sync from Default**: copy `Preferences`, `Bookmarks`, and extension folders from `Default` into every other profile. Identity keys (`signin`, `sync`, cookies, autofill, and similar) are stripped first, so settings propagate without carrying the signed-in account along.
- **Machine-wide policy** (optional, triggers UAC): writes a registry policy under `HKLM\SOFTWARE\Policies\BraveSoftware\Brave` for homepage, forced extension installs, and disabling Brave Rewards and Brave News.
- **Account detection**: each row shows the Google account Brave recorded for the profile (from `Local State`), with a cookie-based sign-in fallback, plus live status: Running, Not running, or Unopened.
- **Safety**: every write to `Local State` and `Preferences` creates a backup (only the newest is kept) and is verified afterward. Profiles currently in use are never modified.

## Requirements

- Windows 10 or 11 (the app uses Windows APIs for process and window handling)
- [Brave Browser](https://brave.com/) at the default install path, or set `BPM_BRAVE_EXE`
- To build: [Rust](https://rustup.rs/) stable and the [Tauri 2 prerequisites](https://tauri.app/start/prerequisites/)

## Building

```powershell
cd app-tauri
cargo build --release
```

The binary appears at `app-tauri/target/release/brave-profile-manager.exe`.

For a dev run from `app-tauri/`:

```powershell
cargo run
```

## Testing

The app includes a self-test that runs all write logic against a fake User Data directory in `%TEMP%`. Your real Brave data is never touched.

```powershell
.\brave-profile-manager.exe --selftest
```

Output goes to stdout and `%TEMP%\bpm-selftest.log`. There is also `--diag`, which writes runtime diagnostics to `%TEMP%\bpm-diag.log`.

## Configuration

Config file: `%APPDATA%\brave-profile-manager\config.json`

| Key | Meaning |
| --- | --- |
| `user_data_dir` | Brave User Data directory. Defaults to `%LOCALAPPDATA%\BraveSoftware\Brave-Browser\User Data`. |
| `prefix` | Last-used prefix, pre-filled in the New and Rename dialogs. |

### Environment variables

| Variable | Effect |
| --- | --- |
| `BPM_USER_DATA` | Override the User Data directory (used by the self-test). |
| `BPM_BRAVE_EXE` | Override the path to `brave.exe`. |
| `BPM_IGNORE_BRAVE` | Ignore any running Brave instance (self-test). |

## Usage notes

- **Sync** and most destructive operations need Brave closed. Use **Stop all** first.
- Creating profiles while Brave runs is allowed, but renaming or deleting a profile that is currently open is skipped, to avoid corrupting locked SQLite files.
- **Policy** writes to `HKLM`, so it triggers a UAC prompt.

## Project layout

```
app-tauri/
├── src/main.rs        # Backend: Tauri commands and self-test
├── ui/index.html      # Frontend: single file, vanilla JS
├── icons/             # App icons
├── tauri.conf.json    # Tauri configuration
├── Cargo.toml
└── build.rs
dist/                  # Prebuilt exe, not tracked in git
```

## License

[MIT](LICENSE)
