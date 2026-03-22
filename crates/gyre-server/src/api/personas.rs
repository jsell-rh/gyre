use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use gyre_common::Id;
use gyre_domain::{BudgetConfig, Persona, PersonaScope};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::AppState;

use super::error::ApiError;
use super::{new_id, now_secs};

#[derive(Deserialize, Serialize, Clone)]
#[serde(tag = "kind", content = "id")]
pub enum PersonaScopeDto {
    Tenant(String),
    Workspace(String),
    Repo(String),
}

impl From<PersonaScopeDto> for PersonaScope {
    fn from(d: PersonaScopeDto) -> Self {
        match d {
            PersonaScopeDto::Tenant(id) => PersonaScope::Tenant(Id::new(id)),
            PersonaScopeDto::Workspace(id) => PersonaScope::Workspace(Id::new(id)),
            PersonaScopeDto::Repo(id) => PersonaScope::Repo(Id::new(id)),
        }
    }
}

impl From<PersonaScope> for PersonaScopeDto {
    fn from(s: PersonaScope) -> Self {
        match s {
            PersonaScope::Tenant(id) => PersonaScopeDto::Tenant(id.to_string()),
            PersonaScope::Workspace(id) => PersonaScopeDto::Workspace(id.to_string()),
            PersonaScope::Repo(id) => PersonaScopeDto::Repo(id.to_string()),
        }
    }
}

#[derive(Deserialize)]
pub struct CreatePersonaRequest {
    pub name: String,
    pub slug: String,
    pub scope: PersonaScopeDto,
    pub system_prompt: String,
    pub capabilities: Option<Vec<String>>,
    pub protocols: Option<Vec<String>>,
    pub model: Option<String>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<u32>,
    pub budget: Option<BudgetConfig>,
}

#[derive(Deserialize)]
pub struct UpdatePersonaRequest {
    pub name: Option<String>,
    pub system_prompt: Option<String>,
    pub capabilities: Option<Vec<String>>,
    pub protocols: Option<Vec<String>>,
    pub model: Option<String>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<u32>,
    pub budget: Option<BudgetConfig>,
}

#[derive(Deserialize)]
pub struct ListPersonasQuery {
    pub scope: Option<String>,
    pub scope_id: Option<String>,
}

#[derive(Serialize)]
pub struct PersonaResponse {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub scope: PersonaScopeDto,
    pub system_prompt: String,
    pub capabilities: Vec<String>,
    pub protocols: Vec<String>,
    pub model: Option<String>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<u32>,
    pub budget: Option<BudgetConfig>,
    pub created_at: u64,
}

impl From<Persona> for PersonaResponse {
    fn from(p: Persona) -> Self {
        Self {
            id: p.id.to_string(),
            name: p.name,
            slug: p.slug,
            scope: PersonaScopeDto::from(p.scope),
            system_prompt: p.system_prompt,
            capabilities: p.capabilities,
            protocols: p.protocols,
            model: p.model,
            temperature: p.temperature,
            max_tokens: p.max_tokens,
            budget: p.budget,
            created_at: p.created_at,
        }
    }
}

pub async fn create_persona(
    _admin: crate::auth::AdminOnly,
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreatePersonaRequest>,
) -> Result<(StatusCode, Json<PersonaResponse>), ApiError> {
    let now = now_secs();
    let mut persona = Persona::new(
        new_id(),
        req.name,
        req.slug,
        PersonaScope::from(req.scope),
        req.system_prompt,
        now,
    );
    persona.capabilities = req.capabilities.unwrap_or_default();
    persona.protocols = req.protocols.unwrap_or_default();
    persona.model = req.model;
    persona.temperature = req.temperature;
    persona.max_tokens = req.max_tokens;
    persona.budget = req.budget;
    state.personas.create(&persona).await?;
    Ok((StatusCode::CREATED, Json(PersonaResponse::from(persona))))
}

