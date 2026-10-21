# Guía de Pulse

Esta guía se ve dentro de la app en Ayuda → Guía de Pulse. Cada función nueva
añade o corrige su tema aquí.

**En Windows**, donde la guía dice ⌘ es **Ctrl**, ⇧ es **Mayús**, ⌥ es **Alt** y ⌫ es **Supr**; la pantalla completa es **F11**. Los menús están en la barra de arriba de la ventana de Pulse, y donde dice Finder es el Explorador de archivos.

## Primeros pasos

Pulse tiene dos vistas: **Session**, para lanzar clips en bucle e improvisar, y **Arrangement**, la línea de tiempo de la canción. **Tab** cambia de una a otra.

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

- **Tempo**: arrastra o escribe (20–999 BPM). Con automatización de tempo (ver «Cambios de tempo») muestra el que suena, con un borde rojo; moverlo a mano la anula hasta **▶ Arrangement**.
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
- **Escala**: la tonalidad de la canción (ver «Tonalidad y escala»), y si los loops nuevos se ajustan a ella.
- **▶ Arrangement**: se ilumina cuando hace falta. Devuelve al Arrangement las pistas que están tocando clips de Session y reactiva la automatización que hayas movido a mano.
- **Teclado**: el teclado del ordenador como teclado MIDI (M).
- **Voz**: tocar cantando, tarareando o silbando al micrófono (ver «Tocar con la voz»).
- **MIDI**: encendido si hay teclados MIDI conectados. Clic para volver a buscarlos.
- **Asignar**: asignar knobs y botones de un controlador MIDI (ver «Controladores MIDI»).
- A la derecha: **☰** (navegador), **Session** / **Arrangement** y el medidor del master.

En la barra de estado, a la derecha, aparece «↑ Pulse X disponible» cuando hay una versión nueva.

## Session

Una columna por pista, con hasta 16 slots (escenas): se ven las que usas y una vacía más, 8 como mínimo; al poner un clip en la última, aparece otra. A la derecha, el botón **+** (nueva pista MIDI), los retornos **A Reverb** y **B Delay** y la columna **Master** con las escenas.

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
- **Acompañar**: un bajo, un arpegio, un pad o acordes rítmicos para sus acordes (ver «Acompañamiento»).
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

### Marcadores

La franja bajo la del loop tiene los **marcadores**: las partes de la canción con su nombre.

- **Doble clic** en la franja: un marcador nuevo ahí. **Clic** en uno: el cursor salta a él. **Arrastrar**: lo mueve (por la rejilla).
- **Clic derecho** en un marcador: renombrarlo, **Ir aquí** o eliminarlo; en la franja vacía, **Añadir marcador aquí**.
- El **Arreglo automático** pone uno en cada parte, y al importar de Live llegan sus locators.

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
- **Acompañar**: un bajo, un arpegio, un pad o acordes rítmicos para sus acordes (ver «Acompañamiento»).
- **Dividir aquí** (⌘E corta en el cursor), **Unir N clips** (⌘J).
- **Desactivar clip** (tecla **0**): el clip se queda donde está, en gris, pero no suena (ni sus notas, ni su audio, ni su automatización); tampoco al exportar. Otra vez lo activa. Con varios seleccionados, los desactiva todos (o los activa, si ya lo estaban todos). También en el menú de los clips de Session.
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

### Grupos

Un grupo junta varias pistas en una: suenan por **su fader y sus efectos** (como un bus), y se pliegan para ganar sitio.

- **Agrupar** (**⌘G**, menú Crear o clic derecho en la cabecera): las pistas seleccionadas pasan a un grupo nuevo donde estaba la primera. Se pueden agrupar grupos (grupos dentro de grupos).
- **Desagrupar** (**⇧⌘G**): quita el grupo; sus pistas se quedan donde están. **Sacar del grupo** (clic derecho en una pista de un grupo) la saca justo debajo.
- **Plegar**: clic en **▾** en la cabecera del grupo; sus pistas se ocultan (**▸** para volver a verlas). En Session se ocultan sus columnas y en el Arrangement sus filas.
- Las pistas de un grupo van justo debajo de él, con una barra del color del grupo en su cabecera (y sangradas en el Arrangement). **Arrastrar** el grupo lo mueve con todas sus pistas; soltar una pista justo debajo de la cabecera de un grupo la mete en él.
- **Duplicar** o **eliminar** un grupo duplica o elimina también sus pistas.
- El grupo tiene **mezclador** (volumen, panorama, envíos, activador, solo), **cadena de efectos** (en Dispositivos) y **automatización**, como cualquier pista. Un **solo** en el grupo hace sonar todas sus pistas; un solo en una de sus pistas mantiene el grupo sonando. **Desactivar** el grupo silencia todo lo suyo, también lo que sus pistas mandan a los retornos.
- **Session**: los slots del grupo se encienden donde sus pistas tienen clips; clic lanza los de esa escena (y para las demás pistas del grupo). Su ■ para todas sus pistas.
- **Arrangement**: la fila del grupo muestra en pequeño los clips de sus pistas. En ella no se ponen clips.
- Un grupo no tiene instrumento ni clips: no se arma ni recibe notas.

### Congelar

**Crear rumble** (clic derecho en la cabecera de una pista, normalmente la del bombo) añade debajo una copia que toca lo mismo por una reverb larga solo húmeda, una saturación Tube con 4× y un filtro de 24 dB a 180 Hz, con un Ducker que se aparta cuando suena la original: el rumble del techno. Ajusta la reverb, el drive o el filtro a tu gusto.

**Congelar pista** (clic derecho en la cabecera) renderiza a audio lo que suenan su instrumento y sus efectos, y la pista pasa a sonar ese audio: deja de gastar CPU. El volumen, el panorama y los envíos siguen en vivo.

- Una pista congelada lleva un marco azul y ✱. Su instrumento y efectos no se pueden editar.
- **Descongelar pista** (o el botón «Descongelar» en el panel de abajo) vuelve a lo de antes. Si cambias el contenido de una pista congelada, se descongela sola; también si cambias el tempo, el swing o la tonalidad del proyecto, o la pista de la que escucha su sidechain.
- Suena como antes también con sidechain (la pista que lo dispara se tiene en cuenta al congelar) y con cambios de tempo (el audio congelado sigue la automatización de tempo tal como sonaba). Las envolventes de volumen, panorama y envíos de los clips de Session siguen en vivo; las de los clips del Arrangement quedan grabadas en el audio.
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

- **⌘E — Dividir clip**: corta los clips seleccionados del Arrangement por el cursor. **Dividir aquí** (clic derecho) corta donde hiciste clic. Cada parte sigue sonando lo mismo que antes; no se tocan los archivos. Una nota MIDI que cruza el corte termina en él y lo que le quedaba se toca al principio de la parte derecha (vuelve a oírse su ataque).
- **⌘J — Unir clips**: dos o más clips de la misma pista en uno, del principio del primero al final del último. Los MIDI juntan sus notas; los de audio se renderizan a un archivo nuevo, con silencio en los huecos.
- **Consolidar** (clic derecho en un clip de audio): lo que suena el clip (con su warp, transposición y ganancia) pasa a un archivo de audio nuevo solo con su trozo. Pierde sus tomas.
- Dos clips de audio seguidos (sin hueco entre ellos, como las partes de un clip dividido o los trozos de tomas) se funden entre sí durante 10 ms en el corte, en vez de bajar cada uno a silencio. Al unirlos, el archivo nuevo conserva ese fundido.
- Por ahora unir y consolidar no aplican los efectos de la pista.

