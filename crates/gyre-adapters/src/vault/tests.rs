//! Vault backend tests against a local HTTP server speaking the Vault KV
//! v2 and JWT-auth API subset (the AC's sanctioned stand-in for a real
//! Vault dev server).

use super::*;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// In-memory Vault state: path → payload (`data` object).
type VaultStore = Arc<Mutex<HashMap<String, serde_json::Value>>>;

/// A namespaced request observed by the stub server: (operation,
/// namespace header).
type NamespaceEvent = (&'static str, Option<String>);

/// What the stub server observed (tokens per path, JWT logins,
/// namespaces, AppRole token generations, version destroys).
#[derive(Default, Clone)]
struct Observed {
    /// (path, token) for every data read.
    reads: Arc<Mutex<Vec<(String, String)>>>,
    /// JWTs presented at the JWT login endpoint.
    jwt_logins: Arc<Mutex<Vec<String>>>,
    /// (op, namespace header) for every namespaced request (data reads,
    /// index reads, lists, deletes, writes, destroys).
    namespaces: Arc<Mutex<Vec<NamespaceEvent>>>,
    /// (path, versions) for every KV v2 destroy request (review
    /// task-273: rotate/replace must destroy the superseded versions).
    destroys: Arc<Mutex<Vec<(String, Vec<String>)>>>,
    /// KV v2 version model (stub server state, not just observation): a
    /// per-path write counter, incremented on every data POST and served
    /// as `/data/metadata/version` on reads — real Vault's versioned
    /// append semantics.
    data_versions: Arc<Mutex<HashMap<String, u64>>>,
    /// AppRole tokens "approle-token-N" with N <= this value are rejected
    /// (403) by the data/metadata handlers — the test flips this to
    /// simulate a cached server token whose TTL expired.
    reject_approle_upto: Arc<std::sync::atomic::AtomicUsize>,
    /// Path prefixes the AGENT token ("test-agent-token") is denied (403)
    /// on — simulates Vault policy scoping: the agent's role may read only
    /// part of the cascade subtree (review task-273: policy denial must
    /// scope the resolution, not abort it).
    agent_denied_prefixes: Arc<Mutex<Vec<String>>>,
}

/// Spawn the stub Vault server; returns (base_url, store, observed).
async fn spawn_vault() -> (String, VaultStore, Observed) {
    use axum::routing::post;

    let store: VaultStore = Arc::new(Mutex::new(HashMap::new()));
    let observed = Observed::default();

    let reads = Arc::clone(&observed.reads);
    let jwt_logins = Arc::clone(&observed.jwt_logins);
    let namespaces = Arc::clone(&observed.namespaces);
    let destroys = Arc::clone(&observed.destroys);
    let data_versions = Arc::clone(&observed.data_versions);
    let data_versions_read = Arc::clone(&data_versions);
    let data_versions_write = Arc::clone(&data_versions);
    let reject_approle_upto = Arc::clone(&observed.reject_approle_upto);
    let agent_denied_reads = Arc::clone(&observed.agent_denied_prefixes);
    let agent_denied_lists = Arc::clone(&observed.agent_denied_prefixes);
    let store_reads = Arc::clone(&store);
    let store_writes = Arc::clone(&store);
    let store_list = Arc::clone(&store);
    let store_delete = Arc::clone(&store);
    // Per-handler clones: each `move` route closure needs its own Arc.
    let ns_data_read = Arc::clone(&namespaces);
    let ns_delete = Arc::clone(&namespaces);
    let ns_destroy = Arc::clone(&namespaces);
    let ns_approle_login = Arc::clone(&namespaces);
    let ns_jwt_login = Arc::clone(&namespaces);
    let reject_reads = Arc::clone(&reject_approle_upto);
    let reject_writes = Arc::clone(&reject_approle_upto);
    let reject_list = Arc::clone(&reject_approle_upto);
    let reject_delete = Arc::clone(&reject_approle_upto);
    let reject_destroy = Arc::clone(&reject_approle_upto);
    let approle_generation = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let approle_gen_login = Arc::clone(&approle_generation);

    // Shared token/namespace acceptance logic: the static server token,
    // agent-scoped tokens, and AppRole-issued tokens newer than
    // `reject_approle_upto` are accepted; anything else is 403 (the
    // fail-closed probe relies on the error surfacing).
    fn token_accepted(token: &str, reject_upto: &std::sync::atomic::AtomicUsize) -> bool {
        if token == "test-agent-token" || token == "static-token" {
            return true;
        }
        if let Some(n) = token.strip_prefix("approle-token-") {
            return n
                .parse::<usize>()
                .map(|n| n > reject_upto.load(std::sync::atomic::Ordering::SeqCst))
                .unwrap_or(false);
        }
        false
    }

    let app = axum::Router::new()
        .route(
            "/v1/auth/approle/login",
            post(move |headers: axum::http::HeaderMap| {
                let generation = approle_gen_login.clone();
                let namespaces = Arc::clone(&ns_approle_login);
                async move {
                    namespaces.lock().await.push((
                        "approle_login",
                        headers
                            .get("X-Vault-Namespace")
                            .and_then(|n| n.to_str().ok())
                            .map(str::to_string),
                    ));
                    let n = generation.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    axum::Json(serde_json::json!({
                        "auth": { "client_token": format!("approle-token-{n}") }
                    }))
                }
            }),
        )
        .route(
            "/v1/auth/jwt/login",
            post(move |headers: axum::http::HeaderMap, body: String| {
                let jwt_logins = Arc::clone(&jwt_logins);
                let namespaces = Arc::clone(&ns_jwt_login);
                async move {
                    namespaces.lock().await.push((
                        "jwt_login",
                        headers
                            .get("X-Vault-Namespace")
                            .and_then(|n| n.to_str().ok())
                            .map(str::to_string),
                    ));
                    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
                    let jwt = v["jwt"].as_str().unwrap_or_default().to_string();
                    jwt_logins.lock().await.push(jwt);
                    axum::Json(serde_json::json!({
                        "auth": { "client_token": "test-agent-token" }
                    }))
                }
            }),
        )
        // KV v2 data read/write.
        .route(
            "/v1/secret/data/*path",
            axum::routing::get(
                move |axum::extract::Path(path): axum::extract::Path<String>,
                      headers: axum::http::HeaderMap| {
                    let reads = Arc::clone(&reads);
                    let namespaces = ns_data_read;
                    let reject_upto = reject_reads;
                    let store = Arc::clone(&store_reads);
                    async move {
                        let token = headers
                            .get("X-Vault-Token")
                            .and_then(|t| t.to_str().ok())
                            .unwrap_or_default()
                            .to_string();
                        if !token_accepted(&token, &reject_upto) {
                            return (
                                axum::http::StatusCode::FORBIDDEN,
                                axum::Json(serde_json::json!({"errors": ["permission denied"]})),
                            );
                        }
                        // Agent-token policy scoping: a denied prefix is a
                        // real Vault policy denial (403), not a token
                        // failure.
                        if token == "test-agent-token"
                            && agent_denied_reads
                                .lock()
                                .await
                                .iter()
                                .any(|p| path.starts_with(p.as_str()))
                        {
                            return (
                                axum::http::StatusCode::FORBIDDEN,
                                axum::Json(serde_json::json!({"errors": ["permission denied"]})),
                            );
                        }
                        namespaces.lock().await.push((
                            "data_read",
                            headers
                                .get("X-Vault-Namespace")
                                .and_then(|n| n.to_str().ok())
                                .map(str::to_string),
                        ));
                        reads
                            .lock()
                            .await
                            .push((format!("secret/data/{path}"), token));
                        let store = store.lock().await;
                        let version = *data_versions_read
                            .lock()
                            .await
                            .get(&format!("secret/data/{path}"))
                            .unwrap_or(&0);
                        match store.get(&format!("secret/data/{path}")) {
                            Some(data) => (
                                axum::http::StatusCode::OK,
                                axum::Json(serde_json::json!({
                                    "data": { "data": data, "metadata": { "version": version } }
                                })),
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
                    let reject_upto = reject_writes;
                    async move {
                        let token = headers
                            .get("X-Vault-Token")
                            .and_then(|t| t.to_str().ok())
                            .unwrap_or_default()
                            .to_string();
                        if !token_accepted(&token, &reject_upto) {
                            return (
                                axum::http::StatusCode::FORBIDDEN,
                                axum::Json(serde_json::json!({"errors": ["permission denied"]})),
                            );
                        }
                        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
                        let data = v["data"].clone();
                        let key = format!("secret/data/{path}");
                        let mut versions = data_versions_write.lock().await;
                        let version = versions.entry(key.clone()).or_insert(0);
                        *version += 1;
                        store.lock().await.insert(key, data);
                        (
                            axum::http::StatusCode::OK,
                            axum::Json(serde_json::json!({})),
                        )
                    }
                },
            ),
        )
        // KV v2 version destroy (review task-273): rotate/replace must
        // destroy the superseded versions — the old value must not stay
        // readable at ?version=N.
        .route(
            "/v1/secret/destroy/*path",
            post(
                move |axum::extract::Path(path): axum::extract::Path<String>,
                      headers: axum::http::HeaderMap,
                      body: String| {
                    let destroys = Arc::clone(&destroys);
                    let namespaces = ns_destroy;
                    let reject_upto = reject_destroy;
                    async move {
                        let token = headers
                            .get("X-Vault-Token")
                            .and_then(|t| t.to_str().ok())
                            .unwrap_or_default()
                            .to_string();
                        if !token_accepted(&token, &reject_upto) {
                            return (
                                axum::http::StatusCode::FORBIDDEN,
                                axum::Json(serde_json::json!({"errors": ["permission denied"]})),
                            );
                        }
                        namespaces.lock().await.push((
                            "destroy",
                            headers
                                .get("X-Vault-Namespace")
                                .and_then(|n| n.to_str().ok())
                                .map(str::to_string),
                        ));
                        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
                        let versions: Vec<String> = v["versions"]
                            .as_array()
                            .map(|a| {
                                a.iter()
                                    .filter_map(|x| x.as_str().map(str::to_string))
                                    .collect()
                            })
                            .unwrap_or_default();
                        destroys
                            .lock()
                            .await
                            .push((format!("secret/data/{path}"), versions));
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
                    let reject_upto = reject_list;
                    let agent_denied = agent_denied_lists;
                    async move {
                        let token = headers
                            .get("X-Vault-Token")
                            .and_then(|t| t.to_str().ok())
                            .unwrap_or_default()
                            .to_string();
                        if !token_accepted(&token, &reject_upto) {
                            return (
                                axum::http::StatusCode::FORBIDDEN,
                                axum::Json(serde_json::json!({"errors": ["permission denied"]})),
                            );
                        }
                        if token == "test-agent-token"
                            && agent_denied
                                .lock()
                                .await
                                .iter()
                                .any(|p| path.starts_with(p.as_str()))
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
                    let namespaces = ns_delete;
                    let reject_upto = reject_delete;
                    async move {
                        let token = headers
                            .get("X-Vault-Token")
                            .and_then(|t| t.to_str().ok())
                            .unwrap_or_default()
                            .to_string();
                        if !token_accepted(&token, &reject_upto) {
                            return (
                                axum::http::StatusCode::FORBIDDEN,
                                axum::Json(serde_json::json!({"errors": ["permission denied"]})),
                            );
                        }
                        namespaces.lock().await.push((
                            "delete",
                            headers
                                .get("X-Vault-Namespace")
                                .and_then(|n| n.to_str().ok())
                                .map(str::to_string),
                        ));
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
async fn agent_resolution_policy_denial_scopes_instead_of_aborting() {
    // platform-model.md §7 (review task-273): Vault policies scope the
    // secrets an agent receives. An operator-scoped role that is DENIED
    // the tenant-level subtree must still receive the workspace/repo
    // scopes it may read — the resolution yields the permitted subset,
    // it does not fail (and everything that is not an explicit policy
    // denial still fails closed).
    let (addr, _store, observed) = spawn_vault().await;
    let repo = VaultSecretRepository::new(config(&addr)).unwrap();

    repo.create(
        &sample_secret("TENANT_KEY", SecretScope::Tenant, "t1", "t1"),
        b"tenant-value",
    )
    .await
    .unwrap();
    repo.create(
        &sample_secret("WORKSPACE_KEY", SecretScope::Workspace, "ws-1", "t1"),
        b"workspace-value",
    )
    .await
    .unwrap();
    repo.create(
        &sample_secret("REPO_KEY", SecretScope::Repo, "repo-1", "t1"),
        b"repo-value",
    )
    .await
    .unwrap();

    // The agent's Vault policy denies the tenant subtree (list AND read).
    observed
        .agent_denied_prefixes
        .lock()
        .await
        .push("gyre-secrets/t1/tenant".to_string());

    let resolved = repo
        .resolve_for_agent_as("t1", "ws-1", "repo-1", None, "agent-jwt-abc")
        .await
        .expect("a policy denial must scope the resolution, not fail it");
    let map: HashMap<String, Vec<u8>> = resolved.into_iter().collect();
    assert_eq!(
        map.get("WORKSPACE_KEY").unwrap(),
        b"workspace-value",
        "the permitted workspace scope must still resolve"
    );
    assert_eq!(
        map.get("REPO_KEY").unwrap(),
        b"repo-value",
        "the permitted repo scope must still resolve"
    );
    assert!(
        !map.contains_key("TENANT_KEY"),
        "the policy-denied tenant scope must not be delivered: {map:?}"
    );

    // The SERVER path is not scoped by agent policy denials: a 403 there
    // is a token/misconfiguration failure that must surface, never a
    // silent subset. (The static server token is accepted everywhere in
    // this stub, so the server resolution still sees everything.)
    let server_resolved = repo
        .resolve_for_agent("t1", "ws-1", "repo-1", None)
        .await
        .unwrap();
    assert_eq!(server_resolved.len(), 3);
}

#[tokio::test]
async fn replace_rewrites_value_and_metadata_as_one_versioned_write() {
    // review task-273: credential rewrites (rotation, re-enrollment) go
    // through the port's atomic `replace` — never delete + create.
    let (addr, store, _observed) = spawn_vault().await;
    let repo = VaultSecretRepository::new(config(&addr)).unwrap();

    let secret = sample_secret("DATABASE_URL", SecretScope::Repo, "repo-1", "t1");
    repo.create(&secret, b"postgres://old").await.unwrap();

    // Replace value + metadata in one call.
    let mut updated = secret.clone();
    updated.expires_at = Some(1_700_000_900);
    updated.last_rotated_at = Some(1_700_000_100);
    repo.replace(&updated, b"postgres://new").await.unwrap();

    let value = repo.get_value(&secret.id, "t1").await.unwrap().unwrap();
    assert_eq!(value, b"postgres://new");
    let found = repo.find_by_id(&secret.id, "t1").await.unwrap().unwrap();
    assert_eq!(found.expires_at, Some(1_700_000_900));
    assert_eq!(found.last_rotated_at, Some(1_700_000_100));

    // The old data path was reused (a versioned overwrite, not a second
    // location): exactly one data entry and one index entry exist.
    {
        let s = store.lock().await;
        let data_entries: Vec<&String> = s
            .keys()
            .filter(|k| k.starts_with("secret/data/gyre-secrets/t1/repo/repo-1/"))
            .collect();
        assert_eq!(data_entries.len(), 1, "replace must not fork locations");
        assert!(s.contains_key("secret/data/gyre-secrets/t1/_idx/id-DATABASE_URL-repo-1"));
    }

    // Port contract: replacing an unknown id fails.
    let mut ghost = sample_secret("GHOST", SecretScope::Repo, "repo-1", "t1");
    ghost.id = Id::new("id-ghost");
    let err = repo.replace(&ghost, b"v").await.unwrap_err();
    assert!(err.to_string().contains("not found"));
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

/// Review task-273: a caller-supplied secret name becomes a KV path
/// segment — separators and traversal segments must be rejected by the
/// adapter itself (defense in depth behind the API validation), or a
/// `../`-bearing name writes outside the caller's tenant subtree and a
/// `/`-bearing name silently vanishes from list/get_by_scope.
#[tokio::test]
async fn create_rejects_path_unsafe_names() {
    let (addr, store, _observed) = spawn_vault().await;
    let repo = VaultSecretRepository::new(config(&addr)).unwrap();

    for bad in ["../escape", "a/b", "a\\b", "..", ".", "bad\u{0}name"] {
        let secret = sample_secret(bad, SecretScope::Repo, "repo-1", "t1");
        let err = repo.create(&secret, b"v").await.unwrap_err();
        assert!(
            err.to_string().contains("traversal") || err.to_string().contains("control"),
            "name {bad:?} must be rejected as path-unsafe, got: {err}"
        );
    }

    // Nothing was written — in particular nothing outside the tenant
    // subtree.
    assert!(
        store.lock().await.is_empty(),
        "no path-unsafe write may land"
    );
}

/// Review task-273: the raw secret `id` (verbatim from the delete/rotate
/// URL path) and the request-body `scope_id` are interpolated into Vault
/// KV paths — a crafted `../`-bearing id escapes the `_idx` subtree (it
/// can read ANOTHER tenant's index entry and target an arbitrary metadata
/// path for the index DELETE), and a `../`-bearing scope_id addresses
/// another scope's subtree. Both must be rejected as path-unsafe segments
/// by the adapter itself.
#[tokio::test]
async fn path_unsafe_ids_and_scope_ids_are_rejected() {
    let (addr, store, observed) = spawn_vault().await;
    let repo = VaultSecretRepository::new(config(&addr)).unwrap();

    // Seed one real secret in tenant t1.
    let real = sample_secret("REAL", SecretScope::Repo, "repo-1", "t1");
    repo.create(&real, b"v").await.unwrap();
    observed.reads.lock().await.clear();

    // A crafted id (the delete/rotate URL-path shape) is rejected on every
    // id-addressed operation — BEFORE any Vault request is made.
    for bad in ["../t2/_idx/x", "a/b", "a\\b", "..", "bad\u{0}id"] {
        let crafted = Id::new(bad);
        let err = repo.get_value(&crafted, "t1").await.unwrap_err();
        assert!(
            err.to_string().contains("traversal") || err.to_string().contains("control"),
            "id {bad:?} must be rejected as path-unsafe, got: {err}"
        );
        let err = repo.delete(&crafted, "t1").await.unwrap_err();
        assert!(
            err.to_string().contains("traversal") || err.to_string().contains("control"),
            "id {bad:?} must be rejected as path-unsafe on delete, got: {err}"
        );
        let err = repo.rotate(&crafted, b"v", "t1").await.unwrap_err();
        assert!(
            err.to_string().contains("traversal") || err.to_string().contains("control"),
            "id {bad:?} must be rejected as path-unsafe on rotate, got: {err}"
        );
    }

    // A crafted request-body scope_id is rejected on create and
    // list_by_scope.
    for bad in ["../escape", "a/b", ".."] {
        let mut secret = sample_secret("NEW", SecretScope::Repo, bad, "t1");
        secret.scope_id = bad.to_string();
        let err = repo.create(&secret, b"v").await.unwrap_err();
        assert!(
            err.to_string().contains("traversal") || err.to_string().contains("control"),
            "scope_id {bad:?} must be rejected as path-unsafe, got: {err}"
        );
        let err = repo
            .list_by_scope(SecretScope::Repo, bad, "t1")
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("traversal") || err.to_string().contains("control"),
            "scope_id {bad:?} must be rejected as path-unsafe on list, got: {err}"
        );
    }

    // No Vault request addressed anything outside tenant t1's subtree —
    // in particular no cross-tenant index read ever happened.
    // (Scoped: the reads guard must be released before the final
    // get_value — its server handler pushes into the same mutex.)
    {
        let reads = observed.reads.lock().await;
        assert!(
            reads
                .iter()
                .all(|(p, _)| p.starts_with("secret/data/gyre-secrets/t1/")),
            "no request may escape the caller's tenant subtree, saw: {reads:?}"
        );
    }
    // The seeded secret is untouched.
    assert_eq!(
        repo.get_value(&real.id, "t1").await.unwrap(),
        Some(b"v".to_vec())
    );
    assert!(store.lock().await.len() >= 2, "seeded entries remain");
}

/// Review task-273: KV v2 writes are versioned appends — after a rotate,
/// the prior version stays readable at `?version=N` (by any token with
/// read on the path, the agent JWT's policy included) unless it is
/// explicitly destroyed. Rotate must destroy every superseded version.
#[tokio::test]
async fn rotate_destroys_superseded_versions() {
    let (addr, _store, observed) = spawn_vault().await;
    let repo = VaultSecretRepository::new(config(&addr)).unwrap();

    let secret = sample_secret("ROT", SecretScope::Repo, "repo-1", "t1");
    repo.create(&secret, b"v1").await.unwrap();
    observed.destroys.lock().await.clear();

    let data_path = "secret/data/gyre-secrets/t1/repo/repo-1/ROT";
    repo.rotate(&secret.id, b"v2", "t1").await.unwrap();

    // The superseded version 1 was destroyed at the secret's data path.
    // (Scoped: the guard must be released before the requests below —
    // the stub's destroy handler pushes into the same mutex.)
    {
        let destroys = observed.destroys.lock().await;
        assert!(
            destroys
                .iter()
                .any(|(p, versions)| p == data_path && versions == &["1".to_string()]),
            "rotate must destroy the superseded version 1, saw: {destroys:?}"
        );
    }

    // The new value is served.
    assert_eq!(
        repo.get_value(&secret.id, "t1").await.unwrap(),
        Some(b"v2".to_vec())
    );

    // A second rotation destroys everything superseded — versions 1 and
    // 2 (version 2 was superseded by this very rotation; robust across
    // retries after a failed destroy).
    repo.rotate(&secret.id, b"v3", "t1").await.unwrap();
    let destroys = observed.destroys.lock().await;
    assert!(
        destroys.iter().any(|(p, versions)| p == data_path
            && versions.contains(&"1".to_string())
            && versions.contains(&"2".to_string())),
        "the second rotation must destroy versions 1 and 2, saw: {destroys:?}"
    );
    assert_eq!(
        repo.get_value(&secret.id, "t1").await.unwrap(),
        Some(b"v3".to_vec())
    );
}

/// Review task-273: `replace` (the rotation / re-enrollment write path)
/// supersedes a version at the same data path — the prior version must
/// be destroyed there too, exactly like rotate.
#[tokio::test]
async fn replace_destroys_superseded_versions_at_unchanged_location() {
    let (addr, _store, observed) = spawn_vault().await;
    let repo = VaultSecretRepository::new(config(&addr)).unwrap();

    let secret = sample_secret("REP", SecretScope::Repo, "repo-1", "t1");
    repo.create(&secret, b"old").await.unwrap();
    observed.destroys.lock().await.clear();

    let mut updated = secret.clone();
    updated.last_rotated_at = Some(1_700_000_100);
    repo.replace(&updated, b"new").await.unwrap();

    let destroys = observed.destroys.lock().await;
    let data_path = "secret/data/gyre-secrets/t1/repo/repo-1/REP";
    assert!(
        destroys
            .iter()
            .any(|(p, versions)| p == data_path && versions == &["1".to_string()]),
        "replace must destroy the superseded version at the data path, saw: {destroys:?}"
    );
    assert_eq!(
        repo.get_value(&secret.id, "t1").await.unwrap(),
        Some(b"new".to_vec())
    );
}

/// Review task-273: with GYRE_VAULT_NAMESPACE set, the X-Vault-Namespace
/// header must ride EVERY server-token request — index reads and deletes
/// previously dropped it, so reads failed OPEN (Ok(None) for a secret
/// that exists in the real namespace) and deletes 404'd (treated as
/// success) while the secret survived. The AppRole and JWT LOGIN
/// requests must carry it too: a namespace-less login authenticates
/// against the root namespace while every KV operation targets the
/// configured one, so both the admin write path and agent-scoped
/// resolution would fail with a namespace configured (Vault Enterprise).
#[tokio::test]
async fn namespace_header_rides_index_reads_and_deletes() {
    let (addr, _store, observed) = spawn_vault().await;
    let mut cfg = config(&addr);
    cfg.namespace = Some("ns-1".to_string());
    // AppRole (not a static token) so the admin write path performs a
    // real login, and the agent resolution below performs a JWT login.
    cfg.token = None;
    cfg.role_id = Some("role".to_string());
    cfg.secret_id = Some("secret".to_string());
    let repo = VaultSecretRepository::new(cfg).unwrap();

    let secret = sample_secret("NS", SecretScope::Repo, "repo-1", "t1");
    repo.create(&secret, b"v").await.unwrap();

    // get_value performs an AppRole login + an index read + a data read;
    // list_by_scope a metadata LIST + data reads; delete two metadata
    // DELETEs; resolve_for_agent_as a JWT login + reads.
    assert_eq!(
        repo.get_value(&secret.id, "t1").await.unwrap(),
        Some(b"v".to_vec())
    );
    repo.list_by_scope(SecretScope::Repo, "repo-1", "t1")
        .await
        .unwrap();
    repo.delete(&secret.id, "t1").await.unwrap();
    // Re-create for the agent-scoped resolution (JWT login path).
    repo.create(&secret, b"v").await.unwrap();
    let resolved = repo
        .resolve_for_agent_as("t1", "ws-1", "repo-1", None, "agent-jwt-ns")
        .await
        .unwrap();
    assert!(
        resolved.iter().any(|(name, _)| name == "NS"),
        "agent resolution must return the secret: {resolved:?}"
    );

    let namespaces = observed.namespaces.lock().await;
    assert!(
        namespaces
            .iter()
            .all(|(_, ns)| ns.as_deref() == Some("ns-1")),
        "every recorded request must carry the configured namespace: {namespaces:?}"
    );
    assert!(
        namespaces.iter().any(|(op, _)| *op == "data_read"),
        "data reads must be observed: {namespaces:?}"
    );
    assert!(
        namespaces.iter().any(|(op, _)| *op == "delete"),
        "deletes must be observed: {namespaces:?}"
    );
    assert!(
        namespaces.iter().any(|(op, _)| *op == "approle_login"),
        "the AppRole login must be observed (and namespaced): {namespaces:?}"
    );
    assert!(
        namespaces.iter().any(|(op, _)| *op == "jwt_login"),
        "the JWT login must be observed (and namespaced): {namespaces:?}"
    );
}

/// Review task-273: an AppRole token whose TTL expired (rejected by
/// Vault) must trigger a re-login on EVERY operation — get_value,
/// list_by_scope, rotate and resolve_for_agent previously fetched the
/// cached token directly, so they 403'd forever after expiry.
#[tokio::test]
async fn expired_approle_token_relogs_in_for_every_operation() {
    let (addr, _store, observed) = spawn_vault().await;
    let mut cfg = config(&addr);
    cfg.token = None;
    cfg.role_id = Some("role".to_string());
    cfg.secret_id = Some("secret".to_string());
    let repo = VaultSecretRepository::new(cfg).unwrap();

    // Seed one secret while the (fresh) AppRole token is accepted.
    let secret = sample_secret("TTL", SecretScope::Repo, "repo-1", "t1");
    repo.create(&secret, b"v1").await.unwrap();

    // Expire the cached token: AppRole tokens with generation <= 1 are
    // now rejected — exactly the shape of a TTL expiry mid-lifetime.
    observed
        .reject_approle_upto
        .store(1, std::sync::atomic::Ordering::SeqCst);

    // Every previously-unwrapped operation must transparently re-login
    // (generation 2) instead of failing forever.
    assert_eq!(
        repo.get_value(&secret.id, "t1").await.unwrap(),
        Some(b"v1".to_vec()),
        "get_value must survive an expired AppRole token"
    );
    repo.list_by_scope(SecretScope::Repo, "repo-1", "t1")
        .await
        .expect("list_by_scope must survive an expired AppRole token");
    repo.rotate(&secret.id, b"v2", "t1")
        .await
        .expect("rotate must survive an expired AppRole token");
    let resolved = repo
        .resolve_for_agent("t1", "ws-1", "repo-1", None)
        .await
        .expect("server-token resolution must survive an expired AppRole token");
    assert!(
        resolved.iter().any(|(name, _)| name == "TTL"),
        "resolution must return the secret: {resolved:?}"
    );
    assert_eq!(
        repo.get_value(&secret.id, "t1").await.unwrap(),
        Some(b"v2".to_vec()),
        "the rotate above must have taken effect"
    );
}
