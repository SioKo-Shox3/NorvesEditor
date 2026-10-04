//! MCP サーバーの設定、loopback listener、認証失効を直列に管理する。

use std::{io, path::PathBuf, sync::Arc, time::Duration};

use tauri::{async_runtime::JoinHandle, AppHandle, Emitter, State, WebviewWindow};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::{
    dto::{McpConfirmationRequestDto, McpServerStateDto, McpSettingsPayload, McpTokenPayload},
    error::BackendError,
    mcp::{reads::McpReadContext, McpAuthorization, McpHttpAuth, McpHttpServer, McpWriteMode},
    mcp_settings::{McpSettings, McpSettingsError},
    mcp_token::{McpToken, McpTokenStore},
    protocol_names::events,
};

use super::authorization::{McpWriteSettings, ScopeError};

#[cfg(test)]
use std::net::Ipv4Addr;
#[cfg(test)]
use tokio::net::TcpListener;

/// Listener の停止を待つ猶予。超過時は task を中止して join する。
pub const SERVER_STOP_GRACE: Duration = Duration::from_secs(2);

#[derive(Clone)]
pub struct McpRuntime {
    inner: Arc<McpRuntimeInner>,
}

struct McpRuntimeInner {
    config_dir: PathBuf,
    token_store: McpTokenStore,
    authorization: McpAuthorization,
    reads: Option<McpReadContext>,
    control: Mutex<McpControl>,
    confirmation_events: Mutex<Option<ConfirmationEventTask>>,
}

struct McpControl {
    settings: McpSettings,
    server: Option<RunningServer>,
    state: McpServerStateDto,
    error: Option<String>,
    initialized: bool,
    closing: bool,
}

struct RunningServer {
    auth: Arc<McpHttpAuth>,
    shutdown: CancellationToken,
    task: JoinHandle<io::Result<()>>,
}

struct ConfirmationEventTask {
    shutdown: CancellationToken,
    task: JoinHandle<()>,
}

impl McpRuntime {
    /// 保存済み設定を読み、listener は非同期の初期化まで開かない。
    pub fn new(config_dir: PathBuf, authorization: McpAuthorization) -> Self {
        Self::build(
            config_dir,
            authorization,
            McpReadContext::default_context(),
            None,
        )
    }

    /// main画面へ確認待ちの更新を送るruntimeを作る。
    pub fn new_with_app(
        config_dir: PathBuf,
        authorization: McpAuthorization,
        app: AppHandle,
    ) -> Self {
        #[cfg(not(test))]
        let reads = McpReadContext::default_context().map(|reads| {
            reads.with_write_service(super::writes::McpWriteService::App(app.clone()))
        });
        // Wryのリンクが必要な管理状態の取得は、単体試験では共通サービスを直接注入する。
        #[cfg(test)]
        let reads = McpReadContext::default_context();
        let confirmation_events = Self::start_confirmation_events(&authorization, app);
        Self::build(config_dir, authorization, reads, Some(confirmation_events))
    }

    fn start_confirmation_events(
        authorization: &McpAuthorization,
        app: AppHandle,
    ) -> ConfirmationEventTask {
        let mut updates = authorization.confirmations().subscribe();
        let shutdown = CancellationToken::new();
        let task_shutdown = shutdown.clone();
        let task = tauri::async_runtime::spawn(async move {
            loop {
                tokio::select! {
                    _ = task_shutdown.cancelled() => break,
                    changed = updates.changed() => {
                        if changed.is_err() {
                            break;
                        }
                        let pending = updates.borrow_and_update().clone();
                        if let Err(error) = app.emit_to(
                            tauri::EventTarget::webview_window("main"),
                            events::MCP_CONFIRMATIONS_CHANGED,
                            pending,
                        ) {
                            tracing::warn!(error = %error, "MCP確認待ちの画面通知に失敗しました");
                        }
                    }
                }
            }
        });
        ConfirmationEventTask { shutdown, task }
    }

