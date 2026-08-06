# ase-tui

TUI extensible en Rust para administrar **SAP/Sybase ASE** mediante `isql` y T-SQL. La interfaz usa Ratatui, Crossterm y un editor modal estilo nvim construido sobre `ratatui-textarea`.

> Estado: MVP implementado y base arquitectónica. Permite navegar conexiones, bases, tablas, procedimientos, funciones y vistas; consultar definiciones; previsualizar tablas; ejecutar T-SQL; editar SP/funciones/vistas; y preparar modificaciones de datos con confirmación. La edición de celdas tipo spreadsheet queda para una fase posterior.

## Características incluidas

- Varias conexiones definidas en `connections.toml`.
- Recarga de perfiles sin reiniciar con `R`.
- Backend desacoplado mediante el trait `DatabaseBackend`.
- Primer backend: SAP ASE a través del ejecutable `isql`.
- Consultas en un worker separado para no bloquear el render.
- Explorador de:
  - bases de datos;
  - tablas;
  - procedimientos almacenados;
  - funciones escalares (`sysobjects.type = 'SF'`);
  - vistas.
- Lectura de definiciones desde `syscomments`.
- Vista informativa del esquema de tablas desde `syscolumns` y `systypes`.
- Preview de hasta 100 registros.
- Editor modal nvim-like con NORMAL, INSERT y VISUAL.
- Conversión automática de `CREATE PROCEDURE/FUNCTION/VIEW` a `CREATE OR REPLACE` al editar.
- Panel inferior con el resultado de ASE mientras el editor permanece abierto.
- Perfil de conexión `RO` o `RW`.
- Confirmación obligatoria antes de DDL/DML.
- Plantilla de edición de datos con `begin tran` y `rollback tran` por defecto.
- Última tecla mostrada al extremo derecho de la barra de estado.
- Cursor de barra en INSERT y bloque en los demás modos.

## Requisitos

- Rust 1.88 o superior.
- SAP ASE 16.x recomendado.
- SAP Open Client/SDK con `isql` disponible.
- Acceso de red al servidor ASE.
- Recomendado: credenciales guardadas con `aseuserstore`.

Comprueba primero que esto funciona fuera de la TUI:

```bash
isql -k ase_dev
```

La sintaxis exacta para crear la clave depende de la versión de SAP Open Client, pero normalmente se configura con `aseuserstore` indicando clave, usuario, servidor y contraseña.

## Configuración rápida

Copia el ejemplo:

```powershell
Copy-Item connections.example.toml connections.toml
```

Perfil recomendado:

```toml
[[connections]]
name = "ASE desarrollo"
backend = "sybase_isql"
isql_path = "isql"
userstore_key = "ase_dev"
database = "master"
charset = "utf8"
allow_writes = false
extra_args = []
```

Alternativa con contraseña en variable de entorno:

```toml
[[connections]]
name = "ASE local"
backend = "sybase_isql"
isql_path = "C:/SAP/OCS-16_0/bin/isql.exe"
server = "ASE_LOCAL"
username = "usuario"
password_env = "ASE_LOCAL_PASSWORD"
database = "master"
allow_writes = true
```

En PowerShell:

```powershell
$env:ASE_LOCAL_PASSWORD = "tu-password"
cargo run --release
```

La alternativa `password_env` termina pasando `-P` al proceso `isql`; por seguridad, usa `userstore_key` siempre que sea posible.

También puedes colocar el archivo en:

- Windows: `%APPDATA%\ase-tui\connections.toml`
- Linux: `~/.config/ase-tui/connections.toml`
- macOS: `~/Library/Application Support/ase-tui/connections.toml`

O indicar una ruta explícita:

```powershell
$env:ASE_TUI_CONFIG = "C:\ruta\connections.toml"
```

## Ejecutar

```bash
cargo run --release
```

Para compilar el binario:

```bash
cargo build --release
```

## Controles

### Navegador

| Tecla | Acción |
|---|---|
| `h` / `l`, `Tab` / `Shift+Tab` | Cambiar panel |
| `j` / `k` | Mover selección o scroll del contenido |
| `g` / `G` | Inicio / final |
| `Enter` | Activar conexión, cargar objetos o abrir definición |
| `c` | Probar conexión |
| `r` | Recargar el panel actual |
| `R` | Volver a leer `connections.toml` |
| `p` | Previsualizar hasta 100 filas de una tabla |
| `e` | Editar procedimiento, función o vista |
| `E` | Abrir plantilla transaccional para editar datos |
| `:` | Abrir editor de consulta T-SQL |
| `?` | Ayuda |
| `q` | Salir |

