// Copyright 2026 SuperFedora Contributors
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};
use std::{collections::HashSet, env, fs, os::unix::{fs::PermissionsExt, process::CommandExt}, path::{Path, PathBuf}, process::{Command, Stdio}, time::{SystemTime, UNIX_EPOCH}};

const USER_JS: &str = "user_pref(\"toolkit.legacyUserProfileCustomizations.stylesheets\", true);\n\
user_pref(\"browser.tabs.inTitlebar\", 1);\nuser_pref(\"browser.tabs.warnOnClose\", false);\n\
user_pref(\"browser.tabs.closeWindowWithLastTab\", true);\nuser_pref(\"browser.shell.checkDefaultBrowser\", false);\n\
user_pref(\"browser.startup.homepage_override.mstone\", \"ignore\");\nuser_pref(\"browser.aboutwelcome.enabled\", false);\n\
user_pref(\"trailhead.firstrun.didSeeAboutWelcome\", true);\nuser_pref(\"datareporting.policy.dataSubmissionPolicyBypassNotification\", true);\n\
user_pref(\"browser.sessionstore.resume_from_crash\", false);\n";

const USER_CHROME: &str = "@namespace url(\"http://www.mozilla.org/keymaster/gatekeeper/there.is.only.xul\");\n\
#navigator-toolbox, #TabsToolbar, #nav-bar, #PersonalToolbar, #titlebar, .titlebar-buttonbox-container, #sidebar-box, #sidebar-splitter {\n\
    visibility: collapse !important; display: none !important; max-height: 0 !important; min-height: 0 !important;\n}\n\
#browser, #appcontent, #tabbrowser-tabbox, #tabbrowser-tabpanels { margin: 0 !important; padding: 0 !important; border: 0 !important; }\n";

const UA: &str = "Mozilla/5.0 (X11; Fedora; Linux x86_64; rv:140.0) Gecko/20100101 Firefox/140.0";

fn resolve_url(base: &str, href: &str) -> String {
    let h = href.trim();
    if h.starts_with("http://") || h.starts_with("https://") { return h.to_string(); }
    let (scheme, rest) = base.split_once("://").unwrap_or(("https", base));
    let host_part = rest.split('/').next().unwrap_or("localhost");
    if let Some(stripped) = h.strip_prefix("//") { return format!("{scheme}://{stripped}"); }
    if h.starts_with('/') { return format!("{scheme}://{host_part}{h}"); }
    match rest.rfind('/') { Some(idx) => format!("{scheme}://{}{h}", &rest[..=idx]), None => format!("{scheme}://{host_part}/{h}") }
}

fn attr_val(tag: &str, key: &str) -> Option<String> {
    let pos = tag.to_ascii_lowercase().find(&format!("{key}="))? + key.len() + 1;
    let after = tag[pos..].trim_start();
    if let Some(q) = after.chars().next() && (q == '"' || q == '\'') { return Some(after[1..1 + after[1..].find(q)?].to_string()); }
    Some(after.split_whitespace().next()?.trim_end_matches('>').to_string())
}

fn clean_title(raw: &str, fallback: &str) -> String {
    let mut head = raw.trim();
    for sep in [" - ", " | ", " — ", " · ", " : "] { if let Some((left, _)) = head.split_once(sep) { head = left.trim(); } }
    let mut dedup: Vec<&str> = Vec::new();
    for w in head.split_whitespace() { if dedup.last().map(|p| !p.eq_ignore_ascii_case(w)).unwrap_or(true) { dedup.push(w); } }
    let joined = dedup.join(" ");
    let low = joined.to_ascii_lowercase();
    let chosen = if joined.is_empty() || matches!(low.as_str(), "index" | "home" | "welcome" | "loading" | "login" | "sign in" | "web") { fallback.to_string() } else { joined };
    if chosen.chars().all(|c| !c.is_alphabetic() || c.is_lowercase()) {
        chosen.split_whitespace().map(|w| { let mut ch = w.chars(); ch.next().map(|f| f.to_uppercase().collect::<String>() + ch.as_str()).unwrap_or_default() }).collect::<Vec<_>>().join(" ")
    } else { chosen }
}

