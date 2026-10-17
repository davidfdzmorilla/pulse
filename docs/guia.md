# Guía de Pulse

Esta guía se ve dentro de la app en Ayuda → Guía de Pulse. Cada función nueva
añade o corrige su tema aquí.

## Primeros pasos

Pulse tiene dos vistas, como Live: **Session**, para lanzar clips en bucle e improvisar, y **Arrangement**, la línea de tiempo de la canción. **Tab** cambia de una a otra.

- Arriba está el **transporte**: tempo, metrónomo, reproducir, parar, grabar, loop y más.
- A la izquierda, el **navegador**: instrumentos, efectos, plugins, muestras y tu biblioteca. **☰** lo muestra u oculta.
- Abajo, el **panel de detalle**: el editor del clip seleccionado o los dispositivos de la pista. **⇧Tab** cambia entre los dos.
- Abajo del todo, la **barra de estado**: cuenta lo que acaba de pasar y por qué algo no se ha podido hacer. Si no hay mensaje, recuerda los atajos principales.

### Hacer sonar algo en un minuto

- Crea una pista MIDI con **⌘T** y elige un instrumento en el navegador (clic en INSTRUMENTOS).
- Haz doble clic en un slot vacío de la pista: se crea un clip MIDI de un compás y se abre en el editor.
- Haz doble clic en el editor para poner notas, o pulsa **M** y toca con el teclado del ordenador.
- Pulsa el ▶ del clip para lanzarlo, y **Espacio** para parar.

La primera vez Pulse abre una canción de demostración (Drums, Bass, Chords, Lead). Después abre el último proyecto (se puede cambiar en Ajustes).

## Transporte

La barra de arriba, de izquierda a derecha. Si la ventana es estrecha, pasa a dos filas.

- **Tempo**: arrastra o escribe (20–999 BPM).
- **●** junto al tempo: **metrónomo**.
- **Cuantización de lanzamiento**: cuándo arrancan los clips que lanzas (Ninguna, 1/16 … 1 Compás, 2 Compases). Por defecto, 1 Compás.
- **Posición** (compás. tiempo. semicorchea). Durante la cuenta atrás muestra «Cuenta N».
- **▶ Reproducir** y **■ Parar** (Espacio). Parar detiene todo, incluida la grabación, y vuelve al marcador de inicio.
- **● Grabar** (F9): graba en el Arrangement (ver «Grabar»).
- **Cuenta atrás**: Sin cuenta, 1, 2 o 4 compases de metrónomo antes de grabar.
- **A — Armar automatización**: si está encendido, al grabar se graban los controles que muevas.
- **⟲ Loop** del Arrangement, con el inicio y la longitud del loop al lado (la región se elige en la franja de arriba del Arrangement).
- **Punch in** y **punch out** (los dos botones rojos junto al loop): al grabar en el Arrangement, solo se graba desde el inicio del loop y/o hasta su final (ver «Grabar»).
- **Rec Q**: cuantiza el inicio de las notas MIDI grabadas (Off, 1/4, 1/8, 1/8T, 1/16, 1/16T, 1/32).
- **Swing** (0–100 %) y su rejilla (1/8 o 1/16): retrasa los contratiempos al reproducir, sin cambiar las notas. 100 % es un shuffle de tresillo. También se aplica al exportar. Vale para todos los clips MIDI, salvo los que tengan su propio groove (ver «Editor MIDI»).
- **▶ Arrangement**: se ilumina cuando hace falta. Devuelve al Arrangement las pistas que están tocando clips de Session y reactiva la automatización que hayas movido a mano.
- **Teclado**: el teclado del ordenador como teclado MIDI (M).
- **MIDI**: encendido si hay teclados MIDI conectados. Clic para volver a buscarlos.
- **Asignar**: asignar knobs y botones de un controlador MIDI (ver «Controladores MIDI»).
- A la derecha: **☰** (navegador), **Session** / **Arrangement** y el medidor del master.

En la barra de estado, a la derecha, aparece «↑ Pulse X disponible» cuando hay una versión nueva.

## Session

Una columna por pista, con 8 slots (escenas). A la derecha, el botón **+** (nueva pista MIDI), los retornos **A Reverb** y **B Delay** y la columna **Master** con las escenas.

### Clips

- **▶** de un clip lo lanza en el siguiente punto de cuantización (y arranca el transporte si estaba parado). Mientras espera, el ▶ parpadea. Una barra abajo muestra por dónde va.
- **Clic en el clip**: lo selecciona y lo abre en el editor.
- **⌘ clic / ⇧ clic**: añade o quita clips de la selección.
- **Doble clic en un slot vacío**: en una pista MIDI crea un clip vacío de un compás; en una de audio abre el selector de archivos.
- **Arrastrar un clip**: mueve los clips seleccionados a otros slots; con **⌥** los copia. Los slots de destino se marcan en azul si caben, en rojo si no. Cada clip necesita un slot libre en una pista de su tipo (audio o MIDI).
- **Arrastrar desde un slot vacío**: selecciona con un recuadro los clips y pistas que toque.
- **El cuadrado** de un slot vacío para la pista. **Parar pista** está bajo cada columna y **Parar todos los clips** bajo el Master.

### Menú del clip (clic derecho)

- **Lanzar**.
- **Copiar al Arrangement**: lo pone al final del Arrangement de su pista, en un compás.
- Solo audio: **Separar en stems…**, **Consolidar**, **Detectar tempo** y **Usar el tempo de este clip en el proyecto**.
- **Duplicar** (⌘D): al siguiente slot libre de abajo.
- **Eliminar** (Supr o ⌫).
- Colores.

### Escenas

- Clic en una escena de la columna Master la lanza: cada pista toca su clip de esa fila, y las que no tienen clip ahí se paran.
- Clic derecho en la escena: **Nombre de la escena**.

