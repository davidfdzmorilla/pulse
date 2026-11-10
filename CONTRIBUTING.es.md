# Contribuir a Pulse

[English](CONTRIBUTING.md) · **Español**

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

No hay acuerdo de colaborador (CLA) ni ninguna licencia adicional: tu
contribución tiene la misma licencia que el resto de Pulse, y conservas sus
derechos de autor.

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
- Si cambias algo que se ve o se usa en la app, actualiza también la guía en
  los dos idiomas (`docs/guia.md` y `docs/guide.md`), que la app muestra en
  Ayuda → Guía de Pulse. Las dos tienen los mismos temas, apartados, plegados
  y avisos en el mismo orden; los tests de `guide.rs` lo comprueban. Si no
  puedes escribir una de las dos, dilo en el pull request y te ayudamos.
- Compilar en Windows: [docs/BUILD_WINDOWS.md](docs/BUILD_WINDOWS.md).

## Contribuciones con ayuda de IA

Puedes usar herramientas de IA (asistentes de código, modelos de chat,
traductores) para preparar una contribución, siempre que **una persona**
responda de ella:

- **Abres tú el pull request y respondes tú a la revisión.** Entiendes cada
  línea que envías y puedes explicar por qué está.
- **Di qué has usado.** Basta una línea en el pull request, por ejemplo
  «Traducido con una herramienta de IA y revisado frente al original» o «Sin IA».
- **Afirma solo lo que hiciste.** Si no pudiste compilar o probar algo, dilo:
  «sin probar» está bien; «probado» sin haberlo probado, no.
- **La firma es tuya.** `Signed-off-by` certifica, como persona, de dónde viene
  el código (ver [Licencia de las contribuciones](#licencia-de-las-contribuciones)).
  Un programa no puede hacer esa declaración por ti.
- **Comenta en el issue antes de empezar** y espera respuesta, para que dos
  personas no hagan el mismo trabajo.

Los pull requests abiertos por agentes autónomos, o enviados en serie a muchos
proyectos, se cierran sin revisión detallada. Si detrás de uno hay una persona,
que responda en el pull request y se puede reabrir.
