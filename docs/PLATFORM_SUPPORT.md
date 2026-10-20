# Pulse por sistema

- **Verificado**: probado en ese sistema.
- **Implementado**: el código está y compila para ese sistema, pero no se ha
  ejecutado en él.
- **Parcial**: con una limitación (ver la nota).
- **No soportado**: no hay implementación.

Linux solo aparece por lo que el código ya contempla. No se compila, no se
prueba y no es una plataforma soportada.

Detalle de Windows: [WINDOWS_SUPPORT.md](WINDOWS_SUPPORT.md).

| Función | macOS (Apple Silicon) | Windows 10/11 x64 | Linux |
|---|---|---|---|
| Compilación nativa | Verificado | Implementado (enlaza desde macOS con `cargo xwin`; sin ejecutar) | No soportado |
| Pruebas automáticas | Verificado (`cargo test --release`) | Implementado (CI escrita; bloqueada por facturación de GitHub) | No soportado |
| Arranque y ventana | Verificado | Implementado | No soportado |
| Rutas de datos | Verificado | Implementado (reglas probadas en macOS) | Implementado (XDG) |
| Abrir y guardar proyectos | Verificado | Implementado | — |
| Proyectos entre sistemas | Verificado (rutas de Windows abiertas en macOS) | Implementado | — |
| Audio (salida, entrada, búfer) | Verificado (CoreAudio) | Parcial: WASAPI compartido, sin ASIO | No soportado |
| Grabación de audio | Verificado | Implementado | No soportado |
| MIDI de entrada | Verificado (CoreMIDI) | Implementado (WinMM) | No soportado |
| Instrumentos y efectos propios | Verificado | Implementado (código común) | — |
| Render offline (exportar, `--render`) | Verificado | Implementado | — |
| Plugins VST3: carga, sonido, estado | Verificado | Implementado | No soportado |
| Plugins CLAP: carga, sonido, estado | Verificado | Implementado | No soportado |
| Editores de plugins | Verificado (NSView) | Implementado (HWND, escala de pantalla) | No soportado |
| IA: batería, Basic Pitch, tempo | Verificado (CPU) | Implementado (CPU; DirectML sin usar) | — |
| Separar stems (descarga del modelo) | Verificado | Implementado | — |
| Atajos de teclado | Verificado (barra de menús) | Implementado (Ctrl; probado con entrada simulada) | — |
| Abrir `.pulse` con doble clic | Verificado | Implementado (instalador) | — |
| Registro de fallos | Verificado | Implementado | Implementado |
| Paquete | Verificado (Pulse.app, `bundle.sh`) | Implementado (zip + Inno Setup) | No soportado |
| Actualizaciones | Verificado (firma de código) | Implementado (SHA-256; sin firma) | No soportado |
| Firma de código | Parcial (certificado local) | No soportado (sin certificado) | — |