### Grabar en un slot

En una pista armada (●), los slots vacíos muestran un punto rojo. Clic en el punto: la toma empieza en el siguiente punto de cuantización (después de la cuenta atrás si la hay y el transporte estaba parado). El slot se pone rojo con «Grabando · compás N». Vuelve a hacer clic para terminar: la toma se redondea a compases enteros y sigue sonando en bucle. Solo se graba en slots vacíos.

## Arrangement

La línea de tiempo. Arriba la **franja del loop**, debajo la **regla** con los números de compás, y una fila por pista.

### Barra de herramientas

- **Zoom**. El zoom también decide la rejilla: con zoom alto los clips se ajustan a tiempos; con zoom bajo, a compases.
- **Seguir cursor**: al reproducir, la vista se desplaza con el cursor.

### Loop y cursor

- **Arrastra en la franja de arriba** para marcar el loop (y se activa). ⟲ en el transporte lo activa o desactiva.
- Al dar la vuelta, las notas de los clips terminan en el final del loop; las que mantienes en el teclado siguen sonando.
- **Clic en la regla**: mueve el cursor y el marcador de inicio. **Arrastrar en la regla**: recorre la canción.

### Clips

- **Doble clic en un hueco**: en una pista MIDI crea un clip vacío de un compás; en una de audio abre el selector de archivos.
- **Clic**: selecciona el clip y lo abre en el editor. **⌘ clic / ⇧ clic**: varios.
- **Arrastrar**: mueve los clips seleccionados, también a otras pistas del mismo tipo. **⌥ + arrastrar** los copia.
- **Borde derecho** (el cursor cambia a ↔): cambia la longitud. Si el clip es más largo que su contenido, este se repite en bucle (unas líneas marcan cada vuelta).
- **Arrastrar en un hueco**: recuadro de selección de clips y pistas (⌘ o ⇧ añaden).
- Suelta archivos de audio en una pista de audio para ponerlos en ese tiempo, o en un hueco vacío para crear una pista nueva.

### Menú del clip (clic derecho)

- **Tomas** (en grabaciones en bucle): elegir la toma que suena y **Mostrar las tomas en filas…** (ver «Tomas y comping»).
- Solo audio: **Separar en stems…**, **Detectar tempo**, **Usar el tempo de este clip en el proyecto**, **Consolidar**.
- **Dividir aquí** (⌘E corta en el cursor), **Unir N clips** (⌘J).
- **Copiar**, **Cortar**, **Duplicar** (⌘D), **Eliminar** (⌫).
- Con un solo clip: **Nombre** y **Color**.

### Menú de un hueco (clic derecho)

- **Pegar aquí**, si hay algo copiado.
- **Añadir muestra aquí…** (audio) o **Nuevo clip MIDI aquí** (MIDI).

### Cabecera de pista

- Botones: **número** (activador: apagado = silencio), **S** (solo), **●** (armar), **In/Auto/Off** (monitorización, solo audio; cada clic pasa al siguiente modo) y **A** (carril de automatización).
- Clic derecho: **Nombre**, **Color**, **Mostrar/Ocultar automatización**, **Mostrar/Ocultar tomas**, **Duplicar**, **Eliminar** y **Congelar**.

Las pistas que están tocando clips de Session se ven oscurecidas: **▶ Arrangement** las devuelve.

## Pistas

- **⌘T**: pista MIDI nueva (con Pulse Synth). **⇧⌘T**: pista de audio nueva. Máximo 64 pistas.
- **Seleccionar**: clic en la cabecera; **⌘ clic** añade o quita; **⇧ clic** selecciona un rango.
- **Mover**: arrastra la cabecera; se mueven todas las seleccionadas (no mientras se graba).
- **Duplicar pista** / **Eliminar pista**: en el clic derecho de la cabecera o en el menú Crear. Con varias seleccionadas actúan sobre todas. Las copias se llaman «<nombre> copia». Siempre queda al menos una pista; ⌘Z deshace.
- **Nombre y color**: clic derecho en la cabecera.

### Congelar

**Congelar pista** (clic derecho en la cabecera) renderiza a audio lo que suenan su instrumento y sus efectos, y la pista pasa a sonar ese audio: deja de gastar CPU. El volumen, el panorama y los envíos siguen en vivo.

- Una pista congelada lleva un marco azul y ✱. Su instrumento y efectos no se pueden editar.
- **Descongelar pista** (o el botón «Descongelar» en el panel de abajo) vuelve a lo de antes. Si cambias el contenido de una pista congelada, se descongela sola.
- **Convertir en audio**: la pista pasa a ser una pista de audio con lo congelado, para siempre.
- Solo se congela una pista a la vez. Una pista de audio sin efectos no necesita congelarse.

## Seleccionar, copiar y pegar clips

- **Seleccionar** funciona igual en las dos vistas: clic, ⌘ clic / ⇧ clic, o un recuadro arrastrando desde un hueco.
- **⌘C / ⌘X / ⌘V**: copiar, cortar y pegar los clips seleccionados, también de varias pistas a la vez y de una vista a otra.
- **Pegar en el Arrangement**: justo después de los clips seleccionados, o en el compás del cursor si no hay ninguno. Cada pista copiada va a la pista que le corresponde, manteniendo las distancias. **Pegar aquí** (clic derecho en un hueco) pega en ese punto exacto.
- **Pegar en Session**: desde el slot seleccionado hacia abajo; si no caben, más abajo.
- **⌘D**: duplicar (en el Arrangement, justo detrás; en Session, al siguiente slot libre).
- **⌫ / Supr**: eliminar.
- En el editor de clips, si hay notas seleccionadas, estos atajos actúan sobre las notas.
- Mientras escribes en un campo de texto, los atajos son del campo.

## Dividir, unir y consolidar

