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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SortParseError {
    Empty,
    UnclosedQuote,
    MissingColumn,
    UnknownColumn(String),
    InvalidDirection(String),
    UnexpectedInput(String),
}

impl fmt::Display for SortParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("el ordenamiento está vacío"),
            Self::UnclosedQuote => f.write_str("hay una comilla sin cerrar"),
            Self::MissingColumn => f.write_str("falta el nombre de la columna"),
            Self::UnknownColumn(column) => write!(f, "columna desconocida: {column}"),
            Self::InvalidDirection(direction) => {
                write!(f, "dirección inválida: {direction}; usa ASC o DESC")
            }
            Self::UnexpectedInput(input) => write!(f, "entrada inesperada: {input}"),
        }
    }
}

impl Error for SortParseError {}

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
    MissingClosingParenthesis,
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
            Self::MissingClosingParenthesis => f.write_str("falta cerrar un paréntesis"),
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilterExpr {
    Condition(FilterSpec),
    And(Vec<FilterExpr>),
    Or(Vec<FilterExpr>),
    Not(Box<FilterExpr>),
}

impl FilterExpr {
    fn combine_and(left: Self, right: Self) -> Self {
        let mut terms = Vec::new();
        match left {
            Self::And(items) => terms.extend(items),
            item => terms.push(item),
        }
        match right {
            Self::And(items) => terms.extend(items),
            item => terms.push(item),
        }
        Self::And(terms)
    }

    fn combine_or(left: Self, right: Self) -> Self {
        let mut terms = Vec::new();
        match left {
            Self::Or(items) => terms.extend(items),
            item => terms.push(item),
        }
        match right {
            Self::Or(items) => terms.extend(items),
            item => terms.push(item),
        }
        Self::Or(terms)
    }
}

pub fn parse_filter_expression(
    expression: &str,
    columns: &[String],
) -> Result<FilterExpr, FilterParseError> {
    FilterParser::new(expression, columns).parse()
}

pub fn parse_sort_expression(
    expression: &str,
    columns: &[String],
) -> Result<Vec<SortSpec>, SortParseError> {
    let expression = expression.trim();
    if expression.is_empty() {
        return Err(SortParseError::Empty);
    }

    split_sort_terms(expression)?
        .into_iter()
        .map(|term| parse_sort_term(term, columns))
        .collect()
}

fn split_sort_terms(expression: &str) -> Result<Vec<&str>, SortParseError> {
    let chars = expression.char_indices().collect::<Vec<_>>();
    let mut terms = Vec::new();
    let mut term_start = 0;
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

        if character == '"' {
            quote = Some(character);
        } else if character == ',' {
            let term = expression[term_start..byte_index].trim();
            if term.is_empty() {
                return Err(SortParseError::UnexpectedInput(
                    "falta una columna después de la coma".to_owned(),
                ));
            }
            terms.push(term);
            term_start = byte_index + character.len_utf8();
        }
        index += 1;
    }

    if quote.is_some() {
        return Err(SortParseError::UnclosedQuote);
    }

    let last = expression[term_start..].trim();
    if last.is_empty() {
        return Err(SortParseError::UnexpectedInput(
            "falta una columna después de la coma".to_owned(),
        ));
    }
    terms.push(last);
    Ok(terms)
}