    fn build(
        config_dir: PathBuf,
        authorization: McpAuthorization,
        reads: Option<McpReadContext>,
        confirmation_events: Option<ConfirmationEventTask>,
    ) -> Self {
        let _ = authorization.set_write_settings(McpWriteSettings::default());
        if let Some(reads) = &reads {
            reads.set_write_permission(crate::mcp::tool_catalog::WritePermission::ReadOnly);
        }
        let loaded = McpSettings::load(&config_dir);
        let (settings, state, error, initialized) = match loaded {
            Ok(settings) => (settings, McpServerStateDto::Disabled, None, false),
            Err(_) => (
                McpSettings::default(),
                McpServerStateDto::StorageFailed,
                Some("MCP設定を読み込めませんでした。設定を保存し直してください。".to_owned()),
                true,
            ),
        };
        Self {
            inner: Arc::new(McpRuntimeInner {
                config_dir,
                token_store: McpTokenStore::default(),
                authorization,
                reads,
                control: Mutex::new(McpControl {
                    settings,
                    server: None,
                    state,
                    error,
                    initialized,
                    closing: false,
                }),
                confirmation_events: Mutex::new(confirmation_events),
            }),
        }
    }

    /// 起動時に保存設定を適用する。既定設定では listener を開かない。
    pub async fn initialize(&self) {
        let mut control = self.inner.control.lock().await;
        self.initialize_locked(&mut control).await;
    }

    /// 設定と公開状態を返す。トークンや認証ヘッダーは返さない。
    pub async fn settings(&self) -> McpSettingsPayload {
        let mut control = self.inner.control.lock().await;
        if !control.closing {
            self.initialize_locked(&mut control).await;
            self.refresh_finished_server(&mut control).await;
        }
        self.payload(&control)
    }

    /// 有効状態とポートを保存し、必要なら待受けを入れ替える。
    pub async fn set_settings(
        &self,
        enabled: bool,
        port: u16,
    ) -> Result<McpSettingsPayload, BackendError> {
        let settings = McpSettings { enabled, port };
        if port == 0 {
            return Err(BackendError::Request {
                message: McpSettingsError::InvalidPort.to_string(),
            });
        }
        let mut control = self.inner.control.lock().await;
        if control.closing {
            return Err(runtime_stopping_error());
        }
        self.initialize_locked(&mut control).await;
        if control.settings == settings
            && (control.state == McpServerStateDto::Running
                || (control.state == McpServerStateDto::Disabled && !settings.enabled))
        {
            self.refresh_finished_server(&mut control).await;
            return Ok(self.payload(&control));
        }
        if control.settings != settings || control.state == McpServerStateDto::StorageFailed {
            settings
                .save(&self.inner.config_dir)
                .map_err(|_| BackendError::McpSettingsStorage)?;
        }

        self.stop_server(&mut control).await;
        control.settings = settings;
        control.error = None;
        if settings.enabled {
            self.start_server(&mut control, None).await;
        } else {
            control.state = McpServerStateDto::Disabled;
        }
        Ok(self.payload(&control))
    }

    /// 書き込みモードとシーン部分木を設定し、旧要求の許可を失効させる。
    pub async fn set_write_access(
        &self,
        mode: McpWriteMode,
        scene_root_id: Option<String>,
    ) -> Result<McpSettingsPayload, BackendError> {
        let control = self.inner.control.lock().await;
        if control.closing {
            return Err(runtime_stopping_error());
        }
        self.inner
            .authorization
            .set_write_settings(McpWriteSettings {
                mode,
                scene_root_id,
            })
            .map_err(|error: ScopeError| BackendError::Request {
                message: error.message().to_owned(),
            })?;
        if let Some(reads) = &self.inner.reads {
            let permission = match mode {
                McpWriteMode::ReadOnly => crate::mcp::tool_catalog::WritePermission::ReadOnly,
                McpWriteMode::Enabled => crate::mcp::tool_catalog::WritePermission::Enabled,
                McpWriteMode::Confirm => crate::mcp::tool_catalog::WritePermission::Confirm,
            };
            reads.set_write_permission(permission);
        }
        Ok(self.payload(&control))
    }

    /// main画面が表示できる承認待ちだけを返す。
    pub async fn pending_confirmations(
        &self,
        window_label: &str,
    ) -> Result<Vec<McpConfirmationRequestDto>, BackendError> {
        require_main_confirmation_window(window_label)?;
        let control = self.inner.control.lock().await;
        if control.closing {
            return Err(runtime_stopping_error());
        }
        Ok(self.inner.authorization.confirmations().pending())
    }

