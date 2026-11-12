"""Export the old Code Compass database to a ppicker-compatible TOML file.

Deliberately does not import ``code_compass.config``: that module creates
files at import time, and migrating must never modify the old data.
"""

import json
import shutil
import sqlite3
import subprocess
import sys
from datetime import datetime
from pathlib import Path
from typing import List, Optional

OLD_DB = Path.home() / ".config" / "code_compass" / "data.db"
DEFAULT_OUTPUT = Path("ppicker-import.toml")


def _to_epoch(value: Optional[str]) -> Optional[int]:
    if not value:
        return None
    try:
        return int(datetime.fromisoformat(str(value)).timestamp())
    except ValueError:
        return None


def read_projects(db_path: Path = OLD_DB) -> List[dict]:
    """Return projects with their category as a single tag. Read-only."""
    if not db_path.is_file():
        raise FileNotFoundError(f"old database not found: {db_path}")
    con = sqlite3.connect(f"file:{db_path.as_posix()}?mode=ro", uri=True)
    try:
        rows = con.execute(
            """
            SELECT p.name, p.path, p.last_opened, c.name
            FROM projects p LEFT JOIN categories c ON c.id = p.category_id
            ORDER BY p.name
            """
        ).fetchall()
    finally:
        con.close()
    return [
        {
            "name": name,
            "path": path,
            "tags": [category] if category else [],
            "last_opened": _to_epoch(last_opened),
        }
        for name, path, last_opened, category in rows
    ]


def to_toml(projects: List[dict]) -> str:
    # JSON string escaping is valid TOML basic-string escaping.
    out = []
    for p in projects:
        out.append("[[project]]")
        out.append(f"name = {json.dumps(p['name'])}")
        out.append(f"path = {json.dumps(p['path'])}")
        out.append(f"tags = {json.dumps(p['tags'])}")
        if p["last_opened"] is not None:
            out.append(f"last_opened = {p['last_opened']}")
        out.append("")
    return "\n".join(out)


def main(argv: List[str]) -> int:
    output: Optional[Path] = None
    if "--output" in argv:
        i = argv.index("--output")
        if i + 1 >= len(argv):
            print("code-compass migrate: --output needs a file", file=sys.stderr)
            return 2
        output = Path(argv[i + 1])

    try:
        projects = read_projects()
    except FileNotFoundError as e:
        print(f"code-compass migrate: {e}", file=sys.stderr)
        return 1
    text = to_toml(projects)
    print(f"Found {len(projects)} project(s).", file=sys.stderr)

    pp = shutil.which("pp")
    if output is None and pp:
        result = subprocess.run([pp, "import", "-"], input=text, text=True)
        return result.returncode

    target = output or DEFAULT_OUTPUT
    target.write_text(text, encoding="utf-8")
    print(
        f"Wrote {target}. Install ppicker (pipx install ppicker), "
        f"then run: pp import {target}",
        file=sys.stderr,
    )
    return 0
