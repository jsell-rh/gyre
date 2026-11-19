//! Vault backend tests against a local HTTP server speaking the Vault KV v2
//! + JWT-auth API subset (the AC's sanctioned stand-in for a real Vault dev
//! server).

use super::*;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// In-memory Vault state: path → payload (`data` object).
type VaultStore = Arc<Mutex<HashMap<String, serde_json::Value>>>;

/// What the stub server observed (tokens per path, JWT logins).
#[derive(Default, Clone)]
struct Observed {
    /// (path, token) for every data read.
    reads: Arc<Mutex<Vec<(String, String)>>>,
    /// JWTs presented at the JWT login endpoint.
    jwt_logins: Arc<Mutex<Vec<String>>>,
}

/// Spawn the stub Vault server; returns (base_url, store, observed).
async fn spawn_vault() -> (String, VaultStore, Observed) {
    use axum::routing::post;

    let store: VaultStore = Arc::new(Mutex::new(HashMap::new()));
    let observed = Observed::default();

    let reads = Arc::clone(&observed.reads);
    let jwt_logins = Arc::clone(&observed.jwt_logins);
    let store_reads = Arc::clone(&store);
    let store_writes = Arc::clone(&store);
    let store_list = Arc::clone(&store);
    let store_delete = Arc::clone(&store);

    let app = axum::Router::new()
        .route(
            "/v1/auth/approle/login",
            post(|| async {
                axum::Json(serde_json::json!({
                    "auth": { "client_token": "test-server-token" }
                }))
            }),
        )
        .route(
            "/v1/auth/jwt/login",
            post(move |body: String| {
                let jwt_logins = Arc::clone(&jwt_logins);
                async move {
                    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
                    let jwt = v["jwt"].as_str().unwrap_or_default().to_string();
                    jwt_logins.lock().await.push(jwt);
                    axum::Json(serde_json::json!({
                        "auth": { "client_token": "test-agent-token" }
                    }))
                }
            }),
        )
        // KV v2 data read/write. The token is validated: the server token
        // and agent-scoped tokens are accepted, anything else is 403 (the
        // fail-closed probe relies on the error surfacing).
        .route(
            "/v1/secret/data/*path",
            axum::routing::get(
                move |axum::extract::Path(path): axum::extract::Path<String>,
                      headers: axum::http::HeaderMap| {
                    let reads = Arc::clone(&reads);
                    let store = Arc::clone(&store_reads);
                    async move {
                        let token = headers
                            .get("X-Vault-Token")
                            .and_then(|t| t.to_str().ok())
                            .unwrap_or_default()
                            .to_string();
                        if token != "test-server-token"
                            && token != "test-agent-token"
                            && token != "static-token"
                        {
                            return (
                                axum::http::StatusCode::FORBIDDEN,
                                axum::Json(serde_json::json!({"errors": ["permission denied"]})),
                            );
                        }
                        reads
                            .lock()
                            .await
                            .push((format!("secret/data/{path}"), token));
                        let store = store.lock().await;
                        match store.get(&format!("secret/data/{path}")) {
                            Some(data) => (
                                axum::http::StatusCode::OK,
                                axum::Json(serde_json::json!({ "data": { "data": data } })),
                            ),
                            None => (
                                axum::http::StatusCode::NOT_FOUND,
                                axum::Json(serde_json::json!({})),
                            ),
                        }
                    }
                },
            )
            .post(
                move |axum::extract::Path(path): axum::extract::Path<String>,
                      headers: axum::http::HeaderMap,
                      body: String| {
                    let store = Arc::clone(&store_writes);
                    async move {
                        let token = headers
                            .get("X-Vault-Token")
                            .and_then(|t| t.to_str().ok())
                            .unwrap_or_default()
                            .to_string();
                        if token != "test-server-token" && token != "static-token" {
                            return (
                                axum::http::StatusCode::FORBIDDEN,
                                axum::Json(serde_json::json!({"errors": ["permission denied"]})),
                            );
                        }
                        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
                        let data = v["data"].clone();
                        store
                            .lock()
                            .await
                            .insert(format!("secret/data/{path}"), data);
                        (
                            axum::http::StatusCode::OK,
                            axum::Json(serde_json::json!({})),
                        )
                    }
                },
            ),
        )
        // KV v2 metadata LIST + DELETE.
        .route(
            "/v1/secret/metadata/*path",
            axum::routing::get(
                move |axum::extract::Path(path): axum::extract::Path<String>,
                      headers: axum::http::HeaderMap| {
                    let store = Arc::clone(&store_list);
                    async move {
                        let token = headers
                            .get("X-Vault-Token")
                            .and_then(|t| t.to_str().ok())
                            .unwrap_or_default()
                            .to_string();
                        if token != "test-server-token"
                            && token != "test-agent-token"
                            && token != "static-token"
                        {
                            return (
                                axum::http::StatusCode::FORBIDDEN,
                                axum::Json(serde_json::json!({"errors": ["permission denied"]})),
                            );
                        }
                        // Return the immediate children (one path component).
                        let prefix = format!("secret/data/{path}/");
                        let store = store.lock().await;
                        let mut keys: Vec<String> = store
                            .keys()
                            .filter_map(|k| k.strip_prefix(&prefix))
                            .map(|rest| rest.split('/').next().unwrap_or(rest).to_string())
                            .collect();
                        keys.sort();
                        keys.dedup();
                        (
                            axum::http::StatusCode::OK,
                            axum::Json(serde_json::json!({ "data": { "keys": keys } })),
                        )
                    }
                },
            )
            .delete(
                move |axum::extract::Path(path): axum::extract::Path<String>,
                      headers: axum::http::HeaderMap| {
                    let store = Arc::clone(&store_delete);
                    async move {
                        let token = headers
                            .get("X-Vault-Token")
                            .and_then(|t| t.to_str().ok())
                            .unwrap_or_default()
                            .to_string();
                        if token != "test-server-token" && token != "static-token" {
                            return (
                                axum::http::StatusCode::FORBIDDEN,
                                axum::Json(serde_json::json!({"errors": ["permission denied"]})),
                            );
                        }
                        let prefix = format!("secret/data/{path}");
                        let mut store = store.lock().await;
                        store.retain(|k, _| !k.starts_with(&prefix));
                        (
                            axum::http::StatusCode::OK,
                            axum::Json(serde_json::json!({})),
                        )
                    }
                },
            ),
        )
        .fallback(|| async { (axum::http::StatusCode::NOT_FOUND, "no such route") });

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://127.0.0.1:{port}"), store, observed)
}