    /// 一度だけ使える要求固有IDを承認または拒否する。
    pub async fn decide_confirmation(
        &self,
        window_label: &str,
        id: &str,
        approved: bool,
    ) -> Result<(), BackendError> {
        require_main_confirmation_window(window_label)?;
        let control = self.inner.control.lock().await;
        if control.closing {
            return Err(runtime_stopping_error());
        }
        let broker = self.inner.authorization.confirmations();
        let accepted = if approved {
            broker.approve(id)
        } else {
            broker.reject(id)
        };
        if accepted {
            Ok(())
        } else {
            Err(BackendError::Request {
                message: "確認待ちが取り下げ済みか、期限切れです。".to_owned(),
            })
        }
    }

    /// 明示要求時だけ保存済みの秘密を返す。通常の状態DTOやイベントへは載せない。
    pub async fn token(&self) -> Result<McpTokenPayload, BackendError> {
        let mut control = self.inner.control.lock().await;
        if control.closing {
            return Err(runtime_stopping_error());
        }
        self.initialize_locked(&mut control).await;
        let token = self
            .inner
            .token_store
            .load_or_create(&self.inner.config_dir)
            .map_err(|_| BackendError::McpTokenStorage)?;
        Ok(McpTokenPayload {
            token: token.expose(),
        })
    }

    /// トークンを置き換え、旧 listener・stream・認証要求を失効させる。
    pub async fn regenerate_token(&self) -> Result<McpSettingsPayload, BackendError> {
        let mut control = self.inner.control.lock().await;
        if control.closing {
            return Err(runtime_stopping_error());
        }
        self.initialize_locked(&mut control).await;
        self.stop_server(&mut control).await;
        let token = match self.inner.token_store.regenerate(&self.inner.config_dir) {
            Ok(token) => token,
            Err(_) => {
                control.state = McpServerStateDto::StorageFailed;
                control.error = Some(
                    "MCPトークンを安全に再生成できませんでした。保存先を確認してください。"
                        .to_owned(),
                );
                return Err(BackendError::McpTokenStorage);
            }
        };
        control.error = None;
        if control.settings.enabled {
            self.start_server(&mut control, Some(token)).await;
        } else {
            control.state = McpServerStateDto::Disabled;
        }
        Ok(self.payload(&control))
    }

    /// アプリ終了時に認証を閉じ、listener task を猶予付きで停止する。
    pub async fn shutdown(&self) {
        let mut control = self.inner.control.lock().await;
        if control.closing {
            return;
        }
        control.closing = true;
        self.stop_server(&mut control).await;
        control.state = McpServerStateDto::Disabled;
        control.error = None;
        drop(control);
        stop_confirmation_event_task(&self.inner.confirmation_events).await;
    }

    async fn initialize_locked(&self, control: &mut McpControl) {
        if control.initialized || control.closing {
            return;
        }
        control.initialized = true;
        if control.settings.enabled {
            self.start_server(control, None).await;
        }
    }

    async fn start_server(&self, control: &mut McpControl, token: Option<McpToken>) {
        let token = match token {
            Some(token) => token,
            None => match self
                .inner
                .token_store
                .load_or_create(&self.inner.config_dir)
            {
                Ok(token) => token,
                Err(_) => {
                    control.state = McpServerStateDto::StorageFailed;
                    control.error = Some(
                        "MCPトークンを安全に読み込めませんでした。Settingsで再生成してください。"
                            .to_owned(),
                    );
                    return;
                }
            },
        };
        let auth = Arc::new(McpHttpAuth::with_authorization(
            token,
            self.inner.authorization.clone(),
        ));
        let server_result = match &self.inner.reads {
            Some(reads) => {
                McpHttpServer::bind_with_reads(
                    control.settings.port,
                    Arc::clone(&auth),
                    reads
                        .clone()
                        .with_authorization(self.inner.authorization.clone()),
                )
                .await
            }
            None => McpHttpServer::bind(control.settings.port, Arc::clone(&auth)).await,
        };
        match server_result {
            Ok(server) => {
                let shutdown = CancellationToken::new();
                let task = tauri::async_runtime::spawn(server.serve(shutdown.clone()));
                control.server = Some(RunningServer {
                    auth,
                    shutdown,
                    task,
                });
                control.state = McpServerStateDto::Running;
                control.error = None;
            }
            Err(error) => {
                tracing::warn!(kind = ?error.kind(), "MCP loopback listener を開始できませんでした");
                control.state = McpServerStateDto::BindFailed;
                control.error = Some(format!(
                    "127.0.0.1:{}でMCPサーバーを開始できませんでした。ポートの使用状況を確認してください。",
                    control.settings.port
                ));
            }
        }
    }

