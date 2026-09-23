//! PURE, runtime-free domain logic for the engine process lifecycle (plan J1).
//!
//! This module deliberately contains NO process spawning, NO async, NO Tauri
//! wiring, and NO commands -- those belong to plan J3. What lives here is the
//! decision/parsing/shaping logic that J3 will call, plus two thin synchronous
//! I/O helpers (`pick_free_port`, `validate_engine_path`) that are trivial to
//! unit-test in isolation.
//!
//! Keeping these functions pure means J3's runtime glue can be reviewed for its
//! lifetime / thread / process concerns without re-litigating the parsing and
//! precedence rules, which are fully covered by the tests at the bottom of this
//! file.
//!
//! J3 (`process_runtime.rs`) now consumes every public item here, so the former
//! crate-wide `#![allow(dead_code)]` is gone; these functions are live.

use std::path::{Path, PathBuf};

use norves_bridge_core::Origin;

use crate::error::BackendError;

/// Resolves the engine executable path by precedence: explicit env value, then a
/// configured value, then the built-in default. Returns the chosen [`PathBuf`]
/// WITHOUT touching the filesystem (validation is [`validate_engine_path`]).
///
/// Precedence is `env` (the `NORVES_ENGINE_PATH` value, read by J3 and passed in
/// here) > `config` > `default`. An empty or whitespace-only string is treated
/// as ABSENT and the resolver falls through to the next source. This function is
/// pure: J3 reads the environment / config and passes the values in; this code
/// never reads `std::env` itself.
pub fn resolve_engine_path(env: Option<&str>, config: Option<&str>, default: &Path) -> PathBuf {
    resolve_engine_path_with_source(env, config, default).0
}

/// 解決したエンジンのパスがどこから来たか。設定欄に出所として見せる。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EnginePathSource {
    /// 環境変数 `NORVES_ENGINE_PATH`。
    Env,
    /// 保存済みのエンジン設定。
    Settings,
    /// 組み込みの既定値。
    Default,
}

/// [`resolve_engine_path`] と同じ優先順位で解決し、採用した出所も返す。
pub fn resolve_engine_path_with_source(
    env: Option<&str>,
    config: Option<&str>,
    default: &Path,
) -> (PathBuf, EnginePathSource) {
    if let Some(value) = first_non_blank([env]) {
        return (PathBuf::from(value), EnginePathSource::Env);
    }
    if let Some(value) = first_non_blank([config]) {
        return (PathBuf::from(value), EnginePathSource::Settings);
    }
    (default.to_path_buf(), EnginePathSource::Default)
}

/// Returns the first candidate that is `Some` and not blank (i.e. contains at
/// least one non-whitespace character), trimmed of surrounding whitespace.
fn first_non_blank<const N: usize>(candidates: [Option<&str>; N]) -> Option<&str> {
    candidates.into_iter().flatten().find_map(|raw| {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    })
}

/// Returns `Ok(())` if `path` exists and is a regular file; else
/// [`BackendError::Process`].
///
/// Defense-in-depth on the BACKEND-resolved path: the frontend never supplies a
/// path, so this is not user-input validation, just a guard that catches a
/// misconfigured / missing engine binary before J3 attempts to spawn it. The
/// error message intentionally leaks nothing beyond the path itself.
pub fn validate_engine_path(path: &Path) -> Result<(), BackendError> {
    if path.is_file() {
        Ok(())
    } else {
        Err(BackendError::Process {
            message: format!("engine executable not found: {}", path.display()),
        })
    }
}

/// エディタが自分で渡す、ユーザーの起動引数では使えない引数。
pub const BRIDGE_PORT_ARG: &str = "--bridge-port";
/// 保存できる起動引数の件数の上限。
///
/// 件数と長さの上限の積(16 KiB)に引用符やパスを足しても、Windows のコマンドラインの
/// 上限(32767 文字)に収まるように選ぶ。
pub const MAX_ENGINE_ARGS: usize = 32;
/// 起動引数 1 件の長さ(UTF-8 のバイト数)の上限。
pub const MAX_ENGINE_ARG_LEN: usize = 512;