- **⌘E — Dividir clip**: corta los clips seleccionados del Arrangement por el cursor. **Dividir aquí** (clic derecho) corta donde hiciste clic. Cada parte sigue sonando lo mismo que antes, también en el corte; no se tocan los archivos.
- **⌘J — Unir clips**: dos o más clips de la misma pista en uno, del principio del primero al final del último. Los MIDI juntan sus notas; los de audio se renderizan a un archivo nuevo, con silencio en los huecos.
- **Consolidar** (clic derecho en un clip de audio): lo que suena el clip (con su warp, transposición y ganancia) pasa a un archivo de audio nuevo solo con su trozo. Pierde sus tomas.
- Dos clips de audio seguidos (sin hueco entre ellos, como las partes de un clip dividido o los trozos de tomas) se funden entre sí durante 10 ms en el corte, en vez de bajar cada uno a silencio. Al unirlos, el archivo nuevo conserva ese fundido.
- Por ahora unir y consolidar no aplican los efectos de la pista.

## Grabar

### Qué pistas graban

Las **armadas** (●). Si pulsas grabar sin ninguna armada, se arman las seleccionadas. Las notas en vivo van a las pistas armadas o, si no hay ninguna, a la seleccionada.

### En el Arrangement

- **F9** o **●** del transporte: empieza a reproducir y a grabar desde donde está el cursor (después de la cuenta atrás si la hay).
- Vuelve a pulsar F9 o ● para dejar de grabar (sigue sonando), o **Espacio** para parar todo.
- **MIDI**: un clip «Toma» que empieza donde empezaste y se redondea a compases. Se aplica Rec Q.
- **Audio**: un clip por pista de audio armada.
- Con **A** (Armar automatización) encendido, también se graban los controles que muevas.
- Lo nuevo se pone encima: los clips que ya había no se borran ni se recortan.

### Grabar la Session en el Arrangement

Mientras grabas en el Arrangement (F9), los clips de Session que suenan quedan escritos en el Arrangement, en su pista y en el momento exacto en que sonaron. Da igual que los lances uno a uno, con escenas o que ya estuvieran sonando antes de grabar (en ese caso, desde donde empezó la grabación).

- Así puedes improvisar una canción lanzando clips y escenas, y que quede montada en el Arrangement.
- Si solo hay clips de Session sonando, grabar no arma ninguna pista.
- Lo que hubiera en el Arrangement de esa pista en ese tramo se sustituye (como en Live).
- Cuando terminas, las pistas siguen tocando la Session: pulsa **▶ Arrangement** para escuchar lo grabado.
- Con el loop activado, cada clip se escribe una sola vez. El punch in/out también se aplica.

### Punch in / out

Para regrabar solo un trozo sin tocar lo de alrededor. Marca el trozo con la región del loop (franja de arriba del Arrangement) y enciende los botones del transporte:

- **Punch in**: lo que suene antes del inicio del loop no se graba. Pon el cursor un poco antes y graba: oyes lo anterior para entrar a tempo, y la toma empieza justo en el inicio.
- **Punch out**: la grabación se para sola al llegar al final del loop y la reproducción sigue.
- Funcionan con el loop activado o no. Con el loop activado y punch in, la toma empieza en el inicio del loop (lo de antes no queda como clip aparte) y cada vuelta es una toma.
- En el Arrangement, unas líneas rojas marcan dónde empieza y dónde acaba la grabación.
- Valen para audio, MIDI y automatización. Se guardan con el proyecto.

### En Session

Ver «Grabar en un slot» en el tema Session.

### Audio

- La primera vez macOS pide permiso para usar el micrófono.
- La entrada se elige en Ajustes → «Entrada (grabar audio)». Se abre al armar una pista de audio.
- Las grabaciones son WAV en `Samples/Grabaciones` dentro de la carpeta del proyecto (o en `Sin guardar/Grabaciones` si aún no lo has guardado; al guardar se mueven).

### Cuenta atrás

Junto al botón de grabar: **Sin cuenta**, **Cuenta 1 c.**, **Cuenta 2 c.**, **Cuenta 4 c.**. Suena el metrónomo esos compases antes de empezar (con el primer tiempo acentuado) y la posición muestra «Cuenta N». Vale para el Arrangement y los slots de Session cuando el transporte está parado. Parar la cancela.

### Monitorización (In · Auto · Off)

Solo en pistas de audio: en Session, tres botones encima del mezclador; en el Arrangement, un botón que va cambiando.

- **Auto** (por defecto): oyes la entrada mientras la pista está armada.
- **In**: oyes siempre la entrada, en vez de los clips.
- **Off**: no oyes la entrada (usa la del micrófono o la interfaz).

Con la pista armada, su medidor muestra el nivel de entrada. Usa auriculares para que el micrófono no recoja lo que suena por los altavoces.

### Latencia

Pulse descuenta la latencia de la entrada y de la salida para que las tomas caigan a tempo. En Ajustes se ven las dos y está el **Ajuste fino de la grabación** (−200 a +200 ms): si las tomas de audio quedan tarde, súbelo; si quedan pronto, bájalo.

## Tomas y comping

Si grabas audio en el Arrangement con el **loop activado** y das varias vueltas, cada vuelta es una **toma**.

- Lo que grabaste antes del inicio del loop queda en un clip aparte.
- Sobre el loop queda un clip con todas las tomas («Toma 1…N»). Suena la última.
- Una vuelta a medias cuenta si dura al menos medio tiempo; lo que no llegaste a grabar queda en silencio.
- En MIDI no hay tomas: las notas de todas las vueltas se juntan en un clip (overdub).

### Elegir toma

Clic derecho en el clip → **Tomas** → la que quieras (la que suena lleva ✓).

### Comping: lo mejor de cada toma

Al terminar la grabación aparece **una fila por toma** debajo de la pista, con su forma de onda. Lo que está sonando se ve iluminado.

