#[tokio::main]
async fn main() -> anyhow::Result<()> {
    aio_plugin_tenant_server::hosting::run().await
}
