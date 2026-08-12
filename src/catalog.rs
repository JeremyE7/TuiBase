use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::{
    config::{AppConfig, ConnectionProfile},
    db::models::{DbObject, ObjectKind},
};

pub const CACHE_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogCache {
    pub version: u32,
    #[serde(default)]
    pub connections: Vec<CachedConnection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedConnection {
    pub connection_key: String,
    pub connection_name: String,
    pub refreshed_at: u64,
    #[serde(default)]
    pub databases: Vec<CachedDatabase>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedDatabase {
    pub name: String,
    #[serde(default)]
    pub objects: Vec<CachedObject>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedObject {
    pub owner: String,
    pub name: String,
    pub kind: CachedObjectKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CachedObjectKind {
    Table,
    Procedure,
    Function,
}

#[derive(Debug, Clone)]
pub struct CatalogEntry {
    pub connection_key: String,
    pub connection_name: String,
    pub database: String,
    pub owner: Option<String>,
    pub name: String,
    pub kind: CatalogEntryKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogEntryKind {
    Database,
    Table,
    Procedure,
    Function,
}

impl Default for CatalogCache {
    fn default() -> Self {
        Self {
            version: CACHE_VERSION,
            connections: Vec::new(),
        }
    }
}

impl CatalogCache {
    pub fn load() -> Result<Self> {
        let path = cache_path()?;
        if !path.is_file() {
            return Ok(Self::default());
        }

        let raw = fs::read_to_string(&path)
            .with_context(|| format!("No se pudo leer el catálogo {}", path.display()))?;
        let cache: Self = toml::from_str(&raw)
            .with_context(|| format!("Catálogo inválido en {}", path.display()))?;
        validate_cache(&cache)?;

        Ok(cache)
    }

    pub fn save(&self) -> Result<()> {
        let path = cache_path()?;
        let parent = path
            .parent()
            .context("La ruta del catálogo no tiene directorio padre")?;
        fs::create_dir_all(parent)
            .with_context(|| format!("No se pudo crear {}", parent.display()))?;

        let raw = toml::to_string_pretty(self).context("No se pudo serializar el catálogo")?;
        fs::write(&path, raw)
            .with_context(|| format!("No se pudo guardar el catálogo {}", path.display()))?;
        Ok(())
    }

    pub fn needs_refresh(&self, config: &AppConfig) -> bool {
        config
            .connections
            .iter()
            .any(|profile| self.needs_refresh_for(profile, config.catalog_ttl_hours))
    }

    pub fn needs_refresh_for(&self, profile: &ConnectionProfile, ttl_hours: u64) -> bool {
        let ttl = ttl_hours.saturating_mul(60 * 60);
        let now = now_secs();

        self.connection(&connection_key(profile))
            .is_none_or(|cached| now.saturating_sub(cached.refreshed_at) >= ttl)
    }

    pub fn connection(&self, key: &str) -> Option<&CachedConnection> {
        self.connections
            .iter()
            .find(|connection| connection.connection_key == key)
    }

    pub fn upsert(&mut self, connection: CachedConnection) {
        if let Some(existing) = self
            .connections
            .iter_mut()
            .find(|existing| existing.connection_key == connection.connection_key)
        {
            *existing = connection;
        } else {
            self.connections.push(connection);
        }
    }

    pub fn entries(&self) -> Vec<CatalogEntry> {
        self.connections
            .iter()
            .flat_map(|connection| {
                connection.databases.iter().flat_map(move |database| {
                    std::iter::once(CatalogEntry {
                        connection_key: connection.connection_key.clone(),
                        connection_name: connection.connection_name.clone(),
                        database: database.name.clone(),
                        owner: None,
                        name: database.name.clone(),
                        kind: CatalogEntryKind::Database,
                    })
                    .chain(database.objects.iter().map(move |object| {
                        CatalogEntry {
                            connection_key: connection.connection_key.clone(),
                            connection_name: connection.connection_name.clone(),
                            database: database.name.clone(),
                            owner: Some(object.owner.clone()),
                            name: object.name.clone(),
                            kind: CatalogEntryKind::from_cached_kind(object.kind),
                        }
                    }))
                })
            })
            .collect()
    }
}

impl CachedConnection {
    pub fn from_objects(profile: &ConnectionProfile, databases: Vec<CachedDatabase>) -> Self {
        Self {
            connection_key: connection_key(profile),
            connection_name: profile.name.clone(),
            refreshed_at: now_secs(),
            databases,
        }
    }
}

impl CachedObject {
    pub fn from_db_object(object: &DbObject) -> Self {
        Self {
            owner: object.owner.clone(),
            name: object.name.clone(),
            kind: CachedObjectKind::from_object_kind(object.kind)
                .expect("catalog only accepts cacheable object kinds"),
        }
    }

    pub fn to_db_object(&self) -> DbObject {
        DbObject {
            owner: self.owner.clone(),
            name: self.name.clone(),
            kind: self.kind.object_kind(),
        }
    }
}

impl CatalogEntry {
    pub fn path(&self) -> String {
        match self.kind {
            CatalogEntryKind::Database => format!("/{}/", self.database),
            _ => format!(
                "/{}/{}/{}.{}",
                self.database,
                self.kind.path_segment(),
                self.owner.as_deref().unwrap_or("dbo"),
                self.name
            ),
        }
    }

    pub fn display_line(&self) -> String {
        format!(
            "[{}] {} [{}]",
            self.connection_name,
            self.path(),
            self.kind.label()
        )
    }

    pub fn search_text(&self) -> String {
        format!(
            "{} {} {} {} {}",
            self.connection_name,
            self.database,
            self.owner.as_deref().unwrap_or_default(),
            self.name,
            self.kind.label()
        )
    }

    pub fn object_kind(&self) -> Option<ObjectKind> {
        match self.kind {
            CatalogEntryKind::Database => None,
            CatalogEntryKind::Table => Some(ObjectKind::Table),
            CatalogEntryKind::Procedure => Some(ObjectKind::Procedure),
            CatalogEntryKind::Function => Some(ObjectKind::Function),
        }
    }
}

impl CachedObjectKind {
    pub fn from_object_kind(kind: ObjectKind) -> Option<Self> {
        match kind {
            ObjectKind::Table => Some(Self::Table),
            ObjectKind::Procedure => Some(Self::Procedure),
            ObjectKind::Function => Some(Self::Function),
            ObjectKind::View => None,
        }
    }

    pub fn object_kind(self) -> ObjectKind {
        match self {
            Self::Table => ObjectKind::Table,
            Self::Procedure => ObjectKind::Procedure,
            Self::Function => ObjectKind::Function,
        }
    }

    pub fn path_segment(self) -> &'static str {
        match self {
            Self::Table => "tablas",
            Self::Procedure => "procedimientos",
            Self::Function => "funciones",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Table => "Tabla",
            Self::Procedure => "Procedimiento",
            Self::Function => "Función",
        }
    }
}

impl CatalogEntryKind {
    fn from_cached_kind(kind: CachedObjectKind) -> Self {
        match kind {
            CachedObjectKind::Table => Self::Table,
            CachedObjectKind::Procedure => Self::Procedure,
            CachedObjectKind::Function => Self::Function,
        }
    }

    fn path_segment(self) -> &'static str {
        match self {
            Self::Database => "",
            Self::Table => "tablas",
            Self::Procedure => "procedimientos",
            Self::Function => "funciones",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Database => "Base de datos",
            Self::Table => "Tabla",
            Self::Procedure => "Procedimiento",
            Self::Function => "Función",
        }
    }
}

pub fn connection_key(profile: &ConnectionProfile) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for value in [
        profile.name.as_str(),
        profile.backend.as_str(),
        profile.isql_path.as_str(),
        profile.server.as_deref().unwrap_or_default(),
        profile.username.as_deref().unwrap_or_default(),
        profile.database.as_deref().unwrap_or_default(),
        profile.charset.as_deref().unwrap_or_default(),
    ] {
        for byte in value.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        hash ^= 0xff;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    for argument in &profile.extra_args {
        for byte in argument.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        hash ^= 0xff;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

pub fn cache_path() -> Result<PathBuf> {
    let directory = dirs::cache_dir().context("No se pudo determinar el directorio de cache")?;
    Ok(directory.join("ase-tui").join("catalog.toml"))
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn cacheable_kinds() -> [ObjectKind; 3] {
    [
        ObjectKind::Table,
        ObjectKind::Procedure,
        ObjectKind::Function,
    ]
}

pub fn validate_cache(cache: &CatalogCache) -> Result<()> {
    if cache.version != CACHE_VERSION {
        bail!(
            "Versión de catálogo no compatible: {} (se esperaba {})",
            cache.version,
            CACHE_VERSION
        );
    }
    Ok(())
}
