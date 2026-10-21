# Pulse

DAW personal inspirado en el flujo de trabajo de Session View: lanzador de clips
cuantizado, escenas, secuenciador MIDI, sintetizadores y mezclador. Implementación
propia en Rust (cpal para audio, egui para la interfaz).

## Ejecutar

```bash
cargo run --release
```

Los proyectos se guardan como `.pulse` (JSON). `cargo test --release` ejecuta las pruebas, que no necesitan tarjeta de sonido.

Sistemas: macOS 13.4 o posterior (Apple Silicon) y Windows 10/11 x64, este pendiente de verificar
en un equipo real. Qué funciona en cada uno: [`docs/PLATFORM_SUPPORT.md`](docs/PLATFORM_SUPPORT.md).
En macOS: [instalar](docs/INSTALL_MACOS.md). En Windows: [compilar](docs/BUILD_WINDOWS.md),
[instalar](docs/INSTALL_WINDOWS.md) y [estado](docs/WINDOWS_SUPPORT.md). Publicar una
versión: [`docs/PUBLICAR.md`](docs/PUBLICAR.md).

## Uso

Todo lo que se puede hacer está en la **guía**, [`docs/guia.md`](docs/guia.md), que la app
muestra en Ayuda → Guía de Pulse (con buscador). Cada función nueva se añade a la guía en
el mismo pull request.

## Código

- `crates/engine`: motor de audio en tiempo real (sin bloqueos ni reservas de memoria en el hilo de audio).
- `crates/app`: interfaz (Session View, mezclador, panel de instrumento, piano roll).
- `crates/stems`: modelos ONNX (batería, audio a MIDI, stems) con ONNX Runtime.
- `crates/beats`: tempo y compases de un audio.
- `crates/paths`: dónde van ajustes, datos, cachés y logs en cada sistema.
- `crates/testplug`: plugin VST3/CLAP mínimo para probar el alojamiento de plugins.

## Hoja de ruta

1. Que unir y consolidar apliquen los efectos de la pista; dividir clips en Session.
2. Editar los marcadores de tempo en el Arrangement.

## Cómo se trabaja en el repositorio

- `main` siempre está lista para publicar: solo recibe trabajo terminado y
  probado, a través de pull requests. No se sube nada directamente a `main`
  (lo impide `.githooks/pre-push`; actívalo en cada copia del repositorio con
  `git config core.hooksPath .githooks`).
- Cada tarea va en su rama: `funcion/<tema>` para algo nuevo, `arreglo/<tema>`
  para corregir un fallo. En la rama, los commits que hagan falta.
- Al terminar, un pull request con lo que cambia y cómo se ha probado. Lo revisa
  y lo fusiona el dueño del repositorio, aplastando los commits en uno: el
  título del pull request (en español) es una línea de las novedades.
- Publicar: `scripts/release.sh [patch|minor|major]` desde `main` al día. La
  versión sale de la última etiqueta (`v1.2.3`); el script pasa las pruebas,
  firma Pulse.app, crea la etiqueta y la publica en GitHub, de donde la app se
  actualiza. El paquete de Windows se añade a esa publicación desde Windows con
  `scripts/package-windows.ps1` (ver [`docs/BUILD_WINDOWS.md`](docs/BUILD_WINDOWS.md)).
- La CI (`.github/workflows/windows.yml`) compila, prueba y empaqueta Pulse en
  Windows en cada pull request; macOS se prueba en local antes de cada pull request.

## Audio a MIDI

«Convertir a MIDI» (menú de un clip de audio) sigue el tono de una voz con pYIN
(`crates/stems/src/pitch.rs`, igual que `librosa.pyin`) para las melodías, y usa Basic Pitch (Spotify, licencia
Apache 2.0, ver `crates/stems/models/BASIC_PITCH_NOTICE`), un modelo ONNX de 230 KB
que va dentro de la app y corre con ONNX Runtime en la CPU. El port a Rust está en
`crates/stems/src/transcribe.rs`; su prueba compara las notas con las del original.
Para comparar pYIN con librosa en un archivo: `PULSE_PITCH_IN=voz.wav PULSE_PITCH_OUT=f0.txt
cargo test --release -p stems dump_track -- --ignored --nocapture`.

    python scripts/transcribe/make_reference.py   # (con `basic-pitch[onnx]`)
    cargo test --release -p stems transcribe

## Separación de stems

«Separar en stems» (menú de un clip de audio, o el editor de clips de audio) usa
HTDemucs (Meta, licencia MIT) con ONNX Runtime, en la CPU y sin conexión. El
modelo (`htdemucs.onnx`, 174 MB) no va dentro de la app: se descarga una vez de
la publicación `modelo-htdemucs-1` del repositorio y se guarda en
`Models` dentro de la carpeta de datos (`~/Library/Application Support/Pulse` en
macOS, `%LOCALAPPDATA%\Pulse` en Windows).

Para regenerarlo (en un entorno de Python con `demucs onnx onnxruntime soundfile`; `demo.wav` es la canción demo de Pulse, `target/release/pulse --render demo.wav`):

    python scripts/stems/export_htdemucs.py salida/
    python scripts/stems/make_reference.py salida/ demo.wav
    PULSE_STEMS_DIR=salida/ cargo test --release -p stems -- --include-ignored

Las pruebas comparan el espectrograma, la reconstrucción, la red y la separación
de una canción con los resultados de PyTorch. Si el archivo cambia, hay que
actualizar su tamaño y su SHA-256 en `crates/app/src/stems_ui.rs` y publicarlo
con otra etiqueta.

## Licencia

Copyright © 2026 David Fernández Morilla.

Pulse es software libre: se puede redistribuir y modificar según los términos
de la [GNU General Public License](LICENSE), versión 3 o (a tu elección)
cualquier versión posterior (`GPL-3.0-or-later`). Se distribuye sin ninguna
garantía.

Lo que Pulse incluye de terceros (crates de Rust, ONNX Runtime, los modelos de
IA, las fuentes) conserva su propia licencia: todas están en
[`packaging/THIRD_PARTY_NOTICES.txt`](packaging/THIRD_PARTY_NOTICES.txt), que va
en los paquetes de macOS y Windows y se ve en **Acerca de Pulse**. Tras cambiar
dependencias se regenera con `python3 scripts/notices.py` (una prueba comprueba
que esté al día).

Para contribuir: [CONTRIBUTING.md](CONTRIBUTING.md).

### Marcas

Ableton y Live son marcas de Ableton AG; VST es una marca de Steinberg Media
Technologies GmbH; CLAP es un estándar de free-audio; Roland, TR-808 y TR-909
son marcas de Roland Corporation. Se nombran solo para describir compatibilidad
(importar sets `.als`, alojar plugins VST3 y CLAP) o un estilo de sonido. Pulse
no tiene relación con ninguna de ellas ni cuenta con su respaldo.
