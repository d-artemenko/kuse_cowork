#!/usr/bin/env python3
import argparse
import json
import os
import platform
import sqlite3
import sys
import urllib.error
import urllib.parse
import urllib.request
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


def load_settings(db_path: Path) -> dict:
    conn = sqlite3.connect(db_path)
    try:
        cur = conn.cursor()
        rows = cur.execute("SELECT key, value FROM settings").fetchall()
    finally:
        conn.close()
    return {k: v for k, v in rows}


def normalize_base_url(raw: str) -> str:
    raw = (raw or "").strip()
    if not raw:
        raise ValueError("moltis_server_url is empty in app settings")
    if "://" not in raw:
        raw = f"http://{raw}"
    return raw


def health_url(base_url: str) -> str:
    parsed = urllib.parse.urlparse(base_url)
    if parsed.scheme not in {"http", "https", "ws", "wss"}:
        raise ValueError(f"unsupported scheme: {parsed.scheme}")
    scheme = "https" if parsed.scheme == "wss" else "http" if parsed.scheme == "ws" else parsed.scheme
    rebuilt = parsed._replace(scheme=scheme, path="/health", params="", query="", fragment="")
    return urllib.parse.urlunparse(rebuilt)


def main() -> int:
    parser = argparse.ArgumentParser(description="Check live Moltis availability using app settings")
    parser.add_argument("--db", type=Path, default=default_db_path(), help="path to kuse-cowork.db")
    parser.add_argument("--timeout", type=float, default=5.0, help="HTTP timeout seconds")
    args = parser.parse_args()

    if not args.db.exists():
        print(json.dumps({"ok": False, "error": f"database not found: {args.db}"}, ensure_ascii=False))
        return 2

    settings = load_settings(args.db)
    base = normalize_base_url(settings.get("moltis_server_url", ""))
    api_key = (settings.get("moltis_api_key", "") or "").strip()
    sidecar = (settings.get("moltis_sidecar_enabled", "") or "").lower() in {"1", "true", "yes", "on"}
    url = health_url(base)

    request = urllib.request.Request(url, method="GET")
    if api_key:
        request.add_header("Authorization", f"Bearer {api_key}")

    payload = {
        "ok": False,
        "db": str(args.db),
        "server_url": base,
        "health_url": url,
        "sidecar_enabled": sidecar,
        "auth_mode": "bearer" if api_key else "none",
    }

    try:
        with urllib.request.urlopen(request, timeout=args.timeout) as resp:
            body = resp.read().decode("utf-8", errors="replace")
            payload["status"] = resp.status
            payload["body"] = body[:4000]
            payload["ok"] = 200 <= resp.status < 300
    except urllib.error.HTTPError as exc:
        body = exc.read().decode("utf-8", errors="replace")
        payload["status"] = exc.code
        payload["error"] = f"http error {exc.code}"
        payload["body"] = body[:4000]
    except urllib.error.URLError as exc:
        reason = getattr(exc, "reason", exc)
        payload["error"] = f"network error: {reason}"
        if sidecar and ("127.0.0.1" in base or "localhost" in base or "[::1]" in base):
            payload["hint"] = (
                "sidecar enabled with local URL, but local Moltis is not reachable; "
                "start sidecar or set reachable Moltis server URL"
            )
    except Exception as exc:
        payload["error"] = f"unexpected error: {exc}"

    print(json.dumps(payload, ensure_ascii=False, indent=2))
    return 0 if payload["ok"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
