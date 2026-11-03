//! A real WebSocket client [`Transport`] built on `tokio-tungstenite`.
//!
//! This is the I/O-backed sibling of the in-memory
//! [`crate::transport::LoopbackTransport`]: it satisfies the exact same
//! [`Transport`] contract, so a [`crate::Dispatcher`] can be spawned over either
//! without code change. It carries exactly one wire envelope per frame as raw
//! JSON text and does **no** Bridge protocol work — all encode/decode stays in
//! the dispatcher.
//!
//! # Scope (G2)
//!
//! * Single connection only. There is **no** reconnect logic here; the upper
//!   layer (G5) owns reconnect by constructing a fresh [`WsClientTransport`].
//! * Local-only by intent: the editor dials `ws://127.0.0.1:<port>` against an
//!   engine on the same machine. TLS is deliberately not enabled (the crate's
//!   default features pull in no TLS backend), so the URL scheme is `ws`, never
//!   `wss`.
//!
//! # Frame mapping
//!
//! `tokio-tungstenite` handles control frames (Ping/Pong/Close) at the protocol
//! level: it auto-replies to a Ping with a Pong. We therefore treat Ping/Pong as
//! transparent keep-alive and keep reading for the next data frame. A `Close`
//! (or stream end) maps to the clean-EOF `Ok(None)` the [`Transport`] contract
//! defines. Per ADR0003 the wire is canonical JSON **text**, so an inbound
//! `Binary` frame is a spec violation and surfaces as
//! [`TransportError::Io`].

use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::protocol::Message;
use tokio_tungstenite::tungstenite::Error as WsError;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

use crate::transport::{Transport, TransportError};

/// The concrete stream type produced by [`connect_async`]: a tungstenite
/// WebSocket layered over a (possibly TLS-wrapped) tokio `TcpStream`. With TLS
/// features off the `MaybeTlsStream` is always the plain variant, but the type
/// alias keeps the wrapper so the signature stays stable if TLS is ever enabled.
type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// A real, single-connection WebSocket client [`Transport`].
///
/// The underlying `WebSocketStream` is `.split()` into an independent write half
/// ([`SinkExt`]) and read half ([`StreamExt`]) so `send` and `recv` borrow
/// disjoint fields and never contend. Both halves are `Send + 'static`, so the
/// whole transport satisfies the [`Transport`] bound and can be moved into a
/// [`crate::Dispatcher`] task.
pub struct WsClientTransport {
    /// Outbound half: frames are written as `Message::Text`.
    writer: SplitSink<WsStream, Message>,
    /// Inbound half: yields tungstenite `Message`s until the peer closes.
    reader: SplitStream<WsStream>,
}

impl WsClientTransport {
    /// Dials `url` (expected `ws://127.0.0.1:<port>`) and returns a connected
    /// transport, or [`TransportError::Io`] if the connection or WebSocket
    /// handshake fails.
    ///
    /// A connect failure is always mapped to `Io`: there is no prior connection
    /// to have been "closed", so `Closed` would be misleading here.
    pub async fn connect(url: &str) -> Result<Self, TransportError> {
        let (stream, _response) = connect_async(url)
            .await
            .map_err(|err| TransportError::Io(format!("ws connect failed: {err}")))?;
        let (writer, reader) = stream.split();
        Ok(WsClientTransport { writer, reader })
    }
}

/// Maps a tungstenite read/write [`WsError`] onto the small transport error set.
///
/// A normal/already-closed connection becomes [`TransportError::Closed`];
/// everything else (I/O, protocol, capacity, …) is an abnormal
/// [`TransportError::Io`].
fn map_ws_error(err: WsError) -> TransportError {
    match err {
        WsError::ConnectionClosed | WsError::AlreadyClosed => TransportError::Closed,
        other => TransportError::Io(other.to_string()),
    }
}

impl Transport for WsClientTransport {
    // The trait declares `-> impl Future + Send`; an `async fn` in the impl
    // satisfies that and reads more clearly than a hand-rolled `async move`.
    async fn send(&mut self, frame: String) -> Result<(), TransportError> {
        // `String: Into<Utf8Bytes>`, so this avoids an extra validation/copy
        // beyond what tungstenite already does for an owned String.
        self.writer
            .send(Message::Text(frame.into()))
            .await
            .map_err(map_ws_error)
    }

