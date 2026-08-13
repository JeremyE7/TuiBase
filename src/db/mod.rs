pub mod backend;
pub mod models;
pub mod query;
pub mod sybase;

use anyhow::{Result, bail};

use crate::config::ConnectionProfile;

use self::{backend::DatabaseBackend, sybase::IsqlBackend};

pub fn backend_for(profile: &ConnectionProfile) -> Result<Box<dyn DatabaseBackend>> {
    match profile.backend.as_str() {
        "sybase_isql" => Ok(Box::new(IsqlBackend)),
        other => bail!("Backend no soportado: {other}"),
    }
}
