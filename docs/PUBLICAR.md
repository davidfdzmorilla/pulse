# Publicar una versión

Las versiones se publican desde `main`, con todo fusionado por pull requests:

```bash
git switch main && git pull
scripts/release.sh            # 0.1.63 → 0.1.64 (minor / major para las otras cifras)
```

El script comprueba que `main` está al día y limpio, que la última etiqueta es
la última publicación, y que está el certificado de firma. Pasa las pruebas
(`--locked`), hace y firma `Pulse.app` (`scripts/bundle.sh`), y lo publica de una
vez: primero como borrador, descarga lo subido y comprueba su SHA-256, su firma y
su versión, y solo entonces lo publica, que es cuando se crea la etiqueta. Si
algo falla antes, borra el borrador: no quedan etiquetas sin publicación ni
números saltados. Las notas son los títulos de los pull requests fusionados.

El paquete de Windows se hace aparte: [BUILD_WINDOWS.md](BUILD_WINDOWS.md).

## El certificado de firma

Pulse se firma con el certificado autofirmado **«Pulse Local Signing»**, que
está en el llavero de inicio de sesión de este Mac. Cada Pulse instalado solo
acepta actualizaciones firmadas con ese mismo certificado (su firma nombra la
huella del certificado, no solo su nombre).

**Si se pierde** (un Mac nuevo, un llavero borrado), ninguna instalación acepta
las versiones nuevas: cada una tendría que bajarse a mano una vez. Para que no
pase, guarda una copia:

1. Acceso a Llaveros → **Mis certificados** → «Pulse Local Signing» (con su
   clave privada) → clic derecho → **Exportar…** → formato `.p12`, con contraseña.
2. Guarda el `.p12` y su contraseña en tu gestor de contraseñas (no en el
   repositorio ni en una carpeta sincronizada sin cifrar).

Para recuperarlo en otro Mac: doble clic en el `.p12` (llavero de inicio de
sesión) y comprueba que aparece:

```bash
security find-identity -p codesigning | grep "Pulse Local Signing"
```

Si se perdiera sin copia: crea otro con el mismo nombre (Acceso a Llaveros →
Asistente para certificados → Crear un certificado, tipo «Firma de código»),
publica, y avisa de que esa versión hay que descargarla a mano.

## Compilar igual siempre

- `rust-toolchain.toml` fija la versión de Rust: rustup la instala sola.
- `--locked`: las dependencias que dice `Cargo.lock`, no otras más nuevas.
- ONNX Runtime (para stems y audio a MIDI) lo descarga `ort` de `cdn.pyke.io`
  la primera vez y lo guarda en `~/Library/Caches/ort.pyke.io`. Sin red y sin
  esa caché no compila: copia esa carpeta de otro Mac, o descarga el paquete y
  apunta `ORT_LIB_LOCATION` a la carpeta con `libonnxruntime.a`.
- Está compilado para macOS 13.4 o posterior, y por eso Pulse pide ese mínimo
  (`.cargo/config.toml` y `Info.plist`).

## Versiones

La versión de una publicación es su etiqueta (`release.sh` la pasa como
`PULSE_VERSION`). Un Pulse compilado a mano se llama por git: la última
publicación y cuánto la pasa («0.1.63-4-gabc1234»), y su informe de fallos dice
el commit. El número de `Cargo.toml` no se usa.
