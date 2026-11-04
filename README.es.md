<p align="center">
  <img src="packaging/icon-1024.png" alt="Logotipo de Pulse" width="112">
</p>

<h1 align="center">Pulse</h1>

<p align="center"><strong>Un DAW independiente para la producción musical creativa.</strong></p>

<p align="center">
Pulse es una estación de trabajo de audio digital escrita desde cero en Rust, con
motor de audio en tiempo real propio, plugins VST3 y CLAP y herramientas creativas
integradas: audio a MIDI, acordes y bajos que siguen tu canción, separación de
stems y análisis de la mezcla. Todo corre en tu ordenador, sin conexión y sin cuentas.
</p>

<p align="center">
  <a href="https://github.com/davidfdzmorilla/pulse/releases/latest"><img alt="Última versión" src="https://img.shields.io/github/v/release/davidfdzmorilla/pulse?label=versi%C3%B3n"></a>
  <a href="LICENSE"><img alt="Licencia: GPL-3.0-or-later" src="https://img.shields.io/badge/licencia-GPL--3.0--or--later-blue"></a>
  <img alt="Sistemas: macOS Apple Silicon, Windows x64 experimental" src="https://img.shields.io/badge/sistemas-macOS%20(Apple%20Silicon)%20%C2%B7%20Windows%20x64%20(experimental)-555">
</p>

<p align="center">
  <a href="docs/DOWNLOADS.md"><strong>Descargar</strong></a> ·
  <a href="#primeros-pasos">Primeros pasos</a> ·
  <a href="#qué-puedes-hacer">Funciones</a> ·
  <a href="docs/guia.md">Guía</a> ·
  <a href="docs/ROADMAP.md">Hoja de ruta</a> ·
  <a href="CONTRIBUTING.es.md">Contribuir</a> ·
  <a href="CHANGELOG.md">Cambios</a> ·
  <a href="https://github.com/davidfdzmorilla/pulse/discussions">Conversaciones</a>
</p>

<p align="center"><a href="README.md">English</a> · <strong>Español</strong></p>

![El Arrangement de Pulse con un tema completo](docs/capturas/arrangement.png)

