use std::collections::BTreeSet;

use crate::db::{
    models::{
        ColumnMetadata, DbObject, IndexMetadata, ObjectKind, TableIdentifier, TableMetadata,
        TablePage,
    },
    query::{FilterOperator, PageCursor, SortDirection, SortSpec, TableQuery},
};

const ROW_MARKER: &str = "__ASE_TUI_ROW__|";
const TEXT_MARKER: &str = "__ASE_TUI_TEXT__|";
const PREVIEW_HEADER_MARKER: &str = "__ASE_TUI_HEADER__|";
const STAGED_COMMITTED_MARKER: &str = "__ASE_TUI_COMMITTED__";
const STAGED_ROLLED_BACK_MARKER: &str = "__ASE_TUI_ROLLED_BACK__";

pub fn row_marker() -> &'static str {
    ROW_MARKER
}

pub fn header_marker() -> &'static str {
    PREVIEW_HEADER_MARKER
}

pub fn text_marker() -> &'static str {
    TEXT_MARKER
}

pub fn staged_committed_marker() -> &'static str {
    STAGED_COMMITTED_MARKER
}

pub fn staged_rolled_back_marker() -> &'static str {
    STAGED_ROLLED_BACK_MARKER
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

fn wire_escape_field(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('|', "\\|")
        .replace('\r', "\\r")
        .replace('\n', "\\n")
}

fn wire_escape_sql(expression: &str) -> String {
    format!(
        "str_replace(str_replace(str_replace(str_replace({expression}, char(92), char(92) + char(92)), char(124), char(92) + char(124)), char(13), char(92) + 'r'), char(10), char(92) + 'n')"
    )
}

fn preview_value_expression(identifier: &str, data_type: &str) -> String {
    let expression = match data_type.to_ascii_lowercase().as_str() {
        "image" => format!("case when {identifier} is null then '<NULL>' else '<IMAGE>' end"),
        _ => format!("isnull(convert(varchar(255), {identifier}), '<NULL>')"),
    };
    wire_escape_sql(&expression)
}

pub fn preview_table(object: &DbObject, row_limit: usize, columns: &[(String, String)]) -> String {
    let table = qualified_identifier(&object.owner, &object.name);
    let header = columns
        .iter()
        .map(|(name, _)| wire_escape_field(name))
        .collect::<Vec<_>>()
        .join("|");

    let header = string_literal(&header);

    let row_expression = columns
        .iter()
        .map(|(column, data_type)| preview_value_expression(&quote_identifier(column), data_type))
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
         from syscolumns c, (select a.usertype, isnull((select max(d.local_type_name) from sybsystemprocs.dbo.spt_datatype_info d where d.ss_dtype = a.type), a.name) name from systypes a) t, sysobjects o\n\
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
         from syscolumns c, (select a.usertype, isnull((select max(d.local_type_name) from sybsystemprocs.dbo.spt_datatype_info d where d.ss_dtype = a.type), a.name) name from systypes a) t, sysobjects o\n\
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
         from syscolumns c, (select a.usertype, isnull((select max(d.local_type_name) from sybsystemprocs.dbo.spt_datatype_info d where d.ss_dtype = a.type), a.name) name from systypes a) t, sysobjects o\n\
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
) -> Result<String, String> {
    let table = qualified_identifier(&object.owner, &object.name);
    let sort = effective_sort_specs(query, columns, indexes);
    let mut predicates = query
        .filters
        .iter()
        .map(|filter| {
            let data_type = columns
                .iter()
                .find(|(column, _)| column.eq_ignore_ascii_case(&filter.column))
                .map(|(_, data_type)| data_type.as_str())
                .ok_or_else(|| format!("no hay metadata para la columna {}", filter.column))?;
            filter_sql(
                filter.column.as_str(),
                data_type,
                filter.operator,
                filter.value.as_deref(),
            )
        })
        .collect::<Result<Vec<_>, String>>()?;
    if let Some(PageCursor::Keyset(values)) = query.page.cursor.as_ref()
        && let Some(seek_clause) = keyset_where_clause(&sort, values, columns)?
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

    Ok(format!(
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
                .map(|(name, _)| wire_escape_field(name))
                .collect::<Vec<_>>()
                .join("|"),
        ),
        projection = row_expression(columns),
        table = table,
        where_clause = where_clause,
        order_clause = order_clause,
    ))
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

