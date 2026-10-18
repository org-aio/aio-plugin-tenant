use aio_plugin_tenant_model::{
    CreateTenantRequest, SwitchTenantRequest, TenantErrorResponse, TenantItem, TenantListing,
    TenantResponse, TenantSession,
};
use serde::{Serialize, de::DeserializeOwned};
use wasm_bindgen::{JsValue, prelude::wasm_bindgen};
use wasm_bindgen_futures::JsFuture;

// 直接绑定宿主 SDK：身份由宿主注入，插件不接触会话 Cookie 或会话标识。
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["window", "aioPlugin"], js_name = json, catch)]
    fn json_call(method: &str, path: &str, body: &JsValue) -> Result<js_sys::Promise, JsValue>;
    #[wasm_bindgen(js_namespace = ["window", "aioPlugin"], js_name = refreshCatalog, catch)]
    fn refresh_catalog_call() -> Result<js_sys::Promise, JsValue>;
}

pub(super) async fn load() -> Result<TenantListing, String> {
    json::<TenantListing>("GET", "/tenants", None::<&()>).await
}

pub(super) async fn save(id: Option<String>, label: String) -> Result<(), String> {
    let request = CreateTenantRequest { label };
    match id {
        Some(_) => {
            empty("PUT", "/tenants/current", &request).await?;
        }
        None => {
            let _: TenantItem = json("POST", "/tenants", Some(&request)).await?;
        }
    }
    Ok(())
}

pub(super) async fn switch(id: String) -> Result<TenantSession, String> {
    let session = json::<TenantSession>(
        "POST",
        "/tenants/switch",
        Some(&SwitchTenantRequest { tenant_id: id }),
    )
    .await?;
    // 会话租户已变化；请求宿主重启应用，重新读取会话、权限和目录。
    JsFuture::from(refresh_catalog_call().map_err(js_error)?)
        .await
        .map_err(js_error)?;
    Ok(session)
}

async fn json<T: DeserializeOwned>(
    method: &str,
    path: &str,
    body: Option<&impl Serialize>,
) -> Result<T, String> {
    let body = body
        .map(|value| value.serialize(&serde_wasm_bindgen::Serializer::json_compatible()))
        .transpose()
        .map_err(|error| error.to_string())?
        .unwrap_or(JsValue::UNDEFINED);
    let response = JsFuture::from(json_call(method, path, &body).map_err(js_error)?)
        .await
        .map_err(js_error)?;
    serde_wasm_bindgen::from_value::<TenantResponse<T>>(response)
        .map(|value| value.data)
        .map_err(|error| error.to_string())
}

async fn empty(method: &str, path: &str, body: &impl Serialize) -> Result<(), String> {
    let body = body
        .serialize(&serde_wasm_bindgen::Serializer::json_compatible())
        .map_err(|error| error.to_string())?;
    JsFuture::from(json_call(method, path, &body).map_err(js_error)?)
        .await
        .map_err(js_error)?;
    Ok(())
}

fn js_error(error: JsValue) -> String {
    let message = error
        .as_string()
        .or_else(|| {
            js_sys::Reflect::get(&error, &JsValue::from_str("message"))
                .ok()
                .and_then(|value| value.as_string())
        })
        .unwrap_or_else(|| "宿主调用失败".into());
    serde_json::from_str::<TenantErrorResponse>(&message)
        .map(|body| body.error)
        .unwrap_or(message)
}
