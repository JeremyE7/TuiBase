use std::thread;

use crossbeam_channel::{Receiver, Sender, unbounded};

use crate::{
    catalog::{CachedConnection, CachedDatabase, CachedObject, cacheable_kinds},
    config::ConnectionProfile,
    db::{
        backend::DatabaseBackend,
        backend_for,
        models::{DbObject, ObjectKind, SqlOutput, TableMetadata, TablePage, TablePreview},
        query::TableQuery,
    },
};

#[derive(Debug)]
pub enum WorkerRequest {
    TestConnection {
        request_id: u64,
        connection_index: usize,
        profile: ConnectionProfile,
    },
    LoadDatabases {
        request_id: u64,
        connection_index: usize,
        profile: ConnectionProfile,
    },
    LoadObjects {
        request_id: u64,
        connection_index: usize,
        database: String,
        kind: ObjectKind,
        profile: ConnectionProfile,
    },
    LoadDefinition {
        request_id: u64,
        connection_index: usize,
        database: String,
        object: DbObject,
        profile: ConnectionProfile,
    },
    PreviewTable {
        request_id: u64,
        connection_index: usize,
        database: String,
        object: DbObject,
        row_limit: usize,
        profile: ConnectionProfile,
    },
    LoadTableMetadata {
        request_id: u64,
        connection_index: usize,
        database: String,
        object: DbObject,
        profile: ConnectionProfile,
    },
    QueryTable {
        request_id: u64,
        connection_index: usize,
        database: String,
        object: DbObject,
        query: TableQuery,
        profile: ConnectionProfile,
    },
    ExecuteSql {
        request_id: u64,
        connection_index: usize,
        database: String,
        sql: String,
        profile: ConnectionProfile,
    },
    RefreshCatalog {
        request_id: u64,
        connection_index: usize,
        profile: ConnectionProfile,
    },
}

#[derive(Debug)]
pub enum WorkerResponse {
    ConnectionTested {
        request_id: u64,
        connection_index: usize,
        result: Result<String, String>,
    },
    DatabasesLoaded {
        request_id: u64,
        connection_index: usize,
        result: Result<Vec<String>, String>,
    },
    ObjectsLoaded {
        request_id: u64,
        connection_index: usize,
        database: String,
        kind: ObjectKind,
        result: Result<Vec<DbObject>, String>,
    },
    DefinitionLoaded {
        request_id: u64,
        connection_index: usize,
        database: String,
        object: DbObject,
        result: Result<String, String>,
    },
    TablePreviewed {
        request_id: u64,
        connection_index: usize,
        database: String,
        object: DbObject,
        result: Result<TablePreview, String>,
    },
    TableMetadataLoaded {
        request_id: u64,
        connection_index: usize,
        database: String,
        object: DbObject,
        result: Result<TableMetadata, String>,
    },
    TablePageLoaded {
        request_id: u64,
        connection_index: usize,
        database: String,
        object: DbObject,
        result: Result<TablePage, String>,
    },
    SqlExecuted {
        request_id: u64,
        connection_index: usize,
        database: String,
        result: Result<SqlOutput, String>,
        elapsed_ms: u64,
    },
    CatalogRefreshed {
        request_id: u64,
        connection_index: usize,
        result: Result<CatalogRefreshResult, String>,
    },
}

#[derive(Debug)]
pub struct CatalogRefreshResult {
    pub connection: CachedConnection,
    pub skipped_databases: Vec<String>,
    pub skipped_kinds: Vec<String>,
}

pub struct WorkerHandle {
    pub requests: Sender<WorkerRequest>,
    pub responses: Receiver<WorkerResponse>,
}

pub fn spawn_worker() -> WorkerHandle {
    let (request_tx, request_rx) = unbounded();
    let (response_tx, response_rx) = unbounded();

    thread::spawn(move || worker_loop(request_rx, response_tx));

    WorkerHandle {
        requests: request_tx,
        responses: response_rx,
    }
}

fn worker_loop(requests: Receiver<WorkerRequest>, responses: Sender<WorkerResponse>) {
    while let Ok(request) = requests.recv() {
        let response = execute_request(request);
        if responses.send(response).is_err() {
            break;
        }
    }
}

