use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use echelon_core::{auth::verify_id_token, unix_time};
use jsonwebtoken::{Algorithm, EncodingKey, Header, jwk::JwkSet};
use rand::rngs::OsRng;
use rsa::{RsaPrivateKey, RsaPublicKey, pkcs8::EncodePrivateKey, traits::PublicKeyParts};
use serde_json::{Value, json};
use std::sync::LazyLock;

static KEYS: LazyLock<(EncodingKey, JwkSet)> = LazyLock::new(|| {
    let private = RsaPrivateKey::new(&mut OsRng, 2048).unwrap();
    let public = RsaPublicKey::from(&private);
    let pem = private.to_pkcs8_pem(Default::default()).unwrap();
    let signing = EncodingKey::from_rsa_pem(pem.as_bytes()).unwrap();
    let jwks = serde_json::from_value(json!({"keys":[{
        "kty":"RSA","kid":"synthetic-key","alg":"RS256","use":"sig",
        "n":URL_SAFE_NO_PAD.encode(public.n().to_bytes_be()),
        "e":URL_SAFE_NO_PAD.encode(public.e().to_bytes_be())
    }]}))
    .unwrap();
    (signing, jwks)
});

fn claims() -> Value {
    json!({"iss":"https://auth.openai.com","sub":"synthetic-user",
        "aud":"synthetic-client","exp":unix_time()+300,"nonce":"synthetic-nonce"})
}
fn signed(value: &Value) -> String {
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some("synthetic-key".into());
    jsonwebtoken::encode(&header, value, &KEYS.0).unwrap()
}

#[test]
fn signature_audience_issuer_nonce_expiry_and_account_are_required() {
    let value = claims();
    let token = signed(&value);
    assert!(
        verify_id_token(
            &KEYS.1,
            &token,
            "synthetic-client",
            Some("synthetic-nonce"),
            Some("synthetic-user")
        )
        .is_ok()
    );
    assert!(
        verify_id_token(
            &KEYS.1,
            &token,
            "other-client",
            Some("synthetic-nonce"),
            None
        )
        .is_err()
    );
    assert!(
        verify_id_token(
            &KEYS.1,
            &token,
            "synthetic-client",
            Some("other-nonce"),
            None
        )
        .is_err()
    );
    assert!(
        verify_id_token(
            &KEYS.1,
            &token,
            "synthetic-client",
            Some("synthetic-nonce"),
            Some("other-user")
        )
        .is_err()
    );
    let mut wrong = value.clone();
    wrong["iss"] = json!("https://example.invalid");
    assert!(verify_id_token(&KEYS.1, &signed(&wrong), "synthetic-client", None, None).is_err());
    wrong = value.clone();
    wrong["exp"] = json!(unix_time() - 1000);
    assert!(verify_id_token(&KEYS.1, &signed(&wrong), "synthetic-client", None, None).is_err());
    wrong = value.clone();
    wrong.as_object_mut().unwrap().remove("nonce");
    assert!(
        verify_id_token(
            &KEYS.1,
            &signed(&wrong),
            "synthetic-client",
            Some("synthetic-nonce"),
            None
        )
        .is_err()
    );
    let other = RsaPrivateKey::new(&mut OsRng, 2048)
        .unwrap()
        .to_pkcs8_pem(Default::default())
        .unwrap();
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some("synthetic-key".into());
    let forged = jsonwebtoken::encode(
        &header,
        &value,
        &EncodingKey::from_rsa_pem(other.as_bytes()).unwrap(),
    )
    .unwrap();
    assert!(verify_id_token(&KEYS.1, &forged, "synthetic-client", None, None).is_err());
}