fn parse_html_meta(base: &str, html: &str) -> (String, String, Vec<(u32, String)>) {
    let lower = html.to_ascii_lowercase();
    let mut title = String::new();
    if let Some(s) = lower.find("<title") && let Some(gt) = lower[s..].find('>') && let Some(e) = lower[s + gt + 1..].find("</title>") {
        title = html[s + gt + 1..s + gt + 1 + e].trim().to_string();
    }
    let (mut meta_t, mut mf_url, mut icons) = (String::new(), String::new(), Vec::new());
    for chunk in html.split('<').skip(1) {
        let Some((tag, _)) = chunk.split_once('>') else { continue };
        let tl = tag.to_ascii_lowercase();
        if tl.starts_with("meta ") && meta_t.is_empty() {
            let k = attr_val(tag, "property").or_else(|| attr_val(tag, "name")).unwrap_or_default().to_ascii_lowercase();
            let v = attr_val(tag, "content").unwrap_or_default();
            if matches!(k.as_str(), "application-name" | "apple-mobile-web-app-title" | "og:site_name" | "og:title") && !v.trim().is_empty() { meta_t = v.trim().to_string(); }
        } else if tl.starts_with("link ") && let Some(href) = attr_val(tag, "href") {
            let rel = attr_val(tag, "rel").unwrap_or_default().to_ascii_lowercase();
            let abs = resolve_url(base, &href);
            if rel.contains("manifest") && mf_url.is_empty() { mf_url = abs; }
            else if rel.contains("apple-touch-icon") || rel.contains("icon") || rel.contains("fluid-icon") {
                let sz: u32 = attr_val(tag, "sizes").unwrap_or_default().split(|c: char| !c.is_ascii_digit()).find(|s| !s.is_empty()).and_then(|s| s.parse().ok()).unwrap_or(0);
                icons.push((sz + if rel.contains("apple") { 100 } else { 0 }, abs));
            }
        }
    }
    (if meta_t.is_empty() { title } else { meta_t }, mf_url, icons)
}

fn write_fallback_png(path: &Path, sz: u32) {
    let mut raw = Vec::with_capacity((sz * (sz * 4 + 1)) as usize);
    for y in 0..sz {
        raw.push(0);
        for x in 0..sz {
            let (nx, ny) = ((x * 256) / sz.max(1), (y * 256) / sz.max(1));
            let c = if (8..=247).contains(&nx) && (8..=247).contains(&ny) && (!(16..=239).contains(&nx) || !(16..=239).contains(&ny)) { [122, 162, 247, 255] }
            else if (22..=233).contains(&nx) && (22..=233).contains(&ny) && (!(26..=229).contains(&nx) || !(26..=229).contains(&ny)) { [42, 195, 222, 255] }
            else if (64..=191).contains(&nx) && (64..=191).contains(&ny) { if !(68..=187).contains(&nx) || !(68..=187).contains(&ny) { [122, 162, 247, 255] } else { [36, 40, 59, 255] } }
            else { [26, 27, 38, 255] };
            raw.extend_from_slice(&c);
        }
    }
    let crc32 = |data: &[u8]| -> u32 {
        let mut crc = 0xFFFF_FFFFu32;
        for &b in data { crc ^= u32::from(b); for _ in 0..8 { crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 }; } }
        !crc
    };
    let push_chunk = |out: &mut Vec<u8>, tag: &[u8; 4], body: &[u8]| {
        out.extend_from_slice(&(body.len() as u32).to_be_bytes());
        let st = out.len(); out.extend_from_slice(tag); out.extend_from_slice(body);
        out.extend_from_slice(&crc32(&out[st..]).to_be_bytes());
    };
    let mut out = vec![137, 80, 78, 71, 13, 10, 26, 10];
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&sz.to_be_bytes()); ihdr.extend_from_slice(&sz.to_be_bytes()); ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    push_chunk(&mut out, b"IHDR", &ihdr);
    let mut z = vec![0x78, 0x01];
    let chunks: Vec<&[u8]> = raw.chunks(65535).collect();
    for (i, ch) in chunks.iter().enumerate() {
        z.push(u8::from(i + 1 == chunks.len()));
        let len = ch.len() as u16;
        z.extend_from_slice(&len.to_le_bytes()); z.extend_from_slice(&(!len).to_le_bytes()); z.extend_from_slice(ch);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in &raw { a = (a + u32::from(byte)) % 65521; b = (b + a) % 65521; }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());
    push_chunk(&mut out, b"IDAT", &z); push_chunk(&mut out, b"IEND", &[]);
    let _ = fs::write(path, out);
}