fn config(addr: &str) -> VaultConfig {
    VaultConfig {
        addr: addr.to_string(),
        token: Some("static-token".to_string()),
        role_id: None,
        secret_id: None,
        kv_mount: "secret".to_string(),
        jwt_mount: "jwt".to_string(),
        jwt_role: "gyre-agent".to_string(),
        namespace: None,
    }
}

fn sample_secret(name: &str, scope: SecretScope, scope_id: &str, tenant: &str) -> Secret {
    Secret {
        id: Id::new(format!("id-{name}-{scope_id}")),
        name: name.to_string(),
        scope,
        scope_id: scope_id.to_string(),
        secret_type: gyre_common::SecretType::Static,
        created_by: "user:admin".to_string(),
        created_at: 1_700_000_000,
        expires_at: None,
        last_rotated_at: None,
        tenant_id: tenant.to_string(),
    }
}

#[tokio::test]
async fn crud_is_served_from_vault_kv_v2() {
    let (addr, store, _observed) = spawn_vault().await;
    let repo = VaultSecretRepository::new(config(&addr)).unwrap();

    let secret = sample_secret("DATABASE_URL", SecretScope::Repo, "repo-1", "t1");
    repo.create(&secret, b"postgres://u:p@h/db").await.unwrap();

    // The value lives in the Vault store (base64 in the payload), not in
    // any local database.
    {
        let s = store.lock().await;
        let key = "secret/data/gyre-secrets/t1/repo/repo-1/DATABASE_URL".to_string();
        let payload = s.get(&key).expect("secret stored in Vault");
        assert_eq!(payload["name"], "DATABASE_URL");
        let value = B64.decode(payload["value_b64"].as_str().unwrap()).unwrap();
        assert_eq!(value, b"postgres://u:p@h/db");
        assert!(s.contains_key("secret/data/gyre-secrets/t1/_idx/id-DATABASE_URL-repo-1"));
    }

    // get_value + find_by_id.
    let value = repo
        .get_value(&secret.id, "t1")
        .await
        .unwrap()
        .expect("secret exists");
    assert_eq!(value, b"postgres://u:p@h/db");
    let found = repo.find_by_id(&secret.id, "t1").await.unwrap().unwrap();
    assert_eq!(found.name, "DATABASE_URL");
    assert_eq!(found.tenant_id, "t1");

    // list_by_scope.
    repo.create(
        &sample_secret("API_KEY", SecretScope::Repo, "repo-1", "t1"),
        b"k",
    )
    .await
    .unwrap();
    let listed = repo
        .list_by_scope(SecretScope::Repo, "repo-1", "t1")
        .await
        .unwrap();
    let names: Vec<&str> = listed.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["API_KEY", "DATABASE_URL"]);

    // rotate.
    repo.rotate(&secret.id, b"postgres://u:p@h/db2", "t1")
        .await
        .unwrap();
    let value = repo.get_value(&secret.id, "t1").await.unwrap().unwrap();
    assert_eq!(value, b"postgres://u:p@h/db2");
    let found = repo.find_by_id(&secret.id, "t1").await.unwrap().unwrap();
    assert!(found.last_rotated_at.is_some());

    // delete removes both the data and the index entry.
    repo.delete(&secret.id, "t1").await.unwrap();
    assert!(repo.get_value(&secret.id, "t1").await.unwrap().is_none());
    {
        let s = store.lock().await;
        assert!(!s.contains_key("secret/data/gyre-secrets/t1/repo/repo-1/DATABASE_URL"));
        assert!(!s.contains_key("secret/data/gyre-secrets/t1/_idx/id-DATABASE_URL-repo-1"));
    }
}

