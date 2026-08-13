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
    Like,
    NotLike,
    IsNull,
    IsNotNull,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilterParseError {
    Empty,
    UnclosedQuote,
    MissingColumn,
    UnknownColumn(String),
    MissingOperator(String),
    MissingValue(String),
    UnexpectedInput(String),
}

impl fmt::Display for FilterParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("el filtro está vacío"),
            Self::UnclosedQuote => f.write_str("hay una comilla sin cerrar"),
            Self::MissingColumn => f.write_str("falta el nombre de la columna"),
            Self::UnknownColumn(column) => write!(f, "columna desconocida: {column}"),
            Self::MissingOperator(condition) => {
                write!(f, "falta un operador en: {condition}")
            }
            Self::MissingValue(operator) => write!(f, "falta un valor después de {operator}"),
            Self::UnexpectedInput(input) => write!(f, "entrada inesperada: {input}"),
        }
    }
}

impl Error for FilterParseError {}

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

pub fn parse_filter_expression(
    expression: &str,
    columns: &[String],
) -> Result<Vec<FilterSpec>, FilterParseError> {
    let expression = expression.trim();
    if expression.is_empty() {
        return Err(FilterParseError::Empty);
    }

    split_filter_conditions(expression)?
        .into_iter()
        .map(|condition| parse_filter_condition(condition, columns))
        .collect()
}

fn split_filter_conditions(expression: &str) -> Result<Vec<&str>, FilterParseError> {
    let chars = expression.char_indices().collect::<Vec<_>>();
    let mut conditions = Vec::new();
    let mut condition_start = 0;
    let mut quote = None;
    let mut index = 0;

    while index < chars.len() {
        let (byte_index, character) = chars[index];
        if let Some(quote_character) = quote {
            if character == quote_character {
                if chars
                    .get(index + 1)
                    .is_some_and(|(_, next)| *next == quote_character)
                {
                    index += 2;
                    continue;
                }
                quote = None;
            }
            index += 1;
            continue;
        }

        if character == '\'' || character == '"' {
            quote = Some(character);
            index += 1;
            continue;
        }

        if expression[byte_index..]
            .get(..3)
            .is_some_and(|keyword| keyword.eq_ignore_ascii_case("AND"))
            && is_keyword_boundary(expression, byte_index, byte_index + 3)
        {
            let condition = expression[condition_start..byte_index].trim();
            if condition.is_empty() {
                return Err(FilterParseError::UnexpectedInput("AND".to_owned()));
            }
            conditions.push(condition);
            condition_start = byte_index + 3;
            index += 3;
            continue;
        }

        index += 1;
    }

    if quote.is_some() {
        return Err(FilterParseError::UnclosedQuote);
    }

    let last = expression[condition_start..].trim();
    if last.is_empty() {
        return Err(FilterParseError::UnexpectedInput("AND".to_owned()));
    }
    conditions.push(last);
    Ok(conditions)
}

fn is_keyword_boundary(expression: &str, start: usize, end: usize) -> bool {
    let before_is_identifier = expression[..start]
        .chars()
        .next_back()
        .is_some_and(is_identifier_character);
    let after_is_identifier = expression[end..]
        .chars()
        .next()
        .is_some_and(is_identifier_character);
    !before_is_identifier && !after_is_identifier
}

fn is_identifier_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_'
}

fn parse_filter_condition(
    condition: &str,
    columns: &[String],
) -> Result<FilterSpec, FilterParseError> {
    let condition = condition.trim();
    let (column_text, remainder) = parse_column(condition)?;
    let column = columns
        .iter()
        .find(|candidate| candidate.eq_ignore_ascii_case(&column_text))
        .cloned()
        .ok_or_else(|| FilterParseError::UnknownColumn(column_text.clone()))?;
    let remainder = remainder.trim_start();

    if let Some(value) = consume_keyword(remainder, "IS NOT NULL") {
        if !value.trim().is_empty() {
            return Err(FilterParseError::UnexpectedInput(value.trim().to_owned()));
        }
        return Ok(FilterSpec::new(
            column,
            FilterOperator::IsNotNull,
            None::<String>,
        ));
    }
    if let Some(value) = consume_keyword(remainder, "IS NULL") {
        if !value.trim().is_empty() {
            return Err(FilterParseError::UnexpectedInput(value.trim().to_owned()));
        }
        return Ok(FilterSpec::new(
            column,
            FilterOperator::IsNull,
            None::<String>,
        ));
    }

    let operators = [
        (">=", FilterOperator::GreaterThanOrEqual),
        ("<=", FilterOperator::LessThanOrEqual),
        ("!=", FilterOperator::NotEquals),
        ("<>", FilterOperator::NotEquals),
        ("=", FilterOperator::Equals),
        (">", FilterOperator::GreaterThan),
        ("<", FilterOperator::LessThan),
    ];
    for (operator_text, operator) in operators {
        if let Some(value) = remainder.strip_prefix(operator_text) {
            return parse_value_filter(column, operator, operator_text, value);
        }
    }
    if let Some(value) = consume_keyword(remainder, "NOT LIKE") {
        return parse_value_filter(column, FilterOperator::NotLike, "NOT LIKE", value);
    }
    if let Some(value) = consume_keyword(remainder, "LIKE") {
        return parse_value_filter(column, FilterOperator::Like, "LIKE", value);
    }

    if remainder.is_empty() {
        return Err(FilterParseError::MissingOperator(condition.to_owned()));
    }
    Err(FilterParseError::MissingOperator(condition.to_owned()))
}

