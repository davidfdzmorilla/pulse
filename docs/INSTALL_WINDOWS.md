# Instalar Pulse en Windows

Para Windows 10 (1809 o posterior) y Windows 11, de 64 bits. Estado del soporte:
[WINDOWS_SUPPORT.md](WINDOWS_SUPPORT.md). **El instalador aún no se ha probado
en un Windows real.**

## Instalar

1. Descarga `Pulse-<versión>-windows-x64-setup.exe` de la publicación.
2. Ábrelo. Pulse no está firmado con un certificado, así que SmartScreen avisa
   («Windows protegió su PC»): **Más información → Ejecutar de todas formas**.
3. Se instala para tu usuario, sin permisos de administrador, en
   `%LOCALAPPDATA%\Programs\Pulse`, donde Pulse puede actualizarse solo. No se
   ofrece instalarlo para todos los usuarios (en Archivos de programa): ahí
   Pulse no podría actualizarse. Si alguna instalación antigua está ahí, Pulse
   lo dice al actualizar: desinstálala e instala la nueva.
4. Crea un acceso en el menú Inicio (y en el escritorio si lo marcas), y asocia
   los archivos `.pulse` a Pulse.

### Copia portable

`Pulse-<versión>-windows-x64.zip` lleva la carpeta `Pulse` con `pulse.exe` y lo
que necesita. Descomprímela donde quieras y abre `pulse.exe`. Así no se asocian
los `.pulse` ni hay acceso en el menú Inicio.

## Primer arranque

- **Audio**: Pulse usa la salida predeterminada de Windows (WASAPI). Ajustes
  (**Ctrl+,**) → Audio permite cambiar la salida, la entrada, la frecuencia y el
  búfer. En Windows la frecuencia es la de la configuración de sonido del
  dispositivo.
- **Micrófono**: si al grabar no entra nada, mira Configuración → Privacidad y
  seguridad → Micrófono → «Permitir que las aplicaciones de escritorio accedan
  al micrófono».
- **Plugins**: Pulse busca VST3 y CLAP en `C:\Program Files\Common Files\VST3` y
  `CLAP`, y en `%LOCALAPPDATA%\Programs\Common\VST3` y `CLAP`. Puedes añadir más
  carpetas en Ajustes → Plugins. Solo plugins de 64 bits.
- **Atajos**: donde la guía dice ⌘ es Ctrl; ⇧ es Mayús, ⌥ es Alt. Pantalla
  completa: F11.

## Dónde guarda cada cosa

| Qué | Dónde |
|---|---|
| Tus proyectos, exportaciones, presets, muestras | `Música\Pulse` (se cambia en Ajustes) |
| Ajustes, MIDI, plugins encontrados, recientes | `%APPDATA%\Pulse` |
| Kits generados, modelo de stems descargado | `%LOCALAPPDATA%\Pulse` |
| Cachés (modelos de tempo) | `%LOCALAPPDATA%\Pulse\Cache` |
| Registro de fallos (`pulse.log`) | `%LOCALAPPDATA%\Pulse\Logs` |
| El programa | `%LOCALAPPDATA%\Programs\Pulse` |

Los proyectos guardan sus audios en su carpeta, con rutas relativas, y se pueden
llevar de un Mac a un PC y al revés. Si al abrir un proyecto falta algún audio,
la barra de estado lo dice y la ruta se conserva.

## Actualizar

- Pulse busca versiones nuevas al arrancar (necesita acceso a GitHub: `gh auth
  login` de GitHub CLI, o un token en Ayuda → Buscar actualizaciones → «Acceso a
  GitHub», que se guarda en el Administrador de credenciales).
- **Descargar e instalar** baja el zip de Windows y comprueba su huella SHA-256 y
  su versión. Después cambia `pulse.exe` (y el runtime de Visual C++ si cambió) y
  guarda el anterior como `pulse (anterior).exe`. **Volver a la versión
  anterior** los intercambia.
- También se puede actualizar ejecutando el instalador de la versión nueva
  encima. Proyectos y ajustes no se tocan.

## Desinstalar

Configuración → Aplicaciones → Pulse → Desinstalar (o «Desinstalar Pulse» en el
menú Inicio). Se borran el programa, sus accesos y la asociación de `.pulse`.
**No se borran** tus proyectos (`Música\Pulse`) ni tus ajustes. Para
borrarlos también, elimina `%APPDATA%\Pulse` y `%LOCALAPPDATA%\Pulse` (y
`Música\Pulse` si ya no quieres tus proyectos).

## Problemas

- **Se cierra solo**: mira `%LOCALAPPDATA%\Pulse\Logs\pulse.log`, que guarda la
  traza de cada fallo.
- **No suena**: comprueba la salida en Ajustes → Audio. Si otro programa usa el
  dispositivo en modo exclusivo, Pulse no puede abrirlo. Prueba también un
  búfer mayor si hay cortes.
- **Un plugin no aparece**: que sea de 64 bits y esté en una de las carpetas de
  arriba o en una añadida en Ajustes. Luego, **Buscar plugins**.
- **El editor de un plugin no se abre**: el plugin puede no tener editor para
  Windows. **Parámetros…** siempre funciona.
- **Falta `VCRUNTIME140.dll` o `MSVCP140.dll`**: reinstala con el instalador, o
  instala el «Microsoft Visual C++ Redistributable» x64.
- **La actualización falla**: el mensaje dice por qué. Siempre puedes instalar la
  versión nueva con su instalador.

## Comprobaciones a mano

Lo que la CI no puede comprobar y hay que probar en un PC con Windows antes de
publicar:

1. Instalar en un Windows limpio (sin Visual Studio) y arrancar.
2. Audio: reproducir el proyecto de ejemplo; cambiar de salida, frecuencia y
   búfer; desconectar un dispositivo USB con Pulse sonando; cerrar Pulse.
3. Grabar audio (y el aviso de privacidad del micrófono).
4. MIDI: un teclado USB toca un instrumento; MIDI learn.
5. Plugins: Surge XT (VST3 y CLAP) o similares: cargar, abrir el editor, cambiar
   el tamaño, cerrar, guardar y reabrir el proyecto con su estado; un plugin que
   falte al reabrir.
6. Pantallas: escala 100 %, 150 % y 200 %; mover el editor de un plugin a otro
   monitor con otra escala.
7. Doble clic en un `.pulse` (también con espacios y acentos en la ruta).
8. Separar stems (descarga del modelo) y «Convertir a MIDI».
9. Exportar audio.
10. Actualizar a una versión publicada con zip de Windows, y volver a la anterior.
