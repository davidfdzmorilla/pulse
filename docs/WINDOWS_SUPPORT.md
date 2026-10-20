# Pulse en Windows: estado

Estado del soporte de Windows x64 (`x86_64-pc-windows-msvc`), qué lo bloqueaba,
qué se ha cambiado y qué está comprobado. Documentos relacionados:
[BUILD_WINDOWS.md](BUILD_WINDOWS.md) (compilar),
[INSTALL_WINDOWS.md](INSTALL_WINDOWS.md) (instalar y usar) y
[PLATFORM_SUPPORT.md](PLATFORM_SUPPORT.md) (matriz por sistema).

**Resumen (20 de octubre de 2026).** El workspace pasa la comprobación de tipos para
Windows x64 desde macOS. Todo lo que bloqueaba Windows está implementado. Pero
**nada se ha ejecutado aún en Windows**: la CI de Windows está escrita y no ha
podido correr (GitHub no inicia trabajos en esta cuenta por facturación), y no
hay ningún equipo Windows disponible. Pulse **no está listo para publicarse en
Windows** hasta que la CI o una prueba manual lo demuestren.

## Qué significa cada estado

- **Verificado**: probado de verdad (en macOS, o con la comprobación cruzada para
  Windows, según se indica).
- **Implementado, pendiente de Windows**: el código está y compila para Windows,
  pero no se ha ejecutado en Windows.
- **Parcial**: funciona con una limitación conocida.
- **Pendiente**: no hecho.
- **Bloqueado**: depende de algo externo.

## Arquitectura

- `crates/engine`: motor de audio en tiempo real (CPAL: CoreAudio en macOS,
  WASAPI en Windows), instrumentos, efectos y alojamiento de plugins VST3 y CLAP.
- `crates/app`: la interfaz (eframe/egui con wgpu), proyectos, actualizador,
  MIDI (midir: CoreMIDI en macOS, WinMM en Windows).
- `crates/stems`: modelos ONNX (batería, Basic Pitch, HTDemucs) con ONNX Runtime
  (`ort`), enlazado dentro del ejecutable.
- `crates/beats`: tempo y compases (Beat This! con `rten`, Rust puro).
- `crates/paths` (nuevo): dónde se guardan ajustes, datos, cachés y logs en cada
  sistema.
- `crates/testplug`: plugin VST3/CLAP mínimo para las pruebas del alojamiento.

## Bloqueos encontrados y soluciones

