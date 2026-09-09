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
- Editor modal nvim-like con NORMAL e INSERT; la selección visual es transitoria.
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

La guía completa está disponible dentro de la aplicación con `Ctrl+?` (en algunos terminales se recibe como `Ctrl+Shift+/`). También se puede abrir con `?` desde el navegador y la tabla.

### Ayuda, tabs y atajos globales

| Tecla | Acción |
|---|---|
| `Ctrl+?` / `Ctrl+Shift+/` | Abrir esta guía desde cualquier modo |
| `Esc` / `?` / `q` en la guía | Cerrar la guía |
| `j` / `k`, `↑` / `↓` | Desplazar la guía |
| `PgUp` / `PgDn`, `Home` / `End` | Desplazar por página o ir al inicio/final |
| `Ctrl+b` | Mostrar/ocultar sidebar |
| `Shift+H` / `Shift+L` | Cambiar a tab anterior/siguiente |
| `Ctrl+Tab` / `Ctrl+Shift+Tab` | Cambiar a tab siguiente/anterior |
| `Ctrl+h` / `Ctrl+l` | Cambiar a tab anterior/siguiente |
| `Ctrl+Backspace` | Cambiar a tab anterior |
| `Ctrl+w` | Cerrar tab activa |

### Navegador

| Tecla | Acción |
|---|---|
| `h` / `l`, `←` / `→`, `Tab` / `Shift+Tab` | Cambiar panel |
| `j` / `k`, `↑` / `↓` | Mover selección |
| `g` / `G`, `Home` / `End` | Primera/última selección |
| `Enter` | Activar conexión, cargar objetos o abrir tabla |
| `1` / `2` / `3` / `4` | Enfocar conexiones/bases/tipos/objetos |
| `r` | Recargar el panel actual |
| `R` | Volver a leer `connections.toml` |
| `c` | Probar conexión |
| `e` | Editar procedimiento, función o vista |
| `E` | Abrir editor transaccional de datos |
| `:` | Abrir editor de consulta T-SQL |
| `/` | Abrir búsqueda global |
| `F5` | Actualizar catálogo |
| `y` | Copiar contenido visible |
| `?` | Abrir ayuda |
| `q` | Salir de la aplicación |
| `Ctrl+c` | Salir inmediatamente |

### Tabla

| Tecla | Acción |
|---|---|
| `j` / `k`, `↑` / `↓` | Cambiar fila |
| `h` / `l`, `←` / `→` | Cambiar columna |
| `g` / `G`, `Home` / `End` | Primera/última fila |
| `Enter` | Ver el valor completo de la celda |
| `e` | Editar celda o abrir picker date/time |
| `i` | Mostrar/ocultar metadata |
| `c` | Buscar y enfocar una columna |
| `p` | Fijar/desfijar columna |
| `f` / `F` | Abrir/limpiar filtro |
| `o` / `O` | Abrir/limpiar ordenamiento |
| `r` | Recargar datos respetando filtro y orden |
| `v` / `V` / `Shift+V` | Selección de celdas/filas |
| `y` / `Y` | Copiar celda/selección o abrir menú de copia |
| `d` / `dd` | Marcar fila/rango o copiar y marcar fila |
| `u` | Deshacer borrador de fila o borrado marcado |
| `+` | Agregar fila nueva |
| `Shift+=` | Clonar la fila seleccionada |
| `Ctrl+s` | Abrir resumen de cambios staged |
| `/` | Búsqueda global |
| `?` | Ayuda |
| `q` / `Esc` | Salir; `Esc` también cancela el modo activo |

En los filtros, ordenamientos y búsqueda de columnas: escribir modifica la entrada, `Tab` completa, `↑/↓` cambia la sugerencia, `Enter` aplica o salta, `Ctrl+l` limpia y `Esc` cancela. En el menú de copia, `j/k` elige, `Enter` continúa y `y/n` decide si incluye cabecera.

Los modales de valor, metadata, resumen, vista previa y error usan `j/k`, `PgUp/PgDn`, `g/G` o `Home/End` para desplazarse y `Enter`/`Esc` para cerrar cuando corresponde. El picker date/time usa `Tab`/`Shift+Tab` o flechas para cambiar componente, `↑/↓` para ajustar, `Space` para alternar `NULL` y `Enter` para guardar. Las confirmaciones usan `y` para confirmar y `n`/`Enter`/`Esc` para cancelar.

