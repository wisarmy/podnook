#!/usr/bin/env python3
"""Populate a temporary Podnook data directory with screenshot-friendly clips.

Usage: python3 scripts/seed-screenshots.py [DATA_DIR]

The real clipboard history is never touched: point PODNOOK_DATA_DIR at this
directory when taking screenshots (scripts/screenshot.sh does that).
"""

from __future__ import annotations

import hashlib
import json
import shutil
import sqlite3
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DEFAULT_DIR = ROOT / ".nsh" / "screenshot-data"


def main() -> None:
    data = Path(sys.argv[1]) if len(sys.argv) > 1 else DEFAULT_DIR
    real = Path.home() / "Library/Application Support/com.podnook.app"
    if data.resolve() == real.resolve():
        raise SystemExit("refusing to overwrite the real Podnook data directory")
    if data.exists():
        shutil.rmtree(data)
    (data / "images").mkdir(parents=True)

    icon = data / "images" / "podnook-icon.png"
    shutil.copy(ROOT / "assets" / "icon.png", icon)

    spec = data / "podnook-spec.pdf"
    spec.write_bytes(
        b"%PDF-1.4\n"
        b"% Podnook sample document\n"
        b"1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n"
        b"2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n"
        b"3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 595 842]>>endobj\n"
        b"trailer<</Root 1 0 R>>\n%%EOF\n"
    )

    db = data / "history.db"
    conn = sqlite3.connect(db)
    conn.executescript(
        """
        CREATE TABLE clips (
          id INTEGER PRIMARY KEY AUTOINCREMENT,
          kind TEXT NOT NULL, content TEXT,
          preview TEXT NOT NULL DEFAULT '', hash TEXT NOT NULL UNIQUE,
          image_path TEXT, image_w INTEGER, image_h INTEGER, image_bytes INTEGER,
          file_uris TEXT, source_app TEXT, source_bundle TEXT,
          byte_size INTEGER NOT NULL DEFAULT 0, truncated INTEGER NOT NULL DEFAULT 0,
          created_at INTEGER NOT NULL, last_copied_at INTEGER NOT NULL,
          copy_count INTEGER NOT NULL DEFAULT 0, pinned INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX clips_order ON clips(pinned DESC, last_copied_at DESC);
        CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
        """
    )

    now = int(time.time())
    created = now - 3 * 86400

    def add(
        kind: str,
        content: str | None,
        preview: str,
        app: str,
        bundle: str,
        age: int,
        *,
        pinned: int = 0,
        copy_count: int = 1,
        image: Path | None = None,
        file_uris: list[Path] | None = None,
        byte_size: int | None = None,
    ) -> None:
        digest = hashlib.sha256(
            f"{kind}\0{content or ''}\0{image or ''}\0{file_uris or ''}".encode()
        ).hexdigest()
        image_w = image_h = image_bytes = None
        if image is not None:
            image_w = image_h = 512
            image_bytes = image.stat().st_size
        if byte_size is None:
            if image_bytes is not None:
                byte_size = image_bytes
            elif file_uris:
                byte_size = sum(path.stat().st_size for path in file_uris)
            else:
                byte_size = len((content or "").encode())
        conn.execute(
            """
            INSERT INTO clips
              (kind, content, preview, hash, image_path, image_w, image_h,
               image_bytes, file_uris, source_app, source_bundle, byte_size,
               truncated, created_at, last_copied_at, copy_count, pinned)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, ?, ?, ?, ?)
            """,
            (
                kind,
                content,
                preview,
                digest,
                str(image) if image else None,
                image_w,
                image_h,
                image_bytes,
                json.dumps([str(path) for path in (file_uris or [])]),
                app,
                bundle,
                byte_size,
                created,
                now - age,
                copy_count,
                pinned,
            ),
        )

    # Pinned: the pod icon and the repository link.
    add(
        "image",
        None,
        "",
        "Keynote",
        "com.apple.iWork.Keynote",
        120,
        pinned=1,
        copy_count=3,
        image=icon,
    )
    add(
        "link",
        "https://github.com/wisarmy/podnook",
        "https://github.com/wisarmy/podnook",
        "Safari",
        "com.apple.Safari",
        300,
        pinned=1,
        copy_count=8,
    )

    # Today.
    add(
        "text",
        "A pod for everything you keep.",
        "A pod for everything you keep.",
        "Notes",
        "com.apple.Notes",
        540,
        copy_count=6,
    )
    add(
        "text",
        'fn main() {\n    let store = Store::open(data_dir)?;\n    println!("local-first");\n}',
        'fn main() { let store = Store::open(data_dir)?; println!("local-first"); }',
        "Xcode",
        "com.apple.dt.Xcode",
        900,
        copy_count=4,
    )
    add(
        "file",
        None,
        "",
        "Finder",
        "com.apple.finder",
        1500,
        copy_count=2,
        file_uris=[spec],
    )
    add(
        "link",
        "https://nsh.dev",
        "https://nsh.dev",
        "Safari",
        "com.apple.Safari",
        2400,
        copy_count=5,
    )
    add(
        "text",
        "Local-first: clipboard history is written to one SQLite file on this Mac. No upload, no network.",
        "Local-first: clipboard history is written to one SQLite file on this Mac. No upload, no network.",
        "Freeform",
        "com.apple.freeform",
        3600,
        copy_count=12,
    )
    add(
        "text",
        "⌘⇧V to summon · ↵ to paste · ⌘K for actions",
        "⌘⇧V to summon · ↵ to paste · ⌘K for actions",
        "Notes",
        "com.apple.Notes",
        7200,
        copy_count=3,
    )

    conn.executemany(
        "INSERT OR REPLACE INTO settings (key, value) VALUES (?, ?)",
        [
            ("hotkey", "mod+shift+v"),
            ("retention.count", "1000"),
            ("retention.days", "30"),
            ("ignored.bundles", '["com.apple.Passwords"]'),
            ("locale", "en"),
        ],
    )
    conn.commit()
    conn.close()
    print(data)


if __name__ == "__main__":
    main()