*El Arrangement: un tema completo con sus secciones, clips MIDI y de audio, transiciones y la automatización del filtro. ([Más capturas](#capturas))*

> **Estado: primera versión pública, en desarrollo activo.** macOS (Apple
> Silicon) es el sistema soportado; Windows es experimental. La interfaz y la
> guía están en español.

## Sistemas

| | macOS 13.4+ (Apple Silicon) | Windows 10/11 x64 | Linux |
|---|---|---|---|
| Descarga | [`Pulse-<versión>.zip`](https://github.com/davidfdzmorilla/pulse/releases/latest) | Aún no se publica: [compílalo](docs/BUILD_WINDOWS.md) o usa un artefacto de la CI | — |
| Estado | **Soportado**: cada versión pasa todas las pruebas y se prueba a mano antes de publicarse | **Experimental**: compila en el Windows de la CI, donde aún fallan algunas pruebas. Se instaló y arrancó allí por última vez el 29-10-2026; sin probar con audio/MIDI reales ni plugins de terceros | No soportado |
| Firma | Certificado propio, **sin notarizar** por Apple (un paso de Gatekeeper la primera vez) | Sin firma (aviso de SmartScreen); las actualizaciones se comprueban con la firma Ed25519 de Pulse | — |

Detalle por función: [docs/PLATFORM_SUPPORT.md](docs/PLATFORM_SUPPORT.md). Los Mac con Intel aún no están soportados.

## Primeros pasos

1. Descarga `Pulse-<versión>.zip` de la
   [última versión](https://github.com/davidfdzmorilla/pulse/releases/latest),
   descomprímelo y **mueve `Pulse.app` a Aplicaciones antes de abrirlo**.
2. La primera vez macOS lo bloquea porque no está notarizado: **Ajustes del
   Sistema → Privacidad y seguridad → Abrir igualmente**. Todos los pasos,
   las sumas SHA-256 y cómo desinstalar: [docs/INSTALL_MACOS.md](docs/INSTALL_MACOS.md)
   y [docs/DOWNLOADS.md](docs/DOWNLOADS.md).
3. La primera vez, Pulse pregunta **¿Qué quieres hacer?**: escuchar su canción
   de ejemplo, empezar de una plantilla por género (house, techno, trap,
   reggaetón, pop, lo-fi, drum & bass, podcast), tararear una idea, remezclar
   una canción o seguir una de sus **lecciones** paso a paso, que se marcan
   solas al hacerlas.
4. Pulse se actualiza solo desde las publicaciones de GitHub (Pulse → Buscar
   actualizaciones…).

## Qué puedes hacer

**Componer**
- Session View con clips y escenas lanzados en tiempo, y Arrangement con
  tomas, comping, transiciones, automatización y arreglo automático por
  secciones (intro, subida, drop, break, salida).
- Editor MIDI con escala, acordes, cuantización y groove. Pitch bend, rueda,
  aftertouch y cualquier CC se graban y se editan en carriles del clip.
- Tonalidad del proyecto, acordes por grados y **Acompañar**: bajos (raíz,
  octavas, contratiempo, caminante, rodante, galope), arpegios, pads, stabs y
  **melodías** (gancho, cantable, lead de trance, pregunta y respuesta) que
  siguen los acordes de un clip.
- **Generar ritmo** en el estilo que elijas (house, techno, trap, reggaetón…),
  escrito o tocado por una IA de batería.
- **De tarareo a canción**: tararea una melodía y Pulse la armoniza y le pone
  batería, bajo y acordes.

**Voz y audio**
- **Audio a MIDI** de voces (pYIN) e instrumentos o acordes (Basic Pitch).
- **Afinar una voz nota a nota**, separar una canción en **stems** (HTDemucs) y
  seguir el tempo de cualquier audio (Beat This!).
- **Transcribir la letra** de una voz con sus tiempos (Whisper), corregirla a
  mano y exportarla como .txt o .lrc.
- **Tocar con la voz**: canta, tararea o haz beatbox en el micrófono y una
  pista lo toca como un teclado MIDI.

**Sonido**
- Instrumentos propios: Pulse Synth, Super Saw, Acid Bass, FM Synth,
  Wavetable, Polychrome, Modular, Pulse Keys (piano, pianos eléctricos y
  órgano), Sampler, Kick Designer, Drum Machine y Drum Pads, con cientos de
  presets de fábrica.
- **Sonidos de Pulse**: kits de batería por género y sonidos sueltos, hechos
  por Pulse sin samples de terceros.
- 23 efectos, entre ellos EQ de 8 bandas, compresor, **compresor de bus**,
  **multibanda (OTT)**, limitador, saturador, de-esser, reverb de convolución
  **Espacio**, delay, chorus, phaser, **Vocoder** y **Voz: tono y formantes**.
- Cadenas de efectos guardables y plugins **VST3 y CLAP** con sus editores.

**Mezclar y terminar**
- Mezclador con grupos, retornos, sidechain (también en un clic desde el
  bombo) y compensación de latencia.
- **Asistente de mezcla y master**: mide la mezcla, explica lo que ve y, si
  quieres, la equilibra, le hace sitio a cada pista y la lleva a la sonoridad
  de streaming, Apple Music, club, hard techno, radio y TV, o la de una
  canción de referencia.
- Exportación a WAV, stems por pista y archivos MIDI; importación de sets de
  Ableton Live (`.als`).
- Versiones del proyecto, autoguardado y recuperación tras un cierre
  inesperado.

**Motor**
- Audio en tiempo real diseñado sin bloqueos ni reservas de memoria en el hilo
  de audio, repartido entre varios núcleos. El motor está diseñado para que el
  render sea determinista con cualquier tamaño de buffer y número de hilos.
  Hay pruebas automáticas que lo cubren, también que un render offline se
  repita igual.

Todo está explicado en la **guía**, [`docs/guia.md`](docs/guia.md), que la app
muestra en Ayuda → Guía de Pulse (con buscador). Cada función nueva se añade a
la guía en el mismo pull request.

## Capturas

![La Session](docs/capturas/session.png)

*La Session: clips y escenas para improvisar, con el mezclador, los envíos y los retornos de Espacio y Delay.*

![El piano roll](docs/capturas/piano-roll.png)

*El piano roll: una melodía escrita por Acompañar sobre los acordes, con su carril de expresión (brillo).*

![Los dispositivos](docs/capturas/dispositivos.png)

*Los dispositivos: el bus de batería con el Compresor de bus y la reverb de convolución Espacio.*

![El editor de audio](docs/capturas/editor-de-audio-con-letra.png)

*El editor de audio: la voz con su letra transcrita, palabra a palabra, bajo la forma de onda.*

![El asistente de mezcla y master](docs/capturas/asistente-de-mezcla.png)

*El asistente de mezcla y master: mide sonoridad, picos y estéreo, y propone cambios que aplicas con un clic.*

Las capturas salen de un proyecto de ejemplo que monta una prueba, sin abrir la app. Se regeneran con `cargo test --release -p app readme_shots -- --ignored`.

## Compilar y ejecutar

Requisitos, Windows y pruebas: [CONTRIBUTING.es.md](CONTRIBUTING.es.md) y, más detallado en inglés, [CONTRIBUTING.md](CONTRIBUTING.md).

```bash
cargo run --release
```

Los proyectos se guardan como `.pulse` (JSON). `cargo test --release` ejecuta
las pruebas, que no necesitan tarjeta de sonido ni abren ventanas. Para
renderizar sin interfaz: `target/release/pulse --render salida.wav`.

## Código

- `crates/engine`: motor de audio en tiempo real, instrumentos y efectos.
- `crates/app`: la interfaz (Session, Arrangement, editores, mezclador,
  navegador) y las herramientas de composición y mezcla.
- `crates/stems`: análisis y modelos de IA con ONNX Runtime (Audio a MIDI,
  batería, stems, letras, acordes y tonalidad).
- `crates/beats`: tempo y compases de un audio.
- `crates/paths`: dónde van ajustes, datos, cachés y logs en cada sistema.
- `crates/firma`: firma Ed25519 de los paquetes que se publican.
- `crates/testplug`: plugin VST3/CLAP mínimo para probar el alojamiento de plugins.

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
- Publicar: `scripts/release.sh [patch|minor|major]` desde `main` al día (ver
  [`docs/PUBLICAR.md`](docs/PUBLICAR.md)). La versión sale de la última
  etiqueta (`v1.2.3`); el script pasa las pruebas, firma Pulse.app, crea la
  etiqueta y la publica en GitHub, de donde la app se actualiza. El paquete de
  Windows se añade a esa publicación desde Windows con
  `scripts/package-windows.ps1` (ver [`docs/BUILD_WINDOWS.md`](docs/BUILD_WINDOWS.md)).
- La CI compila y prueba Pulse en cada pull request, de forma nativa en Windows
  (`.github/workflows/windows.yml`, que además lo empaqueta e instala) y en
  Apple Silicon (`.github/workflows/macos.yml`, que lo empaqueta en `main`).
  Sus paquetes son de prueba, no publicaciones.

## Modelos de IA

Todos corren en la CPU con ONNX Runtime, sin conexión.

- **Audio a MIDI.** Las melodías de una voz salen de pYIN
  (`crates/stems/src/pitch.rs`, igual que `librosa.pyin`). Los instrumentos y
  acordes salen de Basic Pitch (Spotify, Apache 2.0, ver
  `crates/stems/models/BASIC_PITCH_NOTICE`), un modelo de 230 KB que va dentro
  de la app. El port a Rust está en `crates/stems/src/transcribe.rs`, y su
  prueba compara las notas con las del original:

      python scripts/transcribe/make_reference.py   # (con `basic-pitch[onnx]`)
      cargo test --release -p stems transcribe

  Para comparar pYIN con librosa en un archivo: `PULSE_PITCH_IN=voz.wav
  PULSE_PITCH_OUT=f0.txt cargo test --release -p stems dump_track -- --ignored --nocapture`.

- **Separación de stems.** HTDemucs (Meta, MIT). El modelo (`htdemucs.onnx`,
  174 MB) no va dentro de la app: se descarga una vez de la publicación
  `modelo-htdemucs-1` del repositorio y se guarda en `Models`, dentro de la
  carpeta de datos (`~/Library/Application Support/Pulse` en macOS,
  `%LOCALAPPDATA%\Pulse` en Windows). Para regenerarlo, en un entorno de Python
  con `demucs onnx onnxruntime soundfile` (`demo.wav` es la canción de
  ejemplo de Pulse, `target/release/pulse --render demo.wav`):

      python scripts/stems/export_htdemucs.py salida/
      python scripts/stems/make_reference.py salida/ demo.wav
      PULSE_STEMS_DIR=salida/ cargo test --release -p stems -- --include-ignored

  Las pruebas comparan el espectrograma, la reconstrucción, la red y la
  separación de una canción con los resultados de PyTorch. Si el archivo
  cambia, hay que actualizar su tamaño y su SHA-256 en
  `crates/app/src/stems_ui.rs` y publicarlo con otra etiqueta.

- **Letras.** Whisper small cuantizado (OpenAI, MIT, ver
  `crates/stems/models/WHISPER_NOTICE`), en `crates/stems/src/whisper.rs`. Sus
  251 MB se descargan la primera vez que se usa, con aviso, de una revisión
  fija de `onnx-community/whisper-small` en Hugging Face, con su SHA-256
  comprobado, y se guardan en `Models/whisper-small`. El log-mel se compara con
  el de Hugging Face (`scripts/lyrics/make_reference.py`).

- **Batería y tempo.** La IA de batería (`crates/stems/models/DRUMS_NOTICE`) va
  dentro de la app. El tempo y los compases salen de Beat This! (JKU Linz), con
  su port `beat-this`.

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

Para contribuir: [CONTRIBUTING.es.md](CONTRIBUTING.es.md). Cómo se relacionan el núcleo libre y posibles servicios de pago: [docs/PROJECT_MODEL.md](docs/PROJECT_MODEL.md) (en inglés).

### Marcas

Ableton y Live son marcas de Ableton AG; VST es una marca de Steinberg Media
Technologies GmbH; CLAP es un estándar de free-audio; Roland, TR-808 y TR-909
son marcas de Roland Corporation. Se nombran solo para describir compatibilidad
(importar sets `.als`, alojar plugins VST3 y CLAP) o un estilo de sonido. Pulse
no tiene relación con ninguna de ellas ni cuenta con su respaldo.