fn set_live(ljson: &Path, active: bool, state: &str, target: &str, pct: u32, step: &str) {
    let ts = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let _ = fs::write(ljson, json!({"active": active, "state": state, "target": target, "pct": pct, "step": step, "ts": ts}).to_string());
    let _ = Command::new("qs").args(["ipc", "call", "status", "reloadForge"]).stdout(Stdio::null()).stderr(Stdio::null()).status();
}

pub fn run_webapp_forge(args: &[String]) -> i32 {
    if args.len() < 7 { return 2; }
    let (mut url, mut cname) = (args[0].clone(), args[1].clone());
    if url.contains('|') && cname.is_empty() && let Some((u, c)) = url.clone().split_once('|') { url = u.trim().to_string(); cname = c.trim().to_string(); }
    let (mode, wdir, bdir, fjson, ljson) = (&args[2], Path::new(&args[3]), Path::new(&args[4]), Path::new(&args[5]), Path::new(&args[6]));
    let (scheme_s, rest_s) = url.split_once("://").unwrap_or(("https", url.as_str()));
    let (scheme, netloc) = (scheme_s.to_string(), rest_s.split('/').next().unwrap_or("webapp").to_string());
    let host = netloc.split(':').next().unwrap_or("webapp").to_ascii_lowercase().strip_prefix("www.").unwrap_or(netloc.split(':').next().unwrap_or("webapp")).to_string();
    let host_first = host.split('.').next().unwrap_or("webapp");
    let host_label = { let mut ch = host_first.chars(); ch.next().map(|f| f.to_uppercase().collect::<String>() + ch.as_str()).unwrap_or_else(|| "WebApp".into()) };
    let slug_src = if !cname.is_empty() { cname.to_ascii_lowercase() } else if host.matches('.').count() == 1 { host_first.to_string() } else { host.replace('.', "-") };
    let slug = slug_src.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect::<String>().split('-').filter(|s| !s.is_empty()).collect::<Vec<_>>().join("-");
    let slug = if slug.is_empty() { "webapp".to_string() } else { slug };
    let aid = format!("sf-webapp-{slug}");

    let mut html = String::new();
    if let Some(o) = Command::new("curl").args(["-kfsSL", "-m", "6", "-A", UA, "-w", "\n__FINAL_URL__:%{url_effective}", &url]).output().ok() {
        let txt = String::from_utf8_lossy(&o.stdout);
        if let Some((body, eff)) = txt.rsplit_once("\n__FINAL_URL__:") {
            if !eff.trim().is_empty() { url = eff.trim().to_string(); }
            html = body.to_string();
        } else { html = txt.to_string(); }
    }
    let (mut meta_t, mf_url, mut icons) = parse_html_meta(&url, &html);
    if !mf_url.is_empty() && let Some(o) = Command::new("curl").args(["-kfsSL", "-m", "4", "-A", UA, &mf_url]).output().ok() && let Ok(mf) = serde_json::from_slice::<Value>(&o.stdout) {
        if cname.is_empty() && let Some(n) = mf.get("short_name").or_else(|| mf.get("name")).and_then(|v| v.as_str()) && !n.trim().is_empty() { meta_t = n.trim().to_string(); }
        if let Some(arr) = mf.get("icons").and_then(|v| v.as_array()) {
            for ic in arr {
                if let Some(src) = ic.get("src").and_then(|v| v.as_str()) {
                    let sz: u32 = ic.get("sizes").and_then(|v| v.as_str()).unwrap_or("0").split(|c: char| !c.is_ascii_digit()).find(|s| !s.is_empty()).and_then(|s| s.parse().ok()).unwrap_or(0);
                    icons.push((sz + 150, resolve_url(&mf_url, src)));
                }
            }
        }
    }
    let title = clean_title(if !cname.trim().is_empty() { cname.trim() } else { &meta_t }, &host_label);
    set_live(ljson, true, "running", &title, 45, &format!("[2/4] Installing hicolor app icons (16px..512px) for {title}..."));
    icons.sort_by(|a, b| b.0.cmp(&a.0));
    let mut cands: Vec<String> = icons.into_iter().map(|(_, u)| u).collect();
    cands.extend([resolve_url(&url, "images/icon-512x512.png"), resolve_url(&url, "images/icon-192x192.png"), format!("{scheme}://{netloc}/apple-touch-icon.png"), format!("https://www.google.com/s2/favicons?sz=256&domain={host}"), format!("https://icons.duckduckgo.com/ip3/{host}.ico"), format!("{scheme}://{netloc}/favicon.ico")]);
    let home = env::var("HOME").unwrap_or_else(|_| "/home/jeryd".into());
    let hicolor = PathBuf::from(&home).join(".local/share/icons/hicolor");
    let tmp_icon = env::temp_dir().join(format!("{aid}-raw.img"));
    let (mut got_icon, mut seen) = (false, HashSet::new());
    for iu in cands {
        if seen.insert(iu.clone()) && Command::new("curl").args(["-kfsSL", "-m", "5", "-A", UA, "-o"]).arg(&tmp_icon).arg(&iu).stdout(Stdio::null()).stderr(Stdio::null()).status().map(|s| s.success()).unwrap_or(false) && Command::new("magick").arg(&tmp_icon).args(["-resize", "256x256", "PNG32:-"]).stdout(Stdio::null()).stderr(Stdio::null()).status().map(|s| s.success()).unwrap_or(false) {
            got_icon = true; break;
        }
    }
    for sz in [16u32, 32, 48, 64, 128, 256, 512] {
        let dd = hicolor.join(format!("{sz}x{sz}/apps")); let _ = fs::create_dir_all(&dd); let dst = dd.join(format!("{aid}.png"));
        if got_icon { let _ = Command::new("magick").arg(&tmp_icon).args(["-resize", &format!("{sz}x{sz}")]).arg(&dst).stdout(Stdio::null()).stderr(Stdio::null()).status(); }
        if !dst.is_file() { write_fallback_png(&dst, sz); }
    }
    let _ = fs::remove_file(&tmp_icon);

    set_live(ljson, true, "running", &title, 75, &format!("[3/4] Configuring chromeless Firefox profile ({mode}) for {title}..."));
    let pdir = wdir.join(&slug).join("profile"); let _ = fs::create_dir_all(pdir.join("chrome"));
    let _ = fs::write(pdir.join("user.js"), USER_JS); let _ = fs::write(pdir.join("chrome/userChrome.css"), USER_CHROME);
    let bpath = bdir.join(&aid); let _ = fs::create_dir_all(bdir);
    let _ = fs::write(&bpath, format!("#!/usr/bin/env bash\n# Copyright 2026 SuperFedora Contributors\n# SPDX-License-Identifier: Apache-2.0\nexec superforge webapp-exec '{}' '{}' '{}' '{}' \"$@\"\n", pdir.display(), url, mode, aid));
    let _ = fs::set_permissions(&bpath, fs::Permissions::from_mode(0o755));

    let icon_p = hicolor.join(format!("256x256/apps/{aid}.png"));
    let desk_d = PathBuf::from(&home).join(".local/share/applications"); let _ = fs::create_dir_all(&desk_d);
    let desk_p = desk_d.join(format!("{aid}.desktop"));
    let _ = fs::write(&desk_p, format!("[Desktop Entry]\nVersion=1.0\nType=Application\nName={title}\nGenericName=Web Application\nComment={url} (Firefox {mode} profile)\nExec={} %u\nIcon={}\nTerminal=false\nCategories=Network;WebBrowser;\nStartupWMClass={aid}\nStartupNotify=true\n", bpath.display(), icon_p.display()));

    let mut d: Value = fs::read_to_string(fjson).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_else(|| json!({}));
    if !d.is_object() { d = json!({}); }
    if d.get("targets").and_then(|t| t.as_object()).is_none() { d["targets"] = json!({}); }
    let ts = Command::new("date").args(["-u", "+%FT%TZ"]).output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    d["targets"][&slug] = json!({"type": "webapp", "app_id": aid, "name": title, "url": url, "profile": mode, "binary": bpath.display().to_string(), "icon": icon_p.display().to_string(), "desktop": desk_p.display().to_string(), "built_at": ts});
    if let Ok(pretty) = serde_json::to_string_pretty(&d) { let _ = fs::write(fjson, pretty); }
    println!("{aid}|{title}");
    0
}

