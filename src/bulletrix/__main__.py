"""CLI entry point: `python -m bulletrix [--file PATH]` or the `bulletrix` script."""
from __future__ import annotations

import argparse
import sys
import xml.etree.ElementTree as ET
from pathlib import Path

from . import storage
from .app import BulletrixApp
from .opml_import import import_opml
from .storage import DEFAULT_PATH


def main() -> None:
    parser = argparse.ArgumentParser(prog="bulletrix", description="A keyboard-driven, zoomable TUI outliner.")
    parser.add_argument(
        "--file",
        type=Path,
        default=DEFAULT_PATH,
        help=f"Outline JSON file to load/save (default: {DEFAULT_PATH})",
    )
    parser.add_argument(
        "--import",
        dest="import_path",
        type=Path,
        metavar="OPML_FILE",
        help="Import an OPML file, merging it into --file as new top-level "
        "items, then exit without launching the TUI",
    )
    args = parser.parse_args()

    if args.import_path is not None:
        try:
            outline = storage.load(args.file)
            added = import_opml(outline, args.import_path)
            storage.save(outline, args.file)
        except (OSError, ET.ParseError) as exc:
            print(f"Import failed: {exc}", file=sys.stderr)
            sys.exit(1)
        print(f"Imported {added} top-level item(s) from {args.import_path} into {args.file}")
        return

    BulletrixApp(path=args.file).run()


if __name__ == "__main__":
    main()