/// ユーザーが入力した起動引数(1 要素 = 1 引数)を確かめ、保存する形にする。
///
/// 空白だけの要素は空行として捨てる。それ以外の要素は手を加えずに残す(引数はシェルを介さずに
/// そのまま渡すので、引用符や空白に特別な意味は無い)。次のどれかに当たると、何も捨てずにエラーを返す:
/// エディタが渡す `--bridge-port` とぶつかる引数、NUL か改行を含む引数、上限を超える件数・長さ。
pub fn normalize_engine_args(args: &[String]) -> Result<Vec<String>, BackendError> {
    let args: Vec<String> = args
        .iter()
        .filter(|arg| !arg.trim().is_empty())
        .cloned()
        .collect();
    if args.len() > MAX_ENGINE_ARGS {
        return Err(invalid_args(format!(
            "起動引数が多すぎます({} 件。上限は {MAX_ENGINE_ARGS} 件)",
            args.len()
        )));
    }
    for (index, arg) in args.iter().enumerate() {
        let line = index + 1;
        // 前後の空白を無視して比べる。空白を落としてから解釈するエンジンにも効かせるため。
        let trimmed = arg.trim();
        if trimmed == BRIDGE_PORT_ARG || trimmed.starts_with(&format!("{BRIDGE_PORT_ARG}=")) {
            return Err(invalid_args(format!(
                "{line} 件目: {BRIDGE_PORT_ARG} はエディタが渡すので指定できません"
            )));
        }
        if arg.contains('\0') {
            return Err(invalid_args(format!(
                "{line} 件目: NUL 文字を含む引数は使えません"
            )));
        }
        if arg.contains(['\n', '\r']) {
            return Err(invalid_args(format!(
                "{line} 件目: 改行を含む引数は使えません"
            )));
        }
        if arg.len() > MAX_ENGINE_ARG_LEN {
            return Err(invalid_args(format!(
                "{line} 件目: 引数が長すぎます({} バイト。上限は {MAX_ENGINE_ARG_LEN} バイト)",
                arg.len()
            )));
        }
    }
    Ok(args)
}

fn invalid_args(message: String) -> BackendError {
    BackendError::Settings { message }
}

/// エンジンに渡す引数の並びを組み立てる。ユーザーの引数を先に、`--bridge-port <port>` を最後に置く。
/// 引数はこの並びのまま `Command::args` に渡し、シェルは介さない。
pub fn build_engine_args(user_args: &[String], port: u16) -> Vec<String> {
    let mut argv = user_args.to_vec();
    argv.push(BRIDGE_PORT_ARG.to_owned());
    argv.push(port.to_string());
    argv
}

/// Why a stdout handshake line failed to yield the expected READY port.
///
/// Kept distinct from [`BackendError`] so tests can tell a malformed line apart
/// from a port mismatch; J3 maps either into [`BackendError::Process`].
#[derive(Debug, PartialEq, Eq)]
pub enum ReadyError {
    /// The line did not match `READY <u16>` after trimming (missing prefix,
    /// missing/extra tokens, non-numeric, or out-of-range port).
    Malformed { line: String },
    /// The line was well-formed but advertised a port other than the one we
    /// injected on the command line.
    PortMismatch { expected: u16, got: u16 },
}

impl std::fmt::Display for ReadyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadyError::Malformed { line } => {
                write!(f, "malformed engine READY line: {line:?}")
            }
            ReadyError::PortMismatch { expected, got } => {
                write!(
                    f,
                    "engine READY port mismatch: expected {expected}, got {got}"
                )
            }
        }
    }
}

impl From<ReadyError> for BackendError {
    fn from(err: ReadyError) -> Self {
        BackendError::Process {
            message: err.to_string(),
        }
    }
}