pub fn run_webapp_exec(args: &[String]) -> i32 {
    if args.len() < 4 { return 2; }
    let (pdir, url, mode, aid) = (Path::new(&args[0]), &args[1], &args[2], &args[3]);
    let target_url = args.get(4).filter(|s| !s.is_empty()).unwrap_or(url);
    let _ = fs::create_dir_all(pdir.join("chrome"));
    if Command::new("pgrep").args(["-f", &format!("firefox.*--profile {}", pdir.display())]).stdout(Stdio::null()).status().map(|s| s.success()).unwrap_or(false) {
        let _ = Command::new("hyprctl").args(["dispatch", "focuswindow", &format!("class:^({aid})$")]).stdout(Stdio::null()).stderr(Stdio::null()).status();
        if args.len() <= 4 { return 0; }
    }
    if mode == "shared" {
        let home = env::var("HOME").unwrap_or_else(|_| "/home/jeryd".into());
        for base in [PathBuf::from(&home).join(".config/mozilla/firefox"), PathBuf::from(&home).join(".mozilla/firefox")] {
            let Ok(txt) = fs::read_to_string(base.join("profiles.ini")) else { continue };
            let (mut rel_inst, mut rel_def, mut cur_sec, mut sec_p, mut sec_d) = (None, None, String::new(), String::new(), false);
            for line in txt.lines().chain(["[END]"]) {
                let l = line.trim();
                if l.starts_with('[') && l.ends_with(']') {
                    if !cur_sec.starts_with("Install") && sec_d && !sec_p.is_empty() && rel_def.is_none() { rel_def = Some(sec_p.clone()); }
                    cur_sec = l[1..l.len() - 1].to_string(); sec_p.clear(); sec_d = false;
                } else if let Some((k, v)) = l.split_once('=') {
                    let (k, v) = (k.trim(), v.trim());
                    if cur_sec.starts_with("Install") && k == "Default" && !v.is_empty() && rel_inst.is_none() { rel_inst = Some(v.to_string()); }
                    else if k == "Path" { sec_p = v.to_string(); } else if k == "Default" && v == "1" { sec_d = true; }
                }
            }
            let Some(rel) = rel_inst.or(rel_def) else { continue };
            let main = if Path::new(&rel).is_absolute() { PathBuf::from(rel) } else { base.join(rel) };
            if !main.is_dir() { continue; }
            for lnk in ["cert9.db", "cert_override.txt", "pkcs11.txt", "key4.db", "logins.json", "logins.db", "signedInUser.json"] {
                let (s, d) = (main.join(lnk), pdir.join(lnk));
                if s.exists() && !d.exists() && std::os::unix::fs::symlink(&s, &d).is_err() { let _ = fs::copy(&s, &d); }
            }
            for db in ["permissions.sqlite", "content-prefs.sqlite", "webappsstore.sqlite", "webappsstore.sqlite-wal", "webappsstore.sqlite-shm", "cookies.sqlite", "cookies.sqlite-wal", "cookies.sqlite-shm"] {
                let (s, d) = (main.join(db), pdir.join(db));
                if s.is_file() && !d.is_file() { let _ = fs::copy(&s, &d); }
            }
            let (s_def, d_def) = (main.join("storage/default"), pdir.join("storage/default"));
            if s_def.is_dir() && fs::create_dir_all(&d_def).is_ok() && let Ok(entries) = fs::read_dir(&s_def) {
                for e in entries.flatten() {
                    let (se, de) = (e.path(), d_def.join(e.file_name()));
                    if se.is_dir() && !de.exists() { let _ = std::os::unix::fs::symlink(&se, &de); }
                }
            }
            break;
        }
    }
    let _ = Command::new("firefox").env("MOZ_ENABLE_WAYLAND", "1").args(["--no-remote", "--new-instance", "--profile"]).arg(pdir).args(["--name", aid, "--class", aid, target_url]).exec();
    1
}
