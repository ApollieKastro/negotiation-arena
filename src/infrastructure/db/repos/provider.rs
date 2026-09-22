//! Репозиторий провайдеров, моделей и назначений по ролям (SQLite).

use std::sync::Arc;

use rusqlite::{params, OptionalExtension, Row};

use crate::domain::entities::model::{ModelRole, ProviderKind};
use crate::domain::entities::provider::{ModelRecord, Provider, RoleAssignment};
use crate::domain::ports::ProviderRepository;
use crate::error::AppResult;
use crate::infrastructure::db::Database;

pub struct SqliteProviderRepo {
    db: Arc<Database>,
}

impl SqliteProviderRepo {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    fn map_provider(row: &Row<'_>) -> rusqlite::Result<Provider> {
        let kind: String = row.get(2)?;
        let is_enabled: i32 = row.get(6)?;
        Ok(Provider {
            id: row.get(0)?,
            name: row.get(1)?,
            kind: ProviderKind::from_str(&kind).unwrap_or(ProviderKind::OpenAiCompatible),
            base_url: row.get(3)?,
            api_key_encrypted: row.get(4)?,
            api_key_hint: row.get(5)?,
            is_enabled: is_enabled != 0,
            created_at: row.get(7)?,
            updated_at: row.get(8)?,
        })
    }

    fn map_model(row: &Row<'_>) -> rusqlite::Result<ModelRecord> {
        let role: String = row.get(2)?;
        let metadata_raw: String = row.get(7)?;
        Ok(ModelRecord {
            id: row.get(0)?,
            provider_id: row.get(1)?,
            role: ModelRole::from_slug(&role).unwrap_or(ModelRole::Llm),
            model_key: row.get(3)?,
            display_name: row.get(4)?,
            is_enabled: row.get::<_, i32>(5)? != 0,
            created_at: row.get(6)?,
            metadata: serde_json::from_str(&metadata_raw).unwrap_or(serde_json::Value::Null),
        })
    }
}

const SELECT_PROVIDER: &str = "SELECT id, name, kind, base_url, api_key_encrypted, \
    api_key_hint, is_enabled, created_at, updated_at FROM providers";

const SELECT_MODEL: &str = "SELECT id, provider_id, role, model_key, display_name, \
    is_enabled, created_at, metadata FROM models";

impl ProviderRepository for SqliteProviderRepo {
    fn list(&self) -> AppResult<Vec<Provider>> {
        let conn = self.db.conn();
        let sql = format!("{SELECT_PROVIDER} ORDER BY created_at ASC");
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], Self::map_provider)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    fn get(&self, id: &str) -> AppResult<Option<Provider>> {
        let conn = self.db.conn();
        let sql = format!("{SELECT_PROVIDER} WHERE id = ?1");
        conn.query_row(&sql, params![id], Self::map_provider)
            .optional()
            .map_err(Into::into)
    }

    fn upsert(&self, provider: &Provider) -> AppResult<()> {
        let conn = self.db.conn();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO providers (id, name, kind, base_url, api_key_encrypted, api_key_hint,
                is_enabled, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                kind = excluded.kind,
                base_url = excluded.base_url,
                api_key_encrypted = excluded.api_key_encrypted,
                api_key_hint = excluded.api_key_hint,
                is_enabled = excluded.is_enabled,
                updated_at = excluded.updated_at",
            params![
                provider.id,
                provider.name,
                provider.kind.as_str(),
                provider.base_url,
                provider.api_key_encrypted,
                provider.api_key_hint,
                provider.is_enabled as i32,
                provider.created_at,
                now,
            ],
        )?;
        Ok(())
    }

    fn delete(&self, id: &str) -> AppResult<()> {
        let conn = self.db.conn();
        // Модели удаляются каскадом (FK ON DELETE CASCADE).
        conn.execute("DELETE FROM providers WHERE id = ?1", params![id])?;
        Ok(())
    }

    fn list_models(&self, provider_id: Option<&str>) -> AppResult<Vec<ModelRecord>> {
        let conn = self.db.conn();
        match provider_id {
            Some(pid) => {
                let sql =
                    format!("{SELECT_MODEL} WHERE provider_id = ?1 ORDER BY role, display_name");
                let mut stmt = conn.prepare(&sql)?;
                let rows = stmt.query_map(params![pid], Self::map_model)?;
                rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
            }
            None => {
                let sql = format!("{SELECT_MODEL} ORDER BY role, display_name");
                let mut stmt = conn.prepare(&sql)?;
                let rows = stmt.query_map([], Self::map_model)?;
                rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
            }
        }
    }

    fn get_model(&self, id: &str) -> AppResult<Option<ModelRecord>> {
        let conn = self.db.conn();
        let sql = format!("{SELECT_MODEL} WHERE id = ?1");
        conn.query_row(&sql, params![id], Self::map_model)
            .optional()
            .map_err(Into::into)
    }

    fn upsert_model(&self, model: &ModelRecord) -> AppResult<()> {
        let conn = self.db.conn();
        let metadata = model.metadata.to_string();
        conn.execute(
            "INSERT INTO models (id, provider_id, role, model_key, display_name, is_enabled, metadata, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(id) DO UPDATE SET
                provider_id = excluded.provider_id,
                role = excluded.role,
                model_key = excluded.model_key,
                display_name = excluded.display_name,
                is_enabled = excluded.is_enabled,
                metadata = excluded.metadata",
            params![
                model.id,
                model.provider_id,
                model.role.slug(),
                model.model_key,
                model.display_name,
                model.is_enabled as i32,
                metadata,
                model.created_at,
            ],
        )?;
        Ok(())
    }

    fn delete_model(&self, id: &str) -> AppResult<()> {
        let conn = self.db.conn();
        conn.execute("DELETE FROM models WHERE id = ?1", params![id])?;
        Ok(())
    }

    fn role_assignments(&self) -> AppResult<Vec<RoleAssignment>> {
        let conn = self.db.conn();
        let mut stmt = conn.prepare("SELECT role, model_id, updated_at FROM role_assignments")?;
        let rows = stmt.query_map([], |row| {
            let role: String = row.get(0)?;
            Ok((
                ModelRole::from_slug(&role).unwrap_or(ModelRole::Llm),
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;

        let mut out = Vec::new();
        for row in rows {
            let (role, model_id, updated_at) = row?;
            out.push(RoleAssignment {
                role,
                model_id,
                updated_at,
            });
        }
        Ok(out)
    }

    fn set_role_assignment(&self, role: &str, model_id: &str) -> AppResult<()> {
        let conn = self.db.conn();
        conn.execute(
            "INSERT INTO role_assignments (role, model_id, updated_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(role) DO UPDATE SET model_id = excluded.model_id, updated_at = excluded.updated_at",
            params![role, model_id, chrono::Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }
}
