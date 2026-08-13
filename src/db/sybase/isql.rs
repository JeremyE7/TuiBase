use std::{
    io::Write,
    process::{Command, Stdio},
};

use anyhow::{Context, Result, bail};

use crate::{
    config::ConnectionProfile,
    db::{
        backend::DatabaseBackend,
        models::{
            ColumnMetadata, DbObject, IndexMetadata, ObjectKind, SqlOutput, TableMetadata,
            TablePage, TablePreview,
        },
        query::{PageCursor, TableQuery},
    },
};

use super::queries;

pub struct IsqlBackend;

impl IsqlBackend {
    fn run_sql(&self, profile: &ConnectionProfile, database: &str, sql: &str) -> Result<SqlOutput> {
        let mut command = Command::new(&profile.isql_path);
        // Git Bash puede definir LANG como es_ES.UTF-8, pero SAP Open Client
        // solo acepta locales registrados en %SYBASE%\locales\locales.dat.
        // Al eliminarlas, isql utiliza la entrada "default".
        command
            .env_remove("LANG")
            .env_remove("LC_ALL")
            .env_remove("LC_CTYPE");
        command
            .arg("-n")
            .arg("-b")
            .arg("-w")
            .arg("65535")
            .arg("-s")
            .arg("|")
            .arg("-D")
            .arg(database);

        if let Some(charset) = &profile.charset {
            command.arg("-J").arg(charset);
        }

        if let Some(key) = &profile.userstore_key {
            command.arg("-k").arg(key);
        } else {
            if let Some(server) = &profile.server {
                command.arg("-S").arg(server);
            }
            let username = profile
                .username
                .as_deref()
                .context("Falta username en el perfil")?;
            let password = profile
                .password()?
                .context("Falta password_env en el perfil")?;
            command.arg("-U").arg(username).arg("-P").arg(password);
        }

        command.args(&profile.extra_args);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = command.spawn().with_context(|| {
            format!(
                "No se pudo ejecutar '{}'. Revisa isql_path y el PATH del sistema",
                profile.isql_path
            )
        })?;

        let script = format!("{sql}\ngo\nquit\n");
        child
            .stdin
            .take()
            .context("No se pudo abrir stdin de isql")?
            .write_all(script.as_bytes())
            .context("No se pudo enviar T-SQL a isql")?;

        let output = child
            .wait_with_output()
            .context("No se pudo esperar la salida de isql")?;
        let stdout = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
        let stderr = String::from_utf8_lossy(&output.stderr).replace("\r\n", "\n");
        let sql_error = looks_like_ase_error(&stdout) || looks_like_ase_error(&stderr);

        Ok(SqlOutput {
            stdout,
            stderr,
            success: output.status.success() && !sql_error,
        })
    }

    fn structured_rows(
        &self,
        profile: &ConnectionProfile,
        database: &str,
        sql: &str,
    ) -> Result<Vec<Vec<String>>> {
        self.structured_rows_with_empty(profile, database, sql, false)
    }

    fn structured_rows_with_empty(
        &self,
        profile: &ConnectionProfile,
        database: &str,
        sql: &str,
        allow_empty: bool,
    ) -> Result<Vec<Vec<String>>> {
        let output = self.run_sql(profile, database, sql)?;

        if !output.success {
            bail!("isql devolvió un error:\n{}", output.combined());
        }

        let rows = output
            .stdout
            .lines()
            .filter_map(|line| {
                marker_payload(line, queries::row_marker()).map(|row| {
                    row.trim_end_matches('|')
                        .split('|')
                        .map(|value| value.trim().to_owned())
                        .collect::<Vec<_>>()
                })
            })
            .collect::<Vec<_>>();

        if rows.is_empty() && !allow_empty {
            bail!(
                "La consulta respondió, pero no se pudo interpretar la salida.\n\nSTDOUT:\n{}\n\nSTDERR:\n{}",
                output.stdout,
                output.stderr
            );
        }

        Ok(rows)
    }
}

impl DatabaseBackend for IsqlBackend {
    fn test_connection(&self, profile: &ConnectionProfile) -> Result<String> {
        let rows = self.structured_rows(
            profile,
            profile.initial_database(),
            &queries::test_connection(),
        )?;
        let Some(row) = rows.first() else {
            bail!("La conexión respondió, pero no devolvió @@servername/db_name()")
        };
        Ok(row.join(" / "))
    }

    fn list_databases(&self, profile: &ConnectionProfile) -> Result<Vec<String>> {
        let rows = self.structured_rows(profile, "master", &queries::list_databases())?;
        Ok(rows
            .into_iter()
            .filter_map(|row| row.into_iter().next())
            .filter(|name| !name.is_empty())
            .collect())
    }

