# Contribuir a Pulse

Gracias por querer mejorar Pulse. Antes de nada, para cambios grandes abre un
issue: así hablamos de la idea antes de que inviertas tiempo en ella.

## Licencia de las contribuciones

Pulse es GPL-3.0-or-later (ver [LICENSE](LICENSE)). Al enviar una contribución
(un pull request, un parche) declaras que:

1. **Puedes aportarla**: es obra tuya, o tienes derecho a publicarla con esta
   licencia, según el [Developer Certificate of Origin 1.1](https://developercertificate.org/).
   Lo confirmas firmando cada commit con `git commit -s`, que añade la línea
   `Signed-off-by: Tu Nombre <tu@correo>`.
2. **Se publica con la licencia de Pulse**, GPL-3.0-or-later.
3. **Concedes además al autor de Pulse, David Fernández Morilla, una licencia
   mundial, gratuita, no exclusiva, perpetua e irrevocable** para usar, copiar,
   modificar, sublicenciar y distribuir tu contribución, también con otras
   licencias, incluidas las comerciales. Tú conservas los derechos de autor de
   lo que aportas y puedes usarlo como quieras.

El punto 3 permite ofrecer Pulse con otras condiciones algún día (por ejemplo,
una licencia comercial) sin tener que localizar a cada persona que haya
contribuido. Si no estás de acuerdo con él, dilo en el pull request antes de que
se fusione.

No envíes código, muestras, presets, modelos ni imágenes de otros productos
(comerciales o no) salvo que su licencia lo permita y esté indicada. Si añades
una dependencia, ejecuta `python3 scripts/notices.py` para actualizar
`packaging/THIRD_PARTY_NOTICES.txt`.

## Cómo se trabaja

- Una rama por tarea: `funcion/<tema>` o `arreglo/<tema>`, y un pull request a
  `main`. A `main` no se sube nada directamente.
- El título del pull request va en español: al fusionarlo (aplastando los
  commits) se convierte en una línea de las novedades que ve la app.
- Las pruebas se pasan en `release`: `cargo test --release` (en `debug` las de
  interfaz fallan por una aserción de egui).
- El código mantiene su estilo a mano: no pases `cargo fmt` por todo el
  repositorio.
- Si cambias algo que se ve o se usa en la app, actualiza también la guía
  (`docs/guia.md`), que la app muestra en Ayuda → Guía de Pulse.
- Compilar en Windows: [docs/BUILD_WINDOWS.md](docs/BUILD_WINDOWS.md).
