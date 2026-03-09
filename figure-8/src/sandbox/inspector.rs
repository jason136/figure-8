use flume::{Receiver, Sender, unbounded};
use rmcp::serde::{Deserialize, Serialize};
use v8::inspector::V8InspectorClient;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsoleMessage {
    pub level: i32,
    pub message: String,
}

pub struct Inspector {
    console_tx: Sender<ConsoleMessage>,
}

impl Inspector {
    pub fn new() -> (Self, Receiver<ConsoleMessage>) {
        let (console_tx, console_rx) = unbounded();

        (Inspector { console_tx }, console_rx)
    }

    pub fn into_inspector_client(self) -> V8InspectorClient {
        V8InspectorClient::new(Box::new(self))
    }
}

impl v8::inspector::V8InspectorClientImpl for Inspector {
    fn console_api_message(
        &self,
        _context_group_id: i32,
        level: i32,
        message: &v8::inspector::StringView,
        _url: &v8::inspector::StringView,
        _line_number: u32,
        _column_number: u32,
        _stack_trace: &mut v8::inspector::V8StackTrace,
    ) {
        let _ = self.console_tx.send(ConsoleMessage {
            level,
            message: message.to_string(),
        });
    }
}
