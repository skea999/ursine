//! `ursine` — the GUI (libcosmic / COSMIC toolkit).
//!
//! v0.3 spike: minimal window + one protocol round-trip against `ursined`
//! (`capabilities.list`; unauthenticated on purpose — the expected answer is
//! `unauthorized`, which proves the socket path end to end). The login flow
//! and screens arrive next.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::time::Duration;

use cosmic::app::Settings;
use cosmic::{Element, Task};
use ursine_api::{Request, RequestBody, Response, ResponseBody, PROTOCOL_VERSION};

fn socket_path() -> PathBuf {
    std::env::var_os("URSINE_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/run/ursine.sock"))
}

/// One request/response round-trip (blocking; called from a Task).
async fn probe_daemon() -> String {
    let path = socket_path();
    let line = serde_json::to_string(&Request {
        v: PROTOCOL_VERSION,
        id: Some(1),
        token: None,
        body: RequestBody::Call {
            module: ursine_api::ModuleId::Capabilities,
            action: ursine_api::Action::List,
            as_user: None,
        },
    });
    let Ok(line) = line else {
        return "client error: cannot encode request".to_string();
    };

    let path_for_blocking = path.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<String, String> {
        use std::os::unix::net::UnixStream;
        let mut stream = UnixStream::connect(&path_for_blocking).map_err(|e| {
            format!(
                "daemon not reachable ({}): {e}",
                path_for_blocking.display()
            )
        })?;
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .map_err(|e| e.to_string())?;
        stream
            .write_all(format!("{line}\n").as_bytes())
            .map_err(|e| e.to_string())?;
        let mut response = String::new();
        BufReader::new(stream)
            .read_line(&mut response)
            .map_err(|e| e.to_string())?;
        Ok(response)
    })
    .await;

    match result {
        Ok(Ok(raw)) => match serde_json::from_str::<Response>(raw.trim()) {
            Ok(resp) => match resp.body {
                ResponseBody::Ok { result } => match result.as_array() {
                    Some(items) => format!("daemon online — {} capabilities", items.len()),
                    None => "daemon online (unexpected payload)".to_string(),
                },
                ResponseBody::Err { error } => {
                    format!("daemon online — {:?}: {}", error.code, error.message)
                }
                ResponseBody::AuthOk { .. } => "daemon online (auth response)".to_string(),
            },
            Err(e) => format!("daemon replied with unparsable line: {e}"),
        },
        Ok(Err(e)) => e,
        Err(e) => format!("task join error: {e}"),
    }
}

struct Ursine {
    core: cosmic::Core,
    status: String,
}

#[derive(Debug, Clone)]
enum Message {
    DaemonStatus(String),
}

impl cosmic::Application for Ursine {
    type Executor = cosmic::executor::Default;
    type Flags = ();
    type Message = Message;

    const APP_ID: &'static str = "org.feralos.Ursine";

    fn core(&self) -> &cosmic::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut cosmic::Core {
        &mut self.core
    }

    fn init(
        core: cosmic::Core,
        _flags: Self::Flags,
    ) -> (Self, Task<cosmic::Action<Self::Message>>) {
        let app = Self {
            core,
            status: "connecting to ursined…".to_string(),
        };
        let task = Task::future(async move { Message::DaemonStatus(probe_daemon().await) })
            .map(cosmic::Action::App);
        (app, task)
    }

    fn update(&mut self, message: Self::Message) -> Task<cosmic::Action<Self::Message>> {
        match message {
            Message::DaemonStatus(status) => self.status = status,
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Self::Message> {
        // iced 0.14 (libcosmic fork): the `column` helper takes its children.
        cosmic::widget::column(vec![
            cosmic::widget::text("Ursine").size(24).into(),
            cosmic::widget::text(self.status.clone()).into(),
        ])
        .spacing(8)
        .padding(24)
        .into()
    }
}

fn main() -> cosmic::iced::Result {
    cosmic::app::run::<Ursine>(Settings::default(), ())
}
