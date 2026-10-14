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
- **Dispositivos**: desde el navegador, ocho instrumentos y doce efectos, todos automatizables:
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
  - **Kick Designer**: sintetizador de bombos. Cuerpo senoidal afinado a una nota (o a la nota tocada con
    «Key»), con envolvente de tono (cantidad, tiempo, curva), envolvente de amplitud (ataque, hold, decay,
    curva), armónicos, click sintetizado o a partir de una muestra, y distorsión Soft, Hard o Fold. El visor
    dibuja la forma de onda y la curva de tono; trae presets (House Punch, Techno Rumble, Hard Kick, Long 808,
    Trance Thud).
  - **Drum Rack**: 16 pads (C1–D#2, el pad 1 abajo a la izquierda), cada uno con su muestra, afinación,
    longitud, nivel, panorama y grupo de choke. Clic en un pad para escucharlo y editarlo; arrastra una o
    varias muestras sobre los pads. Al elegirlo en una pista vacía arranca con el **Kit básico** (los sonidos
    de la Drum Machine, guardados como WAV en `~/Library/Application Support/Pulse/Kits`).
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
5. Plugins VST3 / CLAP.
