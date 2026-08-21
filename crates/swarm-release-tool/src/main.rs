//! Signs the release manifest. Never shipped: this is the other half of the
//! key whose public part is compiled into every build, and it belongs on the
//! machine that cuts releases, not on the machines that install them.
//!
//! ```text
//! swarm-release keygen PRIVATE_KEY_PATH   writes the key, prints only the public half
//! swarm-release sign PRIVATE_KEY_PATH     unsigned payload on stdin, signed document on stdout
//! ```

use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::process::ExitCode;

use base64ct::{Base64UrlUnpadded, Encoding};
use ed25519_dalek::{Signer, SigningKey};
use swarm_domain::{
    RELEASE_MANIFEST_SCHEMA_VERSION, ReleaseManifest, SignedReleaseManifest,
    canonical_release_manifest,
};

const USAGE: &str = "usage: swarm-release <keygen PRIVATE_KEY_PATH | sign PRIVATE_KEY_PATH>";

fn main() -> ExitCode {
    let mut arguments = env::args().skip(1);
    let result = match (
        arguments.next().as_deref(),
        arguments.next(),
        arguments.next(),
    ) {
        (Some("keygen"), Some(path), None) => keygen(Path::new(&path)),
        (Some("sign"), Some(path), None) => sign(Path::new(&path)),
        _ => Err(USAGE.to_owned()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("swarm-release: {message}");
            ExitCode::FAILURE
        }
    }
}

/// Writes a new signing key, and prints the verifying key and nothing else.
///
/// The private half is never printed, never logged, and never returned: the
/// only way to read it back is the file, which is created 0600 and refuses to
/// overwrite an existing one. Losing this key strands every install that
/// carries its public half, so it belongs in 1Password immediately.
fn keygen(path: &Path) -> Result<(), String> {
    if path.exists() {
        return Err(format!(
            "{} already exists; refusing to overwrite a signing key",
            path.display()
        ));
    }
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed).map_err(|error| format!("no secure randomness: {error}"))?;
    let signing_key = SigningKey::from_bytes(&seed);
    let verifying_key = Base64UrlUnpadded::encode_string(signing_key.verifying_key().as_bytes());

    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|error| format!("could not create {}: {error}", path.display()))?;
    file.write_all(Base64UrlUnpadded::encode_string(&seed).as_bytes())
        .and_then(|()| file.write_all(b"\n"))
        .map_err(|error| format!("could not write {}: {error}", path.display()))?;

    println!("{verifying_key}");
    eprintln!(
        "Signing key written to {}, readable only by you.\n\
         Store it in 1Password now and delete the file. It cannot be recovered,\n\
         and every install carrying the key above verifies nothing without it.",
        path.display()
    );
    Ok(())
}

/// Signs an unsigned manifest payload read from stdin.
fn sign(path: &Path) -> Result<(), String> {
    let encoded = fs::read_to_string(path)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;
    let seed: [u8; 32] = Base64UrlUnpadded::decode_vec(encoded.trim())
        .map_err(|_| "signing key is not base64url".to_owned())?
        .try_into()
        .map_err(|_| "signing key is not 32 bytes".to_owned())?;
    let signing_key = SigningKey::from_bytes(&seed);

    let mut document = String::new();
    io::stdin()
        .read_to_string(&mut document)
        .map_err(|error| format!("could not read the payload: {error}"))?;
    let payload: ReleaseManifest = serde_json::from_str(&document)
        .map_err(|error| format!("payload is not a manifest: {error}"))?;
    if payload.schema_version != RELEASE_MANIFEST_SCHEMA_VERSION {
        return Err(format!(
            "payload declares schema {} but this tool signs schema {RELEASE_MANIFEST_SCHEMA_VERSION}",
            payload.schema_version
        ));
    }

    let canonical = canonical_release_manifest(&payload)
        .map_err(|_| "payload could not be encoded".to_owned())?;
    let signed = SignedReleaseManifest {
        signature: Base64UrlUnpadded::encode_string(&signing_key.sign(&canonical).to_bytes()),
        payload,
    };
    let rendered = serde_json::to_string_pretty(&signed)
        .map_err(|error| format!("signed manifest could not be encoded: {error}"))?;
    println!("{rendered}");
    Ok(())
}