fn parse_sort_term(term: &str, columns: &[String]) -> Result<SortSpec, SortParseError> {
    let term = term.trim();
    let (column_text, remainder) = parse_column(term).map_err(|error| match error {
        FilterParseError::UnclosedQuote => SortParseError::UnclosedQuote,
        FilterParseError::MissingColumn => SortParseError::MissingColumn,
        other => SortParseError::UnexpectedInput(other.to_string()),
    })?;
    let column = columns
        .iter()
        .find(|candidate| candidate.eq_ignore_ascii_case(&column_text))
        .cloned()
        .ok_or_else(|| SortParseError::UnknownColumn(column_text.clone()))?;
    let remainder = remainder.trim();

    let direction = if remainder.is_empty() {
        SortDirection::Ascending
    } else if let Some(rest) = consume_keyword(remainder, "ASC") {
        if !rest.trim().is_empty() {
            return Err(SortParseError::UnexpectedInput(rest.trim().to_owned()));
        }
        SortDirection::Ascending
    } else if let Some(rest) = consume_keyword(remainder, "DESC") {
        if !rest.trim().is_empty() {
            return Err(SortParseError::UnexpectedInput(rest.trim().to_owned()));
        }
        SortDirection::Descending
    } else {
        return Err(SortParseError::InvalidDirection(remainder.to_owned()));
    };

    Ok(SortSpec { column, direction })
}

struct FilterParser<'a, 'columns> {
    expression: &'a str,
    columns: &'columns [String],
    cursor: usize,
}

impl<'a, 'columns> FilterParser<'a, 'columns> {
    fn new(expression: &'a str, columns: &'columns [String]) -> Self {
        Self {
            expression,
            columns,
            cursor: 0,
        }
    }

    fn parse(mut self) -> Result<FilterExpr, FilterParseError> {
        self.skip_whitespace();
        if self.is_eof() {
            return Err(FilterParseError::Empty);
        }

        let expression = self.parse_or()?;
        self.skip_whitespace();
        if self.is_eof() {
            Ok(expression)
        } else {
            Err(FilterParseError::UnexpectedInput(
                self.expression[self.cursor..].trim().to_owned(),
            ))
        }
    }

    fn parse_or(&mut self) -> Result<FilterExpr, FilterParseError> {
        let mut expression = self.parse_and()?;
        while self.consume_keyword("OR") {
            let right = self.parse_and()?;
            expression = FilterExpr::combine_or(expression, right);
        }
        Ok(expression)
    }

    fn parse_and(&mut self) -> Result<FilterExpr, FilterParseError> {
        let mut expression = self.parse_unary()?;
        while self.consume_keyword("AND") {
            let right = self.parse_unary()?;
            expression = FilterExpr::combine_and(expression, right);
        }
        Ok(expression)
    }

