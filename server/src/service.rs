use std::env;

use aio_plugin_tenant_model::TenantItem;
use anyhow::{Context as _, Result, ensure};
use sqlx::{PgPool, postgres::PgPoolOptions};

#[derive(Debug)]
pub struct TenantService {
    pool: PgPool,
}

impl TenantService {
    /// 宿主在发布路由前验证本代连接池可用，表结构由身份插件先行初始化。
    pub async fn initialize(&self) -> Result<()> {
        sqlx::query("SELECT 1").execute(&self.pool).await?;
        Ok(())
    }

    /// 宿主停止本代服务时释放连接池，租户与成员关系仍保存在数据库。
    pub async fn shutdown(&self) {
        self.pool.close().await;
    }

    pub fn from_env() -> Result<Self> {
        let database_url = env::var("AIO_DATABASE_URL")
            .or_else(|_| env::var("AZ_AIO_DATABASE_URL"))
            .context("租户插件缺少 AIO_DATABASE_URL")?;
        Ok(Self {
            pool: PgPoolOptions::new()
                .max_connections(8)
                .connect_lazy(&database_url)
                .context("创建租户数据库连接池失败")?,
        })
    }

    pub async fn list(&self, user_id: &str, current: &str) -> Result<Vec<TenantItem>> {
        let rows = sqlx::query_as::<_, (String, String)>(
            "SELECT tenants.id, tenants.label FROM tenant_memberships memberships JOIN tenants ON tenants.id = memberships.tenant_id WHERE memberships.user_id = $1 ORDER BY tenants.label",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(id, label)| TenantItem {
                current: id == current,
                id,
                label,
            })
            .collect())
    }

    pub async fn rename(&self, tenant_id: &str, label: &str) -> Result<()> {
        let label = label.trim();
        ensure!(
            !label.is_empty() && label.chars().count() <= 80,
            "租户名称需要 1 到 80 个字符"
        );
        let result = sqlx::query("UPDATE tenants SET label = $2 WHERE id = $1")
            .bind(tenant_id)
            .bind(label)
            .execute(&self.pool)
            .await?;
        ensure!(result.rows_affected() == 1, "租户不存在");
        Ok(())
    }

    pub async fn create(&self, user_id: &str, label: &str) -> Result<TenantItem> {
        let label = label.trim();
        ensure!(!label.is_empty(), "租户名称不能为空");
        ensure!(label.chars().count() <= 80, "租户名称不能超过 80 个字符");
        let tenant_id = uuid::Uuid::new_v4().to_string();
        let mut transaction = self.pool.begin().await?;
        sqlx::query("INSERT INTO tenants (id, label) VALUES ($1, $2)")
            .bind(&tenant_id)
            .bind(label)
            .execute(&mut *transaction)
            .await?;
        sqlx::query("INSERT INTO tenant_memberships (tenant_id, user_id, display_name) SELECT $1, id, display_name FROM identity_users WHERE id = $2")
            .bind(&tenant_id).bind(user_id).execute(&mut *transaction).await?;
        sqlx::query("INSERT INTO tenant_member_roles (tenant_id, user_id, role_id) VALUES ($1, $2, 'tenant-admin')")
            .bind(&tenant_id).bind(user_id).execute(&mut *transaction).await?;
        for permission in [
            "plugin:manage",
            "tenant:manage",
            "rbac:manage",
            "dictionary:manage",
            "file:manage",
        ] {
            sqlx::query("INSERT INTO role_permissions (tenant_id, role_id, permission) VALUES ($1, 'tenant-admin', $2)")
                .bind(&tenant_id).bind(permission).execute(&mut *transaction).await?;
        }
        sqlx::query("INSERT INTO role_permissions (tenant_id, role_id, permission) VALUES ($1, 'member', 'workspace:view')")
            .bind(&tenant_id).execute(&mut *transaction).await?;
        transaction.commit().await?;
        Ok(TenantItem {
            id: tenant_id,
            label: label.to_owned(),
            current: false,
        })
    }
}
