//! What an origin publishes about available releases, and what has to be true
//! before any of it is believed.
//!
//! A manifest is a small signed document listing the releases currently on
//! offer. It is fetched over HTTPS, but HTTPS is not what makes it
//! trustworthy: the signature is, and the key that checks the signature is
//! compiled into this binary rather than fetched alongside the document. See
//! ADR 0050.
//!
//! Withdrawal needs no flag. A manifest states what is offered *now*, so a
//! release is withdrawn by ceasing to list it.

use base64ct::{Base64UrlUnpadded, Encoding};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};

use crate::version::SwarmVersion;

/// The manifest shape this build understands. An origin serving anything else
/// is not partially read.
pub const RELEASE_MANIFEST_SCHEMA_VERSION: u32 = 1;

/// How far ahead of us an origin's clock may be before its manifest is refused.
const MAX_CLOCK_SKEW_SECONDS: i64 = 300;

/// The verifying key this build was compiled with, if any.
///
/// A build without one cannot check for releases at all, which is the correct
/// behaviour rather than a degraded one: there would be nothing to distinguish
/// a real manifest from any other document served at the same URL.
#[must_use]
pub fn compiled_release_verifying_key() -> Option<&'static str> {
    match option_env!("SWARM_RELEASE_VERIFYING_KEY") {
        Some(key) if !key.is_empty() => Some(key),
        _ => None,
    }
}

/// Why a manifest was not believed.
///
/// Deliberately coarse. A fetcher has no use for the distinction between a
/// forged signature and a corrupted one, and reporting it invites probing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReleaseManifestError {
    /// The document is not this manifest schema.
    UnsupportedSchema,
    /// The document describes a release in terms this build will not act on.
    Malformed,
    /// The document is stamped further ahead than clock skew explains.
    NotYetValid,
    /// No key was compiled in, so nothing can be checked.
    NoVerifyingKey,
    /// The signature does not belong to the compiled key.
    SignatureRejected,
}

/// What changed in a release, as the operator is told it.
///
/// CARRIED IN THE ARTIFACT BUNDLE, NOT THE MANIFEST, and that is forced rather
/// than preferred. The manifest is signed over the RE-SERIALIZED payload, so a
/// build that does not know a field drops it, recomputes a different canonical
/// form and rejects the signature -- for the whole document, which is how every
/// Hive learns any release exists. Bumping `schema_version` is no better: the
/// check is exact equality, so an older Hive answers `UnsupportedSchema` and
/// stops seeing releases entirely. Either way the damage lands on already
/// deployed Hives and removes the channel that would have carried the fix.
/// `an_unknown_field_in_the_signed_payload_breaks_verification_for_older_builds`
/// holds that line.
///
/// The bundle is still covered by the signature, transitively: the manifest
/// signs `artifact_sha256`, and these notes live inside the artifact that hash
/// is taken over. So they are as trustworthy as the release itself, they cost
/// no second fetch because the artifact is downloaded anyway, and adding to
/// them never touches the document older Hives have to verify.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct ReleaseNotes {
    pub schema: u32,
    /// Newest first, and CUMULATIVE rather than only this release.
    ///
    /// A Hive updating 0.8.14 to 0.8.19 fetches one artifact, so anything it
    /// skipped has to be inside that one. Carrying only the current release
    /// would show such an operator four releases' worth of changes as one line.
    pub releases: Vec<ReleaseVersionNotes>,
}

/// One release's worth of notes.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ReleaseVersionNotes {
    pub version: String,
    pub notes: Vec<ReleaseNote>,
}

