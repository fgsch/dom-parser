#!/usr/bin/env python3
"""Compile every Rust code block in README.md as a standalone binary."""

from __future__ import annotations

import subprocess
import tempfile
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
README = ROOT / "README.md"
OPENING_FENCE = "```rust\n"
CLOSING_FENCE = "\n```"


def rust_blocks(markdown: str) -> list[str]:
    blocks: list[str] = []
    offset = 0

    while True:
        start = markdown.find(OPENING_FENCE, offset)
        if start == -1:
            return blocks
        start += len(OPENING_FENCE)
        end = markdown.find(CLOSING_FENCE, start)
        if end == -1:
            raise SystemExit("README.md has an unterminated Rust code block")
        blocks.append(markdown[start:end])
        offset = end + len(CLOSING_FENCE)


def main() -> None:
    blocks = rust_blocks(README.read_text(encoding="utf-8"))
    if not blocks:
        raise SystemExit("README.md contains no Rust code blocks")

    with tempfile.TemporaryDirectory(prefix="dom-parser-readme-") as directory:
        package = Path(directory)
        binaries = package / "src" / "bin"
        binaries.mkdir(parents=True)

        dependency_path = ROOT.as_posix().replace('"', '\\"')
        (package / "Cargo.toml").write_text(
            "[package]\n"
            'name = "dom-parser-readme-check"\n'
            'version = "0.0.0"\n'
            'edition = "2024"\n\n'
            "[dependencies]\n"
            f'dom-parser = {{ path = "{dependency_path}" }}\n',
            encoding="utf-8",
        )

        for index, block in enumerate(blocks):
            (binaries / f"example_{index}.rs").write_text(block + "\n", encoding="utf-8")

        subprocess.run(
            ["cargo", "check", "--quiet", "--bins", "--manifest-path", str(package / "Cargo.toml")],
            cwd=ROOT,
            check=True,
        )

    print(f"README.md: compiled {len(blocks)} Rust examples")


if __name__ == "__main__":
    main()
