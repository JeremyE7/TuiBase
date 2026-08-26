use std::{fs, path::PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::db::models::{DbObject, ObjectKind};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Tab {
    pub id: u64,
    pub database: String,
    pub owner: String,
    pub name: String,
    pub kind: TabKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TabKind {
    Table,
    Procedure,
    Function,
    View,
}

impl TabKind {
    fn from_object_kind(kind: ObjectKind) -> Self {
        match kind {
            ObjectKind::Table => Self::Table,
            ObjectKind::Procedure => Self::Procedure,
            ObjectKind::Function => Self::Function,
            ObjectKind::View => Self::View,
        }
    }

    fn object_kind(self) -> ObjectKind {
        match self {
            Self::Table => ObjectKind::Table,
            Self::Procedure => ObjectKind::Procedure,
            Self::Function => ObjectKind::Function,
            Self::View => ObjectKind::View,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Table => "Tabla",
            Self::Procedure => "Proc",
            Self::Function => "Func",
            Self::View => "Vista",
        }
    }
}

impl Tab {
    pub fn new(id: u64, database: String, object: &DbObject) -> Self {
        Self {
            id,
            database,
            owner: object.owner.clone(),
            name: object.name.clone(),
            kind: TabKind::from_object_kind(object.kind),
        }
    }

    pub fn to_object(&self) -> DbObject {
        DbObject {
            owner: self.owner.clone(),
            name: self.name.clone(),
            kind: self.kind.object_kind(),
        }
    }

    pub fn title(&self) -> String {
        format!("{} · {}.{}", self.database, self.owner, self.name)
    }

    pub fn short_title(&self) -> String {
        format!("{}.{}", self.owner, self.name)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TabsState {
    pub tabs: Vec<Tab>,
    pub active: usize,
    pub next_id: u64,
}

impl Default for TabsState {
    fn default() -> Self {
        Self {
            tabs: Vec::new(),
            active: 0,
            next_id: 1,
        }
    }
}

impl TabsState {
    pub fn load() -> Result<Self> {
        let path = tabs_path()?;
        if !path.is_file() {
            return Ok(Self::default());
        }
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("No se pudo leer tabs {}", path.display()))?;
        let mut state: Self = toml::from_str(&raw)
            .with_context(|| format!("Tabs inválido en {}", path.display()))?;
        if state.tabs.is_empty() {
            state.active = 0;
        } else {
            state.active = state.active.min(state.tabs.len() - 1);
        }
        let max_id = state.tabs.iter().map(|t| t.id).max().unwrap_or(0);
        state.next_id = state.next_id.max(max_id + 1);
        Ok(state)
    }

    pub fn save(&self) -> Result<()> {
        let path = tabs_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("No se pudo crear {}", parent.display()))?;
        }
        let raw = toml::to_string_pretty(self).context("No se pudo serializar tabs")?;
        fs::write(&path, raw)
            .with_context(|| format!("No se pudo guardar tabs {}", path.display()))?;
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    pub fn active(&self) -> Option<&Tab> {
        self.tabs.get(self.active)
    }

    pub fn push(&mut self, database: String, object: &DbObject) -> u64 {
        // avoid duplicate consecutive same object in same db
        if let Some(active) = self.active() {
            if active.database == database
                && active.owner == object.owner
                && active.name == object.name
                && active.kind == TabKind::from_object_kind(object.kind)
            {
                return active.id;
            }
        }
        let id = self.next_id;
        self.next_id += 1;
        let tab = Tab::new(id, database, object);
        self.tabs.push(tab);
        self.active = self.tabs.len() - 1;
        id
    }

    pub fn close(&mut self, index: usize) -> bool {
        if self.tabs.is_empty() || index >= self.tabs.len() {
            return false;
        }
        self.tabs.remove(index);
        if self.tabs.is_empty() {
            self.active = 0;
        } else if self.active >= self.tabs.len() {
            self.active = self.tabs.len() - 1;
        } else if index < self.active {
            self.active -= 1;
        }
        true
    }

    pub fn close_active(&mut self) -> bool {
        if self.tabs.is_empty() {
            return false;
        }
        self.close(self.active)
    }

    pub fn switch_to(&mut self, index: usize) -> bool {
        if index < self.tabs.len() {
            self.active = index;
            return true;
        }
        false
    }

    pub fn next(&mut self) {
        if self.tabs.is_empty() {
            return;
        }
        self.active = (self.active + 1) % self.tabs.len();
    }

    pub fn prev(&mut self) {
        if self.tabs.is_empty() {
            return;
        }
        if self.active == 0 {
            self.active = self.tabs.len() - 1;
        } else {
            self.active -= 1;
        }
    }
}

fn tabs_path() -> Result<PathBuf> {
    let dir = dirs::cache_dir().context("No se pudo determinar el directorio de cache")?;
    Ok(dir.join("ase-tui").join("tabs.toml"))
}

#[cfg(test)]
mod tests {
    use super::{TabKind, TabsState};
    use crate::db::models::{DbObject, ObjectKind};

    fn obj(owner: &str, name: &str, kind: ObjectKind) -> DbObject {
        DbObject {
            owner: owner.to_owned(),
            name: name.to_owned(),
            kind,
        }
    }

    #[test]
    fn push_and_switch() {
        let mut s = TabsState::default();
        s.push("db1".to_owned(), &obj("dbo", "t1", ObjectKind::Table));
        s.push("db1".to_owned(), &obj("dbo", "p1", ObjectKind::Procedure));
        assert_eq!(s.len(), 2);
        assert_eq!(s.active, 1);
        s.prev();
        assert_eq!(s.active, 0);
        s.next();
        assert_eq!(s.active, 1);
    }

    #[test]
    fn close_adjusts_active() {
        let mut s = TabsState::default();
        s.push("db".to_owned(), &obj("dbo", "a", ObjectKind::Table));
        s.push("db".to_owned(), &obj("dbo", "b", ObjectKind::Table));
        s.push("db".to_owned(), &obj("dbo", "c", ObjectKind::Table));
        s.active = 1;
        s.close(1);
        assert_eq!(s.len(), 2);
        assert_eq!(s.active, 1);
        assert_eq!(s.tabs[1].name, "c");
    }

    #[test]
    fn tab_kind_roundtrip() {
        let mut s = TabsState::default();
        s.push("db".to_owned(), &obj("dbo", "v1", ObjectKind::View));
        let raw = toml::to_string(&s).unwrap();
        let restored: TabsState = toml::from_str(&raw).unwrap();
        assert_eq!(restored.tabs[0].kind, TabKind::View);
    }
}
