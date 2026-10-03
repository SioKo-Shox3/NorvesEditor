# Memory And Buffer Policy

The Bridge is an editor connection channel. Small control messages may be copied, but APIs must preserve explicit ownership and lifetime rules so later optimizations remain possible.

## Required Rules

```text
- Engine live memory is never passed directly to transport.
- Engine adapters convert engine state into snapshots, DTOs, or serialized values.
- Public API ownership must be explicit.
- Borrowed views are valid only for the documented callback scope.
- Owned buffers remain valid until send completion, release, or explicit drop.
- Large payloads require size limits and queue limits.
- Attachment or streaming strategy must be documented before adding large payload paths.
- Public APIs do not expose third-party WebSocket buffer types.
```

## Review Questions

Use these questions for protocol, SDK, and runtime reviews:

```text
- Who owns this buffer?
- Does it live after the callback returns?
- Does it cross a thread boundary?
- Can it be queued, and if so what is the maximum queue size?
- What releases it on failure or disconnect?
- Are raw pointer, string_view, or span lifetimes documented?
```

Alpha does not optimize for zero-copy transport. It does optimize for safe, reviewable ownership boundaries.

## Large-payload strategy: viewport thumbnails

`viewport.getThumbnail`（protocol 0.2）は、エンジンの外部viewportの静止画を
method result内のbase64文字列として返すBridgeメソッドです。Bridgeの添付方式に加え、
MCPは同じsnapshotをエディタbackend経由で取得し、検査・上限処理済みのPNG画像を返します。

```text
- Transport mode:  pull (request/response), never push.
- Image format:    PNG (lossless; mimeType allows a future JPEG switch).
- Max resolution:  640 x 360 (16:9 thumbnail). Larger frames are downscaled by
                   the engine before encoding.
- Hard byte cap:   256 KiB for the raw image bytes (~342 KiB once base64-encoded).
- Max frequency:   <= 1 fps (the UI polls no faster than once per 1000 ms, and
                   only while the panel is visible). No continuous streaming.
- Attachment:      the image is carried inline as a base64 string field
                   (imageBase64) in the JSON result. No out-of-band channel.
- Ownership:       the engine adapter builds the base64 string by value from a
                   snapshot of its framebuffer; it never hands a live engine
                   pointer, span, or framebuffer view to the transport.
```

### Why pull, not push

A push event (`viewport.frame`) would flow through the editor's event broadcast
ring, which is a bounded queue. A high-frequency frame stream would overrun that
ring and force the relay to drop frames (`Lagged`). A pull-style method response
is correlated 1:1 with a request and does **not** travel through the broadcast
ring, so it cannot starve unrelated events. The UI therefore drives cadence
explicitly and never asks faster than 1 fps.

### Why 256 KiB is the cap (and what it is NOT compared against)

The cap is justified against the **WebSocket frame / JSON envelope practical
limit**: a thumbnail result is a single JSON message carried in one WebSocket
text frame, and 256 KiB of image bytes (~342 KiB base64, plus a few hundred bytes
of envelope) stays comfortably within a single frame the transport handles
without fragmentation concerns. 256 KiB is deliberately generous for a
640 x 360 PNG while remaining a hard safety ceiling: an engine that cannot meet
it must downscale further or return an error rather than emit an oversized frame.

This cap is **not** related to `EVENT_BROADCAST_CAPACITY` (the broadcast ring's
**stage count** — how many events may be buffered — not a byte budget). The pull
response does not pass through that ring at all; conflating the two is a category
error. The byte cap governs a single response payload; the broadcast capacity
governs how many small event messages may queue.

### Frequency and continuous streaming

Continuous frame streaming, shared GPU textures, and native window embedding are
explicitly out of scope (see `docs/viewport-strategy.md`, "Post-Alpha Research").
The thumbnail path is a low-frequency still image only.

## MCP画像処理の上限と所有権

MCPは既存の`viewport.getThumbnail` Bridge要求を使います。エディタbackendはGame Viewと
MCPで、進行中の要求と接続世代ごとの1秒snapshot cacheを共有します。共通サービスから
Bridgeへ送る頻度は毎秒1回以下です。MCP要求の失敗はGame Viewの取得状態や再試行間隔に
反映しません。

MCPはPNG形式だけを受け付けます。復号前にbase64形式、Bridgeの256 KiB転送データ上限、PNG署名と
IHDR寸法を検査します。宣言された幅・高さはIHDRと一致させます。寸法は0より大きく、
640 x 360以内であることを確認し、画像復号器が画像領域を確保する前に寸法爆弾を拒否します。
Bridgeの上限は640 x 360 / 256 KiBのままです。MCPは長辺を512 pixel以下へ縮小し、再符号化
したPNGを512 KiB以下にします。MCPの`image/png` contentにはbase64 dataと`mimeType`を載せます。

PNGの復号・縮小・符号化は同期処理workerで行い、同時処理を最大2件に制限します。workerが
両方使用中なら新しい処理を受け付けません。各workerは復号前に6 MiBを予約し、合計128 MiBの
共有処理予算内で動きます。寸法・データ量の検査が割当て量の主要な上限です。画像ライブラリの
`max_alloc`は追加の防御として設定し、それだけを根拠にしません。停止時は受付を閉じ、実行中の
処理が終了するまで待ちます。

Bridge resultは所有権を持つJSON snapshotです。復号は所有pixel bufferを作り、縮小と符号化は
MCP用の所有byte列を作り、base64化で応答用の所有文字列を作ります。これらのbufferはエンジンの
live memoryを参照せず、cacheの期限切れまたは応答送信後に解放されます。画像はBridgeのevent
broadcast ringへ流しません。

## MCP時刻付き画像一覧の上限と所有権

画像一覧ヘルパーは、呼び出し元が所有するPNG byte列を借りて処理し、一覧PNGと説明用テキストを
新たな所有値として返します。撮影要求、ファイル読み込み、Bridge通信は行いません。入力は1〜16枚、
各PNGは2 MiB以下、各辺4096 pixel以下、合計16777216 pixel以下です。時刻は
HH:MM:SS.mmm（分・秒は00〜59）に限定します。

一覧は入力順の4列配置で、各枠に1始まりの番号と時刻を描画します。文字は数字・コロン・小数点の
固定5×7 pixel字形を使い、フォントやOSの描画差を持ち込みません。返すテキストにも同じ順番と
時刻を記載します。出力の長辺は2048 pixel以下、PNGは2 MiB以下です。

画像の展開は1枚ずつ行い、PNGの寸法・色形式を復号前に検査します。作業量の見積りには、全入力PNG
(最大32 MiB)、最大のRGBA展開画像(最大64 MiB)、一覧canvas(最大16 MiB)、縮小画像(2 MiB)、
出力PNG用buffer(4 MiB)、decoder作業領域(8 MiB)を含めます。合計は最大126 MiBで、128 MiBを超える見積りは
処理前に拒否します。入力bufferは呼び出し元が所有し、出力PNGの内容は2 MiBで打ち切ります。
PNGは8 bit以下の形式を受け付け、16 bit形式はこの作業量上限の対象外として拒否します。
