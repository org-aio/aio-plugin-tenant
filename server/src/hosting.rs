//! 独立租户进程：只通过宿主受控身份通道读写租户，不接触数据库。

use anyhow::Result;
use axum::{Json, routing::get};
use az_plugin_process::Host;
use serde_json::{Value, json};
use std::sync::Arc;

/// 生产与真实 HTTP 验收使用同一宿主配置和受控入口。
pub async fn run() -> Result<()> {
    let host = Host::from_env()?;
    let identity = Arc::new(host.identity()?);
    let router = host
        .protect(crate::routes::router(identity))
        .route("/health", get(|| async { "ok" }))
        .route("/aio/describe", get(describe));
    host.serve(router).await
}

/// 页面随整包版本动态贡献；停用整包后入口一起移除。
pub async fn describe() -> Json<Value> {
    Json(json!({"label":"租户管理","pages":[
        {
            "id":"tenants", "label":"租户管理", "entry":"index.html",
            "scene":["system","系统"], "menu_path":[],
            "permission":null, "surface":"workspace"
        },
        {
            "id":"tenant-switcher", "label":"切换租户", "entry":"index.html",
            "scene":null, "menu_path":[],
            "permission":null, "surface":"account-menu"
        }
    ]}))
}
