// viewport.getThumbnail method result types.
//
// Shapes mirror the positive fixture
// bridge/spec/fixtures/methods/viewport.getThumbnail/positive/response-valid.json
// and the schema methods/viewport.getThumbnail.result.schema.json
// (result = { imageBase64, mimeType, width?, height? }).
//
// Generic protocol: ViewportThumbnail carries no engine-specific semantics;
// mimeType is a free-form token and imageBase64 is an opaque base64 snapshot of
// the engine's framebuffer (a snapshot copy, never a live engine pointer). The
// large-payload limits (PNG, max 640x360, 256 KiB hard cap, pull-style, <= 1 fps)
// are documented in docs/memory-buffer-policy.md. Mirrors the Rust
// ViewportThumbnail in
// bridge/crates/norves-bridge-editor-client/src/viewport.rs.

/**
 * Result of the `viewport.getThumbnail` method: a still thumbnail of the engine's
 * external viewport. A DTO copy of generic values, never a live engine pointer.
 *
 * `imageBase64` and `mimeType` are required; `width`/`height` are optional decoded
 * dimensions.
 */
export interface ViewportThumbnail {
  /** Base64-encoded image bytes (a snapshot copy). */
  imageBase64: string;
  /** MIME type of the encoded image, e.g. "image/png". */
  mimeType: string;
  /** Optional decoded width in pixels. */
  width?: number;
  /** Optional decoded height in pixels. */
  height?: number;
}

/** 撮影画像の幅の上限。 */
export const CAPTURE_MAX_WIDTH = 1920;
/** 撮影画像の高さの上限。 */
export const CAPTURE_MAX_HEIGHT = 1080;
/** 描画前に進める固定刻みの上限。 */
export const CAPTURE_MAX_SETTLE_FRAMES = 600;
/** 撮影した PNG ファイルのバイト数の上限。 */
export const CAPTURE_MAX_FILE_BYTES = 32 * 1024 * 1024;

/**
 * 決定的な撮影の要求。値の形式と範囲は schema に従う。
 * schema: bridge/spec/schema/methods/viewport.captureDeterministic.params.schema.json
 * 正例: bridge/spec/fixtures/methods/viewport.captureDeterministic/positive/request-valid.json
 */
export interface ViewportCaptureDeterministicParams {
  /** エディタが作成した撮影先ディレクトリの絶対パス。1〜1024 文字。 */
  captureDir: string;
  /** 拡張子なしのファイル名。`^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$` に従う。 */
  outputName: string;
  /** 撮影エポックで描画前に進める固定刻み。0〜600 の整数。 */
  settleFrames: number;
  /** 出力画像の最大幅。1〜1920 の整数。 */
  maxWidth: number;
  /** 出力画像の最大高さ。1〜1080 の整数。 */
  maxHeight: number;
}

/**
 * 撮影要求の受け付け結果。完了は別のイベントで通知される。
 * schema: bridge/spec/schema/methods/viewport.captureDeterministic.result.schema.json
 * 正例: bridge/spec/fixtures/methods/viewport.captureDeterministic/positive/response-valid.json
 */
export interface ViewportCaptureDeterministicResult {
  /** 完了イベントとの対応に使う識別子。`^[A-Za-z0-9._:-]{1,128}$` に従う。 */
  captureId: string;
}

/** PNG 符号化前の、上から下へ並べた詰め物なし RGBA8 画素の FNV-1a 64 ビット要約値。 */
export interface CapturePixelDigest {
  algorithm: 'fnv1a64-rgba8';
  /** 小文字の 16 桁の 16 進数。 */
  value: string;
}

/** 撮影失敗の詳細。 */
export interface CaptureFailure {
  /** `^[A-Z][A-Z0-9_]*$` に従うコード。既知の値は `OUTPUT_WRITE_FAILED` と `RENDER_FAILED`。 */
  code: string;
  /** 1 文字以上の失敗理由。 */
  message: string;
}

/**
 * 撮影の完了イベントの params。成功時は画像ファイルの情報、失敗時は理由を持つ。
 * schema: bridge/spec/schema/events/viewport.captureCompleted.params.schema.json
 * 正例: bridge/spec/fixtures/events/viewport.captureCompleted/positive/event-succeeded.json
 * 正例: bridge/spec/fixtures/events/viewport.captureCompleted/positive/event-failed.json
 */
export type ViewportCaptureCompletedEvent =
  | {
      /** 受け付け結果と同じ識別子。`^[A-Za-z0-9._:-]{1,128}$` に従う。 */
      captureId: string;
      status: 'succeeded';
      /** 書き込んだ PNG ファイルの絶対パス。1〜2048 文字。 */
      path: string;
      /** 実際の画像幅。1〜1920 の整数で、要求の maxWidth 以下。 */
      width: number;
      /** 実際の画像の高さ。1〜1080 の整数で、要求の maxHeight 以下。 */
      height: number;
      /** 撮影エポック内の固定刻み。0 以上の整数で、要求の settleFrames と等しい。 */
      frame: number;
      pixelDigest: CapturePixelDigest;
      /** 書き込んだ PNG のバイト数。1〜33554432 の整数。 */
      fileBytes: number;
      failure?: never;
    }
  | {
      /** 受け付け結果と同じ識別子。`^[A-Za-z0-9._:-]{1,128}$` に従う。 */
      captureId: string;
      status: 'failed';
      failure: CaptureFailure;
      path?: never;
      width?: never;
      height?: never;
      frame?: never;
      pixelDigest?: never;
      fileBytes?: never;
    };
