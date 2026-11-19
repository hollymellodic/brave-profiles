# Brave Profile Manager

A small Windows desktop app (Tauri 2 + Rust) for managing many Brave browser profiles at once — create, rename, delete, launch, and sync settings across profiles from a single table view.

## Features

- **Bulk create** — generate up to 200 profiles at a time as `Prefix + number` (e.g. `HM01` … `HM200`), with configurable start, digit width, and count.
- **Bulk rename** — re-prefix selected profiles with an optional numbering pattern (`{n}`), custom start, and step. Duplicates and in-use profiles are skipped safely.
- **Bulk delete** — remove selected profiles with a confirmation dialog. The `Default` profile is protected and cannot be deleted.
- **Launch / stop** — open any profile in Brave (`--profile-directory=...`) or close its windows. A graceful `Stop all` asks Brave to close normally (WM_CLOSE) before falling back to force-kill.
- **Sync from Default** — mirror `Preferences`, `Bookmarks`, and extension folders from the `Default` profile into every other profile. Account/session identity keys (`signin`, `sync`, `sync`, cookies, autofill, …) are stripped first, so settings propagate without leaking the signed-in account into other profiles.
- **Machine-wide policy** (optional, requires UAC) — writes a registry policy under `HKLM\SOFTWARE\Policies\BraveSoftware\Brave`: homepage, forced extension install list, and disables Brave Rewards / Brave News.
- **Account awareness** — each row shows the Google account recorded for the profile (read from Brave's `Local State`), with a cookie-based sign-in fallback, plus live status: *Running*, *Not running*, or *Unopened*.
- **Safety** — every write to `Local State` / `Preferences` is backed up (only the latest backup is kept) and verified; operations refuse to touch profiles that are currently in use.

## Requirements

- Windows 10/11 (the app uses Windows-specific APIs for process/window handling)
- [Brave Browser](https://brave.com/) installed at the default path (overridable, see Environment variables)
- For building: [Rust](https://rustup.rs/) (stable) and the usual [Tauri 2 prerequisites](https://tauri.app/start/prerequisites/)

## Building

```powershell
cd app-tauri
cargo build --release
```

The binary lands at `app-tauri/target/release/brave-profile-manager.exe`.

Run the UI in dev mode (from `app-tauri/`):

```powershell
cargo run
```

> The frontend is a single static file (`app-tauri/ui/index.html`) — no Node.js toolchain needed.

## Testing

The app ships with a self-test that runs all write logic against a fake User Data directory in `%TEMP%` (your real Brave data is never touched):

```powershell
.\brave-profile-manager.exe --selftest
```

Results are printed to stdout and written to `%TEMP%\bpm-selftest.log`. There is also a `--diag` flag that dumps runtime diagnostics to `%TEMP%\bpm-diag.log`.

## Configuration

Config is stored at `%APPDATA%\brave-profile-manager\config.json`:

| Key | Meaning |
| --- | --- |
| `user_data_dir` | Brave User Data directory. Defaults to `%LOCALAPPDATA%\BraveSoftware\Brave-Browser\User Data`. |
| `prefix` | Last-used prefix, pre-filled in the New/Rename dialogs. |

### Environment variables

| Variable | Effect |
| --- | --- |
| `BPM_USER_DATA` | Override the User Data directory (used by the self-test). |
| `BPM_BRAVE_EXE` | Override the path to `brave.exe`. |
| `BPM_IGNORE_BRAVE` | Ignore any running Brave instance (self-test). |

## Usage notes

- **Sync** and most destructive operations require Brave to be closed — use **Stop all** first.
- Creating profiles while Brave is running is allowed, but renaming/deleting a profile that is currently open is skipped to avoid corrupting locked SQLite files.
- **Policy** triggers a UAC prompt because it writes to `HKLM`.

## Project layout

```
app-tauri/
├── src/main.rs        # All backend logic (Tauri commands + self-test)
├── ui/index.html      # Single-file frontend (vanilla JS, no build step)
├── icons/             # App icons
├── tauri.conf.json    # Tauri configuration
├── Cargo.toml
└── build.rs
dist/                  # Prebuilt exe (not tracked in git)
```

## License

No license specified yet — all rights reserved by default. Add one if you plan to open this up.
