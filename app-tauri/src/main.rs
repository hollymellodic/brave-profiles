#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

const NO_WINDOW: u32 = 0x0800_0000;
const SRC: &str = "Default";
const POLICY: &str = r"HKLM\SOFTWARE\Policies\BraveSoftware\Brave";
const CRX: &str = "https://clients2.google.com/service/update2/crx";
// dibuang saat cermin: identitas/akun/data pribadi per profile
const IDENTITY: [&str; 12] = [
    "signin", "account_info", "gaia_cookie", "sync", "brave_sync_v2", "gcm",
    "profile", "partition", "autofill", "sessions", "session", "bookmark",
];

#[derive(Serialize, Deserialize, Clone)]
struct Config {
    user_data_dir: String,
    prefix: String,
}

impl Default for Config {
    fn default() -> Self {
        let base = std::env::var("LOCALAPPDATA").unwrap_or_default();
        Config {
            user_data_dir: format!("{base}\\BraveSoftware\\Brave-Browser\\User Data"),
            prefix: String::new(),
        }
    }
}

static JOB: Mutex<Option<(String, u32, u32)>> = Mutex::new(None);

fn job_set(op: &str, done: u32, total: u32) {
    *JOB.lock().unwrap() = Some((op.to_string(), done, total));
}
fn job_clear() {
    *JOB.lock().unwrap() = None;
}
fn job_json() -> Value {
    match JOB.lock().unwrap().as_ref() {
        Some((op, d, t)) => json!({ "op": op, "done": d, "total": t }),
        None => Value::Null,
    }
}

fn cfg_path() -> PathBuf {
    PathBuf::from(std::env::var("APPDATA").unwrap_or_default())
        .join("brave-profile-manager")
        .join("config.json")
}