- **Arrastra en una fila**: esa toma suena en ese trozo. El clip se corta ahí y el trozo lee esa toma desde el mismo punto.
- **Clic en una fila**: esa toma en todo el clip.
- Los trozos contiguos que vuelven a sonar la misma toma se juntan solos.
- Las filas se muestran u ocultan con **Mostrar tomas** (clic derecho en la cabecera) o **Mostrar las tomas en filas…** (clic derecho en el clip).
- ⌘Z deshace. El paso de una toma a otra lleva un fundido cruzado corto, así que no se oye el corte.

## Automatización

### En el Arrangement

- El botón **A** de la cabecera muestra el carril de automatización de la pista. Elige el parámetro en el desplegable (volumen, panorama, envíos, cualquier knob del instrumento, de los efectos o de un plugin; con muchos parámetros aparece un buscador). Los que ya tienen automatización llevan ●.
- **Clic**: añade un punto. **Arrastrar**: lo mueve. **Doble clic**: lo borra.
- **⌥ + arrastrar** un tramo entre dos puntos lo **curva**: hacia arriba abomba la curva hacia arriba, hacia abajo la hunde. Sirve para fundidos y subidas más naturales. **⌥ + doble clic** en el tramo lo vuelve recto.
- **Borrar automatización** (en la cabecera o con clic derecho en el carril) y **Ocultar carril**.
- Los controles automatizados llevan un punto rojo y se mueven solos. Si mueves uno a mano, su automatización queda **anulada** (punto hueco, carril gris) hasta que pulses **▶ Arrangement**.

### Grabar automatización

Con **A** (Armar automatización) encendido en el transporte, al grabar en el Arrangement se graba cualquier control que muevas: con el ratón, con un knob MIDI asignado o en la ventana de un plugin. Lo grabado sustituye a los puntos que había en ese tramo. Al grabar un clip en Session, los controles de esa pista se graban como envolventes del clip.

### Envolventes de clip

En el editor de un clip, el desplegable **Carril** tiene **Velocidad** y **Envolventes del clip**: automatización dentro del clip, que se repite con él. Clic añade un punto, arrastrar lo mueve, doble clic lo borra, y **⌥ + arrastrar** un tramo lo curva (⌥ + doble clic lo endereza). **Borrar envolvente** la quita.

## Editor MIDI

Se abre al seleccionar un clip MIDI. En pistas de batería las filas llevan el nombre de cada sonido.

### Barra

- Nombre del clip, **Loop** (1, 2, 4, 8 o 16 compases), **Rejilla** (1/4 a 1/32, con tresillos), **Nota** (longitud de las notas nuevas).
- **Dibujar** (B): modo lápiz.
- **Groove**: el del proyecto (el swing del transporte) o **uno propio para este clip**, con **Swing** y su rejilla, **Acentos** (contratiempos más suaves) y **Humanizar** (pequeños desplazamientos al azar de tiempo y velocidad, distintos en cada vuelta). Tiene grooves listos: Recto, Swing 1/16, MPC 1/16, Shuffle 1/8 y Humano. Se aplica al tocar: las notas no se mueven.
- **Cuantizar** (⌘U) con su **intensidad** (%) y **finales** (cuantizar también el final de las notas). Actúa sobre las notas seleccionadas o, si no hay, sobre todas.
- **-12, -1, +1, +12**: transponer. **×2**: duplica el loop copiando las notas. **Zoom**.

### Notas

- **Doble clic** en vacío: crea una nota. Doble clic en una nota: la borra.
- **Clic**: selecciona (⇧ añade). **Arrastrar**: mueve (⌥ copia). **Borde derecho**: alarga.
- **Arrastrar en vacío**: recuadro de selección.
- **Clic en vacío**: deselecciona y pone el marcador naranja, donde pega ⌘V.
- **Modo dibujo (B)**: clic añade o borra; clic y arrastrar añade una nota y la alarga.
- **Clic en una tecla** del teclado de la izquierda: la escuchas.

### Teclado

- **⌘A** seleccionar todas, **⌘C / ⌘X / ⌘V** copiar, cortar y pegar (también entre clips), **⌘D** duplicar, **⌫** borrar, **Esc** deseleccionar.
- **← →** mueven por la rejilla (⇧: un compás). **↑ ↓** transponen (⇧: una octava).

### Velocidad

En el carril de abajo, con **Velocidad** elegido, arrastra arriba o abajo cerca de una nota; las seleccionadas cambian juntas.

## Editor de audio

Se abre al seleccionar un clip de audio.

- **Warp**: **Off** (a su velocidad, sin seguir el tempo), **Re-Pitch** (sigue el tempo cambiando velocidad y tono) o **Stretch** (sigue el tempo manteniendo el tono).
- **BPM original** con **÷2** y **×2**.
- **Longitud** en tiempos y **Ajustar al archivo**.
- **Ver**: **El clip** o **Archivo entero**. En «Archivo entero», clic o arrastrar en la forma de onda mueve el **inicio** del clip.
- **Inicio** en segundos (puede ser negativo, para empezar en un compás antes del archivo).
- **Gain** (−36 a +12 dB) y **Transp.** (±24 semitonos, solo en Stretch).
- **Marcadores** de warp y **Quitar**. **Trocear → A Drum Rack**. **Tempo: Detectar / Usar en el proyecto**. **Separar en stems**.
- **Carril**: envolventes del clip (ver «Automatización»).

### Marcadores de warp

Para audios cuyo tempo varía. En la franja bajo la forma de onda (con Warp distinto de Off):

- **Doble clic**: añade un marcador (el primero añade también uno al inicio y otro al final). Doble clic en un marcador lo quita.
- **Arrastrar un marcador**: ese punto del audio cae en ese tiempo; los tramos de los lados se aceleran o frenan.

