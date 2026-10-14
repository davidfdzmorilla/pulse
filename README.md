# Pulse

DAW personal inspirado en el flujo de trabajo de Session View: lanzador de clips
cuantizado, escenas, secuenciador MIDI, sintetizadores y mezclador. Implementación
propia en Rust (cpal para audio, egui para la interfaz).

## Ejecutar

```bash
cargo run --release
```

`cargo test` ejecuta las pruebas del motor (no necesitan tarjeta de sonido).

## Uso

- **Espacio**: reproducir / parar. **Supr**: borrar el clip seleccionado.
- **▶ de un clip**: lanzarlo (espera al siguiente compás según la cuantización).
- **Clic en un clip**: abrirlo en el piano roll. **Doble clic en un slot vacío**: nuevo clip.
- **Piano roll**: clic en vacío añade una nota (arrastra para alargarla), clic en una nota la borra,
  clic en una tecla la escucha.
- **Escenas**: lanzan una fila entera; los slots vacíos paran su pista.

## Estructura

- `crates/engine`: motor de audio en tiempo real (sin bloqueos ni reservas de memoria en el hilo de audio).
- `crates/app`: interfaz (Session View, mezclador, panel de instrumento, piano roll).

## Hoja de ruta

1. Guardar / cargar proyectos e importar sets `.als`.
2. Arrangement View y grabación de la sesión en el arreglo.
3. Pistas de audio, samples y warping.
4. Automatización, envíos y retornos, efectos (EQ, compresor, delay, reverb).
5. Entrada MIDI y grabación de notas.
6. Plugins VST3 / CLAP.
