//! LOAD-BEARING runtime glue for the engine process lifecycle (plan J3).
//!
//! This module owns the impure side of the engine process: spawning the child
//! via `tokio::process`, waiting for its `READY <port>` stdout handshake under a
//! timeout, supervising it with an exit-monitor task, and killing it on stop /
//! app-exit. All the pure decision/parsing/shaping logic it relies on lives in
//! [`crate::process`] (resolve/validate path, parse READY, pick port, generation
//! guard, exited-params builder); this file only wires that logic to real I/O.
//!
//! # Ownership / threading model
//!
//! * [`ProcessState`] is a Tauri-managed singleton, SEPARATE from `BridgeState`.
//!   It holds at most one [`ProcessHandle`] behind a `tokio::sync::Mutex`.
//! * The `tokio::process::Child` is OWNED BY THE MONITOR TASK so it alone can
//!   `wait()` on it; the holder never stores the `Child` (no two owners).
//! * The monitor task is the SOLE emitter of `engine.processExited`. Neither
//!   `stop_engine` nor the app-exit hook emits it; they only trigger the kill and
//!   let the monitor synthesize the single exit event under a generation guard.
//!
//! ## Stop / exit signalling contract
//!
//! Exactly one of two signals reaches the UI per cause, never both:
//!
//! * USER-INITIATED stop (`stop_engine`): emits ONE disconnected
//!   `CONNECTION_STATE` (reason "engine stopped") and INTENTIONALLY suppresses
//!   `engine.processExited`. The suppression is structural: `stop_engine` clears
//!   the holder BEFORE triggering the kill, so when the monitor reaches its
//!   generation guard the holder is already `None` and `monitor_should_emit_exit`
//!   does not fire. The courtesy disconnect (`disconnect_quietly`) tears the relay
//!   down WITHOUT emitting, so the connection-state below is the single source.
//! * UNSOLICITED engine death (crash / external kill): the holder is still set, so
//!   the monitor's generation guard matches and it emits the single
//!   `engine.processExited`. `stop_engine` is not involved.
//!
//! Both drive the UI to a stopped/disconnected view; they never both fire for the
//! same cause.
//!
//! ## No-lock-across-await (mirrors `bridge_state`)
//!
//! The `ProcessState` Mutex is NEVER held across an `.await` that does process or
//! connect I/O. Each command takes the lock only to read / store / clear the
//! handle, drops it, then performs spawn / READY-wait / connect awaits with no
//! guard held. See the per-function comments marking each lock scope.
//!
//! ## Residual orphan risk
//!
//! `kill_on_drop(true)` is a safety net, not a guarantee: on an abrupt abort
//! (SIGKILL of the editor, power loss) neither the explicit app-exit hook nor the
//! drop runs, so the engine child can be orphaned. Hardening this with a Windows
//! Job Object / POSIX process group is deferred post-alpha.

use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tauri::async_runtime::JoinHandle;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{oneshot, Mutex};

use crate::bridge_state::{self, BridgeState};
use crate::dto::ConnectionStatePayload;
use crate::error::BackendError;
use crate::process;
use crate::protocol_names::events;

/// Default engine executable, used when `NORVES_ENGINE_PATH` is unset/blank.
///
/// The supported override is the `NORVES_ENGINE_PATH` environment variable (read
/// in [`launch_engine`]); the built mock-engine location varies per build tree,
/// so this default is a bare relative name resolved against the process working
/// directory. Operators are expected to set `NORVES_ENGINE_PATH` to the absolute
/// path of the engine binary for the alpha.
const DEFAULT_ENGINE_PATH: &str = "norves_mock_engine";

/// Environment variable that overrides the engine executable path.
const ENGINE_PATH_ENV: &str = "NORVES_ENGINE_PATH";

/// How long to wait for the engine's `READY <port>` stdout line before giving up
/// and killing the child.
const READY_TIMEOUT: Duration = Duration::from_secs(10);