| # | Bloqueo | Archivos | Causa | Impacto | Solución | Estado |
|---|---|---|---|---|---|---|
| 1 | El plugin de pruebas no compilaba para Windows | `crates/testplug/src/lib.rs` | En Windows las constantes del SDK VST3 son `i32` y el campo `u32` | `cargo test` no compilaba en Windows | Conversión `as _` | Verificado (comprobación cruzada) |
| 2 | Rutas de macOS dentro del código | `kits.rs`, `settings.rs`, `library.rs`, `stems_ui.rs`, `midi_map.rs`, `plugins.rs`, `commands.rs`, `beats/src/lib.rs` | `HOME` + `Library/Application Support`, `Library/Caches`, `Music` | En Windows los ajustes y modelos irían a sitios sin sentido, o a la carpeta temporal | Crate `paths`: carpetas conocidas de Windows (`SHGetKnownFolderPath`: AppData, LocalAppData, Música, también si se han movido de disco), las de siempre en macOS y XDG en Linux | Implementado; reglas verificadas con pruebas en macOS; pendiente de Windows |
| 3 | Editores de plugins solo en macOS | `app/src/editor.rs`, `engine/src/clap_host.rs`, `engine/src/plugin.rs` | Ventana NSWindow; CLAP solo con la API `cocoa` | En Windows, «Editor» solo daba un mensaje | Ventana Win32 propia, propiedad de la de Pulse, con el editor como hijo (HWND); CLAP con la API `win32`; escala de pantalla (`IPlugViewContentScaleSupport`, `clap_plugin_gui.set_scale`) y cambio de pantalla (`WM_DPICHANGED`) | Implementado, pendiente de Windows |
| 4 | Carpetas de plugins de Windows incompletas | `engine/src/plugin.rs` | `C:\Program Files\Common Files` fijo, sin las del usuario | Plugins instalados por usuario o en otra unidad no se encontraban | `%CommonProgramFiles%\VST3`/`CLAP` y `%LOCALAPPDATA%\Programs\Common\VST3`/`CLAP` | Implementado, pendiente de Windows |
| 5 | Actualizador exclusivo de macOS | `app/src/updates.rs` | Llavero (`security`), `codesign`, `ditto`, `PlistBuddy`, `/bin/sh`, `open`, `/usr/bin/curl` | En Windows no podía buscar ni instalar versiones | Módulo `system` por plataforma. Windows: token en el Administrador de credenciales (`CredReadW`/`CredWriteW`), `curl.exe` y `tar.exe` del sistema, versión leída del recurso de `pulse.exe`, huella SHA-256 de GitHub obligatoria, cambio de `pulse.exe` guardando «pulse (anterior).exe», puesta al día de las DLL del runtime, reinicio con PowerShell | Implementado, pendiente de Windows |
| 6 | SHA-256 con `/usr/bin/shasum` | `app/src/stems_ui.rs` | Utilidad de macOS | La descarga del modelo de stems fallaba en Windows | Crate `sha2` (RustCrypto), igual en todos los sistemas | Verificado en macOS |
| 7 | Abrir carpetas con `open` | `app/src/commands.rs` | Comando de macOS | «Mostrar la carpeta de Pulse» fallaba | `files::show_folder`: `open`, `explorer` o `xdg-open` | Implementado, pendiente de Windows |
| 8 | Fuentes de `/System/Library/Fonts` | `app/src/theme.rs` | Rutas de macOS | En Windows se usaba la fuente de egui y faltaban símbolos | Segoe UI, Consolas y Segoe UI Symbol de `%WINDIR%\Fonts` | Implementado, pendiente de Windows |
| 9 | Atajos de la barra de menús | `app/src/native_menu.rs`, `app/src/app.rs` | En macOS ⌘C/⌘V/⌘X/⌘E/⌘J/⌘,/⌘Q los atiende la barra de menús del sistema, que Windows no tiene | En Windows no habrían hecho nada | Sin barra del sistema se leen del teclado (Ctrl), copiar/cortar/pegar por sus eventos de egui y F11 para pantalla completa | Implementado; probado en macOS con entradas simuladas |
| 10 | Atajos escritos con ⌘ ⇧ ⌥ | 12 archivos de `app/src` | Símbolos de Mac en botones y ayudas | Confuso en Windows | `theme::keys`: «Ctrl+Mayús+S», «Ctrl clic», «F11», «Supr» | Implementado; la conversión se prueba en macOS |
| 11 | Abrir `.pulse` con doble clic | `app/src/open_files.rs` | Solo los Apple Events de macOS | El archivo no se abría | Se lee de los argumentos; el instalador asocia `.pulse` | Implementado; la asociación se prueba en la CI (pendiente de que corra) |
| 12 | Rutas de proyecto entre sistemas | `app/src/library.rs`, `app/src/app.rs` | Las rutas relativas se guardaban con el separador del sistema | Un proyecto guardado en Windows perdía sus audios en macOS | Rutas relativas siempre con «/»; al abrir se aceptan «\\» y las rutas absolutas del otro sistema se conservan; la barra de estado avisa de los audios que faltan | Verificado en macOS (prueba con rutas de Windows) |
| 13 | ONNX Runtime de Windows importa DirectML | `.cargo/config.toml` | El binario precompilado de `ort` para Windows es el de DirectML: importa `DirectML.dll` (no viene con Windows) y `DXCORE.dll` (desde Windows 10 1903) | `pulse.exe` no arrancaría en un Windows sin esa DLL | Carga diferida (`/DELAYLOAD`): Pulse usa la CPU y no las carga | Implementado; pendiente de comprobar con `dumpbin` en Windows (el script de empaquetado lo hace) |
| 14 | Runtime de Visual C++ | `scripts/package-windows.ps1`, `packaging/windows/pulse.iss` | Rust con MSVC y ONNX Runtime (C++) usan `vcruntime140.dll` y `msvcp140.dll`, que no están en todo Windows | Error al arrancar en un Windows limpio | Se empaquetan junto a `pulse.exe` (despliegue local permitido por Microsoft) y el actualizador las pone al día | Implementado, pendiente de Windows |
| 15 | Ventana de consola, icono y versión | `app/src/main.rs`, `app/build.rs` | Programa de consola, sin recursos | Al abrir Pulse salía una consola; icono de egui | `windows_subsystem = "windows"` en release (las opciones de línea de órdenes se enganchan a la consola que las lanzó), recurso de icono y versión con `embed-resource`, icono de la ventana | Implementado, pendiente de Windows |
| 16 | Sin registro de fallos | `app/src/logs.rs` | Un cierre inesperado no dejaba rastro (también en macOS) | No se podía diagnosticar | `pulse.log` en la carpeta de logs, con la traza de cada pánico | Verificado en macOS (escritura y rotación) |
| 17 | Sin empaquetado de Windows | `scripts/package-windows.ps1`, `packaging/windows/` | Solo `bundle.sh` (Pulse.app) | No había nada que instalar | Zip portable + instalador Inno Setup por usuario, sin administrador | Implementado, pendiente de Windows |
| 18 | Sin CI | `.github/workflows/windows.yml` | — | Nada comprobaba Windows | Workflow de Windows: compila, prueba, empaqueta, renderiza, arranca la ventana con un guion e instala y desinstala | **Bloqueado**: GitHub no inicia trabajos («recent account payments have failed or your spending limit needs to be increased») |

