//! Privacy obfuscation for Codexp.
//!
//! Loads `~/.codexp/obfs.toml` (auto-generated on first run) and provides
//! virtual values to replace locally-sensed environment information before
//! it is sent to the model server.

use sha2::Digest;
use std::path::PathBuf;
use std::sync::LazyLock;
use std::sync::OnceLock;
use uuid::Uuid;

/// Default timezone used when obfs.toml is auto-generated.
const DEFAULT_TIMEZONE: &str = "America/New_York";
/// Default locale used when obfs.toml is auto-generated.
const DEFAULT_LOCALE: &str = "en_US.UTF-8";
/// Filename for the privacy configuration.
const OBFS_FILENAME: &str = "obfs.toml";

/// Fields in `[environment]` section.
#[derive(Debug, Clone, Default, serde::Deserialize, serde::Serialize)]
pub struct EnvironmentObfs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_date: Option<String>,
}

/// Fields in `[shell_environment]` section.
#[derive(Debug, Clone, Default, serde::Deserialize, serde::Serialize)]
pub struct ShellEnvironmentObfs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
}

/// Fields in `[metadata]` section.
#[derive(Debug, Clone, Default, serde::Deserialize, serde::Serialize)]
pub struct MetadataObfs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installation_id_passkey: Option<String>,
}

/// Root structure of `obfs.toml`.
#[derive(Debug, Clone, Default, serde::Deserialize, serde::Serialize)]
pub struct ObfsConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub environment: EnvironmentObfs,
    #[serde(default)]
    pub shell_environment: ShellEnvironmentObfs,
    #[serde(default)]
    pub metadata: MetadataObfs,
}

fn default_enabled() -> bool {
    true
}

/// Global lazy-loaded obfuscation config.
static OBFS: LazyLock<Option<ObfsConfig>> = LazyLock::new(load_or_create);

/// Returns the resolved `~/.codexp` path without depending on other crates.
fn codexp_home() -> Option<PathBuf> {
    if let Ok(val) = std::env::var("CODEXP_HOME") {
        let path = PathBuf::from(val);
        if path.is_dir() {
            return Some(path);
        }
    }
    let home = std::env::var("HOME").ok()?;
    let mut p = PathBuf::from(home);
    p.push(".codexp");
    Some(p)
}

/// Loads obfs.toml or creates it with defaults on first run.
fn load_or_create() -> Option<ObfsConfig> {
    let home = codexp_home()?;
    let path = home.join(OBFS_FILENAME);

    if path.exists() {
        let content = std::fs::read_to_string(&path).ok()?;
        match toml::from_str::<ObfsConfig>(&content) {
            Ok(config) => {
                if !config.enabled {
                    return None;
                }
                Some(config)
            }
            Err(_) => None,
        }
    } else {
        // First run: auto-generate with defaults and a random passkey.
        let passkey = Uuid::new_v4().to_string();
        let config = ObfsConfig {
            enabled: true,
            environment: EnvironmentObfs {
                timezone: Some(DEFAULT_TIMEZONE.to_string()),
                current_date: None,
            },
            shell_environment: ShellEnvironmentObfs {
                locale: Some(DEFAULT_LOCALE.to_string()),
            },
            metadata: MetadataObfs {
                installation_id_passkey: Some(passkey),
            },
        };

        // Create the codexp home directory if it doesn't exist yet.
        let _ = std::fs::create_dir_all(&home);

        // Serialize and write. Failure is non-fatal — obfuscation still works in memory.
        if let Ok(serialized) = toml::to_string_pretty(&config) {
            let _ = std::fs::write(&path, &serialized);
        }

        Some(config)
    }
}

/// Returns the configured timezone override, or `None` to pass through.
pub fn timezone() -> Option<String> {
    OBFS
        .as_ref()
        .and_then(|c| c.environment.timezone.clone())
}

/// Returns the configured date override, or `None` to pass through.
pub fn current_date() -> Option<String> {
    OBFS
        .as_ref()
        .and_then(|c| c.environment.current_date.clone())
}

/// Returns the configured locale override, or `None` to pass through.
pub fn locale() -> Option<String> {
    OBFS
        .as_ref()
        .and_then(|c| c.shell_environment.locale.clone())
}

/// Returns the derived installation_id from the passkey, or `None` to pass through.
pub fn installation_id() -> Option<String> {
    let passkey = OBFS
        .as_ref()
        .and_then(|c| c.metadata.installation_id_passkey.clone())?;
    Some(derive_installation_id(&passkey))
}

/// Derives a deterministic UUID v4 from a passkey string.
///
/// The same passkey always produces the same UUID. The output is a valid
/// UUID v4 string indistinguishable from a randomly generated one.
pub fn derive_installation_id(passkey: &str) -> String {
    let mut hasher = sha2::Sha256::new();
    hasher.update(passkey.as_bytes());
    let hash = hasher.finalize();

    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&hash[..16]);
    // Set version to 4 (bits 12-15 of byte 6).
    bytes[6] = (bytes[6] & 0x0F) | 0x40;
    // Set variant to 10xx (bits 6-7 of byte 8).
    bytes[8] = (bytes[8] & 0x3F) | 0x80;

    Uuid::from_bytes(bytes).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_installation_id_is_deterministic() {
        let a = derive_installation_id("test-passkey");
        let b = derive_installation_id("test-passkey");
        assert_eq!(a, b);
    }

    #[test]
    fn derive_installation_id_different_inputs_different_outputs() {
        let a = derive_installation_id("passkey-1");
        let b = derive_installation_id("passkey-2");
        assert_ne!(a, b);
    }

    #[test]
    fn derive_installation_id_is_valid_uuid_v4() {
        let id = derive_installation_id("test");
        let parsed = Uuid::parse_str(&id).expect("should be valid UUID");
        assert_eq!(parsed.get_version_num(), 4);
    }

    #[test]
    fn derive_installation_id_format_matches_uuid_v4() {
        let id = derive_installation_id("another-test");
        // UUID v4 format: 8-4-4-4-12, version digit '4', variant char in [89ab]
        assert_eq!(id.len(), 36);
        assert_eq!(id.as_bytes()[14], b'4');
        assert!(matches!(id.as_bytes()[19], b'8' | b'9' | b'a' | b'b'));
    }
}
