//! Game View と MCP で共有する取得器と、上限付きPNG処理。

use std::{
    io::Cursor,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use image::{imageops::FilterType, DynamicImage, ImageFormat, ImageReader, Limits};
use serde_json::{Map, Value};
use tokio::sync::{watch, Mutex, OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::CancellationToken;

use crate::{bridge_state::BridgeFacade, error::BackendError};

/// Bridgeが受け付けるPNGの画像データ上限。
pub(crate) const BRIDGE_THUMBNAIL_MAX_BYTES: usize = 256 * 1024;
/// Bridgeが受け付けるthumbnailの幅上限。
pub(crate) const BRIDGE_THUMBNAIL_MAX_WIDTH: u32 = 640;
/// Bridgeが受け付けるthumbnailの高さ上限。
pub(crate) const BRIDGE_THUMBNAIL_MAX_HEIGHT: u32 = 360;
/// MCP画像の長辺上限。
pub(crate) const MCP_THUMBNAIL_MAX_LONG_EDGE: u32 = 512;
/// MCPへ返すPNGの上限。
pub(crate) const MCP_THUMBNAIL_MAX_BYTES: usize = 512 * 1024;
/// 同時に保持する画像処理用メモリの上限。
pub(crate) const MCP_IMAGE_MEMORY_BUDGET_BYTES: usize = 128 * 1024 * 1024;
/// 同時に動かすPNG処理workerの上限。
pub(crate) const MCP_IMAGE_WORKER_COUNT: usize = 2;

const THUMBNAIL_CACHE_TTL: Duration = Duration::from_secs(1);
const IMAGE_MEMORY_UNIT_BYTES: usize = 1024 * 1024;
const IMAGE_WORKING_SET_PER_WORKER_BYTES: usize = 6 * IMAGE_MEMORY_UNIT_BYTES;
const IMAGE_MEMORY_UNITS_PER_WORKER: u32 =
    (IMAGE_WORKING_SET_PER_WORKER_BYTES / IMAGE_MEMORY_UNIT_BYTES) as u32;
const IMAGE_DECODER_MAX_ALLOC_BYTES: u64 = 4 * 1024 * 1024;
const MAX_ENCODED_BRIDGE_BYTES: usize = BRIDGE_THUMBNAIL_MAX_BYTES.div_ceil(3) * 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ThumbnailRequest {
    max_width: u32,
    max_height: u32,
}

impl ThumbnailRequest {
    fn from_options(max_width: Option<u32>, max_height: Option<u32>) -> Result<Self, BackendError> {
        let max_width = max_width.unwrap_or(BRIDGE_THUMBNAIL_MAX_WIDTH);
        let max_height = max_height.unwrap_or(BRIDGE_THUMBNAIL_MAX_HEIGHT);
        if max_width == 0
            || max_height == 0
            || max_width > BRIDGE_THUMBNAIL_MAX_WIDTH
            || max_height > BRIDGE_THUMBNAIL_MAX_HEIGHT
        {
            return Err(BackendError::Request {
                message: "thumbnailの寸法は640×360以内で指定してください。".to_owned(),
            });
        }
        Ok(Self {
            max_width,
            max_height,
        })
    }
}

/// Bridge応答の所有済みsnapshot。
#[derive(Clone)]
pub(crate) struct ThumbnailSnapshot {
    pub(crate) generation: u64,
    fetched_at: Instant,
    request: ThumbnailRequest,
    value: Value,
}

impl ThumbnailSnapshot {
    pub(crate) fn value(&self) -> &Value {
        &self.value
    }
}

#[derive(Clone)]
enum SharedThumbnailError {
    NotConnected,
    Request(String),
    Engine { code: String, message: String },
}

impl From<BackendError> for SharedThumbnailError {
    fn from(error: BackendError) -> Self {
        match error {
            BackendError::NotConnected => Self::NotConnected,
            BackendError::Request { message } => Self::Request(message),
            BackendError::Engine { code, message } => Self::Engine { code, message },
            other => Self::Request(other.to_string()),
        }
    }
}

impl SharedThumbnailError {
    fn into_backend(self) -> BackendError {
        match self {
            Self::NotConnected => BackendError::NotConnected,
            Self::Request(message) => BackendError::Request { message },
            Self::Engine { code, message } => BackendError::Engine { code, message },
        }
    }
}

/// MCP image contentに載せる所有済みbase64 PNG。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct McpThumbnailImage {
    pub(crate) data: String,
    pub(crate) mime_type: &'static str,
}

#[derive(Clone)]
pub(crate) struct McpThumbnailService {
    inner: Arc<ThumbnailServiceInner>,
}

struct ThumbnailServiceInner {
    fetch: Mutex<FetchState>,
    image_cache: Mutex<Option<(u64, Instant, McpThumbnailImage)>>,
    image_processing: Mutex<()>,
    processor: McpImageProcessor,
}

#[derive(Default)]
struct FetchState {
    generation: Option<u64>,
    last_started: Option<Instant>,
    cached: Option<ThumbnailSnapshot>,
    in_flight: Option<Arc<ThumbnailFlight>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RequestOrigin {
    Ui,
    Mcp,
}

struct ThumbnailFlight {
    generation: u64,
    request: ThumbnailRequest,
    origin: RequestOrigin,
    result_tx: watch::Sender<Option<Result<ThumbnailSnapshot, SharedThumbnailError>>>,
}

enum FetchDecision {
    Cached(ThumbnailSnapshot),
    Wait(Arc<ThumbnailFlight>),
    Start(Arc<ThumbnailFlight>),
    WaitForCadence(Instant),
}

impl ThumbnailFlight {
    fn new(generation: u64, request: ThumbnailRequest, origin: RequestOrigin) -> Self {
        let (result_tx, _) = watch::channel(None);
        Self {
            generation,
            request,
            origin,
            result_tx,
        }
    }

    async fn result(&self) -> Result<ThumbnailSnapshot, SharedThumbnailError> {
        let mut receiver = self.result_tx.subscribe();
        loop {
            if let Some(result) = receiver.borrow().clone() {
                return result;
            }
            if receiver.changed().await.is_err() {
                return Err(SharedThumbnailError::Request(
                    "thumbnail取得が終了しました。".to_owned(),
                ));
            }
        }
    }
}

impl Default for McpThumbnailService {
    fn default() -> Self {
        Self {
            inner: Arc::new(ThumbnailServiceInner {
                fetch: Mutex::new(FetchState::default()),
                image_cache: Mutex::new(None),
                image_processing: Mutex::new(()),
                processor: McpImageProcessor::default(),
            }),
        }
    }
}

impl McpThumbnailService {
    /// UI要求とMCP要求で同じBridge取得と世代別1秒cacheを使う。
    pub(crate) async fn get_raw(
        &self,
        bridge: &BridgeFacade,
        max_width: Option<u32>,
        max_height: Option<u32>,
        origin: RequestOrigin,
    ) -> Result<ThumbnailSnapshot, BackendError> {
        let request = ThumbnailRequest::from_options(max_width, max_height)?;
        let lease = bridge.pin()?;

        loop {
            let decision = {
                let mut state = self.inner.fetch.lock().await;
                if state.generation != Some(lease.generation) {
                    state.generation = Some(lease.generation);
                    state.cached = None;
                    state.in_flight = None;
                    *self.inner.image_cache.lock().await = None;
                }
                decide_fetch(
                    &mut state,
                    lease.generation,
                    request,
                    origin,
                    Instant::now(),
                )
            };

            match decision {
                FetchDecision::Cached(snapshot) => {
                    return checked_current(bridge, snapshot);
                }
                FetchDecision::Wait(flight) => match flight.result().await {
                    Ok(snapshot) if snapshot.request == request => {
                        return checked_current(bridge, snapshot)
                    }
                    Ok(_) if origin == RequestOrigin::Ui => continue,
                    Ok(_) => {
                        return Err(BackendError::Request {
                            message: "thumbnailの要求寸法を毎秒内で変更できません。".to_owned(),
                        })
                    }
                    Err(_)
                        if origin == RequestOrigin::Ui && flight.origin == RequestOrigin::Mcp =>
                    {
                        continue;
                    }
                    Err(error) => return Err(error.into_backend()),
                },
                FetchDecision::Start(flight) => {
                    let service = self.clone();
                    let fetch_bridge = bridge.clone();
                    let lease = lease.clone();
                    let worker_flight = Arc::clone(&flight);
                    tokio::spawn(async move {
                        service
                            .complete_fetch(fetch_bridge, lease, worker_flight)
                            .await;
                    });
                    match flight.result().await {
                        Ok(snapshot) if snapshot.request == request => {
                            return checked_current(bridge, snapshot)
                        }
                        Ok(_) if origin == RequestOrigin::Ui => continue,
                        Ok(_) => {
                            return Err(BackendError::Request {
                                message: "thumbnailの要求寸法を毎秒内で変更できません。".to_owned(),
                            })
                        }
                        Err(_)
                            if origin == RequestOrigin::Ui
                                && flight.origin == RequestOrigin::Mcp =>
                        {
                            continue;
                        }
                        Err(error) => return Err(error.into_backend()),
                    }
                }
                FetchDecision::WaitForCadence(retry_at) => {
                    if origin == RequestOrigin::Mcp {
                        return Err(BackendError::Request {
                            message: "thumbnailは毎秒1回までです。直前の取得を再利用できません。"
                                .to_owned(),
                        });
                    }
                    tokio::time::sleep_until(tokio::time::Instant::from_std(retry_at)).await;
                }
            }
        }
    }

    /// 最新snapshotをPNGとして検査・縮小し、MCP用image contentへ変換する。
    pub(crate) async fn get_mcp_image(
        &self,
        snapshot: &ThumbnailSnapshot,
        bridge: &BridgeFacade,
    ) -> Result<McpThumbnailImage, BackendError> {
        if !bridge.is_current(snapshot.generation) {
            return Err(BackendError::NotConnected);
        }
        let _processing = self.inner.image_processing.lock().await;
        if let Some((generation, fetched_at, image)) = self.inner.image_cache.lock().await.as_ref()
        {
            if *generation == snapshot.generation
                && *fetched_at == snapshot.fetched_at
                && fetched_at.elapsed() <= THUMBNAIL_CACHE_TTL
            {
                return Ok(image.clone());
            }
        }

        let thumbnail = norves_bridge_editor_client::parse_thumbnail_result(snapshot.value())
            .map_err(|error| BackendError::Request {
                message: format!("viewport.getThumbnailの応答形式が不正です: {error}"),
            })?;
        let image = self.inner.processor.process(thumbnail).await?;
        if !bridge.is_current(snapshot.generation) {
            return Err(BackendError::NotConnected);
        }
        *self.inner.image_cache.lock().await =
            Some((snapshot.generation, snapshot.fetched_at, image.clone()));
        Ok(image)
    }

    /// アプリ終了時に画像処理workerを閉じ、実行中の処理が抜けるまで待つ。
    pub(crate) async fn shutdown(&self) {
        self.inner.processor.shutdown().await;
        self.invalidate().await;
    }

    /// 接続が切れたときに旧世代の画像を解放し、次の接続でもworkerを使える状態に保つ。
    pub(crate) async fn invalidate(&self) {
        {
            let mut state = self.inner.fetch.lock().await;
            state.generation = None;
            state.last_started = None;
            state.cached = None;
            state.in_flight = None;
        }
        *self.inner.image_cache.lock().await = None;
    }

    #[cfg(test)]
    pub(crate) async fn in_flight_waiter_count(&self) -> usize {
        self.inner
            .fetch
            .lock()
            .await
            .in_flight
            .as_ref()
            .map_or(0, |flight| flight.result_tx.receiver_count())
    }

    async fn complete_fetch(
        &self,
        bridge: BridgeFacade,
        lease: crate::bridge_state::BridgeLease,
        flight: Arc<ThumbnailFlight>,
    ) {
        let mut params = Map::new();
        params.insert("maxWidth".to_owned(), Value::from(flight.request.max_width));
        params.insert(
            "maxHeight".to_owned(),
            Value::from(flight.request.max_height),
        );
        let result = bridge
            .send_with_lease(&lease, "viewport.getThumbnail", Some(params))
            .await
            .map_err(SharedThumbnailError::from)
            .and_then(|value| {
                norves_bridge_editor_client::parse_thumbnail_result(&value).map_err(|error| {
                    SharedThumbnailError::Request(format!(
                        "viewport.getThumbnailの応答形式が不正です: {error}"
                    ))
                })?;
                if !bridge.is_current(lease.generation) {
                    return Err(SharedThumbnailError::NotConnected);
                }
                Ok(ThumbnailSnapshot {
                    generation: lease.generation,
                    fetched_at: Instant::now(),
                    request: flight.request,
                    value,
                })
            });

        {
            let mut state = self.inner.fetch.lock().await;
            if state.generation == Some(flight.generation)
                && state
                    .in_flight
                    .as_ref()
                    .is_some_and(|active| Arc::ptr_eq(active, &flight))
            {
                state.in_flight = None;
                if let Ok(snapshot) = &result {
                    state.cached = Some(snapshot.clone());
                }
            }
        }
        flight.result_tx.send_replace(Some(result));
    }
}

fn checked_current(
    bridge: &BridgeFacade,
    snapshot: ThumbnailSnapshot,
) -> Result<ThumbnailSnapshot, BackendError> {
    if bridge.is_current(snapshot.generation) {
        Ok(snapshot)
    } else {
        Err(BackendError::NotConnected)
    }
}

fn decide_fetch(
    state: &mut FetchState,
    generation: u64,
    request: ThumbnailRequest,
    origin: RequestOrigin,
    now: Instant,
) -> FetchDecision {
    if let Some(cached) = &state.cached {
        if cached.generation == generation
            && cached.request == request
            && now.saturating_duration_since(cached.fetched_at) <= THUMBNAIL_CACHE_TTL
        {
            return FetchDecision::Cached(cached.clone());
        }
    }
    if let Some(flight) = &state.in_flight {
        if flight.generation == generation {
            return FetchDecision::Wait(Arc::clone(flight));
        }
    }
    if let Some(last_started) = state.last_started {
        let retry_at = last_started + THUMBNAIL_CACHE_TTL;
        if now < retry_at {
            return FetchDecision::WaitForCadence(retry_at);
        }
    }
    let flight = Arc::new(ThumbnailFlight::new(generation, request, origin));
    state.last_started = Some(now);
    state.in_flight = Some(Arc::clone(&flight));
    FetchDecision::Start(flight)
}

#[derive(Clone)]
pub(crate) struct McpImageProcessor {
    inner: Arc<McpImageProcessorInner>,
}

struct McpImageProcessorInner {
    workers: Arc<Semaphore>,
    memory: Arc<Semaphore>,
    stopped: AtomicBool,
    shutdown: CancellationToken,
    process: Arc<ProcessImageFn>,
}

type ProcessImageFn = dyn Fn(norves_bridge_editor_client::ViewportThumbnail) -> Result<McpThumbnailImage, BackendError>
    + Send
    + Sync;

impl Default for McpImageProcessor {
    fn default() -> Self {
        Self::with_process(process_png_thumbnail)
    }
}

impl McpImageProcessor {
    fn with_process(
        process: impl Fn(
                norves_bridge_editor_client::ViewportThumbnail,
            ) -> Result<McpThumbnailImage, BackendError>
            + Send
            + Sync
            + 'static,
    ) -> Self {
        Self {
            inner: Arc::new(McpImageProcessorInner {
                workers: Arc::new(Semaphore::new(MCP_IMAGE_WORKER_COUNT)),
                memory: Arc::new(Semaphore::new(
                    MCP_IMAGE_MEMORY_BUDGET_BYTES / IMAGE_MEMORY_UNIT_BYTES,
                )),
                stopped: AtomicBool::new(false),
                shutdown: CancellationToken::new(),
                process: Arc::new(process),
            }),
        }
    }

    async fn process(
        &self,
        thumbnail: norves_bridge_editor_client::ViewportThumbnail,
    ) -> Result<McpThumbnailImage, BackendError> {
        if self.inner.stopped.load(Ordering::Acquire) {
            return Err(image_worker_stopped());
        }
        let worker = self
            .inner
            .workers
            .clone()
            .try_acquire_owned()
            .map_err(|_| image_worker_busy())?;
        let memory = self
            .inner
            .memory
            .clone()
            .try_acquire_many_owned(IMAGE_MEMORY_UNITS_PER_WORKER)
            .map_err(|_| image_budget_exceeded())?;
        let process = Arc::clone(&self.inner.process);
        let mut job = tokio::task::spawn_blocking(move || {
            let _worker: OwnedSemaphorePermit = worker;
            let _memory: OwnedSemaphorePermit = memory;
            process(thumbnail)
        });
        tokio::select! {
            result = &mut job => result.map_err(|_| image_worker_failed())?,
            _ = self.inner.shutdown.cancelled() => {
                let _ = job.await;
                Err(image_worker_stopped())
            }
        }
    }

    async fn shutdown(&self) {
        self.inner.stopped.store(true, Ordering::Release);
        self.inner.shutdown.cancel();
        if let Ok(permits) = self
            .inner
            .workers
            .clone()
            .acquire_many_owned(MCP_IMAGE_WORKER_COUNT as u32)
            .await
        {
            self.inner.workers.close();
            self.inner.memory.close();
            drop(permits);
        }
    }
}

fn image_worker_busy() -> BackendError {
    BackendError::Request {
        message: "MCP画像処理workerが使用中です。少し待ってから再試行してください。".to_owned(),
    }
}

fn image_worker_stopped() -> BackendError {
    BackendError::Request {
        message: "MCP画像処理workerは停止しています。".to_owned(),
    }
}

fn image_worker_failed() -> BackendError {
    BackendError::Request {
        message: "MCP画像を処理できませんでした。".to_owned(),
    }
}

fn image_budget_exceeded() -> BackendError {
    BackendError::Request {
        message: "MCP画像処理のメモリ予算を超えました。".to_owned(),
    }
}

fn image_processing_error(message: &'static str) -> BackendError {
    BackendError::Request {
        message: message.to_owned(),
    }
}

fn process_png_thumbnail(
    thumbnail: norves_bridge_editor_client::ViewportThumbnail,
) -> Result<McpThumbnailImage, BackendError> {
    if thumbnail.mime_type != "image/png" {
        return Err(image_processing_error(
            "MCP画像はimage/png形式だけを受け付けます。",
        ));
    }
    if thumbnail.image_base64.len() > MAX_ENCODED_BRIDGE_BYTES {
        return Err(image_processing_error(
            "Bridge画像のbase64データが256 KiBの上限を超えています。",
        ));
    }
    let decoded = STANDARD
        .decode(thumbnail.image_base64.as_bytes())
        .map_err(|_| image_processing_error("Bridge画像のbase64形式が不正です。"))?;
    if decoded.len() > BRIDGE_THUMBNAIL_MAX_BYTES {
        return Err(image_processing_error(
            "Bridge画像データが256 KiBの上限を超えています。",
        ));
    }

    let (width, height) = png_dimensions(&decoded)?;
    if thumbnail.width.is_some_and(|declared| declared != width)
        || thumbnail.height.is_some_and(|declared| declared != height)
    {
        return Err(image_processing_error(
            "thumbnailの宣言寸法とPNGの寸法が一致しません。",
        ));
    }
    if width == 0
        || height == 0
        || width > BRIDGE_THUMBNAIL_MAX_WIDTH
        || height > BRIDGE_THUMBNAIL_MAX_HEIGHT
    {
        return Err(image_processing_error(
            "PNG寸法が640×360のBridge上限を超えています。",
        ));
    }
    let pixels = u64::from(width)
        .checked_mul(u64::from(height))
        .ok_or_else(|| image_processing_error("PNGの画素数を計算できません。"))?;
    let raster_bytes = pixels
        .checked_mul(4)
        .ok_or_else(|| image_processing_error("PNGの展開サイズを計算できません。"))?;
    let reserved_bytes = raster_bytes
        .checked_mul(4)
        .and_then(|bytes| bytes.checked_add(BRIDGE_THUMBNAIL_MAX_BYTES as u64))
        .and_then(|bytes| bytes.checked_add(MCP_THUMBNAIL_MAX_BYTES as u64))
        .ok_or_else(image_budget_exceeded)?;
    if reserved_bytes > IMAGE_WORKING_SET_PER_WORKER_BYTES as u64 {
        return Err(image_budget_exceeded());
    }

    let mut reader = ImageReader::with_format(Cursor::new(decoded.as_slice()), ImageFormat::Png);
    let mut limits = Limits::default();
    limits.max_image_width = Some(BRIDGE_THUMBNAIL_MAX_WIDTH);
    limits.max_image_height = Some(BRIDGE_THUMBNAIL_MAX_HEIGHT);
    limits.max_alloc = Some(IMAGE_DECODER_MAX_ALLOC_BYTES);
    reader.limits(limits);
    let decoded_image = reader
        .decode()
        .map_err(|_| image_processing_error("PNG画像を復号できません。"))?;
    if decoded_image.width() != width || decoded_image.height() != height {
        return Err(image_processing_error(
            "PNGヘッダーと復号後の寸法が一致しません。",
        ));
    }

    let (output_width, output_height) = scaled_dimensions(width, height);
    let source = decoded_image.into_rgba8();
    let output = if (output_width, output_height) == (width, height) {
        source
    } else {
        image::imageops::resize(&source, output_width, output_height, FilterType::Triangle)
    };
    let mut encoded = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(output)
        .write_to(&mut encoded, ImageFormat::Png)
        .map_err(|_| image_processing_error("MCP向けPNGを符号化できません。"))?;
    let png = encoded.into_inner();
    validate_mcp_png_size(png.len())?;
    let data = STANDARD.encode(&png);
    Ok(McpThumbnailImage {
        data,
        mime_type: "image/png",
    })
}

fn validate_mcp_png_size(size: usize) -> Result<(), BackendError> {
    if size > MCP_THUMBNAIL_MAX_BYTES {
        return Err(image_processing_error(
            "MCP向けPNGが512 KiBの上限を超えています。",
        ));
    }
    Ok(())
}

fn png_dimensions(bytes: &[u8]) -> Result<(u32, u32), BackendError> {
    const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    if bytes.len() < 24
        || &bytes[..8] != PNG_SIGNATURE
        || bytes[8..12] != 13u32.to_be_bytes()
        || &bytes[12..16] != b"IHDR"
    {
        return Err(image_processing_error("PNG署名またはIHDRが不正です。"));
    }
    let width = u32::from_be_bytes(
        bytes[16..20]
            .try_into()
            .map_err(|_| image_processing_error("PNGの幅を読み取れません。"))?,
    );
    let height = u32::from_be_bytes(
        bytes[20..24]
            .try_into()
            .map_err(|_| image_processing_error("PNGの高さを読み取れません。"))?,
    );
    Ok((width, height))
}

fn scaled_dimensions(width: u32, height: u32) -> (u32, u32) {
    let longest = width.max(height);
    if longest <= MCP_THUMBNAIL_MAX_LONG_EDGE {
        return (width, height);
    }
    if width >= height {
        (
            MCP_THUMBNAIL_MAX_LONG_EDGE,
            (u64::from(height) * u64::from(MCP_THUMBNAIL_MAX_LONG_EDGE) / u64::from(width)).max(1)
                as u32,
        )
    } else {
        (
            (u64::from(width) * u64::from(MCP_THUMBNAIL_MAX_LONG_EDGE) / u64::from(height)).max(1)
                as u32,
            MCP_THUMBNAIL_MAX_LONG_EDGE,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgba};
    use std::sync::{Condvar, Mutex as TestMutex};

    fn thumbnail_from_png(
        bytes: &[u8],
        width: Option<u32>,
        height: Option<u32>,
    ) -> norves_bridge_editor_client::ViewportThumbnail {
        norves_bridge_editor_client::ViewportThumbnail {
            image_base64: STANDARD.encode(bytes),
            mime_type: "image/png".to_owned(),
            width,
            height,
        }
    }

    fn png(width: u32, height: u32) -> Vec<u8> {
        let image = ImageBuffer::from_fn(width, height, |x, y| {
            Rgba([(x % 251) as u8, (y % 251) as u8, 70, 255])
        });
        let mut cursor = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(image)
            .write_to(&mut cursor, ImageFormat::Png)
            .expect("PNGの試験画像を符号化する");
        cursor.into_inner()
    }

    fn error_message(error: BackendError) -> String {
        match error {
            BackendError::Request { message } => message,
            other => other.to_string(),
        }
    }

    #[test]
    fn png_long_edge_is_scaled_before_mcp_encoding() {
        let input = png(BRIDGE_THUMBNAIL_MAX_WIDTH, BRIDGE_THUMBNAIL_MAX_HEIGHT);
        let output = process_png_thumbnail(thumbnail_from_png(
            &input,
            Some(BRIDGE_THUMBNAIL_MAX_WIDTH),
            Some(BRIDGE_THUMBNAIL_MAX_HEIGHT),
        ))
        .expect("Bridge PNGを読み取る");
        assert_eq!(output.mime_type, "image/png");
        let encoded = STANDARD.decode(output.data).expect("MCP画像のbase64を読む");
        let (width, height) = png_dimensions(&encoded).expect("出力PNGの寸法を読む");
        assert_eq!((width, height), (512, 288));
    }

    #[test]
    fn malformed_png_and_base64_are_rejected() {
        let invalid_png = process_png_thumbnail(thumbnail_from_png(b"not png", None, None))
            .expect_err("PNG以外のデータを拒否する");
        assert!(error_message(invalid_png).contains("IHDR"));

        let invalid_base64 = norves_bridge_editor_client::ViewportThumbnail {
            image_base64: "%%%".to_owned(),
            mime_type: "image/png".to_owned(),
            width: None,
            height: None,
        };
        let error = process_png_thumbnail(invalid_base64).expect_err("不正なbase64を拒否する");
        assert!(error_message(error).contains("base64"));
    }

    #[test]
    fn declared_dimensions_must_match_png_ihdr() {
        let input = png(2, 1);
        let error = process_png_thumbnail(thumbnail_from_png(&input, Some(1), Some(2)))
            .expect_err("宣言寸法とPNG寸法の不一致を拒否する");
        assert!(error_message(error).contains("一致しません"));
    }

    #[test]
    fn dimension_bomb_is_rejected_before_the_decoder_runs() {
        let mut header = b"\x89PNG\r\n\x1a\n".to_vec();
        header.extend_from_slice(&13u32.to_be_bytes());
        header.extend_from_slice(b"IHDR");
        header.extend_from_slice(&u32::MAX.to_be_bytes());
        header.extend_from_slice(&u32::MAX.to_be_bytes());
        let error = process_png_thumbnail(thumbnail_from_png(&header, None, None))
            .expect_err("IHDRの寸法爆弾を復号前に拒否する");
        assert!(error_message(error).contains("640×360"));
    }

    #[test]
    fn bridge_byte_limit_is_rejected_before_png_decoding() {
        let oversized = vec![0u8; BRIDGE_THUMBNAIL_MAX_BYTES + 1];
        let error = process_png_thumbnail(thumbnail_from_png(&oversized, None, None))
            .expect_err("Bridgeのbyte上限を維持する");
        assert!(error_message(error).contains("256 KiB"));
    }

    #[test]
    fn mcp_png_byte_limit_rejects_oversized_encoded_output() {
        validate_mcp_png_size(MCP_THUMBNAIL_MAX_BYTES).expect("MCP PNGの上限ちょうどを受け付ける");
        let error = validate_mcp_png_size(MCP_THUMBNAIL_MAX_BYTES + 1)
            .expect_err("MCP PNGのbyte上限を超える出力を拒否する");
        assert!(error_message(error).contains("512 KiB"));
    }

    #[test]
    fn bridge_dimension_and_byte_limits_remain_strict() {
        assert_eq!(BRIDGE_THUMBNAIL_MAX_WIDTH, 640);
        assert_eq!(BRIDGE_THUMBNAIL_MAX_HEIGHT, 360);
        assert_eq!(BRIDGE_THUMBNAIL_MAX_BYTES, 256 * 1024);
        assert_eq!(
            ThumbnailRequest::from_options(None, None)
                .unwrap()
                .max_width,
            640
        );
        assert!(ThumbnailRequest::from_options(Some(641), None).is_err());
    }

    #[test]
    fn only_the_latest_snapshot_within_one_second_is_reused() {
        let now = Instant::now();
        let request = ThumbnailRequest {
            max_width: BRIDGE_THUMBNAIL_MAX_WIDTH,
            max_height: BRIDGE_THUMBNAIL_MAX_HEIGHT,
        };
        let snapshot = ThumbnailSnapshot {
            generation: 7,
            fetched_at: now,
            request,
            value: Value::Null,
        };
        let mut state = FetchState {
            generation: Some(7),
            last_started: Some(now),
            cached: Some(snapshot),
            in_flight: None,
        };
        assert!(matches!(
            decide_fetch(
                &mut state,
                7,
                request,
                RequestOrigin::Mcp,
                now + Duration::from_millis(999)
            ),
            FetchDecision::Cached(_)
        ));
        assert!(matches!(
            decide_fetch(
                &mut state,
                8,
                request,
                RequestOrigin::Ui,
                now + Duration::from_millis(999)
            ),
            FetchDecision::WaitForCadence(_)
        ));
        assert!(matches!(
            decide_fetch(
                &mut state,
                7,
                request,
                RequestOrigin::Ui,
                now + Duration::from_millis(1001)
            ),
            FetchDecision::Start(_)
        ));
    }

    #[test]
    fn worker_memory_reservations_fit_inside_the_shared_budget() {
        let memory = Arc::new(Semaphore::new(
            MCP_IMAGE_MEMORY_BUDGET_BYTES / IMAGE_MEMORY_UNIT_BYTES,
        ));
        let workers = (0..MCP_IMAGE_WORKER_COUNT)
            .map(|_| {
                memory
                    .clone()
                    .try_acquire_many_owned(IMAGE_MEMORY_UNITS_PER_WORKER)
                    .expect("workerのメモリを共有予算内で確保する")
            })
            .collect::<Vec<_>>();
        assert_eq!(
            memory.available_permits(),
            MCP_IMAGE_MEMORY_BUDGET_BYTES / IMAGE_MEMORY_UNIT_BYTES
                - IMAGE_MEMORY_UNITS_PER_WORKER as usize * MCP_IMAGE_WORKER_COUNT
        );
        drop(workers);
        let all = memory
            .clone()
            .try_acquire_many_owned(
                (MCP_IMAGE_MEMORY_BUDGET_BYTES / IMAGE_MEMORY_UNIT_BYTES) as u32,
            )
            .expect("共有予算の全量を1回だけ確保する");
        assert!(memory
            .try_acquire_many_owned(IMAGE_MEMORY_UNITS_PER_WORKER)
            .is_err());
        drop(all);
    }

    #[tokio::test]
    async fn worker_count_is_bounded_and_shutdown_waits_for_active_work() {
        let active = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let peak = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let gate = Arc::new((TestMutex::new(false), Condvar::new()));
        let process = {
            let active = Arc::clone(&active);
            let peak = Arc::clone(&peak);
            let gate = Arc::clone(&gate);
            move |_thumbnail| {
                let current = active.fetch_add(1, Ordering::AcqRel) + 1;
                peak.fetch_max(current, Ordering::AcqRel);
                let (lock, changed) = &*gate;
                let released = lock.lock().unwrap_or_else(|error| error.into_inner());
                let _released = changed
                    .wait_while(released, |released| !*released)
                    .unwrap_or_else(|error| error.into_inner());
                active.fetch_sub(1, Ordering::AcqRel);
                Ok(McpThumbnailImage {
                    data: "cG5n".to_owned(),
                    mime_type: "image/png",
                })
            }
        };
        let processor = McpImageProcessor::with_process(process);
        let make_thumbnail = || norves_bridge_editor_client::ViewportThumbnail {
            image_base64: "AAAA".to_owned(),
            mime_type: "image/png".to_owned(),
            width: Some(1),
            height: Some(1),
        };
        let first = {
            let processor = processor.clone();
            tokio::spawn(async move { processor.process(make_thumbnail()).await })
        };
        let second = {
            let processor = processor.clone();
            tokio::spawn(async move { processor.process(make_thumbnail()).await })
        };
        tokio::time::timeout(Duration::from_secs(2), async {
            while active.load(Ordering::Acquire) < MCP_IMAGE_WORKER_COUNT {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("2つのworkerが起動する");
        let busy = processor
            .process(make_thumbnail())
            .await
            .expect_err("worker上限を超える処理を受け付けない");
        assert!(error_message(busy).contains("workerが使用中"));

        let stopping = {
            let processor = processor.clone();
            tokio::spawn(async move { processor.shutdown().await })
        };
        assert!(tokio::time::timeout(Duration::from_millis(30), async {
            let _ = stopping.await;
        })
        .await
        .is_err());
        {
            let (lock, changed) = &*gate;
            *lock.lock().unwrap_or_else(|error| error.into_inner()) = true;
            changed.notify_all();
        }
        let _ = first.await.expect("1つ目のworkerが終了する");
        let _ = second.await.expect("2つ目のworkerが終了する");
        assert!(peak.load(Ordering::Acquire) <= MCP_IMAGE_WORKER_COUNT);
        processor.shutdown().await;
        assert!(processor.process(make_thumbnail()).await.is_err());
    }

    #[tokio::test]
    async fn invalidating_a_connection_keeps_image_workers_available() {
        let service = McpThumbnailService::default();
        service.invalidate().await;

        let thumbnail = thumbnail_from_png(&png(1, 1), Some(1), Some(1));
        service
            .inner
            .processor
            .process(thumbnail)
            .await
            .expect("切断後も次の接続で画像処理workerを使える");
    }
}
