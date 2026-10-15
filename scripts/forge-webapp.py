#!/usr/bin/env python3
# Copyright 2026 SuperFedora Contributors
# SPDX-License-Identifier: Apache-2.0
"""scripts/forge-webapp.py — Forge chromeless Firefox WebApps (shared or isolated profile, hicolor icons, .desktop)."""
import datetime, html.parser, io, json, os, re, shutil, ssl, subprocess, sys, time, urllib.parse, urllib.request
from PIL import Image, ImageDraw

LAUNCHER_TPL = '''#!/usr/bin/env python3
# Copyright 2026 SuperFedora Contributors
# SPDX-License-Identifier: Apache-2.0
import configparser, os, shutil, sqlite3, subprocess, sys, tempfile

pdir, url, mode, aid = {pdir!r}, {url!r}, {mode!r}, {aid!r}
target_url = sys.argv[1] if len(sys.argv) > 1 else url
os.makedirs(os.path.join(pdir, "chrome"), exist_ok=True)
if subprocess.run(["pgrep", "-f", f"firefox.*--profile {{pdir}}"], stdout=subprocess.DEVNULL).returncode == 0:
    subprocess.run(["hyprctl", "dispatch", "focuswindow", f"class:^({{aid}})$"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    if len(sys.argv) <= 1:
        sys.exit(0)
if mode == "shared":
    for base in (os.path.expanduser("~/.config/mozilla/firefox"), os.path.expanduser("~/.mozilla/firefox")):
        ini = os.path.join(base, "profiles.ini")
        if not os.path.isfile(ini):
            continue
        cp = configparser.ConfigParser()
        cp.read(ini)
        rel = next((cp.get(s, "Default") for s in cp.sections() if s.startswith("Install") and cp.has_option(s, "Default")), None)
        if not rel:
            rel = next((cp.get(s, "Path") for s in cp.sections() if cp.get(s, "Default", fallback="") == "1"), None)
        main = (rel if os.path.isabs(rel) else os.path.join(base, rel)) if rel else ""
        if main and os.path.isdir(main):
            for lnk in ("cert9.db", "cert_override.txt", "pkcs11.txt", "key4.db", "logins.json", "logins.db", "signedInUser.json"):
                s, d = os.path.join(main, lnk), os.path.join(pdir, lnk)
                if os.path.exists(s) and not os.path.exists(d):
                    try:
                        os.symlink(s, d)
                    except OSError:
                        shutil.copy2(s, d)
            for db in ("permissions.sqlite", "content-prefs.sqlite", "webappsstore.sqlite", "webappsstore.sqlite-wal", "webappsstore.sqlite-shm"):
                s, d = os.path.join(main, db), os.path.join(pdir, db)
                if os.path.isfile(s) and not os.path.isfile(d):
                    try:
                        shutil.copy2(s, d)
                    except OSError:
                        pass
            mc, dc = os.path.join(main, "cookies.sqlite"), os.path.join(pdir, "cookies.sqlite")
            if os.path.isfile(mc):
                if not os.path.isfile(dc):
                    for f in ("cookies.sqlite", "cookies.sqlite-wal", "cookies.sqlite-shm"):
                        if os.path.isfile(os.path.join(main, f)):
                            shutil.copy2(os.path.join(main, f), os.path.join(pdir, f))
                else:
                    with tempfile.TemporaryDirectory() as td:
                        for f in ("cookies.sqlite", "cookies.sqlite-wal", "cookies.sqlite-shm"):
                            if os.path.isfile(os.path.join(main, f)):
                                shutil.copy2(os.path.join(main, f), os.path.join(td, f))
                        try:
                            c = sqlite3.connect(dc, timeout=1.5)
                            c.execute("ATTACH DATABASE ? AS m", (os.path.join(td, "cookies.sqlite"),))
                            c.execute("INSERT OR REPLACE INTO moz_cookies SELECT * FROM m.moz_cookies")
                            c.commit()
                            c.close()
                        except Exception:
                            pass
            s_def, d_def = os.path.join(main, "storage", "default"), os.path.join(pdir, "storage", "default")
            if os.path.isdir(s_def):
                os.makedirs(d_def, exist_ok=True)
                for e in os.listdir(s_def):
                    se, de = os.path.join(s_def, e), os.path.join(d_def, e)
                    if os.path.isdir(se) and not os.path.exists(de):
                        try:
                            os.symlink(se, de)
                        except OSError:
                            pass
            break
os.environ["MOZ_ENABLE_WAYLAND"] = "1"
os.execvp("firefox", ["firefox", "--no-remote", "--new-instance", "--profile", pdir, "--name", aid, "--class", aid, target_url])
'''