#[tokio::test]
async fn create_rejects_duplicate_id_and_duplicate_scope_name() {
    let (addr, _store, _observed) = spawn_vault().await;
    let repo = VaultSecretRepository::new(config(&addr)).unwrap();

    let a = sample_secret("SHARED", SecretScope::Repo, "repo-1", "t1");
    repo.create(&a, b"v").await.unwrap();

    // Same id, different name → rejected (port contract).
    let mut same_id = sample_secret("OTHER", SecretScope::Repo, "repo-1", "t1");
    same_id.id = a.id.clone();
    let err = repo.create(&same_id, b"v").await.unwrap_err();
    assert!(err.to_string().contains("already exists"));

    // Same (scope, scope_id, name), different id → rejected.
    let same_name = sample_secret("SHARED", SecretScope::Repo, "repo-1", "t1");
    let err = repo.create(&same_name, b"v").await.unwrap_err();
    assert!(err.to_string().contains("already exists"));
}

#[tokio::test]
async fn binary_values_roundtrip_without_lossy_conversion() {
    let (addr, _store, _observed) = spawn_vault().await;
    let repo = VaultSecretRepository::new(config(&addr)).unwrap();
    let secret = sample_secret("BIN", SecretScope::Workspace, "ws-1", "t1");
    // Invalid UTF-8 bytes: \xFF\xFE and a NUL — a lossy conversion would
    // mangle them into U+FFFD.
    let value = vec![0xFF, 0xFE, 0x00, 0x80, 0x7F];
    repo.create(&secret, &value).await.unwrap();
    let roundtripped = repo.get_value(&secret.id, "t1").await.unwrap().unwrap();
    assert_eq!(roundtripped, value);
}

