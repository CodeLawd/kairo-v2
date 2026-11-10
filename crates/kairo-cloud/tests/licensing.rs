use std::{path::PathBuf, process::Command};

use kairo_cloud::licensing::valid_license_lease;
use kairo_domain::cloud::{LicenseSnapshot, LicensedFeature, SignedLicenseLease};

#[test]
#[ignore = "requires Node and the Electron checkout's installed server and tsx dependencies"]
fn verifies_a_lease_from_the_unchanged_nest_service() {
    let checkout = std::env::var_os("KAIRO_ELECTRON_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../kairo-presenter")
        })
        .canonicalize()
        .unwrap();
    let output = Command::new("node")
        .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/server-license.cjs"))
        .arg(checkout)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let snapshot: LicenseSnapshot = serde_json::from_slice(&output.stdout).unwrap();
    let mut lease = snapshot.lease.unwrap();
    let trusted = lease.public_key.clone();
    let now = lease.claims.issued_at;
    let check = |lease: &SignedLicenseLease, key: &str, org: &str, device: &str, feature, now| {
        valid_license_lease(Some(lease), Some(key), org, device, feature, now)
    };
    assert!(check(
        &lease,
        &trusted,
        "0123456789abcdef01234567",
        "fixture-booth",
        Some(LicensedFeature::Scripture),
        now
    ));
    assert!(!check(
        &lease,
        &trusted,
        "other",
        "fixture-booth",
        None,
        now
    ));
    assert!(!check(
        &lease,
        &trusted,
        "0123456789abcdef01234567",
        "wrong-device",
        None,
        now
    ));
    assert!(!check(
        &lease,
        &trusted,
        "0123456789abcdef01234567",
        "fixture-booth",
        Some(LicensedFeature::Transcription),
        now
    ));
    assert!(!check(
        &lease,
        &trusted,
        "0123456789abcdef01234567",
        "fixture-booth",
        None,
        lease.claims.expires_at
    ));
    lease.claims.refresh_after += 1.0;
    assert!(!check(
        &lease,
        &trusted,
        "0123456789abcdef01234567",
        "fixture-booth",
        None,
        now
    ));
}

#[test]
fn verifier_rejects_bad_key_and_enforces_grace_and_clock_limits() {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use kairo_domain::{cloud::LicenseLeaseClaims, json::to_js_string};
    use ring::signature::{Ed25519KeyPair, KeyPair as _};

    let now = 1_700_000_000.0;
    let key = Ed25519KeyPair::from_seed_unchecked(&[37; 32]).unwrap();
    let mut der = vec![0x30, 0x2a, 0x30, 5, 6, 3, 0x2b, 0x65, 0x70, 3, 0x21, 0];
    der.extend(key.public_key().as_ref());
    let pem = format!(
        "-----BEGIN PUBLIC KEY-----\n{}\n-----END PUBLIC KEY-----\n",
        STANDARD.encode(der)
    );
    let sign = |claims: LicenseLeaseClaims| {
        let signature = key.sign(to_js_string(&claims).unwrap().as_bytes());
        SignedLicenseLease {
            claims,
            raw_claims: None,
            signature: STANDARD.encode(signature.as_ref()),
            public_key: pem.clone(),
        }
    };
    let claims = LicenseLeaseClaims {
        org_id: "church".into(),
        device_id: "booth".into(),
        features: vec![LicensedFeature::Scripture],
        issued_at: now,
        refresh_after: now + 86_400.0,
        expires_at: now + 14.0 * 86_400.0,
        license_expires_at: None,
    };
    let signed = sign(claims.clone());
    let check = |lease: &SignedLicenseLease| {
        valid_license_lease(Some(lease), Some(&pem), "church", "booth", None, now)
    };
    assert!(check(&signed));
    assert!(!valid_license_lease(
        Some(&signed),
        Some("bad key"),
        "church",
        "booth",
        None,
        now
    ));
    assert!(!valid_license_lease(
        Some(&signed),
        None,
        "church",
        "booth",
        None,
        now
    ));
    assert!(!check(&sign(LicenseLeaseClaims {
        expires_at: claims.expires_at + 1.0,
        ..claims.clone()
    })));
    assert!(!check(&sign(LicenseLeaseClaims {
        license_expires_at: Some(now + 1.0),
        ..claims.clone()
    })));
    assert!(!check(&sign(LicenseLeaseClaims {
        issued_at: now + 61.0,
        ..claims
    })));
}

/// A key pair and its SPKI PEM, and a lease signed over `claims` exactly as given.
struct Signer {
    key: ring::signature::Ed25519KeyPair,
    pem: String,
}

