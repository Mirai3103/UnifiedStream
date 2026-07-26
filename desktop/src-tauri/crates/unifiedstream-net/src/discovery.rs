//! mDNS/DNS-SD advertisement, and the stable device identity that goes in the TXT record.
//!
//! The desktop advertises; the phone browses. PCs are stationary and long-running, phones move
//! between networks and sleep aggressively — and continuous advertisement from a phone is a
//! battery cost with nothing to show for it.

use std::collections::HashMap;
use std::path::Path;

use mdns_sd::{ServiceDaemon, ServiceInfo};
use serde::{Deserialize, Serialize};

use crate::error::{NetError, Result};
use crate::protocol::{txt_keys, SERVICE_TYPE};

/// Who this machine says it is, stable across restarts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceIdentity {
    /// Random UUID generated once, on first run.
    pub id: String,
    /// Human-readable name shown in the peer's device list.
    pub name: String,
}

impl DeviceIdentity {
    /// Load the identity from `path`, generating and persisting one if it is absent.
    ///
    /// A corrupt or unreadable file is replaced rather than treated as fatal: a device that
    /// cannot identify itself is useless, and the identity carries nothing worth recovering.
    ///
    /// # Errors
    ///
    /// Fails if the parent directory cannot be created or the file cannot be written.
    pub fn load_or_create(path: &Path, default_name: &str) -> Result<Self> {
        if let Ok(text) = std::fs::read_to_string(path) {
            match serde_json::from_str::<Self>(&text) {
                Ok(identity) if !identity.id.is_empty() => return Ok(identity),
                Ok(_) => tracing::warn!(?path, "identity file has an empty id; regenerating"),
                Err(e) => {
                    tracing::warn!(?path, error = %e, "identity file unreadable; regenerating");
                }
            }
        }

        let identity = Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: default_name.to_owned(),
        };
        identity.save(path)?;
        tracing::info!(?path, id = %identity.id, "generated device identity");

        Ok(identity)
    }

    /// Persist this identity, overwriting whatever is at `path`.
    ///
    /// # Errors
    ///
    /// Fails if the parent directory cannot be created or the file cannot be written.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| NetError::Discovery(format!("cannot encode identity: {e}")))?;
        std::fs::write(path, text)?;
        Ok(())
    }
}

/// This machine's hostname, or a generic fallback when it cannot be read.
#[must_use]
pub fn default_device_name() -> String {
    hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "UnifiedStream PC".to_owned())
}

/// What to publish in the service advertisement.
#[derive(Debug, Clone)]
pub struct AdvertiseConfig {
    /// This device's stable identity.
    pub identity: DeviceIdentity,
    /// TCP port the control channel listens on.
    pub control_port: u16,
    /// Capability tokens this build supports.
    pub caps: Vec<String>,
}

impl AdvertiseConfig {
    /// Build the TXT record for this configuration.
    #[must_use]
    pub fn txt_properties(&self) -> HashMap<String, String> {
        HashMap::from([
            (
                txt_keys::VERSION.to_owned(),
                crate::protocol::PROTOCOL_VERSION.to_string(),
            ),
            (txt_keys::NAME.to_owned(), self.identity.name.clone()),
            (txt_keys::ID.to_owned(), self.identity.id.clone()),
            (txt_keys::CAPS.to_owned(), self.caps.join(",")),
        ])
    }
}

/// An active mDNS advertisement. Dropping it withdraws the service.
pub struct Advertiser {
    daemon: ServiceDaemon,
    fullname: String,
}

impl std::fmt::Debug for Advertiser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Advertiser")
            .field("fullname", &self.fullname)
            .finish_non_exhaustive()
    }
}

impl Advertiser {
    /// Register the service on every non-loopback interface.
    ///
    /// Addresses are discovered by the daemon rather than passed in, so a machine on both
    /// Ethernet and Wi-Fi is reachable on both without the caller enumerating interfaces.
    ///
    /// # Errors
    ///
    /// Fails if the mDNS daemon cannot start or the service record is rejected.
    pub fn start(config: &AdvertiseConfig) -> Result<Self> {
        let daemon = ServiceDaemon::new()
            .map_err(|e| NetError::Discovery(format!("cannot start mDNS daemon: {e}")))?;

        let host_name = format!("{}.local.", sanitize_hostname(&config.identity.name));
        let service = ServiceInfo::new(
            SERVICE_TYPE,
            &config.identity.name,
            &host_name,
            "",
            config.control_port,
            config.txt_properties(),
        )
        .map_err(|e| NetError::Discovery(format!("invalid service info: {e}")))?
        .enable_addr_auto();

        let fullname = service.get_fullname().to_owned();
        daemon
            .register(service)
            .map_err(|e| NetError::Discovery(format!("cannot register service: {e}")))?;

        tracing::info!(%fullname, port = config.control_port, "advertising service");

        Ok(Self { daemon, fullname })
    }

    /// The registered service instance name.
    #[must_use]
    pub fn fullname(&self) -> &str {
        &self.fullname
    }

