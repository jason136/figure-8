use flume::{Receiver, Sender, unbounded};
use v8::inspector::V8InspectorClient;

pub struct Inspector {
    stdout: Sender<String>,
    stderr: Sender<String>,
}

impl Inspector {
    pub fn new() -> (Self, Receiver<String>, Receiver<String>) {
        let (stdout_tx, stdout_rx) = unbounded();
        let (stderr_tx, stderr_rx) = unbounded();

        (
            Inspector {
                stdout: stdout_tx,
                stderr: stderr_tx,
            },
            stdout_rx,
            stderr_rx,
        )
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
        let msg = message.to_string();
        match level {
            3 | 4 => {
                let _ = self.stderr.send(msg);
            }
            _ => {
                let _ = self.stdout.send(msg);
            }
        }
    }
}
