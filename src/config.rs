use std::{env, fs, path::PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub connections: Vec<ConnectionProfile>,
    #[serde(default = "default_catalog_ttl_hours")]
    pub catalog_ttl_hours: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConnectionProfile {
    pub name: String,
    #[serde(default = "default_backend")]
    pub backend: String,
    #[serde(default = "default_isql_path")]
    pub isql_path: String,
    pub userstore_key: Option<String>,
    pub server: Option<String>,
    pub username: Option<String>,
    pub password_env: Option<String>,
    pub database: Option<String>,
    pub charset: Option<String>,
    #[serde(default)]
    pub allow_writes: bool,
    #[serde(default)]
    pub extra_args: Vec<String>,
}

fn default_backend() -> String {
    "sybase_isql".to_owned()
}

fn default_isql_path() -> String {
    "isql".to_owned()
}

fn default_catalog_ttl_hours() -> u64 {
    24
}

impl AppConfig {
    pub fn load() -> Result<(Self, PathBuf)> {
        let candidates = candidate_paths();
        let Some(path) = candidates.iter().find(|path| path.is_file()).cloned() else {
            let paths = candidates
                .iter()
                .map(|path| format!("  - {}", path.display()))
                .collect::<Vec<_>>()
                .join("\n");
            bail!(
                "No se encontró connections.toml. Rutas revisadas:\n{paths}\n\nCopia connections.example.toml y configura al menos una conexión."
            );
        };

        let raw = fs::read_to_string(&path)
            .with_context(|| format!("No se pudo leer {}", path.display()))?;
        let config: AppConfig = toml::from_str(&raw)
            .with_context(|| format!("Configuración TOML inválida en {}", path.display()))?;
        config.validate()?;
        Ok((config, path))
    }

    pub fn validate(&self) -> Result<()> {
        if self.connections.is_empty() {
            bail!("La configuración debe contener al menos una conexión");
        }

        for connection in &self.connections {
            if connection.name.trim().is_empty() {
                bail!("Una conexión tiene el campo name vacío");
            }
            if connection.backend != "sybase_isql" {
                bail!(
                    "Backend '{}' no soportado todavía en la conexión '{}'",
                    connection.backend,
                    connection.name
                );
            }
            if connection.userstore_key.is_none()
                && (connection.username.is_none() || connection.password_env.is_none())
            {
                bail!(
                    "La conexión '{}' debe usar userstore_key o username + password_env",
                    connection.name
                );
            }
        }
        Ok(())
    }
}

impl ConnectionProfile {
    pub fn initial_database(&self) -> &str {
        self.database.as_deref().unwrap_or("master")
    }

    pub fn password(&self) -> Result<Option<String>> {
        let Some(variable) = &self.password_env else {
            return Ok(None);
        };
        let password = env::var(variable).with_context(|| {
            format!(
                "La variable de entorno '{}' no existe para la conexión '{}'",
                variable, self.name
            )
        })?;
        Ok(Some(password))
    }
}

fn candidate_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(path) = env::var("ASE_TUI_CONFIG") {
        paths.push(PathBuf::from(path));
    }
    paths.push(PathBuf::from("connections.toml"));
    if let Some(config_dir) = dirs::config_dir() {
        paths.push(config_dir.join("ase-tui").join("connections.toml"));
    }
    paths
}
