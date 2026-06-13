# 0001: Editor-Owned Bridge Subsystem

Status: Accepted for alpha

## Context

The former NorvesBridge idea could have become an independent repository, but the alpha needs a tight feedback loop between editor UX, process lifecycle, protocol fixtures, SDK work, mock engine, and conformance tests.

## Decision

NorvesBridge is folded into NorvesEditor as the `bridge/` subsystem for alpha. It contains protocol, SDK, tooling, mock engine, and conformance work.

## Consequences

The bridge boundary remains strict enough for future extraction, but extraction is not an alpha goal. Generic bridge code must not become NorvesLib-specific.
