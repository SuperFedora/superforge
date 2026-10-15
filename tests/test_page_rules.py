#!/usr/bin/env python3
"""tests/test_page_rules.py — Verify SuperFedora Page Rules (16-256 lines), directory density (<= 12), and Zero-Emoji Invariant."""
import os
import re
import sys

REPO_ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
CODE_EXTS = {".sh", ".py", ".qml", ".js", ".css", ".svg", ".rs"}
SKIP_DIRS = {".git", "__pycache__", "node_modules", "target"}

EMOJI_RE = re.compile(
    "[\U0001F300-\U0001FAFF\U0001F600-\U0001F64F"
    "\U0001F680-\U0001F6FF\U0001F1E0-\U0001F1FF]"
)


def check_page_rules():
    failures = []
    for root, dirs, files in os.walk(REPO_ROOT):
        dirs[:] = [d for d in dirs if d not in SKIP_DIRS]
        rel_dir = os.path.relpath(root, REPO_ROOT)
        func_files = [f for f in files if not f.startswith(".")]
        if len(func_files) > 12:
            failures.append(f"Directory density exceeded in {rel_dir}: {len(func_files)} > 12 files")

        for f in files:
            path = os.path.join(root, f)
            rel_path = os.path.relpath(path, REPO_ROOT)
            ext = os.path.splitext(f)[1]
            is_bin = rel_dir == "bin"

            try:
                with open(path, "r", encoding="utf-8") as fh:
                    text = fh.read()
            except UnicodeDecodeError:
                continue

            if EMOJI_RE.search(text):
                failures.append(f"Emoji detected in {rel_path}")

            if ext in CODE_EXTS or is_bin:
                lines = text.splitlines()
                n = len(lines)
                if n < 16 or n > 256:
                    failures.append(f"Line count out of bounds (16-256) in {rel_path}: {n} lines")

    if failures:
        for fail in failures:
            print(f"  [FAIL] {fail}", file=sys.stderr)
        sys.exit(1)

    print("  [OK] All files satisfy Page Rules (16-256 lines), directory density (<= 12), and Zero-Emoji Invariant.")


if __name__ == "__main__":
    print("Running SuperFedora Page Rule & Invariant checks...")
    check_page_rules()
