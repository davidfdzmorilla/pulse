//! `pulse-firma`: makes Pulse's signing key and signs (and checks) what is
//! published for Windows. See docs/PUBLICAR.md.
//!   pulse-firma clave <archivo>      a new private key, in a file only you can read
//!   pulse-firma firmar <clave> <zip> writes <zip>.firma
//!   pulse-firma comprobar <zip>      checks <zip>.firma against Pulse's key

use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let result = match args.get(1).map(String::as_str) {
        Some("clave") if args.len() == 3 => new_key(Path::new(&args[2])),
        Some("firmar") if args.len() == 4 => sign(Path::new(&args[2]), Path::new(&args[3])),
        Some("comprobar") if args.len() == 3 => check(Path::new(&args[2])),
        _ => Err("Uso: pulse-firma clave <archivo> | firmar <clave> <zip> | comprobar <zip>".into()),
    };
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn signature_of(zip: &Path) -> std::path::PathBuf {
    let mut name = zip.as_os_str().to_owned();
    name.push(".firma");
    name.into()
}

fn read_key(path: &Path) -> Result<[u8; 32], String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("No se pudo leer la clave {}: {e}", path.display()))?;
    firma::unhex::<32>(&text).ok_or_else(|| format!("{} no es una clave de pulse-firma", path.display()))
}

/// 32 random bytes from the system.
#[cfg(unix)]
fn random() -> Result<[u8; 32], String> {
    use std::io::Read;
    let mut bytes = [0u8; 32];
    std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut bytes)).map_err(|e| format!("No se pudo leer /dev/urandom: {e}"))?;
    Ok(bytes)
}

#[cfg(not(unix))]
fn new_key(_: &Path) -> Result<(), String> {
    Err("La clave se crea en macOS (pulse-firma clave): aquí solo se firma y se comprueba".into())
}

#[cfg(unix)]
fn new_key(path: &Path) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let secret = random()?;
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    // Never over another key; readable by its owner only.
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(path).map_err(|e| format!("No se pudo crear {}: {e}", path.display()))?;
    file.write_all(format!("{}\n", firma::hex(&secret)).as_bytes()).map_err(|e| e.to_string())?;
    let public = firma::public_of(&secret);
    println!("Clave privada: {}", path.display());
    println!("Clave pública: {}", firma::hex(&public));
    println!("Para crates/firma/src/lib.rs:\npub const PUBLIC_KEY: [u8; 32] = {public:?};");
    Ok(())
}

fn sign(key: &Path, zip: &Path) -> Result<(), String> {
    let secret = read_key(key)?;
    if firma::public_of(&secret) != firma::PUBLIC_KEY {
        return Err("Esa clave no es la de Pulse (su parte pública no es la que lleva Pulse): las copias instaladas rechazarían la firma".into());
    }
    let data = std::fs::read(zip).map_err(|e| format!("No se pudo leer {}: {e}", zip.display()))?;
    let signature = firma::sign(&secret, &data);
    firma::verify(&firma::PUBLIC_KEY, &data, &signature)?;
    let out = signature_of(zip);
    std::fs::write(&out, signature).map_err(|e| format!("No se pudo escribir {}: {e}", out.display()))?;
    println!("{}", out.display());
    Ok(())
}

fn check(zip: &Path) -> Result<(), String> {
    let data = std::fs::read(zip).map_err(|e| format!("No se pudo leer {}: {e}", zip.display()))?;
    let path = signature_of(zip);
    let signature = std::fs::read_to_string(&path).map_err(|e| format!("No se pudo leer {}: {e}", path.display()))?;
    firma::verify(&firma::PUBLIC_KEY, &data, &signature)?;
    println!("{}: firmado por Pulse", zip.display());
    Ok(())
}
