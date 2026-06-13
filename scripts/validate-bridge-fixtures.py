#!/usr/bin/env python3
"""Validate NorvesEditor Bridge golden fixtures against their JSON Schema.

This is the minimal Phase C1 fixture-validation procedure. It is intentionally
dependency-light: the only third-party requirement is `jsonschema`, which is a
developer tool and is not vendored into the repository.

Usage:
    pip install jsonschema
    python scripts/validate-bridge-fixtures.py

Behavior:
- Every file under bridge/spec/fixtures/<group>/positive/ must parse as JSON and
  validate against the schema mapped for <group>.
- Every file under bridge/spec/fixtures/<group>/negative/ must parse as JSON and
  must FAIL validation (a negative fixture that validates is an error).

Exit code is non-zero if any expectation is not met.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

# Fixture group -> schema file (relative to bridge/spec/schema/).
# Later protocol phases add more groups/schemas here.
GROUP_SCHEMA = {
    "envelope": "envelope.schema.json",
}

REPO_ROOT = Path(__file__).resolve().parent.parent
SCHEMA_DIR = REPO_ROOT / "bridge" / "spec" / "schema"
FIXTURE_DIR = REPO_ROOT / "bridge" / "spec" / "fixtures"


def load_json(path: Path):
    with path.open("r", encoding="utf-8") as handle:
        return json.load(handle)


def main() -> int:
    try:
        from jsonschema import Draft202012Validator
    except ImportError:
        print("error: the 'jsonschema' package is required.", file=sys.stderr)
        print("       install it with: pip install jsonschema", file=sys.stderr)
        return 2

    failures: list[str] = []
    checked = 0

    for group, schema_name in GROUP_SCHEMA.items():
        schema_path = SCHEMA_DIR / schema_name
        if not schema_path.exists():
            failures.append(f"missing schema: {schema_path}")
            continue

        schema = load_json(schema_path)
        Draft202012Validator.check_schema(schema)
        validator = Draft202012Validator(schema)

        group_dir = FIXTURE_DIR / group
        for expectation in ("positive", "negative"):
            case_dir = group_dir / expectation
            if not case_dir.exists():
                continue

            for fixture in sorted(case_dir.glob("*.json")):
                checked += 1
                rel = fixture.relative_to(REPO_ROOT)

                try:
                    instance = load_json(fixture)
                except json.JSONDecodeError as exc:
                    failures.append(f"{rel}: not parseable JSON: {exc}")
                    continue

                errors = sorted(validator.iter_errors(instance), key=str)

                if expectation == "positive" and errors:
                    detail = "; ".join(e.message for e in errors)
                    failures.append(f"{rel}: expected VALID but failed: {detail}")
                elif expectation == "negative" and not errors:
                    failures.append(f"{rel}: expected INVALID but it validated")

    if failures:
        print(f"FAIL: {len(failures)} problem(s) across {checked} fixture(s):")
        for failure in failures:
            print(f"  - {failure}")
        return 1

    print(f"OK: {checked} fixture(s) validated.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