#[tokio::test]
async fn agent_resolution_authenticates_with_agent_jwt() {
    let (addr, _store, observed) = spawn_vault().await;
    let repo = VaultSecretRepository::new(config(&addr)).unwrap();

    // Tenant + repo scoped secrets.
    repo.create(
        &sample_secret("TENANT_KEY", SecretScope::Tenant, "t1", "t1"),
        b"tv",
    )
    .await
    .unwrap();
    repo.create(
        &sample_secret("REPO_KEY", SecretScope::Repo, "repo-1", "t1"),
        b"rv",
    )
    .await
    .unwrap();

    let resolved = repo
        .resolve_for_agent_as("t1", "ws-1", "repo-1", None, "agent-jwt-abc")
        .await
        .unwrap();
    let map: HashMap<String, Vec<u8>> = resolved.into_iter().collect();
    assert_eq!(map.get("TENANT_KEY").unwrap(), b"tv");
    assert_eq!(map.get("REPO_KEY").unwrap(), b"rv");

    // The agent's JWT was presented to the JWT auth backend.
    let logins = observed.jwt_logins.lock().await;
    assert_eq!(logins.as_slice(), ["agent-jwt-abc"]);

    // The data reads for the agent resolution used the agent-scoped token
    // (returned by the JWT login), never the server's static token. Only
    // reads AFTER the JWT login belong to the agent resolution (the
    // create() existence checks above legitimately used the server
    // token).
    let reads = observed.reads.lock().await;
    let first_agent_read = reads
        .iter()
        .position(|(_, t)| t == "test-agent-token")
        .expect("agent resolution must read with the JWT-derived token");
    let agent_reads = &reads[first_agent_read..];
    assert!(
        agent_reads
            .iter()
            .any(|(p, t)| p.ends_with("TENANT_KEY") && t == "test-agent-token"),
        "agent resolution must read with the JWT-derived token, saw {agent_reads:?}"
    );
    assert!(
        !agent_reads.iter().any(|(_, t)| t == "static-token"),
        "the server token must not be used for agent-scoped reads"
    );
}

#[tokio::test]
async fn unreachable_vault_fails_closed() {
    // Reserve a port and drop the listener so nothing is listening.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let addr = format!("http://127.0.0.1:{port}");

    let repo = VaultSecretRepository::new(config(&addr)).unwrap();
    let secret = sample_secret("X", SecretScope::Repo, "repo-1", "t1");

    // Every operation errors — there is no database fallback.
    let err = repo.create(&secret, b"v").await.unwrap_err();
    assert!(err.to_string().contains("Vault"));
    let err = repo.get_value(&secret.id, "t1").await.unwrap_err();
    assert!(err.to_string().contains("Vault") || err.to_string().contains("connection"));
    let err = repo
        .resolve_for_agent("t1", "ws-1", "repo-1", None)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("Vault") || err.to_string().contains("connection"));
    let err = repo
        .resolve_for_agent_as("t1", "ws-1", "repo-1", None, "jwt")
        .await
        .unwrap_err();
    assert!(err.to_string().contains("Vault") || err.to_string().contains("connection"));
}

#[tokio::test]
async fn resolution_cascade_nearest_scope_wins_and_expiry_excluded() {
    let (addr, _store, _observed) = spawn_vault().await;
    let repo = VaultSecretRepository::new(config(&addr)).unwrap();

    repo.create(
        &sample_secret("SHARED", SecretScope::Tenant, "t1", "t1"),
        b"tenant-value",
    )
    .await
    .unwrap();
    repo.create(
        &sample_secret("SHARED", SecretScope::Repo, "repo-1", "t1"),
        b"repo-value",
    )
    .await
    .unwrap();
    let mut expired = sample_secret("OLD", SecretScope::Repo, "repo-1", "t1");
    expired.expires_at = Some(1_000_000_000);
    repo.create(&expired, b"stale").await.unwrap();

    let resolved = repo
        .resolve_for_agent("t1", "ws-1", "repo-1", None)
        .await
        .unwrap();
    let map: HashMap<String, Vec<u8>> = resolved.into_iter().collect();
    assert_eq!(map.get("SHARED").unwrap(), b"repo-value");
    assert!(!map.contains_key("OLD"), "expired secret must not resolve");
}

#[test]
fn config_from_env_requires_auth_material() {
    // No GYRE_VAULT_ADDR → fails closed at construction.
    std::env::remove_var("GYRE_VAULT_ADDR");
    std::env::remove_var("GYRE_VAULT_TOKEN");
    std::env::remove_var("GYRE_VAULT_ROLE_ID");
    std::env::remove_var("GYRE_VAULT_SECRET_ID");
    assert!(VaultConfig::from_env().is_err());
}
