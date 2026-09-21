"""Load/save an Outline to a JSON file, with atomic writes."""
from __future__ import annotations

import json
import os
from pathlib import Path

from .models import Outline

DEFAULT_PATH = Path.home() / ".bulletrix" / "outline.json"


def load(path: Path = DEFAULT_PATH) -> Outline:
    if not path.exists():
        return Outline()
    with path.open("r", encoding="utf-8") as f:
        data = json.load(f)
    return Outline.from_dict(data)


def save(outline: Outline, path: Path = DEFAULT_PATH) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp_path = path.with_suffix(path.suffix + ".tmp")
    with tmp_path.open("w", encoding="utf-8") as f:
        json.dump(outline.to_dict(), f, indent=2)
    os.replace(tmp_path, path)
