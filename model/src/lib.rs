use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TenantItem {
    pub id: String,
    pub label: String,
    pub current: bool,
}

/// 列表结果同时返回当前调用者能否管理租户，页面无需再读取宿主会话接口。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TenantListing {
    pub tenants: Vec<TenantItem>,
    pub can_manage: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CreateTenantRequest {
    pub label: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SwitchTenantRequest {
    pub tenant_id: String,
}

/// 切换后的会话摘要；不返回会话标识，也不把旧权限快照交给调用方。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TenantSession {
    pub tenant_id: String,
    pub tenant_label: String,
    pub permissions: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TenantResponse<T> {
    pub data: T,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TenantErrorResponse {
    pub error: String,
}
