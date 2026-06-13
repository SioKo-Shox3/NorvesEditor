# Vision

NorvesEditor is a modern desktop game editor that connects to a C++ engine through a clear Bridge boundary instead of embedding engine internals into the editor UI.

The first usable alpha is intentionally narrow: launch or attach to an engine process, connect through the Bridge protocol, show status/logs, and control runtime state from a Game View panel.

## Product Direction

NorvesEditor should make engine/editor integration inspectable and testable. The editor owns user experience, workspace flow, process lifecycle, and connection state. The engine owns runtime behavior, logs, status, and its native viewport window.

## Reference Engine

NorvesLib is the first reference engine integration candidate, but NorvesEditor is not a NorvesLib-only editor. NorvesLib-specific code belongs in an adapter that maps NorvesLib runtime data into generic Bridge DTOs.

## Alpha Proof

The alpha proves the integration loop:

```text
NorvesEditor launches or attaches to a C++ engine
  -> Bridge connection is established
  -> status/log/runtime events flow into the editor
  -> Game View controls send runtime commands
```

Scene editing, asset workflows, embedded viewport rendering, and public standardization are post-alpha concerns.
