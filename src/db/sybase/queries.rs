use crate::db::models::{DbObject, ObjectKind};

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

#[cfg(test)]
mod tests {
    use super::{qualified_identifier, string_literal};

    #[test]
    fn escapes_sql_literals() {
        assert_eq!(string_literal("O'Brien"), "O''Brien");
    }

    #[test]
    fn quotes_qualified_identifiers() {
        assert_eq!(qualified_identifier("dbo", "order"), "\"dbo\".\"order\"");
    }
}
