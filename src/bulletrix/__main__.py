"""CLI entry point: `python -m bulletrix [--file PATH]` or the `bulletrix` script."""
from __future__ import annotations

import argparse
from pathlib import Path

from .app import BulletrixApp
from .storage import DEFAULT_PATH


def main() -> None:
    parser = argparse.ArgumentParser(prog="bulletrix", description="A keyboard-driven, zoomable TUI outliner.")
    parser.add_argument(
        "--file",
        type=Path,
        default=DEFAULT_PATH,
        help=f"Outline JSON file to load/save (default: {DEFAULT_PATH})",
    )
    args = parser.parse_args()
    BulletrixApp(path=args.file).run()


if __name__ == "__main__":
    main()