fn execute_request(request: WorkerRequest) -> WorkerResponse {
    match request {
        WorkerRequest::TestConnection {
            request_id,
            connection_index,
            profile,
        } => WorkerResponse::ConnectionTested {
            request_id,
            connection_index,
            result: with_backend(&profile, |backend| backend.test_connection(&profile)),
        },
        WorkerRequest::LoadDatabases {
            request_id,
            connection_index,
            profile,
        } => WorkerResponse::DatabasesLoaded {
            request_id,
            connection_index,
            result: with_backend(&profile, |backend| backend.list_databases(&profile)),
        },
        WorkerRequest::LoadObjects {
            request_id,
            connection_index,
            database,
            kind,
            profile,
        } => {
            let result = with_backend(&profile, |backend| {
                backend.list_objects(&profile, &database, kind)
            });
            WorkerResponse::ObjectsLoaded {
                request_id,
                connection_index,
                database,
                kind,
                result,
            }
        }
        WorkerRequest::LoadDefinition {
            request_id,
            connection_index,
            database,
            object,
            profile,
        } => {
            let result = with_backend(&profile, |backend| {
                backend.object_definition(&profile, &database, &object)
            });
            WorkerResponse::DefinitionLoaded {
                request_id,
                connection_index,
                database,
                object,
                result,
            }
        }
        WorkerRequest::PreviewTable {
            request_id,
            connection_index,
            database,
            object,
            row_limit,
            profile,
        } => {
            let result = with_backend(&profile, |backend| {
                backend.preview_table(&profile, &database, &object, row_limit)
            });
            WorkerResponse::TablePreviewed {
                request_id,
                connection_index,
                database,
                object,
                result,
            }
        }
        WorkerRequest::LoadTableMetadata {
            request_id,
            connection_index,
            database,
            object,
            profile,
        } => {
            let result = with_backend(&profile, |backend| {
                backend.table_metadata(&profile, &database, &object)
            });
            WorkerResponse::TableMetadataLoaded {
                request_id,
                connection_index,
                database,
                object,
                result,
            }
        }
        WorkerRequest::QueryTable {
            request_id,
            connection_index,
            database,
            object,
            query,
            profile,
        } => {
            let result = with_backend(&profile, |backend| {
                backend.query_table(&profile, &database, &object, &query)
            });
            WorkerResponse::TablePageLoaded {
                request_id,
                connection_index,
                database,
                object,
                result,
            }
        }
        WorkerRequest::ExecuteSql {
            request_id,
            connection_index,
            database,
            sql,
            profile,
        } => {
            let start = std::time::Instant::now();
            let mut result = with_backend(&profile, |backend| {
                backend.execute(&profile, &database, &sql)
            });
            let elapsed_ms = start.elapsed().as_millis() as u64;
            if let Ok(output) = result.as_mut() {
                output.elapsed_ms = elapsed_ms;
            }
            WorkerResponse::SqlExecuted {
                request_id,
                connection_index,
                database,
                result,
                elapsed_ms,
            }
        }
        WorkerRequest::RefreshCatalog {
            request_id,
            connection_index,
            profile,
        } => {
            let result = with_backend(&profile, |backend| {
                let databases = backend.list_databases(&profile)?;
                let mut cached_databases = Vec::with_capacity(databases.len());
                let mut skipped_databases = Vec::new();
                let mut skipped_kinds = Vec::new();

                for database in databases {
                    let mut objects = Vec::new();
                    for kind in cacheable_kinds() {
                        match backend.list_objects(&profile, &database, kind) {
                            Ok(listed) => {
                                objects.extend(listed.iter().map(CachedObject::from_db_object));
                            }
                            Err(error) if is_permission_error(&error) => {
                                skipped_kinds.push(format!("{database}/{kind}"));
                            }
                            Err(error) => {
                                skipped_databases.push(format!("{database}: {error:#}"));
                                objects.clear();
                                break;
                            }
                        }
                    }

                    if skipped_databases
                        .iter()
                        .any(|item| item.starts_with(&format!("{database}:")))
                    {
                        continue;
                    }

                    cached_databases.push(CachedDatabase {
                        name: database,
                        objects,
                    });
                }

                Ok(CatalogRefreshResult {
                    connection: CachedConnection::from_objects(&profile, cached_databases),
                    skipped_databases,
                    skipped_kinds,
                })
            });

            WorkerResponse::CatalogRefreshed {
                request_id,
                connection_index,
                result,
            }
        }
    }
}

fn is_permission_error(error: &anyhow::Error) -> bool {
    let message = format!("{error:#}").to_ascii_lowercase();
    message.contains("10351")
        || message.contains("permission")
        || message.contains("not authorized")
        || message.contains("no access")
}

fn with_backend<T>(
    profile: &ConnectionProfile,
    operation: impl FnOnce(&dyn DatabaseBackend) -> anyhow::Result<T>,
) -> Result<T, String> {
    let backend = backend_for(profile).map_err(|error| format!("{error:#}"))?;
    operation(backend.as_ref()).map_err(|error| format!("{error:#}"))
}
