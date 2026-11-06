# Pulse por sistema

**Español** · [English](PLATFORM_SUPPORT.en.md)

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
| Pruebas automáticas | Verificado (`cargo test --release`) | CI: todas pasan en `main` desde el 5-11-2026 (ver abajo) | No soportado |
| Arranque y ventana | Verificado | CI el 5-11-2026: un guion abre la ventana, la recorre y guarda un proyecto, en un equipo sin GPU (no prueba tarjetas gráficas reales) | No soportado |
| Rutas de datos | Verificado | Implementado (reglas probadas en macOS) | Implementado (XDG) |
| Abrir y guardar proyectos | Verificado | Implementado | — |
| Proyectos entre sistemas | Verificado (rutas de Windows abiertas en macOS) | Implementado | — |
| Audio (salida, entrada, búfer) | Verificado (CoreAudio) | Parcial: WASAPI compartido, sin ASIO | No soportado |
| Grabación de audio | Verificado | Implementado | No soportado |
| MIDI de entrada | Verificado (CoreMIDI) | Implementado (WinMM) | No soportado |
| Instrumentos y efectos propios | Verificado | Implementado (código común) | — |
| Render offline (exportar, `--render`) | Verificado | CI el 5-11-2026 (`pulse.exe --render`) | — |
| Plugins VST3: carga, sonido, estado | Verificado | Implementado | No soportado |
| Plugins CLAP: carga, sonido, estado | Verificado | Implementado | No soportado |
| Editores de plugins | Verificado (NSView) | Implementado (HWND, escala de pantalla) | No soportado |
| IA: batería, Basic Pitch, tempo | Verificado (CPU) | Implementado (CPU; DirectML sin usar) | — |
| Separar stems (descarga del modelo) | Verificado | Implementado | — |
| Atajos de teclado | Verificado (barra de menús) | Implementado (Ctrl; probado con entrada simulada) | — |
| Abrir `.pulse` con doble clic | Verificado | Parcial: la CI comprueba la asociación del instalador; el doble clic, sin probar | — |
| Registro de fallos | Verificado | Implementado | Implementado |
| Paquete | Verificado (Pulse.app, `bundle.sh`) | CI el 5-11-2026 (zip + Inno Setup: instala y desinstala; `SHA256SUMS.txt`); aún no se adjunta a las publicaciones | No soportado |
| Actualizaciones | Verificado (firma de código) | Implementado (SHA-256 y firma Ed25519 de Pulse; sin probar) | No soportado |
| Firma de código | Parcial (certificado local) | No soportado (sin certificado) | — |

**Nota sobre la CI de Windows.** Desde el 5-11-2026 todo el trabajo de Windows
pasa en `main` (ejecución [42045040404](https://github.com/davidfdzmorilla/pulse/actions/runs/42045040404),
commit `518120e`): compilación, pruebas, empaquetado, sumas, render, arranque
de la ventana e instalador. Lo mismo el de macOS (ejecución [42045040332](https://github.com/davidfdzmorilla/pulse/actions/runs/42045040332)).
Nada de esto se ha probado aún en un Windows real.

## Pruebas de Windows en la CI

Entre el 29-10 y el 5-11-2026 la CI de Windows estuvo en rojo. Estas son las
pruebas que fallaron, por qué y cómo se arreglaron. Ninguna se desactivó ni
se convirtió en aviso.

| Prueba | Cómo fallaba | Causa | Arreglo |
|---|---|---|---|
| `a_kit_dragged_from_the_browser_onto_a_midi_track_loads_there` | Siempre | Fallo de la prueba: esperaba `…\Bombos/Bombo house.wav` (una `/` dentro de un `Path::join`) | La ruta esperada se une parte a parte (#209) |
| `lyrics_are_saved_and_opened_with_the_project` | Siempre | El mismo fallo de la prueba | Igual (#209) |
| `finds_the_notes_basic_pitch_finds` | A veces (y siempre en el Mac M1 de la CI) | Dos fotogramas de inicio de nota debían empatar; un redondeo de un ulp, distinto según CPU e hilos, rompía el empate y la nota empezaba 1–2 fotogramas tarde | `inferred_onsets` conserva el empate; prueba nueva `one_thread_finds_the_same_notes` (#209) |
| `more_threads_give_the_same_notes` | Siempre | Exigía activaciones de ONNX Runtime idénticas bit a bit con 2 y 3 hilos; en Windows x64 difieren en los últimos bits | Notas idénticas en tiempo y altura, amplitud con tolerancia de 1e-4 y activaciones a menos de 3e-4; en macOS sigue exigiendo igualdad exacta (#209) |
| Arranque de la ventana con guion | Siempre desde el 4-11 | Desbordamiento de pila (`0xC00000FD`): la pantalla de bienvenida necesita 1,25–1,5 MB y el hilo principal de Windows tiene 1 MB | Pila de 8 MB al enlazar (`.cargo/config.toml`) (#209) |
| `with_threads_a_sidechain_waits_for_its_source` | A veces | Los hilos de ayuda no llegaban a arrancar (ver abajo) | Hilos con prioridad alta desde que se crean (#211); la prueba repite hasta un minuto si los hilos no han participado y comprueba el sonido en cada repetición |
| `the_audio_threads_do_not_allocate_with_workers` | A veces | Los hilos de ayuda empezaban tarde y su primera ejecución caía dentro de la medida; no se reproducía con los hilos arrancando a tiempo | Igual (#211); pasa sin ninguna reserva de memoria en los hilos |

**Los hilos de ayuda en Windows.** Un hilo nuevo de Windows empieza con
prioridad normal y solo se subía a la del audio al ejecutar su primera
instrucción. Con todas las pruebas a la vez, decenas de hilos de otras
pruebas ocupan los 4 procesadores de la CI con prioridad máxima, y Windows
siempre los atiende antes: en el diagnóstico, en 81 de 82 repeticiones
ninguno de los 3 hilos llegó a arrancar. Ahora el hilo que los crea les sube
la prioridad nada más crearlos (`crates/engine/src/workers.rs`). En Pulse el
hilo de audio de cpal ya corre con prioridad máxima.

Queda una lentitud: las pruebas de estrés del motor tardan unos 27 minutos
en la CI de Windows; a solas, la de reservas de memoria tarda 34 segundos,
y todas juntas en macOS, 82 segundos. Pasan; ver [FIRST_CONTRIBUTIONS.md](FIRST_CONTRIBUTIONS.md#3-windows-ci-make-the-engine-stress-tests-fast-again).
