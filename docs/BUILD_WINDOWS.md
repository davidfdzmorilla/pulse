# Compilar Pulse en Windows

Instrucciones para compilar, probar y empaquetar Pulse en Windows x64 desde
cero. Estado del soporte: [WINDOWS_SUPPORT.md](WINDOWS_SUPPORT.md). Estas
instrucciones siguen lo que hace la CI (`.github/workflows/windows.yml`); aún no
se han ejecutado en un equipo Windows.

## Requisitos

| Qué | Versión | Para qué |
|---|---|---|
| Windows | 10 1809 o posterior, o 11; x64 | Compilar y ejecutar |
| Visual Studio 2022 Build Tools (o Visual Studio 2022) | carga de trabajo «Desarrollo para el escritorio con C++»: MSVC v143 y Windows 11 SDK | Enlazador, `rc.exe` (icono y versión de `pulse.exe`), runtime de Visual C++ que se empaqueta |
| Rust | estable, con `x86_64-pc-windows-msvc` (lo que instala `rustup` en Windows x64) | Compilar |
| Git | cualquiera reciente | Clonar |
| Inno Setup | 6 | Solo para el instalador |
| Conexión a internet | la primera compilación | `ort` descarga ONNX Runtime precompilado de `cdn.pyke.io` |

No hace falta CMake, ni el SDK de ASIO, ni instalar ONNX Runtime aparte.

Con `winget`:

```powershell
winget install Microsoft.VisualStudio.2022.BuildTools --override "--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
winget install Rustlang.Rustup
winget install Git.Git
winget install JRSoftware.InnoSetup   # opcional: el instalador
```

## Compilar y ejecutar

```powershell
git clone https://github.com/davidfdzmorilla/pulse.git
cd pulse
git config core.longpaths true
cargo build --release
.\target\release\pulse.exe
```

`cargo run --release` también vale. Las compilaciones de `cargo` se reconocen
porque no son una versión publicada: no se actualizan solas.

### Opciones de línea de órdenes

`pulse.exe` es una aplicación de ventana, sin consola. Las opciones que
escriben algo usan la consola desde la que se lanzan:

```powershell
.\target\release\pulse.exe --render salida.wav [proyecto.pulse]   # render offline
.\target\release\pulse.exe --check-editors                        # prueba el editor de cada plugin instalado
```

## Pruebas

```powershell
cargo test --release --workspace
```

En `release`: en `debug` las pruebas de interfaz fallan por una aserción de egui
(`TexturesDelta`). No hace falta tarjeta de sonido, MIDI, GPU ni internet. Las
pruebas del alojamiento de plugins compilan `crates/testplug` y lo cargan como
VST3 y CLAP.

Las pruebas marcadas `#[ignore]` son diagnósticos. Necesitan dispositivos o
modelos externos, o son de escucha:

```powershell
cargo test --release -p engine list_devices -- --ignored --nocapture   # dispositivos de audio
```

## Empaquetar

```powershell
pwsh scripts/package-windows.ps1                 # versión de Cargo
pwsh scripts/package-windows.ps1 -Version 0.2.0  # una versión concreta
```