fn load_cfg() -> Config {
    fs::read_to_string(cfg_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_cfg(c: &Config) -> Result<(), String> {
    let p = cfg_path();
    if let Some(d) = p.parent() {
        fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    fs::write(p, serde_json::to_string_pretty(c).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

fn ts() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

// Simpan hanya satu backup terbaru per file — jangan menumpuk sampah di disk.
fn prune_backups(p: &Path) {
    let dir = p.parent().unwrap_or(Path::new("."));
    let base = match p.file_name().and_then(|s| s.to_str()) {
        Some(b) => format!("{b}.bak-"),
        None => return,
    };
    let Ok(rd) = fs::read_dir(dir) else { return };
    let mut baks: Vec<PathBuf> = rd
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|q| {
            q.file_name()
                .and_then(|s| s.to_str())
                .map(|s| s.starts_with(&base))
                .unwrap_or(false)
        })
        .collect();
    baks.sort();
    while baks.len() > 1 {
        let old = baks.remove(0);
        let _ = fs::remove_file(old);
    }
}

fn read_json(p: &Path) -> Option<Value> {
    fs::read_to_string(p).ok().and_then(|s| serde_json::from_str(&s).ok())
}

fn write_json(p: &Path, v: &Value) -> Result<(), String> {
    if p.exists() {
        let bak = PathBuf::from(format!("{}.bak-{}", p.display(), ts()));
        fs::rename(p, bak).map_err(|e| e.to_string())?;
        prune_backups(p);
    }
    fs::write(p, serde_json::to_string(v).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

fn ud() -> PathBuf {
    if let Ok(p) = std::env::var("BPM_USER_DATA") {
        return PathBuf::from(p);
    }
    PathBuf::from(load_cfg().user_data_dir)
}

fn run(exe: &str, args: &[&str]) -> (bool, String) {
    let mut c = Command::new(exe);
    c.args(args);
    #[cfg(windows)]
    c.creation_flags(NO_WINDOW);
    match c.output() {
        Ok(o) => {
            let out = String::from_utf8_lossy(&o.stdout).to_string()
                + &String::from_utf8_lossy(&o.stderr);
            (o.status.success(), out)
        }
        Err(e) => (false, e.to_string()),
    }
}

fn brave_open() -> bool {
    if std::env::var("BPM_IGNORE_BRAVE").is_ok() {
        return false; // self-test tidak boleh bergantung pada Brave yang kebetulan jalan
    }
    let (_, out) = run("tasklist", &["/FI", "IMAGENAME eq brave.exe"]);
    out.to_lowercase().contains("brave.exe")
}

// Profile yang sedang dipakai: Brave mengunci file History (SQLite) selama profile terbuka.
#[cfg(windows)]
fn profile_in_use(ud: &Path, dir: &str) -> bool {
    use std::os::windows::fs::OpenOptionsExt;
    let p = ud.join(dir).join("History");
    if !p.exists() {
        return false;
    }
    match fs::OpenOptions::new().read(true).write(true).share_mode(0).open(&p) {
        Ok(h) => {
            drop(h);
            false
        }
        Err(_) => true,
    }
}
#[cfg(not(windows))]
fn profile_in_use(_ud: &Path, _dir: &str) -> bool {
    false
}

fn running_profiles(ud: &Path) -> Vec<String> {
    if !brave_open() {
        return Vec::new();
    }
    info_cache(ud)
        .as_object()
        .map(|m| m.keys().filter(|d| profile_in_use(ud, d)).cloned().collect())
        .unwrap_or_default()
}

fn need_closed() -> Result<(), String> {
    if brave_open() {
        Err("Brave is running. Close it first (Stop all).".into())
    } else {
        Ok(())
    }
}

fn info_cache(ud: &Path) -> Value {
    read_json(&ud.join("Local State"))
        .and_then(|v| v.get("profile").and_then(|p| p.get("info_cache")).cloned())
        .unwrap_or_else(|| json!({}))
}

fn last_used(ud: &Path) -> String {
    read_json(&ud.join("Local State"))
        .and_then(|v| v.get("profile").and_then(|p| p.get("last_used")).cloned())
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_else(|| SRC.into())
}

fn find_bytes(hay: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && hay.len() >= needle.len() && hay.windows(needle.len()).any(|w| w == needle)
}

fn is_email_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b'+' | b'%' | b'@')
}

// Brave menyimpan email akun Google sebagai teks biasa di Login Data / Web Data / History.
// Tidak perlu dekripsi: cukup ambil deretan karakter email dari file SQLite.
fn scan_emails(data: &[u8], out: &mut Vec<String>) {
    let mut i = 0;
    while i < data.len() {
        if data[i] == b'@' {
            let mut l = i;
            while l > 0 && is_email_char(data[l - 1]) {
                l -= 1;
            }
            let mut r = i + 1;
            while r < data.len() && is_email_char(data[r]) {
                r += 1;
            }
            if let Ok(mut s) = String::from_utf8(data[l..r].to_vec()) {
                s = s.trim_matches(|c: char| c == '.' || c == '-' || c == '_').to_lowercase();
                // label form ("_username" + email) ikut terbaca: buang bagian sebelum kata penanda
                for tag in ["username", "accountname", "newemail", "email"] {
                    if let Some(pos) = s.rfind(tag) {
                        let rest = s[pos + tag.len()..].to_string();
                        if rest.contains('@') {
                            s = rest;
                        }
                    }
                }
                let parts: Vec<&str> = s.split('@').collect();
                if parts.len() == 2 {
                    let (local, dom) = (parts[0], parts[1]);
                    let google = dom == "gmail.com" || dom == "googlemail.com" || dom == "google.com";
                    if google && local.len() >= 2 && local.len() <= 40 {
                        out.push(s);
                    }
                }
            }
            i = r;
        } else {
            i += 1;
        }
    }
}

fn emails_in(ud: &Path, dir: &str, file: &str) -> Vec<String> {
    let p = ud.join(dir).join(file);
    let Ok(data) = fs::read(&p) else { return Vec::new() };
    let mut v = Vec::new();
    scan_emails(&data, &mut v);
    v
}

fn most_frequent(v: &[String]) -> String {
    let mut best = String::new();
    let mut best_n = 0;
    for cand in v {
        let n = v.iter().filter(|x| *x == cand).count();
        if n > best_n {
            best_n = n;
            best = cand.clone();
        }
    }
    best
}

// level: 2 = cookie sign-in ada di profile ini, 1 = cookie Google biasa, 0 = tidak ada
fn google_level(ud: &Path, dir: &str) -> i64 {
    let cands = [ud.join(dir).join("Network").join("Cookies"), ud.join(dir).join("Cookies")];
    for f in cands.iter().filter(|p| p.exists()) {
        let Ok(data) = fs::read(f) else { continue };
        if find_bytes(&data, b"LSID") || find_bytes(&data, b"__Host-GAPS") || find_bytes(&data, b"__Secure-3PSID") {
            return 2;
        }
        if find_bytes(&data, b"SAPISID") || find_bytes(&data, b"SSID") {
            return 1;
        }
    }
    0
}

// Sumber nama akun, urut kepercayaan: Login Data (kredensial tersimpan) ->
// History (akun yang dipakai di profile ini) -> Web Data (autofill).
fn account_name(ud: &Path, dir: &str) -> String {
    let mut u = emails_in(ud, dir, "Login Data");
    u.sort();
    u.dedup();
    if u.len() == 1 {
        return u[0].clone();
    }
    let hist = most_frequent(&emails_in(ud, dir, "History"));
    if !hist.is_empty() {
        return hist;
    }
    let mut w = emails_in(ud, dir, "Web Data");
    w.sort();
    w.dedup();
    if w.len() == 1 {
        return w[0].clone();
    }
    String::new()
}

fn strip_prefix(name: &str, prefix: &str) -> String {
    if !prefix.is_empty() && name.starts_with(prefix) {
        name[prefix.len()..].to_string()
    } else {
        name.to_string()
    }
}

fn with_prefix(name: &str, prefix: &str) -> String {
    if prefix.is_empty() || name.starts_with(prefix) {
        name.to_string()
    } else {
        format!("{prefix}{name}")
    }
}

// Satu tulis + verifikasi; diulang bila Brave menulis ulang Local State.
fn commit_ls(ud: &Path, v: &Value, checks: &[(String, Option<String>)]) -> Result<(), String> {
    let ls = ud.join("Local State");
    for attempt in 0..3 {
        if attempt > 0 {
            std::thread::sleep(Duration::from_millis(700));
        }
        write_json(&ls, v)?;
        std::thread::sleep(Duration::from_millis(700));
        let ic = info_cache(ud);
        let ok = checks.iter().all(|(dir, want)| match want {
            Some(n) => ic.get(dir).and_then(|x| x.get("name")).and_then(|x| x.as_str()) == Some(n.as_str()),
            None => ic.get(dir).is_none(),
        });
        if ok {
            return Ok(());
        }
    }
    Err("Brave keeps rewriting Local State. Click Stop all, then retry.".into())
}

#[tauri::command]
fn get_state() -> Value {
    let cfg = load_cfg();
    let ud = PathBuf::from(&cfg.user_data_dir);
    let ic = info_cache(&ud);
    let running = running_profiles(&ud);
    let mut list: Vec<Value> = Vec::new();
    if let Some(map) = ic.as_object() {
        for (dir, v) in map {
            let mut recorded = v.get("user_name").and_then(|x| x.as_str()).unwrap_or("").to_string()
                + v.get("gaia_name").and_then(|x| x.as_str()).unwrap_or("");
            if recorded.is_empty() {
                recorded = account_name(&ud, dir);
            }
            let level = google_level(&ud, dir);
            list.push(json!({
                "dir": dir,
                "name": v.get("name").and_then(|x| x.as_str()).unwrap_or(dir),
                "account": recorded,
                "google": level,
                "opened": ud.join(dir).join("Preferences").exists(),
                "running": running.iter().any(|d| d == dir),
                "is_default": dir == SRC,
            }));
        }
    }
    list.sort_by(|a, b| {
        let key = |x: &Value| {
            let d = x["dir"].as_str().unwrap_or("");
            let n: u64 = d.rsplit(' ').next().and_then(|s| s.parse().ok()).unwrap_or(0);
            (d != SRC, n)
        };
        key(a).cmp(&key(b))
    });
    json!({
        "user_data_dir": cfg.user_data_dir,
        "prefix": cfg.prefix,
        "brave_open": brave_open(),
        "brave_running": running.len(),
        "last_used": last_used(&ud),
        "job": job_json(),
        "profiles": list,
    })
}

#[tauri::command]
fn create_bulk(prefix: String, start: i64, digits: u32, count: u32) -> Result<String, String> {
    if count == 0 || count > 200 {
        return Err("Count must be between 1 and 200.".into());
    }
    let mut cfg = load_cfg();
    cfg.prefix = prefix.clone();
    save_cfg(&cfg)?;

    let ud = ud();
    let ls = ud.join("Local State");
    let mut v = read_json(&ls).ok_or("Local State unreadable")?;
    let taken: Vec<String> = info_cache(&ud)
        .as_object()
        .map(|m| m.values().filter_map(|x| x["name"].as_str().map(String::from)).collect())
        .unwrap_or_default();
    let mut used_dirs: Vec<String> = info_cache(&ud)
        .as_object()
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default();
    let mut made: Vec<String> = Vec::new();
    let mut checks: Vec<(String, Option<String>)> = Vec::new();
    job_set("Creating", 0, count);
    for i in 0..count {
        let n = start + i as i64;
        let full = format!("{prefix}{:0width$}", n, width = digits as usize);
        if taken.contains(&full) {
            continue;
        }
        let mut k = 1;
        while used_dirs.contains(&format!("Profile {k}")) {
            k += 1;
        }
        let dir = format!("Profile {k}");
        fs::create_dir_all(ud.join(&dir)).map_err(|e| e.to_string())?;
        let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
        v["profile"]["info_cache"][&dir] = json!({
            "name": full, "active_time": secs, "gaia_id": "", "user_name": "",
            "is_using_default_name": false
        });
        used_dirs.push(dir.clone());
        checks.push((dir, Some(full.clone())));
        made.push(full);
        job_set("Creating", (i + 1) as u32, count);
    }
    if made.is_empty() {
        job_clear();
        return Ok("Nothing new to create.".into());
    }
    let committed = commit_ls(&ud, &v, &checks);
    job_clear();
    committed?;
    Ok(format!("Created {} profiles: {}", made.len(),
        if made.len() > 6 { format!("{} ... {}", made[0], made[made.len() - 1]) } else { made.join(", ") }))
}

#[tauri::command]
fn rename_selected(dirs: Vec<String>, prefix: String, pattern: String, start: i64, step: i64) -> Result<String, String> {
    if dirs.is_empty() {
        return Err("No profile selected.".into());
    }
    let cfg = load_cfg();
    let ud = ud();
    let ls = ud.join("Local State");
    let mut v = read_json(&ls).ok_or("Local State unreadable")?;
    let ic = v
        .get("profile")
        .and_then(|p| p.get("info_cache"))
        .cloned()
        .unwrap_or_else(|| json!({}));
    let pat = pattern.trim();
    let numbered = pat.contains("{n}");
    if prefix.trim().is_empty() && !numbered {
        return Err("Prefix is empty, so the pattern must contain {n}.".into());
    }
    if numbered && step == 0 {
        return Err("Step cannot be 0.".into());
    }
    let live = running_profiles(&ud);
    let mut used: Vec<String> = ic
        .as_object()
        .map(|m| m.iter().filter(|(k, _)| !dirs.contains(k))
            .filter_map(|(_, v)| v["name"].as_str().map(String::from)).collect())
        .unwrap_or_default();
    let mut changes: Vec<(String, String)> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut num = start;
    job_set("Renaming", 0, dirs.len() as u32);
    for (i, dir) in dirs.iter().enumerate() {
        let old = ic.get(dir).and_then(|v| v["name"].as_str()).unwrap_or(dir).to_string();
        if live.iter().any(|d| d == dir) {
            skipped.push(format!("{old} (in use)"));
            job_set("Renaming", (i + 1) as u32, dirs.len() as u32);
            continue;
        }
        let bare = strip_prefix(&old, &cfg.prefix);
        let cand = if numbered {
            with_prefix(&pat.replace("{n}", &num.to_string()), &prefix)
        } else {
            with_prefix(&bare, &prefix)
        };
        if numbered {
            num += step;
        }
        if cand.trim().is_empty() || used.contains(&cand) {
            skipped.push(format!("{old} (duplicate)"));
            job_set("Renaming", (i + 1) as u32, dirs.len() as u32);
            continue;
        }
        used.push(cand.clone());
        changes.push((dir.clone(), cand));
        job_set("Renaming", (i + 1) as u32, dirs.len() as u32);
    }
    if !changes.is_empty() {
        if let Some(map) = v["profile"]["info_cache"].as_object_mut() {
            for (d, n) in &changes {
                if let Some(e) = map.get_mut(d) {
                    e["name"] = json!(n);
                    if e.get("shortcut_name").is_some() {
                        e["shortcut_name"] = json!(n);
                    }
                }
            }
        }
        let checks: Vec<(String, Option<String>)> =
            changes.iter().map(|(d, n)| (d.clone(), Some(n.clone()))).collect();
        commit_ls(&ud, &v, &checks)?;
    }
    job_clear();
    let mut msg = format!("Renamed {} profiles", changes.len());
    if !skipped.is_empty() {
        msg += &format!(" · skipped: {}", skipped.join(", "));
    }
    Ok(msg)
}

#[tauri::command]
fn delete_many(dirs: Vec<String>) -> Result<String, String> {
    if dirs.is_empty() {
        return Err("No profile selected.".into());
    }
    if dirs.iter().any(|d| d == SRC) {
        return Err("The Default profile cannot be deleted.".into());
    }
    let ud = ud();
    let live = running_profiles(&ud);
    let ls = ud.join("Local State");
    let mut v = read_json(&ls).ok_or("Local State unreadable")?;
    let mut removed: Vec<String> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    for dir in &dirs {
        let name = v["profile"]["info_cache"]
            .get(dir)
            .and_then(|x| x["name"].as_str())
            .unwrap_or(dir)
            .to_string();
        if live.iter().any(|d| d == dir) {
            skipped.push(format!("{name} (in use)"));
            continue;
        }
        if v["profile"]["info_cache"].get(dir).is_none() {
            skipped.push(format!("{dir} (missing)"));
            continue;
        }
        v["profile"]["info_cache"].as_object_mut().unwrap().remove(dir);
        if v["profile"]["last_used"].as_str() == Some(dir.as_str()) {
            v["profile"]["last_used"] = json!(SRC);
        }
        removed.push(format!("{dir}|{name}"));
    }
    if removed.is_empty() {
        return Ok(format!("Nothing deleted · skipped: {}", skipped.join(", ")));
    }
    let checks: Vec<(String, Option<String>)> = removed
        .iter()
        .map(|item| (item.split_once('|').unwrap_or((item.as_str(), "")).0.to_string(), None))
        .collect();
    commit_ls(&ud, &v, &checks)?;
    job_set("Deleting", 0, removed.len() as u32);
    let mut gone: Vec<String> = Vec::new();
    for (i, item) in removed.iter().enumerate() {
        let (dir, name) = item.split_once('|').unwrap_or((item.as_str(), item.as_str()));
        let target = ud.join(dir);
        let mut last_err = String::new();
        let mut deleted = false;
        for attempt in 0..3 {
            if attempt > 0 {
                std::thread::sleep(Duration::from_millis(250));
            }
            match fs::remove_dir_all(&target) {
                Ok(()) => { deleted = true; break; }
                Err(e) => last_err = e.to_string(),
            }
        }
        if !deleted && target.exists() {
            return Err(format!("Could not delete {dir}: {last_err}"));
        }
        gone.push(name.to_string());
        job_set("Deleting", (i + 1) as u32, removed.len() as u32);
    }
    job_clear();
    let mut msg = format!("Deleted {} profiles: {}", gone.len(),
        if gone.len() > 6 { format!("{} ... {}", gone[0], gone[gone.len() - 1]) } else { gone.join(", ") });
    if !skipped.is_empty() {
        msg += &format!(" · skipped: {}", skipped.join(", "));
    }
    Ok(msg)
}

fn ext_ids(p: &Path) -> Vec<String> {
    fs::read_dir(p)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| e.path().is_dir())
                .filter_map(|e| e.file_name().to_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

fn copy_dir(src: &Path, dst: &Path) -> Result<(), String> {
    fs::create_dir_all(dst).map_err(|e| e.to_string())?;
    for e in fs::read_dir(src).map_err(|e| e.to_string())? {
        let e = e.map_err(|x| x.to_string())?;
        let (p, q) = (e.path(), dst.join(e.file_name()));
        if p.is_dir() {
            copy_dir(&p, &q)?;
        } else {
            fs::copy(&p, &q).map_err(|x| x.to_string())?;
        }
    }
    Ok(())
}

// Ekstensi = file per profile, jadi bisa disalin tanpa policy (tanpa UAC).
// Kalau set ID-nya sudah sama, lewati: menyalin tiap kali itu pemborosan disk.
fn mirror_extensions(ud: &Path, dir: &str) -> Result<u32, String> {
    let (src, dst) = (ud.join(SRC), ud.join(dir));
    let mut copied = 0;
    for sub in ["Extensions", "Extension State", "Local Extension Settings",
                "Extension Rules", "Extension Scripts"] {
        let s = src.join(sub);
        if !s.exists() {
            continue;
        }
        let d = dst.join(sub);
        let (mut a, mut b) = (ext_ids(&s), ext_ids(&d));
        if sub == "Extensions" && !a.is_empty() {
            a.sort();
            b.sort();
            if a == b {
                continue;
            }
        }
        fs::remove_dir_all(&d).ok();
        copy_dir(&s, &d)?;
        copied += 1;
    }
    Ok(copied)
}

#[tauri::command]
fn sync_default() -> Result<String, String> {
    need_closed()?;
    let ud = ud();
    let dirs: Vec<String> = info_cache(&ud)
        .as_object()
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default();
    let (mut pref, mut books, mut ext) = (0, 0, 0);
    job_set("Syncing", 0, dirs.len() as u32);
    for (i, dir) in dirs.iter().enumerate() {
        if dir == SRC || !ud.join(dir).is_dir() {
            job_set("Syncing", (i + 1) as u32, dirs.len() as u32);
            continue;
        }
        for f in ["Preferences", "Secure Preferences"] {
            let mut data = match read_json(&ud.join(SRC).join(f)) {
                Some(d) => d,
                None => continue,
            };
            if let Some(o) = data.as_object_mut() {
                for k in IDENTITY {
                    o.remove(k);
                }
            }
            write_json(&ud.join(dir).join(f), &data)?;
            pref += 1;
        }
        for f in ["Bookmarks", "Bookmarks.bak"] {
            let s = ud.join(SRC).join(f);
            if !s.exists() {
                continue;
            }
            let d = ud.join(dir).join(f);
            if d.exists() {
                let _ = fs::rename(&d, PathBuf::from(format!("{}.bak-{}", d.display(), ts())));
                prune_backups(&d);
            }
            fs::copy(&s, &d).map_err(|e| e.to_string())?;
            books += 1;
        }
        ext += mirror_extensions(&ud, dir)?;
        job_set("Syncing", (i + 1) as u32, dirs.len() as u32);
    }
    job_clear();
    Ok(format!("Synced {} profiles · {pref} preference files, {books} bookmark files, {ext} extension folders",
        dirs.len().saturating_sub(1)))
}

fn reg_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

#[tauri::command]
fn apply_policy() -> Result<String, String> {
    let ud = ud();
    let prefs = read_json(&ud.join(SRC).join("Preferences")).unwrap_or_else(|| json!({}));
    let secure = read_json(&ud.join(SRC).join("Secure Preferences")).unwrap_or_else(|| json!({}));
    let mut reg = String::from("Windows Registry Editor Version 5.00\r\n\r\n");
    reg += &format!("[{POLICY}]\r\n");
    let newtab = prefs.get("homepage_is_newtabpage").and_then(|v| v.as_bool()).unwrap_or(true);
    reg += &format!("\"HomepageIsNewTabPage\"=dword:0000000{}\r\n", if newtab { 1 } else { 0 });
    reg += "\"BraveRewardsDisabled\"=dword:00000001\r\n";
    reg += "\"BraveNewsDisabled\"=dword:00000001\r\n";
    let home = prefs.get("homepage").and_then(|v| v.as_str()).unwrap_or("");
    if !home.is_empty() {
        reg += &format!("\"HomepageLocation\"=\"{}\"\r\n", reg_escape(home));
        reg += "\"ShowHomeButton\"=dword:00000001\r\n";
    }
    let exts: Vec<String> = secure
        .pointer("/extensions/settings")
        .and_then(|v| v.as_object())
        .map(|m| {
            m.iter()
                .filter(|(_, v)| {
                    v.get("from_webstore").and_then(|x| x.as_bool()).unwrap_or(false)
                        && matches!(v.get("location").and_then(|x| x.as_i64()), Some(1) | Some(7))
                })
                .map(|(k, _)| k.clone())
                .collect()
        })
        .unwrap_or_default();
    if !exts.is_empty() {
        reg += &format!("\r\n[{POLICY}\\ExtensionInstallForcelist]\r\n");
        for (i, id) in exts.iter().enumerate() {
            reg += &format!("\"{}\"=\"{id};{CRX}\"\r\n", i + 1);
        }
    }
    let tmp = std::env::temp_dir().join(format!("bpm-policy-{}.reg", ts()));
    fs::write(&tmp, reg).map_err(|e| e.to_string())?;
    let ps = format!(
        "Start-Process -FilePath regedit -ArgumentList '/s','{}' -Verb RunAs -Wait",
        tmp.display()
    );
    let (ok, out) = run("powershell", &["-NoProfile", "-Command", &ps]);
    let _ = fs::remove_file(&tmp);
    if !ok {
        return Err(format!("Policy write failed (admin/UAC required): {out}"));
    }
    Ok(format!("Policy written · {} extensions · homepage: {}",
        exts.len(), if home.is_empty() { "not set in Default" } else { home }))
}

// Tutup semua jendela Brave dengan WM_CLOSE (seperti klik X). taskkill tanpa /F diabaikan
// Chromium, jadi /F hanya dipakai sebagai fallback.
#[cfg(windows)]
mod winmsg {
    pub type Hwnd = isize;
    #[link(name = "user32")]
    extern "system" {
        pub fn EnumWindows(cb: extern "system" fn(Hwnd, isize) -> i32, lparam: isize) -> i32;
        pub fn GetWindowThreadProcessId(hwnd: Hwnd, pid: *mut u32) -> u32;
        pub fn PostMessageW(hwnd: Hwnd, msg: u32, wparam: usize, lparam: isize) -> i32;
        pub fn IsWindowVisible(hwnd: Hwnd) -> i32;
    }
    pub const WM_CLOSE: u32 = 0x0010;

    pub fn close_windows_of(pids: &[u32]) {
        struct Ctx<'a> {
            pids: &'a [u32],
        }
        extern "system" fn cb(hwnd: Hwnd, lparam: isize) -> i32 {
            let ctx = unsafe { &*(lparam as *const Ctx) };
            let mut pid: u32 = 0;
            unsafe {
                GetWindowThreadProcessId(hwnd, &mut pid);
            }
            if ctx.pids.contains(&pid) && unsafe { IsWindowVisible(hwnd) } != 0 {
                unsafe {
                    PostMessageW(hwnd, WM_CLOSE, 0, 0);
                }
            }
            1
        }
        let ctx = Ctx { pids };
        unsafe {
            EnumWindows(cb, &ctx as *const Ctx as isize);
        }
    }
}

fn brave_pids() -> Vec<u32> {
    let (_, out) = run("tasklist", &["/FI", "IMAGENAME eq brave.exe", "/FO", "CSV", "/NH"]);
    out.lines()
        .filter(|l| l.to_lowercase().contains("brave.exe"))
        .filter_map(|l| l.split(',').nth(1).and_then(|s| s.trim_matches('"').parse::<u32>().ok()))
        .collect()
}

#[tauri::command]
fn stop_all() -> Result<String, String> {
    if !brave_open() {
        return Ok("Brave was not running.".into());
    }
    #[cfg(windows)]
    {
        for _ in 0..4 {
            let pids = brave_pids();
            if pids.is_empty() {
                break;
            }
            winmsg::close_windows_of(&pids);
            for _ in 0..10 {
                std::thread::sleep(Duration::from_millis(400));
                if !brave_open() {
                    return Ok("All Brave windows closed.".into());
                }
            }
        }
    }
    run("taskkill", &["/IM", "brave.exe", "/F"]);
    std::thread::sleep(Duration::from_millis(2000));
    if brave_open() {
        Err("Brave is still running; close it manually.".into())
    } else {
        Ok("Brave force-closed (it ignored the close request).".into())
    }
}

#[tauri::command]
fn start_profile(dir: String) -> Result<String, String> {
    let ud = ud();
    if !ud.join(&dir).is_dir() {
        return Err(format!("{dir} has no folder yet."));
    }
    let exe = std::env::var("BPM_BRAVE_EXE").unwrap_or_else(|_| {
        r"C:\Program Files\BraveSoftware\Brave-Browser\Application\brave.exe".to_string()
    });
    if !Path::new(&exe).exists() {
        return Err(format!("brave.exe not found at {exe}"));
    }
    let mut c = Command::new(&exe);
    c.arg(format!("--profile-directory={dir}"));
    #[cfg(windows)]
    c.creation_flags(NO_WINDOW);
    c.spawn().map_err(|e| format!("Could not open Brave: {e}"))?;
    let name = info_cache(&ud)
        .get(&dir)
        .and_then(|v| v.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or(&dir)
        .to_string();
    Ok(format!("Opened {name}"))
}

// Tutup hanya jendela Brave milik satu profile (WM_CLOSE), bukan seluruh Brave.
#[tauri::command]
fn stop_profile(dir: String) -> Result<String, String> {
    let ud = ud();
    let live = running_profiles(&ud);
    if !live.iter().any(|d| d == &dir) {
        return Ok(format!("{dir} is not running."));
    }
    // Semua jendela Brave hidup di satu proses; menutup satu profile hanya mungkin
    // kalau dia satu-satunya yang jalan.
    if live.len() > 1 {
        return Err(format!("{} profiles share one Brave process. Use Stop all.", live.len()));
    }
    #[cfg(windows)]
    {
        winmsg::close_windows_of(&brave_pids());
        for _ in 0..10 {
            std::thread::sleep(Duration::from_millis(300));
            if !profile_in_use(&ud, &dir) {
                return Ok(format!("Stopped {dir}"));
            }
        }
    }
    Err("Brave did not close; use Stop all.".into())
}

// Self-check: logika tulis dijalankan terhadap User Data palsu di temp, data asli tak disentuh.
fn selftest() -> Result<String, String> {
    let root = std::env::temp_dir().join(format!("bpm-selftest-{}", ts()));
    fs::create_dir_all(root.join("Default")).map_err(|e| e.to_string())?;
    fs::write(
        root.join("Local State"),
        r#"{"profile":{"last_used":"Default","info_cache":{"Default":{"name":"Def","active_time":1.0,"gaia_id":"","user_name":"","is_using_default_name":false}}}}"#,
    )
    .map_err(|e| e.to_string())?;
    fs::write(root.join("Default").join("Preferences"),
        r#"{"download":{"prompt_for_download":true},"signin":{"allowed":true},"sync":{"y":2},"custom_links":{"list":[{"title":"A"}]}}"#)
        .map_err(|e| e.to_string())?;
    fs::write(root.join("Default").join("Secure Preferences"), r#"{"protection":{"macs":{}}}"#).map_err(|e| e.to_string())?;
    fs::write(root.join("Default").join("Bookmarks"),
        r#"{"roots":{"bookmark_bar":{"children":[]},"other":{"children":[]}}}"#).map_err(|e| e.to_string())?;
    std::env::set_var("BPM_USER_DATA", &root);
    std::env::set_var("BPM_IGNORE_BRAVE", "1");

    let mut out = String::new();
    fn chk(out: &mut String, label: &str, got: Result<String, String>, want: &[&str]) {
        match got {
            Ok(m) => {
                let ok = want.iter().all(|w| m.contains(w));
                out.push_str(&format!("[{}] {label}: {m}\n", if ok { "PASS" } else { "FAIL" }));
            }
            Err(e) => {
                let ok = want.iter().any(|w| e.contains(w));
                out.push_str(&format!("[{}] {label}: {e}\n", if ok { "PASS" } else { "FAIL" }));
            }
        }
    }
    chk(&mut out, "bulk create HM01..HM03", create_bulk("HM".into(), 1, 2, 3), &["Created 3 profiles", "HM01", "HM03"]);
    let ic = info_cache(&root);
    let three = ["Profile 1", "Profile 2", "Profile 3"].iter().all(|d| ic.get(d).is_some());
    out.push_str(&format!("[{}] 3 profile folders registered\n", if three { "PASS" } else { "FAIL" }));
    chk(&mut out, "rename one with prefix", rename_selected(vec!["Profile 1".into()], "HM".into(), "{n}".into(), 9, 1), &["Renamed 1 profiles"]);
    let ic = info_cache(&root);
    let got = ic["Profile 1"]["name"].as_str().unwrap_or("").to_string();
    out.push_str(&format!("[{}] Profile 1 is now HM9 (got {got})\n", if got == "HM9" { "PASS" } else { "FAIL" }));
    chk(&mut out, "bulk rename numbering", rename_selected(vec!["Profile 2".into(), "Profile 3".into()], "HM".into(), "X{n}".into(), 10, 5), &["Renamed 2 profiles"]);
    let ic = info_cache(&root);
    let (a, b) = (ic["Profile 2"]["name"].as_str().unwrap_or(""), ic["Profile 3"]["name"].as_str().unwrap_or(""));
    out.push_str(&format!("[{}] numbering stepped by 5 ({a}, {b})\n",
        if a == "HMX10" && b == "HMX15" { "PASS" } else { "FAIL" }));
    chk(&mut out, "no prefix and no {n} must fail", rename_selected(vec!["Profile 1".into()], "".into(), "".into(), 1, 1), &["must contain"]);
    let baks = fs::read_dir(&root).map(|rd| rd.filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("Local State.bak-")).count()).unwrap_or(0);
    out.push_str(&format!("[{}] Local State backups pruned to one (found {baks})\n", if baks <= 1 { "PASS" } else { "FAIL" }));

    // Akun Google: dibaca dari cookie, bukan user_name
    let nets = root.join("Profile 1").join("Network");
    fs::create_dir_all(&nets).unwrap();
    fs::write(nets.join("Cookies"), b"xxLSIDyy__Host-GAPSzz").unwrap();
    let signed = google_level(&root, "Profile 1");
    out.push_str(&format!("[{}] signed-in detected from cookies (level {signed})\n", if signed == 2 { "PASS" } else { "FAIL" }));
    let none = google_level(&root, "Profile 2");
    out.push_str(&format!("[{}] profile without cookies has no account (level {none})\n", if none == 0 { "PASS" } else { "FAIL" }));

    for d in ["Default", "Profile 1", "Profile 2", "Profile 3"] {
        fs::create_dir_all(root.join(d).join("Extensions").join("aaa")).unwrap();
        fs::write(root.join(d).join("Extensions").join("aaa").join("manifest.json"), "{}").unwrap();
    }
    chk(&mut out, "sync mirror", sync_default(), &["Synced 3 profiles", "6 preference files", "0 extension folders"]);
    fs::remove_dir_all(root.join("Profile 3").join("Extensions")).unwrap();
    chk(&mut out, "new extension set is copied", sync_default(), &["1 extension folders"]);
    let mirrored = fs::read_to_string(root.join("Profile 1").join("Preferences")).unwrap_or_default();
    let ok = !mirrored.contains("\"signin\"") && !mirrored.contains("\"sync\"") && mirrored.contains("custom_links");
    out.push_str(&format!("[{}] identity keys stripped, custom_links kept\n", if ok { "PASS" } else { "FAIL" }));
    chk(&mut out, "multi delete", delete_many(vec!["Profile 2".into(), "Profile 3".into()]), &["Deleted 2 profiles"]);
    chk(&mut out, "delete Default refused", delete_many(vec![SRC.into()]), &["cannot be deleted"]);

    // start_profile: dibuktikan dengan exe apa pun yang ada (cmd.exe), bukan Brave asli
    std::env::set_var("BPM_BRAVE_EXE", r"C:\Windows\System32\cmd.exe");
    chk(&mut out, "start_profile spawns the browser", start_profile("Profile 1".into()), &["Opened"]);
    std::env::remove_var("BPM_BRAVE_EXE");

    std::env::remove_var("BPM_IGNORE_BRAVE");
    chk(&mut out, "create while Brave is running", create_bulk("LIVE".into(), 1, 2, 2), &["Created 2 profiles", "LIVE01"]);
    let ic = info_cache(&root);
    let live_dirs: Vec<String> = ic.as_object().map(|m| m.keys().cloned().collect()).unwrap_or_default();
    let fresh = live_dirs.iter().find(|d| ic[d]["name"].as_str() == Some("LIVE01")).cloned().unwrap_or_default();
    chk(&mut out, "delete a never-opened profile while Brave runs", delete_many(vec![fresh]), &["Deleted 1 profiles"]);
    let _ = fs::remove_dir_all(&root);
    Ok(out)
}

fn diag() {
    let mut out = String::new();
    let ud = ud();
    out += &format!("user_data={}\n", ud.display());
    out += &format!("brave_open={}\n", brave_open());
    let (ok, text) = run("tasklist", &["/FI", "IMAGENAME eq brave.exe"]);
    out += &format!("tasklist ok={} len={} head={:?}\n", ok, text.len(), text.chars().take(120).collect::<String>());
    out += &format!("running={:?}\n", running_profiles(&ud));
    out += &format!("state_keys={}\n", info_cache(&ud).as_object().map(|m| m.len()).unwrap_or(0));
    let r = start_profile("Profile 7".to_string());
    out += &format!("start_profile={:?}\n", r);
    std::thread::sleep(Duration::from_millis(2500));
    out += &format!("after_start brave_open={} running={:?}\n", brave_open(), running_profiles(&ud));
    let _ = fs::write(std::env::temp_dir().join("bpm-diag.log"), out);
}

fn main() {
    if std::env::args().any(|a| a == "--diag") {
        diag();
        return;
    }
    if std::env::args().any(|a| a == "--selftest") {
        let log = std::env::temp_dir().join("bpm-selftest.log");
        let text = match selftest() {
            Ok(t) => t,
            Err(e) => format!("[FAIL] setup: {e}\n"),
        };
        let _ = fs::write(&log, &text);
        println!("{text}");
        return;
    }
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            get_state, create_bulk, rename_selected, delete_many, sync_default, apply_policy, stop_all,
            start_profile, stop_profile
        ])
        .run(tauri::generate_context!())
        .expect("failed to start app");
}