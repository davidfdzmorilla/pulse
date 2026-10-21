# Instalar Pulse en macOS

Para Macs con Apple Silicon y macOS 13.4 (Ventura) o posterior.

## Instalar

1. Descarga `Pulse-<versión>.zip` de la última publicación en
   [github.com/davidfdzmorilla/pulse/releases](https://github.com/davidfdzmorilla/pulse/releases)
   y ábrelo: sale `Pulse.app`.
2. **Muévelo a Aplicaciones antes de abrirlo** (la tuya, `~/Applications`, o la
   del sistema). Abierto desde Descargas, macOS lo ejecuta desde una copia
   temporal de solo lectura y Pulse no puede actualizarse (lo dice).
3. Ábrelo. Pulse está firmado con un certificado propio, no con uno de Apple,
   así que la primera vez macOS no lo deja abrir («no se puede verificar…»).
   Ve a **Ajustes del Sistema → Privacidad y seguridad**, baja hasta el aviso
   de Pulse y pulsa **Abrir igualmente** (en macOS 15 ya no vale el clic
   derecho → Abrir). Solo hace falta una vez.

   Desde el terminal, lo mismo: `xattr -dr com.apple.quarantine /Applications/Pulse.app`.

4. La primera vez que grabes, macOS pregunta por el micrófono: acepta.

## Actualizar

Pulse busca versiones nuevas al arrancar y se actualiza solo desde su ventana
(Pulse → Buscar actualizaciones…), sin cuenta de GitHub. Las actualizaciones no
vuelven a pedir «Abrir igualmente»: van firmadas con el mismo certificado, y
Pulse no instala ninguna que no lo esté. La versión anterior queda como
«Pulse (anterior)» junto a la nueva, para volver a ella.

## Qué hay dónde

- Tus proyectos, muestras y exportaciones: `~/Music/Pulse` (se puede cambiar en Ajustes).
- Ajustes, kits y modelos: `~/Library/Application Support/Pulse`.
- Si Pulse falla: `~/Library/Logs/Pulse/pulse.log`; y si se cierra de golpe
  (un plugin, por ejemplo), macOS guarda un informe en **Consola → Informes de
  fallos** (`~/Library/Logs/DiagnosticReports/pulse-*.ips`). Ese archivo dice
  en qué línea del código falló (con `scripts/simbolos.py`, ver
  [PUBLICAR.md](PUBLICAR.md)): mándalo si avisas de un fallo.

## Desinstalar

Arrastra `Pulse.app` (y «Pulse (anterior)», si está) a la Papelera. Tus
proyectos siguen en `~/Music/Pulse`; los ajustes, en las carpetas de arriba.
