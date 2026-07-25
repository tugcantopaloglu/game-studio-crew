use studio_core::CliEvent;
use studio_events::EventType;

#[derive(Debug, Default)]
pub struct Stream;

impl Stream {
    pub fn new() -> Self {
        Self
    }

    pub fn observe(
        &mut self,
        _ev: &CliEvent,
        _role: &str,
    ) -> Option<(EventType, serde_json::Value)> {
        None
    }

    pub fn flush(&mut self, _role: &str) -> Option<(EventType, serde_json::Value)> {
        None
    }
}
