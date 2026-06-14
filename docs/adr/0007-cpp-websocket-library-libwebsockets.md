# 0007: C++ WebSocket Library — libwebsockets

Status: Accepted for alpha

## Context

ADR 0003 settled the Bridge transport as a **WebSocket + JSON control channel**,
but it did not decide *how* the C++ engine side implements its WebSocket
**server**. That choice was left open.

Candidates considered were libwebsockets, Boost.Beast, and IXWebSocket. The alpha
scope narrows the requirements substantially:

- **localhost only**, no TLS, a **single editor client** connection at a time.
- No remote connectivity and no multi-tenant fan-out are in scope.

Workstream F established the C++ SDK boundary requirements that any transport
implementation must respect: **public headers must not expose third-party types**
(third-party concerns are hidden behind `ITransport`, following the precedent of
linking `nlohmann` PRIVATE into the source translation units rather than leaking
it through `include/`).

## Decision

1. **Library choice.** The C++ engine side WebSocket server uses
   **libwebsockets**. This is a confirmed decision by the **project owner (the
   user)**, not an open trade-off to be re-litigated by implementers.
2. **Acquisition.** libwebsockets is fetched via **CMake `FetchContent`**
   (vcpkg is **not** used). The dependency is pinned to a **fixed tag,
   `GIT_TAG v4.3.3`** — branch names are forbidden so the build is reproducible.
3. **Type containment.** libwebsockets types (`lws_context*`, `struct lws*`,
   etc.) are confined to the `ITransport` implementation's `.cpp` and must
   **never** appear in any public header (pImpl + PRIVATE link). The invariant is
   that `grep` for `lws_|libwebsockets|websockets` under `include/` yields **0
   hits**.
4. **Threading discipline (normative).** libwebsockets follows a **single service
   thread** discipline:
   - External threads must **not** touch `wsi` or `lws_context` directly. They may
     only **push onto a send queue and call `lws_cancel_service`**.
   - `lws_callback_on_writable` and the actual `lws_write` happen **only inside
     service-thread callbacks**.
   - Send buffers must reserve **`LWS_PRE` bytes of padding** at the head.
   - `lws_write` **partial writes** are handled by re-arming in
     `LWS_CALLBACK_SERVER_WRITEABLE`.
   - A `wsi` is referenced **only inside callbacks** and is never retained
     externally, to avoid dangling use after `LWS_CALLBACK_CLOSED`.
5. **Binding.** The server binds **`127.0.0.1` only** (`0.0.0.0` is forbidden).
   **TLS is disabled** (localhost-only).
6. **Minimal libwebsockets CMake options.** The dependency is built minimally:
   `LWS_WITH_SSL=OFF`, `LWS_WITHOUT_TESTAPPS=ON`,
   `LWS_WITHOUT_TEST_SERVER=ON`, `LWS_WITHOUT_TEST_SERVER_EXTPOLL=ON`,
   `LWS_WITHOUT_TEST_PING=ON`, `LWS_WITHOUT_TEST_CLIENT=ON`,
   `LWS_WITH_MINIMAL_EXAMPLES=OFF`, `LWS_WITH_STATIC=ON`,
   `LWS_WITH_SHARED=OFF`, `LWS_WITHOUT_EXTENSIONS=ON`,
   `LWS_WITH_LIBUV=OFF`, `LWS_WITH_LIBEV=OFF`, `LWS_WITH_LIBEVENT=OFF`,
   `LWS_WITH_GLIB=OFF`, `LWS_WITH_EXTERNAL_POLL=OFF`, `LWS_WITH_ZLIB=OFF`,
   `LWS_WITH_MBEDTLS=OFF`.

## Consequences

- **Benefits.** libwebsockets is a widely used C library, has light dependencies,
  builds as a static library, and is obtained via `FetchContent` so vcpkg is not
  required.
- **Costs / risks (verified in the G1 spike).**
  - The Windows/MSVC build is comparatively involved. The libwebsockets CMake
    unconditionally applies `/W3 /WX` under MSVC, and in a **CP932 (Japanese)**
    environment its UTF-8 sources trigger **C4819**, which `/WX` promotes to a
    fatal error. The fix is to add **`/utf-8`** from the integrating side;
    `DISABLE_WERROR` does **not** help because that path is on the non-MSVC
    branch.
  - `GENHDR` emits an **MSB8065** warning.
  - The **first configure takes about 53 seconds** (including the shallow clone).
- The server implementation's quality now depends on libwebsockets.
- With TLS disabled, remote connections are impossible — this is intentional and
  matches the alpha scope.
- `FetchContent` artifacts live under `build/_deps` and are **not committed**; the
  first configure requires network access.

## Affected workstreams

- **G (this decision):** the C++ engine WebSocket server transport.
- **H (mock engine):** uses the WS server transport.
- **J (engine process lifecycle / endpoint injection):** launches the engine and
  injects the endpoint.

## Verification or migration notes

- The **G1 spike** confirmed configure → build → run green across **three
  conditions**: multiple frames sent back-to-back, partial-write re-arming, and
  multiple connections (libwebsockets v4.3.3, generator
  "Visual Studio 17 2022").
- Bridge fixtures are **unchanged**: `python scripts/validate-bridge-fixtures.py`
  passes.
- If TLS or remote connectivity is needed later, it must be revisited in a new
  ADR. Moving the acquisition method to a submodule or vcpkg likewise requires an
  ADR revision.
