use crate::db::{
    models::{DbObject, IndexMetadata, ObjectKind, TableIdentifier, TableMetadata},
    query::{FilterOperator, PageCursor, SortDirection, SortSpec, TableQuery},
};

const ROW_MARKER: &str = "__ASE_TUI_ROW__|";
const TEXT_MARKER: &str = "__ASE_TUI_TEXT__|";
const PREVIEW_HEADER_MARKER: &str = "__ASE_TUI_HEADER__|";

pub fn row_marker() -> &'static str {
    ROW_MARKER
}

pub fn header_marker() -> &'static str {
    PREVIEW_HEADER_MARKER
}

pub fn text_marker() -> &'static str {
    TEXT_MARKER
}

pub fn test_connection() -> String {
    return format!(
        "set nocount on\nselect '{ROW_MARKER}' + isnull(@@servername, '<sin @@servername>') + '|' + db_name()\n"
    );
}

pub fn list_databases() -> String {
    return format!(
        "set nocount on\n\
         select '{ROW_MARKER}' + rtrim(name)\n\
         from master..sysdatabases\n\
         order by name\n"
    );
}

pub fn list_objects(kind: ObjectKind) -> String {
    return format!(
        "set nocount on\n\
         select '{ROW_MARKER}' + rtrim(user_name(uid)) + '|' + rtrim(name)\n\
         from sysobjects\n\
         where type = '{}'\n\
           and name not like 'sys%'\n\
         order by user_name(uid), name\n",
        kind.sysobjects_type()
    );
}

pub fn object_definition(object: &DbObject) -> String {
    if object.kind == ObjectKind::Table {
        return table_definition(object);
    }

    let owner = string_literal(&object.owner);
    let name = string_literal(&object.name);
    return format!(
        "set nocount on\n\
         select '{TEXT_MARKER}' + convert(varchar(20), c.colid2) + ':' + convert(varchar(20), c.colid) + '|' + c.text\n\
         from syscomments c, sysobjects o\n\
         where c.id = o.id\n\
           and o.name = '{name}'\n\
           and user_name(o.uid) = '{owner}'\n\
         order by c.colid2, c.colid\n"
    );
}

pub fn preview_table(object: &DbObject, row_limit: usize, columns: &[(String, String)]) -> String {
    let table = qualified_identifier(&object.owner, &object.name);
    let header = columns
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>()
        .join("|");

    let header = string_literal(&header);

    let row_expression = columns
        .iter()
        .map(|(column, data_type)| {
            let identifier = quote_identifier(column);

            match data_type.to_ascii_lowercase().as_str() {
                "image" => {
                    format!(
                        "case when {identifier} is null \
                     then '<NULL>' \
                     else '<IMAGE>' end"
                    )
                }
                _ => {
                    format!("isnull(convert(varchar(255), {identifier}), '<NULL>')")
                }
            }
        })
        .collect::<Vec<_>>()
        .join(" + '|' + ");
    return format!(
        "set nocount on\n\
         set quoted_identifier on\n\
         select '{PREVIEW_HEADER_MARKER}{header}'\n\
         set rowcount {row_limit}\n\
         select '{ROW_MARKER}' + {row_expression}\n\
         from {table}\n\
         set rowcount 0\n"
    );
}

fn table_definition(object: &DbObject) -> String {
    let owner = string_literal(&object.owner);
    let name = string_literal(&object.name);
    return format!(
        "set nocount on\n\
         select '{ROW_MARKER}'\n\
              + rtrim(c.name) + '|'\n\
              + rtrim(t.name) + '|'\n\
              + convert(varchar(20), c.length) + '|'\n\
              + convert(varchar(20), isnull(c.prec, 0)) + '|'\n\
              + convert(varchar(20), isnull(c.scale, 0)) + '|'\n\
              + case when convert(int, c.status) & 8 = 8 then 'NULL' else 'NOT NULL' end\n\
         from syscolumns c, systypes t, sysobjects o\n\
         where c.id = o.id\n\
           and c.usertype = t.usertype\n\
           and o.name = '{name}'\n\
           and user_name(o.uid) = '{owner}'\n\
         order by c.colid\n"
    );
}

pub fn string_literal(value: &str) -> String {
    return value.replace('\'', "''");
}

pub fn qualified_identifier(owner: &str, name: &str) -> String {
    return format!("{}.{}", quote_identifier(owner), quote_identifier(name));
}