/// The live half of a running engine process. The `Child` is intentionally NOT
/// stored here — the monitor task owns it so it alone may `wait()`.
struct ProcessHandle {
    /// PROCESS generation for this child, distinct from the connection
    /// generation in `BridgeState`. The monitor carries a copy and only emits /
    /// clears if it still matches the current process generation.
    generation: u64,
    /// Asks the monitor to `start_kill` + `wait` the child. A send error means
    /// the monitor already finished, which is fine.
    kill_tx: oneshot::Sender<()>,
    /// Join handle for the monitor task (kept so the handle is self-describing;
    /// the monitor is detached and clears the holder itself on exit).
    #[allow(dead_code)] // Retained for lifetime clarity; monitor self-clears.
    monitor: JoinHandle<()>,
}

/// Tauri-managed holder for the (at most one) running engine process. Separate
/// from `BridgeState`; guarded by a `tokio::sync::Mutex` never held across an
/// I/O `.await` (see module docs).
pub struct ProcessState {
    inner: Mutex<Option<ProcessHandle>>,
    /// Monotonic source of PROCESS generation ids, bumped per successful launch.
    next_process_gen: AtomicU64,
}

impl Default for ProcessState {
    fn default() -> Self {
        ProcessState {
            inner: Mutex::new(None),
            next_process_gen: AtomicU64::new(0),
        }
    }
}

impl ProcessState {
    /// Allocates a unique generation id for a newly launched process.
    fn alloc_process_gen(&self) -> u64 {
        self.next_process_gen.fetch_add(1, Ordering::Relaxed)
    }
}

/// Extracts a `(exit_code, signal)` pair from a process [`ExitStatus`] in the
/// shape the J1 params builder expects.
///
/// On Windows / normal exits the code is `Some` and there is no signal. On Unix a
/// signal-terminated child has `code() == None`; we then surface the signal
/// number both as a sentinel `exit_code` (128 + signal, the shell convention) and
/// as a `signal` string so the synthesized event is never missing `exitCode`.
/// Pure given the two `Option` inputs, so it is unit-tested without a runtime.
fn exit_code_and_signal(code: Option<i32>, signal: Option<i32>) -> (i64, Option<String>) {
    match (code, signal) {
        (Some(c), _) => (i64::from(c), None),
        (None, Some(sig)) => (128 + i64::from(sig), Some(format!("SIG{sig}"))),
        // No code and no signal: should not happen, but never panic.
        (None, None) => (-1, None),
    }
}

/// Pulls `(code, signal)` out of a real [`std::process::ExitStatus`]. On Unix the
/// signal comes from `ExitStatusExt`; on other platforms there is no signal.
fn split_exit_status(status: std::process::ExitStatus) -> (Option<i32>, Option<i32>) {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        (status.code(), status.signal())
    }
    #[cfg(not(unix))]
    {
        (status.code(), None)
    }
}

