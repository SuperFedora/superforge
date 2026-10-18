// Copyright 2026 SuperFedora Contributors
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;

const CODE_EXTS: &[&str] = &["sh", "py", "qml", "js", "css", "svg", "rs"];
const SKIP_DIRS: &[&str] = &[".git", "__pycache__", "node_modules", "target", "dist"];

fn has_emoji(s: &str) -> bool {
    s.chars().any(|c| {
        let u = c as u32;
        (0x1F300..=0x1FAFF).contains(&u)
            || (0x1F600..=0x1F64F).contains(&u)
            || (0x1F680..=0x1F6FF).contains(&u)
            || (0x1F1E0..=0x1F1FF).contains(&u)
    })
}

fn walk_repo(root: &Path, dir: &Path, failures: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    let mut subdirs = Vec::new();
    let mut files = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let path = entry.path();
        if path.is_dir() {
            if !SKIP_DIRS.contains(&name.as_str()) && !name.starts_with('.') {
                subdirs.push(path);
            }
        } else if path.is_file() {
            files.push((name, path));
        }
    }
    let rel_dir = dir.strip_prefix(root).unwrap_or(dir).display().to_string();
    let func_count = files.iter().filter(|(n, _)| !n.starts_with('.')).count();
    if func_count > 12 {
        failures.push(format!("Directory density exceeded in {rel_dir}: {func_count} > 12 files"));
    }
    for (name, path) in files {
        let rel_path = path.strip_prefix(root).unwrap_or(&path).display().to_string();
        let Ok(text) = fs::read_to_string(&path) else { continue };
        if has_emoji(&text) {
            failures.push(format!("Emoji detected in {rel_path}"));
        }
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let is_bin = rel_dir == "bin";
        if CODE_EXTS.contains(&ext) || is_bin {
            let n = text.lines().count();
            if !(16..=256).contains(&n) {
                failures.push(format!("Line count out of bounds (16-256) in {rel_path}: {n} lines"));
            }
            if ext == "rs" && !text.contains("SPDX-License-Identifier: Apache-2.0") {
                failures.push(format!("Missing Apache-2.0 SPDX header in {rel_path}"));
            }
        }
        if name == "README.md" && rel_dir.is_empty() {
            let n = text.lines().count();
            if n >= 25 {
                failures.push(format!("README.md must be < 25 lines: {n} lines"));
            }
        }
    }
    for sub in subdirs {
        walk_repo(root, &sub, failures);
    }
}

#[test]
fn verify_superfedora_page_rules() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for req in ["AGENTS.md", "LICENSE", "VERSION", "README.md"] {
        assert!(root.join(req).is_file(), "Missing canonical file {req}");
    }
    let mut failures = Vec::new();
    walk_repo(root, root, &mut failures);
    assert!(failures.is_empty(), "Page rule violations:\n{}", failures.join("\n"));
}