    fn parse_unary(&mut self) -> Result<FilterExpr, FilterParseError> {
        if self.consume_keyword("NOT") {
            return Ok(FilterExpr::Not(Box::new(self.parse_unary()?)));
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<FilterExpr, FilterParseError> {
        self.skip_whitespace();
        if self.consume_char('(') {
            self.skip_whitespace();
            if self.consume_char(')') {
                return Err(FilterParseError::UnexpectedInput("()".to_owned()));
            }
            let expression = self.parse_or()?;
            if !self.consume_char(')') {
                return Err(FilterParseError::MissingClosingParenthesis);
            }
            return Ok(expression);
        }

        let start = self.cursor;
        let end = self.condition_end()?;
        self.cursor = end;
        let condition = self.expression[start..end].trim();
        if condition.is_empty() {
            return Err(FilterParseError::UnexpectedInput(
                self.expression[start..].trim().to_owned(),
            ));
        }
        Ok(FilterExpr::Condition(parse_filter_condition(
            condition,
            self.columns,
        )?))
    }

    fn condition_end(&self) -> Result<usize, FilterParseError> {
        let mut index = self.cursor;
        let mut quote = None;
        while index < self.expression.len() {
            let character = self.expression[index..]
                .chars()
                .next()
                .expect("cursor must point to a character");
            let next_index = index + character.len_utf8();

            if let Some(quote_character) = quote {
                if character == quote_character {
                    if self.expression[next_index..]
                        .chars()
                        .next()
                        .is_some_and(|next| next == quote_character)
                    {
                        index = next_index + quote_character.len_utf8();
                        continue;
                    }
                    quote = None;
                }
                index = next_index;
                continue;
            }

            if character == '\'' || character == '"' {
                quote = Some(character);
                index = next_index;
                continue;
            }
            if matches!(character, '(' | ')')
                || self.keyword_at(index, "AND")
                || self.keyword_at(index, "OR")
            {
                break;
            }
            index = next_index;
        }
        if quote.is_some() {
            Err(FilterParseError::UnclosedQuote)
        } else {
            Ok(index)
        }
    }

    fn consume_keyword(&mut self, keyword: &str) -> bool {
        let original_cursor = self.cursor;
        self.skip_whitespace();
        if self.keyword_at(self.cursor, keyword) {
            self.cursor += keyword.len();
            true
        } else {
            self.cursor = original_cursor;
            false
        }
    }

    fn consume_char(&mut self, expected: char) -> bool {
        let original_cursor = self.cursor;
        self.skip_whitespace();
        let Some(character) = self.expression[self.cursor..].chars().next() else {
            self.cursor = original_cursor;
            return false;
        };
        if character == expected {
            self.cursor += character.len_utf8();
            true
        } else {
            self.cursor = original_cursor;
            false
        }
    }

    fn keyword_at(&self, start: usize, keyword: &str) -> bool {
        self.expression[start..]
            .get(..keyword.len())
            .is_some_and(|candidate| {
                candidate.eq_ignore_ascii_case(keyword)
                    && is_keyword_boundary(self.expression, start, start + keyword.len())
            })
    }

    fn skip_whitespace(&mut self) {
        while let Some(character) = self.expression[self.cursor..].chars().next() {
            if !character.is_whitespace() {
                break;
            }
            self.cursor += character.len_utf8();
        }
    }

    fn is_eof(&self) -> bool {
        self.cursor >= self.expression.len()
    }
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
    pub filter: Option<FilterExpr>,
}

impl TableQuery {
    pub fn new(page: PageRequest) -> Self {
        Self {
            page,
            sort: Vec::new(),
            filter: None,
        }
    }

    pub fn add_filter(&mut self, filter: FilterSpec) {
        let condition = FilterExpr::Condition(filter);
        self.filter = Some(match self.filter.take() {
            Some(existing) => FilterExpr::combine_and(existing, condition),
            None => condition,
        });
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
        DEFAULT_PAGE_SIZE, FilterExpr, FilterOperator, FilterSpec, PageCursor, PageRequest,
        SortDirection, SortSpec, TableQuery, parse_filter_expression, parse_sort_expression,
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
        assert!(query.filter.is_none());
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
        let filter = parse_filter_expression(
            "status = 'active' AND total >= 500 AND customer LIKE '%ACME%'",
            &[
                "status".to_owned(),
                "total".to_owned(),
                "customer".to_owned(),
            ],
        )
        .expect("valid filter expression");

        let FilterExpr::And(filters) = filter else {
            panic!("expected an AND expression");
        };
        assert_eq!(filters.len(), 3);
        let FilterExpr::Condition(status) = &filters[0] else {
            panic!("expected a condition");
        };
        assert_eq!(status.operator, FilterOperator::Equals);
        assert_eq!(status.value.as_deref(), Some("active"));
        let FilterExpr::Condition(total) = &filters[1] else {
            panic!("expected a condition");
        };
        assert_eq!(total.operator, FilterOperator::GreaterThanOrEqual);
        let FilterExpr::Condition(customer) = &filters[2] else {
            panic!("expected a condition");
        };
        assert_eq!(customer.operator, FilterOperator::Like);
        assert_eq!(customer.value.as_deref(), Some("%ACME%"));
    }

    #[test]
    fn parses_null_and_not_like_conditions() {
        let filter = parse_filter_expression(
            "deleted_at IS NULL AND name NOT LIKE 'test%'",
            &["deleted_at".to_owned(), "name".to_owned()],
        )
        .expect("valid filter expression");

        let FilterExpr::And(filters) = filter else {
            panic!("expected an AND expression");
        };
        let FilterExpr::Condition(deleted_at) = &filters[0] else {
            panic!("expected a condition");
        };
        assert_eq!(deleted_at.operator, FilterOperator::IsNull);
        assert_eq!(deleted_at.value, None);
        let FilterExpr::Condition(name) = &filters[1] else {
            panic!("expected a condition");
        };
        assert_eq!(name.operator, FilterOperator::NotLike);
        assert_eq!(name.value.as_deref(), Some("test%"));
    }

    #[test]
    fn parses_boolean_precedence_and_parentheses() {
        let filter = parse_filter_expression(
            "status = 'active' OR (status = 'pending' AND NOT archived = 1)",
            &["status".to_owned(), "archived".to_owned()],
        )
        .expect("valid boolean filter expression");

        let FilterExpr::Or(branches) = filter else {
            panic!("expected an OR expression");
        };
        assert_eq!(branches.len(), 2);
        assert!(matches!(&branches[0], FilterExpr::Condition(_)));
        let FilterExpr::And(group) = &branches[1] else {
            panic!("expected a parenthesized AND expression");
        };
        assert_eq!(group.len(), 2);
        assert!(matches!(&group[1], FilterExpr::Not(_)));
    }

    #[test]
    fn table_query_add_filter_keeps_legacy_and_semantics() {
        let mut query = TableQuery::default();
        query.add_filter(FilterSpec::new("id", FilterOperator::Equals, Some("1")));
        query.add_filter(FilterSpec::new(
            "status",
            FilterOperator::Equals,
            Some("active"),
        ));

        let Some(FilterExpr::And(filters)) = query.filter else {
            panic!("expected an AND filter");
        };
        assert_eq!(filters.len(), 2);
    }

    #[test]
    fn rejects_unknown_columns_and_unclosed_quotes() {
        assert!(parse_filter_expression("missing = 1", &["id".to_owned()]).is_err());
        assert!(parse_filter_expression("name = 'Ada", &["name".to_owned()]).is_err());
        assert!(matches!(
            parse_filter_expression("(name = 'Ada'", &["name".to_owned()]),
            Err(super::FilterParseError::MissingClosingParenthesis)
        ));
    }

    #[test]
    fn does_not_split_boolean_keywords_inside_quoted_values() {
        let filter = parse_filter_expression(
            "name = 'A AND B' OR note = 'OR later'",
            &["name".to_owned(), "note".to_owned()],
        )
        .expect("quoted boolean keywords are values");

        let FilterExpr::Or(branches) = filter else {
            panic!("expected an OR expression");
        };
        let FilterExpr::Condition(name) = &branches[0] else {
            panic!("expected a condition");
        };
        assert_eq!(name.value.as_deref(), Some("A AND B"));
        let FilterExpr::Condition(note) = &branches[1] else {
            panic!("expected a condition");
        };
        assert_eq!(note.value.as_deref(), Some("OR later"));
    }

    #[test]
    fn parses_multiple_sort_columns_and_defaults_to_ascending() {
        let sorts = parse_sort_expression(
            "created_at desc, status",
            &["created_at".to_owned(), "status".to_owned()],
        )
        .expect("valid sort expression");

        assert_eq!(sorts.len(), 2);
        assert_eq!(sorts[0], SortSpec::descending("created_at"));
        assert_eq!(sorts[1], SortSpec::ascending("status"));
    }

    #[test]
    fn sort_parser_matches_columns_case_insensitively_and_rejects_invalid_direction() {
        let sorts = parse_sort_expression("STATUS ASC", &["status".to_owned()])
            .expect("column names are case-insensitive");
        assert_eq!(sorts, vec![SortSpec::ascending("status")]);

        assert!(parse_sort_expression("status sideways", &["status".to_owned()]).is_err());
        assert!(parse_sort_expression("missing asc", &["status".to_owned()]).is_err());
    }
}