/// Parses the engine's stdout handshake line `READY <port>` and verifies the
/// port matches the one we injected.
///
/// Tolerates a trailing newline / CR and surrounding whitespace. The mock engine
/// prints exactly `READY <port>\n` to stdout. The accepted grammar is, after
/// trimming ASCII whitespace from both ends: the literal `READY`, exactly one
/// run of whitespace, then a base-10 `u16`. Anything else (missing prefix,
/// missing/extra tokens, non-numeric, or a value outside `u16`) is
/// [`ReadyError::Malformed`]; a parseable port that differs from `expected_port`
/// is [`ReadyError::PortMismatch`].
pub fn parse_ready_line(line: &str, expected_port: u16) -> Result<u16, ReadyError> {
    let malformed = || ReadyError::Malformed {
        line: line.to_owned(),
    };

    let trimmed = line.trim();
    // Require the READY keyword followed by exactly one port token.
    let mut tokens = trimmed.split_whitespace();
    let keyword = tokens.next().ok_or_else(malformed)?;
    if keyword != "READY" {
        return Err(malformed());
    }
    let port_token = tokens.next().ok_or_else(malformed)?;
    // Reject any trailing garbage after the port (e.g. "READY 1 extra").
    if tokens.next().is_some() {
        return Err(malformed());
    }
    // `u16::from_str` rejects non-numeric input and out-of-range values.
    let got: u16 = port_token.parse().map_err(|_| malformed())?;
    if got == expected_port {
        Ok(got)
    } else {
        Err(ReadyError::PortMismatch {
            expected: expected_port,
            got,
        })
    }
}

/// Binds `127.0.0.1:0` to obtain an OS-assigned free TCP port, then drops the
/// listener so the engine can bind it.
///
/// The close->bind race (we drop the listener, the engine then binds the same
/// port) is absorbed by the engine's bind-retry logic. This is the one piece of
/// real I/O in this module; it is synchronous and trivially testable.
pub fn pick_free_port() -> std::io::Result<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    // `listener` is dropped here, releasing the port for the engine to bind.
    Ok(port)
}

/// True only if a monitor task holding `monitor_gen` should emit the exit event,
/// i.e. it still owns the current process generation.
///
/// Mirrors the connection generation guard in `bridge_state.rs`
/// (`relay_should_reset_phase`). A relaunch bumps the current generation, so a
/// stale monitor (carrying an older generation) must NOT attribute its child's
/// exit to the new process.
pub fn monitor_should_emit_exit(current_process_gen: u64, monitor_gen: u64) -> bool {
    current_process_gen == monitor_gen
}