    fn list_objects(
        &self,
        profile: &ConnectionProfile,
        database: &str,
        kind: ObjectKind,
    ) -> Result<Vec<DbObject>> {
        let rows =
            self.structured_rows_with_empty(profile, database, &queries::list_objects(kind), true)?;
        Ok(rows
            .into_iter()
            .filter_map(|row| {
                if row.len() < 2 {
                    return None;
                }
                Some(DbObject {
                    owner: row[0].clone(),
                    name: row[1].clone(),
                    kind,
                })
            })
            .collect())
    }

    fn object_definition(
        &self,
        profile: &ConnectionProfile,
        database: &str,
        object: &DbObject,
    ) -> Result<String> {
        let output = self.run_sql(profile, database, &queries::object_definition(object))?;
        if !output.success {
            bail!("No se pudo leer la definición:\n{}", output.combined());
        }

        if object.kind == ObjectKind::Table {
            let rows = output
                .stdout
                .lines()
                .filter_map(|line| {
                    marker_payload(line, queries::row_marker()).map(|row| {
                        row.trim_end_matches('|')
                            .split('|')
                            .map(str::trim)
                            .collect::<Vec<_>>()
                    })
                })
                .collect::<Vec<_>>();
            let mut ddl = format!(
                "-- Esquema informativo de {}\n-- No incluye índices, constraints, defaults ni triggers.\n-- Para modificar la tabla, abre el editor T-SQL con ':'.\n\n",
                object.qualified_name()
            );
            ddl.push_str("set quoted_identifier on\n\n");
            ddl.push_str(&format!(
                "create table {} (\n",
                queries::qualified_identifier(&object.owner, &object.name)
            ));
            let valid_rows = rows.iter().filter(|row| row.len() >= 6).collect::<Vec<_>>();
            for (index, row) in valid_rows.iter().enumerate() {
                let comma = if index + 1 == valid_rows.len() {
                    ""
                } else {
                    ","
                };
                let column_type = format_column_type(row[1], row[2], row[3], row[4]);
                ddl.push_str(&format!(
                    "    {} {} {}{}\n",
                    quote_display_identifier(row[0]),
                    column_type,
                    row[5],
                    comma
                ));
            }
            ddl.push_str(")\n");
            return Ok(ddl);
        }

        let mut chunks = Vec::new();
        let mut current: Option<String> = None;
        for line in output.stdout.lines() {
            let trimmed = line.trim_end();
            if let Some(payload) = marker_payload(trimmed, queries::text_marker()) {
                if let Some(chunk) = current.take() {
                    chunks.push(clean_text_chunk(chunk));
                }
                let text = payload
                    .split_once('|')
                    .map(|(_, text)| text)
                    .unwrap_or(payload);
                current = Some(text.to_owned());
            } else if let Some(chunk) = current.as_mut() {
                if !trimmed.starts_with('(') || !trimmed.ends_with("affected)") {
                    chunk.push('\n');
                    chunk.push_str(trimmed);
                }
            }
        }
        if let Some(chunk) = current {
            chunks.push(clean_text_chunk(chunk));
        }

        let definition = chunks.concat();
        if definition.trim().is_empty() {
            bail!("No se encontró texto para {}", object.qualified_name());
        }
        Ok(definition)
    }

    fn preview_table(
        &self,
        profile: &ConnectionProfile,
        database: &str,
        object: &DbObject,
        row_limit: usize,
    ) -> Result<TablePreview> {
        let columns = self
            .structured_rows(profile, database, &queries::table_columns(object))?
            .into_iter()
            .filter_map(|row| {
                if row.len() < 2 {
                    None
                } else {
                    Some((row[0].clone(), row[1].clone()))
                }
            })
            .collect::<Vec<_>>();
        if columns.is_empty() {
            bail!("La tabla no contiene columnas");
        }
        let output = self.run_sql(
            profile,
            database,
            &queries::preview_table(object, row_limit, &columns),
        )?;

        if !output.success {
            bail!("No se pudo previsualizar la tabla: \n{}", output.combined())
        }

        parse_table_preview(&output.stdout)
    }

    fn table_metadata(
        &self,
        profile: &ConnectionProfile,
        database: &str,
        object: &DbObject,
    ) -> Result<TableMetadata> {
        let output = self.run_sql(profile, database, &queries::table_metadata(object))?;
        if !output.success {
            bail!(
                "No se pudo leer la metadata de la tabla: {}",
                output.combined()
            );
        }

        let rows = output
            .stdout
            .lines()
            .filter_map(|line| {
                marker_payload(line, queries::row_marker()).map(|row| {
                    row.trim_end_matches('|')
                        .split('|')
                        .map(|value| value.trim().to_owned())
                        .collect::<Vec<_>>()
                })
            })
            .collect::<Vec<_>>();
        parse_table_metadata(
            queries::table_identifier(object),
            rows,
            parse_sp_helpindex(&output.stdout),
        )
    }

