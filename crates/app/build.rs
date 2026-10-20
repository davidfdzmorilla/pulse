//! On Windows, builds pulse.exe's resources: its icon (the Explorer's, the
//! taskbar's) and its version (Properties → Details, and what the updater reads
//! from a download). Elsewhere it does nothing.

fn main() {
    println!("cargo:rerun-if-env-changed=PULSE_VERSION");
    println!("cargo:rerun-if-changed=../../packaging/windows/pulse.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    // The resource compiler comes with Visual Studio's tools: checking from macOS
    // (`cargo check --target x86_64-pc-windows-msvc`) goes without.
    if !cfg!(windows) {
        println!("cargo:warning=pulse.exe sin icono ni versión: los recursos de Windows se compilan en Windows");
        return;
    }
    let version = std::env::var("PULSE_VERSION").unwrap_or_else(|_| std::env::var("CARGO_PKG_VERSION").unwrap());
    let mut numbers = version.split(['.', '-']).map(|p| p.parse::<u16>().unwrap_or(0));
    let (major, minor, patch) = (numbers.next().unwrap_or(0), numbers.next().unwrap_or(0), numbers.next().unwrap_or(0));
    let manifest = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let icon = manifest.join("../../packaging/windows/pulse.ico");
    let icon = icon.display().to_string().replace('\\', "\\\\");
    // Numbers instead of winver.h's names, so no SDK headers are needed.
    let rc = format!(
        r#"1 ICON "{icon}"
1 VERSIONINFO
FILEVERSION {major},{minor},{patch},0
PRODUCTVERSION {major},{minor},{patch},0
FILEFLAGSMASK 0x3f
FILEFLAGS 0x0
FILEOS 0x40004
FILETYPE 0x1
FILESUBTYPE 0x0
BEGIN
    BLOCK "StringFileInfo"
    BEGIN
        BLOCK "040904B0"
        BEGIN
            VALUE "CompanyName", "Pulse"
            VALUE "FileDescription", "Pulse"
            VALUE "FileVersion", "{version}"
            VALUE "InternalName", "pulse"
            VALUE "OriginalFilename", "pulse.exe"
            VALUE "ProductName", "Pulse"
            VALUE "ProductVersion", "{version}"
        END
    END
    BLOCK "VarFileInfo"
    BEGIN
        VALUE "Translation", 0x409, 1200
    END
END
"#
    );
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("pulse.rc");
    std::fs::write(&out, rc).unwrap();
    embed_resource::compile(&out, embed_resource::NONE).manifest_optional().unwrap();
}
