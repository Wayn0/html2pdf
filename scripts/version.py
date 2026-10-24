#!/usr/bin/env python3
"""Keep the application version, lockfile, and release tag aligned (Python 3.11+)."""
import argparse
from pathlib import Path
import re
import tomllib

ROOT = Path(__file__).resolve().parent.parent


def version(value):
    if not re.fullmatch(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)", value):
        raise ValueError("expected a stable version such as 0.1.1 (no v prefix)")
    return tuple(map(int, value.split(".")))


def run(command, value):
    manifest_path, lock_path = ROOT / "Cargo.toml", ROOT / "Cargo.lock"
    manifest, lock = manifest_path.read_text(), lock_path.read_text()
    package = tomllib.loads(manifest)["package"]
    current = package["version"]
    locked = [p for p in tomllib.loads(lock)["package"] if p["name"] == package["name"] and "source" not in p]
    if len(locked) != 1 or locked[0]["version"] != current:
        raise ValueError("Cargo.toml and Cargo.lock application versions disagree")
    if command == "check-tag":
        version(value.removeprefix("v"))
        if value != f"v{current}":
            raise ValueError(f"tag {value!r} must match Cargo.toml version: v{current}")
        print(current)
        return
    if version(value) <= version(current):
        raise ValueError(f"new version must be greater than {current}")
    manifest, count = re.subn(r'(?m)^(version\s*=\s*)"' + re.escape(current) + r'"', lambda m: m[1] + f'"{value}"', manifest, count=1)
    if count != 1:
        raise ValueError("could not locate application version in Cargo.toml")
    pattern = r'(\[\[package\]\]\nname = "' + re.escape(package["name"]) + r'"\nversion = ")' + re.escape(current) + '"'
    lock, count = re.subn(pattern, lambda m: m[1] + value + '"', lock)
    if count != 1:
        raise ValueError("could not locate application version in Cargo.lock")
    # Validate both edits before writing either file.
    assert tomllib.loads(manifest)["package"]["version"] == value
    tomllib.loads(lock)
    manifest_path.write_text(manifest)
    lock_path.write_text(lock)
    print(f"Updated {current} -> {value}. Commit both Cargo files, then push tag v{value}.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["bump", "check-tag"])
    parser.add_argument("value", help="new version, or release tag for check-tag")
    args = parser.parse_args()
    try:
        run(args.command, args.value)
    except (ValueError, KeyError, OSError) as error:
        parser.exit(1, f"version: {error}\n")


if __name__ == "__main__":
    main()
