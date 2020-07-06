use anyhow::Error;
use fehler::throws;
use serde::Serialize;
use shs::{Request, Server};

struct State {
    name: String,
}

#[derive(Serialize)]
struct Resp {
    name: String,
}

#[throws]
fn handler(req: &mut Request<State>) {
    let name = req.with_state(|s| s.name.clone())?;
    req.write_json(&Resp { name })?;
}

#[throws]
fn main() {
    simple_logging::log_to_stderr(log::LevelFilter::Info);

    let state = State {
        name: "hello-example".into(),
    };
    let mut server = Server::new("127.0.0.1:1234", state)?;
    server.route("GET /hello", &handler)?;
    server.launch()?;
}
