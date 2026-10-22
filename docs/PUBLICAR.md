# Publicar una versión

Las versiones se publican desde `main`, con todo fusionado por pull requests:

```bash
git switch main && git pull
scripts/release.sh            # 0.1.63 → 0.1.64 (minor / major para las otras cifras)
```

El script comprueba que `main` está al día y limpio, que la última etiqueta es
la última publicación, y que está el certificado de firma. Pasa las pruebas
(`--locked`), hace y firma `Pulse.app` (`scripts/bundle.sh`) con sus símbolos de
depuración aparte (ver «Leer un informe de fallo»), y lo publica de una
vez: primero como borrador, descarga lo subido y comprueba su SHA-256, su firma
(y que es de «Pulse Local Signing»), su versión y la del binario
(`pulse --version`), y solo entonces lo publica, que es cuando se crea la
etiqueta. Si algo falla antes, también al crear el borrador, o lo paras con
Ctrl-C, borra el borrador: no quedan etiquetas sin publicación ni números
saltados. Las notas son los títulos de los pull requests fusionados.

`bundle.sh` coge el binario de donde lo haya dejado Cargo (también con
`CARGO_TARGET_DIR`) y comprueba que dice la versión que se publica, así que
nunca empaqueta uno viejo. Las rutas de este Mac (el repositorio, el registro de
Cargo) se escriben relativas al compilar y el binario va sin sus símbolos de
depuración (`strip -S`, están en el dSYM): no lleva ninguna ruta de tu carpeta
personal, y si la llevara, `bundle.sh` para. Los zips van sin los atributos
extendidos de macOS, que fuera de él salen como archivos `._`.

El paquete de Windows se hace aparte: [BUILD_WINDOWS.md](BUILD_WINDOWS.md).

## Leer un informe de fallo

Si Pulse se cierra de golpe en macOS, queda un informe en Consola → Informes de
fallos (`~/Library/Logs/DiagnosticReports/pulse-*.ips`). Cada publicación lleva
sus símbolos de depuración (`Pulse-X.Y.Z-simbolos.zip`, que `release.sh` sube y
guarda también en `~/Library/Caches/Pulse/simbolos/X.Y.Z`); con ellos:

```bash
python3 scripts/simbolos.py pulse-2026-10-21-101500.ips
```

dice, hilo por hilo (el que falló primero), cada función de Pulse con su
archivo y línea, y lo que estaba expandido dentro. Si no tiene los símbolos de
esa versión, los descarga de su publicación. Los símbolos salen del perfil
`dist` de `Cargo.toml` (el de publicar): las compilaciones de cada día y las
pruebas no cambian.

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

## La firma de las versiones de Windows

En Windows no hay firma de código propia (como el certificado de macOS), así que
cada zip de Windows lleva una firma Ed25519 de Pulse al lado,
`Pulse-X.Y.Z-windows-x64.zip.firma`. El actualizador de Windows no instala un zip
sin ella o con una que no sea de la clave de Pulse: con la cuenta de GitHub sola
(su contraseña, un token) no se puede poner un programa en los ordenadores de
quien usa Pulse.

- **La clave privada** está en `~/.pulse-firma/clave-actualizaciones`, fuera del
  repositorio y del llavero, legible solo por tu usuario. **Haz una copia** en
  un sitio seguro (un disco cifrado, un gestor de contraseñas: son 64 caracteres
  de texto). Si se pierde, las copias instaladas no aceptarán más
  actualizaciones de Windows hasta que se instale a mano una versión con una
  clave nueva. Si alguien la copia, puede firmar como Pulse: entonces cambia la
  clave y publica una versión con la nueva.
- **La clave pública** va dentro de Pulse (`crates/firma/src/lib.rs`,
  `PUBLIC_KEY`).
- **Firmar:** `cargo run --release -p firma -- firmar ~/.pulse-firma/clave-actualizaciones <zip>`
  escribe `<zip>.firma`, y antes comprueba que la clave es la de Pulse.
  **Comprobar:** `cargo run --release -p firma -- comprobar <zip>`.
- **Una clave nueva** (si se pierde o se filtra): `pulse-firma clave <archivo>`
  la crea, sin sobrescribir ninguna, y escribe la línea de `PUBLIC_KEY` para
  `lib.rs`.

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
