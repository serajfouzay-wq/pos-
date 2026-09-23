//! Hardware fingerprint.
//!
//! Four machine identifiers are read, normalised and turned into two
//! *independent* values:
//!
//! | Output                  | Construction                                         | Where it goes                  |
//! | ----------------------- | ---------------------------------------------------- | ------------------------------ |
//! | [`FingerprintHash`]     | `HMAC-SHA256(key = client_id, canonical components)` | the license token (`fp` claim) |
//! | [`DatabaseKey`]         | `HKDF-SHA256(ikm = components, salt = H(client_id))` | SQLCipher, never persisted     |
//!
//! The fingerprint hash is public (it is inside a readable JWT), so the
//! database key must not be derivable from it — hence a separate derivation
//! from the raw components rather than from the hash.
//!
//! Components (per the spec; never the MAC address):
//! 1. CPU brand string
//! 2. Windows `MachineGuid` (registry)
//! 3. Motherboard serial (WMI `Win32_BaseBoard`)
//! 4. `C:` volume serial (WMI `Win32_LogicalDisk`)
//!
//! Raw component values are secrets-adjacent: they are zeroized on drop and
//! must never be logged or sent anywhere.

use std::fmt;
use std::time::Duration;

use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use uuid::Uuid;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod windows;

/// Version tag mixed into every derivation. Bumping it re-keys every device,
/// so it only changes together with a migration plan.
const DOMAIN: &[u8] = b"pos-factory/hwid/v1";
const DB_KEY_INFO: &[u8] = b"pos-factory/sqlcipher-key/v1";
const DB_SALT_DOMAIN: &[u8] = b"pos-factory/sqlcipher-salt/v1";
const COLLECT_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HwidError {
    #[error("hardware identifier `{0}` is unavailable on this machine")]
    Missing(&'static str),
    #[error("could not read hardware identifiers: {0}")]
    Read(String),
    #[error("reading hardware identifiers timed out")]
    Timeout,
    #[error("hardware fingerprinting is not supported on this platform")]
    Unsupported,
}

/// The four raw identifiers, normalised. Zeroized on drop.
#[derive(Clone, PartialEq, Eq, Zeroize, ZeroizeOnDrop)]
pub struct HardwareComponents {
    cpu_brand: String,
    machine_guid: String,
    board_serial: String,
    volume_serial: String,
}

// Deliberately opaque: raw identifiers must never reach logs.
impl fmt::Debug for HardwareComponents {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("HardwareComponents { .. }")
    }
}

/// Trim, collapse internal whitespace, upper-case. Makes the fingerprint
/// immune to cosmetic differences between WMI/registry providers.
fn normalize(raw: &str) -> String {
    raw.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_uppercase()
}

impl HardwareComponents {
    /// Builds components from raw values. `machine_guid` is mandatory: it is
    /// the component that makes two identical off-the-shelf PCs distinct.
    /// The others may legitimately be empty (many boards report no serial).
    pub fn new(
        cpu_brand: &str,
        machine_guid: &str,
        board_serial: &str,
        volume_serial: &str,
    ) -> Result<Self, HwidError> {
        let components = Self {
            cpu_brand: normalize(cpu_brand),
            machine_guid: normalize(machine_guid),
            board_serial: normalize(board_serial),
            volume_serial: normalize(volume_serial),
        };
        if components.machine_guid.is_empty() {
            return Err(HwidError::Missing("MachineGuid"));
        }
        Ok(components)
    }

    /// Reads the identifiers of the current machine.
    ///
    /// Runs on a dedicated thread: WMI needs its own COM apartment, and the
    /// webview owns the main thread's.
    pub fn collect() -> Result<Self, HwidError> {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("pos-hwid".into())
            .spawn(move || {
                let _ = tx.send(collect_platform());
            })
            .map_err(|e| HwidError::Read(e.to_string()))?;
        rx.recv_timeout(COLLECT_TIMEOUT)
            .map_err(|_| HwidError::Timeout)?
    }