### Editor

| Tecla | Acción |
|---|---|
| `i`, `a`, `A`, `I` | Entrar a INSERT |
| `Esc` | Volver a NORMAL; desde NORMAL solicita cerrar |
| `h`, `j`, `k`, `l` | Mover cursor |
| `w`, `b` | Palabra siguiente / anterior |
| `0`, `$` | Inicio / final de línea |
| `gg`, `G` | Inicio / final del archivo |
| `o`, `O` | Crear línea debajo / arriba |
| `x` | Borrar carácter siguiente |
| `dd`, `yy`, `p` | Cortar línea, copiar línea, pegar |
| `v` | VISUAL |
| `u`, `Ctrl+r` | Deshacer / rehacer |
| `Ctrl+l` | Insertar línea al final y entrar a INSERT |
| `Ctrl+s` | Ejecutar consulta o guardar DDL |

## Seguridad de escritura

Cada perfil comienza idealmente con:

```toml
allow_writes = false
```

En modo `RO`, la aplicación bloquea de forma conservadora palabras asociadas con escritura, entre ellas `ALTER`, `CREATE`, `UPDATE`, `DELETE`, `INSERT`, `DROP`, `TRUNCATE`, `EXEC` y transacciones.

Para habilitar cambios:

```toml
allow_writes = true
```

Aun en `RW`, la TUI muestra una confirmación antes de ejecutar operaciones sensibles. La detección actual es léxica y deliberadamente conservadora; no sustituye permisos mínimos en ASE, auditoría, respaldos ni revisión de scripts.

La plantilla de edición de datos usa `rollback tran` por defecto:

```sql
begin tran

update "dbo"."mi_tabla"
   set columna = valor
 where condicion_unica = valor

rollback tran
```

Cambia a `commit tran` solamente después de revisar el `WHERE` y el resultado.

## Arquitectura

```text
src/
├── main.rs                 terminal y event loop
├── app.rs                  máquina de estados y acciones
├── config.rs               perfiles y recarga de conexiones
├── worker.rs               ejecución de BD fuera del hilo de UI
├── editor/
│   ├── mod.rs
│   └── vim.rs              NORMAL / INSERT / VISUAL
├── ui/
│   └── mod.rs              layout y overlays
└── db/
    ├── mod.rs              fábrica de backends
    ├── backend.rs          trait DatabaseBackend
    ├── models.rs
    └── sybase/
        ├── mod.rs
        ├── isql.rs         adaptador del proceso isql
        └── queries.rs      catálogo ASE y T-SQL
```

La UI no conoce los argumentos de `isql` ni las tablas de sistema de ASE. Para agregar PostgreSQL, SQL Server u otro motor, crea un nuevo adaptador que implemente `DatabaseBackend`, amplía la fábrica de `db/mod.rs` y añade sus campos de configuración.

## Límites actuales

- El preview de tablas muestra la salida textual de `isql`, no una grilla editable.
- La edición de datos se realiza con T-SQL protegido.
- No hay cancelación de un proceso `isql` que ya está ejecutándose.
- No existe todavía historial persistente de consultas.
- Las funciones reconocidas inicialmente son las de tipo `SF` en `sysobjects`.
- La edición directa usa `CREATE OR REPLACE`, disponible en ASE 16; para versiones anteriores habrá que añadir una estrategia `DROP/CREATE`.
- El parser de errores detecta patrones comunes de ASE; puede necesitar ajustes según idioma y versión del cliente.
- La reconstrucción de objetos depende de que el usuario tenga acceso a `syscomments`.

## Siguientes iteraciones recomendadas

1. Modelo tabular estructurado y editor de celda que genere `UPDATE` usando PK.
2. Búsqueda incremental `/` en objetos y dentro del editor.
3. Cancelación y timeout por consulta.
4. Historial local y favoritos.
5. Diff antes de aplicar DDL.
6. Autocompletado con catálogo de tablas, columnas y procedimientos.
7. Backends adicionales bajo el mismo trait.
8. Tests de integración contra un contenedor o ambiente ASE de pruebas.

## Pruebas

```bash
cargo test
```

Las pruebas incluidas cubren el clasificador conservador de escrituras, la conversión `CREATE` → `CREATE OR REPLACE`, navegación acotada y escape básico de identificadores/literales.
