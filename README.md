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
| ⌘T / ⇧⌘T | Nueva pista MIDI / de audio |
| ⌘I | Importar un set de Ableton Live (`.als`) |
| M | Teclado del ordenador como teclado MIDI |
| F9 | Grabar en el Arrangement |
| ⇧⌘R | Exportar audio |
| Supr | Borrar el clip seleccionado |

- **Session**: ▶ de un clip lo lanza (cuantizado), clic lo abre en el editor, doble clic en un slot vacío crea
  uno, clic derecho ofrece duplicar, copiar al Arrangement, color y borrar. Las escenas están en la columna Master.
- **Arrangement**: doble clic en una pista crea un clip; arrastrar lo mueve (también entre pistas); el borde
  derecho cambia su longitud; la franja superior define el loop; clic en la regla mueve el cursor.
  Lanzar clips de sesión anula el Arrangement de esa pista hasta pulsar **▶ Arrangement**.
- **Piano roll**: clic en una nota la selecciona (Shift añade), arrastrar la mueve (Alt copia), el borde
  derecho la alarga, arrastrar en vacío selecciona con un rectángulo, doble clic crea o borra. **B** activa
  el modo dibujo (clic añade o borra). Flechas ←→ mueven en la rejilla (Shift: un compás), ↑↓ transponen
  (Shift: una octava). ⌘A, ⌘C, ⌘X, ⌘V (pega en el marcador naranja, que se coloca con un clic en vacío),
  ⌘D duplica y Supr borra las notas seleccionadas. **⌘U** cuantiza a la rejilla elegida (1/4 a 1/32) con
  la intensidad indicada, opcionalmente también los finales. El carril de velocidad, debajo, se edita
  arrastrando. Clic en una tecla la escucha.
- **Mezclador**: envíos A/B, panorama, fader en dB con medidor, activador (silencio) y solo.
  Dos retornos (Reverb y Delay) y el Master, todos con su cadena de efectos.
- **Audio**: arrastra archivos (WAV, AIFF, FLAC, MP3, OGG, M4A) a la ventana, usa "Añadir muestra…" en el
  navegador, o haz doble clic en un slot o en el Arrangement de una pista de audio. Los loops se detectan por su
  duración (compases enteros) y se sincronizan al tempo; los sonidos cortos quedan sin warp. En el editor de
  muestra eliges el modo de warp (Off, Re-Pitch, Stretch), el BPM original, el inicio (clic en la forma de
  onda), la longitud, la ganancia y la transposición (en Stretch).
- **Tocar y grabar MIDI**: los teclados MIDI conectados se detectan al arrancar (botón MIDI para volver a
  buscar). Con **M** el teclado del ordenador toca notas: A S D F G H J K L son las teclas blancas desde C,
  W E T Y U O P las negras, Z/X cambian de octava y C/V la velocidad. Las notas van a las pistas armadas (●)
  o, si no hay ninguna, a la pista seleccionada. Para grabar en Session, arma la pista y pulsa el ● de un slot
  vacío: la toma empieza en el siguiente compás y, al volver a pulsarlo, se cierra redondeada a compases y
  sigue sonando en bucle. Para grabar en el Arrangement pulsa ● en el transporte (F9) con pistas armadas.
- **Groove**: en el transporte, **Rec Q** ajusta a la rejilla elegida (1/4 a 1/32, también tresillos) el
  inicio de las notas MIDI al terminar cada toma, sobre la rejilla de la canción. **Swing** retrasa al
  reproducir los contratiempos de 1/8 o 1/16 (100 % = shuffle de tresillo) en todos los clips MIDI, sin
  cambiar las notas; también se aplica al exportar. El piano roll admite rejilla y notas de tresillo.
- **Grabar audio**: arma una pista de audio (●); la primera vez macOS pide permiso para usar el micrófono
  (concédeselo a la Terminal si ejecutas Pulse desde ella). Su medidor pasa a mostrar el nivel de entrada.
  Graba igual que MIDI: ● en un slot vacío o ● del transporte. Las tomas se guardan como WAV en `Recordings/`
  junto al proyecto (o en `~/Music/Pulse/Recordings` si aún no lo has guardado) y quedan sincronizadas al tempo.
  No hay monitorización de la entrada todavía: usa la de tu interfaz de audio.