    async fn stop_server(&self, control: &mut McpControl) {
        let Some(running) = control.server.take() else {
            self.inner.authorization.revoke();
            return;
        };
        running.auth.disable();
        running.shutdown.cancel();
        stop_server_task(running.task, SERVER_STOP_GRACE).await;
    }

    async fn refresh_finished_server(&self, control: &mut McpControl) {
        let finished = control
            .server
            .as_ref()
            .is_some_and(|server| server.task.inner().is_finished());
        if !finished {
            return;
        }
        if let Some(running) = control.server.take() {
            running.auth.disable();
            match running.task.await {
                Ok(Ok(())) => {
                    control.state = McpServerStateDto::Disabled;
                    control.error = None;
                }
                Ok(Err(error)) => {
                    tracing::warn!(kind = ?error.kind(), "MCP loopback listener が停止しました");
                    control.state = McpServerStateDto::BindFailed;
                    control.error = Some(
                        "MCPサーバーが停止しました。設定を確認して再試行してください。".to_owned(),
                    );
                }
                Err(_) => {
                    control.state = McpServerStateDto::BindFailed;
                    control.error = Some(
                        "MCPサーバーが停止しました。設定を確認して再試行してください。".to_owned(),
                    );
                }
            }
        }
    }

    fn payload(&self, control: &McpControl) -> McpSettingsPayload {
        let write_settings = self.inner.authorization.write_policy_snapshot().settings;
        McpSettingsPayload {
            enabled: control.settings.enabled,
            port: control.settings.port,
            write_mode: write_settings.mode,
            scene_root_id: write_settings.scene_root_id,
            state: control.state,
            endpoint: if control.state == McpServerStateDto::Running {
                control.settings.endpoint().ok().map(|url| url.to_string())
            } else {
                None
            },
            error: control.error.clone(),
        }
    }
}

fn runtime_stopping_error() -> BackendError {
    BackendError::Request {
        message: "MCPサーバーは終了処理中のため、設定を変更できません。".to_owned(),
    }
}

async fn stop_server_task(mut task: JoinHandle<io::Result<()>>, grace: Duration) -> bool {
    match tokio::time::timeout(grace, &mut task).await {
        Ok(Ok(Ok(()))) => true,
        Ok(Ok(Err(error))) => {
            tracing::warn!(kind = ?error.kind(), "MCP loopback listener の停止中にエラーが発生しました");
            true
        }
        Ok(Err(_)) => true,
        Err(_) => {
            task.abort();
            let _ = task.await;
            false
        }
    }
}

async fn stop_confirmation_event_task(task: &Mutex<Option<ConfirmationEventTask>>) {
    let Some(event_task) = task.lock().await.take() else {
        return;
    };
    event_task.shutdown.cancel();
    let mut join = event_task.task;
    if tokio::time::timeout(SERVER_STOP_GRACE, &mut join)
        .await
        .is_err()
    {
        join.abort();
        let _ = join.await;
    }
}

/// MCP設定を操作できるWebviewラベルだけを許可する。
pub fn is_trusted_settings_window(label: &str) -> bool {
    matches!(label, "main" | "settings")
}

/// MCP確認の閲覧と承認をできるのはmain画面だけ。
pub fn is_main_confirmation_window(label: &str) -> bool {
    label == "main"
}

fn require_trusted_window(window: &WebviewWindow) -> Result<(), BackendError> {
    if is_trusted_settings_window(window.label()) {
        Ok(())
    } else {
        Err(BackendError::Request {
            message: "このウィンドウからMCP設定を操作できません。".to_owned(),
        })
    }
}

fn require_main_confirmation_window(label: &str) -> Result<(), BackendError> {
    if is_main_confirmation_window(label) {
        Ok(())
    } else {
        Err(BackendError::Request {
            message: "MCP確認はメイン画面からだけ操作できます。".to_owned(),
        })
    }
}

