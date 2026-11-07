//! `log.*` (§9.1) is routed to `tracing` with target `script`. Kept in its own test binary so
//! no other test thread races the scoped subscriber.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::Write;
use std::sync::{Arc, Mutex};

use pkrs_core::Pcg64Rng;
use pkrs_script::{LuaScriptHost, MemoryScripts, ScriptHost, ScriptLimits, ScriptOwner};

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Write for Capture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn log_is_routed_to_tracing_with_the_script_target() {
    let capture = Capture::default();
    let writer = capture.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .finish();
    tracing::subscriber::with_default(subscriber, || {
        let mut h = LuaScriptHost::new(
            Box::new(MemoryScripts::new()),
            Box::new(Pcg64Rng::from_seed(1)),
            ScriptLimits::default(),
        )
        .unwrap();
        h.run_source(
            &ScriptOwner::plugin("day-night"),
            "plugins/day-night/init.lua",
            "log.info('dawn', 6)\nlog.warn('late')\nlog.error('broken')\nlog.debug('detail')",
        )
        .unwrap();
        assert!(
            h.run_source(&ScriptOwner::Game, "x.lua", "log.info()")
                .is_ok(),
            "no arguments is fine"
        );
    });
    let out = String::from_utf8(capture.0.lock().unwrap().clone()).unwrap();
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 5, "{out}");
    for (line, (level, text, at)) in lines.iter().zip([
        ("INFO", "dawn\t6", "plugins/day-night/init.lua:1"),
        ("WARN", "late", "plugins/day-night/init.lua:2"),
        ("ERROR", "broken", "plugins/day-night/init.lua:3"),
        ("DEBUG", "detail", "plugins/day-night/init.lua:4"),
    ]) {
        assert!(line.contains(level), "{line}");
        assert!(line.contains(" script: "), "target `script`: {line}");
        assert!(line.contains(text), "{line}");
        assert!(line.contains(at), "{line}");
        assert!(line.contains("plugin `day-night`"), "{line}");
    }
    assert!(lines[4].contains("the game"), "{}", lines[4]);
}
