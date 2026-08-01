#!/usr/bin/env python3
"""Validate local Markdown links, heading levels, and fenced code languages."""

from __future__ import annotations

import re
import sys
from pathlib import Path
from urllib.parse import unquote

ROOT = Path(__file__).resolve().parent.parent
LINK_PATTERN = re.compile(r"(?<!!)\[[^\]]+\]\(([^)]+)\)")
HEADING_PATTERN = re.compile(r"^(#{1,6})\s+\S")
FENCE_PATTERN = re.compile(r"^```(.*)$")


def markdown_files() -> list[Path]:
    files = [ROOT / "README.md", ROOT / "desktop" / "README.md"]
    files.extend(sorted((ROOT / "docs").glob("*.md")))
    files.append(ROOT / ".github" / "release-notes.md")
    return files


def check_file(path: Path) -> list[str]:
    errors: list[str] = []
    if not path.is_file():
        return [f"{path.relative_to(ROOT)}: file is missing"]

    previous_heading = 0
    in_fence = False
    text = path.read_text(encoding="utf-8")
    for line_number, line in enumerate(text.splitlines(), start=1):
        fence = FENCE_PATTERN.match(line)
        if fence:
            suffix = fence.group(1).strip()
            if not in_fence and not suffix:
                errors.append(
                    f"{path.relative_to(ROOT)}:{line_number}: opening code fence needs a language"
                )
            in_fence = not in_fence
            continue

        if in_fence:
            continue

        heading = HEADING_PATTERN.match(line)
        if heading:
            level = len(heading.group(1))
            if previous_heading and level > previous_heading + 1:
                errors.append(
                    f"{path.relative_to(ROOT)}:{line_number}: heading jumps from H{previous_heading} to H{level}"
                )
            previous_heading = level

        for match in LINK_PATTERN.finditer(line):
            target = match.group(1).strip().strip("<>").split(maxsplit=1)[0]
            if target.startswith(("#", "http://", "https://", "mailto:")):
                continue
            relative_target = unquote(target.split("#", 1)[0].split("?", 1)[0])
            resolved = (path.parent / relative_target).resolve()
            try:
                resolved.relative_to(ROOT)
            except ValueError:
                errors.append(
                    f"{path.relative_to(ROOT)}:{line_number}: link escapes repository: {target}"
                )
                continue
            if not resolved.exists():
                errors.append(
                    f"{path.relative_to(ROOT)}:{line_number}: missing link target: {target}"
                )

    if in_fence:
        errors.append(f"{path.relative_to(ROOT)}: unclosed code fence")
    return errors


def main() -> int:
    errors = [error for path in markdown_files() for error in check_file(path)]
    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    print(f"validated {len(markdown_files())} Markdown files")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