impl Signer {
    fn new() -> Self {
        use base64::{Engine as _, engine::general_purpose::STANDARD};
        use ring::signature::{Ed25519KeyPair, KeyPair as _};
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new()).unwrap();
        let key = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();
        let mut der = vec![0x30, 0x2a, 0x30, 5, 6, 3, 0x2b, 0x65, 0x70, 3, 0x21, 0];
        der.extend(key.public_key().as_ref());
        let pem = format!(
            "-----BEGIN PUBLIC KEY-----\n{}\n-----END PUBLIC KEY-----\n",
            STANDARD.encode(der)
        );
        Self { key, pem }
    }

    /// The lease JSON a server signing `claims` with `JSON.stringify` sends.
    fn lease_json(&self, claims: &str) -> String {
        use base64::{Engine as _, engine::general_purpose::STANDARD};
        let signature = STANDARD.encode(self.key.sign(claims.as_bytes()).as_ref());
        format!(
            r#"{{"claims":{claims},"signature":{},"publicKey":{}}}"#,
            serde_json::to_string(&signature).unwrap(),
            serde_json::to_string(&self.pem).unwrap()
        )
    }

    fn valid(&self, lease: &SignedLicenseLease, feature: Option<LicensedFeature>) -> bool {
        valid_license_lease(
            Some(lease),
            Some(&self.pem),
            "church",
            "booth",
            feature,
            NOW,
        )
    }
}

const NOW: f64 = 1_700_000_000.0;

fn claims_text(extra: &str, features: &str) -> String {
    format!(
        r#"{{"orgId":"church",{extra}"deviceId":"booth","features":{features},"issuedAt":1700000000,"refreshAfter":1700086400,"expiresAt":1701209600,"licenseExpiresAt":null}}"#
    )
}