- **Automatización**: en el Arrangement, el botón **A** de una pista despliega su línea de automatización;
  elige el parámetro (volumen, panorama, envíos, cualquier knob del sintetizador o de los efectos) y haz clic
  para añadir puntos, arrástralos para moverlos y doble clic para borrarlos. Los controles automatizados llevan
  un punto rojo y se mueven solos al reproducir. Si mueves uno a mano, su automatización queda anulada (punto
  hueco) hasta que pulses **▶ Arrangement**, como en Live. El volumen y el panorama se suavizan dentro de
  cada bloque de audio para que los cambios no suenen a escalones.
- **Exportar**: Archivo → Exportar audio (⇧⌘R). Renderiza el Arrangement completo o la región del loop a WAV
  de 16 (con dither), 24 o 32 bits, a 44.1 o 48 kHz, incluida la cola de reverbs y delays. Puede normalizar la
  mezcla a un pico dado, o exportar una pista por archivo (stems, cada una en solo con sus envíos). Avisa si la
  mezcla satura. Desde la terminal: `pulse --render salida.wav [proyecto.pulse]` (sin proyecto, la demo).
- **Importar de Live**: ⌘I o arrastrar un `.als` a la ventana. Se importan tempo, loop, escenas, pistas MIDI y
  de audio (nombre, color, volumen, panorama, envíos, activador), clips de sesión y del Arrangement con sus
  notas o sus muestras y ajustes de warp. Las muestras se buscan por ruta y, si el set viene de otro
  ordenador, por nombre dentro de la carpeta del set. Los Drum Racks de Live pasan al Drum Rack de Pulse con
  sus muestras en los mismos pads, y los Simpler/Sampler al Sampler con su nota raíz. Los sintetizadores de
  Live no se pueden leer: esas pistas reciben Pulse Synth (o la Drum Machine si es un Drum Rack sin
  muestras). Los grupos se omiten.