/// One thing that changed, in the operator's terms.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ReleaseNote {
    pub summary: String,
    /// `feature` or `fix`, from the conventional-commit prefix.
    pub kind: String,
    /// Installed but NOT ACTIVE until the worker engine update runs.
    ///
    /// The terminal host is a separate service that deliberately survives an
    /// API restart so worker terminals are not killed mid-turn, so a release
    /// whose change is host-side is on disk and not in effect. Announcing such
    /// a change as available is a confident false claim about what the operator
    /// can now do, which is worse than not mentioning it -- so the modal says
    /// so instead of staying silent or lying.
    pub needs_worker_engine_update: bool,
}

impl ReleaseNotes {
    /// The notes for every release NEWER than `installed`, newest first.
    ///
    /// Filtered here rather than at render time so "what is new to me" has one
    /// definition. An unparseable version on either side is omitted rather than
    /// guessed at: showing the wrong release's notes is worse than showing none.
    #[must_use]
    pub fn newer_than(&self, installed: &str) -> Vec<&ReleaseVersionNotes> {
        let Some(installed) = SwarmVersion::parse(installed) else {
            return Vec::new();
        };
        let mut fresh: Vec<&ReleaseVersionNotes> = self
            .releases
            .iter()
            .filter(|entry| {
                SwarmVersion::parse(&entry.version)
                    .is_some_and(|version| version.supersedes(&installed))
            })
            .collect();
        fresh.sort_by(|left, right| right.version.cmp(&left.version));
        fresh
    }
}

/// A manifest as served, with the signature that covers it.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SignedReleaseManifest {
    pub payload: ReleaseManifest,
    /// Base64url, unpadded, over the canonical encoding of `payload`.
    pub signature: String,
}

/// The signed body.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ReleaseManifest {
    pub schema_version: u32,
    pub issued_at: i64,
    /// Every release currently offered. Absent means withdrawn.
    pub releases: Vec<ReleaseOffer>,
}

/// One release on offer.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ReleaseOffer {
    pub version: String,
    /// The terminal protocol this release speaks.
    pub protocol: String,
    pub artifact_url: String,
    /// Lowercase hex SHA-256 of the artifact, which the signature covers and
    /// the fetcher checks before anything is unpacked.
    pub artifact_sha256: String,
    pub artifact_bytes: u64,
    /// Lets the control room say whether installing this stops workers, at the
    /// moment of consent rather than half an hour later. ADR 0050 point 5.
    pub worker_engine_build_id: String,
    pub notes_url: Option<String>,
}

impl ReleaseOffer {
    /// Whether this offer is expressed in terms worth acting on.
    ///
    /// Checked during verification so a malformed offer never reaches a
    /// fetcher. `https` is required because a signed document that downgrades
    /// its own transport is still worth refusing.
    fn is_well_formed(&self) -> bool {
        SwarmVersion::parse(&self.version).is_some_and(|version| !version.is_development())
            && !self.protocol.is_empty()
            && self.artifact_url.starts_with("https://")
            && self.artifact_sha256.len() == 64
            && self
                .artifact_sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            && self.artifact_bytes > 0
            && !self.worker_engine_build_id.is_empty()
            && self
                .notes_url
                .as_ref()
                .is_none_or(|url| url.starts_with("https://"))
    }

    /// The parsed version, which is known to parse once verified.
    #[must_use]
    pub fn parsed_version(&self) -> Option<SwarmVersion> {
        SwarmVersion::parse(&self.version)
    }
}

impl ReleaseManifest {
    /// The newest offer this Hive could actually install: same terminal
    /// protocol, and a genuine upgrade.
    ///
    /// A development build gets nothing here. `SwarmVersion::supersedes`
    /// refuses to order one in either direction, so replacing a working copy's
    /// binary with a release can never be proposed. ADR 0050 point 4.
    #[must_use]
    pub fn upgrade_for(&self, current: &SwarmVersion) -> Option<&ReleaseOffer> {
        self.installable_offers()
            .into_iter()
            .filter(|(version, _)| version.supersedes(current))
            .max_by(|(left, _), (right, _)| left.cmp(right))
            .map(|(_, offer)| offer)
    }