class MetaParser(html.parser.HTMLParser):
    def __init__(self, base_url):
        super().__init__()
        self.base_url, self.in_title, self.done_title = base_url, False, False
        self.title_parts, self.meta_title, self.manifest_url, self.icons = [], "", "", []

    def handle_starttag(self, tag, attrs):
        d = {k.lower(): (v or "") for k, v in attrs}
        if tag == "title" and not self.done_title:
            self.in_title = True
        elif tag == "meta":
            k, v = (d.get("property") or d.get("name") or "").lower(), d.get("content", "").strip()
            if k in ("application-name", "apple-mobile-web-app-title", "og:site_name", "og:title") and v and not self.meta_title:
                self.meta_title = v
        elif tag == "link" and d.get("href"):
            rel, href = d.get("rel", "").lower(), urllib.parse.urljoin(self.base_url, d["href"])
            if "manifest" in rel and not self.manifest_url:
                self.manifest_url = href
            elif any(x in rel for x in ("apple-touch-icon", "icon", "fluid-icon")):
                sz = sum(int(x) for x in re.findall(r"\d+", d.get("sizes", "0"))[:1])
                self.icons.append((sz + (100 if "apple" in rel else 0), href))

    def handle_data(self, data):
        if self.in_title:
            self.title_parts.append(data)

    def handle_endtag(self, tag):
        if tag == "title" and self.in_title:
            self.in_title, self.done_title = False, True


def fetch_url(u, hdrs, timeout=6):
    try:
        return urllib.request.urlopen(urllib.request.Request(u, headers=hdrs), timeout=timeout)
    except Exception:
        return urllib.request.urlopen(urllib.request.Request(u, headers=hdrs), timeout=timeout, context=ssl._create_unverified_context())


def clean_title(raw, fallback):
    t = re.split(r"\s+[-|—·:]\s+", (raw or "").strip())[0].strip()
    words = t.split()
    t = " ".join(w for i, w in enumerate(words) if i == 0 or w.lower() != words[i - 1].lower()).strip()
    if not t or t.lower() in ("index", "home", "welcome", "loading", "login", "sign in", "web"):
        t = fallback
    return t.title() if t.islower() else t