    fn query_table(
        &self,
        profile: &ConnectionProfile,
        database: &str,
        object: &DbObject,
        query: &TableQuery,
    ) -> Result<TablePage> {
        if query.page.limit == 0 {
            bail!("El límite de la página debe ser mayor que cero");
        }
        if matches!(query.page.cursor.as_ref(), Some(PageCursor::Keyset(_))) {
            bail!("El backend ASE todavía no admite cursores keyset");
        }

        let offset = query.page.cursor.as_ref().map_or(0, cursor_offset);
        let offset =
            usize::try_from(offset).context("El offset de la página es demasiado grande")?;
        let metadata = self.table_metadata(profile, database, object)?;
        let columns = metadata
            .columns
            .iter()
            .map(|column| (column.name.clone(), column.data_type.clone()))
            .collect::<Vec<_>>();
        let output = self.run_sql(
            profile,
            database,
            &queries::query_table(object, query, &columns),
        )?;

        if !output.success {
            bail!("No se pudo consultar la tabla: {}", output.combined());
        }

        let page = parse_table_preview(&output.stdout)?;
        let mut rows = page.rows.into_iter().skip(offset).collect::<Vec<_>>();
        let has_more = rows.len() > query.page.limit;
        if has_more {
            rows.truncate(query.page.limit);
        }
        let next_cursor =
            has_more.then(|| PageCursor::Offset((offset as u64).saturating_add(rows.len() as u64)));

        TablePage::new(page.columns, rows, next_cursor, has_more, None).map_err(Into::into)
    }

    fn execute(&self, profile: &ConnectionProfile, database: &str, sql: &str) -> Result<SqlOutput> {
        self.run_sql(profile, database, sql)
    }
}

fn looks_like_ase_error(output: &str) -> bool {
    output.lines().any(|line| {
        let line = line.trim_start();
        line.starts_with("Msg ")
            || line.contains(", Level ")
            || line.starts_with("CT-LIBRARY error")
    })
}

fn quote_display_identifier(value: &str) -> String {
    return format!("\"{}\"", value.replace('\"', "\"\""));
}

fn format_column_type(name: &str, length: &str, precision: &str, scale: &str) -> String {
    match name.to_ascii_lowercase().as_str() {
        "char" | "varchar" | "nchar" | "nvarchar" | "binary" | "varbinary" | "unichar"
        | "univarchar" => format!("{name}({length})"),
        "numeric" | "decimal" => format!("{name}({precision},{scale})"),
        _ => name.to_owned(),
    }
}

fn parse_table_preview(table: &str) -> Result<TablePreview> {
    let mut columns: Option<Vec<String>> = None;
    let mut rows = Vec::new();

    for line in table.lines() {
        if let Some(payload) = marker_payload(line, queries::header_marker()) {
            columns = Some(parse_fields(payload));
        } else if let Some(payload) = marker_payload(line, queries::row_marker()) {
            rows.push(parse_fields(payload));
        }
    }

    let columns = columns.context("La respuesta no tiene columnas")?;

    if columns.is_empty() {
        bail!("La tabla no contiene columnas");
    }

    if let Some((index, row)) = rows
        .iter()
        .enumerate()
        .find(|(_, row)| row.len() != columns.len())
    {
        bail!(
            "La fila {} tiene {} valores, pero se esperaban {}",
            index + 1,
            row.len(),
            columns.len()
        );
    }

    Ok(TablePreview { columns, rows })
}

fn parse_optional_u32(value: &str) -> Option<u32> {
    value.parse().ok()
}

fn parse_table_metadata(
    identifier: crate::db::models::TableIdentifier,
    rows: Vec<Vec<String>>,
    indexes: Vec<IndexMetadata>,
) -> Result<TableMetadata> {
    let mut columns = Vec::new();

    for row in rows {
        if row.len() >= 8 && row[0] == "COLUMN" {
            columns.push(ColumnMetadata {
                name: row[1].clone(),
                data_type: row[2].clone(),
                length: parse_optional_u32(&row[3]),
                precision: parse_optional_u32(&row[4]),
                scale: parse_optional_u32(&row[5]),
                nullable: row[6] == "1",
                ordinal_position: row[7].parse().unwrap_or(columns.len() + 1),
            });
        }
    }

    if columns.is_empty() {
        bail!("La tabla no contiene columnas");
    }

    Ok(TableMetadata::new(identifier, columns, indexes))
}