#[test]
fn a_claim_field_this_build_does_not_know_still_verifies() {
    let signer = Signer::new();
    let json = signer.lease_json(&claims_text(r#""plan":"pro","#, r#"["scripture"]"#));
    let lease: SignedLicenseLease = serde_json::from_str(&json).unwrap();
    assert!(signer.valid(&lease, Some(LicensedFeature::Scripture)));
}

#[test]
fn a_feature_this_build_does_not_know_is_ignored() {
    let signer = Signer::new();
    let json = signer.lease_json(&claims_text("", r#"["scripture","lyrics"]"#));
    let lease: SignedLicenseLease = serde_json::from_str(&json).unwrap();
    assert_eq!(lease.claims.features, [LicensedFeature::Scripture]);
    assert!(signer.valid(&lease, Some(LicensedFeature::Scripture)));
    assert!(!signer.valid(&lease, Some(LicensedFeature::Transcription)));
}

#[test]
fn tampered_claims_do_not_verify() {
    let signer = Signer::new();
    let json = signer.lease_json(&claims_text("", r#"["scripture"]"#));
    let forged = json.replace(r#""deviceId":"booth""#, r#""deviceId":"other""#);
    let lease: SignedLicenseLease = serde_json::from_str(&forged).unwrap();
    assert!(!valid_license_lease(
        Some(&lease),
        Some(&signer.pem),
        "church",
        "other",
        None,
        NOW
    ));
    // Editing the typed view after receipt does not verify either.
    let mut lease: SignedLicenseLease = serde_json::from_str(&json).unwrap();
    lease.claims.features.push(LicensedFeature::Transcription);
    assert!(!signer.valid(&lease, None));
}

#[test]
fn a_saved_lease_round_trips_and_still_verifies() {
    let signer = Signer::new();
    let claims = claims_text(r#""plan":"pro","#, r#"["scripture","lyrics"]"#);
    let json = signer.lease_json(&claims);
    let lease: SignedLicenseLease = serde_json::from_str(&json).unwrap();
    let saved = serde_json::to_string(&lease).unwrap();
    assert_eq!(saved, json, "the saved lease is byte-identical");
    let restored: SignedLicenseLease = serde_json::from_str(&saved).unwrap();
    assert!(signer.valid(&restored, Some(LicensedFeature::Scripture)));
    // Whitespace the server did not sign over is not part of the payload.
    let spaced = json.replace(&claims, &claims.replace(',', ", "));
    let lease: SignedLicenseLease = serde_json::from_str(&spaced).unwrap();
    assert!(signer.valid(&lease, None));
}

#[tokio::test]
#[ignore = "requires a disposable local Kairo dev server with mail logging and MongoDB"]
async fn native_activation_verifies_live_server_signature() {
    use kairo_cloud::{CloudConfig, CloudSession, licensing::LicensingService};
    use kairo_core::Publisher;
    use kairo_domain::cloud::SignInInput;
    use kairo_net::{CloudApiClient, Method, RequestOptions};
    use kairo_store::secrets::{KeyProtector, SecretError, SecureStore};
    use serde_json::{Value, json};
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
        time::Duration,
    };

    #[derive(Default)]
    struct TestKeyring(Mutex<HashMap<String, [u8; 32]>>);
    impl KeyProtector for TestKeyring {
        fn load(&self, slot: &str) -> Result<Option<[u8; 32]>, SecretError> {
            Ok(self.0.lock().unwrap().get(slot).copied())
        }
        fn save(&self, slot: &str, key: &[u8; 32]) -> Result<(), SecretError> {
            self.0.lock().unwrap().insert(slot.to_owned(), *key);
            Ok(())
        }
    }

    let base =
        std::env::var("KAIRO_TEST_LICENCE_URL").expect("set the disposable loopback server URL");
    assert!(matches!(
        url::Url::parse(&base).unwrap().host_str(),
        Some("127.0.0.1" | "[::1]")
    ));
    let log =
        std::env::var_os("KAIRO_TEST_LICENCE_LOG").expect("set the disposable server log path");
    let offset = std::fs::metadata(&log).unwrap().len() as usize;
    let email = format!("license-fixture-{}@example.invalid", uuid::Uuid::new_v4());
    let password = "Synthetic-fixture-password-42!";
    let owner = CloudApiClient::new(&base, None).unwrap();
    let anonymous = RequestOptions {
        authenticated: false,
        allow_refresh: false,
        timeout: None,
    };
    let _: Value = owner
        .request(
            Method::POST,
            "/v1/auth/signup",
            Some(json!({
                "email": email, "password": password, "name": "Fixture",
                "orgName": "Fixture Church", "deviceId": "fixture-owner",
            })),
            anonymous,
        )
        .await
        .unwrap();

    let token = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let bytes = std::fs::read(&log).unwrap();
            if let Some((_, tail)) =
                String::from_utf8_lossy(&bytes[offset..]).split_once("/verify-email?token=")
            {
                let token = tail
                    .chars()
                    .take_while(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
                    .collect::<String>();
                if !token.is_empty() {
                    break token;
                }
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("verification token from the synthetic development mail");
    let _: Value = owner
        .request(
            Method::POST,
            "/v1/auth/verify-email",
            Some(json!({"token": token})),
            anonymous,
        )
        .await
        .unwrap();
    let credentials: Value = owner
        .request(
            Method::POST,
            "/v1/auth/login",
            Some(json!({
                "email": email, "password": password, "deviceId": "fixture-owner",
            })),
            anonymous,
        )
        .await
        .unwrap();
    owner.set_access_token(Some(
        credentials["accessToken"].as_str().unwrap().to_owned(),
    ));
    let activation: Value = owner
        .request(
            Method::POST,
            "/v1/licensing/key",
            None,
            RequestOptions::default(),
        )
        .await
        .unwrap();

    let profile = tempfile::tempdir().unwrap();
    let protector = Arc::new(TestKeyring::default());
    let topics = Publisher::default();
    let session = CloudSession::open(
        CloudConfig::new(&base, &base, Some(&base)).unwrap(),
        profile.path().to_owned(),
        protector.clone(),
        None,
        topics.clone(),
    )
    .await
    .unwrap();
    assert!(
        session
            .sign_in(SignInInput {
                email,
                password: password.to_owned()
            })
            .await
            .ok
    );
    let service = LicensingService::open_with_trusted_key(
        session.clone(),
        profile.path().to_owned(),
        protector.clone(),
        topics,
        None,
    )
    .await;
    let snapshot = service
        .activate(activation["key"].as_str().unwrap(), Some("Fixture booth"))
        .await
        .unwrap();
    assert!(snapshot.activated);
    assert!(service.get_status().await.activated);
    assert!(service.refresh().await.unwrap().activated);
    service
        .assert_feature(LicensedFeature::Presentation)
        .await
        .unwrap();
    let vault = SecureStore::new(profile.path(), protector.as_ref(), "Kairo");
    assert!(vault.read("license-trusted-key").is_some());
    assert!(vault.read("license-snapshot").is_some());
    session.sign_out().await;
    assert!(!service.get_status().await.activated);
    assert!(vault.read("license-snapshot").is_none());
    session.stop().await;
}
