<!--
Guía de Pulse. Se ve dentro de la app en Ayuda → Guía de Pulse; cada función
nueva añade o corrige su tema aquí. Formato: ver el comentario de
crates/app/src/guide.rs (# área, ## tema con entradilla, ### apartado, 1. pasos,
> Consejo:/Nota:/Atención:, +++ plegado +++, {teclas}, [[enlaces]]).
-->

# Empezar

## Primeros pasos

Pulse es un estudio para hacer música: compones, grabas, editas y mezclas en una sola ventana. Este tema te enseña dónde está cada cosa y cómo hacer sonar algo en un minuto.

### La ventana de Pulse

Pulse tiene dos vistas, y {Tab} cambia de una a otra:

- **Session**: clips en bucle que lanzas cuando quieres, para improvisar y probar ideas. Ver [[Session]].
- **Arrangement**: la línea de tiempo de la canción, de principio a fin. Ver [[Arrangement]].

Alrededor de la vista están siempre las mismas piezas:

- Arriba, el **transporte**: tempo, metrónomo, reproducir, parar, grabar, loop y más. Ver [[Transporte]].
- A la izquierda, el **navegador**: instrumentos, efectos, plugins, muestras y tu biblioteca. **☰** lo muestra u oculta. Ver [[Navegador y biblioteca]].
- Abajo, el **panel de detalle**: el editor del clip seleccionado o los dispositivos de la pista. {⇧Tab} cambia entre los dos.
- Abajo del todo, la **barra de estado**: cuenta lo que acaba de pasar y por qué algo no se ha podido hacer. Si no hay mensaje, recuerda los atajos principales.

### Hacer sonar algo en un minuto

1. Crea una pista MIDI con {⌘T}.
2. Elige un instrumento en el navegador: clic en **INSTRUMENTOS**.
3. Haz doble clic en un slot vacío de la pista. Se crea un clip MIDI de cuatro compases y se abre en el editor.
4. Haz doble clic en el editor para poner notas, o pulsa {M} y toca con el teclado del ordenador.
5. Pulsa el **►** del clip para lanzarlo, y {Espacio} para parar.

> Consejo: la primera vez, Pulse abre una canción de demostración (Drums, Bass, Chords, Lead) para que tengas algo que escuchar y desmontar. Después abre el último proyecto; puedes cambiarlo en [[Ajustes]].

> Consejo: en los menús, para llegar a una opción de un submenú (**⏵**) puedes ir en diagonal: mientras el puntero va hacia el submenú abierto, pasar por encima de otras opciones no lo cambia. Si te paras un momento sobre otra opción con **⏵**, se abre la suya.

> Nota: en Windows, los menús están en la barra de arriba de la ventana de Pulse, donde la guía dice Finder es el Explorador de archivos, y la pantalla completa es {F11}. Las teclas de la guía ya se muestran como en tu sistema (⌘ es Ctrl, ⇧ es Mayús, ⌥ es Alt y ⌫ es Supr).

## Transporte

El transporte es la barra de arriba: desde ahí reproduces, paras, grabas y controlas el tempo de toda la canción. Si la ventana es estrecha, pasa a dos filas.

### Reproducir, parar y grabar

1. Pulsa **► Reproducir** o {Espacio} para que suene.
2. Pulsa **■ Parar** o {Espacio} otra vez. Parar detiene todo, incluida la grabación, y vuelve al marcador de inicio.
3. Para grabar en el Arrangement, pulsa **● Grabar** o {F9}. Ver [[Grabar]].

### Los controles, de izquierda a derecha