## Comprobaciones hechas

Desde macOS 15 (Apple Silicon), con Rust estable:

```bash
rustup target add x86_64-pc-windows-msvc
cargo check --release --target x86_64-pc-windows-msvc --workspace --all-targets   # sin errores ni avisos
cargo test --release                                                              # macOS: todo pasa
cargo build --release                                                             # macOS
```

- `cargo check` para Windows compila el código de Windows (Win32, credenciales,
  versión del ejecutable, editores) y descarga el ONNX Runtime precompilado de
  Windows, pero **no enlaza**. No demuestra que Pulse compile ni funcione en
  Windows.
- Las pruebas nuevas (rutas por sistema, proyectos con rutas de Windows,
  huella SHA-256, paquete por sistema, atajos, registro de fallos) corren en
  macOS. Las marcadas `#[cfg(windows)]` (DLL del runtime) solo correrán en Windows.

## Pendiente de verificar en Windows

En orden de prioridad (la CI hace del 1 al 5 en cuanto pueda correr):

1. Compilación y enlace nativos (`cargo build --release --workspace --all-targets`).
2. `cargo test --release --workspace`: incluye el motor, ONNX Runtime con los tres
   modelos, el alojamiento VST3/CLAP con el plugin de pruebas y el render offline.
3. `dumpbin /dependents pulse.exe`: que `DirectML.dll` y `DXCORE.dll` no se carguen al arrancar.
4. `pulse.exe --render`: un WAV válido.
5. Arranque de la ventana con el guion `scripts/ci/arranque.txt` (wgpu en un
   equipo sin GPU puede necesitar el adaptador de software WARP: si falla, se verá aquí).
6. Instalar, asociar `.pulse`, desinstalar sin borrar ajustes.
7. A mano, con hardware (ver la lista en [INSTALL_WINDOWS.md](INSTALL_WINDOWS.md#comprobaciones-a-mano)):
   audio por WASAPI (dispositivos, frecuencias, búferes, desconexión), grabación
   y permiso del micrófono, MIDI por USB, editores de plugins reales (VST3 y
   CLAP), escala de pantalla 125–200 % y monitores de escalas distintas, y el
   actualizador contra una publicación con el zip de Windows.

## Limitaciones conocidas

- **Audio**: solo WASAPI en modo compartido (el de CPAL por defecto). ASIO no está
  activado: necesita el SDK de ASIO de Steinberg y su licencia. La latencia
  será mayor que con ASIO.
- **Editores de plugins**: flotan sobre Pulse y se minimizan con él, pero no se
  ocultan cuando Pulse pasa a segundo plano (en macOS sí).
- **Plugins de 32 bits**: no se cargan (Pulse es de 64 bits). Windows ARM64 no está
  contemplado.
- **IA en GPU**: los modelos van en la CPU; DirectML no se usa.
- **Firma**: no hay certificado de firma de código. SmartScreen avisará al
  instalar, y el actualizador comprueba que la descarga es la publicada (SHA-256
  que da GitHub) pero no quién la hizo. Para comprobar la autoría hace falta un
  certificado Authenticode, firmar con `signtool` en el empaquetado, y verificar
  con `WinVerifyTrust` en el actualizador.
- **Una sola instancia**: un doble clic en un `.pulse` con Pulse abierto abre otro Pulse.
- **Formato**: el repositorio no usa `cargo fmt` (estilo propio); la CI no lo comprueba.
- **Linux**: se compila la lógica de rutas y plugins, pero no hay editores de
  plugins ni empaquetado; no está soportado.
