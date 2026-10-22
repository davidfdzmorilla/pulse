//! `pulse --scan-plugin <bundle>`, the process a plugin search loads each
//! bundle in: the real binary with Pulse's test plugin, and with a bundle that
//! isn't one.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Pulse's test plugin, built and put in a bundle of its own.
fn test_bundle() -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let target = root.join("target/testplug");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let status = Command::new(cargo).args(["build", "--release", "-p", "pulse-testplug", "--target-dir"]).arg(&target).current_dir(&root).status().expect("cargo build");
    assert!(status.success());
    let library = if cfg!(target_os = "macos") { "libpulse_testplug.dylib" } else if cfg!(windows) { "pulse_testplug.dll" } else { "libpulse_testplug.so" };
    let bundle = target.join("scan-plugins/PulseTest.vst3");
    let binary = engine::plugin::binary(&bundle);
    std::fs::create_dir_all(binary.parent().unwrap()).unwrap();
    std::fs::copy(target.join("release").join(library), &binary).unwrap();
    bundle
}

fn scan(bundle: &Path) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_pulse")).arg("--scan-plugin").arg(bundle).output().expect("pulse");
    assert!(output.status.success(), "{output:?}");
    String::from_utf8_lossy(&output.stdout).lines().rev().find_map(|l| l.strip_prefix("PULSE-SCAN ").map(str::to_string)).expect("an answer")
}

#[test]
fn pulse_scans_a_plugin_in_its_own_process() {
    let bundle = test_bundle();
    let answer: serde_json::Value = serde_json::from_str(&scan(&bundle)).unwrap();
    let found = answer["found"].as_array().expect("found");
    assert!(!found.is_empty(), "{answer}");
    assert_eq!(found[0]["path"].as_str(), Some(bundle.to_str().unwrap()));
    // Not a plugin: said, not a crash.
    let dir = std::env::temp_dir().join(format!("pulse-scan-bad-{}", std::process::id()));
    let fake = engine::plugin::binary(&dir.join("Falso.vst3"));
    std::fs::create_dir_all(fake.parent().unwrap()).unwrap();
    std::fs::write(&fake, b"no soy una biblioteca").unwrap();
    let answer: serde_json::Value = serde_json::from_str(&scan(&dir.join("Falso.vst3"))).unwrap();
    std::fs::remove_dir_all(&dir).ok();
    assert!(answer["error"].as_str().is_some_and(|e| !e.is_empty()), "{answer}");
}