fn quote_identifier(value: &str) -> String {
    return format!("\"{}\"", value.replace('"', "\"\""));
}

pub fn table_columns(object: &DbObject) -> String {
    let owner = string_literal(&object.owner);
    let name = string_literal(&object.name);

    return format!(
        "set nocount on\n\
         select '{ROW_MARKER}' + rtrim(c.name) + '|' + rtrim(t.name)\n\
         from syscolumns c, systypes t, sysobjects o\n\
         where c.id = o.id\n\
           and c.usertype = t.usertype
           and o.name = '{name}'\n\
           and user_name(o.uid) = '{owner}'\n\
         order by c.colid\n"
    );
}

pub fn table_metadata(object: &DbObject) -> String {
    let owner = string_literal(&object.owner);
    let name = string_literal(&object.name);

    return format!(
        "set nocount on\n\
         select '{ROW_MARKER}COLUMN|'\n\
              + rtrim(c.name) + '|'\n\
              + rtrim(t.name) + '|'\n\
              + convert(varchar(20), c.length) + '|'\n\
              + convert(varchar(20), isnull(c.prec, 0)) + '|'\n\
              + convert(varchar(20), isnull(c.scale, 0)) + '|'\n\
              + case when convert(int, c.status) & 8 = 8 then '1' else '0' end + '|'\n\
              + convert(varchar(20), c.colid)\n\
         from syscolumns c, systypes t, sysobjects o\n\
         where c.id = o.id\n\
           and c.usertype = t.usertype\n\
           and o.name = '{name}'\n\
           and user_name(o.uid) = '{owner}'\n\
         order by c.colid\n\
         exec sp_helpindex '{owner}.{name}'\n"
    );
}

pub fn query_table(
    object: &DbObject,
    query: &TableQuery,
    columns: &[(String, String)],
    indexes: &[IndexMetadata],
) -> String {
    let table = qualified_identifier(&object.owner, &object.name);
    let sort = effective_sort_specs(query, columns, indexes);
    let mut predicates = query
        .filters
        .iter()
        .map(|filter| {
            filter_sql(
                filter.column.as_str(),
                filter.operator,
                filter.value.as_deref(),
            )
        })
        .collect::<Vec<_>>();
    if let Some(PageCursor::Keyset(values)) = query.page.cursor.as_ref()
        && let Some(seek_clause) = keyset_where_clause(&sort, values)
    {
        predicates.push(seek_clause);
    }
    let where_clause = if predicates.is_empty() {
        String::new()
    } else {
        format!("\nwhere {}", predicates.join("\n  and "))
    };
    let order_clause = if sort.is_empty() {
        String::new()
    } else {
        format!(
            "\norder by {}",
            sort.iter()
                .map(|sort| {
                    format!(
                        "{} {}",
                        quote_identifier(&sort.column),
                        match sort.direction {
                            SortDirection::Ascending => "asc",
                            SortDirection::Descending => "desc",
                        }
                    )
                })
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    let page_limit = query.page.limit.max(1) as u64;
    let fetch_limit = match query.page.cursor.as_ref() {
        Some(PageCursor::Offset(value)) => value.saturating_add(page_limit).saturating_add(1),
        _ => page_limit.saturating_add(1),
    };

    return format!(
        "set nocount on\n\
         set quoted_identifier on\n\
         set rowcount {fetch_limit}\n\
         select '{PREVIEW_HEADER_MARKER}{header}'\n\
         select '{ROW_MARKER}' + {projection}\n\
         from {table}{where_clause}{order_clause}\n\
         set rowcount 0\n",
        fetch_limit = fetch_limit,
        header = string_literal(
            &columns
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>()
                .join("|"),
        ),
        projection = row_expression(columns),
        table = table,
        where_clause = where_clause,
        order_clause = order_clause,
    );
}

pub fn effective_sort_specs(
    query: &TableQuery,
    columns: &[(String, String)],
    indexes: &[IndexMetadata],
) -> Vec<SortSpec> {
    let mut sort = query.sort.clone();
    let Some(unique_index) = stable_index(indexes, columns) else {
        return sort;
    };

    for column_name in &unique_index.columns {
        if !sort
            .iter()
            .any(|spec| spec.column.eq_ignore_ascii_case(column_name))
        {
            sort.push(SortSpec::ascending(column_name.clone()));
        }
    }

    sort
}

pub fn keyset_pagination_supported(query: &TableQuery, metadata: &TableMetadata) -> bool {
    let columns = metadata
        .columns
        .iter()
        .map(|column| (column.name.clone(), column.data_type.clone()))
        .collect::<Vec<_>>();
    let sort = effective_sort_specs(query, &columns, &metadata.indexes);

    !sort.is_empty()
        && stable_index(&metadata.indexes, &columns).is_some()
        && sort.iter().all(|spec| {
            metadata
                .columns
                .iter()
                .find(|column| column.name.eq_ignore_ascii_case(&spec.column))
                .is_some_and(|column| {
                    !column.nullable && is_keyset_sortable_type(&column.data_type)
                })
        })
}

fn stable_index<'a>(
    indexes: &'a [IndexMetadata],
    columns: &[(String, String)],
) -> Option<&'a IndexMetadata> {
    indexes
        .iter()
        .find(|index| index.is_primary && index_is_usable(index, columns))
        .or_else(|| {
            indexes
                .iter()
                .find(|index| index.is_unique && index_is_usable(index, columns))
        })
}