    async fn recv(&mut self) -> Result<Option<String>, TransportError> {
        // Loop so that transparent control frames (Ping/Pong) do not surface to
        // the caller: tungstenite has already auto-responded to a Ping with a
        // Pong, so we just keep reading for the next data frame.
        loop {
            match self.reader.next().await {
                // Data frame: the one case that yields a wire envelope.
                Some(Ok(Message::Text(text))) => return Ok(Some(text.as_str().to_owned())),
                // Clean close handshake from the peer == EOF.
                Some(Ok(Message::Close(_))) => return Ok(None),
                // Stream exhausted == EOF (peer dropped without a Close frame).
                None => return Ok(None),
                // Keep-alive control frames are transparent: tungstenite auto-
                // replies to Ping with Pong; we read the next frame.
                Some(Ok(Message::Ping(_))) | Some(Ok(Message::Pong(_))) => continue,
                // ADR0003: the wire is canonical JSON text. A binary frame is a
                // protocol violation, not a clean close, so it is an error.
                Some(Ok(Message::Binary(_))) => {
                    return Err(TransportError::Io("unexpected binary frame".to_owned()))
                }
                // `Message::Frame` is only produced when reading raw frames,
                // which we never request; treat an unexpected raw frame as a
                // protocol violation rather than silently looping forever.
                Some(Ok(Message::Frame(_))) => {
                    return Err(TransportError::Io("unexpected raw frame".to_owned()))
                }
                // An underlying read error: closed/already-closed is a clean-ish
                // terminal mapped to `Closed`; anything else is `Io`.
                Some(Err(err)) => return Err(map_ws_error(err)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::net::TcpListener;
    use tokio::task::JoinHandle;
    use tokio_tungstenite::accept_async;

    /// ネットワークとタスクの待機を制限し、停止を段名付きの失敗として報告する。
    const TEST_TIMEOUT: Duration = Duration::from_secs(5);

    /// 時間切れを待機していた段の名前付きで報告する。
    async fn with_timeout<F, T>(label: &str, fut: F) -> T
    where
        F: std::future::Future<Output = T>,
    {
        tokio::time::timeout(TEST_TIMEOUT, fut)
            .await
            .unwrap_or_else(|_| panic!("timed out: {label}"))
    }

    /// OS が割り当てたローカルポートのリスナーと接続先 URL を返す。
    async fn bind_local() -> (TcpListener, String) {
        let listener = with_timeout("bind listener", TcpListener::bind("127.0.0.1:0"))
            .await
            .expect("bind ephemeral port");
        let addr = listener.local_addr().expect("local addr");
        let url = format!("ws://{addr}");
        (listener, url)
    }

    /// 接続を1つ受けて body を実行し、サーバー側の失敗を確認するハンドルを返す。
    fn serve_one<F, Fut>(listener: TcpListener, body: F) -> JoinHandle<()>
    where
        F: FnOnce(WebSocketStream<TcpStream>) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send,
    {
        tokio::spawn(async move {
            let (tcp, _peer) = with_timeout("server accept", listener.accept())
                .await
                .expect("server accepts tcp");
            let ws = with_timeout("server handshake", accept_async(tcp))
                .await
                .expect("server ws handshake");
            body(ws).await;
        })
    }

    #[tokio::test]
    async fn round_trips_a_text_frame() {
        let (listener, url) = bind_local().await;
        // 最初に受け取ったテキストフレームを送り返す。
        let server = serve_one(listener, |mut ws| async move {
            if let Some(Ok(Message::Text(t))) = with_timeout("server recv", ws.next()).await {
                with_timeout("server echo", ws.send(Message::Text(t)))
                    .await
                    .expect("server echo");
            }
        });

        let mut client = with_timeout("connect", WsClientTransport::connect(&url))
            .await
            .expect("client connects");
        with_timeout("send", client.send("hello".to_owned()))
            .await
            .expect("send ok");
        let got = with_timeout("recv", client.recv()).await.expect("recv ok");
        assert_eq!(got, Some("hello".to_owned()));

        with_timeout("server task join", server)
            .await
            .expect("server task joins");
    }

    #[tokio::test]
    async fn preserves_order_of_multiple_frames() {
        let (listener, url) = bind_local().await;
        // 3つのテキストフレームを続けて送信してから閉じる。
        let server = serve_one(listener, |mut ws| async move {
            for i in 0..3 {
                with_timeout(
                    "server send",
                    ws.send(Message::Text(format!("frame-{i}").into())),
                )
                .await
                .expect("server send");
            }
            with_timeout("server close", ws.close(None))
                .await
                .expect("server close");
        });

        let mut client = with_timeout("connect", WsClientTransport::connect(&url))
            .await
            .expect("client connects");
        for i in 0..3 {
            let got = with_timeout("recv", client.recv()).await.expect("recv ok");
            assert_eq!(got, Some(format!("frame-{i}")));
        }

        with_timeout("server task join", server)
            .await
            .expect("server task joins");
    }

    #[tokio::test]
    async fn close_maps_to_clean_eof() {
        let (listener, url) = bind_local().await;
        let server = serve_one(listener, |mut ws| async move {
            with_timeout("server close", ws.close(None))
                .await
                .expect("server close");
        });

        let mut client = with_timeout("connect", WsClientTransport::connect(&url))
            .await
            .expect("client connects");
        let got = with_timeout("recv", client.recv()).await.expect("recv ok");
        assert_eq!(got, None, "a Close frame must surface as clean EOF");

        with_timeout("server task join", server)
            .await
            .expect("server task joins");
    }

    #[tokio::test]
    async fn binary_frame_is_an_error() {
        let (listener, url) = bind_local().await;
        let server = serve_one(listener, |mut ws| async move {
            with_timeout(
                "server send binary",
                ws.send(Message::Binary(vec![1, 2, 3].into())),
            )
            .await
            .expect("server send binary");
            // バイナリフレームを読む前に接続が閉じないよう、クライアントを待つ。
            let _ = with_timeout("server recv", ws.next()).await;
        });

        let mut client = with_timeout("connect", WsClientTransport::connect(&url))
            .await
            .expect("client connects");
        let err = with_timeout("recv", client.recv())
            .await
            .expect_err("binary frame must be an error");
        assert!(
            matches!(err, TransportError::Io(ref m) if m.contains("binary")),
            "expected an Io(\"...binary...\") error, got {err:?}"
        );

        server.abort();
        if let Err(err) = with_timeout("server task cancellation", server).await {
            assert!(err.is_cancelled(), "server task failed: {err}");
        }
    }

    #[tokio::test]
    async fn ping_is_transparent_before_a_text_frame() {
        let (listener, url) = bind_local().await;
        // Ping に続けて Text を送る。クライアントは Ping を読み飛ばして
        // Text を届け、tungstenite が自動的に Pong を返す。
        let server = serve_one(listener, |mut ws| async move {
            with_timeout("server ping", ws.send(Message::Ping(vec![9, 9].into())))
                .await
                .expect("server ping");
            with_timeout("server text", ws.send(Message::Text("after-ping".into())))
                .await
                .expect("server text");
            // 自動返信の Pong を読み、書き込みの途中で接続を切らない。
            while let Some(Ok(msg)) = with_timeout("server recv", ws.next()).await {
                if msg.is_close() {
                    break;
                }
            }
        });

        let mut client = with_timeout("connect", WsClientTransport::connect(&url))
            .await
            .expect("client connects");
        let got = with_timeout("recv", client.recv()).await.expect("recv ok");
        assert_eq!(got, Some("after-ping".to_owned()));

        server.abort();
        if let Err(err) = with_timeout("server task cancellation", server).await {
            assert!(err.is_cancelled(), "server task failed: {err}");
        }
    }

    #[tokio::test]
    async fn connect_to_dead_port_is_io_error() {
        // ポートを確保してからリスナーを破棄し、接続先を不在にする。
        let (listener, url) = bind_local().await;
        drop(listener);

        // 分割したストリームは Debug を実装しないため、結果を match で確認する。
        match with_timeout("connect", WsClientTransport::connect(&url)).await {
            Ok(_) => panic!("connect to a dead port must fail"),
            Err(TransportError::Io(_)) => {}
            Err(other) => panic!("expected Io on connect failure, got {other:?}"),
        }
    }

    /// 実際の WS 接続で要求を送信し、Bridge 形式の応答で解決することを確認する。
    /// 符号化・送信・サーバー側の復号と応答・受信・要求との対応付けまでを通す。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dispatcher_over_ws_resolves_a_request() {
        use crate::Dispatcher;
        use norves_bridge_core::{
            decode_typed, encode_envelope, CorrelationId, Envelope, MethodName, ResponsePayload,
            ValidatedEnvelope, VersionString,
        };

        let (listener, url) = bind_local().await;
        // 要求を1つ読み、その要求に対応する結果を返す。
        let server = serve_one(listener, |mut ws| async move {
            let msg = with_timeout("server recv", ws.next())
                .await
                .expect("server reads a frame")
                .expect("ok");
            let text = match msg {
                Message::Text(t) => t.as_str().to_owned(),
                other => panic!("expected text request, got {other:?}"),
            };
            let id = match decode_typed(&text).expect("server decodes request") {
                ValidatedEnvelope::Request { id, .. } => id,
                other => panic!("expected request, got {other:?}"),
            };
            let response: Envelope = ValidatedEnvelope::Response {
                version: VersionString::try_from("0.1".to_owned()).expect("version"),
                id,
                payload: ResponsePayload::Result(serde_json::json!({ "ok": true })),
                session_id: None,
                seq: None,
            }
            .into();
            let frame = encode_envelope(&response).expect("server encodes response");
            with_timeout("server response send", ws.send(Message::Text(frame.into())))
                .await
                .expect("server sends response");
        });

        let client = with_timeout("connect", WsClientTransport::connect(&url))
            .await
            .expect("client connects");
        let handle = Dispatcher::spawn(client);

        let request = ValidatedEnvelope::Request {
            version: VersionString::try_from("0.1".to_owned()).expect("version"),
            id: CorrelationId::try_from("req-1".to_owned()).expect("id"),
            method: MethodName::try_from("engine.getStatus".to_owned()).expect("method"),
            params: None,
            session_id: None,
            seq: None,
        };
        let outcome = with_timeout("request", handle.request(request, TEST_TIMEOUT))
            .await
            .expect("request resolves");
        match outcome {
            ResponsePayload::Result(value) => {
                assert_eq!(value, serde_json::json!({ "ok": true }));
            }
            ResponsePayload::Error(err) => panic!("expected result, got error {err:?}"),
        }

        with_timeout("dispatcher shutdown", handle.shutdown()).await;
        with_timeout("server task join", server)
            .await
            .expect("server task joins");
    }
}
