use std::{error::Error, fmt};

pub const DEFAULT_PAGE_SIZE: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryError {
    InvalidPageLimit,
}

impl fmt::Display for QueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPageLimit => f.write_str("page limit must be greater than zero"),
        }
    }
}

impl Error for QueryError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageCursor {
    Offset(u64),
    Keyset(Vec<String>),
}

impl PageCursor {
    pub fn offset(value: u64) -> Self {
        Self::Offset(value)
    }

    pub fn keyset(values: Vec<String>) -> Self {
        Self::Keyset(values)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageRequest {
    pub limit: usize,
    pub cursor: Option<PageCursor>,
}

impl PageRequest {
    pub fn new(limit: usize) -> Result<Self, QueryError> {
        if limit == 0 {
            return Err(QueryError::InvalidPageLimit);
        }

        Ok(Self {
            limit,
            cursor: None,
        })
    }

    pub fn after(limit: usize, cursor: PageCursor) -> Result<Self, QueryError> {
        let mut request = Self::new(limit)?;
        request.cursor = Some(cursor);
        Ok(request)
    }
}

impl Default for PageRequest {
    fn default() -> Self {
        Self {
            limit: DEFAULT_PAGE_SIZE,
            cursor: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    Ascending,
    Descending,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SortSpec {
    pub column: String,
    pub direction: SortDirection,
}

impl SortSpec {
    pub fn ascending(column: impl Into<String>) -> Self {
        Self {
            column: column.into(),
            direction: SortDirection::Ascending,
        }
    }

    pub fn descending(column: impl Into<String>) -> Self {
        Self {
            column: column.into(),
            direction: SortDirection::Descending,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterOperator {
    Equals,
    NotEquals,
    Contains,
    StartsWith,
    EndsWith,
    GreaterThan,
    GreaterThanOrEqual,
    LessThan,
    LessThanOrEqual,
    IsNull,
    IsNotNull,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterSpec {
    pub column: String,
    pub operator: FilterOperator,
    pub value: Option<String>,
}

impl FilterSpec {
    pub fn new(
        column: impl Into<String>,
        operator: FilterOperator,
        value: Option<impl Into<String>>,
    ) -> Self {
        Self {
            column: column.into(),
            operator,
            value: value.map(Into::into),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableQuery {
    pub page: PageRequest,
    pub sort: Vec<SortSpec>,
    pub filters: Vec<FilterSpec>,
}

impl TableQuery {
    pub fn new(page: PageRequest) -> Self {
        Self {
            page,
            sort: Vec::new(),
            filters: Vec::new(),
        }
    }
}

impl Default for TableQuery {
    fn default() -> Self {
        Self::new(PageRequest::default())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_PAGE_SIZE, FilterOperator, FilterSpec, PageCursor, PageRequest, SortDirection,
        SortSpec, TableQuery,
    };

    #[test]
    fn page_request_rejects_a_zero_limit() {
        assert!(PageRequest::new(0).is_err());
    }

    #[test]
    fn page_request_can_use_a_keyset_cursor() {
        let request = PageRequest::after(25, PageCursor::keyset(vec!["42".to_owned()]))
            .expect("positive limits are valid");

        assert_eq!(request.limit, 25);
        assert_eq!(
            request.cursor,
            Some(PageCursor::Keyset(vec!["42".to_owned()]))
        );
    }

    #[test]
    fn default_query_starts_with_the_default_page_size() {
        let query = TableQuery::default();

        assert_eq!(query.page.limit, DEFAULT_PAGE_SIZE);
        assert!(query.sort.is_empty());
        assert!(query.filters.is_empty());
    }

    #[test]
    fn specs_keep_backend_neutral_intent() {
        let sort = SortSpec::descending("created_at");
        let filter = FilterSpec::new("status", FilterOperator::Equals, Some("active"));

        assert_eq!(sort.direction, SortDirection::Descending);
        assert_eq!(filter.value.as_deref(), Some("active"));
    }
}
