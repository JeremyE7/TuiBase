use std::{collections::BTreeMap, fs, path::PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TablePreferences {
    #[serde(default)]
    tables: BTreeMap<String, TablePreference>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TablePreference {
    #[serde(default)]
    pub pinned_columns: Vec<String>,
}

impl TablePreferences {
    pub fn load() -> Result<Self> {
        let path = preferences_path()?;
        if !path.is_file() {
            return Ok(Self::default());
        }

        let raw = fs::read_to_string(&path)
            .with_context(|| format!("No se pudieron leer las preferencias {}", path.display()))?;
        toml::from_str(&raw)
            .with_context(|| format!("Preferencias inválidas en {}", path.display()))
    }

    pub fn save(&self) -> Result<()> {
        let path = preferences_path()?;
        let parent = path
            .parent()
            .context("La ruta de preferencias no tiene directorio padre")?;
        fs::create_dir_all(parent)
            .with_context(|| format!("No se pudo crear {}", parent.display()))?;

        let raw =
            toml::to_string_pretty(self).context("No se pudieron serializar las preferencias")?;
        fs::write(&path, raw)
            .with_context(|| format!("No se pudieron guardar las preferencias {}", path.display()))
    }

    pub fn pinned_columns(&self, key: &str) -> &[String] {
        self.tables
            .get(key)
            .map_or(&[], |preference| preference.pinned_columns.as_slice())
    }

    pub fn set_pinned_columns(&mut self, key: impl Into<String>, pinned_columns: Vec<String>) {
        let key = key.into();
        if pinned_columns.is_empty() {
            self.tables.remove(&key);
        } else {
            self.tables.entry(key).or_default().pinned_columns = pinned_columns;
        }
    }
}

pub fn table_preference_key(
    connection_key: &str,
    database: &str,
    owner: &str,
    table: &str,
) -> String {
    format!("{connection_key}:{database}:{owner}:{table}")
}

pub fn preferences_path() -> Result<PathBuf> {
    let directory =
        dirs::config_dir().context("No se pudo determinar el directorio de configuración")?;
    Ok(directory.join("ase-tui").join("table-preferences.toml"))
}

#[cfg(test)]
mod tests {
    use super::{TablePreferences, table_preference_key};

    #[test]
    fn serializes_pinned_columns_per_table() {
        let mut preferences = TablePreferences::default();
        preferences.set_pinned_columns(
            "connection:database:dbo:orders",
            vec!["id".to_owned(), "status".to_owned()],
        );

        let raw = toml::to_string(&preferences).expect("preferences serialize");
        let restored: TablePreferences = toml::from_str(&raw).expect("preferences deserialize");

        assert_eq!(
            restored.pinned_columns("connection:database:dbo:orders"),
            ["id", "status"]
        );
    }

    #[test]
    fn empty_pinned_columns_remove_the_table_entry() {
        let mut preferences = TablePreferences::default();
        preferences.set_pinned_columns("orders", vec!["id".to_owned()]);
        preferences.set_pinned_columns("orders", Vec::new());

        assert!(preferences.pinned_columns("orders").is_empty());
    }

    #[test]
    fn preference_key_includes_connection_and_table_identity() {
        assert_eq!(
            table_preference_key("abc", "sales", "dbo", "orders"),
            "abc:sales:dbo:orders"
        );
    }
}
