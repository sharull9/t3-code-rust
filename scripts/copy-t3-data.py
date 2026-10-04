"""Copy a live T3 data directory for testing a standalone Rust client server.

Uses SQLite online backups rather than copying open database/WAL files.
Never overwrites an existing destination or modifies the source.
"""
import argparse
import json
from pathlib import Path
import shutil
import sqlite3
import uuid


def copy_data(source: Path, destination: Path) -> None:
    source = source.resolve()
    destination = destination.resolve()
    if not (source / "userdata").is_dir():
        raise ValueError(f"No userdata directory at {source}")
    if destination == source or source in destination.parents:
        raise ValueError("Destination must be outside the source directory")
    if destination.exists():
        raise FileExistsError(f"Destination already exists: {destination}")

    def excluded(directory: str, names: list[str]) -> set[str]:
        return {
            name for name in names
            if name.endswith((".sqlite", ".sqlite-wal", ".sqlite-shm", ".sqlite.backup"))
            or name in {"logs", "server-runtime.json", "antigravity-tmp"}
            # Provider runtime links are regenerated; do not follow them outside userdata.
            or (Path(directory) / name).is_symlink()
            or (Path(directory) / name).is_junction()
            or not (Path(directory) / name).exists()
        }

    data = destination / "userdata"
    shutil.copytree(source / "userdata", data, ignore=excluded, ignore_dangling_symlinks=True)
    for database in (source / "userdata").glob("*.sqlite"):
        with sqlite3.connect(database.as_uri() + "?mode=ro", uri=True) as original:
            with sqlite3.connect(data / database.name) as copied:
                original.backup(copied, pages=4096, sleep=0.05)
                # A test copy must not run a second copy of scheduled work.
                if copied.execute("SELECT 1 FROM sqlite_master WHERE name='scheduled_tasks'").fetchone():
                    copied.execute("UPDATE scheduled_tasks SET enabled=0")
        print(f"Backed up {database.name}", flush=True)

    settings_path = data / "settings.json"
    settings = json.loads(settings_path.read_text(encoding="utf-8")) if settings_path.exists() else {}
    settings["continueThreadsAfterServerUpdate"] = False
    for overrides in settings.get("projectSettingsOverrides", {}).values():
        if isinstance(overrides, dict):
            overrides["continueThreadsAfterServerUpdate"] = False
    settings_path.write_text(json.dumps(settings, indent=2) + "\n", encoding="utf-8")
    # Give the test server its own identity while retaining project/thread IDs.
    (data / "environment-id").write_text(str(uuid.uuid4()) + "\n", encoding="utf-8")
    print(f"Copied server data to {destination}", flush=True)
    print("Automatic continuation and scheduled tasks are disabled in the copy.", flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("destination", type=Path)
    parser.add_argument("--source", type=Path, default=Path.home() / ".t3")
    args = parser.parse_args()
    copy_data(args.source, args.destination)