    /// Withdraw the advertisement, sending an mDNS goodbye so browsers drop us promptly.
    ///
    /// # Errors
    ///
    /// Currently infallible; returns [`Result`] so callers need not change when withdrawal
    /// gains a failure mode.
    pub fn shutdown(self) -> Result<()> {
        // Drop does the unregister; this just stops the responder thread rather than waiting
        // for the daemon's own teardown.
        self.unregister();
        let _ = self.daemon.shutdown();
        Ok(())
    }

    fn unregister(&self) {
        match self.daemon.unregister(&self.fullname) {
            Ok(_) => tracing::info!(fullname = %self.fullname, "withdrew advertisement"),
            Err(e) => tracing::warn!(fullname = %self.fullname, error = %e, "unregister failed"),
        }
    }
}

impl Drop for Advertiser {
    fn drop(&mut self) {
        // Without the goodbye, browsers keep showing this machine until the record's TTL
        // expires — a stale entry the user will try to tap.
        self.unregister();
    }
}

/// Reduce a display name to something usable as a DNS host label.
///
/// Device names come from hostnames and user input, so they can carry spaces and punctuation
/// that would produce an invalid record.
fn sanitize_hostname(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches('-');
    if trimmed.is_empty() {
        "unifiedstream".to_owned()
    } else {
        trimmed.to_lowercase()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("unifiedstream-test-{name}-{}", uuid::Uuid::new_v4()));
        path
    }

    #[test]
    fn load_or_create_should_generate_an_identity_when_absent() {
        let path = temp_path("identity-new");
        let identity = DeviceIdentity::load_or_create(&path, "test-pc").expect("must create");

        assert!(!identity.id.is_empty());
        assert_eq!(identity.name, "test-pc");
        assert!(path.is_file(), "identity must be persisted");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn load_or_create_should_return_the_same_identity_on_reload() {
        let path = temp_path("identity-stable");
        let first = DeviceIdentity::load_or_create(&path, "test-pc").expect("must create");
        let second = DeviceIdentity::load_or_create(&path, "different-name").expect("must load");

        assert_eq!(first, second, "identity must survive restart");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn load_or_create_should_replace_a_corrupt_file() {
        let path = temp_path("identity-corrupt");
        std::fs::write(&path, "not json at all").expect("write fixture");

        let identity = DeviceIdentity::load_or_create(&path, "test-pc").expect("must recover");
        assert!(!identity.id.is_empty());

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn load_or_create_should_replace_an_identity_with_an_empty_id() {
        let path = temp_path("identity-empty-id");
        std::fs::write(&path, r#"{"id":"","name":"x"}"#).expect("write fixture");

        let identity = DeviceIdentity::load_or_create(&path, "test-pc").expect("must recover");
        assert!(!identity.id.is_empty());

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn save_should_create_missing_parent_directories() {
        let mut path = temp_path("identity-nested");
        path.push("deeper");
        path.push("identity.json");

        let identity = DeviceIdentity {
            id: "abc".to_owned(),
            name: "test-pc".to_owned(),
        };
        identity.save(&path).expect("must save");
        assert!(path.is_file());

        if let Some(root) = path.parent().and_then(Path::parent) {
            let _ = std::fs::remove_dir_all(root);
        }
    }

    fn config() -> AdvertiseConfig {
        AdvertiseConfig {
            identity: DeviceIdentity {
                id: "3c6e0b8a-9c15-4f8d-b0a1-2d3e4f506172".to_owned(),
                name: "cachy-desktop".to_owned(),
            },
            control_port: crate::DEFAULT_CONTROL_PORT,
            caps: vec!["cam".to_owned(), "mic".to_owned(), "spk".to_owned()],
        }
    }

    #[test]
    fn txt_properties_should_carry_every_required_key() {
        let txt = config().txt_properties();
        assert_eq!(txt.get(txt_keys::VERSION).map(String::as_str), Some("1"));
        assert_eq!(
            txt.get(txt_keys::NAME).map(String::as_str),
            Some("cachy-desktop")
        );
        assert_eq!(
            txt.get(txt_keys::ID).map(String::as_str),
            Some("3c6e0b8a-9c15-4f8d-b0a1-2d3e4f506172")
        );
        assert_eq!(
            txt.get(txt_keys::CAPS).map(String::as_str),
            Some("cam,mic,spk")
        );
    }

    #[test]
    fn txt_properties_should_encode_no_capabilities_as_an_empty_string() {
        let mut cfg = config();
        cfg.caps.clear();
        assert_eq!(
            cfg.txt_properties().get(txt_keys::CAPS).map(String::as_str),
            Some("")
        );
    }

    #[test]
    fn sanitize_hostname_should_replace_spaces_and_punctuation() {
        assert_eq!(sanitize_hostname("Laffy's Desktop!"), "laffy-s-desktop");
    }

    #[test]
    fn sanitize_hostname_should_keep_a_valid_label_unchanged() {
        assert_eq!(sanitize_hostname("cachy-desktop"), "cachy-desktop");
    }

    #[test]
    fn sanitize_hostname_should_fall_back_when_nothing_usable_remains() {
        assert_eq!(sanitize_hostname("   "), "unifiedstream");
        assert_eq!(sanitize_hostname("!!!"), "unifiedstream");
    }

    #[test]
    fn default_device_name_should_never_be_empty() {
        assert!(!default_device_name().is_empty());
    }
}