| Control | Para qué sirve |
|---|---|
| **Tempo** | La velocidad de la canción, de 20 a 999 BPM. Arrastra o escribe. |
| **Compás** | 4/4, 3/4, 6/8, 7/8… El de la canción, o el del principio si cambia más adelante. Ver [[#Cambiar el compás]]. |
| **●** junto al tempo | El **metrónomo**. |
| **Cuantización de lanzamiento** | Cuándo arrancan los clips que lanzas: Ninguna, 1/16 … 1 Compás, 2 Compases. Por defecto, 1 Compás. |
| **Posición** | Por dónde va la canción (compás. tiempo. semicorchea), contada en su compás. Durante la cuenta atrás muestra «Cuenta N». |
| **► Reproducir** / **■ Parar** | {Espacio}. |
| **● Grabar** | {F9}: graba en el Arrangement. |
| **Cuenta atrás** | Sin cuenta, 1, 2 o 4 compases de metrónomo antes de grabar. |
| **A** (Armar automatización) | Encendido, al grabar se graban también los controles que muevas. Ver [[Automatización]]. |
| **⟲ Loop** | Repite una parte del Arrangement. Al lado, su inicio y su longitud; la región se elige en la franja de arriba del Arrangement. |
| **Punch in** / **punch out** | Los dos botones rojos junto al loop: al grabar en el Arrangement, solo se graba desde el inicio del loop y/o hasta su final. Ver [[Grabar]]. |
| **Rec Q** | Cuantiza el inicio de las notas MIDI que grabas: Off, 1/4, 1/8, 1/8T, 1/16, 1/16T, 1/32. |
| **Swing** | De 0 a 100 %, con su rejilla (1/8 o 1/16): retrasa los contratiempos para que el ritmo «balancee». |
| **Escala** | La tonalidad de la canción, y si los loops nuevos se ajustan a ella. Ver [[Tonalidad y escala]]. |
| **► Arrangement** | Se ilumina cuando hace falta: devuelve al Arrangement las pistas que tocan clips de Session y reactiva la automatización que hayas movido a mano. |
| **Teclado** | El teclado del ordenador como teclado MIDI ({M}). Ver [[Teclado del ordenador]]. |
| **Voz** | Tocar cantando, tarareando o silbando al micrófono, o una batería haciendo beatbox. Ver [[Tocar con la voz]]. |
| **MIDI** | Encendido si hay teclados MIDI conectados. Clic para volver a buscarlos. |
| **Asignar** | Asignar knobs y botones de un controlador MIDI. Ver [[Controladores MIDI]]. |
| **☰**, **Session** / **Arrangement** | A la derecha: el navegador, la vista y el [[#El medidor del master]]. |

### El medidor del master

A la derecha del todo, el medidor del master te dice cuánto suena la canción mientras la tocas:

- La **barra** es su nivel. La rayita que se queda es el **pico más alto**, y a su lado tienes su valor en dB.
- Si la mezcla pasa de **0 dBFS**, la barra acaba en rojo y el pico se queda en **rojo** aunque ya no suene tan fuerte: la mezcla satura. Baja los faders o pon un limitador en el master (el [[Asistente de mezcla y master]] lo hace por ti).
- Haz **clic** en el medidor o en el pico para limpiarlo y volver a vigilar.
- Al lado ves la **sonoridad** a corto plazo, en LUFS: la de los últimos 3 segundos, medida como la mide el asistente. Compárala con el destino de tu canción (−14 LUFS para Spotify, −9 para un club); el drop suele sonar 1 o 2 LU por encima de la media de la canción.

+++ Detalles
- El pico es el de lo que sale del master, antes del recorte de seguridad de la salida: si marca +2,7, la mezcla de verdad pasa 2,7 dB de 0.
- La sonoridad no cuenta el metrónomo ni lo que escuchas en el navegador.
+++

### Cambiar el compás

1. Abre el menú **Compás**, junto al tempo, y elige uno: 4/4, 3/4, 2/4, 6/8, 12/8, 5/4, 7/8, 9/8, 5/8, 7/4 o 2/2. En **Otro**, cualquier otro: los tiempos y la figura.
2. Para que cambie a mitad de canción, haz clic derecho en la franja de marcadores del Arrangement, sobre el compás donde cambia, y elige **Compás desde el N**. El cambio se ve en la regla, junto al número del compás.
3. Para quitar un cambio, haz clic derecho sobre ese compás y elige **Quitar el cambio de compás**.

Todo sigue el compás:

- la regla y la rejilla;
- el ajuste al mover clips y el loop;
- **Trocear** por compases;
- la cuantización de lanzamiento («1 Compás» espera al compás que suena);
- el metrónomo y la cuenta atrás;
- el redondeo de las tomas;
- los clips nuevos, que miden cuatro compases (o los que elijas en [[Ajustes]]);
- la posición de la barra de transporte;
- los plugins, que reciben el compás.

En el editor MIDI, la regla y el loop cuentan los compases donde está el clip; los clips de Session, los del compás del principio.

Los generadores escriben también en el compás:

- **Generar ritmo** hace sus compases del compás del clip. El primer pulso lleva el bombo y el segundo, la caja: el 2 en 3/4, el segundo pulso en 6/8. Los charles van en corcheas y el redoble, en el último pulso.
- El **acompañamiento** toca los bajos por pulsos (negras con puntillo en 6/8, grupos de 2+2+3 en 7/8) y acentúa el primero de cada compás.
- **De tarareo a canción** pone un acorde por compás.
- El **arreglo automático** cuenta sus secciones en compases del proyecto, y el redoble dura el último compás.
- Las **transiciones** miden sus compases.

> Nota: la IA de batería solo sabe tocar en 4/4. En otros compases, **Generar ritmo** usa el ritmo escrito y lo avisa.

+++ Cómo cuenta
- Los tiempos son negras: un compás de 3/4 dura 3, uno de 6/8 también 3 (seis corcheas) y uno de 7/8, 3,5.
- El metrónomo marca cada **pulso**: la negra en 4/4 y 3/4, la negra con puntillo en los compases compuestos (6/8, 9/8, 12/8) y la corchea en los irregulares (5/8, 7/8). El primero de cada compás suena más agudo.
- Un cambio va siempre al principio de un compás. Si cambias un compás anterior, los cambios de después siguen en el mismo número de compás.
- Una canción puede tener hasta 16 cambios.
+++

### Tempo que cambia a lo largo de la canción

Si el tempo está automatizado, el campo de tempo muestra el que suena en cada momento, con un borde rojo. Si lo mueves a mano, la automatización deja de mandar hasta que pulses **► Arrangement**. Ver [[Automatización]].

### Swing

El swing retrasa los contratiempos al reproducir, sin cambiar las notas que has escrito. Al 100 % es un shuffle de tresillo. También se aplica al exportar y vale para todos los clips MIDI, salvo los que tengan su propio groove. Ver [[Editor MIDI]].

> Nota: cuando hay una versión nueva de Pulse, aparece «↑ Pulse X disponible» a la derecha de la barra de estado. Ver [[Actualizaciones]].

## Session

La vista Session es una cuadrícula de clips que suenan en bucle y lanzas cuando quieres: sirve para improvisar, probar combinaciones y tocar en directo.

Cada pista es una columna de slots, y cada fila es una **escena**. Se ven las escenas que usas y una vacía más (8 como mínimo, hasta 16); al poner un clip en la última, aparece otra. A la derecha están el botón **+** (nueva pista MIDI), los retornos (**A Reverb** y **B Delay** en un proyecto nuevo) con su propio **+** (nuevo retorno) y la columna **Master** con las escenas.

### Lanzar y parar clips

1. Pulsa el **►** de un clip. Arranca en el siguiente punto de cuantización (y arranca el transporte si estaba parado). Mientras espera, el ► parpadea, y una barra abajo muestra por dónde va.
2. Para parar una pista, pulsa el cuadrado de un slot vacío o **Parar pista**, bajo su columna.
3. Para parar todo, pulsa **Parar todos los clips**, bajo el Master.

### Crear, seleccionar y mover clips

- **Doble clic en un slot vacío**: en una pista MIDI crea un clip vacío de cuatro compases, con el nombre de la pista; en una de audio abre el selector de archivos. Cuánto miden los clips nuevos se elige en [[Ajustes#Proyectos]], y el de cada uno, en **Loop** del [[Editor MIDI]].
- **Clic en un clip**: lo selecciona y lo abre en el editor. {⌘ clic} o {⇧ clic} añade o quita clips de la selección.
- **Arrastrar un clip**: mueve los clips seleccionados a otros slots; con {⌥} los copia. Los slots de destino se marcan en azul si caben y en rojo si no. Cada clip necesita un slot libre en una pista de su tipo (audio o MIDI).
- **Arrastrar desde un slot vacío**: dibuja un recuadro que selecciona los clips y pistas que toque.

### Menú del clip

Clic derecho en un clip:

- **Lanzar**.
- **Copiar al Arrangement**: lo pone al final del Arrangement de su pista, en un compás.
- **Acompañar**: un bajo, un arpegio, un pad, acordes rítmicos o una melodía para sus acordes. Ver [[Acompañamiento]].
- **Duplicar** ({⌘D}): lo copia al siguiente slot libre de abajo.
- **Eliminar** ({Supr} o {⌫}).
- **Desactivar clip**: se queda en su sitio pero no suena. Ver [[Arrangement]].
- Colores.
- Solo en clips de audio: **Separar en stems…**, **Consolidar**, **Invertir**, **Detectar tempo** y **Usar el tempo de este clip en el proyecto**. Ver [[Trocear y separar stems]] y [[Tempo de los audios]].
- Solo en clips MIDI: **Exportar MIDI…**. Ver [[Archivos MIDI]].

### Lanzar escenas

- Clic en una escena de la columna Master la lanza: cada pista toca su clip de esa fila, y las que no tienen clip ahí se paran.
- Clic derecho en la escena: **Nombre de la escena**.

### Grabar en un slot

1. Arma la pista con su botón **●**. Sus slots vacíos muestran un punto rojo.
2. Haz clic en el punto rojo de un slot. La toma empieza en el siguiente punto de cuantización, después de la cuenta atrás si la hay y el transporte estaba parado. El slot se pone rojo con «Grabando · compás N».
3. Vuelve a hacer clic para terminar. La toma se redondea a compases enteros y sigue sonando en bucle.

> Nota: solo se graba en slots vacíos. Para grabar en la línea de tiempo, ver [[Grabar]].

## Arrangement

El Arrangement es la línea de tiempo de la canción: cada pista es una fila, y los clips se colocan de izquierda a derecha en el orden en que suenan.

Arriba está la **franja del loop**, debajo la de **marcadores** y después la **regla** con los números de compás. En la barra de herramientas tienes el **Zoom** y **Seguir cursor**, que desplaza la vista con el cursor al reproducir.

> Consejo: el zoom también decide la rejilla. Con zoom alto los clips se ajustan a tiempos; con zoom bajo, a compases.

### Moverte por la canción

- **Clic en la regla**: mueve el cursor y el marcador de inicio.
- **Arrastrar en la regla**: recorre la canción.
- **Clic en un marcador**: el cursor salta a él.

### Repetir una parte con el loop

1. Arrastra en la franja de arriba para marcar la región. El loop se activa solo.
2. Pulsa **⟲** en el transporte para activarlo o desactivarlo.

Al dar la vuelta, las notas de los clips terminan en el final del loop; las que mantienes en el teclado siguen sonando.

### Marcar las partes de la canción

Los **marcadores** dan nombre a las partes de la canción (intro, estrofa, estribillo…). Están en la franja bajo la del loop.

- **Doble clic** en la franja: un marcador nuevo ahí.
- **Arrastrar** un marcador: lo mueve por la rejilla.
- **Clic derecho** en un marcador: renombrarlo, **Ir aquí**, eliminarlo o poner una **Transición aquí ▸**. Ver [[Transiciones]].
- **Clic derecho** en la franja vacía: **Añadir marcador aquí** y **Transición aquí ▸** en el compás más cercano.

> Nota: el [[Arreglo automático]] pone un marcador en cada parte, y al [[Importar de Ableton Live]] llegan sus locators.

### Poner y mover clips

- **Doble clic en un hueco**: en una pista MIDI crea un clip vacío de cuatro compases (los de [[Ajustes#Proyectos]]), que acaba antes si hay otro clip más adelante; en una de audio abre el selector de archivos.
- **Clic**: selecciona el clip y lo abre en el editor. {⌘ clic} o {⇧ clic}: varios.
- **Arrastrar en un hueco**: recuadro de selección de clips y pistas ({⌘} o {⇧} añaden).
- **Arrastrar un clip**: mueve los seleccionados, también a otras pistas del mismo tipo. {⌥} + arrastrar los copia.
- **Borde derecho** (el cursor cambia a ↔): cambia la longitud. Si el clip es más largo que su contenido, este se repite en bucle; unas líneas marcan cada vuelta.
- **Soltar archivos de audio**: en una pista de audio, se colocan en ese tiempo; en un hueco vacío, crean una pista nueva.

Para copiar, pegar, dividir y unir, ver [[Seleccionar, copiar y pegar clips]] y [[Dividir, unir y consolidar]].

### Fundidos de entrada y salida

Un fundido hace que un clip de audio entre desde el silencio o se vaya a él, sin cortes bruscos.

1. Selecciona un clip de audio. Aparecen dos cuadraditos blancos bajo su título, en las esquinas.
2. Arrastra el de la izquierda hacia dentro para el fundido de entrada, y el de la derecha para el de salida. La curva del volumen se dibuja sobre la onda, y lo que quita se ve sombreado.
3. Para quitar un fundido, haz doble clic en su cuadradito.

Van libres; con {⇧}, por la rejilla.

+++ Cómo funciona
- Se miden en tiempos, así que siguen al tempo.
- Usan una curva de igual potencia, sin bajón de volumen a mitad.
- Juntos nunca ocupan más que el clip.
- Un proyecto con fundidos pide actualizar en versiones anteriores de Pulse, que lo harían sonar sin ellos.
+++

### Menú del clip

Clic derecho en un clip:

- **Tomas** (en grabaciones en bucle): elegir la toma que suena y **Mostrar las tomas en filas…**. Ver [[Tomas y comping]].
- **Acompañar**: un bajo, un arpegio, un pad, acordes rítmicos o una melodía para sus acordes. Ver [[Acompañamiento]].
- **Dividir aquí** ({⌘E} corta donde hiciste clic), **Trocear ▸** (en trozos iguales) y **Unir N clips** ({⌘J}).
- **Desactivar clip** ({0}): ver abajo.
- **Copiar**, **Cortar**, **Duplicar** ({⌘D}) y **Eliminar** ({⌫}).
- Con un solo clip: **Nombre** y **Color**.
- Solo en clips de audio: **Separar en stems…**, **Detectar tempo**, **Usar el tempo de este clip en el proyecto**, **Consolidar** e **Invertir**.
- Solo en clips MIDI: **Exportar MIDI…**. Ver [[Archivos MIDI]].

### Desactivar un clip sin borrarlo

**Desactivar clip** ({0}) deja el clip donde está, en gris, pero sin sonar: ni sus notas, ni su audio, ni su automatización, tampoco al exportar. Repítelo para activarlo.

- Con varios seleccionados, los desactiva todos (o los activa, si ya lo estaban todos).
- También está en el menú de los clips de Session.

+++ Detalles
Si desactivas un clip mientras suena (o mueves, recortas o borras un clip de audio que suena), se apaga con un fundido corto, sin clic.
+++

### Menú de un hueco

Clic derecho en un hueco de una pista:

- **Pegar aquí**, si hay algo copiado.
- **Añadir muestra aquí…** (pistas de audio) o **Nuevo clip MIDI aquí** (pistas MIDI).
- **Transición aquí ▸**: una subida, una bajada, un impacto o un platillo invertido en el compás más cercano. Ver [[Transiciones]].

### Cabecera de pista

Los botones de la cabecera, de izquierda a derecha:

- **Número**: el activador; apagado, la pista no suena.
- **S**: solo.
- **●**: armar para grabar.
- **In / Auto / Off**: monitorización (solo audio); cada clic pasa al siguiente modo. Ver [[Grabar]].
- **A**: muestra el carril de automatización. Ver [[Automatización]].

Clic derecho en la cabecera: **Nombre**, **Color**, **Mostrar/Ocultar automatización**, **Mostrar/Ocultar tomas**, **Duplicar**, **Eliminar** y **Congelar**. Ver [[Pistas]].

> Nota: las pistas que están tocando clips de Session se ven oscurecidas en el Arrangement. **► Arrangement**, en el transporte, las devuelve.

## Pistas

Las pistas son los carriles de la canción: cada una tiene su instrumento o su audio, sus efectos y su fader. Aquí aprendes a crearlas, ordenarlas, agruparlas y congelarlas.

### Crear y ordenar pistas

- {⌘T}: pista MIDI nueva, con Pulse Synth.
- {⇧⌘T}: pista de audio nueva.
- **Seleccionar**: clic en la cabecera. {⌘ clic} añade o quita; {⇧ clic} selecciona un rango.
- **Mover**: arrastra la cabecera. Se mueven todas las seleccionadas (no mientras se graba).
- **Nombre y color**: clic derecho en la cabecera.
- **Duplicar pista** / **Eliminar pista**: clic derecho en la cabecera o menú **Crear**. Con varias seleccionadas, actúan sobre todas. Las copias se llaman «<nombre> copia».

> Nota: puede haber hasta 64 pistas, y siempre queda al menos una. {⌘Z} deshace.

### Agrupar pistas

Un grupo junta varias pistas en una: suenan por **su fader y sus efectos**, como un bus, y se pliegan para ganar sitio.

1. Selecciona las pistas.
2. Pulsa {⌘G} (o menú **Crear**, o clic derecho en la cabecera › **Agrupar**). Pasan a un grupo nuevo donde estaba la primera.
3. Clic en **▾** en la cabecera del grupo para plegarlo; **▸** lo despliega.

- **Desagrupar** ({⇧⌘G}): quita el grupo; sus pistas se quedan donde están.
- **Sacar del grupo** (clic derecho en una pista del grupo): la saca justo debajo.
- **Meter una pista**: suéltala justo debajo de la cabecera del grupo.
- **Arrastrar** el grupo lo mueve con todas sus pistas. **Duplicar** o **eliminar** un grupo duplica o elimina también sus pistas.
- Se pueden agrupar grupos (grupos dentro de grupos).

+++ Cómo funciona un grupo
- Las pistas de un grupo van justo debajo de él, con una barra del color del grupo en su cabecera (y sangradas en el Arrangement). Plegado, en Session se ocultan sus columnas y en el Arrangement sus filas.
- El grupo tiene **mezclador** (volumen, panorama, envíos, activador, solo), **cadena de efectos** (en Dispositivos) y **automatización**, como cualquier pista.
- Un **solo** en el grupo hace sonar todas sus pistas; un solo en una de sus pistas mantiene el grupo sonando.
- **Desactivar** el grupo silencia todo lo suyo, también lo que sus pistas mandan a los retornos.
- **Session**: los slots del grupo se encienden donde sus pistas tienen clips; clic lanza los de esa escena (y para las demás pistas del grupo). Su ■ para todas sus pistas.
- **Arrangement**: la fila del grupo muestra en pequeño los clips de sus pistas. En ella no se ponen clips.
- Un grupo no tiene instrumento ni clips: no se arma ni recibe notas.
+++

### Añadir un rumble al bombo

El **rumble** es el retumbar grave que va pegado al bombo en el techno. Pulse lo crea a partir de tu bombo.

1. Clic derecho en la cabecera de la pista del bombo.
2. Elige **Crear rumble**. Aparece una pista nueva debajo.
3. Si suena demasiado fuerte para tu bombo, baja su fader. Ajusta a tu gusto la reverb, el drive o el filtro.

El rumble **sigue al bombo**: no tiene clips propios, así que si cambias, quitas o lanzas otros bombos, los sigue, y si silencias la pista del bombo se silencia con ella. Cuando el bombo para (en un break), suena aún un compás más o menos y se apaga.

+++ Cómo funciona
- La pista toca lo que suena en la del bombo a través de una reverb larga solo húmeda, una saturación Tube con 4× y un filtro de 24 dB a 180 Hz, con un Ducker que se aparta en cada golpe.
- La saturación iguala su volumen: queda unos 5 dB por debajo de un bombo de hard techno. Con un bombo más flojo, baja su fader.
- No se puede congelar, porque no tiene clips propios.
- Un proyecto con un rumble se abre solo con esta versión de Pulse o una más nueva.
+++

### Sacar un sonido de la batería a su pista

Con una **Drum Machine**, puedes llevar uno de sus sonidos (por ejemplo el bombo) a una pista propia, para darle su EQ, su saturación y su fader, o para que un Ducker escuche solo el bombo.

1. Clic derecho en la cabecera de la pista de la batería.
2. Elige **Sacar a su pista ▸** y el sonido. Aparece una pista nueva debajo que lo toca.
3. Repite con otros sonidos si quieres; cada uno va a su pista.

En la Drum Machine, un sonido sacado lleva una flecha (**Bombo →**); al pasar por encima dice en qué pista suena. Si borras la pista nueva (o deshaces), el sonido vuelve a la batería.

+++ Cómo funciona
- La pista nueva no tiene clips: toca ese sonido tal como sale de la Drum Machine, antes de los efectos de la batería, y la batería deja de tocarlo. Entra con el fader y el panorama de la batería, así que suena igual de fuerte que antes. El patrón sigue siendo un solo clip en la pista de la batería.
- Si silencias la batería, se silencian también sus sonidos sacados.
- Al exportar por pistas, cada sonido sale en la stem de su pista.
- Mientras tenga sonidos sacados, la batería no se puede congelar.
- Un sonido suena en una sola pista: la pista de un sonido no se duplica sola (sonaría dos veces). Duplícala con la batería, por ejemplo agrupándolas y duplicando el grupo: la copia toca sus sonidos de la batería copiada, y los sidechains de dentro escuchan también a las copias.
- Un proyecto con sonidos sacados se abre solo con esta versión de Pulse o una más nueva.
+++

### Congelar una pista

**Congelar** convierte a audio lo que suenan el instrumento y los efectos de una pista, y la pista pasa a sonar ese audio: deja de gastar CPU. El volumen, el panorama y los envíos siguen en vivo.

1. Clic derecho en la cabecera › **Congelar pista**.
2. La pista lleva un marco azul y ✱. Su instrumento y sus efectos no se pueden editar.
3. Para volver atrás, **Descongelar pista** (o el botón **Descongelar** del panel de abajo).

- **Convertir en audio**: la pista pasa a ser una pista de audio con lo congelado, para siempre.

> Atención: si cambias el contenido de una pista congelada, se descongela sola. También si cambias el tempo, el swing o la tonalidad del proyecto, o lo que escucha su sidechain.

> Nota: solo se congela una pista a la vez. Una pista de audio sin efectos no necesita congelarse.

+++ Cómo funciona
- También se descongela si cambia lo que escucha su sidechain: si esa pista escucha a otra, o si es un grupo y cambias sus pistas.
- Suena como antes también con sidechain: lo que lo dispara se tiene en cuenta al congelar (un grupo con todas sus pistas, y la pista que dispara a la que lo dispara).
- Con cambios de tempo, el audio congelado sigue la automatización de tempo tal como sonaba.
- Las envolventes de volumen, panorama y envíos de los clips de Session siguen en vivo; las de los clips del Arrangement quedan grabadas en el audio.
- El primer golpe suena entero (sin el fundido de entrada de los clips) y, al exportar, la cola congelada (reverb, delay) se exporta completa.
+++

# Grabar y tocar

## Grabar

Graba audio desde un micrófono o una interfaz, notas desde un teclado MIDI y los movimientos de los controles. Puedes grabar en el Arrangement, en la línea de tiempo, o en un slot de la Session.

### Elegir qué pistas graban

Graban las pistas **armadas**, las que tienen encendido el botón ● de su cabecera.

- Si pulsas grabar sin ninguna pista armada, se arman las seleccionadas.
- Las notas que tocas en vivo van a las pistas armadas. Si no hay ninguna, van a la seleccionada.

### Grabar en el Arrangement

1. Arma la pista con ●.
2. Pon el cursor donde quieras empezar.
3. Pulsa {F9} o el **●** del transporte. Pulse empieza a reproducir y a grabar desde el cursor, después de la cuenta atrás si la has elegido.
4. Vuelve a pulsar {F9} o **●** para dejar de grabar sin parar la música, o {Espacio} para pararlo todo.

Lo que queda grabado:

- **MIDI**: un clip «Toma» que empieza donde empezaste y se redondea a compases enteros. Se le aplica **Rec Q**, la cuantización de grabación del transporte. El pitch bend, la rueda, el aftertouch y los demás controles que muevas quedan en el clip, bajo las notas. Ver [[Expresión: pitch bend, rueda y controles en el clip]].
- **Audio**: un clip por cada pista de audio armada.
- **Automatización**: con **A** (Armar automatización) encendido en el transporte, también se graban los controles que muevas. Ver [[Automatización]].

> Nota: lo nuevo se pone encima. Los clips que ya había no se borran ni se recortan.

### Grabar en un slot de la Session

En una pista armada, los slots vacíos muestran un punto rojo.

1. Haz clic en el punto. La toma empieza en el siguiente punto de cuantización, después de la cuenta atrás si la hay y el transporte estaba parado.
2. El slot se pone rojo con «Grabando · compás N».
3. Vuelve a hacer clic para terminar. La toma se redondea a compases enteros y sigue sonando en bucle, con el bend y los controles que tocaste (ver [[Expresión: pitch bend, rueda y controles en el clip]]).

Solo se graba en slots vacíos. Más sobre slots y clips en [[Session]].

### Pasar una improvisación de la Session al Arrangement

Mientras grabas en el Arrangement con {F9}, los clips de Session que suenan quedan escritos en el Arrangement, en su pista y en el momento exacto en que sonaron. Así puedes improvisar una canción lanzando clips y escenas y que quede montada en la línea de tiempo.

- Da igual que lances los clips uno a uno, con escenas o que ya estuvieran sonando antes de grabar. En ese caso se escriben desde donde empezó la grabación.
- Si solo hay clips de Session sonando, grabar no arma ninguna pista.
- Lo que hubiera en el Arrangement de esa pista en ese tramo se sustituye.
- Con el loop activado, cada clip se escribe una sola vez. El punch in/out también se aplica.

> Consejo: al terminar, las pistas siguen tocando la Session. Pulsa **► Arrangement** en el transporte para escuchar lo que has grabado.

### Punch in / out

Sirve para regrabar solo un trozo sin tocar lo de alrededor.

1. Marca el trozo con la región del loop, en la franja de arriba del Arrangement.
2. Enciende **Punch in**, **Punch out** o los dos: son los dos botones rojos junto al loop en el transporte.
3. Pon el cursor un poco antes del trozo y graba. Oyes lo anterior para entrar a tempo, pero la toma empieza justo en el inicio del loop.

- **Punch in**: lo que suene antes del inicio del loop no se graba.
- **Punch out**: la grabación se para sola al llegar al final del loop y la reproducción sigue.
- Funcionan con el loop activado o no. Con el loop activado y punch in, la toma empieza en el inicio del loop (lo de antes no queda como clip aparte) y cada vuelta es una toma: ver [[Tomas y comping]].
- En el Arrangement, unas líneas rojas marcan dónde empieza y dónde acaba la grabación.
- Valen para audio, MIDI y automatización, y se guardan con el proyecto.

### Cuenta atrás

Junto al botón de grabar elige **Sin cuenta**, **Cuenta 1 c.**, **Cuenta 2 c.** o **Cuenta 4 c.**. Antes de empezar suena el metrónomo esos compases, con el primer tiempo acentuado, y la posición muestra «Cuenta N».

- Vale para el Arrangement y para los slots de Session cuando el transporte está parado.
- Parar la cancela.

### Grabar audio

- La entrada se elige en **Ajustes › Entrada (grabar audio)**. Se abre al armar una pista de audio.
- Las grabaciones son archivos WAV en `Samples/Grabaciones`, dentro de la carpeta del proyecto. Si aún no lo has guardado, van a `Sin guardar/Grabaciones` y se mueven al guardar.

> Atención: la primera vez, macOS pide permiso para usar el micrófono. En Windows, si no se graba nada, mira **Configuración › Privacidad y seguridad › Micrófono** y activa «Permitir que las aplicaciones de escritorio accedan al micrófono».

### Oír lo que entra: monitorización

La **monitorización** decide si oyes la entrada (tu voz, tu guitarra) a través de Pulse. Solo existe en pistas de audio: en Session son tres botones encima del mezclador; en el Arrangement, un botón que va cambiando.

| Modo | Qué oyes |
|---|---|
| **Auto** (por defecto) | La entrada mientras la pista está armada. |
| **In** | Siempre la entrada, en vez de los clips. |
| **Off** | Nunca la entrada: escúchala directamente del micrófono o de la interfaz. |

Con la pista armada, su medidor muestra el nivel de entrada.

> Consejo: usa auriculares, para que el micrófono no recoja lo que suena por los altavoces.

### Que las tomas caigan a tempo: latencia

La **latencia** es el pequeño retraso que pone el sistema al recibir y al sacar el sonido. Pulse la descuenta para que lo que grabas quede donde lo tocaste.

En **Ajustes** se ven la latencia de entrada y la de salida, y está el **Ajuste fino de la grabación** (de −200 a +200 ms):

- Si las tomas de audio quedan tarde, súbelo.
- Si quedan pronto, bájalo.

### Compensación de latencia

Algunos efectos sacan el sonido un poco tarde. Sin compensarlo, su pista sonaría detrás de las demás. Pulse hace que todo suene a la vez.

- La canción sale tan tarde como su camino más lento. **Ajustes › Audio** muestra ese retraso como **efectos**, junto a las latencias.
- Para tocar o cantar en directo a través de uno de esos efectos sin que el resto se retrase, quita **Compensar la latencia de los efectos** en **Ajustes › Audio**. La exportación compensa siempre.

+++ Cómo funciona
- Efectos que retrasan: el **Limiter** (1,5 ms), el **Saturator** con 4× (36 muestras), el **Tuner** y muchos plugins (Pulse les pregunta cuánto).
- Cada pista espera a la más lenta: dentro de su grupo, a las del grupo; fuera, a todas las del proyecto.
- Los envíos llegan juntos a las pistas de retorno, y lo que sale de ellas llega a tiempo con el resto.
- El metrónomo se retrasa con la canción.
- Las tomas que grabas y las notas que tocas se colocan donde las oíste.
- Al exportar y al congelar, el archivo empieza justo en su sitio, sin el retraso.
- El Tuner se compensa con su latencia para una voz media (unos 5 ms). Como mueve el tono leyendo un periodo antes o después, una voz grave o un bajo quedan unos pocos milisegundos más tarde.
+++

## Tomas y comping

Graba varias veces el mismo trozo seguidas y quédate con lo mejor de cada una. A eso se le llama **comping**: montar una toma perfecta a partir de varias.

### Grabar varias tomas

1. Activa el loop en el Arrangement sobre el trozo que quieres grabar.
2. Arma una pista de audio y graba con {F9}.
3. Deja que dé varias vueltas: cada vuelta es una **toma**.

Al parar:

- Sobre el loop queda un clip con todas las tomas («Toma 1…N»). Suena la última.
- Lo que grabaste antes del inicio del loop queda en un clip aparte.
- Una vuelta a medias cuenta si dura al menos medio tiempo. Lo que no llegaste a grabar queda en silencio.

> Nota: en MIDI no hay tomas. Las notas de todas las vueltas se juntan en un mismo clip (es lo que se llama overdub). Con los controles, una vuelta solo sustituye lo de las vueltas anteriores en el tramo donde moviste ese control.

### Elegir una toma entera

Clic derecho en el clip › **Tomas** › la que quieras. La que suena lleva ✓.

### Montar lo mejor de cada toma

Al terminar la grabación aparece **una fila por toma** debajo de la pista, con su forma de onda. Lo que está sonando se ve iluminado.

1. **Arrastra** sobre un trozo de una fila: esa toma suena en ese trozo. El clip se corta ahí y el trozo lee esa toma desde el mismo punto.
2. Repite con los trozos que quieras de otras tomas.
3. Para usar una toma en todo el clip, haz **clic** en su fila.

- Los trozos contiguos que acaban sonando la misma toma se juntan solos.
- Muestra u oculta las filas con **Mostrar tomas** (clic derecho en la cabecera de la pista) o **Mostrar las tomas en filas…** (clic derecho en el clip).
- {⌘Z} deshace.

+++ Detalles
El paso de una toma a otra lleva un fundido cruzado corto, así que no se oye el corte.
+++

## Controladores MIDI

Toca con un teclado MIDI y controla Pulse con los knobs, faders y botones de tu controlador. Con las macros, un solo knob mueve varios controles a la vez.

### Tocar con un teclado MIDI

Conéctalo y toca: Pulse escucha **todos** los teclados MIDI conectados, en todos los canales.

- El botón **MIDI** del transporte se enciende si hay alguno. Haz clic en él para volver a buscarlos.
- Las notas van a las pistas armadas o, si no hay ninguna, a la seleccionada.
- **Pitch bend**: ±2 semitonos en los instrumentos de Pulse (el Modular tiene su propio rango).
- **Rueda de modulación** (CC 1): vibrato.
- **Brillo** (CC 74) y **aftertouch**: abren el filtro de Pulse Synth, Super Saw, Acid Bass y Wavetable.
- **Expresión** (CC 11): el volumen del instrumento, como un pedal de expresión.
- Al grabar, todo esto queda en el clip. Ver [[Expresión: pitch bend, rueda y controles en el clip]].
- **Pedal de sustain** (CC 64): las teclas que sueltas con el pedal pisado siguen sonando hasta que lo levantas, en cualquier instrumento. Al grabar, cada nota queda tan larga como sonó, así que se reproduce igual sin pedal. Volver a tocar una tecla sostenida termina la nota anterior.
- El Kick Designer y las baterías no usan bend ni rueda. Los plugins lo reciben todo.

### Asignar un knob a un control (MIDI learn)

1. Pulsa **Asignar** en el transporte (o **Transporte › Asignar controles MIDI**). Se abre la ventana «Asignar MIDI» y los controles asignables se marcan con un borde.
2. Haz clic en un control de Pulse: un knob, un fader, un parámetro de plugin o un botón de transporte. El clic lo elige sin cambiarlo.
3. Mueve un knob o pulsa un botón del controlador.
4. Repite con otros controles y pulsa **Terminar**.

> Consejo: el orden da igual. Si mueves primero el knob, tienes 10 segundos para tocar el control.

- Se pueden asignar **Reproducir**, **Parar**, **Grabar**, **Loop**, **Metrónomo** y **Armar automatización**.
- Cada CC mueve un solo control: asignar uno que ya se usa lo mueve al nuevo control. Con **Un knob para varios controles** marcado, lo añade a lo que ya mueve: un solo knob para abrir el filtro, subir la reverb y bajar el volumen a la vez.
- La lista muestra las asignaciones. **×** quita una.
- **Desde** y **Hasta**: la parte del recorrido del control que cubre el knob, en %. Por ejemplo, un filtro de 20 % a 70 % para que no se cierre del todo ni se abra entero. Con **Desde** por encima de **Hasta**, el knob lo mueve al revés.
- **Suavizado**: lo que tarda el control en llegar adonde pones el knob, hasta 2 s. Quita los escalones de un knob de 0–127 y hace barridos lentos con un gesto rápido.
- Un CC asignado ya no llega al instrumento.
- Las asignaciones de parámetros se guardan con el proyecto; las de transporte valen para todos los proyectos.

+++ Detalles
Una asignación sigue a su control: si mueves, duplicas, agrupas o borras pistas, o mueves efectos en la cadena, el knob sigue moviendo lo mismo. Si borras la pista o el efecto, su asignación se va con él. Una que ya no apunta a nada (de un proyecto antiguo) sale como «(ya no existe)» y no hace nada; quítala con **×**.
+++

### Mover varios controles con una macro

**Transporte › Macros…** abre ocho knobs del proyecto. Cada uno mueve a la vez los controles que le asignes, cada control con su recorrido: un solo gesto para abrir el filtro, subir la reverb y meter saturación en el break.

1. Pulsa **controles** bajo una macro.
2. Haz clic en los knobs, faders o parámetros de plugin que quieras que mueva. Se marcan con un borde, y el clic los elige sin cambiarlos.
3. Pulsa otra vez **controles** para terminar.

- En la lista, **Desde** y **Hasta** dicen dónde pone cada control la macro al mínimo y al máximo, en % de su recorrido. Con Desde mayor que Hasta va al revés: la macro abre un filtro mientras cierra otro.
- **×** quita un control. **Nombre** es el que verás en el knob.
- Una macro se puede asignar a un knob MIDI como cualquier control: **Asignar**, clic en la macro y mueve el knob.
- Al grabar automatización se graba el movimiento de cada control que mueve la macro.
- Lo que mueve una macro la sigue al mover pistas o efectos, y se va si los borras.
- Las macros se guardan con el proyecto.

## Teclado del ordenador

Toca notas con el teclado del ordenador cuando no tienes un teclado MIDI a mano. Pulsa {M} o el botón **Teclado** del transporte para encenderlo o apagarlo.

| Teclas | Qué hacen |
|---|---|
| {A} {S} {D} {F} {G} {H} {J} {K} {L} {;} | Teclas blancas, desde C |
| {W} {E} {T} {Y} {U} {O} {P} | Teclas negras |
| {Z} / {X} | Octava abajo / arriba |
| {C} / {V} | Velocidad abajo / arriba |

- El botón muestra la nota base y la velocidad, por ejemplo «Teclado C3 · 100».
- Las notas suenan en las pistas armadas o, si no hay ninguna, en la seleccionada, y se graban como las de un teclado MIDI.

> Nota: mientras escribes en un campo de texto, el teclado no toca.

## Tocar con la voz

Usa tu voz como teclado MIDI: canta, tararea o silba al micrófono y suena el instrumento de la pista, nota a nota, mientras cantas. También puedes hacer beatbox y tocar una batería.

### Tocar notas cantando

1. Selecciona una pista con instrumento, o arma las que quieras que suenen.
2. Pulsa **Voz** en el transporte.
3. Canta, tararea o silba. El botón muestra la nota que suena, por ejemplo «Voz · A2».

> Consejo: usa **auriculares**. Si el micrófono oye los altavoces, toca también lo que suena, sobre todo el bajo.

- Una nota se mantiene aunque la voz oscile un poco a su alrededor, y cambia cuando te asientas en otra. Si paras un instante y vuelves a cantar, es una nota nueva («la-la»).
- Lo fuerte que cantes es la **velocidad** de la nota.
- Con una **Escala** en el proyecto, las notas que cantes desafinadas van a la nota de la escala más cercana. Las que cantes afinadas se quedan, aunque no sean de la escala. Ver [[Tonalidad y escala]].
- La primera vez, macOS pide permiso para usar el micrófono.

### Grabar lo que cantas

Graba como con un teclado MIDI: arma una pista MIDI y graba ({F9} o en un slot) mientras cantas. Con **Seguir el tono**, el bend se graba también en el clip.

### Opciones (clic derecho en Voz)

- **Octava**: −1 o −2 para tocar un bajo con una voz aguda; +1 o +2 para un lead con una voz grave.
- **Acordes**: **Tríadas** o **Con séptima**. Cada nota que cantas suena con su acorde: es la raíz, y encima van la tercera y la quinta (y la séptima) de la **Escala** del proyecto. En Do mayor, un Re da Re menor y un Sol con séptima da Sol7. Sin escala, o con una nota que no es de ella, el acorde es mayor (con séptima mayor). Los acordes se graban nota a nota, como si los tocaras. Para acordes graves, baja la octava.
- **Seguir el tono**: el vibrato de la voz y los deslizamientos hacia otra nota doblan la nota que suena (pitch bend, hasta ±2 semitonos). Si cantas un poco desafinado, se oye desafinado como tu voz; si la **Escala** ha corregido la nota, suena afinada y se mueve desde ahí. Un deslizamiento lento pasa por las notas de en medio. Se oye en vivo y, al grabar, el bend queda en el clip con las notas.

+++ Cómo funciona
- Una nota empieza a sonar unos 25–35 ms después de que empieces a cantarla. Las más graves tardan un poco más: la voz tiene que dar un par de vueltas.
- La voz se sigue en su propio hilo, también con Pulse detrás de otra ventana.
- Para cambiar de nota hay que asentarse en otra unos 30 ms; un salto de octava, unos 60.
+++

### Tocar una batería con beatbox

**Clic derecho** en **Voz** › **Batería (beatbox)**. En vez de cantar notas, haz beatbox y toca la Drum Machine de la pista seleccionada o de las armadas.

| Sonido | Instrumento | Nota |
|---|---|---|
| «pum» | Bombo | C1 |
| «pf» o «ka» | Caja | D1 |
| «ts» | Charles cerrado | F#1 |

- Lo fuerte que lo hagas es la velocidad. El botón muestra el último sonido, por ejemplo «Voz · Caja».
- **Graba** como con un teclado: arma la pista y graba ({F9} o en un slot) mientras haces beatbox. Cada golpe queda como una nota corta.
- La octava, los acordes y seguir el tono son de las notas: en batería no cuentan.

#### Enseñar tus sonidos

Si Pulse no reconoce bien alguno de tus sonidos:

1. Clic derecho en **Voz** y pulsa **Bombo**, **Caja** o **Charles**.
2. Haz ese sonido 4 veces, como lo harías tocando.

Desde entonces ese sonido tuyo toca ese instrumento. Los enseñados llevan ✓ y se guardan en los ajustes de Pulse. Enseñarlo otra vez sustituye lo anterior; **Olvidar lo enseñado** vuelve a los sonidos de serie.

+++ Detalles
Cada golpe suena unos 15–20 ms después de hacerlo.
+++

> Consejo: para pasar a MIDI algo ya grabado, usa **Audio a MIDI › Melodía**, que lo analiza con más calma y saca notas más limpias. Un beatbox grabado en audio, con **Audio a MIDI › Beatbox**. Ver [[Audio a MIDI]].

# Editar

## Seleccionar, copiar y pegar clips

Elige uno o varios clips y cópialos, muévelos o duplícalos, en la misma pista, entre pistas o de una vista a otra. Funciona igual en Session y en el Arrangement.

### Seleccionar clips

- **Clic** en un clip: lo selecciona.
- {⌘ clic} o {⇧ clic}: añade o quita clips de la selección.
- **Arrastrar desde un hueco**: dibuja un recuadro y selecciona lo que queda dentro.

### Copiar, cortar, pegar y duplicar

| Atajo | Qué hace |
|---|---|
| {⌘C} / {⌘X} / {⌘V} | Copiar, cortar y pegar los clips seleccionados, también de varias pistas a la vez y de una vista a otra |
| {⌘D} | Duplicar |
| {⌫} | Eliminar |

- **Pegar en el Arrangement**: los clips van justo después de los seleccionados o, si no hay ninguno, al compás del cursor. Cada pista copiada va a la pista que le corresponde y los clips mantienen sus distancias.
- **Pegar aquí** (clic derecho en un hueco): pega en ese punto exacto.
- **Pegar en Session**: desde el slot seleccionado hacia abajo; si no caben, más abajo.
- **Duplicar** en el Arrangement pone la copia justo detrás. Con varios clips, el bloque entero va detrás del último, sin moverlos entre sí. En Session, la copia va al siguiente slot libre.

> Nota: en el editor de clips, si hay notas seleccionadas, estos atajos actúan sobre las notas. Mientras escribes en un campo de texto, los atajos son del campo.

## Dividir, unir y consolidar

Corta un clip en trozos, junta varios en uno o convierte lo que suena en un archivo de audio nuevo. Todo se hace desde el Arrangement, con atajos o con clic derecho en el clip.

### Cortar un clip en dos o más trozos

1. Haz clic en el clip donde quieras cortar. Queda una marca naranja, ajustada a la rejilla; con {⌥ clic}, justo donde hiciste clic.
2. Pulsa {⌘E} (**Dividir clip**).
3. Para más trozos, repite: clic, {⌘E}, clic, {⌘E}…

- Si no has hecho clic en el clip, {⌘E} corta por el cursor de reproducción (el que mueves en la regla).
- **Dividir aquí** (clic derecho en el clip) corta donde hiciste clic.

### Trocear en partes iguales

Clic derecho en un clip → **Trocear ▸**: **cada tiempo**, **cada 2 tiempos**, **cada compás**, **cada 2** o **cada 4 compases**, siguiendo las líneas de la rejilla.

- Con varios clips seleccionados, los trocea todos.
- Los trozos quedan seleccionados, para moverlos o borrar los que sobran.
- Cada parte suena igual que antes y los archivos no se tocan.

+++ Detalles
- Una nota MIDI que cruza el corte termina en él, y lo que le quedaba se toca al principio de la parte derecha: vuelve a oírse su ataque.
+++

### Unir clips

{⌘J} (**Unir clips**) junta dos o más clips de la misma pista en uno, del principio del primero al final del último.

- Los clips MIDI juntan sus notas.
- Los de audio se renderizan a un archivo nuevo, con silencio en los huecos.

### Consolidar e invertir un audio

- **Consolidar** (clic derecho en un clip de audio): lo que suena el clip, con su warp, transposición, ganancia y fundidos, pasa a un archivo de audio nuevo solo con su trozo.
- **Invertir** (clic derecho en uno o varios clips de audio): crea un archivo nuevo, «… (invertido)», con lo que suena el clip del final al principio. Sirve para un platillo, una voz o un golpe que crece hasta el cambio, como los efectos «reverse». Sus envolventes de clip se dan la vuelta con él, e invertirlo otra vez lo devuelve a como sonaba, con su nombre.

> Atención: al consolidar (y también al unir o invertir), el clip pierde sus tomas. Por ahora unir, consolidar e invertir no aplican los efectos de la pista.

+++ Cómo funcionan los fundidos al cortar
- Al consolidar, unir o invertir, los fundidos del clip quedan dentro del archivo nuevo; el clip nuevo no lleva fundidos, para no aplicarlos dos veces.
- Al **dividir** un clip con fundidos, la parte izquierda conserva el de entrada y la derecha el de salida. Si cortas en mitad de un fundido, cada parte se queda con su trozo de la curva: juntas suenan igual que el clip entero. Un proyecto con un fundido partido así solo se abre en esta versión de Pulse o en una más nueva.
- **Consolidar** o **Invertir** un trozo de un clip dividido guarda en su archivo un poco más de sonido a cada lado, para que siga fundiéndose con su vecino en el corte.
- Dos clips de audio seguidos, sin hueco entre ellos (como las partes de un clip dividido o los trozos de tomas), se funden entre sí durante 10 ms en el corte en vez de bajar cada uno a silencio. Al unirlos, el archivo nuevo conserva ese fundido.
+++

## Transiciones

Efectos para los cambios de sección (subidas, bajadas, impactos), hechos a la medida de tu canción: a su tempo y en su tonalidad.

### Poner una transición

1. En el Arrangement, haz clic derecho en un marcador, en la franja de marcadores o en un hueco de una pista.
2. Elige **Transición aquí ▸** y el tipo.

| Transición | Qué es |
|---|---|
| **Subida** (1, 2, 4 u 8 compases) | Ruido y un tono que suben de grave a agudo y de suave a fuerte, y terminan justo en ese compás. El riser de antes del drop |
| **Bajada** (1, 2 o 4 compases) | Lo contrario, desde ese compás: cae y se apaga (downlifter) |
| **Impacto** | Un golpe grave que baja de tono, con ruido y cola, de dos compases, desde ese compás |
| **Platillo invertido** (1, 2 o 4 compases) | Un platillo al revés que crece hasta ese compás |

### Dónde quedan

- Cada transición es un clip de audio a tu tempo en una pista **Transiciones**, que se crea la primera vez; las siguientes van a ella.
- Si cambias el tempo, la transición lo sigue.
- Los archivos van a `Samples/Transiciones` dentro del proyecto.
- El [[Asistente de mezcla y master]] trata esa pista como efectos: no la comprime ni la mezcla como un bajo.

> Consejo: suenan a −6 dBFS. Bájalas con el fader de la pista y dales reverb o delay con los envíos (ver [[Mezclador]]).

+++ Detalles
- El tono va en la tonalidad del proyecto, si tiene ([[Tonalidad y escala]]), y en La si no.
- Si una subida no cabe entera antes del principio de la canción, empieza en el principio.
+++

## Automatización

La automatización hace que un control se mueva solo mientras suena la canción: el volumen que sube en el estribillo, un filtro que se abre, el tempo que se acelera. Se dibuja con puntos en un carril o se graba moviendo el control.

### Dibujar automatización en una pista

1. En el Arrangement, pulsa el botón **A** de la cabecera de la pista: aparece su carril de automatización.
2. Elige el parámetro en el desplegable: volumen, panorama, envíos o cualquier knob del instrumento, de los efectos o de un plugin. Con muchos parámetros aparece un buscador. Los que ya tienen automatización llevan ●.
3. Haz clic en el carril para añadir puntos.

| Acción | Qué hace |
|---|---|
| Clic | Añade un punto |
| Arrastrar un punto | Lo mueve |
| Doble clic en un punto | Lo borra |
| {⌥ + arrastrar} un tramo | Lo curva: hacia arriba abomba la curva, hacia abajo la hunde. Para fundidos y subidas más naturales |
| {⌥ + doble clic} en un tramo | Lo vuelve recto |

- **Borrar automatización** (en la cabecera o con clic derecho en el carril) la quita.
- **Ocultar carril** lo esconde sin borrarla.

### Mover a mano un control automatizado

Los controles automatizados llevan un punto rojo y se mueven solos. Si mueves uno a mano, su automatización queda **anulada**: el punto se vuelve hueco y el carril, gris. Pulsa **► Arrangement** en el transporte para que vuelva a mandar.

### Cambiar el tempo durante la canción

La última fila del Arrangement es el **Master**. Su botón **A** muestra su automatización: **Tempo**, volumen, panorama y los knobs de sus efectos.

- En el carril **Tempo**, puntos a distintos tempos dan saltos (una intro lenta y un estribillo más rápido). Una línea entre dos puntos es un accelerando o un ritardando; con {⌥ + arrastrar} se curva.
- El carril va de 40 a 240 BPM.
- Al **exportar**, la canción dura lo que dan sus tempos.
- Mover el tempo del transporte a mano anula la automatización (lo indica su borde) hasta que pulses **► Arrangement**.

> Atención: los audios con warp **Off** no siguen el tempo. Suenan a su velocidad, también durante la automatización de tempo, sin desafinar ni saltar; pero si cambias el tempo a mano mientras suenan, sí pueden saltar. En una canción con cambios de tempo, pon en **Stretch** los audios que deban seguirlo (ver [[Editor de audio]]).

+++ Qué sigue al tempo
Todo sigue la automatización de tempo: los clips MIDI, los audios en Stretch, Beats o Re-Pitch, el metrónomo, los delays y los arpegiadores.
+++

### Grabar automatización

1. Enciende **A** (Armar automatización) en el transporte.
2. Graba en el Arrangement (ver [[Grabar]]).
3. Mueve cualquier control: con el ratón, con un knob MIDI asignado o en la ventana de un plugin.

- Lo grabado sustituye a los puntos que había en ese tramo.
- Con el loop activado, cada pasada sustituye a la anterior donde mueves el control y deja lo de antes donde no lo tocas.
- Al grabar un clip en Session, los controles de esa pista se graban como envolventes del clip.

### Envolventes de clip

Automatización dentro de un clip, que se repite con él cada vez que suena.

1. Abre el clip en el editor.
2. En el desplegable **Carril**, elige **Envolventes del clip** (la otra opción es **Velocidad**).
3. Dibuja como en la pista: clic añade un punto, arrastrar lo mueve, doble clic lo borra, {⌥ + arrastrar} un tramo lo curva y {⌥ + doble clic} lo endereza.

- **Borrar envolvente** la quita.
- Las envolventes solo mandan mientras suena el clip. Al terminar, el volumen, el panorama, los envíos y los parámetros vuelven a lo que marcan el mezclador y la automatización de la pista.

## Editor MIDI

Donde escribes y editas las notas de un clip MIDI. Se abre al seleccionar un clip MIDI; en las pistas de batería, cada fila lleva el nombre de su sonido.

Al abrir un clip, y al escribir notas con **Acordes**, **Generar ritmo**, **Acompañar** u **Otra variación**, el editor va a donde están las notas: en una batería, a las filas que suenan (el bombo y la caja primero, si no caben todas).

### Escribir y editar notas

| Acción | Qué hace |
|---|---|
| Doble clic en vacío | Crea una nota |
| Doble clic en una nota | La borra |
| Clic | Selecciona ({⇧ clic} añade) |
| Arrastrar una nota | La mueve ({⌥}: la copia) |
| Arrastrar el borde derecho | Alarga la nota |
| Arrastrar en vacío | Recuadro de selección |
| Clic en vacío | Deselecciona y pone el marcador naranja, donde pega {⌘V} |
| Clic en una tecla del teclado de la izquierda | La escuchas |

- **Modo dibujo** ({B}): clic añade o borra notas; clic y arrastrar añade una nota y la alarga.

### Atajos del editor

| Atajo | Qué hace |
|---|---|
| {⌘A} | Seleccionar todas |
| {⌘C} / {⌘X} / {⌘V} | Copiar, cortar y pegar (también entre clips) |
| {⌘D} | Duplicar |
| {⌫} | Borrar |
| {Esc} | Deseleccionar |
| {← →} | Mover por la rejilla ({⇧}: un compás) |
| {↑ ↓} | Transponer ({⇧}: una octava). Con **Plegar**, una nota de la escala |
| {⌘U} | Cuantizar |

### Cambiar la velocidad

Elige **Velocidad** en el carril de abajo y arrastra arriba o abajo cerca de una nota. Las notas seleccionadas cambian juntas.

### La barra del editor

- **Nombre** del clip.
- **Loop**: 1, 2, 4, 8 o 16 compases (del compás donde está el clip), o cualquier número de semicorcheas en el campo **/16**. Un clip de 3/16, 5/16 o 7/16 sobre un 4/4 se desplaza en cada compás: sirve para polirritmos.
- **Rejilla** (1/4 a 1/32, con tresillos) y **Nota** (longitud de las notas nuevas).
- **Dibujar** ({B}): modo lápiz.
- **Cuantizar** ({⌘U}) con su **intensidad** (%) y **finales** (cuantiza también el final de las notas). Actúa sobre las notas seleccionadas o, si no hay, sobre todas.
- **Prob.** (%): en cuántas vueltas del loop suena cada nota (ver «Notas que no suenan siempre», abajo).
- **-12, -1, +1, +12**: transponer. **×2**: duplica el loop copiando las notas. **Zoom**.
- Con una escala en el proyecto, **Plegar** y **A la escala** (ver [[Tonalidad y escala]]).
- **Acordes**: escribe acordes de la escala, progresiones enteras, y sugiere el siguiente (ver [[Acordes]]).
- En pistas de batería, **Generar ritmo** (ver [[Generar ritmos]]).
- Bajo la regla, una franja muestra el **acorde** que suena en cada tramo.

### Notas que no suenan siempre

Con **Prob.** eliges qué porcentaje de las vueltas del loop suena cada nota seleccionada (o todas, si no hay selección). Un hat al 60 % o una nota fantasma de caja al 30 % hacen que el loop varíe solo. Las notas con probabilidad se ven más pálidas, con su %.

+++ Cómo funciona el azar
- Es al azar, pero siempre el mismo azar: la canción suena igual cada vez que la reproduces, al exportarla y al congelar la pista.
- Cada copia de un clip en el Arrangement tiene su propio azar: si duplicas un loop de charles al 60 %, cada copia toca notas distintas.
+++

### Darle groove a un clip

El **Groove** cambia cuándo y con qué fuerza suenan las notas al tocarlas, sin moverlas.

- **El del proyecto**: el swing del transporte (ver [[Transporte]]).
- **Uno propio para este clip**, con:
  - **Swing** y su rejilla.
  - **Acentos**: contratiempos más suaves.
  - **Humanizar**: pequeños desplazamientos al azar de tiempo y velocidad, distintos en cada vuelta.
- Grooves listos: **Recto**, **Swing 1/16**, **MPC 1/16**, **Shuffle 1/8** y **Humano**.
- **De tus grabaciones**: los grooves que has sacado de un audio (ver el apartado siguiente). Cada nota que cae en la rejilla se mueve como su golpe en la grabación y toma su fuerza. **Tiempo** y **Dinámica** dicen cuánto: 100 %, igual que la grabación; 50 %, a medio camino. **Humanizar** se suma encima.

### Sacar el groove de un audio

Copia el pulso de un loop de batería, un break o tu beatbox y aplícalo a cualquier clip MIDI.

1. Clic derecho en un clip de audio → **Sacar groove**.
2. Pulse escucha dónde cae cada golpe respecto a la rejilla y con qué fuerza, y lo guarda en el proyecto con el nombre del clip.
3. En el **Groove** de cualquier clip MIDI, elígelo en **De tus grabaciones**.

- Pulse sabe si el loop va en **semicorcheas** o en **corcheas** (un shuffle). La barra de estado lo dice, junto con cuánto se retrasan sus contratiempos: «en semicorcheas, los contratiempos un 30 % de paso tarde».

> Atención: un proyecto con grooves sacados de audio no se abre en versiones de Pulse anteriores a esta función (formato 11), para que no se pierdan.

+++ Cómo lo mide
- Analiza los golpes paso a paso a lo largo de un compás.
- Si la grabación entera va un poco tarde o pronto (el clip empieza unos milisegundos antes del golpe), no cuenta: lo que guarda es cómo se separan sus golpes de los tiempos.
- Sigue el warp del clip: si el loop está ajustado al tempo, el groove sale de cómo suena así.
+++

## Expresión: pitch bend, rueda y controles en el clip

Lo que haces con las ruedas y los controles del teclado al grabar queda en el clip, como las notas: el pitch bend, la rueda de modulación, el aftertouch y cualquier CC. También puedes dibujarlo a mano bajo las notas.

### Grabarlo

1. Graba MIDI como siempre, en el Arrangement o en un slot (ver [[Grabar]]).
2. Mueve el bend, la rueda o lo que quieras mientras tocas.
3. Al terminar, el editor abre el clip con el primer control grabado bajo las notas.

- Se guarda tal como sonó, con menos puntos: los que no cambian la forma se quitan.
- Grabando en loop, cada vuelta sustituye un control solo donde lo moviste. Lo demás se queda.
- El **pedal de sustain** no se graba como control: alarga las notas, que suenan igual sin pedal.

### Verlo y dibujarlo

1. Abre el clip en el editor.
2. En **Carril**, bajo **Expresión MIDI del clip**, elige un control que tenga el clip (◆) o añade uno: **Pitch bend**, **Rueda de modulación**, **Aftertouch**, **Brillo**, **Expresión**, **Soplo** o **+ CC** con su número.
3. Dibuja en el carril:

| Herramienta | Qué hace |
|---|---|
| **Lápiz** | Arrastra para dibujar a mano alzada |
| **Línea** | Arrastra de un punto a otro: una rampa recta |
| Arrastrar un punto | Lo mueve |
| Doble clic en un punto | Lo borra |
| {⌥ + arrastrar} entre dos puntos | Curva ese tramo ({⌥ + doble clic}: recto otra vez) |

- **Rejilla**: lo que dibujas cae en la rejilla del editor.
- **Borrar carril** quita ese control del clip.
- El pitch bend se queda en el centro al acercarte a él, como la rueda de verdad.

### Cómo suena

- Los controles se repiten con el clip en cada vuelta, también en el Arrangement y en los clips lanzados.
- Al parar o al acabar el clip, el bend vuelve al centro, y la rueda y el aftertouch, a cero: nunca se queda una nota desafinada.
- En los instrumentos de Pulse: el **bend** dobla ±2 semitonos, la **rueda** da vibrato, el **brillo** (CC 74) y el **aftertouch** abren el filtro de los sintes con filtro, y la **expresión** (CC 11) baja el volumen.
- El **Modular Synth** y los **plugins** reciben todos los controles y hacen lo que tengan asignado.
- Al **exportar MIDI** se escriben; al **importar** un archivo MIDI, su bend, aftertouch y controles llegan como carriles.

> Atención: un proyecto con controles en sus clips no se abre en versiones de Pulse anteriores a esta función (formato 15), para que no se pierdan.

+++ Detalles
- Los controles se leen cada 64 muestras, en las mismas muestras siempre: suena igual con cualquier tamaño de búfer y al exportar.
- Mientras suena un clip con un control, ese control manda; el teclado puede moverlo, pero el clip lo vuelve a fijar en cuanto su curva cambia.
- Entre dos valores grabados con una pausa larga, el valor se mantiene hasta justo antes del siguiente: un salto queda como salto.
+++

## Editor de audio

Donde ajustas cómo suena un clip de audio: si sigue el tempo, su tono, su volumen, sus fundidos y su afinación. Se abre al seleccionar un clip de audio.

### Hacer que un audio siga el tempo

Elige el **Warp**:

| Warp | Qué hace |
|---|---|
| **Off** | Suena a su velocidad, sin seguir el tempo |
| **Re-Pitch** | Sigue el tempo cambiando velocidad y tono, como un disco |
| **Stretch** | Sigue el tempo manteniendo el tono |
| **Beats** | Para baterías: cada golpe empieza donde le toca y suena a su velocidad, sin el eco que deja Stretch al cambiar mucho el tempo. Si el tempo es más lento, entre golpes queda un silencio corto |

- **BPM original**, con **÷2** y **×2** si el detector se ha ido al doble o a la mitad.
- **Tempo: Detectar / Usar en el proyecto** (ver [[Tempo de los audios]]).

### Los controles del clip

- **Longitud** en tiempos, y **Ajustar al archivo**.
- **Ver**: **El clip** o **Archivo entero**. En «Archivo entero», clic o arrastrar en la forma de onda mueve el **inicio** del clip.
- **Inicio** en segundos. Puede ser negativo, para empezar un compás antes del archivo.
- **Fundidos** de **entrada** y **salida**, en tiempos: los mismos que las asas del Arrangement. En Session se aplican en cada vuelta del bucle.
- **Gain** (−36 a +12 dB) y **Transp.** (±24 semitonos, solo en Stretch).
- **Tonalidad** del audio, **Seguir la escala** y **Detectar** (ver [[Tonalidad y escala]]).
- **Marcadores** de warp y **Quitar** (ver abajo).
- **Trocear → A Drum Pads** y **Separar en stems** (ver [[Trocear y separar stems]]).
- **Afinación → Editar notas…**: afinar la voz nota a nota (ver abajo).
- **Carril**: envolventes del clip (ver [[Automatización]]).

### Afinar una voz nota a nota

Como Melodyne: en un clip con una voz, o un instrumento que toque una nota a la vez, ves cada nota como un bloque y la llevas a donde quieras.

1. Selecciona el clip y pulsa **Afinación → Editar notas…**. La primera vez analiza la voz unos segundos.
2. La forma de onda se cambia por sus **notas**: cada una es un bloque sobre el teclado, a la altura a la que suena. Debajo va el tono que cantaste (línea gris) y, si la has cambiado, cómo sonará (del color del clip).
3. Arrastra una nota arriba o abajo, o haz doble clic para afinarla.
4. **← Forma de onda** vuelve al editor de audio. Un clip afinado muestra **Editar notas… ✓**.

| Acción | Qué hace |
|---|---|
| Arrastrar una nota | La cambia de nota, a semitonos enteros: con moverla un poco, una nota desafinada queda afinada. Con {⌥}, se mueve libre, en cents |
| Doble clic en una nota | La afina a la nota de la **Escala** del proyecto más cercana (sin escala, al semitono más cercano) |
| Clic | Elige una nota |
| {⇧ clic} o {⌘ clic} | Añade o quita notas de la elección. Al arrastrar una de las elegidas, se mueven todas |

- **Afinar**, **Sin vibrato** y **Como se cantó** actúan sobre las notas elegidas o, si no hay ninguna, sobre todas. **Sin vibrato** deja cada nota quieta en su tono; **Como se cantó** la devuelve a como estaba.
- Con una nota elegida, arriba ves dónde se cantó y dónde va («A2 +30 cents → A2») y su **vibrato**: 100 % como se cantó, 0 % nada, más de 100 % más marcado.
- {⌘Z} deshace al momento.

> Consejo: para afinar en vivo mientras cantas, o una pista entera sin ir nota a nota, usa el efecto **Tuner** (ver [[Dispositivos y efectos]]).

+++ Cómo funciona
- La voz conserva su timbre al moverla (sus vocales no cambian, no suena «a ardilla») y dura lo mismo. Lo que no tocas queda exactamente igual.
- Al soltar, el clip pasa a sonar un archivo nuevo con la voz afinada, en la carpeta **Afinados** del proyecto. Las copias de ese clip cambian con él; otros clips que usen la grabación original, no.
- Al guardar se borran los archivos afinados que ya no usa nada.
+++

### Ajustar un audio cuyo tempo varía

Con los marcadores de warp clavas momentos del audio a tiempos de la canción. Se ponen en la franja bajo la forma de onda, con Warp distinto de **Off**.

1. Doble clic en la franja: añade un marcador. El primero añade también uno al inicio y otro al final.
2. Arrastra un marcador: ese punto del audio cae en ese tiempo, y los tramos de los lados se aceleran o frenan.
3. Doble clic en un marcador: lo quita.

## Tempo de los audios

Al importar o soltar un audio, Pulse busca su tempo y sus compases y lo ajusta para que vaya al ritmo de tu canción. Tarda más o menos un segundo por minuto de audio.

### Qué hace al importar

- Si encuentra un ritmo claro, el clip pasa a **Stretch** a ese tempo, o a **Beats** si es un loop de batería (por su nombre, o porque no tiene tonalidad).
- El clip empieza en el compás del primer tiempo fuerte y dura compases enteros.
- Si el tempo varía, pone un marcador de warp en cada compás.
- Si no encuentra un ritmo claro, el clip suena a su velocidad.

### Corregirlo a mano

- **Detectar tempo** (clic derecho en un clip de audio, o **Detectar** en el [[Editor de audio]]): vuelve a analizarlo.
- **Usar el tempo de este clip en el proyecto**: pone el proyecto al tempo del clip.

> Consejo: en un loop de hasta 32 s, si el nombre del archivo dice su tempo («Tops 150», «Bass_128bpm») y a ese tempo dura compases justos, Pulse usa ese tempo aunque el detector oiga otro.

+++ Cómo elige el tempo
- Un loop corto (hasta 32 s) dura compases enteros.
- Si su **nombre dice su tempo** y a ese tempo dura compases justos, gana el nombre, diga lo que diga el detector (unos tops sin bombo pueden salirle a 245). También al trocearlo en la Drum Rack.
- Si no, y el tempo encontrado queda cerca del que hace que dure 1, 2, 3, 4, 6, 8, 12 o 16 compases justos, usa ese: un loop de 4 compases a 150 queda a 150, no a 151.
- Si el detector se equivoca más, pero el loop dura compases justos al tempo del proyecto, usa el del proyecto.
- Mientras se analiza, y si no encuentra un ritmo claro, un audio corto se toma por un loop de compases enteros: al tempo de su nombre si lo dice y dura compases justos a él, o de 1, 2, 4 u 8 compases al tempo más cercano al del proyecto.
+++

# Componer

## Tonalidad y escala

La escala es la tonalidad de tu canción. Con ella, el Editor MIDI te enseña qué notas encajan, los acordes salen solos y los loops que añades se ponen en tu tono.

### Elegir la escala

1. En la barra de transporte, abre **Escala**.
2. Pulsa **Detectar en la canción** para que Pulse la busque, o elige una de las 24 a mano (mayores y menores).
3. Para quitarla, elige **Sin escala**.

**Detectar en la canción** escucha las notas de los clips MIDI (sin las baterías) y los audios que tienen una tonalidad clara. La barra de estado dice qué escala ha elegido, y avisa si no está clara.

> Nota: mi menor y sol mayor usan las mismas notas. Si sale la relativa y no es la que quieres, elígela a mano.

La escala se guarda con el proyecto y se deshace con {⌘Z}. Las pistas de batería no la usan.

### Escribir notas en la escala

Con una escala en el proyecto, el [[Editor MIDI]] te ayuda a no salirte:

- Las filas de las notas de la escala se ven más claras, y las de la tónica en naranja, con su nombre.
- **Plegar** deja solo las filas de la escala, todas con su nombre, más las de las notas que ya tengas fuera de ella, para que no se pierda ninguna. Al arrastrar notas arriba o abajo, o con {↑} {↓} y **-1 / +1**, saltan de nota en nota de la escala: no hay forma de salirse.
- **A la escala** lleva a la nota más cercana de la escala las que no son de ella: las seleccionadas o, si no hay selección, todas.

En [[Audio a MIDI]], **Melodía** usa la escala del proyecto en vez de buscar una.

### Loops en tu tonalidad

Los audios que añades (arrastrándolos, desde el navegador o con Importar) se ponen en la escala del proyecto, igual que se ponen a su tempo. Un loop de piano en fa menor suena en tu canción en la menor sin que hagas nada.

- Si el proyecto aún **no tiene escala**, toma la del primer loop con tonalidad, y los siguientes se ajustan a ella.
- Si **cambias la escala** del proyecto, todos los loops que la siguen se mueven con ella.
- Las **baterías** no se tocan, aunque su nombre diga una tonalidad: un loop de hats «Tops_150_Dm» se queda como está.

En el [[Editor de audio]] de cada clip:

- **Tonalidad** muestra la del archivo, y **Detectar** la busca de nuevo.
- **Seguir la escala** lo ajusta a la del proyecto o lo deja en la suya.
- Mover **Transp.** a mano hace que deje de seguir la escala.

Para que los loops nuevos no se ajusten, desmarca **Ajustar los loops nuevos a la escala** en el menú **Escala** del transporte.

+++ Cómo funciona
- Pulse busca la **tonalidad de cada archivo** primero en su nombre, como la escriben las librerías de loops («Pad_120_Am», «Keys F#m», «Bass_E», «124Gm»), y si no, en su sonido, cuando está clara. Si el nombre dice una y el sonido otra muy distinta, gana el sonido.
- El clip se **transpone por el camino más corto**, como mucho 6 semitonos, a la escala del proyecto. Si uno es mayor y el otro menor, va a la relativa: un loop en mi menor en una canción en sol mayor no se mueve.
- Cuentan como batería los audios con nombre de batería (kick, snare, hat, perc, drum, tops, groove, shuffle, rumble, fill, conga…) y los audios sin notas que destaquen.
+++

## Acordes

Pulse lee los acordes de tus clips mientras editas y te ayuda a escribirlos: un clic por acorde, con sugerencias de cuál va después y progresiones listas.

### Ver los acordes de un clip

En el [[Editor MIDI]], bajo la regla, una franja dice el acorde que suena en cada tramo: **Lam**, **Sol7**, **Do/Mi** (Do con Mi en el bajo)… Con una escala en el proyecto, también su grado: **Lam · vi**.

- Se actualiza mientras editas.
- Marca como mucho un acorde cada medio compás.
- Las notas sueltas de una melodía por encima no cambian el acorde.

Para sacar los acordes de un audio (una canción, un stem, un loop de piano), usa [[Audio a MIDI]] → **Acordes**.

### Escribir una progresión

Necesitas una **Escala** en el proyecto (ver [[Tonalidad y escala]]).

1. Haz clic en un hueco del editor donde quieras empezar: ahí queda el marcador naranja.
2. Abre el menú **Acordes** de la barra del Editor MIDI.
3. Haz clic en un acorde: se escribe en el marcador y el marcador pasa al final del acorde.
4. Sigue haciendo clic: clic, clic, clic construye la progresión.

Las notas nuevas quedan seleccionadas, para moverlas o transponerlas.

- Si los acordes no caben, el **loop del clip se alarga** en compases enteros hasta que caben todos (en el Arrangement, también el clip, hasta el siguiente). Una progresión de cuatro acordes en un clip de un compás lo deja en cuatro.
- {⌘Z} quita los acordes y devuelve el marcador naranja adonde estaba; {⇧⌘Z} los vuelve a poner.
- Un clip que aún no tiene nombre propio pasa a llamarse como su progresión: «Lam Fa Do Sol».

> Consejo: los acordes que suelen ir después del que hay antes del marcador salen en naranja («Después de Sol suelen ir: Do, Lam»). Si no sabes cómo seguir, empieza por ellos.

### Opciones del menú Acordes

- Los **siete acordes de la escala**, con su grado (I, ii, iii, IV, V, vi, vii°).
- En menor, también el **V mayor** (de la menor armónica), la dominante que lleva con fuerza a la tónica.
- **Con séptima**: cuatriadas en vez de tríadas (Domaj7, Rem7, Sol7…).
- **Duración** de cada acorde: 2 tiempos, 1 compás o 2 compases.
- **Progresiones** listas:

| Escala | Progresiones |
|---|---|
| Mayor | Pop (I–V–vi–IV), Balada, Años 50, Jazz (ii–V–I), Rock y Canon |
| Menor | Épica (i–VI–III–VII), Andaluza (i–VII–VI–V), Menor, Dramática y Lamento |

+++ Cómo coloca las notas
Cada acorde lleva el bajo abajo y las demás notas juntas, en la inversión más cercana a la del acorde anterior. Así las voces se mueven lo menos posible, como lo tocaría un pianista.
+++

## Acompañamiento

Pulse toca contigo: a partir de un clip con acordes escribe un bajo, un arpegio, un pad, acordes rítmicos o una melodía que los siguen compás a compás.

### Añadir una parte

1. Haz clic derecho en un clip con acordes: MIDI, o audio (un piano, una guitarra, una canción).
2. Elige **Acompañar** y una parte.
3. La parte se escribe en el mismo slot de Session o en el mismo tramo del Arrangement.

¿En qué pista?

- Si hay una **pista vacía de su papel** (la **Bajo** de una plantilla, una «Pad», «Arpegio» o «Acordes»), en esa, con su sonido.
- Si no, en una pista nueva justo debajo, que se llama como la parte: **Bajo**, **Pad**, **Arpegio** o **Acordes** (o **Bajo 2** si ya hay otra).

El clip se llama como lo que toca («Bajo: contratiempo»). Una pista nueva llega con un instrumento que le va: Acid para los bajos, FM para los arpegios y SuperSaw para el pad. Cámbialo como cualquier otro.

Entra con el fader de la pista de los acordes (el arpegio y el pad, 1,5 dB por debajo), para que al sumar un bajo, un pad y un arpegio la mezcla no pase de 0 dB.

### Las partes

| Parte | Qué toca |
|---|---|
| **Bajo: raíz** | La fundamental en cada tiempo, con alguna octava y notas de paso hacia el acorde siguiente. |
| **Bajo: octavas** | Fundamental y octava en corcheas (disco). |
| **Bajo: contratiempo** | En los contratiempos (house). |
| **Bajo: caminante** | Una nota por tiempo recorriendo el acorde; llega al siguiente por un semitono. |
| **Arpegio ascendente** y **Arpegio arriba y abajo** | Las notas del acorde una tras otra. |
| **Pad** | Los acordes sostenidos, con las voces enlazadas. |
| **Acordes rítmicos** | Acordes cortos con un ritmo sincopado. |

### Melodía

Las cuatro últimas partes de **Acompañar** escriben la melodía principal: el lead, el gancho o el topline. Van a la pista vacía **Melodía** (o **Lead**) si la hay; si no, a una pista nueva **Melodía** con un lead de fábrica que le va al estilo, 3 dB por debajo de los acordes.

| Estilo | Qué toca | Sonido de serie |
|---|---|---|
| **Melodía: gancho** | Un riff corto y sincopado de un compás que se repite (house, techno). | Pulse Synth «Pluck · Corto» |
| **Melodía: cantable** | Una frase de dos compases con notas largas, que se puede cantar (pop). | FM «Lead · Flauta FM» |
| **Melodía: lead de trance** | Corcheas y semicorcheas que saltan entre las notas del acorde. | Super Saw «Lead · Trance clásico» |
| **Melodía: pregunta y respuesta** | Un compás pregunta y el siguiente responde con el mismo ritmo. | Pulse Synth «Lead · Sierra brillante» |

Suena a canción, no a notas al azar:

- En cada tiempo fuerte toca una nota del acorde que suena; entre medias, notas de la escala que van por grados. Nunca choca un semitono con el acorde en un tiempo fuerte.
- Un motivo de uno o dos compases se repite por todo el clip y se adapta a cada acorde.
- Las frases van en parejas: la primera queda abierta y la segunda cierra en la tónica o la fundamental del acorde, igual que el final del clip.
- Sigue el compás del clip (4/4, 3/4, 6/8…) y su largo.

En el [[Editor MIDI]] de la melodía, junto a **Otra variación**, tienes tres desplegables:

- **Estilo**: la misma toma en otro estilo. El clip cambia de nombre si aún se llama como el estilo.
- **Densidad**: **Pocas notas**, **Normal** o **Muchas notas**.
- **Registro**: **Grave** (Do3–Sol4), **Medio** (La3–Mi5) o **Agudo** (Sol4–Re6).

Cambiarlos reescribe la misma toma; **Otra variación** escribe otra.

> Consejo: ¿quieres la melodía en un clip concreto? Crea un clip MIDI vacío en la pista del lead, en la misma escena (Session) o encima del tramo de los acordes (Arrangement), y elige en él **Acompañar** y una melodía. Pulse escribe la melodía en ese clip sobre los acordes que suenan con él en las demás pistas.

+++ Cómo elige las notas
La tonalidad es la **Escala** del proyecto si los acordes encajan en ella; si no, la que mejor encaja con los acordes. Sobre un acorde que se sale de la escala (Mi mayor en La menor), la melodía usa sus notas (Sol#) en vez de las de la escala que rozarían con ellas (Sol). Una nota a contratiempo que se alarga hasta el acorde siguiente lo anticipa, si no roza con el acorde de debajo; una en un tiempo fuerte solo se alarga si el acorde siguiente también la tiene.
+++

### Probar otra versión

En el [[Editor MIDI]] del clip nuevo, **Otra variación** lo vuelve a escribir de otra manera sobre los mismos acordes, tantas veces como quieras, hasta que lo edites.

> Consejo: los acordes se leen como en la franja de [[Acordes]]. Con una **Escala** en el proyecto salen mejor.

## Generar ritmos

**Generar ritmo** escribe una base de batería en el estilo que elijas: house, techno, trap, reggaetón… Puede escribirla tal cual o tocarla la IA de Pulse, con variaciones y redobles como un batería.

### Generar una base

1. Abre en el [[Editor MIDI]] un clip de una pista de batería (Drum Machine o Drum Pads).
2. En **Generar ritmo**, elige el **estilo**, el **nivel** y el **largo**.
3. Pulsa **Generar**. Las notas del clip se sustituyen por la base, y un clip que aún no tiene nombre propio toma el del estilo («Techno»).
4. **Otra versión** pasa a otra base del mismo estilo y nivel. {⌘Z} recupera las anteriores.

> Consejo: cada estilo suena a sí mismo a su tempo. **Tempo del proyecto a… BPM** lo pone.

### Opciones

- **Estilos**: House, Techno, Hardgroove, Hard techno, Trap, Drum & Bass, Hip hop, Pop / Rock, Reggaetón y Breakbeat.
- **Nivel**: **Básico** (lo esencial), **Medio** e **Intenso** (más golpes, percusión y notas fantasma).
- **Largo**: 1, 2, 4 u 8 compases. El ritmo se repite y el **cuarto compás** da la vuelta a la frase con una variación.
- **Redoble al final de cada frase**: en el último tiempo de cada 8 compases (o del clip, si tiene 4 o más), el de cada estilo. El reggaetón no lleva.
- **El bombo sigue al bajo**: añade bombos donde el bajo empieza sus notas (en corcheas), si hay una pista de bajo sonando con el clip.
- **Sonidos**: qué instrumentos puede usar la base: bombo, caja, palmada, charles, ride, crash, rim, toms, shaker, pandereta, cencerro, congas y clave.
  - Cada estilo trae los suyos marcados (el techno sin ride ni cencerro, el pop con crash, toms y pandereta), y puedes quitar o añadir los que quieras.
  - Los que el kit no tiene salen tachados.
  - Con el **crash** marcado y un clip de 4 compases o más, suena al empezar cada frase.
- **Sonido del estilo**: en una Drum Machine, pone la máquina del estilo (**808** para trap, hip hop y reggaetón; **909** para el resto) afinada como suena el estilo: el bombo largo y grave del trap, el seco del techno, la caja aguda del drum & bass… Si has afinado tú la máquina, solo cambia de máquina y respeta tus ajustes.

Funciona con cualquier kit: Pulse busca cada sonido por su nombre (kick/bombo, snare/caja, clap, hat/charles, tom, rim…) y usa lo que haya.

### Que lo toque la IA

Marca **Que lo toque la IA** y el modelo de batería de Pulse oye la base del estilo y la toca a su manera: la mantiene, la varía y mete redobles y cambios donde acaban las frases. Cada **Otra versión** es una interpretación distinta.

- Con **Básico** toca más a lo seguro; con **Intenso** se suelta más.
- El bombo que la base lleva en cada tiempo (el cuatro por cuatro del house, el techno, el hardgroove o el hard techno) se queda siempre: la IA varía el resto.
- Toca con los **Sonidos** marcados. Los toms entran en los redobles; desde **Medio**, también la pandereta y el shaker, y en **Intenso**, además, congas y cencerro.
- Toca en segundo plano: mientras tanto el botón dice **Generando…** y puedes seguir trabajando. La base entra en su clip en cuanto está; si estás en otro clip, cuando vuelvas a él.

> Nota: la IA va dentro de Pulse y funciona sin internet.

+++ Cómo son los estilos
- Las bases están escritas a mano, como las tocaría un batería de cada estilo: el bombo a negras y el charles abierto a contratiempo del house, la caja en el 3 y los redobles de charles del trap, el dos-pasos del drum & bass, el dembow del reggaetón…, con sus acentos y notas fantasma.
- **Hardgroove** (142 BPM) hace rodar toms y congas sobre el bombo a negras, con un poco de swing, y charles que solo suenan algunas vueltas (probabilidad por nota), así que el loop no se repite igual.
- **Hard techno** (152 BPM) lleva un bombo grave, largo y recortado (con Drive, para que corte en la mezcla), que galopa en Intenso, y redobles de palmas al final de cada frase.
- El hip hop lleva **swing**, y el house un poco: se pone en el **Groove** del clip.
- La IA es una red pequeña entrenada con las baterías de unas 70.000 canciones (Lakh MIDI Dataset) y de baterías reales (Groove MIDI Dataset).
+++

## De tarareo a canción

Tararea, canta o silba una melodía y Pulse hace una canción con ella: la melodía en notas, acordes, bajo y batería en el estilo que elijas.

### Hacer una canción con tu melodía

1. Graba la melodía en una pista de audio **con el metrónomo**, al tempo del proyecto (ver [[Grabar]]). También vale un clip MIDI que tenga una melodía.
2. Haz clic derecho en el clip → **De tarareo a canción** → un estilo (House, Techno, Pop / Rock, Reggaetón, Trap…).
3. Debajo de su pista aparecen cuatro pistas nuevas, en el mismo slot o en el mismo tramo del Arrangement.

Con un clip de audio, Pulse primero escucha la melodía unos segundos. La barra de estado dice qué acordes ha puesto («Do Fa Sol Do»). Todo es un solo paso: {⌘Z} lo quita.

### Las cuatro pistas

- **Melodía**: lo que tarareaste en notas, en un sintetizador. Cada nota va a su semicorchea, aunque entres un poco tarde o desafines un poco. Si sobra o falta alguna nota, corrígela en el [[Editor MIDI]].
- **Acordes**: un acorde de la escala por compás, el que mejor va con la melodía en ese compás. Según el estilo, pad sostenido o acordes rítmicos.
- **Bajo**: sobre los acordes, a la manera del estilo: en los contratiempos en house y techno, en la raíz en pop o trap, en octavas en breakbeat.
- **Batería**: un ritmo del estilo en una Drum Machine con su sonido, con un redoble al final de cada frase de ocho compases (o del clip, si es más corto).

Cada parte es una pista normal: cámbiale el instrumento, edita las notas o escribe otro bajo o pad sobre sus acordes con [[Acompañamiento|Acompañar]] (y luego **Otra variación** en su editor).

Las pistas entran con el fader bajo (la batería a −8 dB, el resto a −10,5 dB), para que la canción deje margen en el master y no sature.

+++ Cómo elige los acordes
- Las notas de la melodía en los tiempos fuertes cuentan más.
- Donde la melodía no lo deja claro, pone los acordes que suelen seguirse. Empieza y acaba en la tónica.
- En menor, el V puede ser mayor (Mi en La menor).
- La escala sale de la melodía si el proyecto no tiene una, y pasa a ser la del proyecto. Si el proyecto la tiene, usa esa.
+++

## Audio a MIDI

Saca las notas de cualquier audio (una voz, un piano, una batería, tu beatbox) y las pone en una pista MIDI nueva, para tocarlas con otro sonido o editarlas.

### Convertir un clip

1. Haz clic derecho en un clip de audio → **Convertir a MIDI**.
2. Elige qué quieres sacar (ver la tabla).
3. Las notas aparecen en una pista MIDI nueva justo debajo, en el mismo sitio: el mismo slot en Session, el mismo tramo en el Arrangement.

Tarda unos segundos y se hace en este Mac, sin internet; la barra de estado muestra el progreso. Las notas siguen el tempo del clip (su warp) y la fuerza de cada una pasa a su velocidad.

| Opción | Para qué |
|---|---|
| **Melodía** | Una nota cada vez: una voz, un tarareo o un instrumento solista. Canta una idea, conviértela y tócala con cualquier sinte. |
| **Melodía sin ajustar** | Igual, pero cada nota al semitono más cercano en vez de a la escala. Para melodías con notas fuera de la tonalidad, o un instrumento bien afinado. |
| **Armonía** | Todas las notas a la vez: acordes, piano o guitarra. |
| **Acordes** | Los acordes de una canción o de un instrumento, uno por cambio (como mucho cada medio compás), con el bajo y las voces enlazadas. Ignora la batería y los golpes cortos. |
| **Batería** | Cada golpe pasa a bombo, caja o charles, según dónde tiene su energía, en una Drum Machine. |
| **Beatbox** | Tu beatbox grabado pasa a batería. |

> Consejo: se combina bien con [[Trocear y separar stems|Separar en stems]]. Separa una canción y convierte el bajo o la voz en MIDI.

### Beatbox a batería

- **«pum»** es el bombo, **«pf»** o **«ka»** la caja y **«ts»** el charles, igual que con la **Voz** en modo batería (ver [[Tocar con la voz]]) y con los sonidos que le hayas enseñado allí.
- Cada golpe va a su semicorchea, así que no hace falta clavar el tiempo. Graba con el metrónomo.
- Da igual lo fuerte que grabaras: el golpe más fuerte suena con toda la velocidad y los demás en proporción.
- La barra de estado dice cuántos golpes de cada uno ha puesto.

### En qué tonalidad salen las notas

En **Melodía**, cada nota va a la de la escala de la canción más cercana, para que cantar un poco alto o bajo no cambie la nota. Las notas que cantas afinadas se quedan aunque no sean de la escala.

La tonalidad es la **Escala** del proyecto si la has elegido (ver [[Tonalidad y escala]]). Si no, sale de tu voz, de las notas MIDI del proyecto y de los audios con una tonalidad clara (las baterías no cuentan). La barra de estado dice cuál ha usado. **Acordes** también usa la escala del proyecto si la hay; si no, la del audio.

+++ Cómo funciona
- La melodía sigue el tono de la voz fotograma a fotograma con pYIN, el método de las herramientas para transcribir canto, también con voces muy graves. La corta en notas donde se queda en otra nota o vuelves a atacar.
- La armonía la encuentra Basic Pitch, un modelo de Spotify (licencia Apache 2.0) que va dentro de la app.
+++

## Trocear y separar stems

Dos formas de partir un audio: en golpes, para tocarlos desde pads, o en sus instrumentos (batería, bajo, voz y el resto).

### Trocear un loop en pads

1. Abre el clip en el [[Editor de audio]].
2. En **Trocear**, elige cómo cortar: **Transitorios** (en cada golpe), **1/16**, **1/8** o **1/4**.
3. Pulsa **A Drum Pads**.

El clip se corta en golpes, hasta 16, cada uno en un pad de un **Drum Pads** en una pista nueva. Un clip MIDI los toca en orden en el mismo sitio. Al cambiar el tempo, el groove lo sigue sin estirar el audio, y en el [[Editor MIDI]] puedes reordenar o repetir golpes.

El Drum Pads también tiene **Trocear archivo…**, para cortar un audio directamente en sus pads.

#### Ajustar los cortes a mano

Marca **Editar cortes**, bajo «Trocear». Los cortes salen numerados sobre la forma de onda, con una franja debajo:

- **Arrastra** un corte para moverlo.
- **Doble clic** en un corte lo quita; en un hueco, añade otro (hasta 16, uno por pad).
- **A Drum Pads** trocea entonces por tus cortes.

> Atención: cambiar el modo (Transitorios, 1/16…) vuelve a buscar los cortes y pierdes los que hayas movido.

### Separar en stems

Un stem es una parte de la mezcla por separado. Pulse saca cuatro: **Batería**, **Bajo**, **Voz** y **Resto**.

1. Haz clic derecho en un clip de audio → **Separar en stems…**, o usa el botón del editor de audio.
2. Espera: puedes seguir trabajando mientras tanto. **Cancelar** la detiene.
3. Aparecen cuatro pistas nuevas debajo, **Batería**, **Bajo**, **Resto** y **Voz**, con el clip en el mismo sitio, y la original se desactiva para que todo suene igual.

Cada pista nueva es una copia de la original, con sus efectos, volumen, panorama, envíos, automatización y grupo, y el clip conserva sus envolventes.

- Se hace en este Mac, sin enviar nada a internet.
- La primera vez descarga el modelo (174 MB) de GitHub, sin cuenta. Si la conexión se corta o cancelas, la próxima vez sigue desde donde se quedó.
- Se hace una separación a la vez, y hacen falta 4 pistas libres (máximo 64 pistas).

## Arreglo automático

Cuando tienes las ideas en escenas de Session, **Crear › Arreglo automático…** hace con ellas una canción entera en el Arrangement: intro, estrofas, estribillos o drops, con subidas y cortes.

### Crear la canción

1. Abre **Crear › Arreglo automático…**.
2. Elige una **estructura** (ver abajo).
3. Revisa la tabla: puedes cambiar los **compases** de cada parte, **qué pistas suenan** en cada una (casillas) y los redobles y cortes.
4. Pulsa **Crear**. Pulse te lleva al Arrangement, con un **marcador** con el nombre de cada parte y el loop en toda la canción.

Las estructuras:

- **Electrónica**: intro, subida, drop, break, subida, drop, outro.
- **Pop**: intro, estrofa, estribillo, estrofa, estribillo, puente, estribillo, final.
- **Corta**: intro, A, B, final.

> Atención: **Crear** sustituye lo que hubiera en el Arrangement. {⌘Z} lo recupera.

> Consejo: llama a tus escenas como las partes («Intro», «Drop», «Estribillo», «Break»…) y cada parte usará la suya.

### Qué hace en cada parte

- Cada parte toma una escena (ver «Cómo elige» abajo).
- Cada parte lleva solo las pistas que le tocan según su **papel**: sin bajo en la intro, sin batería en el break… En el estribillo o el drop suena todo: si una pista no tiene clip en esa escena, trae el suyo de otra.
- **Subidas**: el último compás de la batería es un **redoble** de caja que acelera y crece, y el resto de la batería calla debajo.
- **Cortes**: el último tiempo antes del drop va en silencio, para que el drop entre con fuerza.

+++ Cómo elige
- **Escenas**: cada parte toma la escena que se llama como ella («Break» para el break, «Intro», «Drop», «Estribillo»…; también «Build» o «Subida» para la subida). Si no hay ninguna, una que se llame como una parte de su energía («Groove» para la estrofa). Si tampoco, la más llena (más batería, bajo y melodía) es el estribillo o el drop, la siguiente la estrofa y la más ligera la intro.
- **Papeles**: Pulse deduce el papel de cada pista (batería, bajo, armonía, melodía) del instrumento, el nombre y las notas. El Kick Designer es batería. Una pista de audio es lo que sean sus loops: un loop de batería (en **Beats**, o con nombre de batería como «Tops_150») es batería, aunque la pista se llame «6 Audio».
- **Redobles**: son de caja, o de palmas si la caja no está en el 2 y el 4; nunca del bombo. Lo que calla debajo es el resto de la batería: el bombo en su pista y los loops.
+++

# Sonido

## Instrumentos

Los instrumentos son lo que suena en una pista MIDI. Pulse trae sintetizadores, samplers y cajas de ritmos.

### Poner un instrumento

1. Selecciona una pista MIDI.
2. En el navegador, abre **INSTRUMENTOS** y haz clic en uno.

Los editores grandes (**Editor ⤢**) se abren de uno en uno dentro de la ventana.

### Sintetizadores

| Instrumento | Para qué |
|---|---|
| **Pulse Synth** | Sintetizador sustractivo sencillo. |
| **Pulse Keys** | Piano, pianos eléctricos y órgano. Ver [[Pulse Keys]]. |
| **Super Saw** | Hasta 7 dientes de sierra desafinados: leads y pads. |
| **Acid Bass** | Bajo monofónico con filtro resonante, acento y slide. |
| **FM Synth** | Dos operadores con realimentación: campanas, pianos eléctricos. Cuanto más fuerte tocas, más brillante suena. |
| **Wavetable Synth** y **Polychrome Synth** | Ver [[Wavetable y Polychrome]]. |
| **Modular Synth** | Ver [[Modular Synth]]. |
| **Kick Designer** | Bombos. Ver [[Kick Designer]]. |

### Sampler

Toca una muestra en todo el teclado, desde su nota raíz.

- Arrastra un audio a su zona o usa **Cargar…**.
- Modos: **Clásico** (suena mientras mantienes la nota), **One-shot** (suena entera) y **Loop**.
- Controles: Root, Tune, Start, filtro y ADSR.

#### Multisample

Para instrumentos grabados nota a nota (un piano, unas cuerdas):

1. Carga o arrastra **varias** muestras a la vez.
2. Se reparten por el teclado según la nota de su nombre: «Piano C3», «Rhodes_F#2», «Strings Db4» (C3 es el do central).
3. Haz clic en una tecla del mapa para oírla.

- El mapa muestra las zonas sobre el teclado; arriba, las velocidades altas.
- **−12 / +12** mueven todo el mapa una octava, para librerías que llaman C4 al do central.
- **Zonas…** permite ajustar a mano las notas, la raíz y las velocidades de cada muestra.
- Hasta 32 muestras. Cargar una sola vuelve al modo normal.

+++ Cómo reparte las muestras
- Cada muestra cubre las teclas hasta la mitad de camino de sus vecinas y suena tal cual en su nota.
- Varias de la misma nota son **capas de velocidad**: la más suave abajo.
- Las que no llevan nota van una por tecla desde C3.
+++

### Drum Machine

16 sonidos sintetizados: bombo, rimshot, caja, palmas, hats cerrado y abierto, tres toms, cencerro, crash, ride, shaker, pandereta, conga y clave.

- Haz clic en un sonido para escucharlo. Se llaman como en **Generar ritmo**: Bombo, Caja, Palmada, Charles, Charles abierto, toms, Cencerro, Pandereta…
- Cada sonido tiene **Tune**, **Decay**, **Level** y **Drive**. Drive lo recorta como un clipper: más sucio y con más pegada; Level sigue siendo su volumen.
- **Máquina** cambia todos los sonidos a la vez:
  - **808**: bombo largo y redondo, palma, cencerro. Hip hop, trap, reggaetón.
  - **909**: bombo con pegada y charles brillantes. House, techno.
  - **Clásica**: los sonidos primeros de Pulse.
- Para llevar un sonido a su propia pista (y darle sus efectos), haz clic derecho en la cabecera de la pista → **Sacar a su pista ▸**.

> Nota: en los proyectos de cuando la Drum Machine tenía 8 sonidos, los 8 nuevos entran con Level a 0, para que suenen como sonaban. Súbelos si quieres oírlos.

### Drum Pads

16 pads con tus muestras. El 1 está abajo a la izquierda y empieza en C1.

- Haz clic en un pad para escucharlo y seleccionarlo.
- Arrastra uno o varios audios sobre los pads.
- Por pad: **Tune**, **Length**, **Level**, **Pan** y grupo de **choke**: los pads del mismo grupo se cortan entre sí, como el hi-hat abierto y el cerrado.
- **Kits ▾** carga un kit entero: el **Kit básico** (los sonidos de la Drum Machine) o uno de los diez kits por género de [[Sonidos de Pulse]].
- **Trocear archivo…** corta un loop en los pads (ver [[Trocear y separar stems]]).

## Pulse Keys

Un piano, dos pianos eléctricos y un órgano, sintetizados por Pulse: no cargan muestras, ocupan poco y responden a cómo tocas.

### Ponerlo en una pista

1. Selecciona una pista MIDI.
2. En el navegador, abre **INSTRUMENTOS** y haz clic en **Pulse Keys**.
3. Elige el **Modelo** a la izquierda del dispositivo, o uno de sus 34 sonidos en **Presets › De fábrica**, agrupados en Piano, Púas, Lengüetas y Órgano: pianos de pop, balada, jazz, cine o lo-fi, eléctricos de soul, funk o chill, y órganos de blues, reggae, iglesia, rock o house.

Al cambiar de modelo empiezas desde su sonido de partida; el volumen se queda como estaba.

| Modelo | Suena como |
|---|---|
| **Piano** | Un piano acústico de cola. |
| **Púas** | Un piano eléctrico de púas, tipo Rhodes: redondo, con campana al atacar. |
| **Lengüetas** | Un piano eléctrico de lengüetas, tipo Wurlitzer: más nasal y con mordida. |
| **Órgano** | Un órgano de ruedas fónicas, tipo Hammond, con altavoz giratorio. |

### Tocar con expresión

- La fuerza de cada nota cambia el volumen y, en el piano y los eléctricos, también el brillo: suave suena oscuro y fuerte, brillante. **Dinámica** dice cuánto; a 0, todas suenan igual.
- **Pedal de sustain**: las notas que sueltas siguen sonando hasta que levantas el pedal, y se graban así de largas. Ver [[Controladores MIDI#Tocar con un teclado MIDI]].
- Pitch bend y rueda de modulación (vibrato), como en los demás instrumentos de Pulse.

### Piano

- **Tono**: lo duros que son los martillos. Más alto, más brillante.
- **Duración**: cuánto tardan las notas en apagarse solas mientras las mantienes.
- **Apagado**: cuánto suena una nota al soltarla.
- **Martillo**: el golpe del martillo al empezar la nota.
- **Cuerdas**: cuánto se desafinan entre sí las cuerdas de cada nota, hasta unos 7 cents. Poco es un piano recién afinado; mucho, un piano de bar.
- **Cuerpo**: la resonancia de la tabla armónica y de la tapa.
- **Ancho**: los graves a la izquierda y los agudos a la derecha, como sentado al piano.

> Nota: como en un piano de verdad, las teclas más agudas (de F#5 hacia arriba) no tienen apagadores y siguen sonando al soltarlas.

### Pianos eléctricos

**Púas** y **Lengüetas** comparten controles:

- **Campana**: el brillo metálico del ataque.
- **Ladrido**: lo que gruñe la pastilla cuando tocas fuerte. En las notas suaves apenas se nota.
- **Tono**, **Duración**, **Apagado** y **Martillo**, como en el piano.
- **Trémolo** y **Vel. trémolo**: en Púas el sonido va de un lado a otro; en Lengüetas, el volumen sube y baja.
- **Drive**: satura el amplificador.

### Órgano

1. Saca los **registros** (las nueve barras) arrastrándolos hacia abajo: 0 es dentro y 8, del todo fuera. Con doble clic, uno sale o entra entero.
2. Elige la velocidad del altavoz giratorio con **Lento** o **Rápido**.

- Los registros, de izquierda a derecha: 16' (una octava por debajo), 5⅓', 8' (la nota), 4', 2⅔', 2', 1⅗', 1⅓' y 1'. Los marrones son los graves, los blancos las octavas y los negros las quintas y terceras.
- Puedes moverlos mientras tocas: cambian también las notas que ya suenan.
- **Percusión**: un golpe brillante al empezar la nota, en la **2ª** o la **3ª** armónica. Como en el órgano de verdad, solo suena si no hay otra tecla pulsada: toca ligado para que no se repita.
- **Perc. caída**: cuánto dura la percusión.
- **Click**: el chasquido de los contactos al pulsar y soltar.
- **Leslie**: cuánto suena por el altavoz giratorio. Al cambiar de velocidad, la bocina tarda alrededor de un segundo en llegar y el tambor, varios. La velocidad se puede automatizar (**Leslie rápido**).
- **Tono** quita agudos y **Drive** satura, para un órgano de rock.
- El órgano no responde a la fuerza de las notas.

> Consejo: algunas registraciones conocidas (cada cifra es una barra): **888000000** jazz, **888800000** gospel, **838000000** blues, **886000000** iglesia, **800000888** silbante, **006802000** balada y **888888888** todo fuera. Están entre los presets de fábrica.

+++ Cómo funciona
- No hay muestras: todo se sintetiza mientras tocas.
- **Piano**: un modelo físico de cuerdas (guías de onda). Cada tecla tiene dos cuerdas en los graves y tres en el resto, afinadas unos cents entre sí. Una cede su energía deprisa y las otras sostienen y baten: es la doble caída del piano. Cada cuerda es un retardo cerrado por un filtro de pérdidas (los agudos se apagan antes), uno de dispersión (la rigidez de la cuerda estira los armónicos hacia arriba) y uno de afinación fina. La afinación también va estirada, como la de un piano afinado a oído. El martillo empuja la cuerda unos milisegundos: menos y más seco cuanto más fuerte tocas, y por eso suena más brillante. Golpea cerca de un octavo de la cuerda, lo que quita algunos armónicos. Aparte suena la tabla armónica: decenas de modos de madera que dan el golpe del ataque (**Martillo**), más cinco resonancias entre 98 y 1700 Hz y dos reflejos cortos (**Cuerpo**). Los apagadores apagan antes los agudos.
- **Púas y Lengüetas**: la púa o la lengüeta vibra con pocos armónicos y un parcial de campana, inarmónico, que se apaga en décimas de segundo. Luego pasa por una pastilla descentrada: las oscilaciones grandes salen torcidas y aparecen armónicos pares, que son el «ladrido».
- **Órgano**: cada registro es una rueda fónica de seno puro con su propia fase. Por encima de la última rueda (unos 5,9 kHz), los registros vuelven una octava atrás, como en el original. El altavoz giratorio separa graves y agudos en 800 Hz. La bocina (agudos) gira a 0,8 Hz en lento y 6,8 Hz en rápido; el tambor (graves), a 0,67 y 5,9 Hz. Hay efecto Doppler y dos micrófonos para el estéreo.
- Suenan hasta 32 notas a la vez; si hace falta otra, se reutiliza la que menos suena.
+++

## Kick Designer

Un sintetizador solo para bombos: dibujas cómo cae el tono y el volumen, eliges el cuerpo, el click y la distorsión, y lo exportas como WAV si quieres.

En la cadena de la pista tiene un editor pequeño; **Editor ⤢** abre el completo.

### Dibujar las curvas

Las pestañas **Tono**, **Amp** y **Drive** tienen una curva cada una:

- Arrastra los puntos para moverlos.
- Doble clic añade un punto, o lo borra si haces doble clic sobre uno.
- Arrastra el círculo de un tramo para curvarlo.
- Clic derecho restablece la curva.
- En el editor grande, **Ajustar a rejilla** encaja los puntos.

### Controles

- **Cuerpo**: seno, triángulo, cuadrada o sierra, con fase y armónicos.
- **Sub**: un grave que sigue al cuerpo.
- **Click**: 12 sintetizados, o una muestra tuya (arrástrala al recuadro del click).
- **Key**: la nota que tocas afina el cuerpo, en vez de Tune.
- **Distorsión** (Soft, Hard, Fold, Tube, Crush), **EQ**, **Punch** y **Limitador**.
- **Duración**: libre o sincronizada al tempo, de 1/16 a un compás.
- Un osciloscopio muestra el ataque, y un espectro la fundamental y su nota.
- **Exportar WAV…** guarda un golpe como archivo.

Trae 18 presets de fábrica por estilos (ver [[Presets]]).

## Wavetable y Polychrome

Dos sintetizadores grandes con el mismo motor. **Wavetable** es el de uso general; **Polychrome** lo organiza en varios motores, con dos filtros y un secuenciador.

### Wavetable Synth

Una wavetable es una serie de formas de onda por la que el oscilador se desplaza: al moverse por ella, el sonido cambia de timbre.

- **Dos osciladores**. Cada uno puede ser **Wavetable**, **Sample**, **Granular**, **Spectral**, **Analog** o **Harmonic**, con Pos, octava, semitono, fino, nivel, pan, unísono y warp (Sync, Bend, PWM, Mirror, Quantize, FM).
- **Sub**, **ruido** y **filtro** (9 modos, con su curva). El modo **Comb** suena afinado a su corte en todo el teclado; con mucha resonancia y un golpe corto de ruido, hace de cuerda pulsada.
- 3 envolventes y 4 **LFOs dibujables** (clic derecho para formas básicas).
- **Arpegiador**, y modos Poly, Mono y Legato con Glide.
- **FX**: hasta 6 efectos propios, que se guardan con el preset.
- **MATRIX**: 16 rutas de modulación (fuente, cantidad, destino).

Trae 38 presets de fábrica por categorías: Bass, Lead, Pad, Pluck, Keys, Arp y FX (ver [[Presets]]). En varios, la **Macro 1** hace lo más útil del sonido: abre el filtro, cambia la vocal o da la profundidad del wobble.

#### Wavetables propias

- Trae 12 wavetables. **Cargar wavetable…** abre las tuyas.
- **Editor de wavetables…** permite dibujarlas: dibuja fotogramas clave con el ratón (o parte de los de la tabla actual) y la tabla pasa suavemente de uno a otro. Se guarda como WAV de wavetable estándar.

#### Modulación arrastrando

1. Busca la fila **FUENTES** (por ejemplo «✚ LFO 1»).
2. Arrastra la fuente a cualquier knob.
3. El knob muestra anillos de color. Con clic derecho en él ajustas o quitas cada modulación.

### Polychrome Synth

El mismo motor que Wavetable, en páginas **SYNTH**, **SEQ**, **FX** y **MATRIX**, con más cosas:

- **Dos filtros**, en serie o en paralelo, con balance entre ellos.
- Fuentes **aleatorias** (S&H y suave) y 2 **combinadores** que mezclan dos fuentes.
- **Secuenciador de 16 pasos** (SEQ): nota, velocidad, gate, probabilidad, slide y un carril de modulación.
  - Los pasos se dibujan arrastrando sobre ellos.
  - Mientras mantienes una tecla, toca los pasos transpuestos a ella.
  - **Variar** cambia notas al azar dentro de la escala.

Trae 34 presets de fábrica por categorías: metales, cuerdas y leads analógicos, bajos, órganos, campanas y flautas armónicos, filtros dobles (vocales, cruces y una cuerda pulsada), secuencias, pads y efectos (ver [[Presets]]).

## Modular Synth

Un sinte modular de pila: apilas módulos que generan y procesan sonido, de arriba abajo, y los modulas arrastrando. **Editor ⤢** abre el editor.

### Montar el sonido

La pila se lee de arriba abajo, dentro de **grupos**; cada grupo empieza de cero. Caben hasta 32 módulos.

- **Generan** sonido: Analog, Wavetable, Sample, Granular y Noise.
- **Procesan** lo de arriba: Filter, Nonlinear Filter y Distortion.
- **Encaminan**: Mix, Aux y **Output** (con destino Lane 1–3, Master o Sideband).

Cada módulo tiene botones para activarlo, **▲ ▼** moverlo, **⧉** duplicarlo, **×** quitarlo y **+** insertar otro debajo. **+ Añadir módulo** añade uno al final.

### Modulación

- **Con audio**: arrastra el asa verde **⟿** de un módulo a Phase, Harmonic, Shift, Level o Semi de otro (para FM, ring…), o al Input de un Aux.
- **Con moduladores** (hasta 32): Envelope, LFO, Curve, Random, Audio Follower, Pitch Tracker, Note, Velocity, Pressure, Pitch Wheel, MIDI CC, Macro, Remap, límites, Scale, Sample & Hold…
- **8 macros**.

Para modular un knob con un modulador:

1. Arrastra el **✚** del modulador a cualquier knob, también de otros moduladores.
2. En **⋯** de cada destino ajustas la curva y los límites.
3. Clic derecho en un knob modulado para ajustarlo.

### Voz y efectos

- **Voz**: Poly, Mono o Legato, polifonía, rango de pitch bend, glide y **unísono** (varios modos, incluidos acordes y Shepard).
- **Carriles de efectos**: 3 carriles con hasta 8 efectos cada uno, con Poly, Mute, Solo y envío a otro carril o al Master.

## Presets

Un preset guarda el sonido de un instrumento para usarlo en otros proyectos. Se manejan desde el menú **Presets** de la barra de cada instrumento y de los editores grandes.

### Guardar tu sonido

1. Abre **Presets**.
2. En **MIS PRESETS**, escribe un nombre.
3. Pulsa **Guardar** o {Intro}. Con un nombre que ya existe, se sobrescribe.

**×** junto a un preset lo borra.

### Usar un preset

- **DE FÁBRICA**: los sonidos que trae Pulse, en todos sus instrumentos menos el Sampler y los Drum Pads (que tocan tus muestras). Van por categorías (Bajo, Lead, Pad, Teclas…, o por estilo en las baterías): pasa el ratón por una para ver sus sonidos. Los instrumentos con pocos presets los muestran en una sola lista.
- **MIS PRESETS**: los tuyos.
- En el navegador (**BIBLIOTECA › Presets**), doble clic en un preset lo aplica a la pista MIDI seleccionada.

Se guardan en la carpeta `Presets` de Pulse (ver [[Proyectos y carpeta de Pulse]]).

> Atención: los presets que usan muestras guardan dónde están. Si mueves las muestras, el preset las pierde.

> Nota: los plugins no usan este menú: guardan su estado con el proyecto.

## Plugins VST3 y CLAP

Pulse carga instrumentos y efectos de otros fabricantes en formato VST3 y CLAP, los estándares de plugins de audio.

### Buscar tus plugins

1. En el navegador, abre la sección **PLUGINS (VST3 · CLAP)**.
2. Pulsa **Buscar plugins VST3**.

Busca en las carpetas de plugins del sistema y en las que añadas en **Ajustes › Plugins** (ver [[Ajustes]]).

> Nota: cada plugin se carga para la búsqueda en un proceso aparte. Si uno se cuelga o se cierra al cargarse, Pulse no se cierra: dice cuál ha sido y lo deja fuera de las siguientes búsquedas hasta que lo actualices o lo reinstales.

### Usar un plugin

- Clic en un **instrumento** lo pone en la pista MIDI seleccionada.
- Clic en un **efecto** lo añade al final de la cadena.

En el dispositivo del plugin:

- **Editor** abre la ventana propia del plugin, que flota sobre Pulse.
- **Parámetros…** abre todos sus parámetros en una lista, con buscador.
- 6 knobs muestran sus parámetros principales, primero los automatizados.

Todos sus parámetros se pueden automatizar (ver [[Automatización]]) y asignar a un controlador MIDI (ver [[Controladores MIDI]]). Su estado se guarda con el proyecto.

Los plugins saben el tempo, en qué compás está la canción y si suena o está parada. Sus delays, LFOs y secuenciadores sincronizados siguen la canción también tras saltar, en bucle o al exportar una sección.

Si un plugin ya no está instalado, su dispositivo lo avisa y no suena.

Una pista o un retorno con un plugin se procesa siempre en el núcleo del audio, no en los otros (ver [[Ajustes]]): algunos plugins solo aceptan que se les llame desde ahí.

### Plugins de otras carpetas

Un plugin es código que se ejecuta dentro de Pulse. Por eso un proyecto solo carga plugins de tus carpetas de plugins: las del sistema y las de **Ajustes › Plugins**.

Si un proyecto (por ejemplo, de otra persona) apunta a un plugin de otra carpeta, Pulse no lo carga y lo avisa. Si te fías de él, añade su carpeta en **Ajustes › Plugins** y se carga al momento.

+++ Carpetas del sistema
| Sistema | Carpetas |
|---|---|
| macOS | `/Library/Audio/Plug-Ins/VST3` y `CLAP`, y las de tu usuario en `~/Library` |
| Windows | `C:\Program Files\Common Files\VST3` y `CLAP`, y las de tu usuario en `%LOCALAPPDATA%\Programs\Common` |

Pulse para Windows es de 64 bits: los plugins de 32 bits no se cargan.
+++

# Mezclar

## Mezclador

El mezclador ajusta cuánto suena cada pista, dónde suena entre los altavoces y cuánto manda a la reverb y al delay. Está debajo de cada columna de [[Session]].

### Los controles de cada pista

De arriba abajo:

- **In / Auto / Off** (solo en pistas de audio): si oyes lo que entra por la tarjeta de sonido. Ver [[Grabar]].
- **A**, **B**…: los **envíos** a los retornos, uno por retorno y de dos en dos. Cuanto más alto, más sonido de esa pista va a la reverb, el delay o lo que tenga el retorno.
- **Pan**: el panorama, a la izquierda o a la derecha.
- **Fader** de volumen, con su medidor. Doble clic lo deja en 0 dB; con {⇧} pulsado se mueve más fino.
- **Número de pista**: es el activador. Apagado, la pista no suena.
- **S** (solo): solo suena esta pista (y las demás que estén en solo).
- **●**: arma la pista para grabar.

> Consejo: los retornos y el Master también tienen su cadena de efectos. Haz clic en su cabecera y mira la pestaña **Dispositivos** ([[Dispositivos y efectos]]).

### Usar los retornos

Un **retorno** es una pista de efectos compartida: varias pistas le mandan una parte de su sonido (con los envíos A, B…) y suenan con la misma reverb o el mismo delay. Un proyecto nuevo trae dos, **A Reverb** y **B Delay**.

1. Sube el envío **A** de una pista para darle sala, o el **B** para darle eco.
2. Para un retorno nuevo, usa **Crear › Retorno** ({⌥⌘T}) o el **+** a la derecha de los retornos. Aparece vacío, como «C Retorno».
3. Ponle efectos en **Dispositivos** y súbele los envíos de las pistas que quieras.

Para quitar uno: clic derecho en su cabecera › **Eliminar retorno**.

> Atención: al eliminar un retorno, las pistas dejan de enviarle sonido y se pierden su automatización de envío, sus envolventes de clip y los controles MIDI y macros que lo movían.

+++ Detalles
- Un proyecto puede tener hasta 12 retornos (de la A a la L), o ninguno.
- Al eliminar uno, los retornos de después suben una letra, y los nombres que empiezan por su letra la cambian («C Delay» pasa a «B Delay»).
- Un proyecto con dos retornos se sigue abriendo en versiones anteriores de Pulse. Con más o con menos, las anteriores piden actualizar.
+++

### Mover los knobs

- Arrastra arriba o abajo para girarlo; con {⇧} pulsado, el ajuste es fino.
- Doble clic lo devuelve a su valor por defecto.
- Un punto rojo indica que está automatizado. Si el punto está hueco, lo has movido a mano y la automatización está anulada (ver [[Automatización]]).

## Dispositivos y efectos

La pestaña **Dispositivos** ({⇧Tab} en el panel de abajo) muestra la cadena de efectos de la pista, el retorno o el Master seleccionado. En una pista MIDI va primero el instrumento y después los efectos, de izquierda a derecha.

### Añadir y ordenar efectos

1. Selecciona la pista, el retorno o el Master.
2. En el navegador, abre **EFECTOS DE AUDIO** y haz clic en un efecto: se pone al final de la cadena.
3. En cada efecto, el **punto** lo enciende o lo apaga, **◄ ►** lo mueven y **×** lo quita.

Si haces clic en un instrumento del navegador, sustituye al de la pista MIDI seleccionada (ver [[Instrumentos]]).

### Qué hace cada efecto

| Efecto | Para qué sirve |
|---|---|
| **EQ Three** | Subir o bajar graves, medios y agudos, rápido. |
| **EQ Eight** | Ecualizar con precisión: quitar graves que sobran, una frecuencia que pita, dar brillo. |
| **Compressor** | Igualar lo fuerte y lo suave para que la pista no se pierda ni salte. |
| **Compresor de bus** | Pegar una batería, un grupo o la mezcla entera (ver [[Compresor de bus]]). |
| **Multibanda (OTT)** | Aplastar y abrillantar a lo OTT: sube lo flojo y baja lo fuerte en tres bandas (ver [[Multibanda (OTT)]]). |
| **Limiter** | Subir el volumen sin pasar de un techo; para el master. |
| **Gate** | Silenciar el ruido de fondo entre golpes o frases. |
| **Transient** | Dar más o menos pegada y más o menos cola a cada golpe. |
| **Tuner** | Afinar una voz o un instrumento a la escala (como Auto-Tune). |
| **De-esser** | Bajar las eses y silbidos de una voz. |
| **Saturator** | Calentar, ensuciar o romper el sonido. |
| **Bitcrusher** | Sonido digital, de pocos bits. |
| **Auto Filter** | Filtro con barrido al ritmo; los barridos ácidos. |
| **Ducker** | Que una pista se aparte o «bombee» con el bombo. |
| **Chorus** y **Phaser** | Movimiento y anchura. |
| **Delay** | Eco a tempo. |
| **Reverb** | Sala. |
| **Espacio** | Reverb de convolución: salas, placa y muelle de verdad, sin sonido metálico. |
| **Utility** | Volumen, anchura estéreo, graves en mono y arreglar fase. |

#### EQ Eight

Un ecualizador de ocho bandas, como el EQ Eight de Live. La gráfica muestra la curva (naranja) sobre el **espectro** de lo que sale del EQ (gris), en vivo: ves dónde están los graves que sobran o la frecuencia que pita.

1. Arrastra el **punto numerado** de una banda: a izquierda y derecha cambias la frecuencia; arriba y abajo, la ganancia. Con {⌥} pulsado, arriba y abajo cambian la **Q** (lo estrecha que es).
2. Doble clic en un punto enciende o apaga la banda. Las apagadas se ven huecas.
3. Los botones **1–8** bajo la gráfica eligen qué banda muestran los controles de la derecha: su **forma** y sus ruedas **Freq**, **Gain** y **Q**. **Output** sube o baja todo al final.

Las formas:

- **Corte graves** y **Corte agudos**: quitan lo que hay por debajo o por encima de la frecuencia.
- **Estante graves** y **Estante agudos**: suben o bajan todo lo que hay por debajo o por encima.
- **Campana**: sube o baja alrededor de la frecuencia.
- **Notch**: quita una frecuencia muy concreta (un zumbido de 50 Hz, un pitido).

> Consejo: un EQ Eight nuevo no cambia el sonido. Enciende la banda 1 (un corte de graves a 40 Hz) para quitar los graves que no se oyen pero ocupan.

+++ Detalles
- Los cortes son de 48 dB por octava (casi de golpe) o de 12 (suave). Una Q por encima de 0,71 resalta el corte.
- En los estantes, una Q por encima de 0,71 marca el borde con un pequeño realce, de unos 3 dB como mucho.
- La gráfica va de −15 a +15 dB. Si la curva sube más (un corte con mucha Q, varias campanas juntas), arriba se escribe hasta dónde llega, por ejemplo «+25 dB».
- Uno nuevo trae las bandas 2–7 encendidas y planas, y los cortes (1 y 8) apagados. Las bandas a 0 dB no gastan nada.
- Las 25 ruedas se pueden automatizar y asignar a MIDI como las de cualquier efecto, y los puntos siguen a la automatización.
- El espectro se dibuja para el primer EQ Eight de la cadena que estás viendo, y empieza de cero al cambiar de pista. Escucha los dos canales: lo que solo suena en estéreo también se ve. Un tono a todo volumen marca 0 dB, y los graves se ven igual de definidos a 96 o 192 kHz.
- No está entre los efectos internos del Wavetable, el Polychrome ni las líneas del Modular, que llevan seis controles por efecto como mucho.
- Un proyecto con un EQ Eight no se abre en un Pulse anterior: pide actualizar.
+++

#### Compressor y Limiter

El **Compressor** baja lo que pasa del umbral, para que las partes fuertes y las suaves queden más juntas. Controles: **Threshold** (umbral), **Ratio**, **Attack**, **Release** y **Makeup** (ganancia para recuperar volumen), y **Sidechain** (ver abajo).

El **Limiter** sube el volumen con **Gain** sin dejar que nada pase del techo (**Ceiling**). **Release** es lo que tarda en soltar.

Los dos tienen a la derecha un medidor **GR**: cuántos dB bajan el volumen en cada momento.

+++ Todos los parámetros
- Compressor: Threshold −60 a 0 dB (−18), Ratio 1:1 a 20:1 (4:1), Attack 0,1–200 ms (10), Release 5–2000 ms (120), Makeup 0–24 dB.
- Limiter: Gain 0–24 dB, Ceiling −12 a 0 dB (−0,3), Release 10–1000 ms (120).
+++

#### Gate

Una **puerta de ruido**: deja pasar el sonido solo cuando supera el umbral, y lo cierra cuando baja. Quita el ruido de fondo entre frase y frase de una voz, el zumbido de un amplificador o la cola de una caja.

- **Threshold**: el nivel a partir del cual se abre.
- **Return**: cuánto por debajo del umbral tiene que bajar para cerrarse. Así no se abre y se cierra sin parar con un sonido que ronda el umbral.
- **Attack**: lo que tarda en abrirse del todo (muy corto: no se pierde el golpe).
- **Hold**: cuánto se queda abierta después de que el sonido baje.
- **Release**: lo que tarda en cerrarse del todo.
- **Floor**: lo que deja pasar cerrada (−80 dB: nada; −20 dB: baja el fondo sin quitarlo).

> Consejo: con **Sidechain**, se abre cuando suena otra pista, no la suya: un pad que corta al ritmo de los charles (el «trance gate»), o un bajo que solo suena cuando suena el bombo.

#### Transient

Cambia el golpe y la cola de cada sonido, sin umbral, sea cual sea su volumen. Un sonido sostenido, sin golpes, pasa igual.

- **Attack**: por encima de 0 %, el principio de cada golpe suena más fuerte (una batería con más pegada); por debajo, más suave (redondea una caja o una guitarra muy picada).
- **Sustain**: por encima de 0 %, lo que sigue al golpe dura más (más sala, más cuerpo); por debajo, se acorta (una batería más seca y apretada).
- **Output**: el volumen al final.

#### Tuner

Un afinador de voz, como Auto-Tune. Lleva cada nota a la nota más cercana de la escala, en vivo o en lo que ya está grabado. Funciona con una voz o con cualquier instrumento que toque una nota a la vez (un bajo, un violín, un silbido). Ponlo en una pista de audio.

- **Escala**: **Del proyecto** (la de [[Tonalidad y escala]]), **Cromática** (la nota más cercana, sea cual sea) o una escala concreta (Do mayor, La menor…).
- **Speed**: lo que tarda en llegar a la nota. A 0 ms va al instante y cada cambio de nota es un salto: el efecto robótico del pop y el trap. Con 20–50 ms afina sin que se note; con 100–400 ms, solo corrige lo que se aleja mucho.
- **Humanize**: cuanto más alto, más libre deja una nota que mantienes (su vibrato se queda), mientras las notas cortas siguen afinadas.
- **Amount**: cuánto del camino recorre: al 100 % llega a la nota; al 50 %, se queda a medias.

+++ Cómo funciona
- Con **Del proyecto**, si el proyecto no tiene escala, usa todas las notas, y cambia en cuanto le pongas una.
- Una nota que no es de la escala va a la de la escala más cercana: un Fa# desafinado en Do mayor suena como Sol.
- Sigue de un Do2 (65 Hz) a un Do#6 (1100 Hz). Lo que no tiene tono (respiraciones, eses, silencio) pasa sin cambios.
- Suena un poco tarde, algo más de un periodo de la nota: unos 5 ms con una voz media (220 Hz), 10 con una grave (110 Hz) y hasta 17 con las notas más bajas de un bajo. Las demás pistas lo esperan esos 5 ms (ver la compensación de latencia en [[Grabar]]).
- No está entre los efectos internos de los sintes, que ya tocan afinado.
- Un proyecto con un Tuner no se abre en un Pulse anterior: pide actualizar.
+++

#### De-esser

Baja las eses y los silbidos de una voz («s», «ch», «z») cuando saltan, sin apagar el resto.

- **Frequency**: desde dónde escucha y baja (2–16 kHz). Las eses de una voz grave suelen estar en 4–6 kHz; las de una aguda, en 6–9 kHz.
- **Threshold**: a partir de qué nivel de esa parte aguda empieza a bajarla.
- **Range**: lo más que la baja (hasta 24 dB). Unos 6–8 dB bastan; más, y la voz suena como si ceceara.

> Consejo: ponlo después del compresor, porque comprimir sube las eses.

+++ Cómo funciona
- Separa la voz en lo que suena por debajo y por encima de Frequency, escucha solo la parte aguda y la baja mientras pase del umbral.
- Mientras no actúa, no cambia nada del sonido, y la voz por debajo de Frequency nunca se toca.
- Un proyecto con un De-esser no se abre en un Pulse anterior: pide actualizar.
+++

#### Saturator y Bitcrusher

El **Saturator** tiene **Drive** (cuánto empuja), **Output** y cinco **tipos**:

- **Soft**: el de siempre, suave.
- **Hard**: recorte duro.
- **Fold**: plegado, brillante.
- **Tube**: asimétrico y cálido, como una válvula.
- **Crush**: reducción de bits.

El botón **4×** hace que mucho drive no ensucie los agudos.

El **Bitcrusher** reduce los **bits** y la frecuencia de muestreo (**Downsample**): el sonido de las máquinas digitales antiguas.

+++ Detalles
- **4×** trabaja a cuatro veces la frecuencia de muestreo para evitar el aliasing (los agudos falsos que aparecen al saturar mucho). Gasta algo más de CPU, menos de un 1 % de un núcleo por pista, y retrasa 36 muestras.
- Saturator: Drive 0–36 dB (6), Output −36 a +6 dB (−3), Dry/Wet.
- Bitcrusher: Bits 2–16 (8), Downsample 1–40× (4), Dry/Wet.
+++

#### Auto Filter

Un filtro paso bajo, paso alto o paso banda, con un LFO sincronizado al tempo que mueve el corte solo.

- **24 dB** corta el doble por octava.
- **Drive** satura lo que entra al filtro.
- Con **Res** por encima del 95 %, el filtro suena solo (auto-oscila) a su frecuencia: el silbido de los barridos ácidos. Su nivel se queda acotado aunque muevas el corte hasta arriba o lo automatices a saltos.

#### Ducker

Hace que la pista **bombee**: baja el volumen y vuelve, al ritmo.

- Sin sidechain, va sincronizado al tempo (**Ritmo**).
- Con **Sidechain**, bombea con cada golpe de la pista elegida (normalmente el bombo): baja de golpe y vuelve. **Vuelve en** (el menú, 1/4 por defecto) es el tiempo de esa vuelta; el knob **Release** es qué parte de ese tiempo tarda en subir (100 %: todo).
- **Threshold**: el golpe más flojo que lo hace bombear. Súbelo si bombea con cosas que no son el bombo.
- **Solo graves** (activado por defecto): solo escucha por debajo de 600 Hz, así que bombea con el bombo y no con los charles o las palmas de la misma pista.

+++ Detalles
- Vuelve en Release aunque la cola del bombo siga sonando, así que con bombos largos de 140–150 BPM también respira entre golpe y golpe.
- Threshold: bombea del todo a partir de 12 dB por encima del umbral.
- Quita **Solo graves** con un bombo de hardstyle, cuya cola suena tan fuerte en graves como el golpe.
- Con la Drum Machine entera como sidechain, los toms graves y las congas también lo mueven un poco. Si molesta, saca el bombo a su pista (**Sacar a su pista ▸ Bombo**) y escucha esa.
+++

#### Chorus, Phaser, Delay y Reverb

- **Chorus**: duplica el sonido con pequeñas variaciones; lo ensancha y lo mueve.
- **Phaser**: un barrido que da vueltas, como un remolino.
- **Delay**: eco con tiempo sincronizado (de 1/16 a un compás), **Ping Pong** (rebota de un lado a otro), feedback (cuántas repeticiones) y filtro.
- **Reverb**: sala, con tamaño, amortiguación y anchura.

+++ Todos los parámetros
- Chorus: Rate 0,05–5 Hz, Depth, Delay 2–20 ms, Dry/Wet.
- Phaser: Rate 0,05–8 Hz, Depth, Freq 100–4000 Hz, Feedback, Dry/Wet.
- Delay: Feedback (hasta 95 %), Filter 500–18 000 Hz, Dry/Wet.
- Reverb: Size, Damping, Width, Dry/Wet.
+++

#### Espacio (reverb de convolución)

Una reverb que suena a un sitio de verdad: Pulse calcula la respuesta de cada espacio (cómo rebota un golpe en sus paredes y cómo se apaga), y el sonido pasa por ella. Es más natural y más suave que la **Reverb**, que puede sonar metálica en colas largas. Gasta poco: alrededor de un 1 % de un núcleo, aunque la cola dure 8 segundos, y nada mientras no le llega sonido. No retrasa nada.

1. Elige el **Espacio**:
   - **Ambiente**: solo las primeras reflexiones; aire y cuerpo sin cola.
   - **Habitación** y **Estudio**: cerca y natural, para batería y guitarras.
   - **Sala** y **Sala grande**: la reverb larga y redonda.
   - **Catedral**: muy larga, y los graves duran más.
   - **Placa**: densa desde el primer instante y brillante; la de las voces y la caja.
   - **Muelle**: el «boing» del dub y el surf.
   - **Infinito**: una cola de muchos segundos que entra despacio, para pads y ambient.
2. Ajusta **Decay** (lo que dura la cola, en segundos) y **Size** (más grande, las primeras reflexiones llegan más tarde).
3. Con **Pre-delay** separas la voz de su reverb; con **Low Cut** y **High Cut** le quitas graves que embarran y agudos que chillan.

El dibujo muestra las primeras reflexiones (rayas) y cómo cae la cola en graves (azul), medios (naranja) y agudos (amarillo): en un espacio real, los agudos se apagan antes.

**Presets** trae ajustes por uso: **Voces · Placa brillante**, **Batería · Habitación**, **Pads · Catedral**, **Ambiente · Infinito**, y los **Retorno · …**, con todo el sonido mojado, para un retorno.

- En un retorno, deja **Dry/Wet** al 100 % y manda más o menos desde cada pista.
- Al cambiar el espacio, **Decay** o **Size**, Pulse calcula el espacio nuevo y pasa a él sin cortes en menos de medio segundo. Por eso esos tres no se automatizan; el resto sí.
- Entra en mono y sale en estéreo: una pista a la izquierda tiene su reverb repartida por los dos lados, como en una sala.

+++ Todos los parámetros
- Espacio: Ambiente, Habitación, Estudio, Sala, Sala grande, Catedral, Placa, Muelle, Infinito.
- Decay: del 25 % al 200 % de lo que dura el espacio (hasta 10 s de cola).
- Size: 50–150 %.
- Pre-delay 0–250 ms, Low Cut 20–2000 Hz (20 Hz: apagado), High Cut 1000–20 000 Hz (20 000 Hz: apagado), Width, Dry/Wet.
+++

> Nota: un proyecto que use Espacio no se abre en un Pulse anterior: pide actualizar.

#### Utility

Volumen (**Gain**) y anchura estéreo (**Width**: 0 % es mono, 200 % muy abierto), y dos arreglos:

- **Bass Mono**: por debajo de esa frecuencia, el sonido va en mono (0 Hz: apagado). Los graves en estéreo, en un club o un móvil, se pierden o retumban distinto; con 100–150 Hz se asientan sin cambiar lo de arriba.
- **Invertir R**: da la vuelta al canal derecho. Arregla una pista grabada con un canal al revés, que en mono casi desaparece.

> Nota: un proyecto que use Bass Mono o Invertir R no se abre en un Pulse anterior: pide actualizar.

### Efectos MIDI

Cambian las notas antes de que lleguen al instrumento, en pistas MIDI.

1. Selecciona la pista MIDI.
2. En el navegador, abre **EFECTOS MIDI** y haz clic en uno. Aparece en Dispositivos, antes del instrumento.
3. Como los de audio: el punto los activa, **◄ ►** los mueven y **×** los quita. Hasta 4 por pista, en orden.

- **Arpegiador**: toca las notas que mantienes una a una, a tempo.
  - **Orden**: arriba, abajo, arriba y abajo, como se tocan o al azar. **Ritmo**: de 1/4 a 1/32, con tresillos. **Octavas** (1–4) y **Duración** de cada nota.
- **Acorde**: cada nota se convierte en un acorde.
  - **En la escala**: le añade su tercera y su quinta (y **séptima**) de la escala, así que los acordes encajan con la canción (en do mayor, un re da re–fa–la, un sol da sol–si–re). La escala puede ser la del proyecto o una propia.
  - Sin «En la escala»: los **semitonos añadidos**, con atajos (Mayor, Menor, Quinta, Octava, Sus4, Séptima). **Velocidad añadidas**: lo fuerte de las notas nuevas.
- **Escala**: lleva las notas que se salen de la escala a la más cercana. Con ella no hay nota que desafine. Usa la del proyecto o una propia.

> Consejo: se combinan. **Acorde + Arpegiador** arpegia acordes tocando una sola nota.

+++ Detalles
- El Arpegiador, con el transporte en marcha, va con la rejilla; parado, empieza al tocar y sigue el tempo.
- **Al azar**, con el transporte en marcha, cada paso de la rejilla tiene siempre su nota: la canción suena igual cada vez que la tocas y al exportarla.
- Funcionan con el teclado y con los clips.
- Al grabar se graban las notas que tocas, antes de los efectos.
+++

### Sidechain: que una pista se aparte de otra

El **Compressor**, el **Compresor de bus**, el **Gate** y el **Ducker** pueden escuchar a otra pista en vez de a la suya. El caso típico: el bajo o el pad se apartan cada vez que suena el bombo, para que no se tapen (y ese «bombeo» del house).

1. Pon un Compressor, un Compresor de bus, un Gate o un Ducker en la pista que debe apartarse (el bajo).
2. En su menú **Sidechain**, elige la pista que manda (el bombo).

- Escucha el sonido de la pista elegida **tras sus efectos y antes de su fader**: funciona aunque esa pista esté silenciada o con el fader abajo. Puedes tener un bombo solo para disparar el sidechain.
- Puede escuchar a un **grupo**: oye la suma de sus pistas tras los efectos del grupo.

+++ Detalles
- Sigue funcionando con otra pista en solo, en los stems y al congelar.
- La fuente se recuerda aunque muevas, agrupes o renombres las pistas.
- Vale también en pistas de grupos, retornos y el Master.
+++

## Compresor de bus

Un compresor para pegar: hace que una batería, un grupo de sintes o la mezcla entera suenen como una sola cosa, como el compresor de bus de una mesa SSL (el Glue Compressor de Live). Ponlo en el **grupo** de la batería o en el **Master**.

### Usarlo en el bus de batería

1. Agrupa la batería (ver [[Pistas#Agrupar pistas]]) y selecciona el grupo.
2. En el navegador, **EFECTOS DE AUDIO** → **Compresor de bus**. O abre **Presets** en su barra y elige **Bus de batería**.
3. Baja **Threshold** hasta que el medidor **GR** marque 2–4 dB en los golpes.
4. Sube **Makeup** lo que ha bajado, para comparar al mismo volumen encendiéndolo y apagándolo.

> Consejo: con **Attack** a 10–30 ms el principio de cada golpe pasa antes de que comprima: más pegada. Con 0,1–1 ms lo aplasta todo.

### Los controles

- **Ratio**: **2:1** pega sin notarse (master, grupos), **4:1** es el de siempre en batería y **10:1** casi limita (para el paralelo).
- **Attack** (0,01–30 ms) y **Release** (0,1–1,2 s): lo que tarda en bajar y en soltar.
- **Auto**: release automático. Suelta rápido tras un golpe suelto y despacio cuando lleva rato comprimiendo, así no bombea. Con él encendido, el knob **Release** no hace nada.
- **Makeup**: recupera el volumen que quita.
- **Knee**: lo suave que entra. A 0 dB comprime de golpe al pasar el umbral; con más, va entrando poco a poco.
- **SC Filter**: el compresor deja de oír los graves por debajo de esta frecuencia. Con 80–150 Hz el bombo ya no hace bajar toda la batería en cada golpe. Abajo del todo (20 Hz) está apagado.
- **Dry/Wet**: mezcla el sonido sin comprimir. Con un 30–50 % y mucha compresión tienes **compresión en paralelo**: el cuerpo aplastado y los golpes intactos.
- **Clip**: recorta suave los picos a la salida, por debajo de 0 dBFS.
- **Sidechain**: que escuche a otra pista (ver [[Dispositivos y efectos#Sidechain: que una pista se aparte de otra]]).

### Presets

| Preset | Para qué |
|---|---|
| **Bus de batería** | 4:1, ataque de 10 ms, release Auto y filtro a 90 Hz: la batería junta sin perder el bombo. |
| **Paralelo duro** | 10:1 muy rápido y aplastado, al 40 %: cuerpo y sala sin tocar los golpes. |
| **Pegada (ataque lento)** | 30 ms de ataque: los golpes salen hacia delante. |
| **Pegar la mezcla** | 2:1 suave en el master, 1–2 dB de GR. |
| **Master con clip** | Como el anterior y con **Clip**: un poco más de volumen. |
| **Sintes juntos** y **Voces** | Para grupos de sintes o de voces. |

+++ Detalles
- Detecta los picos de los dos canales juntos (no mueve la imagen estéreo) y no retrasa el sonido.
- Las ruedas se pueden automatizar y asignar a MIDI; Ratio, Auto y Clip se eligen en el dispositivo.
- Threshold −40 a 0 dB (−12), Attack 0,01–30 ms (10), Release 0,1–1,2 s (0,4), Makeup 0–20 dB, Knee 0–12 dB (3), SC Filter 20–1000 Hz (apagado), Dry/Wet 0–100 % (100).
- Un proyecto con un Compresor de bus no se abre en un Pulse anterior: pide actualizar.
+++

## Multibanda (OTT)

El sonido «OTT» de la EDM: parte el sonido en graves, medios y agudos y, en cada banda, **baja lo fuerte y sube lo flojo**. Todo queda delante, denso y brillante: pads que respiran, leads que cortan, colas y ruiditos que salen a la luz. Es el preset OTT de Live y el plugin OTT de Xfer.

### Usarlo

1. Ponlo en la pista (un pad, un lead, un bajo de future bass) o en un retorno.
2. Empieza con **Depth** al 20–40 %: al 100 % es exagerado a propósito.
3. Mueve **Time**: más corto es más agresivo y bombea; más largo, más suave.
4. Ajusta el volumen con **Output** para comparar encendido y apagado.

> Consejo: en un pad, baja **Low Out** y sube **High Out** un poco: brillo sin más barro.

> Atención: sube lo flojo hasta 18 dB más sus ganancias. Las colas, el ruido y las reverbs se oyen mucho más; si sobra, baja **Upward**.

### Los controles

- **Depth**: cuánto hace. A 0 % no cambia nada; al 100 %, todo.
- **Time**: multiplica los tiempos de ataque y release de las tres bandas (×0,1 a ×10).
- **Downward** y **Upward**: cuánto baja lo fuerte y cuánto sube lo flojo, por separado.
- **Input** y **Output**: volumen a la entrada (empuja más hacia la compresión) y a la salida.
- **Dry/Wet**: mezcla el sonido sin procesar, en fase con el procesado.
- **Low/Mid** y **Mid/High**: dónde se cortan las bandas (88 Hz y 2,5 kHz al empezar).
- **Low In**, **Mid In**, **High In**: cuánto entra en cada banda; más es más compresión en esa banda.
- **Low Out**, **Mid Out**, **High Out**: el volumen de cada banda a la salida.

A la derecha, los medidores **L**, **M** y **H** muestran qué hace cada banda: hacia arriba (verde) la está subiendo, hacia abajo (naranja) la está bajando.

### Presets

| Preset | Para qué |
|---|---|
| **OTT clásico** | Todo al 100 %: el sonido OTT entero. |
| **OTT suave** | Al 30 %: más denso sin que se note. |
| **Solo hacia abajo** | Sin upward: aplasta lo fuerte sin subir el ruido. |
| **Brillo en pads** | Menos graves y más agudos, más lento: pads abiertos. |
| **Lead que corta** | Rápido, con los medios y agudos delante. |
| **Bajo gordo** | Corte a 120 Hz y agudos abajo: un bajo denso que no silba. |
| **Batería aplastada** | En paralelo, al 60 %. |
| **Voz de cerca** | Muy poco, para una voz delante. |

+++ Detalles
- Las bandas se cortan con filtros Linkwitz-Riley de 24 dB por octava; sin compresión suman plano (las tres juntas suenan como la entrada). No retrasa el sonido.
- Cada banda escucha su nivel RMS. Lo que suena más flojo de −90 dBFS no se sube: el silencio sigue en silencio.
- Las 15 ruedas se pueden automatizar y asignar a MIDI.
- Un proyecto con un Multibanda no se abre en un Pulse anterior: pide actualizar.
+++

## Asistente de mezcla y master

El asistente escucha tu canción, te dice cómo suena y qué arreglar antes de publicarla, y lo arregla con un botón. Todo lo que hace son volúmenes, panoramas y efectos normales que puedes retocar o quitar.

### Usarlo

1. Abre **Archivo › Asistente de mezcla y master…**.
2. Elige el **Destino**: dónde se va a escuchar la canción (Spotify, club…).
3. Pulsa **Analizar la mezcla**.
4. Lee los **Consejos** y pulsa **Aplicar** en los que quieras, o **Aplicar todo** para que lo haga en orden.

Tras cada **Aplicar** vuelve a analizar solo, para que veas el resultado. Si el informe no cabe en la pantalla, la ventana se desplaza.

> Consejo: todo lo que hace el asistente se deshace con {⌘Z}, paso a paso.

### Lo que mide

Al analizar, Pulse toca la canción (o el loop) sin sonido y más rápido que en tiempo real, y mide la mezcla entera y cada pista sola:

- **Sonoridad** (LUFS): cómo de fuerte se percibe la canción. Es la medida que usan Spotify, YouTube o Apple Music para igualar el volumen de todas las canciones.
- **Pico real** (dBTP): el punto más alto de la señal, también entre muestras. Ahí nace la distorsión al pasar a MP3 o AAC.
- **Rango** (LU): cuánto cambia de volumen entre partes.
- **Lo más fuerte**: los 3 segundos más fuertes.
- **Estéreo** (correlación): 1 es mono; por debajo de 0, parte de la mezcla se pierde al escucharla en mono.

En **Pistas** ves el pico, la sonoridad y el estéreo de cada una.

+++ Cómo lo mide
- Mide la mezcla entera y cada pista sola a la vez, una por núcleo del procesador.
- Las pistas se miden en seco, sin lo que mandan a los retornos: la cola de la reverb o del delay no cuenta como parte suave de la voz.
- Las pistas se miden antes del master, sin su limitador ni su fader: un limitador sobre cada pista sola subiría las flojas y descolocaría el balance.
- La mezcla sí se mide tras el master, y cada parte de la canción también.
- Al volver a analizar, solo vuelve a tocar las pistas que han cambiado. Si de una solo ha cambiado el fader, ni eso: sus medidas se mueven esos dB.
+++

### Elegir el destino

| Destino | Sonoridad |
|---|---|
| Spotify / YouTube / Tidal | −14 LUFS |
| Apple Music | −16 LUFS |
| Club / DJ / SoundCloud | −9 LUFS |
| **Hard techno / rave** | −7 LUFS |
| Radio y TV | −23 LUFS |
| **Como la referencia** | la de tu canción de referencia |

En todos, el pico real queda por debajo de −1 dBTP.

### Compararte con una referencia

En **Referencia**, **Elegir canción…** carga una canción que suene como quieres. Pulse compara su sonoridad y su **balance de graves, medios y agudos** con el de tu mezcla. En la gráfica por octavas, las barras son tu mezcla y la línea, la referencia. **Aplicar** pone un **EQ Three** en el master que acerca tu balance al suyo.

### Los consejos

Cada consejo dice qué pasa y, si se puede arreglar, trae un botón **Aplicar**. Cuando algo ya está bien, sale en verde.

Varios consejos dependen del **papel** de cada pista (voz, bajo, bombo, pad…). Pulse lo saca de su nombre («Kick», «Bajo», «Voz», «Coros», «Pad», «Hats»…, en español o en inglés), de su instrumento (el Kick Designer es el bombo, el Acid el bajo, una Drum Machine la batería) o, si no, de cómo suena (casi todo graves: un bajo; casi todo agudos: percusión). Las pistas sin papel claro («otra») no se tocan.

> Consejo: ponles a las pistas nombres claros («Voz», «Bajo», «Kick») y el asistente acertará más.

#### Sonoridad

La canción suena más floja o más fuerte de lo que pide el destino. **Aplicar** pone un **Limiter** en el master y le busca la ganancia. Sube hasta la sonoridad del destino, pero sin aplastar la canción: si tuviera que apretarla demasiado, se para antes y te dice qué hacer (subir la mezcla antes del master, comprimir el bus o saturar la batería y el bajo). Entonces el consejo ya no trae botón: más limitador no es la solución.

+++ Cómo lo calcula
- Toca la mezcla una vez hasta el limitador y prueba varias ganancias a la vez sobre esa grabación, midiendo en cada una la sonoridad, el pico real y cada parte de la canción.
- Como mucho, el limitador le quita **6 dB a los picos**. Si la mezcla llega con margen (sus picos por debajo del techo), ese margen se sube además, porque no aprieta nada.
- Las **partes** siguen como estaban: el drop o el estribillo por encima del break, de la intro y de la subida. Cada parte puede acercarse al drop un tercio de lo que estaba por debajo (de 0,5 a 2,5 LU), y la subida se queda al menos 1,15 LU por debajo. Sin marcadores, mira el rango de sonoridad: puede perder un tercio.
- El Limiter mira el **pico real**, también entre muestras, y baja la ganancia de forma suave durante 2 ms antes de cada pico, sin escalones. El techo queda en −1,3 dB y el pico real de la mezcla, por debajo de −1 dBTP.
- Para destinos de −9 LUFS o más fuertes (club, hard techno), pone además un **Saturator Hard 4×** justo antes del Limiter, que recorta 3 dB de los picos (los del bombo), y le acorta el release a 40 ms. Es como se masteriza el género: el limitador solo, con un bombo cada 400 ms, bombea en vez de subir el volumen y deja pasar picos reales.
- Si la mezcla está por encima del destino: en uno de streaming te avisa de que la plataforma la bajará; en uno de club, de que más apretada solo pierde pegada.
+++

#### Mezcla de partida

Pone cada pista donde suele empezar una mezcla (volumen, panorama y graves), para que sigas a oído desde ahí. El consejo dice qué cambiaría en cada pista y **Aplicar** lo hace todo de una vez.

+++ Cómo lo calcula
- **Volumen**: cada papel, a su nivel habitual junto al bombo (o la batería, o la voz si no hay): la voz igual de fuerte, el bajo y la batería 2 dB por debajo, la solista 3, los acordes 5, los coros 7, pads y percusión 8, efectos 10. Una batería sin bombo (lo has sacado a su pista: quedan charles, palmas…) va 6 dB por debajo de él. Si hay varias pistas del mismo papel, se reparten ese nivel. No sube ninguna por encima de −1 dBFS ni más allá del tope de su fader. Solo cambia lo que se aleje más de 1,5 dB.
- **Panorama**: el bombo, el bajo y la voz, al centro. Si hay dos o más pistas de acordes, pads, coros, percusión o solistas en el centro, las abre a izquierda y derecha. Lo que ya has panoramizado tú no lo toca.
- **Graves**: a lo que no los necesita (voz, coros, acordes, pads, solistas, percusión, efectos) y aun así tiene algo por debajo de unos 60 Hz, le pone un **EQ Eight** con corte de graves: 90 Hz en la voz, 250 Hz en la percusión, 150 Hz en el resto.
+++

#### Pistas por encima de 0 dB

Una pista que pasa de 0 dB satura en cuanto pasa por algo que no lo aguante. **Aplicar** le baja el volumen.

#### Graves que chocan

Dos pistas llenan los graves (por debajo de 120 Hz) a la vez y se emborronan. Si una es la batería, **Aplicar** pone a la otra un **Ducker** con sidechain de la batería, para que se aparte cada vez que suena. Si no, recorta los graves de la que menos tiene con un **EQ Three**.

+++ Cómo lo calcula
- Cuenta como batería una Drum Machine, una Drum Rack, el **Kick Designer** (el bombo en su propia pista) y una pista de audio con nombre de batería («Kick», «Tops», «Perc»…).
- Si las dos son batería y una es el Kick Designer, el bombo se queda los graves y se recortan los de la otra.
- Si una ya se aparta de la otra con un sidechain (el bajo del bombo, o un rumble), no es un choque: lo dice en verde.
+++

#### Se tapan en los medios

Dos pistas suenan a la vez en la misma zona y con un nivel parecido, como una voz y unos acordes, o un sinte y una guitarra: ninguna se oye clara. El consejo dice en qué frecuencia, y **Aplicar** abre ahí un hueco en la pista que cede, para que la otra se oiga sin bajar el volumen de ninguna.

+++ Cómo lo calcula
- Busca parejas que suenan de 250 Hz a 4 kHz con un nivel a menos de 6 dB la mayor parte del tiempo que coinciden.
- El hueco es de 4 dB, una campana estrecha en el **EQ Eight** de la pista que cede (en uno nuevo si no tiene).
- Cede la que no es protagonista: la voz manda sobre todo, luego la solista, los coros, la batería, el bajo, los acordes, los pads, la percusión y los efectos. Entre dos del mismo papel, cede la más floja.
- El hueco va donde la protagonista suena más fuerte de entre las zonas en que se tapan: su cuerpo, que es lo que necesita para oírse.
- No cuenta batería contra percusión (se turnan) ni el bombo o el bajo (su sitio son los graves: para eso está **Graves que chocan**).
- Si la pista ya tiene un hueco ahí, sale en verde.
+++

#### Compresión de partida

La voz, los coros, la solista, el bajo o la batería van de muy suave a muy fuerte: en la mezcla unas partes se pierden y otras saltan. **Aplicar** les pone un **Compressor** ajustado a cómo suenan, que sigue sonando más o menos igual de fuerte.

+++ Cómo lo calcula
- Salta cuando de sus partes suaves a las fuertes hay más de 8 dB en la voz y los coros, 6 en el bajo, 10 en la solista y la batería.
- Le quita a lo fuerte 5 dB en la voz, 4 en los coros y el bajo, 3 en la solista y la batería. El umbral se calcula con su nivel antes del fader y el panorama, que es lo que oye el compresor.
- Ratio 3:1 en la voz y los coros, 2,5:1 en la solista y 4:1 en el bajo y la batería. Ataque rápido en las voces (5 ms) y lento en la batería (25 ms), para que pase el golpe.
- Release de una semicorchea al tempo de la canción (una corchea en el bajo) y ganancia para recuperar la mitad de lo que quita.
- Si la pista ya tiene un compresor (sin sidechain), sale en verde. Una pista que se aparta de otra con un sidechain (un rumble o un bajo que bombean con el bombo) no se comprime: su nivel se mueve a propósito.
+++

#### Eses que saltan

En una voz o unos coros, las eses («s», «ch», «z») suenan casi tan fuertes como las vocales y pinchan, sobre todo tras comprimir. **Aplicar** pone un **De-esser** ajustado a la voz. El consejo dice cuánto las bajará.

+++ Cómo lo calcula
- Salta cuando las eses están a menos de 8 dB de las vocales.
- Pone el De-esser en la frecuencia de sus eses: 4,5, 6 o 7,5 kHz, según dónde estén.
- Calcula el umbral para bajarlas unos 6 dB, pero nunca tanto como para tocar el brillo de las vocales.
- Si la pista ya tiene un De-esser, sale en verde.
+++

#### Nivelar la voz

La voz cambia mucho de una frase a otra frente a la base: unas frases se pierden y otras tapan la música. **Aplicar** escribe una automatización de volumen en su pista, como haría alguien moviendo el fader frase a frase.

+++ Cómo lo calcula
- Salta cuando alguna frase se aleja al menos 2 dB.
- Mide cada frase: dónde suena la voz, y cuánto queda por encima del resto de la mezcla en ese momento.
- Lleva cada frase a lo que está la mayoría, como mucho 6 dB arriba o abajo.
- Los cambios caen en los silencios entre frases, así que no se oyen. Respeta los cambios de tempo.
- Si la pista ya tiene automatización de volumen, no la toca (sale en verde). Si luego el asistente sube o baja esa pista, mueve también su automatización.
+++

#### Espacio de partida

Las pistas que suenan secas, sin mandar nada a ningún retorno, se quedan todas delante y la mezcla suena plana. **Aplicar** pone sus envíos a la reverb y al delay según su papel, y crea esos retornos si faltan.

+++ Cómo lo calcula
- Voz: reverb 22 % y delay 14 %. Coros: reverb 32 % y delay 8 %, para que queden detrás. Solista: reverb 20 % y delay 16 %. Efectos: reverb 30 % y delay 12 %.
- Pads: reverb 28 %. Acordes: 18 %. Percusión: 12 %. Batería: 8 %.
- El bombo y el bajo, secos: si mandan más de un 15 % a la reverb, avisa de que embarra los graves y la quita.
- Las pistas que ya mandan a algún retorno se quedan como están, y los grupos también (mandan sus pistas).
- Usa el retorno que tenga una Reverb y el que tenga un Delay. Si falta alguno y queda sitio, lo crea: una **Reverb** con un EQ Eight que le quita los graves (bajo 250 Hz) y lo más agudo, y un **Delay** de corchea con puntillo a tempo, que se oscurece en cada repetición y tampoco lleva graves.
+++

#### Ecos al final de frase

El truco de los «delay throws»: la última palabra de una frase se repite en eco y rellena el silencio que sigue, mientras la voz cantada sigue limpia. **Aplicar** escribe una automatización en el envío de la voz al delay.

+++ Cómo lo calcula
- Busca las frases que terminan antes de un silencio en el que cabe el eco: al menos lo que tarda el delay y 0,75 s, más que una respiración.
- El envío se abre al 40 % en la última palabra de esas frases y vuelve a donde estaba en cuanto acaban.
- Usa el retorno que tenga un Delay; si no hay, lo crea (corchea con puntillo a tempo).
- Si la pista ya tiene algún envío automatizado, no lo toca (sale en verde).
+++

#### Secciones que crecen

Si la canción tiene marcadores, mide cada parte. El estribillo o el drop deben sonar más grandes que lo que lleva a ellos; si no, la canción no llega a ningún sitio. **Aplicar** escribe una automatización de volumen en el master que baja lo justo las partes que llevan al estribillo.

+++ Cómo lo calcula
- El estribillo o el drop deben sonar por encima de las estrofas, subidas y preestribillos. Si están a menos de 1 LU, baja esas partes hasta dejarlos 1,5 LU por encima, como mucho 6 dB.
- Se fija en el nombre del marcador: «Estribillo», «Coro», «Chorus», «Drop» o «Hook» son lo que debe crecer; «Estrofa», «Verse», «Subida», «Build», «Pre…» o «Groove» son lo que lleva a ello. «Pre-estribillo» y «Subida al drop» cuentan como lo que lleva.
- La intro, el break, el puente y el final no se tocan, porque pueden ser más suaves a propósito.
- Baja esas partes en vez de subir el estribillo: el master va detrás del limitador, y subirlo saturaría.
- Si el master ya tiene automatización de volumen, no la toca.
+++

#### Fase

La mezcla o alguna pista está en contrafase: sus dos canales se anulan entre sí y, en mono (un móvil, un club), se pierde. **Aplicar** lo arregla en cada pista con su **Utility** (el último que tenga, o uno nuevo).

+++ Cómo lo calcula
- Un canal al revés (correlación por debajo de −0,5): invierte el derecho.
- Demasiado abierta (por debajo de −0,2): la estrecha lo justo para que en mono pierda poco (correlación 0,2).
- Graves en estéreo (una décima parte o más de la pista es grave, y su lado está a menos de 10 dB de su centro): los pone en mono por debajo de 120 Hz.
+++

#### Dinámica

Avisa si la mezcla está muy apretada o tiene partes muy lejos en volumen. No trae botón: es para que lo revises a mano.

### Aplicar todo

**Aplicar todo** (junto a «Consejos») hace los cambios de los consejos uno tras otro, en el orden de una mezcla. Vuelve a medir después de cada uno, porque cada cambio decide el siguiente:

1. Pistas por encima de 0 dB.
2. Mezcla de partida.
3. Graves.
4. Huecos en los medios: todos a la vez, uno por cada pareja de pistas, donde más se tapan.
5. Compresión.
6. Eses.
7. Nivelar la voz.
8. Espacio.
9. Un reajuste de volúmenes, porque recortar, apartar y comprimir los mueven.
10. Ecos al final de frase.
11. Secciones que crecen.
12. El master: el EQ si hay referencia, y la sonoridad del destino con el limitador.

Al terminar dice lo que ha hecho, y no vuelve a pedirte lo que acaba de hacer: lo que queda en la lista son sugerencias para aplicar a mano, si quieres. Cada paso se deshace con {⌘Z}.

Mientras cambia pistas, solo vuelve a tocar las que ha cambiado, y la mezcla entera solo cuando la necesita (para las secciones y la sonoridad). En una canción de 3 minutos tarda unos 30 segundos.

### Escuchar antes y después

**Escuchar: Antes · Después** aparece en cuanto el asistente cambia algo. Deja oír la mezcla como estaba antes de su primer cambio y como está ahora.

1. Dale a reproducir.
2. Pasa de **Antes** a **Después** mientras suena.
3. Cuando acabes, pulsa **Dejar de comparar**.

Las dos suenan al mismo volumen: la más fuerte se baja hasta la otra. Así comparas cómo suena y no cuánto, porque lo más fuerte siempre parece mejor.

> Nota: comparar no toca el proyecto ni el historial de {⌘Z}. **Dejar de comparar**, cerrar la ventana o cambiar cualquier cosa de la mezcla la devuelve a como está. Si la mezcla ha cambiado sin volver a analizarla, avisa de que no están igualadas.

# Archivos

## Navegador y biblioteca

El panel de la izquierda: de ahí sacas instrumentos, efectos, plugins y muestras, y exploras tu biblioteca. Muéstralo u ocúltalo con **☰**.

Arriba pone el **destino**: la pista seleccionada, que es donde va lo que elijas.

### Qué hay en el navegador

- **INSTRUMENTOS**, **EFECTOS DE AUDIO** y **PLUGINS**: haz clic en uno para ponerlo en la pista. Ver [[Instrumentos]], [[Dispositivos y efectos]] y [[Plugins VST3 y CLAP]].
- **MUESTRAS**: **Añadir muestra…**.
- **BIBLIOTECA**: busca muestras, proyectos y presets por nombre, y explora las carpetas Samples, [[Sonidos de Pulse]], Proyectos, Exportaciones, Presets y las tuyas.
- **PISTAS**: nueva pista MIDI o de audio.

Para añadir una carpeta tuya, pulsa **Añadir carpeta…**. Para quitarla, clic derecho › **Quitar del navegador**.

### Escuchar y usar archivos

| Archivo | Clic | Doble clic | Arrastrar |
|---|---|---|---|
| Audio | Lo escucha (otro clic lo para) | Lo añade a la pista de audio seleccionada | A un slot, una pista, un pad o el Sampler |
| MIDI (.mid) | — | Trae sus partes en pistas nuevas | A una pista MIDI: todas sus notas en un clip ahí |
| Proyecto | — | Lo abre | — |
| Preset (o kit) | — | Lo aplica a la pista seleccionada | A una pista MIDI: se lo aplica |

- {↑} {↓}: tras hacer clic en un archivo, pasan al anterior o al siguiente y lo escuchan. Un clic fuera del navegador devuelve las flechas al resto de Pulse.
- Sobre los .mid, ver [[Archivos MIDI]].

### Escuchar loops a tempo

Con **A tempo** marcado (arriba, viene marcado de serie), los loops se escuchan al tempo del proyecto y en bucle, como sonarían en un clip.

- Si la canción está sonando, entran a tiempo con ella, en su compás. Con el loop activado vuelven atrás con ella, en el mismo instante y sin clic.
- Al lado de la casilla pone a qué tempo va el loop.
- La primera vez que escuchas un archivo tarda un momento en sonar, mientras Pulse busca su tempo.
- Sin marcar, o con archivos de más de 32 s, cada archivo suena una vez a su velocidad.

+++ Detalles
Vale para loops de hasta 32 s. Los de batería se estiran en modo Beats, como en un clip.
+++

### Arrastrar desde el Finder

| Sueltas… | Dónde | Qué pasa |
|---|---|---|
| Un audio | En un slot o una pista de audio | Va ahí |
| Un audio | En un pad o el Sampler | Se carga |
| Un audio | En un hueco vacío | Crea una pista de audio |
| Un `.mid` | En un hueco vacío, o sobre una pista MIDI | Trae sus partes en pistas nuevas o, sobre una pista MIDI, todas sus notas en un clip (ver [[Archivos MIDI]]) |
| Un `.pulse` | En cualquier sitio | Se abre |
| Un `.als` | En cualquier sitio | Se importa (ver [[Importar de Ableton Live]]) |

- Varios audios van seguidos. En Session, si no caben en los 16 slots, siguen en una pista nueva.
- Una **carpeta** trae los audios que tiene, también los de sus subcarpetas, ordenados por nombre.
- La barra de estado dice cuántos se añadieron y cuáles no se pudieron leer.
- Antes de abrir un `.pulse` o importar un `.als`, Pulse pregunta si hay cambios sin guardar.
- También funciona abrir con doble clic en el Finder o soltar en el icono del Dock.

## Sonidos de Pulse

Una biblioteca de muestras que viene con Pulse: kits de batería por género, bombos, cajas, charles, percusión, 808, bajos, acordes y efectos. Todo lo ha sintetizado Pulse con sus propios instrumentos y efectos: no hay grabaciones de nadie, y puedes usarlo en tus canciones sin límites.

### Abrirla

1. En el navegador, abre **BIBLIOTECA › Sonidos de Pulse**.
2. La primera vez, Pulse crea los sonidos: una barra dice cuántos lleva. Tarda unos segundos y no para nada; puedes seguir trabajando.
3. Ya están: escúchalos con un clic y arrástralos a una pista, un pad o el Sampler, como cualquier muestra (ver [[Navegador y biblioteca#Escuchar y usar archivos]]).

La búsqueda de la biblioteca también los encuentra: prueba «bombo techno», «808» o «subida».

### Qué hay

| Carpeta | Qué tiene |
|---|---|
| Bombos | Uno por estilo: house, techno saturado y con rumble, trap, drum and bass, lo-fi, garage, afro, ochentero, minimal, boom bap y electro |
| Cajas y palmas | Cajas, palmadas, aros, un chasquido y la caja ochentera con reverb puerta |
| Charles y platos | Charles cerrados y abiertos de cada estilo, crash y ride |
| Percusión | Congas, bongó, cencerros, clave, bloque de madera, shakers, pandereta, toms, una percusión metálica, un blip y una gota |
| 808 y subs | 808 largo, saturado, corto y con golpe, y dos subs, todos en Do |
| Bajos | Golpes de bajo en Do: acid, reese, FM, de órgano, techno, sierra, slap y rebotado |
| Stabs y acordes | Acordes con la nota en el nombre: stab de house y de órgano, acorde dub, rave, quintas, piano eléctrico, lo-fi, metales y ochentero |
| FX | Subidas de 4 y 8 compases (a 120 BPM), subida de ruido, bajada, impacto, crash al revés, barrido de ruido, caída de sub, láser, zap, viento y ruido de vinilo |
| Bucles | El ritmo de cada kit, de dos compases, con el tempo en el nombre |
| Kits | Los diez kits, para cargarlos en los Drum Pads |

- Los sonidos de cada carpeta están a un volumen parecido, con el pico por debajo de −0,3 dB: puedes cambiar uno por otro sin reajustar la mezcla.
- Los que tienen nota la llevan en el nombre, afinados a menos de 10 cents: «808 largo (Do)» es un Do. Los 808 y los bajos están en C1, y los acordes en C3: en el [[Instrumentos#Sampler|Sampler]], pon **Root** en esa nota y cada tecla sonará en la suya.
- Los bucles se escuchan a tempo con **A tempo** (ver [[Navegador y biblioteca#Escuchar loops a tempo]]): su tempo viene en el nombre, como «House 124».

### Cargar un kit en los Drum Pads

Cada kit llena los 16 pads con el orden de siempre: bombo en C1, aro, caja, palmada, una segunda caja, toms, charles cerrado y abierto (el cerrado corta al abierto), shaker, percusiones, crash, un efecto o acorde y ride.

- En los **Drum Pads**, abre **Kits ▾** y elige uno. Pasa el ratón por encima para leer cómo suena.
- O en el navegador, en **Sonidos de Pulse › Kits**: doble clic carga el kit en la pista seleccionada, y arrastrarlo a una pista MIDI se lo pone a esa pista.

| Kit | Cómo suena |
|---|---|
| House | Bombo redondo con pegada, palmada larga con sala, charles abierto a contratiempo, shaker, congas y un stab de órgano |
| Techno | Bombo saturado con rumble, charles metálicos, palmada en placa, aro y cencerro distorsionados |
| Trap | Bombo corto y 808 largo en Do, caja y palmada secas, chasquido y charles nítidos para redobles |
| Drum & Bass | Bombo seco, caja tensa que restalla, caja suave para los golpes fantasma, charles cortos y ride |
| Lo-Fi | Todo apagado, saturado y con menos resolución, y un acorde de piano gastado y ruido de vinilo |
| UK Garage | Bombo corto que rebota, aro y caja nítidos, charles rápidos, shaker y pandereta |
| Afro House | Bombo cálido, congas, bongó, shaker, clave, cencerro y toms |
| Synthwave | Caja y palmada con reverb puerta de los 80, toms grandes y un acorde ochentero |
| Minimal | Bombo con clic, aros y charles diminutos, bloque de madera, blips y gotas |
| Hip Hop | Boom bap: bombo que golpea, caja gorda con sala, charles apagados y un acorde de piano eléctrico |

> Consejo: el bucle de cada kit, en **Bucles**, es el mismo kit tocando su ritmo. Escúchalo para elegir kit antes de cargarlo.

+++ Dónde están
Pulse los crea en su carpeta de datos (`~/Library/Application Support/Pulse/Sonidos de Pulse` en macOS, `%LOCALAPPDATA%\Pulse\Sonidos de Pulse` en Windows), en WAV de 24 bits a 48 kHz: unos 47 MB. Cuando una canción usa uno y la guardas, se copia a la carpeta del proyecto, como cualquier audio.

Si una versión nueva de Pulse trae sonidos cambiados, los crea en otra carpeta, al lado: tus canciones siguen sonando con los de antes.
+++

## Proyectos y carpeta de Pulse

Cómo guardar y abrir tus canciones, empezar desde una plantilla y dónde vive cada archivo. Pulse guarda copias solo y te deja recuperar el trabajo si algo falla.

### Guardar y abrir

Casi todo está en el menú **Archivo**:

| Atajo | Qué hace |
|---|---|
| {⌘N} | Proyecto nuevo (o desde la plantilla elegida en Ajustes) |
| {⌘O} | Abrir |
| {⌘S} | Guardar |
| {⇧⌘S} | Guardar como |
| {⌥⌘S} | Versiones (ver [[Versiones del proyecto]]) |
| {⌘Z} / {⇧⌘Z} | Deshacer / rehacer (menú **Edición**) |

- **Abrir recientes**: los 10 últimos proyectos.
- **Volver a la versión guardada**: descarta los cambios desde el último guardado.
- **Nuevo desde plantilla ▸** y **Guardar como plantilla…**: ver más abajo.
- **Mostrar la carpeta de Pulse** la abre en el Finder (en el Explorador de archivos en Windows).
- El título de la ventana lleva * si hay cambios sin guardar. También cuentan los que haces dentro de un plugin: en su editor, con un controlador o en **Parámetros…**.
- Antes de perder cambios sin guardar, Pulse pregunta: **Guardar**, **No guardar** o **Cancelar**.

> Consejo: puedes deshacer mientras suena. Solo cambia lo que deshaces: las demás pistas siguen como estaban, con sus colas de reverb y delay y sus plugins. Lo mismo al añadir una pista.

Al guardar, todos los audios que usa el proyecto se copian a su carpeta; los grabados en un proyecto sin guardar se mueven. {⌘Z} después de guardar deshace tu último cambio, y los clips siguen apuntando a sus audios en la carpeta del proyecto.

### La carpeta de Pulse

Todo tu trabajo vive en la **carpeta de Pulse**: por defecto Música/Pulse (`~/Music/Pulse` en macOS; la carpeta Música de tu usuario en Windows). Se cambia en [[Ajustes]].

| Carpeta | Qué guarda |
|---|---|
| `Proyectos/<nombre>/` | Cada proyecto, con sus audios en `Samples/` y sus versiones en `Versiones/` |
| `Plantillas/<nombre>/` | Tus plantillas, cada una con sus audios |
| `Exportaciones` | Lo que exportas |
| `Presets`, `Wavetables`, `Samples` | Los tuyos |
| `Sin guardar` | Lo que graba o crea un proyecto aún sin guardar; al guardarlo se mueve a su carpeta |

- Dentro de `Samples/` de cada proyecto: Importados, Grabaciones, Stems, Trozos, Consolidados y Congelados.
- La carpeta de un proyecto se puede copiar entera a otro ordenador, también de macOS a Windows y al revés.

> Atención: si al abrir un proyecto falta algún audio (estaba fuera de su carpeta o en otro ordenador), Pulse lo dice en la barra de estado y conserva su ruta. Pon el archivo en su sitio o vuelve a importarlo.

+++ Ajustes, registros e informes de fallos
Los ajustes y lo que Pulse genera no son tu trabajo y no están en la carpeta de Pulse:
- macOS: `~/Library/Application Support/Pulse`.
- Windows: los ajustes en `%APPDATA%\Pulse`, y kits y modelos en `%LOCALAPPDATA%\Pulse`.

Si Pulse se cierra por un fallo, deja lo que pasó en `pulse.log` (`~/Library/Logs/Pulse` en macOS, `%LOCALAPPDATA%\Pulse\Logs` en Windows), con la versión, el sistema y el commit. Si se cierra de golpe sin dejarlo (un plugin que falla, por ejemplo), en macOS hay un informe en **Consola › Informes de fallos** (`~/Library/Logs/DiagnosticReports/pulse-*.ips`).
+++

### Plantillas

Una **plantilla** es un proyecto del que partir. Guarda tus pistas, instrumentos, efectos, muestras de los Drum Pads y el Sampler, mezcla, envíos, retornos, master, tempo, tonalidad, escenas, macros y asignaciones MIDI. No guarda el contenido: ni clips, ni arreglo, ni automatización, ni marcadores.

Para empezar desde una: **Archivo › Nuevo desde plantilla ▸**. Pulse trae dos, y debajo de una línea van las tuyas:

- **Beat**: Batería 909 con compresor, Bajo ácido, Acordes y Melodía con envíos a la reverb y al delay, a 124 BPM.
- **Grabar voz**: una pista Base para el instrumental y dos de audio, Voz y Coros, con ecualizador, compresor y envío a la reverb, a 90 BPM.

Para crear una:

1. Monta el proyecto como quieras empezar tus canciones.
2. **Archivo › Guardar como plantilla…** y ponle un nombre.

- Las muestras que use se copian a la carpeta de la plantilla, así que sigue funcionando aunque borres o muevas el proyecto. Tu proyecto no cambia.
- Con el nombre de una que ya existe, la sustituye, muestras incluidas (no se acumulan copias). Con el de una de Pulse, la tuya va primero.
- Un proyecto que empieza desde una plantilla está sin guardar: {⌘S} pregunta dónde y se lleva sus muestras a su carpeta.

Para que {⌘N} parta siempre de una plantilla, elígela en **Ajustes › Proyectos › Proyecto nuevo (⌘N)**. **Vacío** son dos pistas MIDI, como siempre. Si esa plantilla no se puede abrir (dañada, borrada o de un Pulse más nuevo), {⌘N} abre un proyecto vacío y te lo dice abajo.

> Nota: para borrar o renombrar una plantilla, borra o renombra su carpeta en `Plantillas` (con **Mostrar la carpeta de Pulse**). Al renombrar, cambia también el `.pulse` de dentro, que se llama igual que la carpeta.

### Autoguardado y recuperación

Cada 30 segundos, si hay cambios sin guardar, Pulse guarda una copia aparte. Tu archivo `.pulse` no se toca hasta que guardas.

Si Pulse se cierra sin guardar ni preguntar (un fallo, el ordenador se apaga), al abrirlo otra vez pregunta si quieres recuperar los cambios:

- **Recuperar**: vuelve el proyecto tal como estaba en el último autoguardado, marcado como sin guardar. Guárdalo con {⌘S} para conservarlo.
- **Descartar**: lo aparta como `descartado.pulse` en la misma carpeta, por si acaso, hasta la próxima vez.

Las grabaciones también se protegen: mientras grabas audio, lo que entra se va escribiendo a disco. Si Pulse se cierra a mitad, la toma queda con lo grabado hasta ese momento.

+++ Detalles
- Las copias van a la carpeta `Recuperación` de los ajustes: `~/Library/Application Support/Pulse/Recuperación` en macOS, `%LOCALAPPDATA%\Pulse\Recuperación` en Windows.
- Si la copia no se puede abrir (por ejemplo, la hizo una versión más nueva de Pulse), se aparta como «autoguardado.pulse (dañado)» y el siguiente autoguardado no la pisa.
- La grabación en curso se escribe en `Samples/Grabaciones` como «Grabación sin terminar…». Al parar se convierte en la toma de siempre y esa copia se borra. Si Pulse se cierra a mitad, queda ahí, y la pregunta de recuperar la menciona. Si al parar no se puede escribir la toma (disco lleno), esa copia tampoco se borra y la barra de estado dice dónde está.
- Si Pulse se cerró mientras reabría el último proyecto al arrancar, la vez siguiente no lo reabre solo, porque puede ser ese proyecto lo que lo cierra. Ábrelo desde **Abrir recientes** cuando quieras.
- Si el sonido de una pista deja de ser válido (NaN, infinito o un nivel absurdo, por encima de +60 dBFS, por un archivo dañado, un plugin que falla o un filtro que se dispara), Pulse la silencia en ese momento, vacía sus efectos y lo dice en la barra de estado. El resto sigue sonando.
- Guardar es todo o nada: Pulse escribe primero una copia completa y solo entonces sustituye el archivo. Un cierre, un disco lleno o un corte de luz a mitad de guardar dejan el archivo anterior entero.
- Si el proyecto tuviera algún número no válido (NaN o infinito, por ejemplo de un set de Live dañado), se guarda como 0 y la barra de estado dice cuántos.
- Si un archivo de ajustes o de listas tiene algo que Pulse no entiende (un valor de una versión más nueva, una entrada dañada), solo se pierde eso: el resto se conserva y queda una copia del original como «… (dañado)». Si no se puede leer nada, Pulse lo aparta con ese nombre en vez de sobrescribirlo.
+++

## Versiones del proyecto

Pulse guarda el historial de tu canción: prueba ideas sin miedo y vuelve a cualquier punto. Abre la ventana con **Archivo › Versiones…** ({⌥⌘S}).

### Guardar una versión

1. Escribe un nombre, por ejemplo «Con el bajo nuevo», y si quieres una nota.
2. Pulsa **Guardar versión**. Queda guardado cómo está el proyecto ahora.

Las versiones con nombre se quedan para siempre.

Además, cada vez que guardas con {⌘S} se hace una versión **automática**, si algo cambió desde la anterior. Se guardan las 20 últimas.

### Ver qué cambió

- Cada versión dice **cuándo** se hizo y **qué cambió** respecto a la anterior: «Tempo 120 → 124», «+ Pista «Voz»», ««Bass»: 1 → 2 clips en Session, mezcla»…
- **Comparar con ahora**: lo que ha cambiado desde esa versión hasta el proyecto tal como está.

### Volver a una versión

**Volver a esta** abre esa versión en lugar de lo que hay. Guarda para quedarte con ella.

> Consejo: no se pierde nada. Lo que había se guarda antes como versión con nombre («Antes de volver a…»), que las automáticas nunca desplazan, y {⌘Z} lo deshace.

- **Renombrar**: a una automática, ponerle nombre hace que se quede para siempre.
- **Borrar**: pide confirmación.

> Nota: las versiones viven en la carpeta del proyecto (`Versiones/`) y usan sus mismos audios. Hace falta que el proyecto esté guardado en su carpeta: si es un `.pulse` suelto de antes, usa **Guardar como…**.

## Importar de Ableton Live

Abre en Pulse un set de Ableton Live (`.als`) con sus pistas, clips y mezcla. Usa {⌘I} (**Archivo › Importar set de Live**) o arrastra el `.als` a la ventana.

> Atención: sustituye el proyecto actual. El `.als` no se toca.

### Qué se importa

- Tempo, compás (y sus cambios), loop y escenas.
- Pistas MIDI y de audio, con nombre, color, volumen, panorama, envíos, activador y solo.
- Los clips de Session y del Arrangement, con sus notas o sus muestras y su warp.
- Los **grupos**, como grupos de Pulse, con sus pistas dentro y su mezclador.
- Los **locators**, como marcadores.
- Los retornos, con nombre y volumen. Los dos primeros conservan la reverb y el delay de Pulse; los demás, hasta 12, llegan vacíos.
- Los Drum Racks con muestras pasan al Drum Pads de Pulse; Simpler y Sampler, al Sampler.

### Qué no se importa

- Los sintes de Live: esas pistas reciben Pulse Synth.
- Los efectos y la automatización.

Las muestras se buscan por su ruta y, si el set viene de otro ordenador, por su nombre en la carpeta del set.

## Archivos MIDI

Pulse abre y guarda archivos MIDI estándar (**.mid**). Trae partes de otro programa (una progresión, una batería, una canción entera) o llévate las tuyas.

### Traer un MIDI

Usa **Archivo › Importar MIDI…**, o arrastra el .mid desde el Finder o el navegador de Pulse.

- **En un hueco vacío, o con Importar**: cada parte del archivo va a una pista MIDI nueva. Una parte es cada canal de cada pista del archivo. En el Arrangement van al principio; en la Session, al primer slot.
- **Sobre una pista MIDI** (un slot vacío o su carril, en el tiempo donde sueltas): todas las notas van en un solo clip, ahí.

Cómo quedan:

- Las pistas nuevas se llaman como en el archivo. Sin nombre, se llaman por su sonido General MIDI («Bajo», «Piano», «Cuerdas»…).
- El canal 10 es la batería y va a una caja de ritmos.
- Los pianos, pianos eléctricos y órganos (por su sonido General MIDI) entran con [[Pulse Keys]] y un preset parecido. Las demás partes suenan con el sinte: cámbialo por el instrumento que quieras.
- El pedal de sustain del archivo alarga las notas que sostiene, igual que al tocarlas.
- Cada clip dura hasta el compás donde acaba su última nota.
- Si el proyecto está vacío, toma el tempo y el compás del archivo (con sus cambios). Si no, suena al tempo y en el compás del proyecto, y te dice cuáles traía el archivo.

> Nota: los cambios de tempo de dentro del archivo no se traen.

### Llevarte el MIDI

- **Archivo › Exportar MIDI…** guarda la canción: cada pista MIDI con clips en el Arrangement es una pista del archivo, con su nombre, al tempo y con los compases del proyecto (y sus cambios).
- Clic derecho en un clip MIDI › **Exportar MIDI…** guarda solo ese clip.

+++ Detalles
- La batería va en el canal 10 para que otros programas la reconozcan.
- Un clip que en el Arrangement dura más que él mismo se repite como suena. Los clips desactivados no van.
+++

## Exportar audio

Convierte tu canción en un archivo de audio para escucharla, compartirla o publicarla, o en una pista por archivo (stems) para mezclarla en otro sitio. Abre el diálogo con {⇧⌘R} (**Archivo › Exportar audio…**).

> Consejo: antes de exportar, pasa por el [[Asistente de mezcla y master]].

### Exportar la canción

1. Pulsa {⇧⌘R}.
2. En **Qué**, deja **Mezcla completa**.
3. En **Rango**, elige **Toda la canción**.
4. Elige el **Formato** y pulsa **Exportar…**.

Por defecto se guarda en la carpeta `Exportaciones`. El diálogo recuerda lo que elegiste la última vez (formato, rango, stems, normalización…), también entre sesiones.

Se exporta el **Arrangement**, no la Session, con la cola de reverbs y delays. Lo exportado suena igual que lo que oyes, y Pulse avisa si la mezcla satura.

### Qué exportar

- **Mezcla completa**: la canción en un archivo.
- **Una pista por archivo (stems)**: cada pista como suena en solo, con sus envíos y su grupo.
  - No se exportan las pistas desactivadas (ni las de un grupo desactivado), ni los grupos como stem aparte: sus pistas ya pasan por ellos.
  - Si tienes alguna pista en solo, solo se exportan las que suenan con ese solo, como en la mezcla.
  - **Con la cadena del master**: quítala para que los stems sumados den la mezcla. Si un grupo o un retorno comprime o satura, sumados se le parecen pero no la dan exacta, y el diálogo lo avisa.

### Qué parte

- **Toda la canción**.
- **Región del loop**.
- **Por secciones**: un archivo por cada parte de la canción, de un marcador al siguiente (el último, hasta el final), con el número y el nombre del marcador: «Tema - 1 Intro.wav», «Tema - 2 Drop.wav»… Lo que haya antes del primer marcador sale como «Inicio». Con stems, una tanda por sección: «Tema - 2 Drop - 1 Drums.wav».

### Exportar versiones

Si el proyecto tiene versiones con nombre (ver [[Versiones del proyecto]]), marca en **Versiones** las que quieras exportar también. Cada una sale tal como era (su tempo, sus pistas, su mezcla) y con su nombre al final del archivo: «Tema - Con el bajo nuevo.wav». **Solo las versiones** deja fuera lo de ahora.

### Formato

| Formato | Para qué | Opciones |
|---|---|---|
| **WAV** | Calidad máxima, sin pérdida | 16 bits (con dither), 24 o 32 bits (float) |
| **FLAC** | Sin pérdida, como el WAV, y más o menos la mitad de grande | 16 o 24 bits |
| **MP3** | Escuchar y compartir | 192, 256 o 320 kbps |

- Los tres, a **44.1** o **48 kHz**.
- **Normalizar**: lleva el pico a un valor. No está en stems.

> Nota: un MP3 sale normalizado, como mucho a −1 dBFS: al decodificarlo sube un poco, y a 0 saturaría.

### Archivos que ya existen

- Si al exportar stems, secciones o versiones ya hay archivos con esos nombres, de una exportación anterior, Pulse pregunta antes de sustituirlos.
- Dos versiones con el mismo nombre salen en archivos distintos («… - Mezcla.wav», «… - Mezcla 2.wav»).
- Los archivos aparecen enteros o no aparecen. Si cancelas o falla (disco lleno), no queda nada a medias.

+++ Cómo funciona
- **Dither**: en WAV de 16 bits es distinto en cada archivo, para que el ruido de los stems no se sume en fase.
- **MP3 sin huecos**: lleva la información para reproducirse sin huecos. Empieza y acaba en la misma muestra que el WAV, sin el silencio que añade el codificador, así que encaja en una sesión o en un loop.
- **Stems a la vez**: se renderizan a la vez, uno por núcleo del procesador, y cada uno solo con lo que puede sonar en él (su pista, sus grupos y las pistas que escuchan sus sidechains). 16 stems tardan poco más que la canción una vez. Suenan exactamente igual que en solo.
- **Plugins**: si el proyecto tiene plugins, los stems se renderizan de uno en uno, y sus copias se cierran en el hilo principal de Pulse, como piden los plugins VST3 (algunos se cierran de golpe si no).
- **Secciones**: cada sección empieza con lo que ya sonaba (también una nota larga que empezó muchos compases antes) y acaba con su cola. Empieza y acaba justo en sus marcadores, aunque el tempo esté automatizado. Una pista que no suena en una sección no tiene stem de ella, en vez de un archivo en silencio; al terminar, el aviso dice cuántos se han omitido.
- **Colas**: espera lo que tarda el delay más lento del proyecto antes de dar la cola por terminada, así que caben también los ecos de un delay largo.
- **Inicio**: empieza a todo volumen desde la primera muestra. Al exportar la **Región del loop** se toca desde unos compases antes, así que lo que ya sonaba (notas largas, colas) está desde el principio.
- **Igual que lo que oyes**: muestra a muestra, con cualquier tamaño de búfer. La automatización se aplica cada 64 muestras, en los mismos puntos, y los LFOs del Auto Filter y el Phaser se mueven en los mismos instantes. El Polychrome, con un búfer que no sea múltiplo de 64, puede mover su modulación unas muestras antes o después: no se oye.
- **Escritura segura**: mientras se exporta, los archivos se escriben ocultos al lado y solo toman su nombre al terminar todos.
+++

# Pulse

## Ajustes

Dónde elegir la tarjeta de sonido, los dispositivos MIDI, las carpetas de plugins y cómo arranca Pulse. Se abren con {⌘,} (**Pulse › Ajustes…**) y los cambios se aplican al momento.

### Audio

- **Salida** y **entrada** (la entrada es la que se usa para grabar).
- **Frecuencia** y **tamaño del búfer**: más pequeño responde antes; si oyes cortes, súbelo.
- **Latencias medidas**, también la de los **efectos**.
- **Ajuste fino de la grabación** y **Compensar la latencia de los efectos** (ver [[Grabar]]).

Cuando el proyecto pesa, Pulse reparte las pistas y los retornos entre varios núcleos del procesador (hasta la mitad de los que tiene, y como mucho seis). Así caben muchas más pistas, instrumentos y efectos sin cortes, y suena exactamente igual que en un solo núcleo (así exporta): lo que oyes es lo que exportas. Con poco que procesar, lo hace todo en uno y los demás descansan.

### MIDI

Los dispositivos conectados, y un botón para volver a buscarlos.

### Plugins

Las carpetas donde Pulse busca plugins, y **Buscar plugins** (ver [[Plugins VST3 y CLAP]]).

### Carpeta de Pulse

Dónde está, **Cambiar…** (lo que ya hay no se mueve) y **Mostrar en Finder** (ver [[Proyectos y carpeta de Pulse]]).

### Proyectos

- Abrir el último proyecto al arrancar.
- De qué plantilla parte {⌘N} (ver [[Proyectos y carpeta de Pulse]]).
- **Clips MIDI nuevos**: cuántos compases mide un clip MIDI al crearlo con doble clic o con **Nuevo clip MIDI aquí** (1, 2, 4 u 8; cuatro de entrada).
- Buscar actualizaciones al arrancar.

## Actualizaciones

Pulse busca versiones nuevas por su cuenta y las instala con un clic, guardando la anterior por si quieres volver.

### Instalar una versión nueva

1. Pulse busca al arrancar y cada 6 horas. También puedes buscar con **Pulse › Buscar actualizaciones…**.
2. Si hay una, la ventana muestra sus novedades. Elige **Descargar e instalar**, **Más tarde** u **Omitir esta versión**.
3. Al terminar, pulsa **Reiniciar ahora**. Si hay cambios sin guardar, pregunta antes.

La primera vez que arranca una versión nueva, enseña sus novedades.

### Volver a la versión anterior

La versión anterior se guarda como «Pulse (anterior)». **Volver a la versión anterior** la recupera.

> Atención: si abres con una versión anterior un proyecto guardado con una más nueva, la barra de estado lo dice y, antes de guardarlo encima, Pulse pregunta: esa versión no conoce lo nuevo y se perdería. **Guardar como…** guarda una copia sin tocar el original. Los ajustes que no conoce se conservan.

### Si algo no va

- Si Pulse no puede reemplazarse donde está, lo dice antes de descargar nada. Pasa si lo abres sin moverlo de Descargas (macOS lo ejecuta desde una copia temporal: muévelo a Aplicaciones) o si está en una carpeta en la que tu usuario no puede escribir.
- Si algo falla al cambiar de versión, todo queda como estaba, también la versión anterior guardada.
- Solo se anuncian las versiones que traen paquete para tu sistema.

+++ Acceso a GitHub (opcional)
No hace falta cuenta de GitHub: el repositorio es público. **Acceso a GitHub (opcional)** solo sirve si GitHub dice que hay demasiadas consultas (tiene un límite por hora). Usa la sesión de `gh auth login` o un token pegado ahí, que se guarda en el llavero en macOS y en el Administrador de credenciales en Windows. Si el token caduca, Pulse sigue sin él.
+++

+++ En Windows y comprobaciones de seguridad
- En Windows se sustituye `pulse.exe`, y el anterior queda como «pulse (anterior).exe» en la carpeta de instalación.
- Antes de instalar, Pulse comprueba que la descarga es la publicada (su huella SHA-256) y que es la versión anunciada.
+++

## Acerca de Pulse

**Pulse › Acerca de Pulse** muestra la versión y la licencia. Pulse es software libre (GPL-3.0 o posterior): puedes usarlo, estudiarlo, modificarlo y compartirlo.

- **Licencia (GPL-3.0)** abre el texto completo de la licencia.
- **Componentes de terceros** abre las licencias de lo que Pulse incluye: las librerías, ONNX Runtime, los modelos de IA y las fuentes.
- El código está en GitHub (el enlace de la ventana).

> Nota: si distribuyes una versión modificada de Pulse, compártela también con su código y con la misma licencia.

## Atajos de teclado

Todos los atajos de Pulse en una página. **Ayuda › Atajos de teclado** muestra esta lista en una ventana, y los atajos con {⌘} también están en la barra de menús.

### Proyecto y archivos

| Atajo | Qué hace |
|---|---|
| {⌘N} | Proyecto nuevo |
| {⌘O} | Abrir |
| {⌘S} / {⇧⌘S} | Guardar / Guardar como |
| {⌥⌘S} | Versiones del proyecto |
| {⌘I} | Importar set de Live |
| {⇧⌘R} | Exportar audio |
| {⌘,} | Ajustes (audio, MIDI, plugins) |
| {⌘Q} | Salir |

### Editar

| Atajo | Qué hace |
|---|---|
| {⌘Z} / {⇧⌘Z} | Deshacer / Rehacer |
| {⌘X} / {⌘C} / {⌘V} | Cortar / Copiar / Pegar los clips seleccionados (o las notas seleccionadas en el editor) |
| {⌘D} | Duplicar |
| {⌘E} / {⌘J} | Dividir el clip donde hiciste clic / unir clips (Arrangement) |
| {0} | Desactivar o activar los clips seleccionados |
| {⌫} | Eliminar |

### Pistas y selección

| Atajo | Qué hace |
|---|---|
| {⌘T} / {⇧⌘T} | Nueva pista MIDI / de audio |
| {⌥⌘T} | Nuevo retorno |
| {⌘G} | Agrupar las pistas seleccionadas |
| {⇧⌘G} | Desagrupar el grupo seleccionado |
| {⌘ clic} / {⇧ clic} | Seleccionar varias pistas (en sus cabeceras) o varios clips |
| Arrastrar cabecera | Mover las pistas seleccionadas |
| Arrastrar en un hueco | Seleccionar clips y pistas con un recuadro |
| Arrastrar clip / {⌥} | Mover los clips seleccionados / copiarlos |

### Vistas y transporte

| Atajo | Qué hace |
|---|---|
| {Tab} | Cambiar entre Session y Arrangement |
| {⇧Tab} | Cambiar entre el clip y los dispositivos |
| {⌃⌘F} | Pantalla completa |
| {Espacio} | Reproducir / Parar |
| {F9} | Grabar en el Arrangement |

### Editor de clips

| Atajo | Qué hace |
|---|---|
| {⌘A} | Seleccionar todas las notas |
| {⌘U} | Cuantizar notas |
| {B} | Lápiz |
| {← →} / {↑ ↓} | Mover / transponer notas ({⇧}: compás u octava) |
| {Esc} | Deseleccionar notas |

### Teclado del ordenador

| Atajo | Qué hace |
|---|---|
| {M} | Teclado del ordenador como teclado MIDI |
| {A S D F G H J K L} | Teclas blancas |
| {W E T Y U O P} | Teclas negras |
| {Z} / {X} | Octava abajo / arriba |
| {C} / {V} | Velocidad abajo / arriba |

Más en [[Teclado del ordenador]].
