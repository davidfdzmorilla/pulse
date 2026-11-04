# Pulse por sistema

- **Verificado**: probado en ese sistema.
- **Implementado**: el código está y compila para ese sistema, pero no se ha
  ejecutado en él.
- **CI**: comprobado en la CI de GitHub (`windows-2022`, sin tarjeta de sonido,
  MIDI ni plugins de terceros), no en un equipo real.
- **Parcial**: con una limitación (ver la nota).
- **No soportado**: no hay implementación.

Linux solo aparece por lo que el código ya contempla. No se compila, no se
prueba y no es una plataforma soportada.

Detalle de Windows: [WINDOWS_SUPPORT.md](WINDOWS_SUPPORT.md).

| Función | macOS (Apple Silicon) | Windows 10/11 x64 | Linux |
|---|---|---|---|
| Compilación nativa | Verificado | CI (MSVC; también enlaza desde macOS con `cargo xwin`) | No soportado |
| Pruebas automáticas | Verificado (`cargo test --release`) | Parcial: la CI corre; en `main` fallan 3 pruebas en cada ejecución y 3 a veces (ver abajo) | No soportado |
| Arranque y ventana | Verificado | CI el 29-10-2026 (la ventana arranca y sigue un guion, en un equipo sin GPU: no prueba tarjetas gráficas reales); no ha vuelto a ejecutarse, ver la nota | No soportado |
| Rutas de datos | Verificado | Implementado (reglas probadas en macOS) | Implementado (XDG) |
| Abrir y guardar proyectos | Verificado | Implementado | — |
| Proyectos entre sistemas | Verificado (rutas de Windows abiertas en macOS) | Implementado | — |
| Audio (salida, entrada, búfer) | Verificado (CoreAudio) | Parcial: WASAPI compartido, sin ASIO | No soportado |
| Grabación de audio | Verificado | Implementado | No soportado |
| MIDI de entrada | Verificado (CoreMIDI) | Implementado (WinMM) | No soportado |
| Instrumentos y efectos propios | Verificado | Implementado (código común) | — |
| Render offline (exportar, `--render`) | Verificado | CI el 29-10-2026 (`pulse.exe --render`); ver la nota | — |
| Plugins VST3: carga, sonido, estado | Verificado | Implementado | No soportado |
| Plugins CLAP: carga, sonido, estado | Verificado | Implementado | No soportado |
| Editores de plugins | Verificado (NSView) | Implementado (HWND, escala de pantalla) | No soportado |
| IA: batería, Basic Pitch, tempo | Verificado (CPU) | Implementado (CPU; DirectML sin usar) | — |
| Separar stems (descarga del modelo) | Verificado | Implementado | — |
| Atajos de teclado | Verificado (barra de menús) | Implementado (Ctrl; probado con entrada simulada) | — |
| Abrir `.pulse` con doble clic | Verificado | Parcial: la CI comprueba la asociación del instalador; el doble clic, sin probar | — |
| Registro de fallos | Verificado | Implementado | Implementado |
| Paquete | Verificado (Pulse.app, `bundle.sh`) | CI el 29-10-2026 (zip + Inno Setup: instala y desinstala); ver la nota; aún no se adjunta a las publicaciones | No soportado |
| Actualizaciones | Verificado (firma de código) | Implementado (SHA-256 y firma Ed25519 de Pulse; sin probar) | No soportado |
| Firma de código | Parcial (certificado local) | No soportado (sin certificado) | — |

**Nota sobre la CI de Windows.** Desde el 29-10-2026 las pruebas fallan, y en
`main` eso para el trabajo antes de empaquetar. Así que el paquete, el render,
el arranque de la ventana y el instalador **no se han vuelto a ejecutar desde
ese día**. En la rama `funcion/preparar-lanzamiento` el workflow los ejecuta
también cuando falla una prueba (el trabajo sigue en rojo por las pruebas).

## Pruebas que fallan en Windows

Estado del 4 de noviembre de 2026. Las pruebas se han ejecutado en la CI de
Windows sobre `main` (ejecuciones 41627764299, 41654073501, 41697364751,
41789816848 y 41794967564), en la CI de Windows sobre esta rama (41808950553) y en macOS (en local y en la CI de macOS). Ninguna
en un Windows real. Tabla completa, con las causas, en
[FIRST_CONTRIBUTIONS.md](FIRST_CONTRIBUTIONS.md#3-windows-audio-to-midi-with-any-number-of-threads).

| Prueba | En la CI de Windows | En la rama `funcion/preparar-lanzamiento` |
|---|---|---|
| `a_kit_dragged_from_the_browser_onto_a_midi_track_loads_there` | Falla siempre | Corregida (fallo de la prueba); **pasa en la CI de Windows** (ejecución [41808950553](https://github.com/davidfdzmorilla/pulse/actions/runs/41808950553), 4-11-2026) y en macOS |
| `lyrics_are_saved_and_opened_with_the_project` | Falla siempre | Corregida (fallo de la prueba); **pasa en la CI de Windows** (ejecución [41808950553](https://github.com/davidfdzmorilla/pulse/actions/runs/41808950553), 4-11-2026) y en macOS |
| `more_threads_give_the_same_notes` | Falla siempre | Pendiente |
| `finds_the_notes_basic_pitch_finds` | Falla a veces (y siempre en el Mac M1 de la CI) | Corregida en `inferred_onsets`; pasa en macOS, sin ejecutar en el M1 de la CI ni en Windows |
| `with_threads_a_sidechain_waits_for_its_source` | Falla a veces | Pendiente; no se reproduce en macOS |
| `the_audio_threads_do_not_allocate_with_workers` | Falla a veces | Pendiente; causa desconocida; no se reproduce en macOS |
