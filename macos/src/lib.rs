//! Lo propio de macOS de Alfredo Caja de escritorio (F10 del plan de paridad con Go,
//! docs/paridad-go/PRD-F10-apple.md, T10.2–T10.8).
//!
//! El escritorio es la app Tauri v2 de F9 (`apps/desktop`). Este crate junta lo que sólo existe en
//! el Mac para que el crate de Tauri lo llame detrás de `#[cfg(target_os = "macos")]`:
//!
//! - [`cups`]: impresión directa por CUPS (cola *raw* para ESC/POS, PDF/A4 a una cola con driver),
//!   lectura del estado de las colas y creación de una cola *raw* para una térmica USB (T10.3–T10.4).
//! - [`serial`]: puertos serie del Mac (`/dev/cu.*`) (T10.3–T10.4).
//! - [`power`]: que el Mac no se duerma ni frene la app (*assertions* de IOKit, App Nap) (T10.5).
//! - [`keychain`]: la sesión de la persona en el Llavero (T10.2).
//! - [`identity`]: el `deviceId` estable del Mac (T10.7).
//! - [`window`]: insignia del Dock y la decisión de traer al frente o hacer rebotar el Dock (T10.2).
//! - [`support`]: verificación del instalador de la herramienta de soporte remoto (T10.8).
//!
//! Las funciones puras (armado de argumentos, *parsers*, reglas) se prueban con `cargo test` en
//! cualquier sistema; las que tocan el sistema sólo compilan en macOS.

pub mod cups;
pub mod identity;
pub mod keychain;
pub mod power;
pub mod serial;
pub mod support;
pub mod window;

/// Nombre del servicio en el Llavero y carpeta de datos de la app (el identificador del escritorio).
pub const APP_ID: &str = "cl.asistentealfredo.caja.escritorio";
