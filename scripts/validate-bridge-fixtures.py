#!/usr/bin/env python3
"""Validate NorvesEditor Bridge golden fixtures against their JSON Schemas.

This validator is intentionally dependency-light: the only third-party
requirement is `jsonschema` (which pulls in `referencing`). These are developer
tools and are not vendored into the repository.

Usage:
    pip install jsonschema
    python scripts/validate-bridge-fixtures.py

Two-layer validation
---------------------
Every fixture is a full Bridge envelope. Validation is composed of two layers:

1. Envelope layer (phase C1): the whole fixture is validated against
   ``bridge/spec/schema/envelope.schema.json``. The envelope schema validates
   only envelope structure; ``method``/``event`` names are checked by *pattern*,
   not against a registry.

2. Payload layer (phase C2+): for fixtures under ``methods/`` and ``events/``,
   the ``params``/``result`` payload is *additionally* validated against a
   per-method/per-event payload schema. The envelope schema is never modified to
   do this; payload schemas live in separate files and are composed here.

Directory conventions
---------------------
::

    bridge/spec/schema/
      envelope.schema.json
      common.schema.json
      methods/<method>.params.schema.json
      methods/<method>.result.schema.json
      events/<event>.params.schema.json

    bridge/spec/fixtures/
      envelope/{positive,negative}/*.json          # envelope layer only
      methods/<method>/{positive,negative}/*.json   # envelope + payload layers
      events/<event>/{positive,negative}/*.json     # envelope + payload layers

Payload schema selection for ``methods/<method>/`` and ``events/<event>/``
fixtures is driven by the directory name plus the fixture's ``kind``:

- a request fixture validates ``params`` against ``methods/<method>.params``;
- a response fixture with ``result`` validates it against
  ``methods/<method>.result``;
- a response fixture with ``error`` is validated by the envelope layer only
  (the error object is defined in the envelope schema);
- an event fixture validates ``params`` against ``events/<event>.params``.

Cross-file ``$ref`` (e.g. ``error.reported`` referencing the envelope's
``$defs/error``, or payload schemas referencing ``common.schema.json``) is
resolved through a ``referencing`` registry built from every ``*.schema.json``
file keyed by its absolute ``$id``.

Behavior
--------
- Every ``positive/`` fixture must parse as JSON and pass ALL applicable layers.
- Every ``negative/`` fixture must parse as JSON and FAIL at least one layer
  (a negative fixture that fully validates is an error).

Exit code is non-zero if any expectation is not met.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
SCHEMA_DIR = REPO_ROOT / "bridge" / "spec" / "schema"
FIXTURE_DIR = REPO_ROOT / "bridge" / "spec" / "fixtures"

ENVELOPE_SCHEMA = "envelope.schema.json"


def load_json(path: Path):
    with path.open("r", encoding="utf-8") as handle:
        return json.load(handle)


def build_validator_factory():
    """Return (make_validator, error). make_validator(schema_dict) -> validator.

    All ``*.schema.json`` files are registered in a shared referencing registry
    so cross-file ``$ref`` by absolute ``$id`` resolves.
    """
    from jsonschema import Draft202012Validator
    from referencing import Registry, Resource

    resources = []
    for schema_file in sorted(SCHEMA_DIR.rglob("*.json")):
        doc = load_json(schema_file)
        schema_id = doc.get("$id")
        if not schema_id:
            raise ValueError(f"schema missing $id: {schema_file.relative_to(REPO_ROOT)}")
        Draft202012Validator.check_schema(doc)
        resources.append((schema_id, Resource.from_contents(doc)))

    registry = Registry().with_resources(resources)

    def make_validator(schema_dict):
        return Draft202012Validator(schema_dict, registry=registry)

    return make_validator


class Checker:
    def __init__(self, make_validator):
        self._make_validator = make_validator
        self._cache: dict[Path, object] = {}
        self.failures: list[str] = []
        self.checked = 0

    def validator_for(self, schema_path: Path):
        if schema_path not in self._cache:
            self._cache[schema_path] = self._make_validator(load_json(schema_path))
        return self._cache[schema_path]

    def errors_against(self, schema_path: Path, instance, rel, label: str) -> list[str]:
        """Return human messages; a missing schema is itself a failure."""
        if not schema_path.exists():
            self.failures.append(
                f"{rel}: missing {label} schema {schema_path.relative_to(REPO_ROOT)}"
            )
            # Treat as a hard config error: report and signal "had errors" so a
            # positive fixture does not silently pass without its payload schema.
            return [f"missing schema {schema_path.name}"]
        validator = self.validator_for(schema_path)
        return [e.message for e in validator.iter_errors(instance)]

    def payload_errors(self, instance, group: str, name: str, rel) -> list[str]:
        """Validate the params/result payload for a method/event fixture."""
        kind = instance.get("kind")
        if group == "events":
            schema_path = SCHEMA_DIR / "events" / f"{name}.params.schema.json"
            return self.errors_against(
                schema_path, instance.get("params", {}), rel, "event params"
            )

        # group == "methods"
        if kind == "request":
            schema_path = SCHEMA_DIR / "methods" / f"{name}.params.schema.json"
            return self.errors_against(
                schema_path, instance.get("params", {}), rel, "method params"
            )
        if kind == "response" and "result" in instance:
            schema_path = SCHEMA_DIR / "methods" / f"{name}.result.schema.json"
            return self.errors_against(
                schema_path, instance["result"], rel, "method result"
            )
        # response with error, or any other shape: envelope layer is enough.
        return []

    def run_group(self, group: str, envelope_validator) -> None:
        group_root = FIXTURE_DIR / group
        if not group_root.exists():
            return

        if group == "envelope":
            self._run_leaf(group_root, envelope_validator, None, None)
            return

        # methods/ and events/ contain one subdirectory per method/event name.
        for name_dir in sorted(p for p in group_root.iterdir() if p.is_dir()):
            self._run_leaf(name_dir, envelope_validator, group, name_dir.name)

    def _run_leaf(self, root: Path, envelope_validator, group, name) -> None:
        for expectation in ("positive", "negative"):
            case_dir = root / expectation
            if not case_dir.exists():
                continue
            for fixture in sorted(case_dir.glob("*.json")):
                self.checked += 1
                rel = fixture.relative_to(REPO_ROOT)
                try:
                    instance = load_json(fixture)
                except json.JSONDecodeError as exc:
                    self.failures.append(f"{rel}: not parseable JSON: {exc}")
                    continue

                errors = [e.message for e in envelope_validator.iter_errors(instance)]
                if group is not None:
                    errors += self.payload_errors(instance, group, name, rel)

                if expectation == "positive" and errors:
                    detail = "; ".join(errors)
                    self.failures.append(f"{rel}: expected VALID but failed: {detail}")
                elif expectation == "negative" and not errors:
                    self.failures.append(f"{rel}: expected INVALID but it validated")


def main() -> int:
    try:
        make_validator = build_validator_factory()
    except ImportError:
        print("error: the 'jsonschema' package is required.", file=sys.stderr)
        print("       install it with: pip install jsonschema", file=sys.stderr)
        return 2

    envelope_schema_path = SCHEMA_DIR / ENVELOPE_SCHEMA
    if not envelope_schema_path.exists():
        print(f"error: missing envelope schema {envelope_schema_path}", file=sys.stderr)
        return 2

    checker = Checker(make_validator)
    envelope_validator = checker.validator_for(envelope_schema_path)

    for group in ("envelope", "methods", "events"):
        checker.run_group(group, envelope_validator)

    if checker.failures:
        print(f"FAIL: {len(checker.failures)} problem(s) across {checker.checked} fixture(s):")
        for failure in checker.failures:
            print(f"  - {failure}")
        return 1

    print(f"OK: {checker.checked} fixture(s) validated.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
