# Technology Decisions

These are alpha-level decisions extracted from `docs/alpha-project-plan.md`. Version pinning and dependency selection must be re-checked against current official documentation when implementation starts.

| Area | Alpha Decision | Notes |
| --- | --- | --- |
| App shell | Tauri 2 | Native backend integration with a web UI surface. |
| Editor backend | Rust + Tokio | Owns process lifecycle, Bridge sessions, reconnect, and event fan-out. |
| Editor frontend | TypeScript + Vite + React by default | ADR may change the frontend framework before implementation. |
| Bridge transport | WebSocket | Local reliable bidirectional control channel. |
| Bridge codec | JSON text frames | Canonical debug format for alpha. |
| Protocol validation | JSON Schema + golden fixtures | Cross-language contract for Rust, TypeScript, and C++. |
| Engine SDK | Standalone C++ SDK | Does not depend on NorvesLib, Tauri, React, or TypeScript. |
| C++ language | C++20 minimum | C++23 requires ADR or explicit toolchain policy. |
| Viewport | External engine window | Native embedding and frame streaming are post-alpha research. |

Do not expose selected WebSocket library types through public SDK APIs. Choose the C++ WebSocket backend by ADR before implementing transport.
