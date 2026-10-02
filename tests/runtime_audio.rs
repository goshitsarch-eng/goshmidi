//! Explicit live-runtime check; ordinary CI remains independent of audio devices.
use dmidiplayer::core::player::{EngineCommand, PlayerEvent, spawn_engine};
use std::time::{Duration, Instant};
#[test]
#[ignore = "requires an audio server and GOSH_MIDI_TEST_SOUNDFONT"]
fn real_fluidsynth_output_plays_the_fixture() {
    let font =
        std::env::var("GOSH_MIDI_TEST_SOUNDFONT").expect("Set a verified test SoundFont path");
    let (handle, events, worker) = spawn_engine();
    handle.send(EngineCommand::SetSoundFont(font));
    handle.send(EngineCommand::SelectOutput {
        backend: "FluidSynth".into(),
        port: "fluidsynth".into(),
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(Instant::now() < deadline, "Output initialization timed out");
        if let Some(PlayerEvent::OutputChanged {
            backend, warning, ..
        }) = events.recv_lossless_timeout(Duration::from_millis(100))
        {
            assert_eq!(backend, "FluidSynth");
            assert_eq!(warning, None);
            break;
        }
    }
    handle.send(EngineCommand::Load(
        std::fs::canonicalize("examples/test.mid").unwrap(),
    ));
    loop {
        assert!(Instant::now() < deadline, "Song load timed out");
        if let Some(PlayerEvent::Opened(_)) =
            events.recv_lossless_timeout(Duration::from_millis(100))
        {
            break;
        }
    }
    handle.send(EngineCommand::Play);
    let mut notes = 0;
    let mut finished = false;
    while Instant::now() < deadline {
        match events.recv_lossless_timeout(Duration::from_millis(100)) {
            Some(PlayerEvent::NoteOn { .. }) => notes += 1,
            Some(PlayerEvent::Finished) => {
                finished = true;
                break;
            }
            _ => {}
        }
    }
    handle.send(EngineCommand::Shutdown);
    worker.join().unwrap();
    assert!(finished);
    assert!(notes > 0);
}