#[tauri::command]
pub async fn get_mcp_settings(
    window: WebviewWindow,
    runtime: State<'_, McpRuntime>,
) -> Result<McpSettingsPayload, BackendError> {
    require_trusted_window(&window)?;
    Ok(runtime.settings().await)
}

#[tauri::command]
pub async fn set_mcp_settings(
    window: WebviewWindow,
    runtime: State<'_, McpRuntime>,
    enabled: bool,
    port: u16,
) -> Result<McpSettingsPayload, BackendError> {
    require_trusted_window(&window)?;
    runtime.set_settings(enabled, port).await
}

#[tauri::command]
pub async fn set_mcp_write_access(
    window: WebviewWindow,
    runtime: State<'_, McpRuntime>,
    mode: McpWriteMode,
    scene_root_id: Option<String>,
) -> Result<McpSettingsPayload, BackendError> {
    require_trusted_window(&window)?;
    runtime.set_write_access(mode, scene_root_id).await
}

#[tauri::command]
pub async fn get_mcp_confirmations(
    window: WebviewWindow,
    runtime: State<'_, McpRuntime>,
) -> Result<Vec<McpConfirmationRequestDto>, BackendError> {
    runtime.pending_confirmations(window.label()).await
}

#[tauri::command]
pub async fn approve_mcp_confirmation(
    window: WebviewWindow,
    runtime: State<'_, McpRuntime>,
    confirmation_id: String,
) -> Result<(), BackendError> {
    runtime
        .decide_confirmation(window.label(), &confirmation_id, true)
        .await
}

#[tauri::command]
pub async fn reject_mcp_confirmation(
    window: WebviewWindow,
    runtime: State<'_, McpRuntime>,
    confirmation_id: String,
) -> Result<(), BackendError> {
    runtime
        .decide_confirmation(window.label(), &confirmation_id, false)
        .await
}

#[tauri::command]
pub async fn get_mcp_token(
    window: WebviewWindow,
    runtime: State<'_, McpRuntime>,
) -> Result<McpTokenPayload, BackendError> {
    require_trusted_window(&window)?;
    runtime.token().await
}