### Editor SQL libre

| Tecla | Acción |
|---|---|
| `Ctrl+s` | Ejecutar consulta o guardar DDL |
| `Ctrl+Enter` | Ejecutar solo la selección visual |
| `Ctrl+d` | Asociar la consulta a la base seleccionada |
| `Ctrl+j` | Enfocar/volver de la consola de resultados |
| `Ctrl+Space` / `Ctrl+n` | Abrir autocompletado |
| `K` en NORMAL | Mostrar información bajo el cursor |
| `Ctrl+a` | Seleccionar todo el buffer |
| `Ctrl+v` | Pegar en INSERT o alternar visual por bloques |
| `Ctrl+c` en VISUAL | Copiar selección |
| `i`, `a`, `A`, `I` | Entrar a INSERT |
| `Esc` | Volver a NORMAL sin cerrar |
| `q` | Volver al navegador desde NORMAL |
| `h`, `j`, `k`, `l`, flechas | Mover cursor |
| `w` / `W`, `b` / `B`, `e` / `E` | Mover por palabras |
| `0` / `^` / `$`, `Home` / `End` | Inicio/final de línea |
| `gg` / `G`, `nG` | Inicio/final del archivo o saltar a línea `n` |
| `PgUp` / `PgDn` | Desplazamiento vertical |
| `o` / `O` | Crear línea debajo/arriba |
| `x` / `Delete`, `X` | Borrar carácter siguiente/anterior |
| `d` / `c` / `y` + movimiento | Borrar/cambiar/copiar rango |
| `dd` / `cc` / `yy` | Operar sobre líneas completas |
| `Y` | Copiar líneas completas |
| `p` / `P` | Pegar después/antes; usa clipboard como fallback |
| `u` / `Ctrl+r` | Deshacer/rehacer |
| `v` / `V` / `Ctrl+v` | Visual carácter/línea/bloque |
| `J` | Unir línea con la siguiente |
| `Ctrl+l` en INSERT | Insertar una línea al final |
| `Ctrl+w` en INSERT | Borrar palabra anterior |
| `Ctrl+u` en INSERT | Borrar hasta el inicio de línea |
| `Esc` en VISUAL | Cancelar selección |
| `y` en VISUAL | Copiar y salir |
| `d` / `x` en VISUAL | Borrar y salir |
| `c` en VISUAL | Cambiar selección e insertar |
| `I` / `A` en VISUAL por bloques | Insertar al inicio/final de cada línea |
| `1..9` | Prefijo de cantidad para movimientos/operaciones |

### Consola de resultados SQL

| Tecla | Acción |
|---|---|
| `Ctrl+j` | Enfocar consola o volver al editor |
| `Esc` | Volver al editor |
| `Ctrl+PgUp/PgDn` | Cambiar entre resultados |
| `j` / `k`, `↑` / `↓` | Navegar filas o texto |
| `h` / `l`, `←` / `→` | Navegar columnas en tablas |
| `PgUp/PgDn` | Desplazar texto o filas |
| `g` / `Home`, `G` / `End` | Inicio/final del resultado |
| `y` | Copiar línea/fila seleccionada |
| `Y` | Copiar todo el resultado |
| `Ctrl+l` | Limpiar resultado actual |
| `Ctrl+Shift+l` | Limpiar todos los resultados |

### Búsqueda y autocompletado

| Tecla | Acción |
|---|---|
| `/` | Abrir búsqueda global desde navegador/tabla |
| `Ctrl+Space` / `Ctrl+n` | Abrir autocompletado en el editor |
| `↑` / `↓` | Elegir sugerencia |
| `Tab` | Aceptar sugerencia |
| `Enter` | Navegar al elemento seleccionado |
| `Ctrl+l` | Limpiar la entrada activa |
| `Esc` | Cancelar búsqueda/autocompletado |

El encabezado del editor muestra el modo activo como `NORMAL` o `INSERT`.

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
│   └── vim.rs              NORMAL / INSERT / selección visual
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