## Tempo de los audios

Al importar o soltar un audio, Pulse busca su tempo y sus compases (aproximadamente un segundo de análisis por minuto de audio). Si encuentra un ritmo claro, el clip pasa a **Stretch** a ese tempo, empieza en el compás del primer tiempo fuerte y dura compases enteros. Si el tempo varía, pone un marcador de warp en cada compás. Así el audio va al ritmo del metrónomo.

- **Detectar tempo** (clic derecho en un clip de audio, o «Detectar» en el editor): vuelve a analizarlo.
- **Usar el tempo de este clip en el proyecto**: pone el proyecto al tempo del clip.
- Si no encuentra un ritmo claro, el clip suena a su velocidad.

## Trocear y separar stems

### Trocear a un Drum Rack

En el editor de audio, **Trocear** (Transitorios, 1/16, 1/8 o 1/4) y **A Drum Rack**: corta el clip en golpes (hasta 16), cada uno en un pad de un Drum Rack en una pista nueva, con un clip MIDI que los toca en orden en el mismo sitio. Al cambiar el tempo el groove lo sigue sin estirar el audio, y puedes reordenar o repetir golpes en el editor MIDI. El Drum Rack también tiene **Trocear archivo…**.

- **Editar cortes** (casilla bajo «Trocear»): muestra los cortes numerados sobre la forma de onda, con una franja debajo. **Arrastra** un corte para moverlo, **doble clic** en uno para quitarlo o en un hueco para añadir otro (hasta 16, uno por pad). **A Drum Rack** trocea entonces por tus cortes. Cambiar el modo (Transitorios, 1/16…) vuelve a buscar los cortes.

### Separar en stems

Clic derecho en un clip de audio → **Separar en stems…** (o el botón del editor). Crea cuatro pistas nuevas debajo, **Batería**, **Bajo**, **Resto** y **Voz**, con el mismo clip en el mismo sitio, y desactiva la original para que suene igual.

- Se hace en este Mac, sin enviar nada a internet, y puedes seguir trabajando mientras tanto. **Cancelar** la detiene.
- La primera vez descarga el modelo (174 MB) de GitHub: hace falta acceso a GitHub (`gh auth login`).
- Una separación a la vez, y hacen falta 4 pistas libres (máximo 64).

## Audio a MIDI

Saca las notas de cualquier audio y las pone en una pista MIDI nueva, justo debajo y en el mismo sitio (el mismo slot en Session, el mismo tramo en el Arrangement). Clic derecho en un clip de audio → **Convertir a MIDI**:

- **Melodía**: una nota cada vez. Para una voz, un tarareo o un instrumento solista: canta una idea, conviértela y tócala con cualquier sinte.
- **Armonía**: todas las notas a la vez. Para acordes, piano o guitarra.
- **Batería**: cada golpe pasa a bombo, caja o charles (según dónde tiene su energía), en una Drum Machine.

Se hace en este Mac, sin internet, en unos segundos; la barra de estado muestra el progreso. Las notas siguen el tempo del clip (su warp) y su fuerza pasa a la velocidad. Se combina bien con **Separar en stems**: separa una canción y convierte el bajo o la voz en MIDI.

La melodía sigue el tono de la voz fotograma a fotograma (pYIN, el método de las herramientas para transcribir canto), también con voces muy graves, y la corta en notas donde cambia de semitono o vuelves a atacar. La armonía la encuentra Basic Pitch, un modelo de Spotify (licencia Apache 2.0) que va dentro de la app.

## Mezclador

Está debajo de cada columna en Session.

- **In / Auto / Off** (pistas de audio): monitorización.
- **A** y **B**: envíos a los retornos (A Reverb, B Delay). **Pan**: panorama.
- **Fader** de volumen con medidor (doble clic = 0 dB, ⇧ = fino).
- **Número** (activador; apagado = silencio), **S** (solo) y **●** (armar).
- Los retornos y el Master tienen su cadena de efectos: haz clic en su cabecera y mira la pestaña Dispositivos.

### Knobs

Arrastra arriba o abajo; **⇧** para ajuste fino; **doble clic** vuelve al valor por defecto. Un punto rojo indica que está automatizado (hueco si lo has anulado a mano).

## Dispositivos y efectos

La pestaña **Dispositivos** (⇧Tab) del panel de abajo muestra la cadena de la pista, el retorno o el Master seleccionado: en pistas MIDI el instrumento y después los efectos, de izquierda a derecha.

- **Añadir**: clic en un efecto del navegador (EFECTOS DE AUDIO) lo pone al final de la cadena. Clic en un instrumento sustituye el de la pista MIDI seleccionada.
- En cada efecto: el **punto** lo activa o desactiva, **◀ ▶** lo mueven y **×** lo quita.

### Efectos

- **EQ Three**: graves, medios y agudos, con sus frecuencias de corte.
- **Compressor**: umbral, ratio, ataque, release y ganancia.
- **Limiter**: ganancia, techo y release.
- **Saturator**: drive y salida.
- **Bitcrusher**: bits y reducción de muestreo.
- **Auto Filter**: paso bajo, alto o banda, con LFO sincronizado al tempo.
- **Ducker**: bombeo de volumen sincronizado al tempo (efecto sidechain sin sidechain).
- **Chorus** y **Phaser**.
- **Delay**: tiempo sincronizado (1/16 a un compás), Ping Pong, feedback y filtro.
- **Reverb**: tamaño, amortiguación y anchura.
- **Utility**: ganancia y anchura estéreo (0 % mono, 200 % muy abierto).

## Instrumentos

Se eligen en el navegador (INSTRUMENTOS) con una pista MIDI seleccionada.