    /// Unambiguous encoding: domain tag, then each component length-prefixed,
    /// so `("AB", "C")` and `("A", "BC")` can never collide.
    fn canonical(&self) -> Zeroizing<Vec<u8>> {
        let mut out = Zeroizing::new(Vec::with_capacity(256));
        out.extend_from_slice(DOMAIN);
        for part in [
            &self.cpu_brand,
            &self.machine_guid,
            &self.board_serial,
            &self.volume_serial,
        ] {
            let bytes = part.as_bytes();
            let len = u32::try_from(bytes.len()).unwrap_or(u32::MAX);
            out.extend_from_slice(&len.to_be_bytes());
            out.extend_from_slice(bytes);
        }
        out
    }

    /// The device fingerprint embedded in license tokens.
    pub fn fingerprint(&self, client_id: Uuid) -> FingerprintHash {
        // HMAC keyed by the client id: the same PC yields unrelated
        // fingerprints for different clients (no cross-client correlation).
        let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(client_id.as_bytes())
            .expect("HMAC accepts keys of any length");
        mac.update(&self.canonical());
        FingerprintHash(hex::encode(mac.finalize().into_bytes()))
    }

    /// The SQLCipher key for this client's database on this machine.
    pub fn database_key(&self, client_id: Uuid) -> DatabaseKey {
        let salt = Sha256::new()
            .chain_update(DB_SALT_DOMAIN)
            .chain_update(client_id.as_bytes())
            .finalize();
        let hkdf = Hkdf::<Sha256>::new(Some(&salt), &self.canonical());
        let mut key = [0u8; 32];
        hkdf.expand(DB_KEY_INFO, &mut key)
            .expect("32 bytes is a valid HKDF-SHA256 output length");
        DatabaseKey(Zeroizing::new(key))
    }
}

/// Lower-case hex HMAC-SHA256. Public — safe to store and transmit.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FingerprintHash(String);

impl FingerprintHash {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for FingerprintHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// 256-bit SQLCipher raw key. Zeroized on drop; never persisted or logged.
pub struct DatabaseKey(Zeroizing<[u8; 32]>);

impl DatabaseKey {
    /// Value for `PRAGMA key`: SQLCipher's raw-key syntax `x'<64 hex>'`, which
    /// skips the passphrase KDF — the key is already uniformly random.
    pub fn sqlcipher_pragma_value(&self) -> Zeroizing<String> {
        Zeroizing::new(format!("x'{}'", hex::encode(*self.0)))
    }
}

impl fmt::Debug for DatabaseKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DatabaseKey(..)")
    }
}

#[cfg(windows)]
fn collect_platform() -> Result<HardwareComponents, HwidError> {
    windows::collect()
}

#[cfg(target_os = "linux")]
fn collect_platform() -> Result<HardwareComponents, HwidError> {
    linux::collect()
}

