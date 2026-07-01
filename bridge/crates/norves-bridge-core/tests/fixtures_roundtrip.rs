//! Fixture-driven conformance tests for the Bridge JSON codec (envelope layer).
//!
//! Walks `bridge/spec/fixtures` and exercises every `*.json` fixture against
//! [`decode_validated`] / [`encode_envelope`]. Fixtures are read-only ground
//! truth shared with the Python validator; this test never writes them.
//!
//! Three behavioural groups, classified purely by path (never by hard-coded
//! file names):
//!
//! * **positive** (`envelope/positive`, `methods/.../positive`,
//!   `events/.../positive`) — must `decode_validated` to `Ok` and round-trip
//!   value-equal through `encode_envelope`.
//! * **envelope-rejectable negatives** (`envelope/negative`) — must be rejected
//!   at the envelope layer (`decode_validated` returns `Err`).
//! * **payload-only negatives** (`methods/.../negative`, `events/.../negative`)
//!   — invalid only in their `params` / `result` payload, which this layer does
//!   NOT yet validate. They are therefore *accepted* (`Ok`) here. Enforcing
//!   them is deferred — see the payload test below.

use std::fs;
use std::path::{Path, PathBuf};

use norves_bridge_core::{decode_validated, encode_envelope};

/// Absolute path to `bridge/spec/fixtures`, resolved at compile time.
///
/// This crate lives at `bridge/crates/norves-bridge-core`, so the repository
/// root is three levels up from `CARGO_MANIFEST_DIR`.
fn fixtures_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../bridge/spec/fixtures")
}

/// Recursively collects every `*.json` file under `dir`. No `walkdir`
/// dependency — a small hand-rolled recursion keeps dev-deps empty.
fn collect_json_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries =
        fs::read_dir(dir).unwrap_or_else(|e| panic!("failed to read dir {}: {e}", dir.display()));
    for entry in entries {
        let entry = entry.expect("failed to read dir entry");
        let path = entry.path();
        if path.is_dir() {
            collect_json_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("json") {
            out.push(path);
        }
    }
}

/// All fixture `*.json` paths, with each path normalized to use `/` separators
/// so classification is OS-independent.
fn all_fixtures() -> Vec<(PathBuf, String)> {
    let root = fixtures_root();
    assert!(
        root.is_dir(),
        "fixtures root not found at {} (CARGO_MANIFEST_DIR resolution wrong?)",
        root.display()
    );
    let mut files = Vec::new();
    collect_json_files(&root, &mut files);
    files
        .into_iter()
        .map(|p| {
            let normalized = p.to_string_lossy().replace('\\', "/");
            (p, normalized)
        })
        .collect()
}

/// Path-based classification of a fixture.
#[derive(Debug, PartialEq, Eq)]
enum Group {
    /// Must decode+validate and round-trip value-equal.
    Positive,
    /// Must be rejected at the envelope layer.
    EnvelopeRejectable,
    /// Invalid only in payload; accepted at the envelope layer for now.
    PayloadOnly,
    /// Not a fixture we classify (e.g. README); excluded from all groups.
    Ignored,
}

fn classify(normalized_path: &str) -> Group {
    let is_positive = normalized_path.contains("/positive/");
    let is_negative = normalized_path.contains("/negative/");
    let is_envelope = normalized_path.contains("/fixtures/envelope/");
    let is_method = normalized_path.contains("/fixtures/methods/");
    let is_event = normalized_path.contains("/fixtures/events/");

    if is_positive && (is_envelope || is_method || is_event) {
        Group::Positive
    } else if is_negative && is_envelope {
        Group::EnvelopeRejectable
    } else if is_negative && (is_method || is_event) {
        Group::PayloadOnly
    } else {
        Group::Ignored
    }
}

fn read_fixture(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read fixture {}: {e}", path.display()))
}