fn keyset_where_clause(
    sort: &[SortSpec],
    values: &[String],
    columns: &[(String, String)],
) -> Result<Option<String>, String> {
    if sort.is_empty() || sort.len() != values.len() {
        return Ok(None);
    }

    let literals = sort
        .iter()
        .enumerate()
        .map(|(index, spec)| {
            let data_type = columns
                .iter()
                .find(|(column, _)| column.eq_ignore_ascii_case(&spec.column))
                .map(|(_, data_type)| data_type.as_str())
                .ok_or_else(|| format!("no hay metadata para la columna {}", spec.column))?;
            filter_value_sql(data_type, &values[index])
        })
        .collect::<Result<Vec<_>, String>>()?;

    let branches = sort
        .iter()
        .enumerate()
        .map(|(index, spec)| {
            let comparison = format!(
                "{} {} {}",
                quote_identifier(&spec.column),
                match spec.direction {
                    SortDirection::Ascending => ">",
                    SortDirection::Descending => "<",
                },
                literals[index],
            );
            if index == 0 {
                comparison
            } else {
                let equal_prefix = sort[..index]
                    .iter()
                    .enumerate()
                    .map(|(prefix_index, prefix)| {
                        format!(
                            "{} = {}",
                            quote_identifier(&prefix.column),
                            literals[prefix_index],
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n      and ");
                format!("({equal_prefix}\n      and {comparison})")
            }
        })
        .collect::<Vec<_>>();

    Ok(Some(format!("({})", branches.join("\n   or "))))
}

fn row_expression(columns: &[(String, String)]) -> String {
    columns
        .iter()
        .map(|(column, data_type)| {
            let identifier = quote_identifier(column);
            let expression = match data_type.to_ascii_lowercase().as_str() {
                "image" => {
                    format!("case when {identifier} is null then '<NULL>' else '<IMAGE>' end")
                }
                "text" | "unitext" => {
                    format!("case when {identifier} is null then '<NULL>' else '<TEXT>' end")
                }
                _ => format!("isnull(convert(varchar(255), {identifier}), '<NULL>')"),
            };
            wire_escape_sql(&expression)
        })
        .collect::<Vec<_>>()
        .join(" + '|' + ")
}

fn filter_sql(
    column: &str,
    data_type: &str,
    operator: FilterOperator,
    value: Option<&str>,
) -> Result<String, String> {
    let identifier = quote_identifier(column);
    match operator {
        FilterOperator::IsNull => Ok(format!("{identifier} is null")),
        FilterOperator::IsNotNull => Ok(format!("{identifier} is not null")),
        FilterOperator::Equals => comparison_sql(&identifier, data_type, "=", value),
        FilterOperator::NotEquals => comparison_sql(&identifier, data_type, "<>", value),
        FilterOperator::GreaterThan => comparison_sql(&identifier, data_type, ">", value),
        FilterOperator::GreaterThanOrEqual => comparison_sql(&identifier, data_type, ">=", value),
        FilterOperator::LessThan => comparison_sql(&identifier, data_type, "<", value),
        FilterOperator::LessThanOrEqual => comparison_sql(&identifier, data_type, "<=", value),
        FilterOperator::Like => like_sql(&identifier, data_type, "like", value),
        FilterOperator::NotLike => like_sql(&identifier, data_type, "not like", value),
        FilterOperator::Contains => {
            like_pattern_sql(&identifier, data_type, "like", "%", "%", value)
        }
        FilterOperator::StartsWith => {
            like_pattern_sql(&identifier, data_type, "like", "", "%", value)
        }
        FilterOperator::EndsWith => {
            like_pattern_sql(&identifier, data_type, "like", "%", "", value)
        }
    }
}

fn comparison_sql(
    identifier: &str,
    data_type: &str,
    operator: &str,
    value: Option<&str>,
) -> Result<String, String> {
    let value = filter_value_sql(data_type, value.unwrap_or_default())?;
    Ok(format!("{identifier} {operator} {value}"))
}

fn like_sql(
    identifier: &str,
    data_type: &str,
    operator: &str,
    value: Option<&str>,
) -> Result<String, String> {
    let value = escape_like_expression(value.unwrap_or_default());
    let expression = if is_lob_type(data_type) {
        identifier.to_owned()
    } else {
        format!("convert(varchar(255), {identifier})")
    };
    Ok(format!("{expression} {operator} '{value}' escape '\\'"))
}

fn like_pattern_sql(
    identifier: &str,
    data_type: &str,
    operator: &str,
    prefix: &str,
    suffix: &str,
    value: Option<&str>,
) -> Result<String, String> {
    let value = format!(
        "{}{}{}",
        prefix,
        escape_like_pattern(value.unwrap_or_default()),
        suffix
    );
    let expression = if is_lob_type(data_type) {
        identifier.to_owned()
    } else {
        format!("convert(varchar(255), {identifier})")
    };
    Ok(format!("{expression} {operator} '{value}' escape '\\'"))
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

fn filter_value_sql(data_type: &str, value: &str) -> Result<String, String> {
    let raw_value = value;
    let value = value.trim();
    if value.eq_ignore_ascii_case("NULL") || value.eq_ignore_ascii_case("<NULL>") {
        return Err("usa IS NULL o IS NOT NULL para buscar valores NULL".to_owned());
    }

    if !is_numeric_type(data_type) {
        return Ok(format!("'{}'", string_literal(raw_value)));
    }

    match data_type.to_ascii_lowercase().as_str() {
        "tinyint" | "smallint" | "int" | "integer" | "bigint" => value
            .parse::<i128>()
            .map(|_| value.to_owned())
            .map_err(|_| format!("{data_type} requiere un entero válido")),
        "numeric" | "decimal" | "money" | "smallmoney" => validate_decimal_syntax(value)
            .map(|_| value.to_owned())
            .map_err(|_| format!("{data_type} requiere un número decimal válido")),
        "float" | "real" | "double" | "double precision" => value
            .parse::<f64>()
            .ok()
            .filter(|number| number.is_finite())
            .map(|_| value.to_owned())
            .ok_or_else(|| format!("{data_type} requiere un número válido")),
        "bit" => match value.to_ascii_lowercase().as_str() {
            "0" | "false" => Ok("0".to_owned()),
            "1" | "true" => Ok("1".to_owned()),
            _ => Err("bit requiere 0, 1, true o false".to_owned()),
        },
        _ => Ok(format!("'{}'", string_literal(raw_value))),
    }
}

fn is_numeric_type(data_type: &str) -> bool {
    matches!(
        data_type.to_ascii_lowercase().as_str(),
        "tinyint"
            | "smallint"
            | "int"
            | "integer"
            | "bigint"
            | "numeric"
            | "decimal"
            | "money"
            | "smallmoney"
            | "float"
            | "real"
            | "double"
            | "double precision"
            | "bit"
    )
}

fn is_lob_type(data_type: &str) -> bool {
    matches!(
        data_type.to_ascii_lowercase().as_str(),
        "text" | "unitext" | "ntext" | "image"
    )
}

pub fn table_identifier(object: &DbObject) -> TableIdentifier {
    TableIdentifier::new(object.owner.clone(), object.name.clone())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableSqlPreview {
    pub sql: String,
    pub blockers: Vec<String>,
}

pub fn preview_staged_table_changes(
    metadata: &TableMetadata,
    page: &TablePage,
    drafts: &[(usize, String, String, String)],
    deleted_rows: &[usize],
    new_rows: &[usize],
) -> TableSqlPreview {
    let table = qualified_identifier(&metadata.identifier.schema, &metadata.identifier.name);
    let page_columns = page
        .columns
        .iter()
        .map(|column| (column.clone(), String::new()))
        .collect::<Vec<_>>();
    let identity = stable_index(&metadata.indexes, &page_columns);
    let deleted_rows = deleted_rows.iter().copied().collect::<BTreeSet<_>>();
    let new_rows = new_rows.iter().copied().collect::<BTreeSet<_>>();
    let mut statements = Vec::new();
    let mut blockers = Vec::new();

    let update_rows = drafts
        .iter()
        .map(|draft| draft.0)
        .filter(|row_index| !new_rows.contains(row_index) && !deleted_rows.contains(row_index))
        .collect::<BTreeSet<_>>();
    for row_index in update_rows {
        let row_drafts = drafts
            .iter()
            .filter(|draft| draft.0 == row_index)
            .collect::<Vec<_>>();
        match update_statement(&table, metadata, page, identity, row_index, &row_drafts) {
            Ok(statement) => statements.push(statement),
            Err(error) => blockers.push(format!("UPDATE fila {}: {error}", row_index + 1)),
        }
    }

    for row_index in &deleted_rows {
        if new_rows.contains(row_index) {
            continue;
        }
        match delete_statement(&table, metadata, page, identity, *row_index) {
            Ok(statement) => statements.push(statement),
            Err(error) => blockers.push(format!("DELETE fila {}: {error}", row_index + 1)),
        }
    }

    for row_index in &new_rows {
        match insert_statement(&table, metadata, page, drafts, *row_index) {
            Ok(statement) => statements.push(statement),
            Err(error) => blockers.push(format!("INSERT fila {}: {error}", row_index + 1)),
        }
    }

    let sql = if statements.is_empty() {
        "-- No se generaron sentencias seguras para previsualizar.\n".to_owned()
    } else {
        transactional_staged_sql(&statements)
    };

    TableSqlPreview { sql, blockers }
}

fn transactional_staged_sql(statements: &[String]) -> String {
    let guarded_statements = statements
        .iter()
        .map(|statement| {
            format!(
                "if @ase_tui_failed = 0\nbegin\n{statement}\nif @@error <> 0\n    select @ase_tui_failed = 1\nend"
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    format!(
        "-- Vista previa local de cambios staged, no se ha ejecutado SQL.\n-- La ejecución usa una transacción y solo confirma si todas las sentencias tienen éxito.\nset nocount on\nset quoted_identifier on\n\ndeclare @ase_tui_failed int\nselect @ase_tui_failed = 0\nbegin transaction\nif @@error <> 0\n    select @ase_tui_failed = 1\n\n{guarded_statements}\n\nif @ase_tui_failed = 0\nbegin\n    commit transaction\n    if @@error = 0\n        select '{STAGED_COMMITTED_MARKER}'\n    else\n    begin\n        rollback transaction\n        select '{STAGED_ROLLED_BACK_MARKER}'\n    end\nend\nelse\nbegin\n    rollback transaction\n    select '{STAGED_ROLLED_BACK_MARKER}'\nend\n"
    )
}

fn update_statement(
    table: &str,
    metadata: &TableMetadata,
    page: &TablePage,
    identity: Option<&IndexMetadata>,
    row_index: usize,
    drafts: &[&(usize, String, String, String)],
) -> Result<String, String> {
    let assignments = drafts
        .iter()
        .map(|draft| {
            let column = page_column_metadata(metadata, page, &draft.1)?;
            let value = sql_value(&draft.3, column)
                .map_err(|error| format!("columna {}: {error}", draft.1))?;
            Ok(format!("{} = {value}", quote_identifier(&draft.1)))
        })
        .collect::<Result<Vec<_>, String>>()?;
    if assignments.is_empty() {
        return Err("no contiene celdas editadas".to_owned());
    }
    let where_clause = identity_where_clause(metadata, page, identity, row_index)?;
    Ok(format!(
        "update {table}\nset {}\nwhere {where_clause}",
        assignments.join(",\n    ")
    ))
}

fn delete_statement(
    table: &str,
    metadata: &TableMetadata,
    page: &TablePage,
    identity: Option<&IndexMetadata>,
    row_index: usize,
) -> Result<String, String> {
    let where_clause = identity_where_clause(metadata, page, identity, row_index)?;
    Ok(format!("delete from {table}\nwhere {where_clause}"))
}

fn insert_statement(
    table: &str,
    metadata: &TableMetadata,
    page: &TablePage,
    drafts: &[(usize, String, String, String)],
    row_index: usize,
) -> Result<String, String> {
    let row = page
        .rows
        .get(row_index)
        .ok_or_else(|| "la fila ya no está cargada".to_owned())?;
    if row.len() != page.columns.len() {
        return Err("la fila no coincide con el ancho de columnas cargado".to_owned());
    }

    let values = page
        .columns
        .iter()
        .enumerate()
        .map(|(column_index, column_name)| {
            let column = metadata_column(metadata, column_name)
                .ok_or_else(|| format!("no hay metadata para la columna {column_name}"))?;
            let raw_value = drafts
                .iter()
                .find(|draft| draft.0 == row_index && draft.1.eq_ignore_ascii_case(column_name))
                .map(|draft| draft.3.as_str())
                .unwrap_or_else(|| row[column_index].as_str());
            sql_value(raw_value, column).map_err(|error| format!("columna {column_name}: {error}"))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let columns = page
        .columns
        .iter()
        .map(|column| quote_identifier(column))
        .collect::<Vec<_>>()
        .join(", ");
    Ok(format!(
        "insert into {table} ({columns})\nvalues ({})",
        values.join(", ")
    ))
}

fn identity_where_clause(
    metadata: &TableMetadata,
    page: &TablePage,
    identity: Option<&IndexMetadata>,
    row_index: usize,
) -> Result<String, String> {
    let identity = identity
        .ok_or_else(|| "no se encontró una clave primaria ni un índice único usable".to_owned())?;
    let row = page
        .rows
        .get(row_index)
        .ok_or_else(|| "la fila ya no está cargada".to_owned())?;
    if row.len() != page.columns.len() {
        return Err("la fila no coincide con el ancho de columnas cargado".to_owned());
    }

    identity
        .columns
        .iter()
        .map(|column_name| {
            let column_index = page_column_index(page, column_name)
                .ok_or_else(|| format!("la identidad usa una columna no cargada: {column_name}"))?;
            let column = page_column_metadata(metadata, page, column_name)?;
            let raw_value = row
                .get(column_index)
                .ok_or_else(|| format!("no hay valor para la identidad {column_name}"))?;
            if is_null_marker(raw_value.trim()) {
                return Err(format!("la identidad {column_name} contiene NULL"));
            }
            let value = sql_value(raw_value, column)
                .map_err(|error| format!("identidad {column_name}: {error}"))?;
            Ok(format!("{} = {value}", quote_identifier(column_name)))
        })
        .collect::<Result<Vec<_>, String>>()
        .map(|clauses| clauses.join("\n  and "))
}

fn page_column_metadata<'a>(
    metadata: &'a TableMetadata,
    page: &TablePage,
    column_name: &str,
) -> Result<&'a ColumnMetadata, String> {
    if page_column_index(page, column_name).is_none() {
        return Err(format!("la columna no está cargada: {column_name}"));
    }
    metadata_column(metadata, column_name)
        .ok_or_else(|| format!("no hay metadata para la columna {column_name}"))
}

fn metadata_column<'a>(
    metadata: &'a TableMetadata,
    column_name: &str,
) -> Option<&'a ColumnMetadata> {
    metadata
        .columns
        .iter()
        .find(|column| column.name.eq_ignore_ascii_case(column_name))
}

fn page_column_index(page: &TablePage, column_name: &str) -> Option<usize> {
    page.columns
        .iter()
        .position(|column| column.eq_ignore_ascii_case(column_name))
}

fn sql_value(value: &str, column: &ColumnMetadata) -> Result<String, String> {
    let raw_value = value;
    let value = value.trim();
    if is_null_marker(value) {
        return if column.nullable {
            Ok("NULL".to_owned())
        } else {
            Err("la columna no permite NULL".to_owned())
        };
    }

    let data_type = column.data_type.to_ascii_lowercase();
    match data_type.as_str() {
        "tinyint" | "smallint" | "int" | "integer" | "bigint" => value
            .parse::<i128>()
            .map(|_| value.to_owned())
            .map_err(|_| format!("{data_type} requiere un entero")),
        "numeric" | "decimal" | "money" | "smallmoney" => validate_decimal_syntax(value)
            .map(|_| value.to_owned())
            .map_err(|_| format!("{data_type} requiere un número decimal válido")),
        "float" | "real" | "double" | "double precision" => value
            .parse::<f64>()
            .ok()
            .filter(|number| number.is_finite())
            .map(|_| value.to_owned())
            .ok_or_else(|| format!("{data_type} requiere un número válido")),
        "bit" => match value.to_ascii_lowercase().as_str() {
            "0" | "false" => Ok("0".to_owned()),
            "1" | "true" => Ok("1".to_owned()),
            _ => Err("bit requiere 0, 1, true o false".to_owned()),
        },
        "char" | "varchar" | "nchar" | "nvarchar" | "unichar" | "univarchar" | "date"
        | "datetime" | "smalldatetime" | "time" | "uniqueidentifier" => {
            Ok(format!("'{}'", string_literal(raw_value)))
        }
        "text" | "unitext" | "ntext" => {
            if matches!(value, "<TEXT>" | "<IMAGE>") {
                Err(format!(
                    "tipo {data_type} contiene un marcador LOB no editable"
                ))
            } else {
                Ok(format!("'{}'", string_literal(raw_value)))
            }
        }
        "image" | "binary" | "varbinary" => Err(format!(
            "tipo {data_type} requiere una representación binaria no disponible"
        )),
        _ => Err(format!("tipo {data_type} no soportado para SQL staged")),
    }
}

fn is_null_marker(value: &str) -> bool {
    value.eq_ignore_ascii_case("NULL") || value.eq_ignore_ascii_case("<NULL>")
}

fn validate_decimal_syntax(value: &str) -> Result<(), ()> {
    let value = value
        .strip_prefix('+')
        .or_else(|| value.strip_prefix('-'))
        .unwrap_or(value);
    let (integer, fraction) = value.split_once('.').unwrap_or((value, ""));
    if integer.is_empty() && fraction.is_empty() {
        return Err(());
    }
    if !integer.is_empty() && !integer.chars().all(|character| character.is_ascii_digit()) {
        return Err(());
    }
    if !fraction.is_empty() && !fraction.chars().all(|character| character.is_ascii_digit()) {
        return Err(());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::db::{
        models::{
            ColumnMetadata, DbObject, IndexMetadata, TableIdentifier, TableMetadata, TablePage,
        },
        query::{FilterOperator, FilterSpec, PageCursor, PageRequest, SortSpec, TableQuery},
    };

    use super::{
        ObjectKind, keyset_pagination_supported, preview_staged_table_changes, preview_table,
        qualified_identifier, query_table, sql_value, staged_committed_marker,
        staged_rolled_back_marker, string_literal, table_columns, table_identifier, table_metadata,
        validate_decimal_syntax, wire_escape_field,
    };

    #[test]
    fn escapes_sql_literals() {
        assert_eq!(string_literal("O'Brien"), "O''Brien");
    }

    #[test]
    fn escapes_wire_fields_without_colliding_with_delimiters() {
        assert_eq!(wire_escape_field("a|b\\c\r\nd"), "a\\|b\\\\c\\r\\nd");
    }

    #[test]
    fn escapes_preview_and_paged_row_values_before_serializing_them() {
        let object = DbObject {
            owner: "dbo".to_owned(),
            name: "orders".to_owned(),
            kind: ObjectKind::Table,
        };
        let columns = vec![("notes|label".to_owned(), "varchar".to_owned())];

        let preview_sql = preview_table(&object, 10, &columns);
        assert!(preview_sql.contains("notes\\|label"));
        assert!(preview_sql.contains("str_replace"));
        assert!(preview_sql.contains("char(124)"));

        let query_sql =
            query_table(&object, &TableQuery::default(), &columns, &[]).expect("valid query");
        assert!(query_sql.contains("str_replace"));
        assert!(query_sql.contains("char(13)"));
    }

    #[test]
    fn previews_safe_staged_updates_deletes_and_inserts() {
        let metadata = TableMetadata::new(
            TableIdentifier::new("dbo", "orders"),
            vec![
                ColumnMetadata {
                    name: "id".to_owned(),
                    data_type: "int".to_owned(),
                    length: None,
                    precision: None,
                    scale: None,
                    nullable: false,
                    ordinal_position: 1,
                },
                ColumnMetadata {
                    name: "name".to_owned(),
                    data_type: "varchar".to_owned(),
                    length: Some(100),
                    precision: None,
                    scale: None,
                    nullable: false,
                    ordinal_position: 2,
                },
                ColumnMetadata {
                    name: "amount".to_owned(),
                    data_type: "decimal".to_owned(),
                    length: None,
                    precision: Some(10),
                    scale: Some(2),
                    nullable: true,
                    ordinal_position: 3,
                },
            ],
            vec![IndexMetadata {
                name: "pk_orders".to_owned(),
                columns: vec!["id".to_owned()],
                is_unique: false,
                is_primary: true,
            }],
        );
        let page = TablePage::new(
            vec!["id".to_owned(), "name".to_owned(), "amount".to_owned()],
            vec![
                vec!["1".to_owned(), "Ada".to_owned(), "10.00".to_owned()],
                vec!["2".to_owned(), "Grace".to_owned(), "20.00".to_owned()],
                vec!["3".to_owned(), "New".to_owned(), "12.50".to_owned()],
            ],
            None,
            false,
            None,
        )
        .expect("valid table page");
        let drafts = vec![
            (0, "name".to_owned(), "Ada".to_owned(), "Grace".to_owned()),
            (
                2,
                "name".to_owned(),
                "New".to_owned(),
                "New'Name".to_owned(),
            ),
        ];

        let preview = preview_staged_table_changes(&metadata, &page, &drafts, &[1], &[2]);

        assert!(preview.blockers.is_empty(), "{:?}", preview.blockers);
        assert!(preview.sql.contains("begin transaction"));
        assert!(preview.sql.contains("if @ase_tui_failed = 0"));
        assert!(preview.sql.contains("commit transaction"));
        assert!(preview.sql.contains("rollback transaction"));
        assert!(preview.sql.contains(staged_committed_marker()));
        assert!(preview.sql.contains(staged_rolled_back_marker()));
        assert!(preview.sql.contains("update \"dbo\".\"orders\""));
        assert!(preview.sql.contains("\"name\" = 'Grace'"));
        assert!(!preview.sql.contains(';'));
        assert!(!preview.sql.contains("where \"id\" = 1;"));
        assert!(!preview.sql.contains("where \"id\" = 2;"));
        assert!(!preview.sql.contains("values (3, 'New''Name', 12.50);"));
        assert!(preview.sql.contains("where \"id\" = 1"));
        assert!(
            preview
                .sql
                .contains("delete from \"dbo\".\"orders\"\nwhere \"id\" = 2")
        );
        assert!(preview.sql.contains(
            "insert into \"dbo\".\"orders\" (\"id\", \"name\", \"amount\")\nvalues (3, 'New''Name', 12.50)"
        ));
    }

    #[test]
    fn preserves_string_whitespace_and_rejects_repeated_decimal_signs() {
        let column = ColumnMetadata {
            name: "name".to_owned(),
            data_type: "varchar".to_owned(),
            length: Some(100),
            precision: None,
            scale: None,
            nullable: false,
            ordinal_position: 1,
        };

        assert_eq!(
            sql_value("  Grace  ", &column).expect("valid string"),
            "'  Grace  '"
        );
        assert!(validate_decimal_syntax("--1").is_err());
    }

    #[test]
    fn blocks_staged_changes_without_identity_or_valid_typed_values() {
        let metadata = TableMetadata::new(
            TableIdentifier::new("dbo", "orders"),
            vec![
                ColumnMetadata {
                    name: "id".to_owned(),
                    data_type: "int".to_owned(),
                    length: None,
                    precision: None,
                    scale: None,
                    nullable: false,
                    ordinal_position: 1,
                },
                ColumnMetadata {
                    name: "name".to_owned(),
                    data_type: "varchar".to_owned(),
                    length: Some(100),
                    precision: None,
                    scale: None,
                    nullable: true,
                    ordinal_position: 2,
                },
            ],
            Vec::new(),
        );
        let page = TablePage::new(
            vec!["id".to_owned(), "name".to_owned()],
            vec![
                vec!["1".to_owned(), "Ada".to_owned()],
                vec!["2".to_owned(), "Grace".to_owned()],
                vec!["not-an-int".to_owned(), "New".to_owned()],
            ],
            None,
            false,
            None,
        )
        .expect("valid table page");
        let drafts = vec![(0, "name".to_owned(), "Ada".to_owned(), "Grace".to_owned())];

        let preview = preview_staged_table_changes(&metadata, &page, &drafts, &[1], &[2]);

        assert_eq!(preview.blockers.len(), 3);
        assert!(
            preview
                .blockers
                .iter()
                .any(|blocker| blocker.contains("UPDATE fila 1"))
        );
        assert!(
            preview
                .blockers
                .iter()
                .any(|blocker| blocker.contains("DELETE fila 2"))
        );
        assert!(
            preview
                .blockers
                .iter()
                .any(|blocker| blocker.contains("INSERT fila 3"))
        );
        assert!(!preview.sql.contains("update \"dbo\".\"orders\""));
        assert!(!preview.sql.contains("delete from \"dbo\".\"orders\""));
        assert!(!preview.sql.contains("insert into \"dbo\".\"orders\""));
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
        )
        .expect("valid query");

        assert!(sql.contains("order by \"created_at\" desc"));
        assert!(sql.contains("like '%active''%"));
        assert!(sql.contains("from \"dbo\".\"orders\""));
        assert!(sql.contains("set quoted_identifier on"));
        assert!(sql.contains("set rowcount 11"));
        assert!(!sql.contains("active'%'"));
    }

    #[test]
    fn renders_numeric_filters_as_typed_literals() {
        let object = DbObject {
            owner: "dbo".to_owned(),
            name: "orders".to_owned(),
            kind: ObjectKind::Table,
        };
        let mut query = TableQuery::default();
        query
            .filters
            .push(FilterSpec::new("id", FilterOperator::Equals, Some("42")));
        query.filters.push(FilterSpec::new(
            "amount",
            FilterOperator::GreaterThanOrEqual,
            Some("10.50"),
        ));

        let sql = query_table(
            &object,
            &query,
            &[
                ("id".to_owned(), "int".to_owned()),
                ("amount".to_owned(), "decimal".to_owned()),
            ],
            &[],
        )
        .expect("valid numeric filters");

        assert!(sql.contains("\"id\" = 42"));
        assert!(sql.contains("\"amount\" >= 10.50"));
        assert!(!sql.contains("\"id\" = '42'"));
    }

    #[test]
    fn rejects_invalid_numeric_filter_values_before_sql_execution() {
        let object = DbObject {
            owner: "dbo".to_owned(),
            name: "orders".to_owned(),
            kind: ObjectKind::Table,
        };
        let mut query = TableQuery::default();
        query
            .filters
            .push(FilterSpec::new("id", FilterOperator::Equals, Some("abc")));

        let error = query_table(&object, &query, &[("id".to_owned(), "int".to_owned())], &[])
            .expect_err("invalid numeric filter must be rejected");

        assert!(error.contains("int requiere un entero válido"));
    }

    #[test]
    fn filters_lob_columns_without_converting_them() {
        let object = DbObject {
            owner: "dbo".to_owned(),
            name: "documents".to_owned(),
            kind: ObjectKind::Table,
        };
        let mut query = TableQuery::default();
        query.filters.push(FilterSpec::new(
            "payload",
            FilterOperator::Like,
            Some("0x%"),
        ));
        query.filters.push(FilterSpec::new(
            "notes",
            FilterOperator::Contains,
            Some("contract"),
        ));

        let sql = query_table(
            &object,
            &query,
            &[
                ("payload".to_owned(), "image".to_owned()),
                ("notes".to_owned(), "text".to_owned()),
            ],
            &[],
        )
        .expect("valid LOB filters");

        assert!(sql.contains("\"payload\" like '0x%'"));
        assert!(sql.contains("\"notes\" like '%contract%'"));
        assert!(!sql.contains("convert(varchar(255), \"payload\")"));
        assert!(!sql.contains("convert(varchar(255), \"notes\")"));
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
        )
        .expect("valid query");

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
        )
        .expect("valid query");

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
        )
        .expect("valid query");

        assert!(sql.contains("set rowcount 11"));
        assert!(sql.contains("\"created_at\" < '2026-08-14 10:00:00'"));
        assert!(sql.contains("\"created_at\" = '2026-08-14 10:00:00'"));
        assert!(sql.contains("\"id\" > 42"));
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
            &[
                ("name".to_owned(), "varchar".to_owned()),
                ("code".to_owned(), "varchar".to_owned()),
            ],
            &[],
        )
        .expect("valid query");

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
        )
        .expect("valid query");

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
    fn resolves_alias_types_to_base_types_in_metadata_queries() {
        let object = DbObject {
            owner: "dbo".to_owned(),
            name: "orders".to_owned(),
            kind: ObjectKind::Table,
        };

        let columns_sql = table_columns(&object);
        assert!(columns_sql.contains("rtrim(t.name)"));
        assert!(columns_sql.contains("c.usertype = t.usertype"));

        let metadata_sql = table_metadata(&object);
        assert!(metadata_sql.contains(
            "(select a.usertype, isnull((select max(d.local_type_name) from sybsystemprocs.dbo.spt_datatype_info d where d.ss_dtype = a.type), a.name) name from systypes a) t"
        ));
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