/// Builds the params map for a backend-SYNTHESIZED `engine.processExited` event.
///
/// Matches `engine.processExited.params.schema.json`: a required integer
/// `exitCode`, an optional non-empty `signal`, and `origin` fixed to the wire
/// value for editor-backend. `exitCode` is modeled as `i64` because process exit
/// codes can be negative or large on Windows. `origin` reuses the core
/// [`Origin`] enum's serialization to stay wire-transparent (it renders as
/// `"editor-backend"`). A `signal` of `None` -- or `Some("")`, which the schema
/// forbids via `minLength: 1` -- is omitted entirely.
pub fn build_process_exited_params(
    exit_code: i64,
    signal: Option<String>,
) -> serde_json::Map<String, serde_json::Value> {
    let mut params = serde_json::Map::new();
    params.insert(
        "exitCode".to_owned(),
        serde_json::Value::Number(exit_code.into()),
    );
    if let Some(signal) = signal {
        if !signal.is_empty() {
            params.insert("signal".to_owned(), serde_json::Value::String(signal));
        }
    }
    // Serialize the core enum rather than hardcoding the literal so the wire
    // value tracks `Origin::EditorBackend` (renders as "editor-backend").
    let origin = serde_json::to_value(Origin::EditorBackend)
        .expect("Origin serializes to a JSON string infallibly");
    params.insert("origin".to_owned(), origin);
    params
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- 起動引数 -------------------------------------------------------------

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    fn settings_error(result: Result<Vec<String>, BackendError>) -> String {
        match result {
            Err(BackendError::Settings { message }) => message,
            other => panic!("Settings エラーになるはず: {other:?}"),
        }
    }

    #[test]
    fn engine_args_drop_blank_lines_and_keep_others_verbatim() {
        let args = strings(&["", "--scene", "  ", "a b \"c\"", "\t", " --x=1 ", "$(rm)"]);
        assert_eq!(
            normalize_engine_args(&args).unwrap(),
            strings(&["--scene", "a b \"c\"", " --x=1 ", "$(rm)"])
        );
        assert_eq!(normalize_engine_args(&[]).unwrap(), Vec::<String>::new());
    }

    #[test]
    fn engine_args_reject_bridge_port() {
        for bad in [
            "--bridge-port",
            "--bridge-port=9000",
            "--bridge-port=",
            " --bridge-port",
            "\t--bridge-port=1 ",
        ] {
            let message = settings_error(normalize_engine_args(&strings(&["--ok", bad])));
            assert!(message.contains("2 件目"), "{bad:?}: {message}");
        }
        // 似ているだけの引数は通す。
        assert!(normalize_engine_args(&strings(&[
            "--bridge-portx",
            "-bridge-port",
            "x=--bridge-port"
        ]))
        .is_ok());
    }

    #[test]
    fn engine_args_reject_nul_and_line_breaks() {
        for bad in ["a\0b", "\0", "a\nb", "a\rb"] {
            settings_error(normalize_engine_args(&strings(&[bad])));
        }
    }

    #[test]
    fn engine_args_enforce_count_and_length_limits() {
        let at_limit = vec!["x".to_owned(); MAX_ENGINE_ARGS];
        assert_eq!(
            normalize_engine_args(&at_limit).unwrap().len(),
            MAX_ENGINE_ARGS
        );
        let over = vec!["x".to_owned(); MAX_ENGINE_ARGS + 1];
        settings_error(normalize_engine_args(&over));

        // 空行は件数に数えない。
        let mut with_blanks = at_limit.clone();
        with_blanks.extend(vec![String::new(); 10]);
        assert!(normalize_engine_args(&with_blanks).is_ok());

        let longest = "a".repeat(MAX_ENGINE_ARG_LEN);
        assert!(normalize_engine_args(&[longest]).is_ok());
        // 長さは文字数ではなくバイト数で数える。
        let multibyte = "あ".repeat(MAX_ENGINE_ARG_LEN / 3 + 1);
        settings_error(normalize_engine_args(&[multibyte]));
        settings_error(normalize_engine_args(&["a".repeat(MAX_ENGINE_ARG_LEN + 1)]));
    }

    #[test]
    fn engine_args_put_user_args_before_bridge_port() {
        assert_eq!(
            build_engine_args(&strings(&["--scene", "main.scene"]), 5123),
            strings(&["--scene", "main.scene", "--bridge-port", "5123"])
        );
        assert_eq!(build_engine_args(&[], 1), strings(&["--bridge-port", "1"]));
    }

    // --- resolve_engine_path -----------------------------------------------

    #[test]
    fn resolve_prefers_env_over_config_and_default() {
        let default = Path::new("default.exe");
        let resolved = resolve_engine_path(Some("env.exe"), Some("config.exe"), default);
        assert_eq!(resolved, PathBuf::from("env.exe"));
    }

    #[test]
    fn resolve_uses_config_when_env_absent() {
        let default = Path::new("default.exe");
        let resolved = resolve_engine_path(None, Some("config.exe"), default);
        assert_eq!(resolved, PathBuf::from("config.exe"));
    }

    #[test]
    fn resolve_falls_back_to_default_when_both_absent() {
        let default = Path::new("default.exe");
        let resolved = resolve_engine_path(None, None, default);
        assert_eq!(resolved, PathBuf::from("default.exe"));
    }

    #[test]
    fn resolve_treats_blank_strings_as_absent() {
        let default = Path::new("default.exe");
        // Empty env, whitespace-only config -> falls through to default.
        assert_eq!(
            resolve_engine_path(Some(""), Some("   "), default),
            PathBuf::from("default.exe")
        );
        // Empty env but real config -> uses config.
        assert_eq!(
            resolve_engine_path(Some("  "), Some("config.exe"), default),
            PathBuf::from("config.exe")
        );
    }

    #[test]
    fn resolve_trims_surrounding_whitespace() {
        let default = Path::new("default.exe");
        assert_eq!(
            resolve_engine_path(Some("  env.exe  "), None, default),
            PathBuf::from("env.exe")
        );
    }

    #[test]
    fn resolve_with_source_reports_env_settings_and_default() {
        let default = Path::new("default.exe");
        assert_eq!(
            resolve_engine_path_with_source(Some("env.exe"), Some("config.exe"), default),
            (PathBuf::from("env.exe"), EnginePathSource::Env)
        );
        assert_eq!(
            resolve_engine_path_with_source(Some("  "), Some("config.exe"), default),
            (PathBuf::from("config.exe"), EnginePathSource::Settings)
        );
        assert_eq!(
            resolve_engine_path_with_source(None, Some(" "), default),
            (PathBuf::from("default.exe"), EnginePathSource::Default)
        );
    }

    #[test]
    fn resolve_with_source_agrees_with_resolve() {
        // launch_engine と get_engine_settings が同じパスを指すこと。
        let default = Path::new("default.exe");
        let cases = [
            (Some("env.exe"), Some("config.exe")),
            (None, Some("config.exe")),
            (Some(""), None),
            (None, None),
        ];
        for (env, config) in cases {
            assert_eq!(
                resolve_engine_path(env, config, default),
                resolve_engine_path_with_source(env, config, default).0
            );
        }
    }

    // --- validate_engine_path ----------------------------------------------

    #[test]
    fn validate_accepts_existing_regular_file() {
        // Use this very source file as a guaranteed-existing regular file.
        let this_file = Path::new(file!());
        assert!(validate_engine_path(this_file).is_ok());
    }

    #[test]
    fn validate_rejects_missing_path() {
        let missing = Path::new("definitely-not-a-real-engine-binary-xyz.exe");
        let err = validate_engine_path(missing).expect_err("missing path must error");
        match err {
            BackendError::Process { message } => {
                // The message carries the path and nothing more sensitive.
                assert!(message.contains("definitely-not-a-real-engine-binary-xyz.exe"));
            }
            other => panic!("expected Process error, got {other:?}"),
        }
    }

    #[test]
    fn validate_rejects_directory() {
        // A directory exists but is not a regular file -> rejected.
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        assert!(validate_engine_path(dir).is_err());
    }

    // --- parse_ready_line ---------------------------------------------------

    #[test]
    fn parse_ready_accepts_exact_line_with_newline() {
        assert_eq!(parse_ready_line("READY 51234\n", 51234), Ok(51234));
    }

    #[test]
    fn parse_ready_tolerates_crlf_and_spaces() {
        assert_eq!(parse_ready_line("READY 51234\r\n", 51234), Ok(51234));
        assert_eq!(parse_ready_line("  READY 51234  ", 51234), Ok(51234));
    }

    #[test]
    fn parse_ready_rejects_port_mismatch() {
        assert_eq!(
            parse_ready_line("READY 9999\n", 51234),
            Err(ReadyError::PortMismatch {
                expected: 51234,
                got: 9999
            })
        );
    }

    #[test]
    fn parse_ready_rejects_non_numeric_port() {
        assert_eq!(
            parse_ready_line("READY abc", 51234),
            Err(ReadyError::Malformed {
                line: "READY abc".to_owned()
            })
        );
    }

    #[test]
    fn parse_ready_rejects_wrong_prefix() {
        assert_eq!(
            parse_ready_line("NOPE 1", 1),
            Err(ReadyError::Malformed {
                line: "NOPE 1".to_owned()
            })
        );
    }

    #[test]
    fn parse_ready_rejects_empty_line() {
        assert_eq!(
            parse_ready_line("", 1),
            Err(ReadyError::Malformed {
                line: String::new()
            })
        );
    }

    #[test]
    fn parse_ready_rejects_out_of_range_port() {
        // 70000 > u16::MAX -> not a valid port -> Malformed (not a mismatch).
        assert_eq!(
            parse_ready_line("READY 70000", 51234),
            Err(ReadyError::Malformed {
                line: "READY 70000".to_owned()
            })
        );
    }

    #[test]
    fn parse_ready_rejects_trailing_garbage() {
        assert_eq!(
            parse_ready_line("READY 51234 extra", 51234),
            Err(ReadyError::Malformed {
                line: "READY 51234 extra".to_owned()
            })
        );
    }

    #[test]
    fn ready_error_maps_to_process_backend_error() {
        let backend: BackendError = ReadyError::PortMismatch {
            expected: 1,
            got: 2,
        }
        .into();
        match backend {
            BackendError::Process { message } => assert!(message.contains("mismatch")),
            other => panic!("expected Process error, got {other:?}"),
        }
    }

    // --- pick_free_port -----------------------------------------------------

    #[test]
    fn pick_free_port_returns_bindable_nonzero_port() {
        let port = pick_free_port().expect("binding 127.0.0.1:0 should succeed");
        assert_ne!(port, 0, "OS-assigned port must be nonzero");
        // The port was released on drop, so we can bind it again here.
        let rebind = std::net::TcpListener::bind(("127.0.0.1", port));
        assert!(rebind.is_ok(), "released port should be re-bindable");
    }

    // --- monitor_should_emit_exit ------------------------------------------

    #[test]
    fn monitor_emits_only_on_matching_generation() {
        assert!(monitor_should_emit_exit(3, 3));
        // Stale monitor from an earlier process generation: must not emit.
        assert!(!monitor_should_emit_exit(3, 2));
        // Any mismatch (even a higher monitor gen, which shouldn't happen) -> no.
        assert!(!monitor_should_emit_exit(3, 4));
    }

    // --- build_process_exited_params ---------------------------------------

    #[test]
    fn exited_params_without_signal_has_exit_code_and_origin_only() {
        let params = build_process_exited_params(0, None);
        let keys: Vec<&str> = params.keys().map(String::as_str).collect();
        assert_eq!(keys, vec!["exitCode", "origin"]);
        assert_eq!(params["exitCode"], serde_json::json!(0));
        assert_eq!(params["origin"], serde_json::json!("editor-backend"));
        assert!(params["exitCode"].is_i64(), "exitCode must be an integer");
    }

    #[test]
    fn exited_params_with_signal_includes_signal() {
        let params = build_process_exited_params(137, Some("SIGKILL".to_owned()));
        assert_eq!(params["signal"], serde_json::json!("SIGKILL"));
        assert_eq!(params["exitCode"], serde_json::json!(137));
        assert_eq!(params["origin"], serde_json::json!("editor-backend"));
    }

    #[test]
    fn exited_params_omits_empty_signal() {
        // Schema requires signal minLength 1, so an empty string is dropped.
        let params = build_process_exited_params(1, Some(String::new()));
        assert!(
            !params.contains_key("signal"),
            "empty signal must be omitted"
        );
        let keys: Vec<&str> = params.keys().map(String::as_str).collect();
        assert_eq!(keys, vec!["exitCode", "origin"]);
    }

    #[test]
    fn exited_params_handles_negative_exit_code() {
        // Windows can report large/negative exit codes; i64 preserves them.
        let params = build_process_exited_params(-1073741819, None);
        assert_eq!(params["exitCode"], serde_json::json!(-1073741819_i64));
    }
}