- **Dispositivos**: desde el navegador, once instrumentos y doce efectos, todos automatizables:
  - **Pulse Synth**: sintetizador sustractivo sencillo.
  - **Super Saw**: hasta 7 dientes de sierra desafinados y abiertos en estéreo (leads y pads de trance/EDM).
  - **Acid Bass**: bajo monofónico con filtro de 4 polos resonante, envolvente de filtro, acento (velocidad
    > 80 %) y slide entre notas que se solapan.
  - **FM Synth**: dos operadores con realimentación (campanas, e-pianos, bajos metálicos).
  - **Drum Machine**: bombo, rimshot, caja, palmas, hats cerrado/abierto, tom y cencerro, cada uno con
    afinación, decay y nivel (mapa General MIDI; 47/48/50 tocan el tom más agudo).
  - **Sampler**: toca una muestra cromáticamente desde su nota raíz, con punto de inicio, ADSR, filtro y tres
    modos: Clásico (mientras se mantiene la nota), One-shot (entera) y Loop. Arrastra un archivo de audio al
    dispositivo o usa «Cargar…».
  - **Wavetable Synth**: sintetizador de wavetables con editor completo («Editor ⤢»):
    - 2 osciladores de wavetable con mipmaps sin aliasing, posición, octava/semitono/fino, unísono de hasta
      16 voces (detune, blend, anchura), fase y fase aleatoria, y warp (Sync, Bend +/-, PWM, Mirror,
      Quantize, FM del otro oscilador). 12 wavetables incluidas y **carga de las tuyas** (WAV con frames de
      2048 muestras, o el tamaño indicado en el chunk `clm ` del archivo, o un único ciclo).
    - Sub (seno, triángulo, cuadrada, sierra) y ruido con color.
    - Filtro Low/High 12 y 24, Band, Notch, Ladder, Comb y Formant, con drive, mezcla, seguimiento de
      teclado y elección de qué fuentes lo atraviesan; curva de respuesta en pantalla.
    - 3 envolventes (la 1 es el volumen), 4 LFOs con forma **dibujada a mano**, Free/Retrig/Env y
      sincronía al tempo, y **matriz de modulación** de 16 filas (fuentes: envolventes, LFOs, velocidad,
      nota, aleatorio y 4 macros; destino: cualquier parámetro).
    - Cada oscilador puede ser **Wavetable**, **Sample** (la muestra, cromática desde C3, con loop),
      **Granular** (nube de granos: tamaño, densidad, spray, pitch aleatorio, anchura) o **Spectral**
      (cualquier audio convertido en wavetable por su espectro; Pos recorre el tiempo de la muestra).
    - **Modulación arrastrando**: arrastra una fuente («✥ LFO 1», de la tira FUENTES o de su panel) a
      cualquier knob; el knob muestra anillos de color con lo que lo mueve y, con clic derecho, ajustas o
      quitas cada modulación.
    - **Arpegiador** (Up, Down, Up/Down, Random, Played; 1/32 a 1/2 con tresillos; 1-4 octavas; gate).
    - **Rack de efectos interno** (página FX, hasta 6 efectos de Pulse) que se guarda con el preset.
    - **Editor de wavetables**: dibuja fotogramas clave con el ratón (o parte de la tabla actual), con
      formas base, normalizar, suavizar, invertir y espejo; la tabla hace morph entre ellos y se guarda como
      WAV de wavetable estándar (frames de 2048 con chunk `clm `), que también abren otros sintes.
    - Poly, Mono o Legato con glide. 16 presets de fábrica (leads, bajos Reese/Growl/Wobble, plucks, arpegio,
      pads, teclados, riser), varios con sus efectos.
  - **Polychrome Synth**: el mismo motor organizado como un sinte de varios motores, con editor propio
    (páginas SYNTH, SEQ, FX y MATRIX):
    - Dos motores, cada uno **Analog** (de seno a triángulo, sierra y pulso con PW, unísono y **drift**),
      **Harmonic** (aditivo: número de parciales, inclinación, pares/impares y formante), Wavetable, Sample,
      Granular o Spectral, más la capa de utilidades (sub y ruido).
    - **Dos filtros** en serie o en paralelo con balance; los dos con los 9 modos.
    - Modulación: 3 envolventes, 4 LFOs dibujables, **aleatorios** (S&H y suave, con velocidad), 2
      **combinadores** (×, +, mín, máx, media de dos fuentes), 4 macros y la matriz; todo arrastrable a los
      knobs.
    - **Secuenciador de 16 pasos**: nota, velocidad, gate, probabilidad, slide y un carril de modulación
      («Seq Mod»), dibujados arrastrando sobre los pasos; toca transpuesto a la tecla que mantienes.
    - Arpegiador y rack de efectos como el Wavetable Synth. 14 presets (pads y metales analógicos, órgano y
      coro aditivos, línea acid, stab de techno, pluck melódico, doble filtro…).
  - **Modular Synth**: sinte modular de pila (editor «Editor ⤢»), en construcción por fases:
    - **Pila de hasta 32 módulos** leída de arriba abajo dentro de **grupos** (cada grupo empieza de cero):
      generadores (Analog con 4 ondas, sync y PW; Wavetable; Sample con raíz, offset y loop; Noise con
      pendiente y estéreo) que suman su señal a la de arriba; efectos (**Filter** con 7 tipos y pendiente
      1×/2×, **Distortion** con 6 tipos, bias y spread) que procesan lo de arriba; **Mix**, **Aux** y
      salidas **Output** con su envolvente, ganancia, panorama y envío a un carril, al master o a ninguna
      parte (solo como fuente de modulación). Mover, duplicar, insertar debajo y quitar módulos.
    - Generadores con Level (hasta 200 %), Semi, Cent, **Harmonic** (multiplicador; ×0 sin seguimiento
      de teclado), **Shift** en Hz, fase y **fase aleatoria**, y unísono (voces, detune, spread, blend).
    - **Modulación de audio**: arrastra el asa verde «⟿» de un módulo a Phase (FM clásica), Harmonic
      (FM lineal), Shift (FM en Hz), Level (ring), Semi (FM exponencial) de otro, o al Input de un Aux.
    - 3 carriles de efectos encadenables, moduladores (Envelope, LFO, Random, Curve, Note, Velocity,
      Macro) arrastrables a cualquier knob, 8 macros, mono/legato/glide. 10 presets.
  - **Kick Designer**: sintetizador de bombos con editor completo («Editor ⤢» en el dispositivo):
    - Capas: **cuerpo** (seno, triángulo, cuadrada o sierra, fase inicial, armónicos), **sub** senoidal limpio
      que sigue al cuerpo en fase, y **click** (12 sintetizados: Noise, Snap, Tick, Square, Zap, Metal, Thump,
      Pulse, Wood, Air, FM, Crackle; o una muestra tuya), con tono, longitud y afinación.
    - Tres envolventes **dibujadas a mano** (Tono, Amp, Drive) sobre la forma de onda, con rejilla en ms y
      «Ajustar a rejilla»: arrastra los puntos, doble clic para añadir o borrar, arrastra el círculo de un
      tramo para curvarlo, clic derecho para restablecer. P. Curve y D. Curve añaden tensión encima.
    - Proceso: distorsión Soft, Hard, Fold, Tube o Crush; EQ (graves, medios con frecuencia, agudos);
      **Punch** (compresión con ganancia) y limitador a -0.3 dBFS.
    - Afinación a una nota con lectura en Hz, o «Key» para seguir la nota tocada; duración libre o
      **sincronizada al tempo** (1/16 a un compás); sensibilidad a velocidad.
    - Osciloscopio del ataque y espectro con la fundamental detectada y su nota.
    - **Exportar WAV** de un golpe. 18 presets de fábrica por estilos.
  - **Drum Rack**: 16 pads (C1–D#2, el pad 1 abajo a la izquierda), cada uno con su muestra, afinación,
    longitud, nivel, panorama y grupo de choke. Clic en un pad para escucharlo y editarlo; arrastra una o
    varias muestras sobre los pads. Al elegirlo en una pista vacía arranca con el **Kit básico** (los sonidos
    de la Drum Machine, guardados como WAV en `~/Library/Application Support/Pulse/Kits`).
- **Presets**: cada instrumento tiene un menú «Presets» en su barra de título con los de fábrica (de momento,
  los del Kick Designer) y los tuyos: escribe un nombre y «Guardar». Se guardan con sus muestras (click, pads)
  en `~/Library/Application Support/Pulse/Presets/<instrumento>/`, uno por archivo JSON.
- **Trocear loops**: en un clip de audio, «Trocear → A Drum Rack» corta la región del clip en golpes (por
  transitorios o cada 1/16, 1/8 o 1/4) y crea una pista con un Drum Rack, un pad por golpe, y un clip MIDI en
  el mismo sitio que los toca en orden: al cambiar el tempo el groove lo sigue sin estirar el audio, y se
  pueden reordenar, quitar o repetir golpes en el piano roll. El Drum Rack también tiene «Trocear archivo…».
  Los trozos se guardan en `~/Library/Application Support/Pulse/Slices`. Un golpe mucho más flojo que la cola
  del anterior (un hat justo detrás de una caja larga) puede no detectarse: entonces, usa la rejilla.
  - Efectos: EQ Three, Compressor, **Limiter**, Saturator, **Bitcrusher**, Auto Filter, **Ducker** (bombeo
    sincronizado al tempo), **Chorus**, **Phaser**, Delay, Reverb y **Utility** (ganancia y anchura estéreo).
  Knobs: arrastrar arriba/abajo, Shift para ajuste fino, doble clic para el valor por defecto.
- `crates/engine`: motor de audio en tiempo real (sin bloqueos ni reservas de memoria en el hilo de audio).
- `crates/app`: interfaz (Session View, mezclador, panel de instrumento, piano roll).

## Hoja de ruta

1. Envolventes de automatización dentro de los clips de sesión; curvas entre puntos; grooves por clip.
2. Monitorización y compensación de latencia al grabar audio; grabar la sesión en el Arrangement;
   cuantización de lo grabado.
3. Warp con marcadores múltiples y algoritmos de stretch de mayor calidad.
4. Multisamples (zonas por tecla y velocidad) en el Sampler; marcadores de troceado editables a mano.
5. Sintes del motor de wavetables: multisample, efectos modulables desde la matriz, buses de efectos en
   paralelo, polirritmos en el secuenciador, lectura de presets de otros sintes.
6. Plugins VST3 / CLAP.

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
  actualiza.

## Separación de stems

«Separar en stems» (menú de un clip de audio, o el editor de clips de audio) usa
HTDemucs (Meta, licencia MIT) con ONNX Runtime, en la CPU y sin conexión. El
modelo (`htdemucs.onnx`, 174 MB) no va dentro de la app: se descarga una vez de
la publicación `modelo-htdemucs-1` del repositorio y se guarda en
`~/Library/Application Support/Pulse/Models`.

Para regenerarlo (en un entorno de Python con `demucs onnx onnxruntime soundfile`):

    python scripts/stems/export_htdemucs.py salida/
    python scripts/stems/make_reference.py salida/ demo.wav
    PULSE_STEMS_DIR=salida/ cargo test --release -p stems -- --include-ignored

Las pruebas comparan el espectrograma, la reconstrucción, la red y la separación
de una canción con los resultados de PyTorch. Si el archivo cambia, hay que
actualizar su tamaño y su SHA-256 en `crates/app/src/stems_ui.rs` y publicarlo
con otra etiqueta.