/// `launch_engine`: resolve + validate the engine path (backend-side only), spawn
/// the child, await its `READY <port>` line under a timeout, start the exit
/// monitor, then connect the bridge on that port. Rolls back the process on any
/// connect failure. Takes NO path from the frontend.
#[tauri::command]
pub async fn launch_engine(
    bridge: State<'_, BridgeState>,
    process_state: State<'_, ProcessState>,
    app: AppHandle,
) -> Result<ConnectionStatePayload, BackendError> {
    // 1. Brief lock: reject if a process is already running. Drop before awaits.
    {
        let guard = process_state.inner.lock().await;
        if guard.is_some() {
            return Err(BackendError::Process {
                message: "engine already running".to_owned(),
            });
        }
    } // guard dropped: all spawn / READY / connect I/O runs WITHOUT the lock.

    // 2. Resolve + validate the engine path entirely backend-side. The env read
    //    is the impure J3 side of the pure J1 resolver; config is None for alpha.
    let env_value = std::env::var(ENGINE_PATH_ENV).ok();
    let path =
        process::resolve_engine_path(env_value.as_deref(), None, Path::new(DEFAULT_ENGINE_PATH));
    process::validate_engine_path(&path)?;

    // 3. Pick a free loopback port for the engine to bind.
    let port = process::pick_free_port().map_err(|e| BackendError::Process {
        message: format!("failed to allocate a free port: {e}"),
    })?;

    // 4. Spawn the child with stdout piped for the READY handshake. kill_on_drop
    //    is the safety net; the explicit kill paths are preferred.
    let mut child = Command::new(&path)
        .arg("--bridge-port")
        .arg(port.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| BackendError::Process {
            message: format!("failed to spawn engine process: {e}"),
        })?;

    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            // Could not capture stdout: kill and fail.
            let _ = child.start_kill();
            return Err(BackendError::Process {
                message: "engine stdout was not captured".to_owned(),
            });
        }
    };

    // 5. Await the READY line under a timeout. On timeout / EOF / read error /
    //    malformed line, kill the child and roll back.
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    let read_result = tokio::time::timeout(READY_TIMEOUT, reader.read_line(&mut line)).await;
    match read_result {
        Ok(Ok(0)) => {
            let _ = child.start_kill();
            return Err(BackendError::Process {
                message: "engine closed stdout before sending READY".to_owned(),
            });
        }
        Ok(Ok(_)) => { /* got a line; parse below */ }
        Ok(Err(e)) => {
            let _ = child.start_kill();
            return Err(BackendError::Process {
                message: format!("failed to read engine READY line: {e}"),
            });
        }
        Err(_elapsed) => {
            let _ = child.start_kill();
            return Err(BackendError::Process {
                message: format!(
                    "timed out after {}s waiting for engine READY",
                    READY_TIMEOUT.as_secs()
                ),
            });
        }
    }
    // Parse READY; a malformed line / port mismatch kills the child via the
    // explicit kill below before returning the mapped error.
    if let Err(ready_err) = process::parse_ready_line(&line, port) {
        let _ = child.start_kill();
        return Err(ready_err.into());
    }

    // 6. Allocate the process generation, create the kill channel, spawn the
    //    monitor (which takes ownership of `child`), then store the handle.
    //
    //    Spawn-before-store window (alpha-accepted, self-healing): the monitor is
    //    spawned just before the holder is stored. If the child dies in this tiny
    //    gap, the monitor runs its generation guard against a holder that is still
    //    `None` (not yet stored) and emits nothing, then this code stores a handle
    //    whose child is already dead. That stale handle is self-healed by the very
    //    next step: `connect_on_port` cannot reach the dead engine, fails, and the
    //    connect-failure rollback below takes the handle and triggers its kill,
    //    clearing the holder. So no orphaned holder survives. Closing the window
    //    fully (store atomically before spawn) is deferred post-alpha; it would
    //    require restructuring the handle/monitor wiring and is not worth the risk.
    let generation = process_state.alloc_process_gen();
    let (kill_tx, kill_rx) = oneshot::channel::<()>();
    let monitor = spawn_exit_monitor(app.clone(), generation, child, kill_rx);

    {
        // Brief lock: store the live handle. No await between here and the
        // connect below holds this guard.
        let mut guard = process_state.inner.lock().await;
        *guard = Some(ProcessHandle {
            generation,
            kill_tx,
            monitor,
        });
    } // guard dropped before the connect awaits.

    // 7. Connect the bridge on the engine's port via the shared helper (which
    //    owns the Disconnected -> Connecting -> Connected phase transition and
    //    emits CONNECTION_STATE). On failure, roll the process back: trigger the
    //    kill and clear the holder.
    match bridge_state::connect_on_port(app.clone(), bridge.inner(), port).await {
        Ok(payload) => Ok(payload),
        Err(err) => {
            // Roll back: take the handle (if this generation still owns it) and
            // signal the monitor to kill. The monitor will also clear the holder
            // under its generation guard, but we clear eagerly here too.
            let handle = {
                let mut guard = process_state.inner.lock().await;
                match guard.take() {
                    Some(h) if h.generation == generation => Some(h),
                    // A newer launch already replaced us (should not happen while
                    // we hold no Connecting slot, but be defensive): put it back.
                    other => {
                        *guard = other;
                        None
                    }
                }
            };
            if let Some(handle) = handle {
                let _ = handle.kill_tx.send(());
            }
            Err(err)
        }
    }
}

