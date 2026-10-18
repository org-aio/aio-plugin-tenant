use super::http;
use aio_plugin_tenant_model::TenantItem;
use az_ui_components::{
    admin::{
        AsyncResult, CollectionTable, EditorDialog, PageHeader, PageSurface, RequestState,
        SortValue, StatusMessage,
    },
    badge::{Badge, BadgeVariant},
    button::{Button, ButtonSize, ButtonVariant},
    data_table::{DataTableCellContext, DataTableColumn},
    input::Input,
};
use dioxus::prelude::*;
use dioxus_icons::lucide::{LogIn, Pencil, Plus, RefreshCw};

#[allow(non_snake_case)]
pub fn TenantPage() -> Element {
    let mut revision = use_signal(|| 0_u64);
    let resource = use_resource(move || {
        let _ = revision();
        http::load()
    });
    let mut editor = use_signal(|| None::<Option<TenantItem>>);
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let listing = match resource.read().as_ref().cloned() {
        Some(Ok(value)) => value,
        Some(Err(error)) => {
            return rsx! { PageSurface { RequestState { error, on_retry: move |_| revision += 1 } } };
        }
        None => return rsx! { PageSurface { RequestState {} } },
    };
    let can_manage = listing.can_manage;
    let tenants = listing.tenants;
    rsx! {
        PageSurface {
            PageHeader { title: "租户管理", detail: format!("已加入 {} 个租户", tenants.len()),
                Button { variant: ButtonVariant::Outline, size: ButtonSize::Icon, title: "刷新租户", aria_label: "刷新租户", onclick: move |_| revision += 1, RefreshCw {} }
                if can_manage { Button { onclick: move |_| editor.set(Some(None)), Plus {} "新建租户" } }
            }
            if let Some(message) = error() { StatusMessage { error: true, message } }
            if busy() { p { role: "status", "正在切换租户" } }
            CollectionTable { label: "租户", rows: tenants,
                columns: vec![DataTableColumn::leaf("label", "租户名称").width(260), DataTableColumn::leaf("id", "租户 ID").width(320), DataTableColumn::leaf("status", "状态").width(130), DataTableColumn::leaf("actions", "操作").width(130)],
                row_key: |t: TenantItem| t.id, search_text: |t: TenantItem| format!("{} {}", t.label, t.id), sort_value: |(t, _): (TenantItem, String)| SortValue::Text(t.label), sortable: vec!["label".into()],
                render_cell: move |context: DataTableCellContext<TenantItem>| { let tenant = context.row; match context.column.key.as_str() {
                    "label" => rsx! { "{tenant.label}" }, "id" => rsx! { code { class: "admin-code", "{tenant.id}" } },
                    "status" => rsx! { if tenant.current { Badge { variant: BadgeVariant::Outline, "当前租户" } } },
                    "actions" => rsx! { div { class: "admin-actions",
                        if tenant.current && can_manage { Button { variant: ButtonVariant::Ghost, size: ButtonSize::IconSm, title: "重命名租户", aria_label: "重命名租户", onclick: move |_| editor.set(Some(Some(tenant.clone()))), Pencil {} } }
                        else if !tenant.current { Button { variant: ButtonVariant::Outline, disabled: busy(), aria_label: "切换到 {tenant.label}", onclick: move |_| {
                            if busy() { return; } busy.set(true); error.set(None); let id = tenant.id.clone();
                            spawn(async move {
                                if let Err(message) = http::switch(id).await { error.set(Some(message)); busy.set(false); }
                            });
                        }, LogIn {} "切换" } }
                    } }, _ => rsx! {},
                } },
            }
        }
        if let Some(value) = editor() { TenantEditor { value, on_close: move |_| editor.set(None), on_saved: move |_| { editor.set(None); revision += 1; } } }
    }
}

#[component]
fn TenantEditor(
    value: Option<TenantItem>,
    on_close: Callback<()>,
    on_saved: Callback<()>,
) -> Element {
    let mut label = use_signal(|| value.as_ref().map(|t| t.label.clone()).unwrap_or_default());
    rsx! { EditorDialog { title: if value.is_some() { "重命名租户" } else { "新建租户" }, description: "租户名称最多 80 个字符。", on_close, on_saved,
        save: move |_| -> AsyncResult<()> { let id = value.as_ref().map(|t| t.id.clone()); let label = label(); Box::pin(async move { http::save(id, label).await }) },
        label { class: "admin-field", span { "租户名称" } Input { aria_label: "租户名称", value: label(), required: true, maxlength: "80", oninput: move |event: FormEvent| label.set(event.value()) } }
    } }
}