    /// The newest offer this Hive could run at all, upgrade or not.
    ///
    /// A working copy is told a release exists even though it is offered
    /// nothing: "0.2.0 has been released" is useful, and pretending otherwise
    /// would leave a developer worse informed than a user.
    #[must_use]
    pub fn newest_offer(&self) -> Option<&ReleaseOffer> {
        self.installable_offers()
            .into_iter()
            .max_by(|(left, _), (right, _)| left.cmp(right))
            .map(|(_, offer)| offer)
    }

    /// Every offer this Hive could install, newest last.
    ///
    /// THE PROTOCOL NO LONGER GATES DISCOVERY, and removing that is the whole
    /// point rather than a simplification.
    ///
    /// This filtered on `offer.protocol == protocol`, an exact match against
    /// the Hive's own compiled `PROTOCOL_VERSION`. That was correct while
    /// `update` REFUSED a protocol change: a release the installer would not
    /// take was not worth offering. ccc4872 made `update` perform the migration
    /// instead, so the filter began excluding releases the Hive can install.
    ///
    /// v0.9.0 is what that cost. Fully published, signed, artifact fetchable
    /// with matching hashes, every deploy step green — and visible to nobody,
    /// because it declared protocol 10 and every Hive in the field compiled 9.
    /// The operator: "I keep checking and my copy of swarm doesn't see anything
    /// newer than 0.8.17."
    ///
    /// An offer whose version cannot be parsed is still dropped, because
    /// nothing downstream can order it — but the manifest is asked whether it
    /// listed anything, so dropping them all is reported rather than read as
    /// being up to date.
    fn installable_offers(&self) -> Vec<(SwarmVersion, &ReleaseOffer)> {
        self.releases
            .iter()
            .filter_map(|offer| Some((offer.parsed_version()?, offer)))
            .collect()
    }
}

/// The bytes a signature is made over.
///
/// # Errors
/// Returns an error when the payload cannot be encoded.
pub fn canonical_release_manifest(
    payload: &ReleaseManifest,
) -> Result<Vec<u8>, ReleaseManifestError> {
    serde_json::to_vec(payload).map_err(|_| ReleaseManifestError::Malformed)
}