/// Spawns the exit-monitor task that OWNS the `child`. It is the SOLE emitter of
/// `engine.processExited`.
///
/// It selects between the child's natural exit and a kill request on `kill_rx`;
/// either way it obtains the `ExitStatus`, then under a generation guard (so a
/// stale monitor from before a relaunch cannot clobber a newer process) it
/// clears the holder and emits exactly one exit event.
fn spawn_exit_monitor(
    app: AppHandle,
    my_gen: u64,
    mut child: Child,
    kill_rx: oneshot::Receiver<()>,
) -> JoinHandle<()> {
    tauri::async_runtime::spawn(async move {
        // Wait for natural exit OR a kill request, then ensure the child is
        // reaped so we always have an ExitStatus.
        let status = tokio::select! {
            natural = child.wait() => natural,
            _ = kill_rx => {
                let _ = child.start_kill();
                child.wait().await
            }
        };

        let status = match status {
            Ok(status) => status,
            Err(e) => {
                // Could not reap the child: log and clear the holder defensively
                // but do not emit a fabricated exit event.
                tracing::warn!(error = %e, "exit-monitor: failed to wait on engine child");
                let state = app.state::<ProcessState>();
                let mut guard = state.inner.lock().await;
                if matches!(&*guard, Some(h) if h.generation == my_gen) {
                    *guard = None;
                }
                drop(guard);
                return;
            }
        };

        let (code, signal) = split_exit_status(status);
        let (exit_code, signal_name) = exit_code_and_signal(code, signal);

        // Generation guard: read the current process generation, and only clear
        // + emit if we still own it. Decide under the lock, drop, then emit.
        let state = app.state::<ProcessState>();
        let should_emit = {
            let mut guard = state.inner.lock().await;
            let current_gen = guard.as_ref().map(|h| h.generation);
            match current_gen {
                Some(current) if process::monitor_should_emit_exit(current, my_gen) => {
                    // Still the current process: clear the holder.
                    *guard = None;
                    true
                }
                // A newer process replaced us, or the holder was already cleared
                // (e.g. by launch_engine's connect-failure rollback): do nothing.
                _ => false,
            }
        }; // guard dropped before the synchronous emit below.

        if should_emit {
            let params = process::build_process_exited_params(exit_code, signal_name);
            if let Err(err) = app.emit(
                events::ENGINE_PROCESS_EXITED,
                serde_json::Value::Object(params),
            ) {
                tracing::warn!(error = %err, "exit-monitor: failed to emit engine.processExited");
            }
        }
    })
}