pub async fn list_personas(
    State(state): State<Arc<AppState>>,
    Query(q): Query<ListPersonasQuery>,
) -> Result<Json<Vec<PersonaResponse>>, ApiError> {
    let personas =
        if let (Some(scope), Some(scope_id)) = (q.scope.as_deref(), q.scope_id.as_deref()) {
            let scope_enum = match scope {
                "tenant" => PersonaScope::Tenant(Id::new(scope_id)),
                "workspace" => PersonaScope::Workspace(Id::new(scope_id)),
                "repo" => PersonaScope::Repo(Id::new(scope_id)),
                _ => return Err(ApiError::InvalidInput(format!("unknown scope: {scope}"))),
            };
            state.personas.list_by_scope(&scope_enum).await?
        } else {
            state.personas.list().await?
        };
    Ok(Json(
        personas.into_iter().map(PersonaResponse::from).collect(),
    ))
}

pub async fn get_persona(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<PersonaResponse>, ApiError> {
    let persona = state
        .personas
        .find_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("persona {id} not found")))?;
    Ok(Json(PersonaResponse::from(persona)))
}

pub async fn update_persona(
    _admin: crate::auth::AdminOnly,
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<UpdatePersonaRequest>,
) -> Result<Json<PersonaResponse>, ApiError> {
    let mut persona = state
        .personas
        .find_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("persona {id} not found")))?;
    if let Some(name) = req.name {
        persona.name = name;
    }
    if let Some(prompt) = req.system_prompt {
        persona.system_prompt = prompt;
    }
    if let Some(caps) = req.capabilities {
        persona.capabilities = caps;
    }
    if let Some(protos) = req.protocols {
        persona.protocols = protos;
    }
    if let Some(model) = req.model {
        persona.model = Some(model);
    }
    if let Some(temp) = req.temperature {
        persona.temperature = Some(temp);
    }
    if let Some(max_tok) = req.max_tokens {
        persona.max_tokens = Some(max_tok);
    }
    if let Some(budget) = req.budget {
        persona.budget = Some(budget);
    }
    state.personas.update(&persona).await?;
    Ok(Json(PersonaResponse::from(persona)))
}

pub async fn delete_persona(
    _admin: crate::auth::AdminOnly,
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    state
        .personas
        .find_by_id(&Id::new(&id))
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("persona {id} not found")))?;
    state.personas.delete(&Id::new(id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use crate::mem::test_state;
    use axum::{body::Body, Router};
    use http::{Request, StatusCode};
    use tower::ServiceExt;

    fn app() -> Router {
        crate::api::api_router().with_state(test_state())
    }

    async fn body_json(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn create_and_list_personas() {
        let app = app();
        let body = serde_json::json!({
            "name": "security",
            "slug": "security",
            "scope": { "kind": "Tenant", "id": "t1" },
            "system_prompt": "You are a security reviewer.",
            "capabilities": ["security", "owasp"]
        });
        let create_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/personas")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(create_resp.status(), StatusCode::CREATED);
        let created = body_json(create_resp).await;
        assert_eq!(created["name"], "security");
        assert_eq!(created["scope"]["kind"], "Tenant");

        let list_resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/personas?scope=tenant&scope_id=t1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(list_resp.status(), StatusCode::OK);
        let list = body_json(list_resp).await;
        assert_eq!(list.as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn get_persona_not_found() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/personas/nonexistent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn update_persona() {
        let app = app();
        let body = serde_json::json!({
            "name": "reviewer",
            "slug": "reviewer",
            "scope": { "kind": "Workspace", "id": "ws1" },
            "system_prompt": "Review code carefully."
        });
        let create_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/personas")
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let created = body_json(create_resp).await;
        let id = created["id"].as_str().unwrap().to_string();

        let update_body = serde_json::json!({ "name": "senior-reviewer" });
        let update_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri(format!("/api/v1/personas/{id}"))
                    .header("content-type", "application/json")
                    .header("authorization", "Bearer test-token")
                    .body(Body::from(serde_json::to_vec(&update_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(update_resp.status(), StatusCode::OK);
        let updated = body_json(update_resp).await;
        assert_eq!(updated["name"], "senior-reviewer");
    }
}
