# Pulse

DAW personal inspirado en el flujo de trabajo de Session View: lanzador de clips
cuantizado, escenas, secuenciador MIDI, sintetizadores y mezclador. Implementación
propia en Rust (cpal para audio, egui para la interfaz).

## Ejecutar

```bash
cargo run --release
```

Los proyectos se guardan como `.pulse` (JSON). `cargo test` ejecuta las pruebas, que no necesitan tarjeta de sonido.

## Uso

| Atajo | Acción |
| --- | --- |
| Espacio | Reproducir / parar |
| Tab | Cambiar entre Session y Arrangement |
| ⇧Tab | Cambiar entre editor de clip y dispositivos |
| ⌘Z / ⇧⌘Z | Deshacer / rehacer |
| ⌘S / ⇧⌘S / ⌘O / ⌘N | Guardar / guardar como / abrir / nuevo |
| ⌘D | Duplicar el clip seleccionado |
| ⌘T | Nueva pista MIDI |
| Supr | Borrar el clip seleccionado |

- **Session**: ▶ de un clip lo lanza (cuantizado), clic lo abre en el editor, doble clic en un slot vacío crea
  uno, clic derecho ofrece duplicar, copiar al Arrangement, color y borrar. Las escenas están en la columna Master.
- **Arrangement**: doble clic en una pista crea un clip; arrastrar lo mueve (también entre pistas); el borde
  derecho cambia su longitud; la franja superior define el loop; clic en la regla mueve el cursor.
  Lanzar clips de sesión anula el Arrangement de esa pista hasta pulsar **▶ Arrangement**.
- **Mezclador**: envíos A/B, panorama, fader en dB con medidor, activador (silencio) y solo.
  Dos retornos (Reverb y Delay) y el Master, todos con su cadena de efectos.
- **Dispositivos**: Pulse Synth o Drum Kit más efectos (EQ Three, Compressor, Saturator, Auto Filter,
  Delay, Reverb) desde el navegador. Knobs: arrastrar arriba/abajo, Shift para ajuste fino, doble clic para
  el valor por defecto.

## Estructura

- `crates/engine`: motor de audio en tiempo real (sin bloqueos ni reservas de memoria en el hilo de audio).
- `crates/app`: interfaz (Session View, mezclador, panel de instrumento, piano roll).

## Hoja de ruta

1. Importar sets `.als` de Ableton Live.
2. Pistas de audio, samples y warping.
3. Automatización de parámetros.
4. Entrada MIDI y grabación de notas (y de la sesión en el Arrangement).
5. Plugins VST3 / CLAP.