/// `stop_engine`: best-effort graceful disconnect then hard kill. Idempotent: no
/// running process returns `Ok(())`.
///
/// Emits ONE disconnected `CONNECTION_STATE` so the UI reliably learns about a
/// user stop, and INTENTIONALLY suppresses `engine.processExited`: the holder is
/// cleared here BEFORE the kill is triggered, so the monitor's generation guard
/// sees `None` and does not emit (see the module "Stop / exit signalling
/// contract"). The courtesy disconnect tears the relay down WITHOUT emitting, so
/// this command is the SINGLE source of the disconnected state on a user stop.
#[tauri::command]
pub async fn stop_engine(
    bridge: State<'_, BridgeState>,
    process_state: State<'_, ProcessState>,
    app: AppHandle,
) -> Result<(), BackendError> {
    // 1. Take the handle out under a brief lock. None -> nothing to do.
    let handle = {
        let mut guard = process_state.inner.lock().await;
        guard.take()
    }; // guard dropped before any awaits below.

    let Some(handle) = handle else {
        return Ok(());
    };

    // 2. Best-effort graceful: close the WS so a real engine sees a clean
    //    disconnect. (The mock-engine stays resident on WS close by contract, so
    //    this is courtesy; the kill below is what actually stops it.) This tears
    //    the relay down WITHOUT emitting and leaves the BridgeState Phase as
    //    Disconnected. No lock on ProcessState is held across this await.
    bridge_state::disconnect_quietly(bridge.inner()).await;

    // 3. Trigger the kill. The monitor will start_kill + wait, but because the
    //    holder was already cleared in step 1 its generation guard sees `None`
    //    and it will NOT emit engine.processExited. A send error means the monitor
    //    already exited, which is fine. We do NOT await the child here.
    let _ = handle.kill_tx.send(());

    // 4. Emit the single disconnected connection-state for the user stop. This
    //    mirrors `bridge_disconnect`'s emit and is the only signal the UI gets for
    //    a user-initiated stop (the aborted relay's Closed path does not emit, and
    //    the monitor's exit event is suppressed per the contract above). `emit` is
    //    synchronous in Tauri 2, so no lock is held across an await here.
    let payload = ConnectionStatePayload::disconnected(Some("engine stopped".to_owned()));
    let _ = app.emit(events::CONNECTION_STATE, payload);

    Ok(())
}

/// Best-effort kill of a running engine on app exit, invoked from the Tauri
/// `RunEvent` hook (plan J3). Synchronous: takes the handle out under a blocking
/// lock and signals the monitor to kill. `kill_on_drop(true)` is the safety net,
/// but this explicit hook is preferred because drop is not guaranteed on abort
/// (see module docs on residual orphan risk).
pub fn kill_engine_on_exit(app: &AppHandle) {
    let state = app.state::<ProcessState>();
    // The RunEvent callback is synchronous; use blocking_lock. No await is held.
    let handle = {
        let mut guard = state.inner.blocking_lock();
        guard.take()
    };
    if let Some(handle) = handle {
        let _ = handle.kill_tx.send(());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_engine_path_is_a_bare_relative_name() {
        // The default is a relative name; the supported override is the env var.
        let path = Path::new(DEFAULT_ENGINE_PATH);
        assert!(path.is_relative(), "default engine path should be relative");
        assert_eq!(
            path.file_name().and_then(|s| s.to_str()),
            Some(DEFAULT_ENGINE_PATH)
        );
    }

    #[test]
    fn alloc_process_gen_is_monotonic() {
        let state = ProcessState::default();
        assert_eq!(state.alloc_process_gen(), 0);
        assert_eq!(state.alloc_process_gen(), 1);
        assert_eq!(state.alloc_process_gen(), 2);
    }

    #[test]
    fn exit_code_from_normal_exit_has_no_signal() {
        assert_eq!(exit_code_and_signal(Some(0), None), (0, None));
        assert_eq!(exit_code_and_signal(Some(137), None), (137, None));
        // A negative Windows-style code is preserved as i64.
        assert_eq!(
            exit_code_and_signal(Some(-1073741819), None),
            (-1073741819, None)
        );
    }

    #[test]
    fn exit_code_from_signal_uses_sentinel_and_signal_name() {
        // SIGKILL (9): 128 + 9 sentinel code, signal name surfaced.
        assert_eq!(
            exit_code_and_signal(None, Some(9)),
            (137, Some("SIG9".to_owned()))
        );
    }

    #[test]
    fn exit_code_with_neither_code_nor_signal_is_sentinel() {
        assert_eq!(exit_code_and_signal(None, None), (-1, None));
    }

    // NOTE: the launch -> connect -> stop happy path and the
    // kill / single-exit / relaunch-no-double-fire behaviors require a real
    // child process + runtime and are exercised by the env-gated e2e in J5.
}
