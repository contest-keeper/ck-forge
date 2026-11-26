//! Makes the update key and signs installers with it. The files have the format of `tauri signer`
//! (a minisign file in base64, empty password), so the key made with it keeps working and
//! installed copies keep accepting updates.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use minisign::{KeyPair, PublicKeyBox, SecretKeyBox, SignatureBox};

const KEY_VARIABLE: &str = "CK_UPDATE_SIGNING_KEY";
const PUBKEY_VARIABLE: &str = "CK_UPDATE_PUBKEY";
const USAGE: &str = "usage: update-signer generate <private-out> <public-out> | sign <file>";

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let result = match args.as_slice() {
        [command, private, public] if command == "generate" => {
            generate(Path::new(private), Path::new(public))
        }
        [command, file] if command == "sign" => match std::env::var(KEY_VARIABLE) {
            Ok(key) => sign(
                &key,
                std::env::var(PUBKEY_VARIABLE).ok().as_deref(),
                Path::new(file),
            ),
            Err(_) => Err(format!("{KEY_VARIABLE} is not set or is not text")),
        },
        _ => Err(USAGE.to_owned()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("update-signer: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Writes a new key pair. Never replaces a file: losing a key that installed copies trust would
/// stop every later update.
fn generate(private: &Path, public: &Path) -> Result<(), String> {
    // Both are checked first, so a refusal does not leave half a pair behind.
    for path in [private, public] {
        if path.exists() {
            return Err(format!("{} already exists", path.display()));
        }
    }
    let pair = KeyPair::generate_encrypted_keypair(Some(String::new()))
        .map_err(|e| format!("could not make a key: {e}"))?;
    let secret = pair
        .sk
        .to_box(None)
        .map_err(|e| format!("could not write the private key: {e}"))?;
    let shared = pair
        .pk
        .to_box()
        .map_err(|e| format!("could not write the public key: {e}"))?;
    write_new(private, &STANDARD.encode(secret.into_string()))?;
    write_new(public, &STANDARD.encode(shared.into_string()))
}

fn write_new(path: &Path, text: &str) -> Result<(), String> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options
        .open(path)
        .and_then(|mut file| file.write_all(text.as_bytes()))
        .map_err(|e| format!("could not write {}: {e}", path.display()))
}

/// Checks the signature with the public key the app is built with: if the stored pair did not match,
/// every installed copy would refuse the update.
fn check(pubkey: &str, signature: &SignatureBox, file: &Path) -> Result<(), String> {
    let public = STANDARD
        .decode(pubkey.trim())
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .and_then(|text| PublicKeyBox::from_string(&text).ok())
        .and_then(|boxed| boxed.into_public_key().ok())
        .ok_or_else(|| format!("{PUBKEY_VARIABLE} is not a minisign public key in base64"))?;
    let data = File::open(file).map_err(|e| format!("could not read {}: {e}", file.display()))?;
    minisign::verify(&public, signature, data, true, false, false)
        .map_err(|_| format!("the signature does not match {PUBKEY_VARIABLE}"))
}

/// Writes `<file>.sig`, after checking it against `pubkey` when one is given. No error here may quote
/// the key.
fn sign(key: &str, pubkey: Option<&str>, file: &Path) -> Result<(), String> {
    let text = STANDARD
        .decode(key.trim())
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .ok_or_else(|| format!("{KEY_VARIABLE} is not a key in base64"))?;
    // The password must be given: `None` makes minisign ask for it on the terminal.
    let secret = SecretKeyBox::from_string(&text)
        .and_then(|boxed| boxed.into_secret_key(Some(String::new())))
        .map_err(|_| {
            format!("{KEY_VARIABLE} is not a minisign private key with an empty password")
        })?;
    let data = File::open(file).map_err(|e| format!("could not read {}: {e}", file.display()))?;
    let signature = minisign::sign(None, &secret, data, None, None)
        .map_err(|e| format!("could not sign {}: {e}", file.display()))?;
    if let Some(pubkey) = pubkey {
        check(pubkey, &signature, file)?;
    }
    let mut out = file.as_os_str().to_owned();
    out.push(".sig");
    std::fs::write(&out, STANDARD.encode(signature.into_string()))
        .map_err(|e| format!("could not write the signature: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use minisign_verify::{PublicKey, Signature};

    fn decoded(path: &Path) -> String {
        let text = std::fs::read_to_string(path).expect("file");
        String::from_utf8(STANDARD.decode(text).expect("base64")).expect("text")
    }

    #[test]
    fn a_generated_key_signs_what_the_app_accepts_and_is_never_replaced() {
        let folder = std::env::temp_dir().join(format!("update-signer-{}", std::process::id()));
        std::fs::create_dir(&folder).expect("folder");
        let (private, public) = (folder.join("private"), folder.join("public"));
        generate(&private, &public).expect("generated");
        let key = std::fs::read_to_string(&private).expect("key");

        // `tauri signer` hides the key behind scrypt even for an empty password. A library that
        // skips that step still passes the round trip below, but cannot read the key in use.
        let raw = |path| {
            let text = decoded(path);
            STANDARD
                .decode(text.lines().nth(1).expect("key line"))
                .expect("base64")
        };
        // The key number: in the clear in the public key, hidden at byte 54 of the private one.
        assert_ne!(raw(&private)[54..62], raw(&public)[2..10]);

        assert!(generate(&private, &folder.join("other")).is_err());
        assert!(generate(&folder.join("other"), &public).is_err());
        assert!(!folder.join("other").exists());
        assert_eq!(std::fs::read_to_string(&private).expect("key"), key);

        let setup = folder.join("setup.exe");
        std::fs::write(&setup, b"installer bytes").expect("setup");
        // A trailing newline is what a secret store or a shell may add.
        let shared = std::fs::read_to_string(&public).expect("public key");
        sign(&format!("{key}\n"), Some(&shared), &setup).expect("signed");

        // A public key of another pair is refused and nothing is written.
        let (other_private, other_public) = (folder.join("p2"), folder.join("pub2"));
        generate(&other_private, &other_public).expect("second pair");
        let wrong = std::fs::read_to_string(&other_public).expect("public key");
        let unsigned = folder.join("unsigned.exe");
        std::fs::write(&unsigned, b"x").expect("file");
        assert!(sign(&key, Some(&wrong), &unsigned).is_err());
        assert!(!folder.join("unsigned.exe.sig").exists());

        // The same checks as `verify` in app-egui/src/update.rs.
        let pubkey = PublicKey::decode(&decoded(&public)).expect("public key");
        let signature = Signature::decode(&decoded(&folder.join("setup.exe.sig"))).expect("sig");
        let mut verifier = pubkey.verify_stream(&signature).expect("same key");
        verifier.update(b"installer bytes");
        verifier.finalize().expect("signature matches");

        std::fs::remove_dir_all(folder).expect("cleaned");
    }

    #[test]
    fn a_bad_key_fails_without_quoting_it_and_writes_no_signature() {
        let file = Path::new("Cargo.toml");
        let not_a_key = STANDARD.encode("untrusted comment: x\nSECRET");
        for bad in ["", "SECRET!!", "/w==", &not_a_key] {
            let error = sign(bad, None, file).expect_err("refused");
            assert!(error.starts_with(KEY_VARIABLE), "{error}");
            assert!(!error.contains("SECRET"), "{error}");
        }
        assert!(!Path::new("Cargo.toml.sig").exists());
    }
}