Deja en `dist\`:

- `Pulse\`: `pulse.exe`, el runtime de Visual C++ (`vcruntime140*.dll`,
  `msvcp140*.dll`…), `LICENSE.txt` (la GPL) y `THIRD_PARTY_NOTICES.txt`;
- `Pulse-<versión>-windows-x64.zip`: esa carpeta, comprimida (la copia portable
  y lo que descarga el actualizador);
- `Pulse-<versión>-windows-x64-setup.exe`: el instalador, si está Inno Setup 6;
- `dependencias.txt`: las DLL que carga `pulse.exe` (`dumpbin /dependents`).
  `DirectML.dll` y `DXCORE.dll` no deben salir entre las normales, solo entre
  las de carga diferida.

El script comprueba que `pulse.exe` dice ser la versión pedida, porque el
actualizador lo comprueba.

## Publicar una versión para Windows

`scripts/release.sh` (en macOS) crea la etiqueta y la publicación con el zip de
macOS. El zip de Windows se añade a esa misma publicación:

```powershell
git checkout v0.2.0
pwsh scripts/package-windows.ps1 -Version 0.2.0
gh release upload v0.2.0 dist\Pulse-0.2.0-windows-x64.zip dist\Pulse-0.2.0-windows-x64-setup.exe
```

El nombre importa: el actualizador de Windows busca el archivo que acaba en
`-windows-x64.zip`, y el de macOS ignora los que llevan `-windows-`. GitHub
calcula la huella SHA-256 de cada archivo subido. El actualizador de Windows no
instala nada sin ella.

La CI también puede hacer el paquete. En Actions → Windows → Run workflow, con
la versión: los archivos quedan como artefactos del trabajo y no se publican.

## Problemas frecuentes

- **`link.exe` no encontrado / `LNK1181`**: faltan las herramientas de C++ de
  Visual Studio o el Windows SDK. Instálalos con la carga de trabajo de C++.
- **`embed-resource`/`rc.exe` falla**: falta el Windows SDK. El icono y la
  versión de `pulse.exe` se compilan con él.
- **La descarga de ONNX Runtime falla** (`ort-sys`): hace falta acceso a
  `cdn.pyke.io` en la primera compilación. Sin red, descarga el paquete en otro
  sitio y apunta `ORT_LIB_LOCATION` a la carpeta con `onnxruntime.lib`.
- **`pulse.exe` no arranca, «falta VCRUNTIME140.dll»**: ejecuta el de
  `dist\Pulse` (lleva el runtime), o instala el Visual C++ Redistributable x64.
- **`pulse.exe` no arranca, falta `DirectML.dll`**: no debería pasar (se carga en
  diferido, ver `.cargo/config.toml`). Si pasa, mira `dist\dependencias.txt` y
  avisa.
- **Compilaciones muy lentas**: excluye la carpeta del repositorio del análisis en
  tiempo real de Microsoft Defender.
- **Rutas largas**: `git config core.longpaths true`, y si hace falta activa
  `LongPathsEnabled` en el registro.

## Enlazar para Windows desde macOS

Sirve para ver que todo compila y enlaza sin un PC con Windows, no para
ejecutarlo. Usa `lld-link` en vez del `link.exe` de Visual Studio, y descarga el
CRT y el SDK de Windows: aceptas la licencia de Microsoft.

```bash
brew install llvm lld
cargo install cargo-xwin --locked
rustup target add x86_64-pc-windows-msvc
export PATH="$(brew --prefix llvm)/bin:$(brew --prefix lld)/bin:$PATH"
XWIN_ACCEPT_LICENSE=1 cargo xwin build --release --target x86_64-pc-windows-msvc --workspace --all-targets
llvm-readobj --coff-imports target/x86_64-pc-windows-msvc/release/pulse.exe   # las DLL que carga
```

`llvm-rc` compila el icono y la versión. Sin él en el `PATH`, `pulse.exe` sale
sin ellos y la compilación lo avisa.

## Estilo y lint

El código mantiene su propio estilo, sin `cargo fmt`, porque reescribiría todo
el repositorio. La CI pasa `cargo clippy` solo como información: sus avisos no
bloquean.

## Integración continua

`.github/workflows/windows.yml` corre en cada pull request, en cada push a `main`
y a mano. En `windows-2022`:

1. Instala Rust estable y restaura la caché de compilación.
2. Compila el workspace entero (`--all-targets`).
3. Pasa `cargo clippy` (informativo).
4. Pasa `cargo test --release --workspace`.
5. Instala Inno Setup y empaqueta (zip, instalador y dependencias).
6. Renderiza el proyecto de ejemplo con `pulse.exe --render` (con espacios y «ñ»
   en la ruta).
7. Arranca la ventana de Pulse con el guion `scripts/ci/arranque.txt`, que hace
   capturas (paso informativo: el equipo no tiene GPU).
8. Instala en silencio, comprueba el acceso del menú Inicio y la asociación de
   `.pulse`, desinstala y comprueba que los ajustes del usuario siguen.
9. Guarda como artefactos los paquetes, los registros y las capturas.

No publica nada, no usa secretos y no necesita dispositivos de audio ni MIDI.
