use kairo_core::engine::{EngineHandle, EngineOptions};

#[test]
fn constructing_services_does_not_launch_an_engine() {
    let engine = EngineHandle::new(EngineOptions::new("/not-a-real-kairo-engine"));
    assert_eq!(engine.pid(), None);
    // Shutdown before startup must leave the engine unlaunched even if a
    // delayed startup callback arrives afterwards.
    engine.shutdown();
    engine.start();
    assert_eq!(engine.pid(), None);
}
