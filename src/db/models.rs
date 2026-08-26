use std::{error::Error, fmt};

use ratatui::widgets::TableState;

use super::query::PageCursor;

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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TableIdentifier {
    pub schema: String,
    pub name: String,
}

impl TableIdentifier {
    pub fn new(schema: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            schema: schema.into(),
            name: name.into(),
        }
    }

    pub fn qualified_name(&self) -> String {
        format!("{}.{}", self.schema, self.name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnMetadata {
    pub name: String,
    pub data_type: String,
    pub length: Option<u32>,
    pub precision: Option<u32>,
    pub scale: Option<u32>,
    pub nullable: bool,
    pub ordinal_position: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexMetadata {
    pub name: String,
    pub columns: Vec<String>,
    pub is_unique: bool,
    pub is_primary: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableMetadata {
    pub identifier: TableIdentifier,
    pub columns: Vec<ColumnMetadata>,
    pub indexes: Vec<IndexMetadata>,
}

impl TableMetadata {
    pub fn new(
        identifier: TableIdentifier,
        columns: Vec<ColumnMetadata>,
        indexes: Vec<IndexMetadata>,
    ) -> Self {
        Self {
            identifier,
            columns,
            indexes,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TablePage {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub next_cursor: Option<PageCursor>,
    pub has_more: bool,
    pub total_rows: Option<u64>,
}

impl TablePage {
    pub fn new(
        columns: Vec<String>,
        rows: Vec<Vec<String>>,
        next_cursor: Option<PageCursor>,
        has_more: bool,
        total_rows: Option<u64>,
    ) -> Result<Self, TablePageError> {
        for (row_index, row) in rows.iter().enumerate() {
            if row.len() != columns.len() {
                return Err(TablePageError::RowWidthMismatch {
                    row_index,
                    expected: columns.len(),
                    actual: row.len(),
                });
            }
        }

        Ok(Self {
            columns,
            rows,
            next_cursor,
            has_more,
            total_rows,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TablePageError {
    RowWidthMismatch {
        row_index: usize,
        expected: usize,
        actual: usize,
    },
}

impl fmt::Display for TablePageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RowWidthMismatch {
                row_index,
                expected,
                actual,
            } => write!(
                f,
                "row {row_index} has {actual} values, expected {expected}"
            ),
        }
    }
}

impl Error for TablePageError {}

#[derive(Debug, Clone)]
pub struct SqlOutput {
    pub stdout: String,
    pub stderr: String,
    pub success: bool,
    pub elapsed_ms: u64,
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

#[cfg(test)]
mod tests {
    use super::{TableIdentifier, TablePage, TablePageError};

    #[test]
    fn table_identifier_builds_a_qualified_name() {
        let identifier = TableIdentifier::new("dbo", "orders");

        assert_eq!(identifier.qualified_name(), "dbo.orders");
    }

    #[test]
    fn table_page_rejects_rows_with_a_different_width() {
        let error = TablePage::new(
            vec!["id".to_owned(), "name".to_owned()],
            vec![vec!["1".to_owned()]],
            None,
            false,
            None,
        )
        .expect_err("inconsistent rows must be rejected");

        assert_eq!(
            error,
            TablePageError::RowWidthMismatch {
                row_index: 0,
                expected: 2,
                actual: 1,
            }
        );
    }
}
