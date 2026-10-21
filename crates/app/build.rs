//! On Windows, builds pulse.exe's resources: its icon (the Explorer's, the
//! taskbar's) and its version (Properties → Details, and what the updater reads
//! from a download). Elsewhere it does nothing.

fn main() {
    println!("cargo:rerun-if-env-changed=PULSE_VERSION");
    // Builds of our own are named after git: the last release and how far past
    // it ("0.1.63-5-gabc1234"), not Cargo's number, which isn't kept up. And the
    // commit, for the error log.
    for file in ["HEAD", "packed-refs", "refs/tags"] {
        println!("cargo:rerun-if-changed=../../.git/{file}");
    }
    let git = |args: &[&str]| {
        std::process::Command::new("git").args(args).output().ok().filter(|o| o.status.success()).and_then(|o| String::from_utf8(o.stdout).ok()).map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
    };
    if let Some(described) = git(&["describe", "--tags", "--match", "v[0-9]*"]) {
        println!("cargo:rustc-env=PULSE_GIT_VERSION={}", described.trim_start_matches('v'));
    }
    if let Some(commit) = git(&["rev-parse", "--short=10", "HEAD"]) {
        println!("cargo:rustc-env=PULSE_COMMIT={commit}");
    }
    println!("cargo:rerun-if-changed=../../packaging/windows/pulse.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
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
    // On Windows the resource compiler is Visual Studio's rc.exe. From macOS
    // (`cargo xwin`, `cargo check --target x86_64-pc-windows-msvc`) it is LLVM's
    // llvm-rc when it's on the PATH; without it pulse.exe goes without them.
    match embed_resource::compile(&out, embed_resource::NONE) {
        embed_resource::CompilationResult::NotAttempted(why) if !cfg!(windows) => {
            println!("cargo:warning=pulse.exe sin icono ni versión (falta llvm-rc): {why}");
        }
        result => result.manifest_required().unwrap(),
    }
}