- **Pulse Synth**: sintetizador sustractivo sencillo.
- **Super Saw**: hasta 7 dientes de sierra desafinados (leads y pads).
- **Acid Bass**: bajo monofónico con filtro resonante, acento y slide.
- **FM Synth**: dos operadores con realimentación (campanas, pianos eléctricos).
- **Wavetable Synth**, **Polychrome Synth** y **Modular Synth**: ver sus temas.
- **Kick Designer**: ver su tema.
- **Sampler**: toca una muestra cromáticamente desde su nota raíz. Arrastra un audio a su zona o usa **Cargar…**. Modos **Clásico** (mientras mantienes la nota), **One-shot** (entera) y **Loop**. Root, Tune, Start, filtro y ADSR.
  - **Multisample**: carga o arrastra **varias** muestras a la vez y se reparten por el teclado según la nota de su nombre («Piano C3», «Rhodes_F#2», «Strings Db4»; C3 es el do central). Cada una cubre las teclas hasta la mitad de camino de sus vecinas y suena tal cual en su nota. Varias de la misma nota son **capas de velocidad**: la más suave abajo. Las que no llevan nota van una por tecla desde C3.
  - El mapa muestra las zonas sobre el teclado (arriba, las velocidades altas); clic en una tecla para oírla. **−12 / +12** mueven todo el mapa una octava (librerías que llaman C4 al do central). **Zonas…** permite ajustar a mano las notas, la raíz y las velocidades de cada muestra.
  - Hasta 32 muestras. Cargar una sola vuelve al modo normal.
- **Drum Machine**: 8 sonidos sintetizados (bombo, rimshot, caja, palmas, hats cerrado y abierto, tom, cencerro), cada uno con Tune, Decay y Level. Clic en un sonido para escucharlo.
- **Drum Rack**: 16 pads de muestras (el 1 abajo a la izquierda, desde C1).
  - Clic en un pad lo escucha y lo selecciona. Arrastra uno o varios audios sobre los pads.
  - Por pad: Tune, Length, Level, Pan y grupo de **choke** (los pads del mismo grupo se cortan entre sí, como el hi-hat abierto y cerrado).
  - **Kit básico**: carga los sonidos de la Drum Machine. **Trocear archivo…**: corta un loop en los pads.

Los editores grandes (**Editor ⤢**) se abren de uno en uno dentro de la ventana.

## Kick Designer

Sintetizador de bombos. En la cadena tiene un editor pequeño y **Editor ⤢** abre el completo.

- **Curvas dibujadas** (pestañas **Tono**, **Amp** y **Drive**): arrastra los puntos, doble clic añade o borra, arrastra el círculo de un tramo para curvarlo, clic derecho para restablecer. **Ajustar a rejilla** en el editor grande.
- **Cuerpo** (seno, triángulo, cuadrada o sierra, con fase y armónicos), **sub** que lo sigue y **click** (12 sintetizados o una muestra tuya: arrástrala al recuadro del click).
- **Key**: la nota tocada afina el cuerpo en vez de Tune.
- **Distorsión** (Soft, Hard, Fold, Tube, Crush), **EQ**, **Punch** y **Limitador**.
- **Duración**: libre o sincronizada al tempo (1/16 a un compás).
- Osciloscopio del ataque y espectro con la fundamental y su nota.
- **Exportar WAV…**: guarda un golpe como archivo.
- 18 presets de fábrica por estilos.

## Wavetable y Polychrome

### Wavetable Synth

- Dos osciladores; cada uno puede ser **Wavetable**, **Sample**, **Granular**, **Spectral**, **Analog** o **Harmonic**. Con Pos, octava, semitono, fino, nivel, pan, unísono y warp (Sync, Bend, PWM, Mirror, Quantize, FM).
- 12 wavetables incluidas, **Cargar wavetable…** para las tuyas y **Editor de wavetables…** para dibujarlas: dibuja fotogramas clave con el ratón (o parte de los de la tabla actual) y la tabla hace morph entre ellos. Se guarda como WAV de wavetable estándar.
- **Sub**, **ruido**, **filtro** (9 modos, con su curva), 3 envolventes, 4 **LFOs dibujables** (clic derecho para formas básicas), **arpegiador** y Poly/Mono/Legato con Glide.
- **FX**: hasta 6 efectos propios, que se guardan con el preset.
- **MATRIX**: 16 rutas de modulación (fuente, cantidad, destino).
- **Modulación arrastrando**: arrastra una fuente («✚ LFO 1» de la fila FUENTES) a cualquier knob. El knob muestra anillos de color; con clic derecho ajustas o quitas cada modulación.

### Polychrome Synth

El mismo motor organizado como un sinte de varios motores (páginas SYNTH, SEQ, FX y MATRIX):

- **Dos filtros** en serie o en paralelo, con balance.
- Fuentes **aleatorias** (S&H y suave) y 2 **combinadores** de dos fuentes.
- **Secuenciador de 16 pasos** (SEQ): nota, velocidad, gate, probabilidad, slide y un carril de modulación; se dibujan arrastrando sobre los pasos. Mientras mantienes una tecla, toca los pasos transpuestos a ella. **Variar** cambia notas al azar dentro de la escala.

## Modular Synth

Un sinte modular de pila. **Editor ⤢** abre el editor.

- **Generadores**: pila de hasta 32 módulos leída de arriba abajo, dentro de **grupos** (cada grupo empieza de cero). Analog, Wavetable, Sample, Granular y Noise generan; Filter, Nonlinear Filter y Distortion procesan lo de arriba; Mix, Aux y **Output** (con destino Lane 1–3, Master o Sideband).
- Cada módulo: activar, **▲ ▼** mover, **⧉** duplicar, **×** quitar, **+** insertar debajo. **+ Añadir módulo** al final.
- **Modulación de audio**: arrastra el asa verde **⟿** de un módulo a Phase, Harmonic, Shift, Level o Semi de otro (FM, ring…), o al Input de un Aux.
- **Voz**: Poly/Mono/Legato, polifonía, rango de pitch bend, glide y **unísono** (varios modos, incluidos acordes y Shepard).
- **Carriles de efectos**: 3 carriles con hasta 8 efectos cada uno, Poly, Mute, Solo y envío a otro carril o al Master.
- **Moduladores** (hasta 32): Envelope, LFO, Curve, Random, Audio Follower, Pitch Tracker, Note, Velocity, Pressure, Pitch Wheel, MIDI CC, Macro, Remap, límites, Scale, Sample & Hold… Arrastra su **✚** a cualquier knob (también de otros moduladores). En **⋯** de cada destino: curva y límites. Clic derecho en un knob modulado para ajustarlo.
- **8 macros**.

## Presets

El menú **Presets** de la barra de cada instrumento (y de los editores grandes):

- **DE FÁBRICA**: Kick Designer, Wavetable, Polychrome y Modular tienen presets de fábrica.
- **MIS PRESETS**: los tuyos. Escribe un nombre y pulsa **Guardar** (o Intro); con el mismo nombre se sobrescribe. **×** borra uno.
- Se guardan en la carpeta `Presets` de Pulse. Los que usan muestras guardan la ruta de las muestras: si las mueves, el preset las pierde.
- En el navegador (BIBLIOTECA → Presets), **doble clic** en un preset lo aplica a la pista MIDI seleccionada.

Los plugins no usan este menú: guardan su estado con el proyecto.

## Plugins VST3 y CLAP

- En el navegador, sección **PLUGINS (VST3 · CLAP)**, pulsa **Buscar plugins VST3**. Busca en las carpetas de plugins del sistema y en las que añadas en Ajustes → Plugins.
- Clic en un **instrumento** lo pone en la pista MIDI seleccionada; clic en un **efecto** lo añade al final de la cadena.
- En el dispositivo del plugin:
  - **Editor** abre la ventana propia del plugin, que flota sobre Pulse.
  - **Parámetros…** abre todos sus parámetros en una lista, con buscador.
  - 6 knobs con sus parámetros principales (primero los automatizados).
- Todos sus parámetros se pueden automatizar y asignar a un controlador MIDI. Su estado se guarda con el proyecto.
- Si un plugin ya no está instalado, su dispositivo lo avisa y no suena.

## Controladores MIDI

### Teclados

Pulse escucha **todos** los teclados MIDI conectados, en todos los canales. El botón **MIDI** del transporte se enciende si hay alguno; clic para volver a buscar. Las notas van a las pistas armadas o, si no hay ninguna, a la seleccionada.

- **Pitch bend**: ±2 semitonos en los instrumentos de Pulse (el Modular tiene su propio rango).
- **Rueda de modulación** (CC 1): vibrato.
- El Kick Designer y las baterías no usan bend ni rueda. Los plugins reciben todo.

### Asignar MIDI (MIDI learn)

Pulsa **Asignar** en el transporte (o Transporte → Asignar controles MIDI). Se abre la ventana «Asignar MIDI».

- Toca un control de Pulse (knob, fader, parámetro de plugin o botón de transporte) y mueve un knob o pulsa un botón del controlador. **El orden da igual**: si mueves primero el knob, tienes 10 segundos para tocar el control.
- Los controles asignables se marcan con un borde mientras está activo; hacer clic los elige sin cambiarlos.
- Se pueden asignar Reproducir, Parar, Grabar, Loop, Metrónomo y Armar automatización.
- Un CC por control: asignar uno que ya se usa lo mueve.
- La lista muestra las asignaciones; **×** quita una. **Terminar** sale.
- Un CC asignado ya no llega al instrumento.
- Las asignaciones de parámetros se guardan con el proyecto; las de transporte, para todos los proyectos.

## Teclado del ordenador

Pulsa **M** (o el botón **Teclado**) para tocar con el teclado del ordenador:

- **A S D F G H J K L** (y ;): teclas blancas desde C. **W E T Y U O P**: negras.
- **Z / X**: octava abajo / arriba. **C / V**: velocidad abajo / arriba.
- El botón muestra la nota base y la velocidad («Teclado C3 · 100»).
- No toca mientras escribes en un campo de texto.

## Navegador y biblioteca

El panel de la izquierda (☰ lo muestra u oculta). Arriba pone el **destino**: la pista seleccionada.

- **INSTRUMENTOS**, **EFECTOS DE AUDIO** y **PLUGINS**: clic para usarlos (ver sus temas).
- **MUESTRAS**: **Añadir muestra…**.
- **BIBLIOTECA**: busca muestras, proyectos y presets por nombre, y explora las carpetas Samples, Proyectos, Exportaciones, Presets y las tuyas (**Añadir carpeta…**; clic derecho → **Quitar del navegador**).
  - Audio: **clic** lo escucha (otro clic lo para), **doble clic** lo añade a la pista de audio seleccionada, y se puede **arrastrar** a un slot, una pista, un pad o el Sampler.
  - Proyectos: doble clic para abrir. Presets: doble clic para aplicar.
- **PISTAS**: nueva pista MIDI o de audio.

### Arrastrar desde Finder

- Un audio suelto en un slot o una pista de audio va ahí; en un pad o el Sampler, se carga; en un hueco vacío, crea una pista de audio. Varios archivos van seguidos.
- Un `.pulse` se abre y un `.als` se importa (pregunta antes si hay cambios sin guardar).
- También funciona abrir con doble clic en Finder o soltar en el icono del Dock.

## Proyectos y carpeta de Pulse

Todo vive en la **carpeta de Pulse**, por defecto Música/Pulse (`~/Music/Pulse`; se cambia en Ajustes):

- `Proyectos/<nombre>/`: cada proyecto en su carpeta, con sus audios en `Samples/` (Importados, Grabaciones, Stems, Trozos, Consolidados, Congelados). La carpeta entera se puede copiar a otro Mac.
- `Exportaciones`, `Presets`, `Wavetables` y `Samples` (las tuyas).
- `Sin guardar`: lo que graba o crea un proyecto que aún no has guardado. Al guardarlo se mueve a su carpeta.

### Archivo

- **⌘N** nuevo, **⌘O** abrir, **Abrir recientes** (los 10 últimos), **⌘S** guardar, **⇧⌘S** guardar como, **Volver a la versión guardada**.
- Al guardar, todos los audios que usa el proyecto se copian a su carpeta.
- Antes de perder cambios sin guardar, Pulse pregunta (Guardar / No guardar / Cancelar).
- **⌘Z / ⇧⌘Z**: deshacer y rehacer.
- **Mostrar la carpeta de Pulse** la abre en Finder.
- El título de la ventana lleva * si hay cambios sin guardar.

## Importar de Ableton Live

**⌘I** (Archivo → Importar set de Live) o arrastra un `.als` a la ventana. Sustituye el proyecto actual; el `.als` no se toca.

- Se importan: tempo, loop, escenas, pistas MIDI y de audio (nombre, color, volumen, panorama, envíos, activador, solo) y los clips de Session y del Arrangement, con sus notas o sus muestras y su warp.
- Los Drum Racks con muestras pasan al Drum Rack de Pulse; Simpler y Sampler, al Sampler.
- No se importan los sintes de Live (esas pistas reciben Pulse Synth), los efectos, la automatización ni los grupos.
- Las muestras se buscan por ruta y, si el set viene de otro ordenador, por nombre en la carpeta del set.

## Exportar audio

**⇧⌘R** (Archivo → Exportar audio…):

- **Qué**: **Mezcla completa** o **Una pista por archivo (stems)** (cada pista en solo con sus envíos).
- **Rango**: **Toda la canción** o **Región del loop**.
- **Formato**: WAV de 16 bits (con dither), 24 o 32 bits; **44.1** o **48 kHz**.
- **Normalizar**: pico a un valor (no en stems).
- Se exporta el **Arrangement** (no la Session), con la cola de reverbs y delays. Avisa si la mezcla satura.
- Por defecto se guarda en la carpeta `Exportaciones`.

## Ajustes

**⌘,** (Pulse → Ajustes…). Los cambios se aplican al momento.

- **Audio**: salida, entrada (para grabar), frecuencia y tamaño del búfer (más pequeño responde antes; si oyes cortes, súbelo). Latencias medidas y **Ajuste fino de la grabación**.
- **MIDI**: dispositivos conectados y volver a buscar.
- **Plugins**: carpetas donde buscarlos y **Buscar plugins**.
- **Carpeta de Pulse**: dónde está, **Cambiar…** (lo que ya hay no se mueve) y **Mostrar en Finder**.
- **Proyectos**: abrir el último proyecto al arrancar y buscar actualizaciones al arrancar.

## Actualizaciones

Pulse busca versiones nuevas al arrancar y cada 6 horas (también en Pulse → **Buscar actualizaciones…**).

- Si hay una, la ventana muestra sus novedades: **Descargar e instalar**, **Más tarde** u **Omitir esta versión**.
- Al terminar, **Reiniciar ahora** (pregunta antes si hay cambios sin guardar).
- La versión anterior se guarda como «Pulse (anterior)»: **Volver a la versión anterior** la recupera.
- La primera vez que arranca una versión nueva enseña sus novedades.
- Hace falta acceso a GitHub (`gh auth login` o un token en «Acceso a GitHub»).

## Atajos de teclado

| Atajo | Qué hace |
|---|---|
| ⌘N | Proyecto nuevo |
| ⌘O | Abrir |
| ⌘S / ⇧⌘S | Guardar / Guardar como |
| ⌘I | Importar set de Live |
| ⇧⌘R | Exportar audio |
| ⌘Q | Salir |
| ⌘, | Ajustes (audio, MIDI, plugins) |
| ⌘Z / ⇧⌘Z | Deshacer / Rehacer |
| ⌘X / ⌘C / ⌘V | Cortar / Copiar / Pegar los clips seleccionados (o las notas seleccionadas en el editor) |
| ⌘D | Duplicar |
| ⌘E / ⌘J | Dividir el clip en el cursor / unir clips (Arrangement) |
| ⌫ | Eliminar |
| ⌘A | Seleccionar todas las notas |
| ⌘T / ⇧⌘T | Nueva pista MIDI / de audio |
| ⌘ clic / ⇧ clic | Seleccionar varias pistas (en sus cabeceras) o varios clips |
| Arrastrar cabecera | Mover las pistas seleccionadas |
| Arrastrar en un hueco | Seleccionar clips y pistas con un recuadro |
| Arrastrar clip / ⌥ | Mover los clips seleccionados / copiarlos |
| Tab | Cambiar entre Session y Arrangement |
| ⇧Tab | Cambiar entre el clip y los dispositivos |
| ⌃⌘F | Pantalla completa |
| Espacio | Reproducir / Parar |
| F9 | Grabar en el Arrangement |
| M | Teclado del ordenador como teclado MIDI |
| A S D F G H J K L | Teclas blancas (con el teclado del ordenador) |
| W E T Y U O P | Teclas negras |
| Z / X | Octava abajo / arriba |
| C / V | Velocidad abajo / arriba |
| ⌘U | Cuantizar notas (editor de clips) |
| B | Lápiz (editor de clips) |
| ← → / ↑ ↓ | Mover / transponer notas (editor de clips; ⇧: compás u octava) |
| Esc | Deseleccionar notas |

Los atajos con ⌘ también están en la barra de menús. Ayuda → Atajos de teclado muestra esta lista en una ventana.