fn index_is_usable(index: &IndexMetadata, columns: &[(String, String)]) -> bool {
    !index.columns.is_empty()
        && index.columns.iter().all(|column| {
            columns
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case(column))
        })
}

fn is_keyset_sortable_type(data_type: &str) -> bool {
    !matches!(
        data_type.to_ascii_lowercase().as_str(),
        "image" | "text" | "unitext" | "ntext"
    )
}

fn keyset_where_clause(sort: &[SortSpec], values: &[String]) -> Option<String> {
    if sort.is_empty() || sort.len() != values.len() {
        return None;
    }

    let branches = sort
        .iter()
        .enumerate()
        .map(|(index, spec)| {
            let comparison = format!(
                "{} {} '{}'",
                quote_identifier(&spec.column),
                match spec.direction {
                    SortDirection::Ascending => ">",
                    SortDirection::Descending => "<",
                },
                string_literal(&values[index]),
            );
            if index == 0 {
                comparison
            } else {
                let equal_prefix = sort[..index]
                    .iter()
                    .enumerate()
                    .map(|(prefix_index, prefix)| {
                        format!(
                            "{} = '{}'",
                            quote_identifier(&prefix.column),
                            string_literal(&values[prefix_index]),
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n      and ");
                format!("({equal_prefix}\n      and {comparison})")
            }
        })
        .collect::<Vec<_>>();

    Some(format!("({})", branches.join("\n   or ")))
}

fn row_expression(columns: &[(String, String)]) -> String {
    columns
        .iter()
        .map(|(column, data_type)| {
            let identifier = quote_identifier(column);
            match data_type.to_ascii_lowercase().as_str() {
                "image" => {
                    format!("case when {identifier} is null then '<NULL>' else '<IMAGE>' end")
                }
                "text" | "unitext" => {
                    format!("case when {identifier} is null then '<NULL>' else '<TEXT>' end")
                }
                _ => format!("isnull(convert(varchar(255), {identifier}), '<NULL>')"),
            }
        })
        .collect::<Vec<_>>()
        .join(" + '|' + ")
}

fn filter_sql(column: &str, operator: FilterOperator, value: Option<&str>) -> String {
    let identifier = quote_identifier(column);
    match operator {
        FilterOperator::IsNull => format!("{identifier} is null"),
        FilterOperator::IsNotNull => format!("{identifier} is not null"),
        FilterOperator::Equals => comparison_sql(&identifier, "=", value),
        FilterOperator::NotEquals => comparison_sql(&identifier, "<>", value),
        FilterOperator::GreaterThan => comparison_sql(&identifier, ">", value),
        FilterOperator::GreaterThanOrEqual => comparison_sql(&identifier, ">=", value),
        FilterOperator::LessThan => comparison_sql(&identifier, "<", value),
        FilterOperator::LessThanOrEqual => comparison_sql(&identifier, "<=", value),
        FilterOperator::Like => like_sql(&identifier, "like", value),
        FilterOperator::NotLike => like_sql(&identifier, "not like", value),
        FilterOperator::Contains => like_pattern_sql(&identifier, "like", "%", "%", value),
        FilterOperator::StartsWith => like_pattern_sql(&identifier, "like", "", "%", value),
        FilterOperator::EndsWith => like_pattern_sql(&identifier, "like", "%", "", value),
    }
}

fn comparison_sql(identifier: &str, operator: &str, value: Option<&str>) -> String {
    format!(
        "{identifier} {operator} '{}'",
        string_literal(value.unwrap_or_default())
    )
}

fn like_sql(identifier: &str, operator: &str, value: Option<&str>) -> String {
    format!(
        "convert(varchar(255), {identifier}) {operator} '{}' escape '\\'",
        escape_like_expression(value.unwrap_or_default())
    )
}

fn like_pattern_sql(
    identifier: &str,
    operator: &str,
    prefix: &str,
    suffix: &str,
    value: Option<&str>,
) -> String {
    format!(
        "convert(varchar(255), {identifier}) {operator} '{}{}{}' escape '\\'",
        prefix,
        escape_like_pattern(value.unwrap_or_default()),
        suffix
    )
}

fn escape_like_pattern(value: &str) -> String {
    string_literal(
        &value
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_"),
    )
}

fn escape_like_expression(value: &str) -> String {
    string_literal(&value.replace('\\', "\\\\"))
}

pub fn table_identifier(object: &DbObject) -> TableIdentifier {
    TableIdentifier::new(object.owner.clone(), object.name.clone())
}

#[cfg(test)]
mod tests {
    use crate::db::{
        models::{ColumnMetadata, DbObject, IndexMetadata, TableIdentifier, TableMetadata},
        query::{FilterOperator, FilterSpec, PageCursor, PageRequest, SortSpec, TableQuery},
    };

    use super::{
        ObjectKind, keyset_pagination_supported, qualified_identifier, query_table, string_literal,
        table_identifier, table_metadata,
    };

    #[test]
    fn escapes_sql_literals() {
        assert_eq!(string_literal("O'Brien"), "O''Brien");
    }

    #[test]
    fn quotes_qualified_identifiers() {
        assert_eq!(qualified_identifier("dbo", "order"), "\"dbo\".\"order\"");
    }

    #[test]
    fn translates_table_query_without_interpolating_identifiers_as_values() {
        let object = DbObject {
            owner: "dbo".to_owned(),
            name: "orders".to_owned(),
            kind: ObjectKind::Table,
        };
        let mut query = TableQuery::new(PageRequest::new(10).expect("valid page size"));
        query.sort.push(SortSpec::descending("created_at"));
        query.filters.push(FilterSpec::new(
            "status",
            FilterOperator::Contains,
            Some("active'"),
        ));

        let sql = query_table(
            &object,
            &query,
            &[
                ("id".to_owned(), "int".to_owned()),
                ("status".to_owned(), "varchar".to_owned()),
            ],
            &[],
        );

        assert!(sql.contains("order by \"created_at\" desc"));
        assert!(sql.contains("like '%active''%"));
        assert!(sql.contains("from \"dbo\".\"orders\""));
        assert!(sql.contains("set quoted_identifier on"));
        assert!(sql.contains("set rowcount 11"));
        assert!(!sql.contains("active'%'"));
    }

    #[test]
    fn appends_a_unique_index_as_a_stable_pagination_tie_breaker() {
        let object = DbObject {
            owner: "dbo".to_owned(),
            name: "orders".to_owned(),
            kind: ObjectKind::Table,
        };
        let mut query = TableQuery::default();
        query.sort.push(SortSpec::descending("created_at"));

        let sql = query_table(
            &object,
            &query,
            &[
                ("id".to_owned(), "int".to_owned()),
                ("created_at".to_owned(), "datetime".to_owned()),
            ],
            &[IndexMetadata {
                name: "ux_orders_id".to_owned(),
                columns: vec!["id".to_owned()],
                is_unique: true,
                is_primary: false,
            }],
        );

        assert!(sql.contains("order by \"created_at\" desc, \"id\" asc"));
    }

    #[test]
    fn uses_a_unique_index_when_no_explicit_sort_is_requested() {
        let object = DbObject {
            owner: "dbo".to_owned(),
            name: "orders".to_owned(),
            kind: ObjectKind::Table,
        };

        let sql = query_table(
            &object,
            &TableQuery::default(),
            &[("id".to_owned(), "int".to_owned())],
            &[IndexMetadata {
                name: "pk_orders".to_owned(),
                columns: vec!["id".to_owned()],
                is_unique: false,
                is_primary: true,
            }],
        );

        assert!(sql.contains("order by \"id\" asc"));
    }

    #[test]
    fn emits_a_keyset_predicate_for_mixed_multi_column_sorting() {
        let object = DbObject {
            owner: "dbo".to_owned(),
            name: "orders".to_owned(),
            kind: ObjectKind::Table,
        };
        let mut query = TableQuery::new(PageRequest::new(10).expect("valid page size"));
        query.sort.push(SortSpec::descending("created_at"));
        query.page.cursor = Some(PageCursor::keyset(vec![
            "2026-08-14 10:00:00".to_owned(),
            "42".to_owned(),
        ]));

        let sql = query_table(
            &object,
            &query,
            &[
                ("id".to_owned(), "int".to_owned()),
                ("created_at".to_owned(), "datetime".to_owned()),
            ],
            &[IndexMetadata {
                name: "pk_orders".to_owned(),
                columns: vec!["id".to_owned()],
                is_unique: true,
                is_primary: false,
            }],
        );

        assert!(sql.contains("set rowcount 11"));
        assert!(sql.contains("\"created_at\" < '2026-08-14 10:00:00'"));
        assert!(sql.contains("\"created_at\" = '2026-08-14 10:00:00'"));
        assert!(sql.contains("\"id\" > '42'"));
        assert!(sql.contains("order by \"created_at\" desc, \"id\" asc"));
    }

    #[test]
    fn keyset_support_requires_non_nullable_scalar_sort_columns_and_a_unique_index() {
        let metadata = TableMetadata::new(
            TableIdentifier::new("dbo", "orders"),
            vec![ColumnMetadata {
                name: "id".to_owned(),
                data_type: "int".to_owned(),
                length: None,
                precision: None,
                scale: None,
                nullable: false,
                ordinal_position: 1,
            }],
            vec![IndexMetadata {
                name: "pk_orders".to_owned(),
                columns: vec!["id".to_owned()],
                is_unique: true,
                is_primary: false,
            }],
        );

        assert!(keyset_pagination_supported(
            &TableQuery::default(),
            &metadata
        ));

        let mut nullable_metadata = metadata.clone();
        nullable_metadata.columns[0].nullable = true;
        assert!(!keyset_pagination_supported(
            &TableQuery::default(),
            &nullable_metadata
        ));
    }

    #[test]
    fn escapes_literal_like_wildcards_but_preserves_explicit_like_patterns() {
        let object = DbObject {
            owner: "dbo".to_owned(),
            name: "orders".to_owned(),
            kind: ObjectKind::Table,
        };
        let mut query = TableQuery::default();
        query.filters.push(FilterSpec::new(
            "name",
            FilterOperator::Contains,
            Some("50%_off"),
        ));
        query
            .filters
            .push(FilterSpec::new("code", FilterOperator::Like, Some("A%")));

        let sql = query_table(
            &object,
            &query,
            &[("name".to_owned(), "varchar".to_owned())],
            &[],
        );

        assert!(sql.contains("like '%50\\%\\_off%' escape '\\'"));
        assert!(sql.contains("like 'A%' escape '\\'"));
    }

    #[test]
    fn renders_lob_columns_without_implicit_varchar_conversion() {
        let object = DbObject {
            owner: "dbo".to_owned(),
            name: "documents".to_owned(),
            kind: ObjectKind::Table,
        };
        let query = TableQuery::default();

        let sql = query_table(
            &object,
            &query,
            &[
                ("id".to_owned(), "int".to_owned()),
                ("payload".to_owned(), "image".to_owned()),
                ("notes".to_owned(), "text".to_owned()),
            ],
            &[],
        );

        assert!(sql.contains("'<IMAGE>'"));
        assert!(sql.contains("'<TEXT>'"));
        assert!(!sql.contains("convert(varchar(255), \"payload\")"));
        assert!(!sql.contains("convert(varchar(255), \"notes\")"));
    }

    #[test]
    fn maps_database_object_to_table_identifier() {
        let object = DbObject {
            owner: "dbo".to_owned(),
            name: "orders".to_owned(),
            kind: ObjectKind::Table,
        };

        assert_eq!(table_identifier(&object).qualified_name(), "dbo.orders");
    }

    #[test]
    fn delegates_index_metadata_to_ase_native_procedure() {
        let object = DbObject {
            owner: "dbo".to_owned(),
            name: "orders".to_owned(),
            kind: ObjectKind::Table,
        };

        let sql = table_metadata(&object);

        assert!(sql.contains("exec sp_helpindex 'dbo.orders'"));
        assert!(!sql.contains("index_col"));
        assert!(!sql.contains("sysindexes"));
    }
}
