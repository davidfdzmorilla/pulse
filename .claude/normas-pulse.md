# Normas de Pulse para los agentes

Pulse es un DAW en Rust para productores de música electrónica.

- **Crates:** `app` (interfaz egui, binario `pulse`), `engine` (audio en tiempo real), `stems`, `beats`, `paths` y `firma`.
- **La guía de usuario:** `docs/guia.md`. Se ve dentro de la app; su formato está explicado en la cabecera de `crates/app/src/guide.rs`.

## Lo que no se hace nunca

- **Abrir ventanas de la app** ni lanzar `target/release/pulse` con interfaz: eso incluye `scripts/guion.sh` y `PULSE_SCREENSHOT`.
  Para ver la interfaz, usa en una prueba el render sin ventana:
  - `PulseApp::headless(project)`;
  - `egui::Context::run_ui` con eventos simulados;
  - `crate::offscreen::Canvas` para sacar PNG.

  Para oír, usa el render offline: `crate::export::render` en pruebas, o `pulse --render salida.wav proyecto.pulse`.
- **`cargo fmt`.** El código tiene estilo a mano; respétalo.
- **`sed -i`.** Edita con la herramienta Edit o con Python.
- **Tocar el llavero, los certificados, `scripts/release.sh` o la clave de firma** (`~/.pulse-firma`).
- **Hacer push.** Haz commit en tu rama y nada más.

## Cómo se trabaja

- **Pruebas:** siempre en release, `cargo test --release` (en debug, las pruebas de interfaz fallan). Antes de terminar, pasa las del crate que toques, y si tocas el motor, también `-p app`.
- **Windows:** comprueba que sigue compilando con `cargo check --release --target x86_64-pc-windows-msvc --workspace --all-targets`. El único aviso esperado es el de llvm-rc.
- **Comentarios** en inglés, cortos y útiles. **Textos de la interfaz y de la guía** en español de España, de tú y directos.
- **Commits:** el título en español, porque acaba en las notas de versión y tiene que entenderlo un usuario. Termina el mensaje con `Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>`.
- **Guía:** cada función nueva o cambio visible actualiza su tema en `docs/guia.md`. Las pruebas de `guide.rs` comprueban los enlaces y los atajos.
- **El hilo de audio nunca pide memoria** ni bloquea. Hay una prueba de estrés que lo vigila: `crates/engine/tests/stress.rs`.
- **El sonido no depende del tamaño de búfer** y se repite igual en cada render: hay pruebas de ambas cosas.
- **No inventes:** si dudas de cómo funciona algo, lee el código. Si encuentras un fallo fuera de tu encargo, no lo arregles: apúntalo en el informe.

## Informe final

En español, breve, para alguien que no ha visto tu trabajo:

- qué has hecho;
- qué has comprobado y cómo;
- qué queda pendiente;
- qué has encontrado de paso;
- el commit y la rama;
- las rutas completas de los WAV o PNG que haya que mirar o escuchar.
