//! Audio the tests use, made by Pulse itself (nothing to download or keep
//! beside the code).

use std::path::PathBuf;
use std::sync::OnceLock;

/// The tests' small song (`Project::demo`, 120 BPM, eight bars and its tail)
/// as a 24-bit, 48 kHz WAV: rendered the first time a test asks for it in a run.
pub fn demo_song() -> PathBuf {
    static SONG: OnceLock<PathBuf> = OnceLock::new();
    SONG.get_or_init(|| {
        let path = std::env::temp_dir().join(format!("pulse-demo-song-{}.wav", std::process::id()));
        crate::export::render_project(&path, &crate::project::Project::demo()).expect("the demo song renders");
        path
    })
    .clone()
}

/// Calls `step` until it says it is done, for at most `seconds`: else a
/// clear failure (a background thread that hung or died) instead of a test
/// that never ends.
pub fn wait_for(what: &str, seconds: u64, mut step: impl FnMut() -> bool) {
    let started = std::time::Instant::now();
    while !step() {
        assert!(started.elapsed().as_secs() < seconds, "{what}: sin terminar tras {seconds} s (¿un hilo se colgó o murió?)");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[test]
fn the_demo_song_is_the_demo_rendered() {
    let song = engine::Sample::load(&demo_song()).unwrap();
    assert_eq!(song.sample_rate, 48_000.0);
    // 16 s of song and the effects' tail.
    assert!((16.0..20.0).contains(&song.duration()), "{} s", song.duration());
    assert!(crate::export::peak(&song.left, &song.right) > 0.1);
}