/// Checks a manifest against the compiled verifying key.
///
/// Everything the document asserts about itself is checked before the
/// signature, and nothing it asserts is used before the signature passes.
///
/// # Errors
/// Returns the coarse reason the document was not believed.
pub fn verify_release_manifest<'a>(
    document: &'a SignedReleaseManifest,
    verifying_key: Option<&str>,
    now: i64,
) -> Result<&'a ReleaseManifest, ReleaseManifestError> {
    let payload = &document.payload;
    if payload.schema_version != RELEASE_MANIFEST_SCHEMA_VERSION {
        return Err(ReleaseManifestError::UnsupportedSchema);
    }
    if payload.issued_at < 0 || !payload.releases.iter().all(ReleaseOffer::is_well_formed) {
        return Err(ReleaseManifestError::Malformed);
    }
    if payload.issued_at > now.saturating_add(MAX_CLOCK_SKEW_SECONDS) {
        return Err(ReleaseManifestError::NotYetValid);
    }
    let verifying_key = verifying_key.ok_or(ReleaseManifestError::NoVerifyingKey)?;
    let key: [u8; 32] = Base64UrlUnpadded::decode_vec(verifying_key)
        .map_err(|_| ReleaseManifestError::NoVerifyingKey)?
        .try_into()
        .map_err(|_| ReleaseManifestError::NoVerifyingKey)?;
    let signature: [u8; 64] = Base64UrlUnpadded::decode_vec(&document.signature)
        .map_err(|_| ReleaseManifestError::SignatureRejected)?
        .try_into()
        .map_err(|_| ReleaseManifestError::SignatureRejected)?;
    let canonical = canonical_release_manifest(payload)?;
    VerifyingKey::from_bytes(&key)
        .and_then(|key| key.verify(&canonical, &Signature::from_bytes(&signature)))
        .map_err(|_| ReleaseManifestError::SignatureRejected)?;
    Ok(payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    const PROTOCOL: &str = "3";

    fn keypair() -> (SigningKey, String) {
        let signing_key = SigningKey::from_bytes(&[7u8; 32]);
        let public = Base64UrlUnpadded::encode_string(signing_key.verifying_key().as_bytes());
        (signing_key, public)
    }

    fn offer(version: &str, engine: &str) -> ReleaseOffer {
        ReleaseOffer {
            version: version.to_owned(),
            protocol: PROTOCOL.to_owned(),
            artifact_url: format!("https://releases.example/swarm-{version}.tar.gz"),
            artifact_sha256: "a".repeat(64),
            artifact_bytes: 4096,
            worker_engine_build_id: engine.to_owned(),
            notes_url: None,
        }
    }

    fn sign(payload: ReleaseManifest, signing_key: &SigningKey) -> SignedReleaseManifest {
        let canonical = canonical_release_manifest(&payload).unwrap();
        SignedReleaseManifest {
            signature: Base64UrlUnpadded::encode_string(&signing_key.sign(&canonical).to_bytes()),
            payload,
        }
    }

    fn manifest(releases: Vec<ReleaseOffer>) -> ReleaseManifest {
        ReleaseManifest {
            schema_version: RELEASE_MANIFEST_SCHEMA_VERSION,
            issued_at: 1_000,
            releases,
        }
    }

    fn note(summary: &str, engine: bool) -> ReleaseNote {
        ReleaseNote {
            summary: summary.to_owned(),
            kind: "feature".to_owned(),
            needs_worker_engine_update: engine,
        }
    }

    fn version_notes(version: &str, notes: Vec<ReleaseNote>) -> ReleaseVersionNotes {
        ReleaseVersionNotes {
            version: version.to_owned(),
            notes,
        }
    }

    /// A Hive that skipped versions sees everything it missed, not only the newest.
    ///
    /// This is the common case rather than the exotic one: anyone away for a day
    /// updates across several releases at once. Showing them only the newest
    /// would silently drop the rest, and they would have no way to know.
    #[test]
    fn release_notes_cover_every_version_the_operator_skipped() {
        let notes = ReleaseNotes {
            schema: 1,
            releases: vec![
                version_notes("0.8.19", vec![note("A shell opens from the menu", false)]),
                version_notes("0.8.17", vec![note("Workers survive a reload", true)]),
                version_notes(
                    "0.8.16",
                    vec![note("Attachments stop being rejected", false)],
                ),
                version_notes("0.8.14", vec![note("Older than the operator has", false)]),
            ],
        };

        let fresh = notes.newer_than("0.8.15");
        assert_eq!(
            fresh
                .iter()
                .map(|entry| entry.version.as_str())
                .collect::<Vec<_>>(),
            vec!["0.8.19", "0.8.17", "0.8.16"],
            "everything newer than the running version, newest first"
        );
        assert!(
            fresh.iter().all(|entry| entry.version.as_str() != "0.8.14"),
            "a release the operator already had is not new to them"
        );

        // The host-side marker survives selection, because the modal has to be
        // able to say a change is installed but not yet in effect.
        assert!(
            fresh
                .iter()
                .flat_map(|entry| entry.notes.iter())
                .any(|note| note.needs_worker_engine_update),
            "a host-side change must stay distinguishable after filtering"
        );

        assert!(
            notes.newer_than("0.8.19").is_empty(),
            "nothing is new to a Hive already on the newest release"
        );
        assert!(
            notes.newer_than("not-a-version").is_empty(),
            "an unreadable running version shows nothing rather than guessing"
        );
    }

    /// THE MANIFEST CANNOT GAIN A FIELD. This is why release notes travel in
    /// the artifact bundle rather than here.
    ///
    /// `canonical_release_manifest` re-serializes the DESERIALIZED struct rather
    /// than hashing the bytes as received. So a build that does not know a
    /// field drops it on the way in, re-encodes without it, and computes a
    /// different canonical form than the signer signed. The signature then
    /// fails -- not for the release carrying the new field, but for the WHOLE
    /// MANIFEST, which is the document every Hive reads to learn that any
    /// release exists at all.
    ///
    /// Bumping `schema_version` instead is no better: the check above is exact
    /// equality, so an older Hive answers `UnsupportedSchema` and stops seeing
    /// releases entirely.
    ///
    /// Either way the failure lands on every ALREADY DEPLOYED Hive and takes
    /// away the mechanism that would have delivered the fix. That asymmetry is
    /// what makes this worth a test rather than a comment.
    #[test]
    fn an_unknown_field_in_the_signed_payload_breaks_verification_for_older_builds() {
        let (signing_key, public) = keypair();
        let document = sign(manifest(vec![offer("0.3.0", "engine-c")]), &signing_key);

        // Exactly what a NEWER signer produces: the same payload plus a field
        // this build has never heard of, signed over the fuller encoding.
        let mut raw: serde_json::Value = serde_json::to_value(&document).unwrap();
        let newer = raw["payload"]["releases"][0].as_object_mut().unwrap();
        newer.insert("notes".to_owned(), serde_json::json!(["something new"]));
        let canonical_for_newer = serde_json::to_vec(&raw["payload"]).unwrap();
        raw["signature"] = serde_json::json!(Base64UrlUnpadded::encode_string(
            &signing_key.sign(&canonical_for_newer).to_bytes()
        ));

        // This build reads it. serde drops the field it does not know, so the
        // canonical form it recomputes is the OLD one and the signature over
        // the new one cannot match.
        let as_this_build_sees_it: SignedReleaseManifest =
            serde_json::from_value(raw).expect("an unknown field still deserializes");
        assert!(
            matches!(
                verify_release_manifest(&as_this_build_sees_it, Some(&public), 2_000),
                Err(ReleaseManifestError::SignatureRejected)
            ),
            "adding a field to the signed payload must be understood as breaking \
             every older Hive's ability to verify ANY release"
        );
    }

    #[test]
    fn a_signed_manifest_offers_the_newest_release_that_supersedes_this_one() {
        let (signing_key, public) = keypair();
        let document = sign(
            manifest(vec![
                offer("0.1.0", "engine-a"),
                offer("0.3.0", "engine-c"),
                offer("0.2.0", "engine-b"),
            ]),
            &signing_key,
        );

        let payload = verify_release_manifest(&document, Some(&public), 2_000).unwrap();
        let current = SwarmVersion::parse("0.1.0").unwrap();
        let upgrade = payload.upgrade_for(&current).unwrap();

        assert_eq!(upgrade.version, "0.3.0");
        assert_eq!(upgrade.worker_engine_build_id, "engine-c");
    }

    /// The whole design rests on this. A document altered after signing must
    /// not be readable, however plausible its contents are.
    #[test]
    fn a_manifest_edited_after_signing_is_refused() {
        let (signing_key, public) = keypair();
        let mut document = sign(manifest(vec![offer("0.2.0", "engine-b")]), &signing_key);
        document.payload.releases[0].artifact_url =
            "https://elsewhere.example/swarm.tar.gz".to_owned();

        assert_eq!(
            verify_release_manifest(&document, Some(&public), 2_000).unwrap_err(),
            ReleaseManifestError::SignatureRejected
        );
    }

    /// A correctly-formed manifest signed by the wrong key is the attack this
    /// exists to stop, and it is not distinguished from a corrupted one.
    #[test]
    fn a_manifest_signed_by_another_key_is_refused() {
        let (_, public) = keypair();
        let impostor = SigningKey::from_bytes(&[9u8; 32]);
        let document = sign(manifest(vec![offer("9.9.9", "engine-x")]), &impostor);

        assert_eq!(
            verify_release_manifest(&document, Some(&public), 2_000).unwrap_err(),
            ReleaseManifestError::SignatureRejected
        );
    }

    /// A build with no compiled key does not fall back to trusting the
    /// document. There is nothing to fall back to.
    #[test]
    fn a_build_without_a_compiled_key_checks_nothing() {
        let (signing_key, _) = keypair();
        let document = sign(manifest(vec![offer("0.2.0", "engine-b")]), &signing_key);

        assert_eq!(
            verify_release_manifest(&document, None, 2_000).unwrap_err(),
            ReleaseManifestError::NoVerifyingKey
        );
    }

    /// "A release that cannot speak this Hive's terminal protocol is not
    /// offered, because offering it would make the operator the one who
    /// discovers the incompatibility." ADR 0050 point 2.
    #[test]
    fn the_newest_release_wins_whatever_protocol_it_declares() {
        let (signing_key, public) = keypair();
        let mut newer = offer("0.9.0", "engine-z");
        newer.protocol = "4".to_owned();
        let document = sign(
            manifest(vec![newer, offer("0.2.0", "engine-b")]),
            &signing_key,
        );

        let payload = verify_release_manifest(&document, Some(&public), 2_000).unwrap();
        let current = SwarmVersion::parse("0.1.0").unwrap();

        // THIS TEST ASSERTED THE OPPOSITE, and it was right to until ccc4872.
        // It pinned "a release declaring another protocol is not offered",
        // which was correct while `update` refused a protocol change: offering
        // a release the installer would decline helps nobody. `update` now
        // performs the migration, so the same rule hides releases the Hive can
        // install — and it hid v0.9.0 from every Hive in the field while
        // reporting them up to date.
        //
        // The version decides now, and only the version.
        assert_eq!(payload.upgrade_for(&current).unwrap().version, "0.9.0");
        assert_eq!(payload.newest_offer().unwrap().version, "0.9.0");
    }

    /// "Replacing someone's checkout-built binary with a release would discard
    /// work whose contents nothing can enumerate." ADR 0050 point 4. It is
    /// still told the release exists.
    #[test]
    fn a_working_copy_is_told_about_a_release_and_offered_nothing() {
        let (signing_key, public) = keypair();
        let document = sign(manifest(vec![offer("0.2.0", "engine-b")]), &signing_key);

        let payload = verify_release_manifest(&document, Some(&public), 2_000).unwrap();
        let current = SwarmVersion::parse("0.1.0-dev-5394d9a6b872-20260820201150").unwrap();

        assert!(payload.upgrade_for(&current).is_none());
        assert_eq!(payload.newest_offer().unwrap().version, "0.2.0");
    }

    /// A signature covers whatever it covers. Refusing malformed offers is
    /// what keeps a signed-but-nonsensical document away from the fetcher.
    /// One way to make an offer unactionable, and what it is called.
    type BrokenOffer = (&'static str, fn(&mut ReleaseOffer));

    #[test]
    fn an_offer_the_fetcher_could_not_act_on_is_refused_before_it_reaches_one() {
        let (signing_key, public) = keypair();
        let cases: [BrokenOffer; 6] = [
            ("plain http", |offer: &mut ReleaseOffer| {
                offer.artifact_url = "http://releases.example/swarm.tar.gz".to_owned();
            }),
            ("a digest that is not one", |offer: &mut ReleaseOffer| {
                offer.artifact_sha256 = "not-a-digest".to_owned();
            }),
            ("an uppercase digest", |offer: &mut ReleaseOffer| {
                offer.artifact_sha256 = "A".repeat(64);
            }),
            ("an empty artifact", |offer: &mut ReleaseOffer| {
                offer.artifact_bytes = 0;
            }),
            ("a development version", |offer: &mut ReleaseOffer| {
                offer.version = "0.2.0-dev-5394d9a6b872-20260820201150".to_owned();
            }),
            ("notes served over http", |offer: &mut ReleaseOffer| {
                offer.notes_url = Some("http://notes.example".to_owned());
            }),
        ];

        for (description, break_it) in cases {
            let mut broken = offer("0.2.0", "engine-b");
            break_it(&mut broken);
            let document = sign(manifest(vec![broken]), &signing_key);
            assert_eq!(
                verify_release_manifest(&document, Some(&public), 2_000).unwrap_err(),
                ReleaseManifestError::Malformed,
                "{description} should not survive verification"
            );
        }
    }

    #[test]
    fn a_manifest_from_the_future_or_another_schema_is_refused() {
        let (signing_key, public) = keypair();
        let document = sign(manifest(vec![offer("0.2.0", "engine-b")]), &signing_key);
        assert_eq!(
            verify_release_manifest(&document, Some(&public), 1).unwrap_err(),
            ReleaseManifestError::NotYetValid
        );

        let mut future_schema = manifest(vec![offer("0.2.0", "engine-b")]);
        future_schema.schema_version = RELEASE_MANIFEST_SCHEMA_VERSION + 1;
        let document = sign(future_schema, &signing_key);
        assert_eq!(
            verify_release_manifest(&document, Some(&public), 2_000).unwrap_err(),
            ReleaseManifestError::UnsupportedSchema
        );
    }

    /// THE DEFECT THIS EXISTS FOR, at the layer where it happened.
    ///
    /// v0.9.0 declared protocol 10, every Hive in the field compiled 9, and the
    /// exact-equality filter meant a fully published and correctly signed
    /// release was visible to nobody. The operator: "I keep checking and my
    /// copy of swarm doesn't see anything newer than 0.8.17."
    ///
    /// This is asserted at DISCOVERY rather than at install, because
    /// test-field-upgrade.sh writes the release-apply request directly — it
    /// proves a Hive can install such a release and never asks whether it can
    /// find one, which is exactly how this survived a full deploy with every
    /// step green.
    #[test]
    fn a_release_that_moves_the_protocol_is_still_offered() {
        let (signing_key, public) = keypair();
        let mut newer = offer("0.2.0", "engine-b");
        newer.protocol = "10".to_owned();
        let document = sign(manifest(vec![newer]), &signing_key);

        let payload = verify_release_manifest(&document, Some(&public), 2_000).unwrap();
        let current = SwarmVersion::parse("0.1.0").unwrap();

        assert_eq!(payload.newest_offer().unwrap().version, "0.2.0");
        assert_eq!(payload.upgrade_for(&current).unwrap().version, "0.2.0");
    }

    /// And it is offered in the other direction too: a Hive that has already
    /// moved on must still see a release declaring an older protocol, or every
    /// Hive that takes one becomes invisible to the next.
    #[test]
    fn an_offer_declaring_an_older_protocol_is_still_seen() {
        let (signing_key, public) = keypair();
        let mut older_protocol = offer("0.3.0", "engine-c");
        older_protocol.protocol = "1".to_owned();
        let document = sign(manifest(vec![older_protocol]), &signing_key);

        let payload = verify_release_manifest(&document, Some(&public), 2_000).unwrap();
        assert_eq!(payload.newest_offer().unwrap().version, "0.3.0");
    }

    #[test]
    fn an_empty_manifest_offers_nothing_without_being_an_error() {
        let (signing_key, public) = keypair();
        let document = sign(manifest(Vec::new()), &signing_key);

        let payload = verify_release_manifest(&document, Some(&public), 2_000).unwrap();
        let current = SwarmVersion::parse("0.1.0").unwrap();

        assert!(payload.upgrade_for(&current).is_none());
        assert!(payload.newest_offer().is_none());
    }
}
