#!/usr/bin/env python3
"""Fail when UI text is hard-coded in source: every string in `src/` is English
and comes from `locales/`.

Comments may be Chinese; string literals may not.
"""

from __future__ import annotations

import pathlib
import re
import sys

CJK = re.compile(r"[\u3040-\u30ff\u3400-\u4dbf\u4e00-\u9fff\uf900-\ufaff\uac00-\ud7af]")
STRING = re.compile(r'"([^"\\]|\\.)*"')

ROOT = pathlib.Path(__file__).resolve().parent.parent


def main() -> int:
    problems: list[str] = []
    for path in sorted((ROOT / "src").rglob("*.rs")):
        for number, line in enumerate(path.read_text().splitlines(), 1):
            code = line.split("//", 1)[0]
            for literal in STRING.finditer(code):
                if CJK.search(literal.group(0)):
                    problems.append(
                        f"{path.relative_to(ROOT)}:{number}: {literal.group(0)}"
                    )
    if problems:
        print("UI text must live in locales/, not in source:", file=sys.stderr)
        for problem in problems:
            print(f"  {problem}", file=sys.stderr)
        return 1
    print("✓ no hard-coded UI text in src/")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
