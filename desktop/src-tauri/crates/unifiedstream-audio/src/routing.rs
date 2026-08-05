//! System audio routing on Linux: making the virtual sink the default output, and undoing it.
//!
//! PipeWire and PulseAudio have no way for a process to capture what is already playing on a
//! real output device, so the only route to system audio is to become the default output. That
//! makes this whole mechanism — the switch, the memo that survives a crash, and the sweep that
//! repairs one — specific to this platform, which is why it lives behind
//! [`crate::AudioRouting`] rather than in the application layer.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{AudioRouting, RoutingFuture, SINK_NODE_ID};

/// File name under the application config directory holding the memo.
const MEMO_FILE: &str = "speaker-routing.json";

/// What `pactl` remembered before the virtual sink took over the default output.
///
/// Persisted *before* switching, so a crash while routed can still restore the user's device
/// on the next launch — a hijacked default that survives our death is unacceptable.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RoutingMemo {
    /// `pactl` name of the sink that was the default before routing was enabled.
    previous_default: String,
}

/// Run one `pactl` invocation, returning trimmed stdout.
async fn pactl(args: &[&str]) -> Result<String, String> {
    let output = tokio::process::Command::new("pactl")
        .args(args)
        .output()
        .await
        .map_err(|e| format!("could not run pactl: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "pactl {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// The PipeWire/PulseAudio implementation of [`AudioRouting`], driven through `pactl`.
pub struct PactlRouting {
    /// Where the previous default output is remembered while routing is enabled.
    memo_path: PathBuf,
}

impl PactlRouting {
    /// Routing that keeps its memo under the given application config directory.
    #[must_use]
    pub fn new(config_dir: &Path) -> Self {
        Self {
            memo_path: config_dir.join(MEMO_FILE),
        }
    }

    /// Restore the remembered default output and forget the memo. Idempotent.
    async fn restore_now(&self) {
        let Ok(text) = std::fs::read_to_string(&self.memo_path) else {
            return; // no memo: routing was never enabled, or already restored
        };
        if let Ok(memo) = serde_json::from_str::<RoutingMemo>(&text) {
            match pactl(&["set-default-sink", &memo.previous_default]).await {
                Ok(_) => tracing::info!(sink = %memo.previous_default, "default output restored"),
                Err(e) => tracing::warn!(error = %e, "could not restore the default output"),
            }
        }
        let _ = std::fs::remove_file(&self.memo_path);
    }
}

impl AudioRouting for PactlRouting {
    fn enable(&self) -> RoutingFuture<'_, Result<(), String>> {
        Box::pin(async move {
            let current = pactl(&["get-default-sink"]).await?;
            if current != SINK_NODE_ID {
                let memo = RoutingMemo {
                    previous_default: current,
                };
                let text = serde_json::to_string(&memo)
                    .map_err(|e| format!("could not encode routing memo: {e}"))?;
                if let Some(parent) = self.memo_path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                std::fs::write(&self.memo_path, text)
                    .map_err(|e| format!("could not remember the previous output: {e}"))?;
            }
            pactl(&["set-default-sink", SINK_NODE_ID]).await?;
            tracing::info!("system audio routed to the virtual sink");
            Ok(())
        })
    }

    fn restore(&self) -> RoutingFuture<'_, ()> {
        Box::pin(self.restore_now())
    }

    fn sweep_stale(&self) -> RoutingFuture<'_, ()> {
        // Every clean path deletes the memo, so its presence at startup means the last run died
        // while the virtual sink held the default output. The sink itself died with that process
        // — PipeWire already fell back to a real device — but the *remembered* default may still
        // name our node, so the persisted previous device is put back.
        Box::pin(async move {
            if self.memo_path.is_file() {
                tracing::warn!("found a stale routing takeover from a previous run; restoring");
                self.restore_now().await;
            }
        })
    }
}
