use std::fmt;

use ratatui::widgets::TableState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectKind {
    Table,
    Procedure,
    Function,
    View,
}

impl ObjectKind {
    pub const ALL: [Self; 4] = [Self::Table, Self::Procedure, Self::Function, Self::View];

    pub fn sysobjects_type(self) -> &'static str {
        match self {
            Self::Table => "U",
            Self::Procedure => "P",
            Self::Function => "SF",
            Self::View => "V",
        }
    }

    pub fn editable(self) -> bool {
        return matches!(self, Self::Procedure | Self::Function | Self::View);
    }
}

impl fmt::Display for ObjectKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::Table => "Tablas",
            Self::Procedure => "Procedimientos",
            Self::Function => "Funciones",
            Self::View => "Vistas",
        };
        return write!(f, "{text}");
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbObject {
    pub owner: String,
    pub name: String,
    pub kind: ObjectKind,
}

impl DbObject {
    pub fn qualified_name(&self) -> String {
        return format!("{}.{}", self.owner, self.name);
    }
}

#[derive(Debug, Clone)]
pub struct SqlOutput {
    pub stdout: String,
    pub stderr: String,
    pub success: bool,
}

impl SqlOutput {
    pub fn combined(&self) -> String {
        match (self.stdout.trim().is_empty(), self.stderr.trim().is_empty()) {
            (false, false) => format!("{}\n\nSTDERR:\n{}", self.stdout, self.stderr),
            (false, true) => self.stdout.clone(),
            (true, false) => self.stderr.clone(),
            (true, true) => "Comando ejecutado sin salida.".to_owned(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct TablePreview {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
}
