struct State {
    name: String,
}

#[derive(serde::Serialize)]
struct Resp {
    name: String,
}

#[fehler::throws(anyhow::Error)]
fn handler(req: &mut shs::Request<State>) {
    let name = req.with_state(|s| s.name.clone())?;
    req.write_json(&Resp { name })?;
}

#[fehler::throws(anyhow::Error)]
fn main() {
    simple_logging::log_to_stderr(log::LevelFilter::Info);

    let mut routes = shs::Routes::new();
    routes.add("GET /hello", &handler)?;
    shs::serve(
        "127.0.0.1:1234",
        routes,
        std::sync::Arc::new(std::sync::RwLock::new(State {
            name: "hello-example".into(),
        })),
    )?;
}
