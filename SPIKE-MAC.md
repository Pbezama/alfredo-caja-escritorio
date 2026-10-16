# Spike de macOS (F10 T10.1) — 16-10-2026

Sobre el escritorio de F9 (rama `paridad-go-f9`, `dece3c75`), en el Mac de Pablo (Apple Silicon,
**macOS 26.5.1**, Xcode 26.5, Rust 1.99). Se borra al cerrar T10.10; lo útil pasa a `docs/DESKTOP.md`.

| # | Pregunta | Respuesta | Evidencia |
|---|---|---|---|
| 1 | ¿Carga remota e IPC con la *capability* `remote` de F9? | **Sí (carga).** La app universal abre la web staff remota (pantalla «Inicie sesión en esta estación») en WKWebView. El IPC del puente queda para la QA con sesión (T10.10). | Captura en `.context/paridad-go/f10/sim/` (mac-app); `cargo build --release` del crate de F9 compila en macOS sin cambios (6 min) |
| 2 | Script de inicialización antes del JS | Pendiente de la QA con sesión (el código de F9 lo inyecta igual en las dos plataformas) | — |
| 3 | *Autoplay* de la alarma | Pendiente (necesita una sesión de caja con un pedido). Plan B listo en el contrato (`sound.play`) | — |
| 4 | Timers con la ventana oculta | Pendiente de medir 15 min con sesión. El crate `alfredo-macos` trae las *assertions* de IOKit (probadas con `pmset`) | `apps/desktop/macos/src/power.rs` |
| 5 | Bytes por IPC | Pendiente (QA con sesión) | — |
| 6 | **CUPS *raw*** | **macOS 26 rechaza las colas `-m raw`** («lpadmin: ya no se admiten colas sin procesar en macOS»). Sí funciona una cola con el **PPD genérico** (`drv:///sample.drv/generic.ppd`) **imprimiendo con `lp -o raw`**: los bytes ESC/POS, las tildes en cp858 y el corte llegaron **idénticos** a un *listener* TCP (`socket://127.0.0.1:9101`). CUPS avisa que los drivers quedarán obsoletos en versiones futuras: el respaldo es USB directo (`rusb`) o TCP. | `cups.rs` (`lp_raw_args`, `lpadmin_raw_queue_command`, test `e2e` con `ALFREDO_CUPS_E2E=1`) |
| 7 | Red local en macOS 15+ | `NSLocalNetworkUsageDescription` y `NSBonjourServices` van en `apps/desktop/macos/Info.plist`; el aviso se prueba en la QA | — |
| 8 | Firma, notarización, tamaño y memoria | **Binario universal** (`lipo`: x86_64 + arm64). App **21,4 MB**, DMG **10,1 MB**. Memoria en reposo con la web cargada: **~90 MB** RSS. Firma Developer ID y notarización: ver abajo | `tauri build --target universal-apple-darwin --bundles app,dmg` |
| 9 | Updater sin permiso de escritura en /Applications | Pendiente (QA en un Mac con usuario estándar) | — |

**Recomendaciones:** transporte USB = **cola con el PPD genérico + `lp -o raw`** (no `-m raw`); TCP 9100
y serie (`/dev/cu.*`) con el código de F9. macOS mínimo **13** (sin evidencia en contra). La alarma
nativa queda como plan B hasta medir el *autoplay* en la QA.

## Firma y notarización

Certificado **«Developer ID Application: CreceTec SpA (5894Q84A9X)»** (`8LAR6WG752`, G2, vence
15-11-2027), creado el 16-10-2026 con la sesión del titular; `.p12` en
`~/.local/keys/apple-developer-id/developer-id.p12` e importado al llavero de build
`alfredo-build.keychain-db`. Resultado de la firma y la notarización: ver `ESTADO-F10.md`.