#[tauri::command]
pub async fn regenerate_mcp_token(
    window: WebviewWindow,
    runtime: State<'_, McpRuntime>,
) -> Result<McpSettingsPayload, BackendError> {
    require_trusted_window(&window)?;
    runtime.regenerate_token().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        sync::atomic::{AtomicU64, Ordering},
    };

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "norves-mcp-runtime-{}-{sequence}",
                std::process::id()
            ));
            fs::create_dir_all(&path).expect("一時設定ディレクトリを作成する");
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[tokio::test]
    async fn only_main_can_read_approve_or_reject_a_confirmation_and_shutdown_withdraws_it() {
        use crate::mcp::confirmation::{
            tests::{poll_pending, request},
            ConfirmationResult,
        };

        let directory = TestDirectory::new();
        let authorization = McpAuthorization::default();
        let runtime = McpRuntime::new(directory.0.clone(), authorization.clone());
        let broker = authorization.confirmations();
        for approved in [true, false] {
            let lease = authorization.current_lease();
            let mut pending = Box::pin(broker.request(request(), &lease, std::future::pending()));
            poll_pending(pending.as_mut()).await;
            let id = broker.pending()[0].id.clone();
            for label in [
                "settings",
                "game",
                "Main",
                "main-child",
                "",
                "https://localhost",
            ] {
                assert!(runtime.pending_confirmations(label).await.is_err());
                for decision in [true, false] {
                    assert!(runtime
                        .decide_confirmation(label, &id, decision)
                        .await
                        .is_err());
                }
                assert_eq!(broker.pending().len(), 1);
            }
            assert_eq!(
                runtime
                    .pending_confirmations("main")
                    .await
                    .expect("mainが取得する")[0]
                    .id,
                id
            );
            runtime
                .decide_confirmation("main", &id, approved)
                .await
                .expect("mainが判断する");
            assert_eq!(
                pending.await,
                if approved {
                    ConfirmationResult::Approved
                } else {
                    ConfirmationResult::Rejected
                }
            );
            let error = runtime
                .decide_confirmation("main", &id, approved)
                .await
                .expect_err("二度目を拒否する");
            assert!(!error.to_string().contains(&id));
            assert!(!format!("{error:?}").contains(&id));
        }

        let lease = authorization.current_lease();
        let mut pending = Box::pin(broker.request(request(), &lease, std::future::pending()));
        poll_pending(pending.as_mut()).await;
        tokio::time::timeout(Duration::from_secs(1), runtime.shutdown())
            .await
            .expect("確認待ち中も終了する");
        assert_eq!(pending.await, ConfirmationResult::Cancelled);
        assert!(broker.pending().is_empty());
        assert!(runtime.pending_confirmations("main").await.is_err());
    }

    #[tokio::test]
    async fn confirmation_lifecycle_does_not_write_secret_ids_to_logs() {
        use crate::mcp::confirmation::tests::{poll_pending, request};
        use std::io::Write;
        use tracing::instrument::WithSubscriber;

        #[derive(Clone)]
        struct Capture(Arc<std::sync::Mutex<Vec<u8>>>);
        impl Write for Capture {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                self.0
                    .lock()
                    .expect("ログを捕捉する")
                    .extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let capture = Capture(Arc::default());
        let writer = capture.clone();
        let subscriber = tracing_subscriber::fmt()
            .without_time()
            .with_ansi(false)
            .with_max_level(tracing::Level::TRACE)
            .with_writer(move || writer.clone())
            .finish();
        let secrets = async {
            let directory = TestDirectory::new();
            let authorization = McpAuthorization::default();
            let runtime = McpRuntime::new(directory.0.clone(), authorization.clone());
            let broker = authorization.confirmations();
            let mut secrets = Vec::new();
            for outcome in 0..5 {
                let mut lease = authorization.current_lease();
                if outcome == 4 {
                    lease.set_request_deadline(
                        tokio::time::Instant::now() + Duration::from_millis(20),
                    );
                }
                let mut pending =
                    Box::pin(broker.request(request(), &lease, std::future::pending()));
                poll_pending(pending.as_mut()).await;
                let id = broker.pending()[0].id.clone();
                match outcome {
                    0 | 1 => runtime
                        .decide_confirmation("main", &id, outcome == 0)
                        .await
                        .expect("確認に回答する"),
                    2 => lease.cancel_request(),
                    3 => {
                        authorization.revoke();
                    }
                    _ => {}
                }
                tracing::info!(result = ?pending.await, "確認要求が終了しました");
                let error = runtime
                    .decide_confirmation("main", &id, true)
                    .await
                    .expect_err("使用済みIDを拒否する");
                tracing::warn!(error = ?error, "確認に回答できませんでした");
                secrets.push(id);
            }
            runtime.shutdown().await;
            secrets
        }
        .with_subscriber(subscriber)
        .await;
        let bytes = capture.0.lock().expect("捕捉ログを読む").clone();
        let log = String::from_utf8(bytes).expect("ログはUTF-8");
        assert!(log.contains("確認要求が終了しました"));
        for secret in secrets {
            assert!(!log.contains(&secret));
        }
    }

    #[tokio::test]
    async fn default_settings_keep_the_port_closed_until_enabled() {
        let directory = TestDirectory::new();
        let runtime = McpRuntime::new(directory.0.clone(), McpAuthorization::default());
        let payload = runtime.settings().await;
        assert!(!payload.enabled);
        assert_eq!(payload.port, 49_770);
        assert_eq!(payload.write_mode, McpWriteMode::ReadOnly);
        assert_eq!(payload.scene_root_id, None);
        assert_eq!(payload.state, McpServerStateDto::Disabled);
        assert!(payload.endpoint.is_none());
        assert!(runtime.inner.control.lock().await.server.is_none());
    }

    #[tokio::test]
    async fn write_access_changes_revoke_old_requests_and_restart_read_only() {
        let directory = TestDirectory::new();
        let authorization = McpAuthorization::default();
        authorization
            .set_write_settings(McpWriteSettings {
                mode: McpWriteMode::Enabled,
                scene_root_id: Some("stale-scope".to_owned()),
            })
            .expect("事前の許可を作る");
        let old_lease = authorization.current_lease();
        let runtime = McpRuntime::new(directory.0.clone(), authorization.clone());
        let initial = runtime.settings().await;
        assert_eq!(initial.write_mode, McpWriteMode::ReadOnly);
        assert_eq!(initial.scene_root_id, None);
        assert!(!old_lease.is_current());

        let enabled_lease = authorization.current_lease();
        let enabled = runtime
            .set_write_access(McpWriteMode::Enabled, Some("allowed-root".to_owned()))
            .await
            .expect("書き込み可の範囲を設定する");
        assert_eq!(enabled.write_mode, McpWriteMode::Enabled);
        assert_eq!(enabled.scene_root_id.as_deref(), Some("allowed-root"));
        assert!(!enabled_lease.is_current());

        let confirm_lease = authorization.current_lease();
        let confirmed = runtime
            .set_write_access(McpWriteMode::Confirm, None)
            .await
            .expect("都度確認へ変更する");
        assert_eq!(confirmed.write_mode, McpWriteMode::Confirm);
        assert!(confirmed.scene_root_id.is_none());
        assert!(!confirm_lease.is_current());

        let invalid = runtime
            .set_write_access(McpWriteMode::Enabled, Some(String::new()))
            .await;
        assert!(matches!(invalid, Err(BackendError::Request { .. })));
        runtime.shutdown().await;
    }

    #[tokio::test]
    async fn enable_disable_and_port_change_are_serialized_and_rebind() {
        let directory = TestDirectory::new();
        let runtime = McpRuntime::new(directory.0.clone(), McpAuthorization::default());
        let first = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("空きポートを確保する");
        let first_port = first.local_addr().expect("portを読む").port();
        drop(first);
        let second = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("別の空きポートを確保する");
        let second_port = second.local_addr().expect("portを読む").port();
        drop(second);

        let running = runtime
            .set_settings(true, first_port)
            .await
            .expect("起動する");
        assert_eq!(running.state, McpServerStateDto::Running);
        let first_probe = TcpListener::bind((Ipv4Addr::LOCALHOST, first_port)).await;
        assert!(first_probe.is_err());

        let rebound = runtime
            .set_settings(true, second_port)
            .await
            .expect("portを変更する");
        assert_eq!(rebound.state, McpServerStateDto::Running);
        assert_eq!(rebound.port, second_port);
        assert!(TcpListener::bind((Ipv4Addr::LOCALHOST, first_port))
            .await
            .is_ok());
        assert!(TcpListener::bind((Ipv4Addr::LOCALHOST, second_port))
            .await
            .is_err());

        let disabled = runtime
            .set_settings(false, second_port)
            .await
            .expect("停止する");
        assert_eq!(disabled.state, McpServerStateDto::Disabled);
        assert!(TcpListener::bind((Ipv4Addr::LOCALHOST, second_port))
            .await
            .is_ok());
        runtime.shutdown().await;
    }

    #[tokio::test]
    async fn shutdown_is_final_and_rejects_later_runtime_changes() {
        let directory = TestDirectory::new();
        let runtime = McpRuntime::new(directory.0.clone(), McpAuthorization::default());
        let reservation = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("空きportを確保する");
        let port = reservation.local_addr().expect("portを読む").port();
        drop(reservation);

        runtime
            .set_settings(true, port)
            .await
            .expect("MCP listenerを開始する");
        runtime.shutdown().await;
        assert_eq!(runtime.settings().await.state, McpServerStateDto::Disabled);
        assert!(runtime.set_settings(true, port).await.is_err());
        assert!(runtime.token().await.is_err());
        assert!(runtime.regenerate_token().await.is_err());
        assert!(TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await.is_ok());
    }

    #[tokio::test]
    async fn concurrent_setting_operations_wait_for_the_lifecycle_lock() {
        let directory = TestDirectory::new();
        let runtime = McpRuntime::new(directory.0.clone(), McpAuthorization::default());
        let reservation = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("空きportを確保する");
        let port = reservation.local_addr().expect("portを読む").port();
        drop(reservation);

        let guard = runtime.inner.control.lock().await;
        let pending_runtime = runtime.clone();
        let mut pending =
            tokio::spawn(async move { pending_runtime.set_settings(true, port).await });
        assert!(
            tokio::time::timeout(Duration::from_millis(30), &mut pending)
                .await
                .is_err(),
            "同時設定変更はlifecycle lockの解放まで待つ"
        );
        drop(guard);
        pending
            .await
            .expect("設定taskが完了する")
            .expect("設定taskが成功する");
        let payload = runtime.settings().await;
        assert_eq!(payload.state, McpServerStateDto::Running);
        runtime.shutdown().await;
    }

    #[tokio::test]
    async fn bind_failure_is_reported_without_exposing_a_secret() {
        let directory = TestDirectory::new();
        let occupied = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("portを確保する");
        let port = occupied.local_addr().expect("portを読む").port();
        let runtime = McpRuntime::new(directory.0.clone(), McpAuthorization::default());
        let payload = runtime.set_settings(true, port).await.expect("状態を得る");
        assert_eq!(payload.state, McpServerStateDto::BindFailed);
        assert!(payload
            .error
            .as_deref()
            .is_some_and(|error| !error.is_empty()));
        assert!(serde_json::to_string(&payload)
            .expect("状態を直列化する")
            .find("token")
            .is_none());
        runtime.shutdown().await;
        drop(occupied);
    }

    #[tokio::test]
    async fn token_regeneration_revokes_leases_and_keeps_secret_explicit() {
        let directory = TestDirectory::new();
        let authorization = McpAuthorization::default();
        let runtime = McpRuntime::new(directory.0.clone(), authorization.clone());
        let reservation = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("空きportを確保する");
        let port = reservation.local_addr().expect("portを読む").port();
        drop(reservation);
        runtime
            .set_settings(true, port)
            .await
            .expect("MCP listenerを開始する");
        let first = runtime.token().await.expect("秘密の明示表示");
        let lease = authorization.current_lease();
        let regenerated = runtime
            .regenerate_token()
            .await
            .expect("トークンを作り直す");
        assert_eq!(regenerated.state, McpServerStateDto::Running);
        assert!(!lease.is_current());
        assert!(lease.cancellation_token().is_cancelled());
        let second = runtime.token().await.expect("新しい秘密を明示表示");
        assert_ne!(first.token, second.token);
        let ordinary =
            serde_json::to_string(&runtime.settings().await).expect("通常状態を直列化する");
        assert!(!ordinary.contains(&second.token));
        assert!(TcpListener::bind((Ipv4Addr::LOCALHOST, port))
            .await
            .is_err());
        runtime.shutdown().await;
    }

    #[tokio::test]
    async fn startup_restores_enabled_setting_but_default_does_not_bind() {
        let directory = TestDirectory::new();
        let reserved = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("空きポートを得る");
        let port = reserved.local_addr().expect("portを読む").port();
        drop(reserved);
        McpSettings {
            enabled: true,
            port,
        }
        .save(&directory.0)
        .expect("enabled設定を保存する");
        let runtime = McpRuntime::new(directory.0.clone(), McpAuthorization::default());
        assert_eq!(runtime.settings().await.state, McpServerStateDto::Running);
        runtime.shutdown().await;
        assert!(TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await.is_ok());
    }

    #[tokio::test]
    async fn shutdown_aborts_and_joins_a_task_after_the_grace_period() {
        assert_eq!(SERVER_STOP_GRACE, Duration::from_secs(2));
        let (dropped, dropped_rx) = tokio::sync::oneshot::channel();
        struct DropNotice(Option<tokio::sync::oneshot::Sender<()>>);
        impl Drop for DropNotice {
            fn drop(&mut self) {
                if let Some(sender) = self.0.take() {
                    let _ = sender.send(());
                }
            }
        }
        let task = tauri::async_runtime::spawn(async move {
            let _notice = DropNotice(Some(dropped));
            std::future::pending::<io::Result<()>>().await
        });
        let started = tokio::time::Instant::now();
        let stopped = stop_server_task(task, SERVER_STOP_GRACE).await;
        assert!(started.elapsed() >= SERVER_STOP_GRACE);
        assert!(!stopped, "猶予超過のtaskは中止してjoinする");
        tokio::time::timeout(Duration::from_secs(1), dropped_rx)
            .await
            .expect("中止したtaskをdropする")
            .expect("中止が完了した");
    }

    #[test]
    fn only_main_and_settings_windows_can_manage_mcp() {
        assert!(is_trusted_settings_window("main"));
        assert!(is_trusted_settings_window("settings"));
        assert!(!is_trusted_settings_window("connection"));
        assert!(!is_trusted_settings_window("other"));
    }
}
