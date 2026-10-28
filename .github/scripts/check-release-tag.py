"""Require an exact v<version> tag for every workspace member."""

import json
import subprocess
import sys
import tomllib
from pathlib import Path


def main():
    tag = sys.argv[1]
    metadata = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--format-version", "1", "--no-deps", "--locked"],
        text=True,
    ))
    root = Path(metadata["workspace_root"]) / "Cargo.toml"
    with root.open("rb") as manifest:
        version = tomllib.load(manifest)["workspace"]["package"]["version"]
    expected = f"v{version}"
    if tag != expected:
        sys.exit(f"Tag {tag!r} does not match workspace version: expected {expected!r}")
    members = set(metadata["workspace_members"])
    for package in metadata["packages"]:
        if package["id"] in members and package["version"] != version:
            sys.exit(f"{package['name']} version {package['version']} does not match {tag}")
    print(f"Validated {tag} against {len(members)} workspace crates")


if __name__ == "__main__":
    main()
