use crate::db::{
    models::{DbObject, ObjectKind, TableIdentifier},
    query::{FilterOperator, PageCursor, SortDirection, TableQuery},
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
         select '{ROW_MARKER}INDEX|'\n\
              + rtrim(i.name) + '|'\n\
              + case when (i.status & 2) = 2 then '1' else '0' end + '|'\n\
              + case when (i.status & 2048) = 2048 then '1' else '0' end + '|'\n\
              + convert(varchar(10), isnull(i.key1, 0)) + '|'\n\
              + convert(varchar(10), isnull(i.key2, 0)) + '|'\n\
              + convert(varchar(10), isnull(i.key3, 0)) + '|'\n\
              + convert(varchar(10), isnull(i.key4, 0)) + '|'\n\
              + convert(varchar(10), isnull(i.key5, 0)) + '|'\n\
              + convert(varchar(10), isnull(i.key6, 0)) + '|'\n\
              + convert(varchar(10), isnull(i.key7, 0)) + '|'\n\
              + convert(varchar(10), isnull(i.key8, 0)) + '|'\n\
              + convert(varchar(10), isnull(i.key9, 0)) + '|'\n\
              + convert(varchar(10), isnull(i.key10, 0)) + '|'\n\
              + convert(varchar(10), isnull(i.key11, 0)) + '|'\n\
              + convert(varchar(10), isnull(i.key12, 0)) + '|'\n\
              + convert(varchar(10), isnull(i.key13, 0)) + '|'\n\
              + convert(varchar(10), isnull(i.key14, 0)) + '|'\n\
              + convert(varchar(10), isnull(i.key15, 0)) + '|'\n\
              + convert(varchar(10), isnull(i.key16, 0))\n\
         from sysindexes i, sysobjects o\n\
         where i.id = o.id\n\
           and o.name = '{name}'\n\
           and user_name(o.uid) = '{owner}'\n\
           and i.name is not null\n\
         order by i.name\n"
    );
}

pub fn query_table(object: &DbObject, query: &TableQuery, columns: &[String]) -> String {
    let table = qualified_identifier(&object.owner, &object.name);
    let where_clause = query
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
    let where_clause = if where_clause.is_empty() {
        String::new()
    } else {
        format!("\nwhere {}", where_clause.join("\n  and "))
    };
    let order_clause = if query.sort.is_empty() {
        String::new()
    } else {
        format!(
            "\norder by {}",
            query
                .sort
                .iter()
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
    let offset = match query.page.cursor.as_ref() {
        Some(PageCursor::Offset(value)) => *value,
        _ => 0,
    };
    let fetch_limit = offset
        .saturating_add(query.page.limit.max(1) as u64)
        .saturating_add(1);

    return format!(
        "set nocount on\n\
         set quoted_identifier on\n\
         set rowcount {fetch_limit}\n\
         select '{PREVIEW_HEADER_MARKER}{header}'\n\
         select '{ROW_MARKER}' + {projection}\n\
         from {table}{where_clause}{order_clause}\n\
         set rowcount 0\n",
        fetch_limit = fetch_limit,
        header = string_literal(&columns.join("|")),
        projection = row_expression(columns),
        table = table,
        where_clause = where_clause,
        order_clause = order_clause,
    );
}

fn row_expression(columns: &[String]) -> String {
    columns
        .iter()
        .map(|column| {
            format!(
                "isnull(convert(varchar(255), {}), '<NULL>')",
                quote_identifier(column)
            )
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
        FilterOperator::Contains => like_sql(&identifier, "%", "%", value),
        FilterOperator::StartsWith => like_sql(&identifier, "", "%", value),
        FilterOperator::EndsWith => like_sql(&identifier, "%", "", value),
    }
}

fn comparison_sql(identifier: &str, operator: &str, value: Option<&str>) -> String {
    format!(
        "{identifier} {operator} '{}'",
        string_literal(value.unwrap_or_default())
    )
}

fn like_sql(identifier: &str, prefix: &str, suffix: &str, value: Option<&str>) -> String {
    format!(
        "convert(varchar(255), {identifier}) like '{}{}{}'",
        prefix,
        string_literal(value.unwrap_or_default()),
        suffix
    )
}

pub fn table_identifier(object: &DbObject) -> TableIdentifier {
    TableIdentifier::new(object.owner.clone(), object.name.clone())
}

#[cfg(test)]
mod tests {
    use crate::db::{
        models::DbObject,
        query::{FilterOperator, FilterSpec, PageRequest, SortSpec, TableQuery},
    };

    use super::{
        ObjectKind, qualified_identifier, query_table, string_literal, table_identifier,
        table_metadata,
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

        let sql = query_table(&object, &query, &["id".to_owned(), "status".to_owned()]);

        assert!(sql.contains("order by \"created_at\" desc"));
        assert!(sql.contains("like '%active''%"));
        assert!(sql.contains("from \"dbo\".\"orders\""));
        assert!(sql.contains("set quoted_identifier on"));
        assert!(sql.contains("set rowcount 11"));
        assert!(!sql.contains("active'%'"));
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
    fn uses_ase_index_keys_instead_of_sql_server_catalogs() {
        let object = DbObject {
            owner: "dbo".to_owned(),
            name: "orders".to_owned(),
            kind: ObjectKind::Table,
        };

        let sql = table_metadata(&object);

        assert!(sql.contains("i.key1"));
        assert!(sql.contains("i.key16"));
        assert!(!sql.contains("sysindexkeys"));
    }
}