fn parse_sp_helpindex(output: &str) -> Vec<IndexMetadata> {
    output
        .lines()
        .filter_map(|line| {
            let fields = line
                .split('|')
                .map(str::trim)
                .filter(|field| !field.is_empty())
                .collect::<Vec<_>>();
            if fields.len() < 3
                || fields[0] == "Object has the following indexes"
                || fields[0] == "index_name"
                || fields[0].starts_with('(')
                || fields[0].starts_with("return status")
            {
                return None;
            }

            let description = fields.get(2)?.to_ascii_lowercase();
            if !description.contains("clustered") && !description.contains("nonclustered") {
                return None;
            }

            Some(IndexMetadata {
                name: fields[0].to_owned(),
                columns: fields[1]
                    .split(',')
                    .map(str::trim)
                    .filter(|column| !column.is_empty())
                    .map(ToOwned::to_owned)
                    .collect(),
                is_unique: description.contains("unique"),
                is_primary: false,
            })
        })
        .collect()
}

fn cursor_offset(cursor: &PageCursor) -> u64 {
    match cursor {
        PageCursor::Offset(value) => *value,
        PageCursor::Keyset(_) => 0,
    }
}

fn parse_fields(value: &str) -> Vec<String> {
    value
        .trim_end_matches('|')
        .split('|')
        .map(str::trim)
        .map(ToOwned::to_owned)
        .collect()
}

fn marker_payload<'a>(line: &'a str, marker: &str) -> Option<&'a str> {
    let position = line.find(marker)?;
    Some(&line[position + marker.len()..])
}

fn clean_text_chunk(mut chunk: String) -> String {
    if chunk.ends_with('|') {
        chunk.pop();
    }

    while chunk.ends_with(' ') || chunk.ends_with('\t') {
        chunk.pop();
    }

    chunk
}

#[cfg(test)]
mod tests {
    use crate::db::models::{IndexMetadata, TableIdentifier};

    use super::{
        format_column_type, parse_sp_helpindex, parse_table_metadata, quote_display_identifier,
    };

    #[test]
    fn formats_only_parameterized_types() {
        assert_eq!(format_column_type("int", "4", "10", "0"), "int");
        assert_eq!(format_column_type("varchar", "50", "0", "0"), "varchar(50)");
        assert_eq!(
            format_column_type("numeric", "9", "12", "2"),
            "numeric(12,2)"
        );
    }

    #[test]
    fn quotes_display_identifiers() {
        assert_eq!(quote_display_identifier("order"), "\"order\"");
        assert_eq!(quote_display_identifier("a\"b"), "\"a\"\"b\"");
    }

    #[test]
    fn groups_index_columns_from_metadata_rows() {
        let metadata = parse_table_metadata(
            TableIdentifier::new("dbo", "orders"),
            vec![
                vec![
                    "COLUMN".to_owned(),
                    "id".to_owned(),
                    "int".to_owned(),
                    "4".to_owned(),
                    "10".to_owned(),
                    "0".to_owned(),
                    "0".to_owned(),
                    "1".to_owned(),
                ],
                vec![
                    "COLUMN".to_owned(),
                    "created_at".to_owned(),
                    "datetime".to_owned(),
                    "8".to_owned(),
                    "0".to_owned(),
                    "0".to_owned(),
                    "1".to_owned(),
                    "2".to_owned(),
                ],
            ],
            vec![IndexMetadata {
                name: "ix_orders".to_owned(),
                columns: vec!["id".to_owned(), "created_at".to_owned()],
                is_unique: true,
                is_primary: false,
            }],
        )
        .expect("column metadata is present");

        assert_eq!(metadata.indexes[0].columns, ["id", "created_at"]);
    }

    #[test]
    fn ignores_empty_index_columns() {
        let metadata = parse_table_metadata(
            TableIdentifier::new("dbo", "orders"),
            vec![vec![
                "COLUMN".to_owned(),
                "id".to_owned(),
                "int".to_owned(),
                "4".to_owned(),
                "10".to_owned(),
                "0".to_owned(),
                "0".to_owned(),
                "1".to_owned(),
            ]],
            vec![IndexMetadata {
                name: "ix_orders".to_owned(),
                columns: vec!["id".to_owned()],
                is_unique: false,
                is_primary: false,
            }],
        )
        .expect("column metadata is present");

        assert_eq!(metadata.indexes[0].columns, ["id"]);
    }

    #[test]
    fn parses_sp_helpindex_rows() {
        let output = "Object has the following indexes\n|ncsysobjects| name, uid, schemaid|nonclustered, unique| 0|\n|csysobjects| id|clustered, unique| 0|\n(2 rows affected)\n";

        assert_eq!(
            parse_sp_helpindex(output),
            vec![
                IndexMetadata {
                    name: "ncsysobjects".to_owned(),
                    columns: vec!["name".to_owned(), "uid".to_owned(), "schemaid".to_owned()],
                    is_unique: true,
                    is_primary: false,
                },
                IndexMetadata {
                    name: "csysobjects".to_owned(),
                    columns: vec!["id".to_owned()],
                    is_unique: true,
                    is_primary: false,
                },
            ]
        );
    }
}