#[cfg(not(any(windows, target_os = "linux")))]
fn collect_platform() -> Result<HardwareComponents, HwidError> {
    Err(HwidError::Unsupported)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLIENT: Uuid = Uuid::from_u128(0x8f14e45f_ceea_467a_9a4e_3b2f1c9d0a11);

    fn sample() -> HardwareComponents {
        HardwareComponents::new(
            "Intel(R) Core(TM) i5-10400 CPU @ 2.90GHz",
            "4c4c4544-0042-3510-8052-b4c04f4e3232",
            "/7X2K2P2/CNCMK0009P012T/",
            "A1B2C3D4",
        )
        .expect("valid")
    }

    #[test]
    fn fingerprint_is_stable_and_hex() {
        let fp = sample().fingerprint(CLIENT);
        assert_eq!(fp, sample().fingerprint(CLIENT));
        assert_eq!(fp.as_str().len(), 64);
        assert!(fp
            .as_str()
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()));
    }

    #[test]
    fn normalisation_ignores_cosmetic_differences() {
        let messy = HardwareComponents::new(
            "  intel(r) core(tm)   i5-10400 cpu @ 2.90ghz ",
            "4C4C4544-0042-3510-8052-B4C04F4E3232",
            "/7x2k2p2/cncmk0009p012t/ ",
            "a1b2c3d4",
        )
        .expect("valid");
        assert_eq!(messy.fingerprint(CLIENT), sample().fingerprint(CLIENT));
    }

    #[test]
    fn every_component_matters() {
        let base = sample().fingerprint(CLIENT);
        let variants = [
            HardwareComponents::new(
                "AMD Ryzen 5",
                "4c4c4544-0042-3510-8052-b4c04f4e3232",
                "/7X2K2P2/CNCMK0009P012T/",
                "A1B2C3D4",
            ),
            HardwareComponents::new(
                "Intel(R) Core(TM) i5-10400 CPU @ 2.90GHz",
                "00000000-0042-3510-8052-b4c04f4e3232",
                "/7X2K2P2/CNCMK0009P012T/",
                "A1B2C3D4",
            ),
            HardwareComponents::new(
                "Intel(R) Core(TM) i5-10400 CPU @ 2.90GHz",
                "4c4c4544-0042-3510-8052-b4c04f4e3232",
                "OTHER",
                "A1B2C3D4",
            ),
            HardwareComponents::new(
                "Intel(R) Core(TM) i5-10400 CPU @ 2.90GHz",
                "4c4c4544-0042-3510-8052-b4c04f4e3232",
                "/7X2K2P2/CNCMK0009P012T/",
                "DEADBEEF",
            ),
        ];
        for variant in variants {
            assert_ne!(variant.expect("valid").fingerprint(CLIENT), base);
        }
    }

    #[test]
    fn length_prefixing_prevents_boundary_collisions() {
        let a = HardwareComponents::new("AB", "GUID", "C", "").expect("valid");
        let b = HardwareComponents::new("A", "GUID", "BC", "").expect("valid");
        assert_ne!(a.fingerprint(CLIENT), b.fingerprint(CLIENT));
    }

    #[test]
    fn fingerprints_are_unlinkable_across_clients() {
        let other = Uuid::from_u128(1);
        assert_ne!(sample().fingerprint(CLIENT), sample().fingerprint(other));
    }

    #[test]
    fn database_key_is_independent_of_the_public_fingerprint() {
        let key = sample().database_key(CLIENT).sqlcipher_pragma_value();
        let fp = sample().fingerprint(CLIENT);
        assert!(key.starts_with("x'") && key.ends_with('\'') && key.len() == 67);
        assert!(!key.contains(fp.as_str()));
        assert_eq!(
            *key,
            *sample().database_key(CLIENT).sqlcipher_pragma_value()
        );
        assert_ne!(
            *key,
            *sample()
                .database_key(Uuid::from_u128(1))
                .sqlcipher_pragma_value()
        );
    }

    #[test]
    fn machine_guid_is_mandatory() {
        assert_eq!(
            HardwareComponents::new("CPU", "  ", "S", "V"),
            Err(HwidError::Missing("MachineGuid"))
        );
    }

    #[test]
    fn debug_output_never_leaks_components() {
        let rendered = format!("{:?} {:?}", sample(), sample().database_key(CLIENT));
        assert!(!rendered.contains("A1B2C3D4") && !rendered.contains("4C4C"));
    }

    /// Exercises the real platform reader (WMI + registry on the Windows CI runner).
    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn collects_from_this_machine() {
        match HardwareComponents::collect() {
            Ok(components) => {
                assert_eq!(
                    components.fingerprint(CLIENT),
                    components.fingerprint(CLIENT)
                );
            }
            // Minimal Linux containers may have no machine-id; Windows must work.
            Err(HwidError::Missing(_)) if cfg!(target_os = "linux") => {}
            Err(other) => panic!("collection failed: {other}"),
        }
    }
}
