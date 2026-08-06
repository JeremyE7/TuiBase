use anyhow::Result;

use crate::{config::ConnectionProfile, db::models::TablePreview};

use super::models::{DbObject, ObjectKind, SqlOutput};

pub trait DatabaseBackend: Send + Sync {
    fn test_connection(&self, profile: &ConnectionProfile) -> Result<String>;
    fn list_databases(&self, profile: &ConnectionProfile) -> Result<Vec<String>>;
    fn list_objects(
        &self,
        profile: &ConnectionProfile,
        database: &str,
        kind: ObjectKind,
    ) -> Result<Vec<DbObject>>;
    fn object_definition(
        &self,
        profile: &ConnectionProfile,
        database: &str,
        object: &DbObject,
    ) -> Result<String>;
    fn preview_table(
        &self,
        profile: &ConnectionProfile,
        database: &str,
        object: &DbObject,
        row_limit: usize,
    ) -> Result<TablePreview>;
    fn execute(&self, profile: &ConnectionProfile, database: &str, sql: &str) -> Result<SqlOutput>;
}
