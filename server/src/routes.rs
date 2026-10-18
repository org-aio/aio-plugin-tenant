//! 租户 HTTP 入口：身份和权限由宿主注入，写操作仍会经过宿主身份服务复核。

use aio_plugin_tenant_model::{
    CreateTenantRequest, SwitchTenantRequest, TenantErrorResponse, TenantItem, TenantListing,
    TenantResponse, TenantSession,
};
use axum::{
    Extension, Json, Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use az_plugin_process::{Caller, IdentityService};
use std::sync::Arc;

type Service = Arc<IdentityService>;

pub fn router(identity: Service) -> Router {
    Router::new()
        .route("/tenants", get(list).post(create))
        .route("/tenants/switch", post(switch))
        .route("/tenants/current", put(rename))
        .with_state(identity)
}

async fn list(
    State(identity): State<Service>,
    Extension(caller): Extension<Caller>,
) -> Result<Json<TenantResponse<TenantListing>>, TenantError> {
    let tenants = identity
        .list_tenants(&caller.context_id)
        .await
        .map_err(TenantError::from_identity)?;
    Ok(Json(TenantResponse {
        data: TenantListing {
            tenants: tenants
                .into_iter()
                .map(|tenant| TenantItem {
                    id: tenant.id,
                    label: tenant.label,
                    current: tenant.current,
                })
                .collect(),
            can_manage: caller.has_permission("tenant:manage"),
        },
    }))
}

async fn create(
    State(identity): State<Service>,
    Extension(caller): Extension<Caller>,
    Json(request): Json<CreateTenantRequest>,
) -> Result<Json<TenantResponse<TenantItem>>, TenantError> {
    require_tenant_manage(&caller)?;
    let tenant = identity
        .create_tenant(&caller.context_id, &request.label)
        .await
        .map_err(TenantError::from_identity)?;
    Ok(Json(TenantResponse {
        data: TenantItem {
            id: tenant.id,
            label: tenant.label,
            current: tenant.current,
        },
    }))
}

async fn rename(
    State(identity): State<Service>,
    Extension(caller): Extension<Caller>,
    Json(request): Json<CreateTenantRequest>,
) -> Result<StatusCode, TenantError> {
    require_tenant_manage(&caller)?;
    identity
        .rename_current_tenant(&caller.context_id, &request.label)
        .await
        .map_err(TenantError::from_identity)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn switch(
    State(identity): State<Service>,
    Extension(caller): Extension<Caller>,
    Json(request): Json<SwitchTenantRequest>,
) -> Result<Json<TenantResponse<TenantSession>>, TenantError> {
    let session = identity
        .switch_tenant(&caller.context_id, &request.tenant_id)
        .await
        .map_err(TenantError::from_identity)?;
    Ok(Json(TenantResponse {
        data: TenantSession {
            tenant_id: session.tenant_id,
            tenant_label: session.tenant_label,
            permissions: session.permissions,
        },
    }))
}

fn require_tenant_manage(caller: &Caller) -> Result<(), TenantError> {
    if caller.has_permission("tenant:manage") {
        Ok(())
    } else {
        Err(TenantError::forbidden("当前角色没有租户管理权限"))
    }
}

struct TenantError {
    status: StatusCode,
    message: String,
}

impl TenantError {
    fn forbidden(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            message: message.into(),
        }
    }

    /// 身份服务的失败大多来自业务校验；权限与成员关系保留 403 语义。
    fn from_identity(error: anyhow::Error) -> Self {
        let message = format!("{error:#}");
        let status = if message.contains("权限") || message.contains("不属于") {
            StatusCode::FORBIDDEN
        } else {
            StatusCode::BAD_REQUEST
        };
        Self { status, message }
    }
}

impl IntoResponse for TenantError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(TenantErrorResponse {
                error: self.message,
            }),
        )
            .into_response()
    }
}
