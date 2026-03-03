#!/usr/bin/env python3
import argparse
import json
import os
import platform
import sqlite3
from pathlib import Path


def default_db_path() -> Path:
    system = platform.system().lower()
    if system == "darwin":
        base = Path.home() / "Library" / "Application Support"
    elif system == "windows":
        appdata = os.environ.get("APPDATA")
        if not appdata:
            raise RuntimeError("APPDATA is not set")
        base = Path(appdata)
    else:
        xdg = os.environ.get("XDG_DATA_HOME")
        base = Path(xdg) if xdg else (Path.home() / ".local" / "share")
    return base / "kuse-cowork" / "kuse-cowork.db"


def main() -> int:
    parser = argparse.ArgumentParser(description="Dump Kuse UI runtime errors from SQLite")
    parser.add_argument("--limit", type=int, default=50, help="max rows to print")
    parser.add_argument("--db", type=Path, default=default_db_path(), help="path to kuse-cowork.db")
    args = parser.parse_args()

    if not args.db.exists():
        print(json.dumps({"error": f"database not found: {args.db}"}))
        return 1

    conn = sqlite3.connect(args.db)
    try:
        cur = conn.cursor()
        cur.execute(
            """
            SELECT id, source, message, stack, context, timestamp
            FROM ui_runtime_errors
            ORDER BY timestamp DESC
            LIMIT ?
            """,
            (max(1, args.limit),),
        )
        rows = cur.fetchall()
        payload = [
            {
                "id": row[0],
                "source": row[1],
                "message": row[2],
                "stack": row[3],
                "context": row[4],
                "timestamp": row[5],
            }
            for row in rows
        ]
        print(json.dumps({"count": len(payload), "items": payload}, ensure_ascii=False, indent=2))
    finally:
        conn.close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
