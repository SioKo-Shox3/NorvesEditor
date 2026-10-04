//! 名前付きまとまりの所有権に使う秘密と、単調時計による期限。

use std::{fmt, time::Duration};
use tokio::time::Instant;

use crate::error::BackendError;

pub(super) const MAX_EDITS: usize = 128;
pub(super) const IDLE_TIMEOUT: Duration = Duration::from_secs(5 * 60);
pub(super) const TOTAL_TIMEOUT: Duration = Duration::from_secs(15 * 60);

#[derive(Clone, PartialEq)]
pub(super) struct GroupSecret(String);

impl fmt::Debug for GroupSecret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("GroupSecret([REDACTED])")
    }
}

impl GroupSecret {
    pub(super) fn generate() -> Result<Self, BackendError> {
        let mut bytes = [0_u8; 32];
        getrandom::fill(&mut bytes).map_err(|_| BackendError::Request {
            message: "まとまりIDの乱数を生成できません。".to_owned(),
        })?;
        Ok(Self(
            bytes.iter().map(|byte| format!("{byte:02x}")).collect(),
        ))
    }

    pub(super) fn expose(&self) -> &str {
        &self.0
    }

    pub(super) fn matches(&self, candidate: &str) -> bool {
        candidate.len() == self.0.len()
            && self
                .0
                .bytes()
                .zip(candidate.bytes())
                .fold(0_u8, |diff, (a, b)| diff | (a ^ b))
                == 0
    }
}

pub(super) async fn wait_deadline(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending::<()>().await,
    }
}
