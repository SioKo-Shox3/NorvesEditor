# Engine Integration

NorvesEditor integrates with engines through the Bridge protocol and a standalone C++ engine-side SDK. The generic Bridge layer must remain independent from any one engine.

## Generic Boundary

```text
NorvesEditor UI
  -> Tauri command/event wrappers
Rust backend
  -> Bridge editor client runtime
WebSocket + JSON
C++ engine-side SDK
  -> engine adapter
```

The C++ SDK public API must not expose Tauri, React, TypeScript, NorvesLib, or third-party WebSocket types.

## NorvesLib Adapter

NorvesLib is the first reference engine adapter candidate. Its adapter is responsible for:

```text
- Mapping NorvesLib runtime/log/status into Bridge DTOs.
- Marshaling runtime commands onto the safe NorvesLib thread/context.
- Avoiding direct transport of NorvesLib live object memory.
- Keeping NorvesLib-specific containers and object rules out of the generic SDK.
```

If the adapter lives in the NorvesLib repository, follow NorvesLib repository rules there. If an integration shim is needed in NorvesEditor, isolate it from the generic C++ SDK.

## Mock Engine First

Early implementation should prove protocol and transport behavior with a mock engine before binding to NorvesLib. Mock-engine parity is acceptable for internal alpha progress; public alpha should include NorvesLib integration or a documented adapter task list.