## Arreglo automático

Cuando tienes las ideas en escenas de Session, **Crear → Arreglo automático…** hace con ellas una canción entera en el Arrangement.

- Elige una **estructura**: **Electrónica** (intro, subida, drop, break, subida, drop, outro), **Pop** (intro, estrofa, estribillo, estrofa, estribillo, puente, estribillo, final) o **Corta** (intro, A, B, final).
- Cada parte toma una escena: si tus escenas se llaman como partes («Intro», «Estrofa», «Drop», «Estribillo», «Break»…), esas; si no, la más llena (más batería, bajo y melodía) es el estribillo o el drop, la siguiente la estrofa y la más ligera la intro.
- Cada parte lleva solo las pistas que le tocan según su **papel**, que Pulse deduce del instrumento, el nombre y las notas (batería, bajo, armonía, melodía): sin bajo en la intro, sin batería en el break… En el estribillo o el drop suena todo: si una pista no tiene clip en esa escena, trae el suyo de otra.
- **Subidas**: el último compás de la batería es un **redoble** de caja que acelera y crece. **Cortes**: el último tiempo antes del drop, en silencio, para que entre con fuerza.
- Antes de crearla, en la tabla puedes cambiar los **compases** de cada parte, **qué pistas suenan** en cada una (casillas) y los redobles y cortes.
- **Crear** sustituye lo que hubiera en el Arrangement (⌘Z lo recupera), pone un **marcador** con el nombre de cada parte, el loop en toda la canción, y te lleva al Arrangement.

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
- Lo que hubiera en el Arrangement de esa pista en ese tramo se sustituye.
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

- La primera vez macOS pide permiso para usar el micrófono. En Windows, si no graba nada, mira Configuración → Privacidad y seguridad → Micrófono («Permitir que las aplicaciones de escritorio accedan al micrófono»).
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

### Master y cambios de tempo

La última fila del Arrangement es el **Master**. Su **A** muestra su automatización: **Tempo**, volumen, panorama y los knobs de sus efectos.

- **Tempo** hace que la canción cambie de velocidad: puntos a distintos tempos dan saltos (una intro lenta, un estribillo más rápido), y una línea entre dos puntos, un accelerando o un ritardando (con **⌥ + arrastrar** se curva). El carril va de 40 a 240 BPM.
- Todo sigue el tempo: los clips MIDI, los audios en Stretch, Beats o Re-Pitch, el metrónomo, los delays y los arpegiadores. Los audios con warp **Off** no siguen el tempo (y en un cambio de tempo pueden saltar): en una canción con cambios, ponlos en **Stretch**.
- Al **exportar**, la canción dura lo que dan sus tempos.
- Mover el tempo del transporte a mano anula la automatización (lo dice su borde) hasta **▶ Arrangement**.

### Grabar automatización

Con **A** (Armar automatización) encendido en el transporte, al grabar en el Arrangement se graba cualquier control que muevas: con el ratón, con un knob MIDI asignado o en la ventana de un plugin. Lo grabado sustituye a los puntos que había en ese tramo; con el loop activado, cada pasada sustituye a la anterior donde mueves el control y deja lo de antes donde no lo tocas. Al grabar un clip en Session, los controles de esa pista se graban como envolventes del clip.

### Envolventes de clip

En el editor de un clip, el desplegable **Carril** tiene **Velocidad** y **Envolventes del clip**: automatización dentro del clip, que se repite con él. Clic añade un punto, arrastrar lo mueve, doble clic lo borra, y **⌥ + arrastrar** un tramo lo curva (⌥ + doble clic lo endereza). **Borrar envolvente** la quita. Las envolventes mandan solo mientras suena el clip: al terminar, el volumen, el panorama, los envíos y los parámetros vuelven a lo que marcan el mezclador y la automatización de la pista.

## Editor MIDI

Se abre al seleccionar un clip MIDI. En pistas de batería las filas llevan el nombre de cada sonido.

### Barra

- Nombre del clip, **Loop** (1, 2, 4, 8 o 16 compases, o cualquier número de semicorcheas en el campo **/16**: un clip de 3/16, 5/16 o 7/16 sobre el 4/4 se desplaza en cada compás, para polirritmos), **Rejilla** (1/4 a 1/32, con tresillos), **Nota** (longitud de las notas nuevas).
- **Dibujar** (B): modo lápiz.
- **Groove**: el del proyecto (el swing del transporte) o **uno propio para este clip**, con **Swing** y su rejilla, **Acentos** (contratiempos más suaves) y **Humanizar** (pequeños desplazamientos al azar de tiempo y velocidad, distintos en cada vuelta). Tiene grooves listos: Recto, Swing 1/16, MPC 1/16, Shuffle 1/8 y Humano. Se aplica al tocar: las notas no se mueven.
- **Cuantizar** (⌘U) con su **intensidad** (%) y **finales** (cuantizar también el final de las notas). Actúa sobre las notas seleccionadas o, si no hay, sobre todas.
- **Prob.** (%): cuántas de las veces que pasa el loop suena cada nota seleccionada (o todas, si no hay selección). Un hat al 60 % o una nota fantasma de caja al 30 % hacen que el loop varíe solo. Es al azar, pero siempre el mismo azar: suena igual cada vez que reproduces la canción, al exportarla y al congelar la pista. Las notas con probabilidad se ven más pálidas, con su %.
- **-12, -1, +1, +12**: transponer. **×2**: duplica el loop copiando las notas. **Zoom**.
- Con una escala en el proyecto: **Plegar** y **A la escala** (ver «Tonalidad y escala»).
- **Acordes**: escribe los acordes de la escala, progresiones enteras y sugiere el siguiente (ver «Acordes»).
- En pistas de batería, **Generar ritmo** (ver «Generar ritmos»).
- Bajo la regla, una franja con el **acorde** que suena en cada tramo (ver «Acordes»).

### Notas

- **Doble clic** en vacío: crea una nota. Doble clic en una nota: la borra.
- **Clic**: selecciona (⇧ añade). **Arrastrar**: mueve (⌥ copia). **Borde derecho**: alarga.
- **Arrastrar en vacío**: recuadro de selección.
- **Clic en vacío**: deselecciona y pone el marcador naranja, donde pega ⌘V.
- **Modo dibujo (B)**: clic añade o borra; clic y arrastrar añade una nota y la alarga.
- **Clic en una tecla** del teclado de la izquierda: la escuchas.

### Teclado

- **⌘A** seleccionar todas, **⌘C / ⌘X / ⌘V** copiar, cortar y pegar (también entre clips), **⌘D** duplicar, **⌫** borrar, **Esc** deseleccionar.
- **← →** mueven por la rejilla (⇧: un compás). **↑ ↓** transponen (⇧: una octava). Con **Plegar**, una nota de la escala.