def set_live(ljson, active, state, target, pct, step):
    try:
        with open(ljson, "w", encoding="utf-8") as fh:
            json.dump({"active": active, "state": state, "target": target, "pct": pct, "step": step, "ts": int(time.time())}, fh)
        subprocess.run(["qs", "ipc", "call", "status", "reloadForge"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    except Exception:
        pass


def main():
    url, cname, mode, wdir, bdir, fjson, ljson = sys.argv[1:8]
    if "|" in url and not cname:
        url, cname = [x.strip() for x in url.split("|", 1)]
    parsed = urllib.parse.urlparse(url)
    host = (parsed.hostname or "webapp").lower().removeprefix("www.")
    host_label = host.split(".")[0].capitalize()
    slug = re.sub(r"[^a-z0-9]+", "-", cname.lower() if cname else (host.split(".")[0] if host.count(".") == 1 else host.replace(".", "-"))).strip("-") or "webapp"
    aid, title = f"sf-webapp-{slug}", cname.strip()
    hdrs = {"User-Agent": "Mozilla/5.0 (X11; Fedora; Linux x86_64; rv:140.0) Gecko/20100101 Firefox/140.0"}
    mp = MetaParser(url)
    try:
        with fetch_url(url, hdrs, 6) as r:
            url = r.geturl() or url
            mp.base_url = url
            mp.feed(r.read(262144).decode("utf-8", errors="ignore"))
    except Exception:
        pass
    if mp.manifest_url:
        try:
            with fetch_url(mp.manifest_url, hdrs, 4) as mr:
                mf = json.loads(mr.read(131072).decode("utf-8", errors="ignore"))
                if not title and (mf.get("short_name") or mf.get("name")):
                    mp.meta_title = (mf.get("short_name") or mf.get("name")).strip()
                for ic in mf.get("icons") or []:
                    if ic.get("src"):
                        sz = sum(int(x) for x in re.findall(r"\d+", str(ic.get("sizes", "0")))[:1])
                        mp.icons.append((sz + 150, urllib.parse.urljoin(mp.manifest_url, ic["src"])))
        except Exception:
            pass
    title = clean_title(title or mp.meta_title or "".join(mp.title_parts), host_label)
    set_live(ljson, True, "running", title, 45, f"[2/4] Installing hicolor app icons (16px..512px) for {title}...")
    mp.icons.sort(key=lambda x: x[0], reverse=True)
    img, cands = None, [u for _, u in mp.icons] + [
        urllib.parse.urljoin(url, "images/icon-512x512.png"),
        urllib.parse.urljoin(url, "images/icon-192x192.png"),
        f"{parsed.scheme}://{parsed.netloc}/apple-touch-icon.png",
        f"https://www.google.com/s2/favicons?sz=256&domain={host}",
        f"https://icons.duckduckgo.com/ip3/{host}.ico",
        f"{parsed.scheme}://{parsed.netloc}/favicon.ico",
    ]
    for iu in dict.fromkeys(cands):
        try:
            with fetch_url(iu, hdrs, 5) as ir:
                im = Image.open(io.BytesIO(ir.read())).convert("RGBA")
                if im.width >= 16:
                    img = im
                    break
        except Exception:
            continue
    if img is None:
        img = Image.new("RGBA", (256, 256), (26, 27, 38, 255))
        dr = ImageDraw.Draw(img)
        dr.rectangle([8, 8, 247, 247], outline=(122, 162, 247, 255), width=8)
        dr.rectangle([22, 22, 233, 233], outline=(42, 195, 222, 255), width=4)
        dr.rectangle([64, 64, 191, 191], fill=(36, 40, 59, 255), outline=(122, 162, 247, 255), width=4)
    hicolor = os.path.expanduser("~/.local/share/icons/hicolor")
    for sz in (16, 32, 48, 64, 128, 256, 512):
        dd = os.path.join(hicolor, f"{sz}x{sz}", "apps")
        os.makedirs(dd, exist_ok=True)
        img.resize((sz, sz), Image.Resampling.LANCZOS).save(os.path.join(dd, f"{aid}.png"), "PNG")

    set_live(ljson, True, "running", title, 75, f"[3/4] Configuring chromeless Firefox profile ({mode}) for {title}...")
    pdir = os.path.join(wdir, slug, "profile")
    os.makedirs(os.path.join(pdir, "chrome"), exist_ok=True)
    with open(os.path.join(pdir, "user.js"), "w", encoding="utf-8") as fh:
        fh.write(
            'user_pref("toolkit.legacyUserProfileCustomizations.stylesheets", true);\n'
            'user_pref("browser.tabs.inTitlebar", 1);\n'
            'user_pref("browser.tabs.warnOnClose", false);\n'
            'user_pref("browser.tabs.closeWindowWithLastTab", true);\n'
            'user_pref("browser.shell.checkDefaultBrowser", false);\n'
            'user_pref("browser.startup.homepage_override.mstone", "ignore");\n'
            'user_pref("browser.aboutwelcome.enabled", false);\n'
            'user_pref("trailhead.firstrun.didSeeAboutWelcome", true);\n'
            'user_pref("datareporting.policy.dataSubmissionPolicyBypassNotification", true);\n'
            'user_pref("browser.sessionstore.resume_from_crash", false);\n'
        )
    with open(os.path.join(pdir, "chrome", "userChrome.css"), "w", encoding="utf-8") as fh:
        fh.write(
            '@namespace url("http://www.mozilla.org/keymaster/gatekeeper/there.is.only.xul");\n'
            "#navigator-toolbox, #TabsToolbar, #nav-bar, #PersonalToolbar, #titlebar, .titlebar-buttonbox-container, #sidebar-box, #sidebar-splitter {\n"
            "    visibility: collapse !important; display: none !important; max-height: 0 !important; min-height: 0 !important;\n"
            "}\n"
            "#browser, #appcontent, #tabbrowser-tabbox, #tabbrowser-tabpanels { margin: 0 !important; padding: 0 !important; border: 0 !important; }\n"
        )
    bpath = os.path.join(bdir, aid)
    with open(bpath, "w", encoding="utf-8") as fh:
        fh.write(LAUNCHER_TPL.format(pdir=pdir, url=url, mode=mode, aid=aid))
    os.chmod(bpath, 0o755)
    icon_p = os.path.join(hicolor, "256x256", "apps", f"{aid}.png")
    desk_d = os.path.expanduser("~/.local/share/applications")
    os.makedirs(desk_d, exist_ok=True)
    desk_p = os.path.join(desk_d, f"{aid}.desktop")
    with open(desk_p, "w", encoding="utf-8") as fh:
        fh.write(
            f"[Desktop Entry]\nVersion=1.0\nType=Application\nName={title}\nGenericName=Web Application\n"
            f"Comment={url} (Firefox {mode} profile)\nExec={bpath} %u\nIcon={icon_p}\nTerminal=false\n"
            f"Categories=Network;WebBrowser;\nStartupWMClass={aid}\nStartupNotify=true\n"
        )
    d = json.load(open(fjson, encoding="utf-8")) if os.path.exists(fjson) else {}
    d.setdefault("targets", {})[slug] = {
        "type": "webapp", "app_id": aid, "name": title, "url": url, "profile": mode, "binary": bpath, "icon": icon_p,
        "desktop": desk_p, "built_at": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
    }
    with open(fjson, "w", encoding="utf-8") as fh:
        json.dump(d, fh, indent=2)
    print(f"{aid}|{title}")


if __name__ == "__main__":
    main()