/// Guards against a silent walk/classification regression: if a fixture is
/// added, removed, or moved between groups, these fixed counts break first and
/// point at the discrepancy. Counts are the approved Phase D2 totals.
#[test]
fn fixture_counts_are_exhaustive() {
    let mut positive = 0usize;
    let mut envelope_rejectable = 0usize;
    let mut payload_only = 0usize;
    let mut ignored = 0usize;

    for (_, normalized) in all_fixtures() {
        match classify(&normalized) {
            Group::Positive => positive += 1,
            Group::EnvelopeRejectable => envelope_rejectable += 1,
            Group::PayloadOnly => payload_only += 1,
            Group::Ignored => ignored += 1,
        }
    }

    assert_eq!(positive, 73, "positive fixture count drifted");
    assert_eq!(
        envelope_rejectable, 14,
        "envelope-rejectable fixture count drifted"
    );
    assert_eq!(payload_only, 66, "payload-only fixture count drifted");
    assert_eq!(
        positive + envelope_rejectable + payload_only,
        153,
        "total classified fixture count drifted"
    );
    assert_eq!(ignored, 0, "unexpectedly ignored a *.json fixture");
}

/// Positive fixtures must decode+validate and round-trip value-equal.
///
/// Equality is on `serde_json::Value`, not raw strings, so JSON field order and
/// whitespace differences are absorbed — only semantic content must match.
#[test]
fn roundtrip_positive_fixtures() {
    let mut checked = 0usize;
    for (path, normalized) in all_fixtures() {
        if classify(&normalized) != Group::Positive {
            continue;
        }
        let json = read_fixture(&path);

        let env = decode_validated(&json).unwrap_or_else(|e| {
            panic!(
                "positive fixture failed to decode+validate {}: {e}",
                path.display()
            )
        });

        let encoded = encode_envelope(&env).unwrap_or_else(|e| {
            panic!("positive fixture failed to encode {}: {e}", path.display())
        });

        let orig_value: serde_json::Value = serde_json::from_str(&json)
            .unwrap_or_else(|e| panic!("positive fixture not valid JSON {}: {e}", path.display()));
        let roundtrip_value: serde_json::Value = serde_json::from_str(&encoded)
            .unwrap_or_else(|e| panic!("re-encoded JSON not parseable {}: {e}", path.display()));

        assert_eq!(
            orig_value,
            roundtrip_value,
            "positive fixture did not round-trip value-equal: {}",
            path.display()
        );
        checked += 1;
    }
    assert_eq!(checked, 73, "expected to round-trip 73 positive fixtures");
}

/// Envelope-layer negatives must be rejected by `decode_validated` — either at
/// the `serde` deserialize step or at `Envelope::validate`. Any `Err` passes.
#[test]
fn envelope_negative_rejected() {
    let mut checked = 0usize;
    for (path, normalized) in all_fixtures() {
        if classify(&normalized) != Group::EnvelopeRejectable {
            continue;
        }
        let json = read_fixture(&path);
        assert!(
            decode_validated(&json).is_err(),
            "envelope negative fixture was unexpectedly accepted: {}",
            path.display()
        );
        checked += 1;
    }
    assert_eq!(checked, 14, "expected to reject 14 envelope negatives");
}

/// Payload-layer negatives are valid *envelopes*; only their `params`/`result`
/// payload is wrong. This phase validates the envelope layer only, so these are
/// expected to be ACCEPTED (`Ok`) here. Asserting `Ok` makes the current,
/// approved limitation explicit rather than silent: the round-trip / schema
/// contract for the payload is NOT verified by this test.
///
// TODO(payload): enforce method/event params/result schema in a later phase.
#[test]
fn payload_negative_accepted_at_envelope_layer() {
    let mut checked = 0usize;
    for (path, normalized) in all_fixtures() {
        if classify(&normalized) != Group::PayloadOnly {
            continue;
        }
        let json = read_fixture(&path);
        assert!(
            decode_validated(&json).is_ok(),
            "payload negative fixture was rejected at the envelope layer \
             (unexpected — payload validation is a future phase): {}",
            path.display()
        );
        checked += 1;
    }
    assert_eq!(checked, 66, "expected to accept 66 payload-only negatives");
}
