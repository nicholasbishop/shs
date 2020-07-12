use anyhow::Error;
use fehler::throws;
use serde::Serialize;
use shs::{Request, Server};

#[derive(Serialize)]
struct Resp {
    name: String,
}

#[throws]
fn handler(req: &mut Request) {
    req.write_json(&Resp {
        name: "hello".into(),
    })?;
}

#[throws]
fn main() {
    simple_logging::log_to_stderr(log::LevelFilter::Info);

    let mut server = Server::new("127.0.0.1:1234")?;
    server.route("GET /hello", &handler)?;
    server.launch()?;
}
