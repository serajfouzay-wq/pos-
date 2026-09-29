//! A file made the way the generator makes it, checked the way the till
//! checks it.

use std::io::Write;

use minisign::{KeyPair, PublicKey};

use super::*;

const CLIENT: Uuid = Uuid::from_u128(0x0199_0000_0000_7000_8000_0000_0000_0c1a);
const TARGET: &str = "windows-x86_64";
const FILE: &str = "Acme_0.2.0_x64-setup.exe";

struct Signer {
    pair: KeyPair,
}

impl Signer {
    fn new() -> Self {
        Self {
            pair: KeyPair::generate_encrypted_keypair(Some(String::new())).expect("key"),
        }
    }

    /// Base64 of the public key file, as the till is built with.
    fn public(&self) -> String {
        let pk = PublicKey::from_secret_key(&self.pair.sk).expect("pk");
        base64::engine::general_purpose::STANDARD.encode(pk.to_box().expect("box").into_string())
    }

    fn file(&self, installer: &[u8], comment: &str, manifest: serde_json::Value) -> Vec<u8> {
        let signature = minisign::sign(
            Some(&self.pair.pk),
            &self.pair.sk,
            Cursor::new(installer),
            Some(comment),
            None,
        )
        .expect("sign");
        let mut manifest = manifest;
        manifest["signature"] = base64::engine::general_purpose::STANDARD
            .encode(signature.into_string())
            .into();
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file(MANIFEST, options).expect("m");
        zip.write_all(manifest.to_string().as_bytes()).expect("w");
        zip.start_file(manifest["installer"].as_str().expect("name"), options)
            .expect("i");
        zip.write_all(installer).expect("w");
        zip.finish().expect("zip").into_inner()
    }
}

fn comment(client: Uuid, version: &str, target: &str, file: &str) -> String {
    format!(
        "timestamp:1790000000\tfile:{file}\tversion:{version}\tclient:{client}\ttarget:{target}"
    )
}

fn manifest(version: &str, target: &str) -> serde_json::Value {
    serde_json::json!({
        "format": 1,
        "client_id": CLIENT,
        "client_slug": "acme",
        "version": version,
        "target": target,
        "notes": " Faster receipts ",
        "installer": FILE,
        "created_at": "2026-09-29T10:00:00.000Z",
    })
}

#[test]
fn a_signed_file_for_this_till_is_accepted() {
    let signer = Signer::new();
    let key = signer.public();
    let file = signer.file(
        b"MZ new till",
        &comment(CLIENT, "0.2.0", TARGET, FILE),
        manifest("0.2.0", TARGET),
    );
    let update = verify(&file, Some(&key), CLIENT, "0.1.9", TARGET).expect("verified");
    assert_eq!(update.installer, b"MZ new till");
    assert_eq!(update.installer_name, FILE);
    assert_eq!(update.info.version, "0.2.0");
    assert_eq!(update.info.notes, "Faster receipts");
    assert!(update.info.newer);
    // The same file on a till already on 0.2.0 verifies but is not newer.
    let same = verify(&file, Some(&key), CLIENT, "0.2.0", TARGET).expect("verified");
    assert!(!same.info.newer);
}

#[test]
fn anything_else_is_refused() {
    let signer = Signer::new();
    let key = signer.public();
    let good = comment(CLIENT, "0.2.0", TARGET, FILE);
    let check = |file: &[u8]| verify(file, Some(&key), CLIENT, "0.1.9", TARGET).err();

    // No key built in, or not an update file at all.
    let file = signer.file(b"MZ", &good, manifest("0.2.0", TARGET));
    assert!(verify(&file, None, CLIENT, "0.1.9", TARGET).is_err());
    assert!(check(b"PK not really").is_some());

    // Another shop's file (signed for another client).
    let other = signer.file(
        b"MZ",
        &comment(Uuid::from_u128(1), "0.2.0", TARGET, FILE),
        manifest("0.2.0", TARGET),
    );
    assert!(check(&other)
        .expect("refused")
        .message
        .contains("another shop"));

    // Signed by another key (someone else's generator).
    let stranger = Signer::new().file(b"MZ", &good, manifest("0.2.0", TARGET));
    assert!(check(&stranger)
        .expect("refused")
        .message
        .contains("signature"));

    // The manifest raised to a version the signature does not name.
    let edited = signer.file(b"MZ", &good, manifest("9.9.9", TARGET));
    assert!(check(&edited).expect("refused").message.contains("altered"));

    // A Linux file on a Windows till.
    let linux = signer.file(
        b"ELF",
        &comment(CLIENT, "0.2.0", "linux-x86_64", FILE),
        manifest("0.2.0", "linux-x86_64"),
    );
    assert!(check(&linux)
        .expect("refused")
        .message
        .contains("linux-x86_64"));

    // A newer file format.
    let mut future = manifest("0.2.0", TARGET);
    future["format"] = 2.into();
    assert!(check(&signer.file(b"MZ", &good, future)).is_some());

    // A path in the installer name.
    let mut sneaky = manifest("0.2.0", TARGET);
    sneaky["installer"] = "../evil.exe".into();
    assert!(check(&signer.file(b"MZ", &good, sneaky)).is_some());

    assert_eq!(
        comment_field("a:1\tversion:0.2.0", "version"),
        Some("0.2.0")
    );
    assert!(this_target().ends_with(std::env::consts::ARCH));
}

#[cfg(unix)]
#[test]
fn an_appimage_is_swapped_whole() {
    let dir = tempfile::TempDir::new().expect("tmp");
    let current = dir.path().join("POS.AppImage");
    std::fs::write(&current, b"old").expect("old");
    replace_appimage(&current, b"new").expect("replace");
    assert_eq!(std::fs::read(&current).expect("read"), b"new");
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(&current)
        .expect("meta")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o755);
    assert_eq!(
        std::fs::read_dir(dir.path()).expect("dir").count(),
        1,
        "no leftovers"
    );
}
