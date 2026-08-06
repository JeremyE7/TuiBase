use std::thread;

use crossbeam_channel::{Receiver, Sender, unbounded};

use crate::{
    config::ConnectionProfile,
    db::{
        backend::DatabaseBackend,
        backend_for,
        models::{DbObject, ObjectKind, SqlOutput, TablePreview},
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
    ExecuteSql {
        request_id: u64,
        connection_index: usize,
        database: String,
        sql: String,
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
    SqlExecuted {
        request_id: u64,
        connection_index: usize,
        database: String,
        result: Result<SqlOutput, String>,
    },
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
        WorkerRequest::ExecuteSql {
            request_id,
            connection_index,
            database,
            sql,
            profile,
        } => {
            let result = with_backend(&profile, |backend| {
                backend.execute(&profile, &database, &sql)
            });
            WorkerResponse::SqlExecuted {
                request_id,
                connection_index,
                database,
                result,
            }
        }
    }
}

fn with_backend<T>(
    profile: &ConnectionProfile,
    operation: impl FnOnce(&dyn DatabaseBackend) -> anyhow::Result<T>,
) -> Result<T, String> {
    let backend = backend_for(profile).map_err(|error| format!("{error:#}"))?;
    operation(backend.as_ref()).map_err(|error| format!("{error:#}"))
}
