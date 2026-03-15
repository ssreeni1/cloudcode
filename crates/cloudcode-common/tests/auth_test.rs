use cloudcode_common::auth::AuthMethod;

#[test]
fn api_key_roundtrip() {
    let auth = AuthMethod::ApiKey {
        key: "sk-ant-12345".to_string(),
    };
    let json = serde_json::to_string(&auth).unwrap();
    let deserialized: AuthMethod = serde_json::from_str(&json).unwrap();

    match deserialized {
        AuthMethod::ApiKey { key } => assert_eq!(key, "sk-ant-12345"),
        other => panic!("Expected ApiKey, got {:?}", other),
    }
}

#[test]
fn api_key_tagged_format() {
    let auth = AuthMethod::ApiKey {
        key: "test-key".to_string(),
    };
    let json = serde_json::to_string(&auth).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["method"], "api_key");
    assert_eq!(value["key"], "test-key");
}

#[test]
fn oauth_roundtrip() {
    let auth = AuthMethod::OAuth {
        token: "oauth-token-abc".to_string(),
    };
    let json = serde_json::to_string(&auth).unwrap();
    let deserialized: AuthMethod = serde_json::from_str(&json).unwrap();

    match deserialized {
        AuthMethod::OAuth { token } => assert_eq!(token, "oauth-token-abc"),
        other => panic!("Expected OAuth, got {:?}", other),
    }
}

#[test]
fn oauth_tagged_format() {
    let auth = AuthMethod::OAuth {
        token: "tok".to_string(),
    };
    let json = serde_json::to_string(&auth).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(value["method"], "oauth");
}

#[test]
fn tag_key_is_method_not_type() {
    // Ensure the discriminant key is "method", not "type"
    let auth = AuthMethod::ApiKey {
        key: "k".to_string(),
    };
    let json = serde_json::to_string(&auth).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert!(
        value.get("method").is_some(),
        "Tag key should be 'method'"
    );
    assert!(
        value.get("type").is_none(),
        "Tag key should NOT be 'type'"
    );
}

#[test]
fn deserialize_unknown_method_returns_error() {
    let json = r#"{"method":"password","password":"secret"}"#;
    let result = serde_json::from_str::<AuthMethod>(json);
    assert!(result.is_err());
}

#[test]
fn api_key_with_empty_key() {
    let auth = AuthMethod::ApiKey {
        key: String::new(),
    };
    let json = serde_json::to_string(&auth).unwrap();
    let deserialized: AuthMethod = serde_json::from_str(&json).unwrap();

    match deserialized {
        AuthMethod::ApiKey { key } => assert!(key.is_empty()),
        other => panic!("Expected ApiKey, got {:?}", other),
    }
}

#[test]
fn deserialize_from_handwritten_api_key_json() {
    let json = r#"{"method":"api_key","key":"my-key"}"#;
    let auth: AuthMethod = serde_json::from_str(json).unwrap();
    match auth {
        AuthMethod::ApiKey { key } => assert_eq!(key, "my-key"),
        other => panic!("Expected ApiKey, got {:?}", other),
    }
}

#[test]
fn deserialize_missing_method_tag_returns_error() {
    let json = r#"{"key":"my-key"}"#;
    let result = serde_json::from_str::<AuthMethod>(json);
    assert!(result.is_err());
}

#[test]
fn api_key_with_special_characters() {
    let key_val = "sk-ant-api03-special/chars+here=end";
    let auth = AuthMethod::ApiKey {
        key: key_val.to_string(),
    };
    let json = serde_json::to_string(&auth).unwrap();
    let deserialized: AuthMethod = serde_json::from_str(&json).unwrap();

    match deserialized {
        AuthMethod::ApiKey { key } => assert_eq!(key, key_val),
        other => panic!("Expected ApiKey, got {:?}", other),
    }
}