fn parse_column(condition: &str) -> Result<(String, &str), FilterParseError> {
    if condition.is_empty() {
        return Err(FilterParseError::MissingColumn);
    }

    if condition.starts_with('"') {
        let chars = condition.char_indices().collect::<Vec<_>>();
        let mut index = 1;
        while index < chars.len() {
            if chars[index].1 == '"' {
                if chars.get(index + 1).is_some_and(|(_, next)| *next == '"') {
                    index += 2;
                    continue;
                }
                let column = condition[1..chars[index].0].replace("\"\"", "\"");
                return Ok((column, &condition[chars[index].0 + 1..]));
            }
            index += 1;
        }
        return Err(FilterParseError::UnclosedQuote);
    }

    let end = condition
        .char_indices()
        .find(|(_, character)| {
            character.is_whitespace() || matches!(character, '=' | '!' | '<' | '>')
        })
        .map_or(condition.len(), |(index, _)| index);
    let column = condition[..end].trim();
    if column.is_empty() {
        return Err(FilterParseError::MissingColumn);
    }
    Ok((column.to_owned(), &condition[end..]))
}

fn consume_keyword<'a>(input: &'a str, keyword: &str) -> Option<&'a str> {
    if input.len() < keyword.len() || !input[..keyword.len()].eq_ignore_ascii_case(keyword) {
        return None;
    }
    let remainder = &input[keyword.len()..];
    if remainder
        .chars()
        .next()
        .is_some_and(is_identifier_character)
    {
        return None;
    }
    Some(remainder)
}

fn parse_value_filter(
    column: String,
    operator: FilterOperator,
    operator_text: &str,
    value: &str,
) -> Result<FilterSpec, FilterParseError> {
    let value = parse_filter_value(value.trim())
        .ok_or_else(|| FilterParseError::MissingValue(operator_text.to_owned()))?;
    Ok(FilterSpec::new(column, operator, Some(value)))
}

fn parse_filter_value(value: &str) -> Option<String> {
    if value.is_empty() {
        return None;
    }
    let first = value.chars().next()?;
    if first != '\'' && first != '"' {
        if value.contains(['\'', '"']) || value.chars().any(char::is_whitespace) {
            return None;
        }
        return Some(value.to_owned());
    }

    let chars = value.char_indices().collect::<Vec<_>>();
    let mut index = 1;
    while index < chars.len() {
        if chars[index].1 == first {
            if chars.get(index + 1).is_some_and(|(_, next)| *next == first) {
                index += 2;
                continue;
            }
            if !value[chars[index].0 + 1..].trim().is_empty() {
                return None;
            }
            return Some(
                value[1..chars[index].0].replace(&format!("{first}{first}"), &first.to_string()),
            );
        }
        index += 1;
    }
    None
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
        SortSpec, TableQuery, parse_filter_expression,
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

    #[test]
    fn parses_multiple_conditions_with_quoted_values() {
        let filters = parse_filter_expression(
            "status = 'active' AND total >= 500 AND customer LIKE '%ACME%'",
            &[
                "status".to_owned(),
                "total".to_owned(),
                "customer".to_owned(),
            ],
        )
        .expect("valid filter expression");

        assert_eq!(filters.len(), 3);
        assert_eq!(filters[0].operator, FilterOperator::Equals);
        assert_eq!(filters[0].value.as_deref(), Some("active"));
        assert_eq!(filters[1].operator, FilterOperator::GreaterThanOrEqual);
        assert_eq!(filters[2].operator, FilterOperator::Like);
        assert_eq!(filters[2].value.as_deref(), Some("%ACME%"));
    }

    #[test]
    fn parses_null_and_not_like_conditions() {
        let filters = parse_filter_expression(
            "deleted_at IS NULL AND name NOT LIKE 'test%'",
            &["deleted_at".to_owned(), "name".to_owned()],
        )
        .expect("valid filter expression");

        assert_eq!(filters[0].operator, FilterOperator::IsNull);
        assert_eq!(filters[0].value, None);
        assert_eq!(filters[1].operator, FilterOperator::NotLike);
        assert_eq!(filters[1].value.as_deref(), Some("test%"));
    }

    #[test]
    fn rejects_unknown_columns_and_unclosed_quotes() {
        assert!(parse_filter_expression("missing = 1", &["id".to_owned()]).is_err());
        assert!(parse_filter_expression("name = 'Ada", &["name".to_owned()]).is_err());
    }
}