### Velocidad

En el carril de abajo, con **Velocidad** elegido, arrastra arriba o abajo cerca de una nota; las seleccionadas cambian juntas.

## Generar ritmos

En el **Editor MIDI** de un clip de batería (Drum Machine o Drum Pads), **Generar ritmo** escribe una base en un estilo:

- **Estilos**: House, Techno, Hardgroove, Hard techno, Trap, Drum & Bass, Hip hop, Pop / Rock, Reggaetón y Breakbeat. Las bases están escritas a mano, como las tocaría un batería de cada estilo (el bombo a negras y el charles abierto a contratiempo del house, la caja en el 3 y los redobles de charles del trap, el dos-pasos del drum & bass, el dembow del reggaetón…), con sus acentos y notas fantasma. **Hardgroove** (142 BPM) hace rodar toms y congas sobre el bombo a negras, con un poco de swing, y charles que solo suenan algunas vueltas (probabilidad por nota), así que el loop no se repite igual; **Hard techno** (152 BPM) lleva un bombo grave y largo, que galopa en Intenso, y redobles de palmas al final de cada frase. Cada estilo suena a sí mismo a su tempo: **Tempo del proyecto a… BPM** lo pone.
- **Nivel**: **Básico** (lo esencial), **Medio** e **Intenso** (más golpes, percusión, notas fantasma).
- **Largo**: 1, 2, 4 u 8 compases. El ritmo se repite y el **cuarto compás** da la vuelta a la frase con una variación.
- **Que lo toque la IA**: el modelo de batería de Pulse oye la base del estilo y la toca a su manera, como un batería: la mantiene, la varía y mete redobles y cambios donde acaban las frases. Cada **Otra versión** es una interpretación distinta. Con **Básico** toca más a lo seguro; con **Intenso**, se suelta más. Toca con los **Sonidos** marcados: los toms entran en los redobles, y desde **Medio** también la pandereta y el shaker (en **Intenso**, además, congas y cencerro). Es una red pequeña entrenada con las baterías de unas 70.000 canciones (Lakh MIDI Dataset) y de baterías reales (Groove MIDI Dataset); va dentro de Pulse y funciona sin internet. Toca en segundo plano: mientras tanto el botón dice **Generando…** y puedes seguir trabajando; la base entra en su clip en cuanto está (si estás en otro clip, cuando vuelvas a él).
- **Redoble al final de cada frase**: en el último tiempo de cada 8 compases (o del clip, si tiene 4 o más), el de cada estilo (el reggaetón no lleva).
- **El bombo sigue al bajo**: añade bombos donde el bajo empieza sus notas (en corcheas), si hay una pista de bajo sonando con el clip.
- El hip hop lleva **swing** (y el house, un poco): se pone en el **Groove** del clip.
- **Sonidos**: qué instrumentos puede usar la base (bombo, caja, palmada, charles, ride, crash, rim, toms, shaker, pandereta, cencerro, congas, clave). Cada estilo trae los suyos marcados (el techno sin ride ni cencerro, el pop con crash, toms y pandereta) y puedes quitar o añadir los que quieras. Los que el kit no tiene salen tachados. Con el **crash** marcado y un clip de 4 compases o más, suena al empezar cada frase.
- **Sonido del estilo**: en una Drum Machine, pone la máquina del estilo (**808** para trap, hip hop y reggaetón; **909** para el resto) afinada como suena el estilo: el bombo largo y grave del trap, el seco del techno, la caja aguda del drum & bass… Si has afinado tú la máquina, solo cambia de máquina y respeta tus ajustes.
- Funciona con cualquier kit: Pulse busca cada sonido por su nombre (kick/bombo, snare/caja, clap, hat/charles, tom, rim…) y usa lo que haya.
- **Generar** sustituye las notas del clip; **Otra versión** pasa a otra base del mismo estilo y nivel. ⌘Z recupera las anteriores.

## Tonalidad y escala

El proyecto puede tener una escala: la tonalidad de la canción. Se elige en **Escala**, en la barra de transporte:

- **Detectar en la canción**: escucha las notas de los clips MIDI (sin las baterías) y los audios con una tonalidad clara, y elige la escala. La barra de estado dice cuál, y avisa si no está clara. Una canción en mi menor y otra en sol mayor usan las mismas notas: si sale la relativa y no es la que quieres, elígela a mano.
- O cualquiera de las 24 a mano (mayores y menores), o **Sin escala**.

Con una escala, en el **Editor MIDI**:

- Las filas de las notas de la escala se ven más claras, y las de la tónica en naranja, con su nombre.
- **Plegar**: solo quedan las filas de la escala, todas con su nombre (y las de las notas que no son de ella, para que no se pierda ninguna). Al arrastrar notas arriba o abajo, o con ↑ ↓ y **-1 / +1**, se mueven de nota en nota de la escala: no hay forma de salirse.
- **A la escala**: lleva las notas que no son de la escala a la más cercana (las seleccionadas o, si no hay, todas).

En **Audio a MIDI**, **Melodía** usa la escala del proyecto en vez de buscar una.

Las pistas de batería no usan la escala. Se guarda con el proyecto y se deshace con ⌘Z.

### Loops en tu tonalidad

Los audios que añades (arrastrándolos, desde el navegador o con Importar) se ponen en la escala del proyecto, igual que se ponen a su tempo:

- Pulse busca la **tonalidad de cada archivo**: primero en su nombre, como la escriben las librerías de loops («Pad_120_Am», «Keys F#m», «Bass_E», «124Gm»), y si no, en su sonido, cuando está clara. Si el nombre dice una y el sonido otra muy distinta, gana el sonido.
- El clip se **transpone por el camino más corto** (como mucho 6 semitonos) a la escala del proyecto. Si uno es mayor y el otro menor, va a la relativa: un loop en mi menor en una canción en sol mayor no se mueve.
- Si el proyecto aún **no tiene escala**, toma la del primer loop con tonalidad, y los siguientes se ajustan a ella.
- Si **cambias la escala** del proyecto, todos los loops que la siguen se mueven con ella.
- Las **baterías** (por su nombre: kick, snare, hat, perc, drum, tops, groove, shuffle, rumble, fill, conga…) y los audios sin notas que destaquen no se tocan, aunque su nombre diga una tonalidad: un loop de hats «Tops_150_Dm» se queda como está.

En el **Editor de audio** de cada clip, **Tonalidad** muestra la del archivo; **Seguir la escala** lo ajusta o lo deja en la suya; **Detectar** la busca de nuevo. Mover **Transp.** a mano hace que deje de seguir la escala. Para no ajustar los loops nuevos, desmarca **Ajustar los loops nuevos a la escala** en el menú **Escala** del transporte.

## Acordes

### Ver los acordes

En el **Editor MIDI**, bajo la regla, una franja dice el acorde que suena en cada tramo (como mucho uno cada medio compás): **Lam**, **Sol7**, **Do/Mi** (Do con Mi en el bajo)… Con una escala en el proyecto, también su grado: **Lam · vi**. Se actualiza mientras editas. Las notas sueltas de una melodía por encima no cambian el acorde.

Para sacar los acordes de un audio (una canción, un stem, un loop de piano), usa **Audio a MIDI → Acordes**.

### Escribir acordes

Con una **Escala** en el proyecto, el menú **Acordes** de la barra del Editor MIDI:

- Los **siete acordes de la escala**, con su grado (I, ii, iii, IV, V, vi, vii°). Clic: escribe el acorde en el marcador naranja y lo mueve al final, así que clic, clic, clic construye una progresión.
- **Sugerencias**: los acordes que suelen ir después del que hay antes del marcador salen en naranja («Después de Sol suelen ir: Do, Lam»).
- En menor, también el **V mayor** (menor armónica), la dominante que lleva con fuerza a la tónica.
- **Con séptima**: cuatriadas en vez de tríadas (Domaj7, Rem7, Sol7…).
- **Duración** de cada acorde: 2 tiempos, 1 compás o 2 compases.
- **Progresiones** listas: en mayor Pop (I–V–vi–IV), Balada, Años 50, Jazz (ii–V–I), Rock y Canon; en menor Épica (i–VI–III–VII), Andaluza (i–VII–VI–V), Menor, Dramática y Lamento.

Cada acorde lleva el bajo abajo y las demás notas juntas, en la inversión más cercana a la del acorde anterior: las voces se mueven lo menos posible, como lo tocaría un pianista. Las notas nuevas quedan seleccionadas, para moverlas o transponerlas.

## Acompañamiento

Pulse toca contigo: clic derecho en un clip con acordes (MIDI, o audio: un piano, una guitarra, una canción) → **Acompañar**, y elige una parte. Se escribe en una pista nueva justo debajo, en el mismo slot o en el mismo tramo del Arrangement, siguiendo los acordes del clip compás a compás.

- **Bajo: raíz**: la fundamental en cada tiempo, con alguna octava y notas de paso hacia el acorde siguiente.
- **Bajo: octavas**: fundamental y octava en corcheas (disco).
- **Bajo: contratiempo**: en los contratiempos (house).
- **Bajo: caminante**: una nota por tiempo recorriendo el acorde, y llega al siguiente por un semitono.
- **Arpegio ascendente** y **Arpegio arriba y abajo**: las notas del acorde una tras otra.
- **Pad**: los acordes sostenidos, con las voces enlazadas.
- **Acordes rítmicos**: acordes cortos con un ritmo sincopado.

Cada parte llega con un instrumento que le va (Acid para los bajos, FM para los arpegios, SuperSaw para el pad); cámbialo como cualquier otro. En el **Editor MIDI** del clip nuevo, **Otra variación** lo escribe de otra manera sobre los mismos acordes, tantas veces como quieras, hasta que lo edites. Los acordes salen como en la franja de acordes (ver «Acordes»); con una **Escala** en el proyecto, mejor.

## Editor de audio

Se abre al seleccionar un clip de audio.

- **Warp**: **Off** (a su velocidad, sin seguir el tempo), **Re-Pitch** (sigue el tempo cambiando velocidad y tono) **Stretch** (sigue el tempo manteniendo el tono) o **Beats** (para baterías: cada golpe empieza donde le toca y suena a su velocidad, sin el eco que deja Stretch al cambiar mucho el tempo; entre golpes, si el tempo es más lento, queda un silencio corto).
- **BPM original** con **÷2** y **×2**.
- **Longitud** en tiempos y **Ajustar al archivo**.
- **Ver**: **El clip** o **Archivo entero**. En «Archivo entero», clic o arrastrar en la forma de onda mueve el **inicio** del clip.
- **Inicio** en segundos (puede ser negativo, para empezar en un compás antes del archivo).
- **Gain** (−36 a +12 dB) y **Transp.** (±24 semitonos, solo en Stretch).
- **Tonalidad** del audio, **Seguir la escala** y **Detectar** (ver «Loops en tu tonalidad»).
- **Marcadores** de warp y **Quitar**. **Trocear → A Drum Pads**. **Tempo: Detectar / Usar en el proyecto**. **Separar en stems**.
- **Carril**: envolventes del clip (ver «Automatización»).

### Marcadores de warp

Para audios cuyo tempo varía. En la franja bajo la forma de onda (con Warp distinto de Off):

- **Doble clic**: añade un marcador (el primero añade también uno al inicio y otro al final). Doble clic en un marcador lo quita.
- **Arrastrar un marcador**: ese punto del audio cae en ese tiempo; los tramos de los lados se aceleran o frenan.

## Tempo de los audios

Al importar o soltar un audio, Pulse busca su tempo y sus compases (aproximadamente un segundo de análisis por minuto de audio). Si encuentra un ritmo claro, el clip pasa a **Stretch** a ese tempo (a **Beats** si es un loop de batería: por su nombre, o porque no tiene tonalidad), empieza en el compás del primer tiempo fuerte y dura compases enteros. Si el tempo varía, pone un marcador de warp en cada compás. Así el audio va al ritmo del metrónomo. Un loop corto (hasta 32 s) dura compases enteros: si el tempo encontrado se queda cerca del que lo hace de 1, 2, 3, 4, 6, 8, 12 o 16 compases justos, se usa ese (un loop de 4 compases a 150 queda a 150, no a 151); si el detector se equivoca más, pero el loop dura compases justos al tempo del proyecto, ese. Mientras se analiza, y si no encuentra un ritmo claro, un audio corto se toma por un loop de compases enteros: al tempo de su nombre si lo dice («Tops 150», «Bass_128bpm») y dura compases justos a él, o de 1, 2, 4 u 8 compases al tempo más cercano al del proyecto.

- **Detectar tempo** (clic derecho en un clip de audio, o «Detectar» en el editor): vuelve a analizarlo.
- **Usar el tempo de este clip en el proyecto**: pone el proyecto al tempo del clip.
- Si no encuentra un ritmo claro, el clip suena a su velocidad.

## Trocear y separar stems

### Trocear a un Drum Pads

En el editor de audio, **Trocear** (Transitorios, 1/16, 1/8 o 1/4) y **A Drum Pads**: corta el clip en golpes (hasta 16), cada uno en un pad de un Drum Pads en una pista nueva, con un clip MIDI que los toca en orden en el mismo sitio. Al cambiar el tempo el groove lo sigue sin estirar el audio, y puedes reordenar o repetir golpes en el editor MIDI. El Drum Pads también tiene **Trocear archivo…**.

- **Editar cortes** (casilla bajo «Trocear»): muestra los cortes numerados sobre la forma de onda, con una franja debajo. **Arrastra** un corte para moverlo, **doble clic** en uno para quitarlo o en un hueco para añadir otro (hasta 16, uno por pad). **A Drum Pads** trocea entonces por tus cortes. Cambiar el modo (Transitorios, 1/16…) vuelve a buscar los cortes.

### Separar en stems

Clic derecho en un clip de audio → **Separar en stems…** (o el botón del editor). Crea cuatro pistas nuevas debajo, **Batería**, **Bajo**, **Resto** y **Voz**, con el mismo clip en el mismo sitio, y desactiva la original para que suene igual: cada pista nueva es una copia de la original, con sus efectos, su volumen, panorama, envíos, automatización y grupo, y el clip conserva sus envolventes.

- Se hace en este Mac, sin enviar nada a internet, y puedes seguir trabajando mientras tanto. **Cancelar** la detiene.
- La primera vez descarga el modelo (174 MB) de GitHub, sin cuenta. Si la conexión se corta o cancelas, la próxima vez sigue desde donde se quedó.
- Una separación a la vez, y hacen falta 4 pistas libres (máximo 64).

## Audio a MIDI

Saca las notas de cualquier audio y las pone en una pista MIDI nueva, justo debajo y en el mismo sitio (el mismo slot en Session, el mismo tramo en el Arrangement). Clic derecho en un clip de audio → **Convertir a MIDI**:

- **Melodía**: una nota cada vez. Para una voz, un tarareo o un instrumento solista: canta una idea, conviértela y tócala con cualquier sinte.
- **Melodía sin ajustar**: igual, pero cada nota al semitono más cercano en vez de a la escala de la canción. Para melodías con notas fuera de la tonalidad, o un instrumento bien afinado.
- **Armonía**: todas las notas a la vez. Para acordes, piano o guitarra.
- **Acordes**: los acordes de una canción o de un instrumento, uno por cambio (como mucho cada medio compás), con el bajo y las voces enlazadas, listos para tocarlos con otro sonido. Ignora la batería y los golpes cortos. Usa la **Escala** del proyecto si la hay; si no, la del audio.
- **Batería**: cada golpe pasa a bombo, caja o charles (según dónde tiene su energía), en una Drum Machine.

Se hace en este Mac, sin internet, en unos segundos; la barra de estado muestra el progreso. Las notas siguen el tempo del clip (su warp) y su fuerza pasa a la velocidad. Se combina bien con **Separar en stems**: separa una canción y convierte el bajo o la voz en MIDI.

La melodía sigue el tono de la voz fotograma a fotograma (pYIN, el método de las herramientas para transcribir canto), también con voces muy graves, y la corta en notas donde se queda en otra nota o vuelves a atacar. En **Melodía**, cada nota va a la de la escala de la canción más cercana, para que cantar un poco alto o bajo no cambie la nota; la tonalidad es la **Escala** del proyecto si la has elegido; si no, sale de tu voz, de las notas MIDI del proyecto y de los audios con una tonalidad clara (las baterías no cuentan). La barra de estado dice cuál ha usado. Las notas que cantas afinadas se quedan aunque no sean de la escala. La armonía la encuentra Basic Pitch, un modelo de Spotify (licencia Apache 2.0) que va dentro de la app.

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
- **Compressor**: umbral, ratio, ataque, release y ganancia, y **Sidechain** (ver abajo).
- **Limiter**: ganancia, techo y release.
- **Saturator**: drive y salida, con cinco **tipos** (Soft, el de siempre; Hard, recorte duro; Fold, plegado brillante; Tube, asimétrico y cálido; Crush, reducción de bits) y **4×**, que trabaja a cuatro veces la frecuencia de muestreo para que mucho drive no ensucie los agudos (aliasing; gasta más CPU y retrasa 36 muestras).
- **Bitcrusher**: bits y reducción de muestreo.
- **Auto Filter**: paso bajo, alto o banda, con LFO sincronizado al tempo. **24 dB** corta el doble por octava. **Drive** satura lo que entra al filtro. Con **Res** por encima del 95 %, el filtro suena solo (auto-oscila) a su frecuencia: el silbido de los barridos ácidos.
- **Ducker**: bombeo de volumen. Sin sidechain, sincronizado al tempo (**Ritmo**); con **Sidechain**, baja cada vez que suena la pista elegida (**Release**: cuánto tarda en volver, en fracción del tiempo elegido).
- **Chorus** y **Phaser**.
- **Delay**: tiempo sincronizado (1/16 a un compás), Ping Pong, feedback y filtro.
- **Reverb**: tamaño, amortiguación y anchura.
- **Utility**: ganancia y anchura estéreo (0 % mono, 200 % muy abierto).

### Efectos MIDI

Cambian las notas antes de que lleguen al instrumento (en pistas MIDI). Se añaden desde el navegador (**EFECTOS MIDI**) con la pista seleccionada y aparecen en Dispositivos, antes del instrumento; hasta 4 por pista, en orden (el punto los activa, **◀ ▶** los mueven, **×** los quita).

- **Arpegiador**: toca las notas que mantienes una a una, en tiempo. **Orden**: arriba, abajo, arriba y abajo, como se tocan o al azar. **Ritmo**: de 1/4 a 1/32, con tresillos. **Octavas** (1–4) y **Duración** de cada nota. Con el transporte en marcha va con la rejilla; parado, empieza al tocar y sigue el tempo. Funciona con el teclado y con los clips.
- **Acorde**: cada nota se convierte en un acorde. **En la escala**: le añade su tercera y su quinta (y **séptima**) de la escala, así que los acordes encajan con la canción (en do mayor, un re da re–fa–la, un sol da sol–si–re). La escala puede ser la del proyecto o una propia. Sin «En la escala»: los **semitonos añadidos**, con atajos (Mayor, Menor, Quinta, Octava, Sus4, Séptima). **Velocidad añadidas**: lo fuerte de las notas nuevas.
- **Escala**: lleva las notas fuera de la escala a la más cercana: con ella no hay nota que desafine. La del proyecto o una propia.

Se combinan: **Acorde + Arpegiador** arpegia acordes tocando una sola nota. Al grabar se graban las notas que tocas (antes de los efectos).

### Sidechain

El **Compressor** y el **Ducker** pueden escuchar a otra pista en vez de a la suya: elige cuál en **Sidechain**. El caso típico: el bajo o el pad se apartan cada vez que suena el bombo, para que no se tapen (y ese «bombeo» del house).

- Escucha el sonido de la pista elegida **tras sus efectos y antes de su fader**: funciona aunque esa pista esté silenciada o con el fader abajo (puedes tener un bombo solo para disparar el sidechain).
- La fuente se recuerda aunque muevas, agrupes o renombres las pistas.
- Vale también en pistas de grupos, retornos y el Master.

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
- **Drum Machine**: 16 sonidos sintetizados (bombo, rimshot, caja, palmas, hats cerrado y abierto, tres toms, cencerro, crash, ride, shaker, pandereta, conga y clave), cada uno con Tune, Decay y Level. Clic en un sonido para escucharlo. **Máquina**: **808** (bombo largo y redondo, palma, cencerro: hip hop, trap, reggaetón), **909** (bombo con pegada y charles brillantes: house, techno) o **Clásica**, sus sonidos primeros. En los proyectos de cuando tenía 8 sonidos, los 8 nuevos entran con Level a 0, para que suenen como sonaban: súbelos si quieres oírlos.
- **Drum Pads**: 16 pads de muestras (el 1 abajo a la izquierda, desde C1).
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
- Las carpetas del sistema: en macOS, `/Library/Audio/Plug-Ins/VST3` y `CLAP` (y las de tu usuario en `~/Library`); en Windows, `C:\Program Files\Common Files\VST3` y `CLAP` (y las de tu usuario en `%LOCALAPPDATA%\Programs\Common`). Pulse para Windows es de 64 bits: los plugins de 32 bits no se cargan.

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
- Un CC por control: asignar uno que ya se usa lo mueve. Con **Un knob para varios controles** marcado, lo añade a lo que ya mueve: un solo knob para abrir el filtro, subir la reverb y bajar el volumen a la vez.
- La lista muestra las asignaciones; **×** quita una. **Terminar** sale.
- **Desde** y **Hasta**: la parte del recorrido del control que cubre el knob (en %). Por ejemplo, un filtro de 20 % a 70 % para que no se cierre del todo ni se abra entero. Con **Desde** por encima de **Hasta**, el knob lo mueve al revés.
- **Suavizado**: lo que tarda el control en llegar adonde pones el knob (hasta 2 s). Quita los escalones de un knob de 0–127 y hace barridos lentos con un gesto rápido.
- Un CC asignado ya no llega al instrumento.
- Las asignaciones de parámetros se guardan con el proyecto; las de transporte, para todos los proyectos.

### Macros

**Transporte → Macros…** abre ocho knobs del proyecto. Cada uno mueve a la vez los controles que le asignes, cada control con su recorrido: un solo gesto para abrir el filtro, subir la reverb y meter saturación en el break.

- Pulsa **controles** bajo una macro y toca los knobs, faders o parámetros de plugin que quieras que mueva (se marcan con un borde; el clic los elige sin cambiarlos). Pulsa otra vez para terminar.
- En la lista, **Desde** y **Hasta** dicen dónde pone cada control la macro al mínimo y al máximo (en % de su recorrido). Con Desde mayor que Hasta va al revés: la macro abre un filtro mientras cierra otro. **×** quita uno. **Nombre**: el que verás en el knob.
- Una macro se puede asignar a un knob MIDI como cualquier control (Asignar, toca la macro y mueve el knob).
- Al grabar automatización, lo que se graba es el movimiento de cada control que mueve la macro.
- Las macros se guardan con el proyecto.

## Teclado del ordenador

Pulsa **M** (o el botón **Teclado**) para tocar con el teclado del ordenador:

- **A S D F G H J K L** (y ;): teclas blancas desde C. **W E T Y U O P**: negras.
- **Z / X**: octava abajo / arriba. **C / V**: velocidad abajo / arriba.
- El botón muestra la nota base y la velocidad («Teclado C3 · 100»).
- No toca mientras escribes en un campo de texto.

## Tocar con la voz

Tu voz como teclado MIDI: pulsa **Voz** en la barra de transporte y canta, tararea o silba al micrófono. Suena la pista seleccionada (o las armadas), nota a nota, mientras cantas.

- Una nota se mantiene aunque la voz oscile un poco a su alrededor, y cambia cuando te asientas en otra (unos 50 ms). Si paras un instante y vuelves a cantar, es una nota nueva («la-la»). Lo fuerte que cantes es la velocidad.
- Con una **Escala** en el proyecto, las notas que cantes desafinadas van a la nota de la escala más cercana; las que cantes afinadas se quedan, aunque no sean de la escala.
- **Clic derecho** en **Voz**: la **octava**. −1 o −2 para tocar un bajo con una voz aguda; +1 o +2 para un lead con una voz grave.
- **Graba** como un teclado MIDI: arma una pista MIDI y graba (F9 o en un slot) mientras cantas.
- El botón muestra la nota que suena («Voz · A2»).
- Usa **auriculares**: si el micrófono oye los altavoces, toca también lo que suena (sobre todo el bajo).
- La primera vez, macOS pide permiso para usar el micrófono.

Para pasar a MIDI algo ya grabado, usa **Audio a MIDI → Melodía**, que lo analiza con más calma y saca notas más limpias.

## Navegador y biblioteca

El panel de la izquierda (☰ lo muestra u oculta). Arriba pone el **destino**: la pista seleccionada.

- **INSTRUMENTOS**, **EFECTOS DE AUDIO** y **PLUGINS**: clic para usarlos (ver sus temas).
- **MUESTRAS**: **Añadir muestra…**.
- **BIBLIOTECA**: busca muestras, proyectos y presets por nombre, y explora las carpetas Samples, Proyectos, Exportaciones, Presets y las tuyas (**Añadir carpeta…**; clic derecho → **Quitar del navegador**).
  - Audio: **clic** lo escucha (otro clic lo para), **doble clic** lo añade a la pista de audio seleccionada, y se puede **arrastrar** a un slot, una pista, un pad o el Sampler.
  - **A tempo** (arriba, marcado de serie): los loops (hasta 32 s) se escuchan al tempo del proyecto y en bucle, como sonarían en un clip (Beats si son de batería). Si la canción está sonando, entran a tiempo con ella, en su compás. La primera vez que escuchas un archivo tarda un momento en sonar, mientras busca su tempo; al lado de la casilla pone a qué tempo va el loop. Sin marcar, o con archivos más largos, cada archivo suena una vez a su velocidad.
  - **↑ ↓**: tras hacer clic en un archivo, pasan al anterior o al siguiente y lo escuchan. Un clic fuera del navegador devuelve las flechas al resto de Pulse.
  - Proyectos: doble clic para abrir. Presets: doble clic para aplicar.
- **PISTAS**: nueva pista MIDI o de audio.

### Arrastrar desde Finder

- Un audio suelto en un slot o una pista de audio va ahí; en un pad o el Sampler, se carga; en un hueco vacío, crea una pista de audio. Varios archivos van seguidos; en Session, si no caben en los 16 slots, siguen en una pista nueva. Una **carpeta** trae los audios que tiene (también en sus carpetas), por nombre. La barra de estado dice cuántos se añadieron y cuáles no se pudieron leer.
- Un `.pulse` se abre y un `.als` se importa (pregunta antes si hay cambios sin guardar).
- También funciona abrir con doble clic en Finder o soltar en el icono del Dock.

## Proyectos y carpeta de Pulse

Todo vive en la **carpeta de Pulse**, por defecto Música/Pulse (`~/Music/Pulse` en macOS, la carpeta Música de tu usuario en Windows; se cambia en Ajustes):

- `Proyectos/<nombre>/`: cada proyecto en su carpeta, con sus audios en `Samples/` (Importados, Grabaciones, Stems, Trozos, Consolidados, Congelados) y sus versiones en `Versiones/`. La carpeta entera se puede copiar a otro ordenador, también de macOS a Windows y al revés. Si al abrir un proyecto falta algún audio (estaba fuera de su carpeta o en otro ordenador), Pulse lo dice en la barra de estado y conserva su ruta: pon el archivo en su sitio o vuelve a importarlo.
- `Exportaciones`, `Presets`, `Wavetables` y `Samples` (las tuyas).
- `Sin guardar`: lo que graba o crea un proyecto que aún no has guardado. Al guardarlo se mueve a su carpeta.

### Archivo

- **⌘N** nuevo, **⌘O** abrir, **Abrir recientes** (los 10 últimos), **⌘S** guardar, **⇧⌘S** guardar como, **Volver a la versión guardada**, **⌥⌘S** versiones (ver «Versiones del proyecto»).
- Al guardar, todos los audios que usa el proyecto se copian a su carpeta.
- Antes de perder cambios sin guardar, Pulse pregunta (Guardar / No guardar / Cancelar).
- **⌘Z / ⇧⌘Z**: deshacer y rehacer. Se puede hacer mientras suena: solo cambia lo que deshaces; las demás pistas siguen como estaban, con sus colas de reverb y delay y sus plugins (lo mismo al añadir una pista).
- **Mostrar la carpeta de Pulse** la abre en Finder (en el Explorador en Windows).
- Los ajustes y lo que Pulse genera no son tu trabajo y no están en la carpeta de Pulse: en macOS en `~/Library/Application Support/Pulse`; en Windows, los ajustes en `%APPDATA%\Pulse` y kits y modelos en `%LOCALAPPDATA%\Pulse`. Si Pulse se cierra por un fallo, deja lo que pasó en `pulse.log` (`~/Library/Logs/Pulse` en macOS, `%LOCALAPPDATA%\Pulse\Logs` en Windows), con la versión, el sistema y el commit. Si se cierra de golpe sin dejarlo (un plugin que falla, por ejemplo), en macOS hay un informe en Consola → Informes de fallos (`~/Library/Logs/DiagnosticReports/pulse-*.ips`).
- El título de la ventana lleva * si hay cambios sin guardar. También cuentan los que haces dentro de un plugin (en su editor, con un controlador o en **Parámetros…**).
- Guardar es todo o nada: Pulse escribe primero una copia completa y solo entonces sustituye el archivo, así que un cierre, un disco lleno o un corte de luz a mitad de guardar dejan el archivo anterior entero. Si el proyecto tuviera algún número no válido (NaN o infinito, por ejemplo de un set de Live dañado), se guarda como 0 y la barra de estado dice cuántos. Si un archivo de ajustes o de listas está dañado, Pulse lo aparta como «… (dañado)» en vez de sobrescribirlo.

### Autoguardado y recuperación

- Cada 30 segundos, si hay cambios sin guardar, Pulse guarda una copia aparte en la carpeta `Recuperación` de sus ajustes (`~/Library/Application Support/Pulse/Recuperación` en macOS, `%LOCALAPPDATA%\Pulse\Recuperación` en Windows). Tu archivo `.pulse` no se toca hasta que guardas.
- Si Pulse se cierra sin guardar ni preguntar (un fallo, el ordenador se apaga), al abrirlo otra vez pregunta si quieres **Recuperar** los cambios. Vuelve el proyecto tal como estaba en el último autoguardado, marcado como sin guardar: guárdalo con ⌘S para conservarlo. **Descartar** lo aparta como `descartado.pulse` en la misma carpeta, por si acaso, hasta la próxima vez.
- Mientras grabas audio, lo que entra se va escribiendo a disco en `Samples/Grabaciones` como «Grabación sin terminar…». Al parar se convierte en la toma de siempre y esa copia se borra; si Pulse se cierra a mitad, queda ahí con lo grabado hasta ese momento, y la pregunta de recuperar lo menciona.
- Si Pulse se cerró mientras reabría el último proyecto al arrancar, la vez siguiente no lo reabre solo (puede ser ese proyecto lo que lo cierra). Ábrelo desde **Abrir recientes** cuando quieras.
- Si el sonido de una pista deja de ser válido (NaN o infinito, por un archivo dañado o un plugin que falla), Pulse la silencia en ese momento, vacía sus efectos y lo dice en la barra de estado; el resto sigue sonando.

## Versiones del proyecto

Pulse guarda el historial de tu canción: puedes probar ideas sin miedo y volver a cualquier punto. **Archivo → Versiones…** (**⌥⌘S**) abre la ventana de versiones.

- **Guardar versión**: escribe un nombre («Con el bajo nuevo») y, si quieres, una nota, y guarda cómo está el proyecto ahora. Las versiones con nombre se quedan para siempre.
- **Automáticas**: cada vez que guardas (⌘S) se hace también una versión, si algo cambió desde la anterior. Se guardan las 20 últimas.
- Cada versión dice **cuándo** se hizo y **qué cambió** respecto a la anterior: «Tempo 120 → 124», «+ Pista «Voz»», ««Bass»: 1 → 2 clips en Session, mezcla»…
- **Comparar con ahora**: lo que ha cambiado desde esa versión hasta el proyecto tal como está.
- **Volver a esta**: abre esa versión en lugar de lo que hay. Lo que había se guarda antes como versión («Antes de volver a…»), así que no se pierde nada, y **⌘Z** lo deshace. Guarda para quedarte con ella.
- **Renombrar** (una automática con nombre pasa a quedarse para siempre) y **Borrar** (pide confirmación).

Las versiones viven en la carpeta del proyecto (`Versiones/`) y usan sus mismos audios. Hace falta que el proyecto esté guardado en su carpeta: si es un `.pulse` suelto de antes, usa **Guardar como…**.

## Importar de Ableton Live

**⌘I** (Archivo → Importar set de Live) o arrastra un `.als` a la ventana. Sustituye el proyecto actual; el `.als` no se toca.

- Se importan: tempo, loop, escenas, pistas MIDI y de audio (nombre, color, volumen, panorama, envíos, activador, solo) y los clips de Session y del Arrangement, con sus notas o sus muestras y su warp.
- Los Drum Racks con muestras pasan al Drum Pads de Pulse; Simpler y Sampler, al Sampler.
- Los **grupos** llegan como grupos de Pulse, con sus pistas dentro y su mezclador.
- Los **locators** llegan como marcadores.
- No se importan los sintes de Live (esas pistas reciben Pulse Synth), los efectos ni la automatización.
- Las muestras se buscan por ruta y, si el set viene de otro ordenador, por nombre en la carpeta del set.

## Exportar audio

**⇧⌘R** (Archivo → Exportar audio…):

- **Qué**: **Mezcla completa** o **Una pista por archivo (stems)**: cada pista que suena en solo, con sus envíos y su grupo. No se exportan las pistas desactivadas (ni las de un grupo desactivado) ni los grupos como stem aparte (sus pistas ya pasan por él). **Con la cadena del master** (en stems): quítala para que los stems sumados den la mezcla. Los stems se renderizan a la vez, uno por núcleo del procesador, y cada uno solo con lo que puede sonar en él (su pista, sus grupos y las pistas que escuchan sus sidechains): 16 stems tardan poco más que la canción una vez. Suenan exactamente igual que en solo. Si el proyecto tiene plugins, se renderizan de uno en uno.
- **Rango**: **Toda la canción**, **Región del loop** o **Por secciones**: un archivo por cada parte de la canción, de un marcador al siguiente (el último, hasta el final), con el número y el nombre del marcador: «Tema - 1 Intro.wav», «Tema - 2 Drop.wav»… Lo que haya antes del primer marcador sale como «Inicio». Cada sección empieza con lo que ya sonaba y acaba con su cola. Con stems, una tanda por sección: «Tema - 2 Drop - 1 Drums.wav».
- **Versiones** (si el proyecto tiene versiones con nombre): marca las que quieras exportar también, cada una tal como era (su tempo, sus pistas, su mezcla) y con su nombre al final del archivo: «Tema - Con el bajo nuevo.wav». **Solo las versiones** deja fuera lo de ahora.
- **Formato**:
  - **WAV**: 16 bits (con dither), 24 o 32 bits (float).
  - **FLAC**: sin pérdida, como el WAV, y más o menos la mitad de grande; 16 o 24 bits.
  - **MP3**: para escuchar y compartir, a 192, 256 o 320 kbps.
  - A **44.1** o **48 kHz**, en cualquiera de los tres.
- El diálogo recuerda lo que elegiste la última vez (formato, rango, stems, normalización…), también entre sesiones.
- **Normalizar**: pico a un valor (no en stems).
- Se exporta el **Arrangement** (no la Session), con la cola de reverbs y delays (también los ecos de un delay largo: espera lo que tarda el delay más lento del proyecto antes de dar la cola por terminada). Empieza a todo volumen desde la primera muestra. Al exportar la **Región del loop** se toca desde unos compases antes, así que lo que ya sonaba (notas largas, colas) está desde el principio. Lo exportado suena igual que lo que oyes, con cualquier tamaño de búfer: la automatización se aplica cada 64 muestras, en los mismos puntos. Avisa si la mezcla satura.
- Por defecto se guarda en la carpeta `Exportaciones`.
- Antes de exportar, mira el **Asistente de mezcla y master**.

## Asistente de mezcla y master

**Archivo → Asistente de mezcla y master…** te dice cómo suena tu mezcla y qué arreglar antes de publicarla, con un botón **Aplicar** en cada consejo.

- **Analizar la mezcla**: toca la canción (o el loop) sin sonido, más rápido que en tiempo real, la mezcla entera y cada pista sola (a la vez, una por núcleo), y las mide:
  - **Sonoridad** (LUFS integrados, la medida que usan Spotify, YouTube o Apple Music para igualar el volumen de las canciones).
  - **Pico real** (dBTP: también entre muestras, donde nace la distorsión al pasar a MP3 o AAC).
  - **Rango** (LU: cuánto cambia de volumen entre partes), **lo más fuerte** (3 segundos) y **estéreo** (correlación: 1 mono, por debajo de 0 se pierde en mono).
- **Destino**: Spotify / YouTube / Tidal (−14 LUFS), Apple Music (−16), club / DJ / SoundCloud (−9), radio y TV (−23), o **como la referencia**. Todos con el pico real por debajo de −1 dBTP.
- **Consejos**:
  - **Sonoridad** por debajo o por encima del destino: **Aplicar** pone un **Limiter** en el master y lo ajusta midiendo cada vez, hasta llegar (y con el pico real por debajo de −1 dBTP). Si la mezcla no da más sin aplastarla, se para y te lo dice: para más volumen, comprime o satura las pistas.
  - **Pistas por encima de 0 dB**: **Aplicar** les baja el volumen.
  - **Graves que chocan**: dos pistas llenando los graves (por debajo de 120 Hz) a la vez. Si una es la batería, **Aplicar** pone a la otra un **Ducker** con **sidechain** de la batería (se aparta cada vez que suena); si no, recorta los graves de la que menos tiene con un **EQ Three**.
  - **Fase**: mezcla o pistas en contrafase, que en mono (un móvil, un club) se pierden.
  - **Dinámica**: muy apretada o con partes muy lejos en volumen.
- **Referencia**: **Elegir canción…** carga una canción que suene como quieres. Se compara su sonoridad y su **balance de graves, medios y agudos** con el tuyo (en la gráfica por octavas: barras, tu mezcla; línea, la referencia), y **Aplicar** pone un **EQ Three** en el master que lo acerca.
- **Pistas**: el pico, la sonoridad y el estéreo de cada una.

Tras **Aplicar** se vuelve a analizar sola, para que veas el resultado. Todo lo que hace son dispositivos normales (Limiter, Ducker, EQ Three) y volúmenes: puedes retocarlos o quitarlos, y se deshace con ⌘Z.

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
- No hace falta cuenta de GitHub: el repositorio es público. **Acceso a GitHub (opcional)** solo sirve si GitHub dice que hay demasiadas consultas (tiene un límite por hora): usa la sesión de `gh auth login` o un token pegado ahí (se guarda en el llavero en macOS y en el Administrador de credenciales en Windows). Si el token caduca, Pulse sigue sin él.
- Solo se anuncian las versiones que traen paquete para tu sistema.
- Si Pulse no puede reemplazarse donde está, lo dice antes de descargar nada: abierto sin moverlo de Descargas (macOS lo ejecuta desde una copia temporal: muévelo a Aplicaciones) o en una carpeta en la que tu usuario no puede escribir.
- Si algo falla al cambiar de versión, todo queda como estaba, también la versión anterior guardada.
- Si vuelves a una versión anterior y abres un proyecto guardado con una más nueva, la barra de estado lo dice, y antes de guardarlo encima Pulse pregunta: esa versión no conoce lo nuevo y se perdería. **Guardar como…** guarda una copia sin tocar el original. Los ajustes que no conoce se conservan.
- En Windows se sustituye `pulse.exe` y el anterior queda como «pulse (anterior).exe» en la carpeta de instalación. Antes de instalarla, Pulse comprueba que la descarga es la publicada (su huella SHA-256) y que es la versión anunciada.

## Acerca de Pulse

Pulse → **Acerca de Pulse** muestra la versión y la licencia. Pulse es software libre (GPL-3.0 o posterior): puedes usarlo, estudiarlo, modificarlo y compartirlo. Si distribuyes una versión modificada, también con su código y con la misma licencia. Su código está en GitHub (el enlace de la ventana).

- **Licencia (GPL-3.0)** abre el texto completo de la licencia.
- **Componentes de terceros** abre las licencias de lo que Pulse incluye: las librerías, ONNX Runtime, los modelos de IA y las fuentes.

## Atajos de teclado

| Atajo | Qué hace |
|---|---|
| ⌘N | Proyecto nuevo |
| ⌘O | Abrir |
| ⌘S / ⇧⌘S | Guardar / Guardar como |
| ⌥⌘S | Versiones del proyecto |
| ⌘I | Importar set de Live |
| ⇧⌘R | Exportar audio |
| ⌘Q | Salir |
| ⌘, | Ajustes (audio, MIDI, plugins) |
| ⌘Z / ⇧⌘Z | Deshacer / Rehacer |
| ⌘X / ⌘C / ⌘V | Cortar / Copiar / Pegar los clips seleccionados (o las notas seleccionadas en el editor) |
| ⌘D | Duplicar |
| ⌘E / ⌘J | Dividir el clip en el cursor / unir clips (Arrangement) |
| 0 | Desactivar o activar los clips seleccionados |
| ⌫ | Eliminar |
| ⌘A | Seleccionar todas las notas |
| ⌘T / ⇧⌘T | Nueva pista MIDI / de audio |
| ⌘G | Agrupar las pistas seleccionadas |
| ⇧⌘G | Desagrupar el grupo seleccionado |
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
