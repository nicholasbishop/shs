use anyhow::Error;
use fehler::throws;
use serde::Serialize;
use shs::{serve, Request, Routes};
use std::sync::Arc;

#[derive(Clone)]
struct State {
    name: String,
}

#[derive(Serialize)]
struct MyResp {
    name: String,
    value: String,
}

#[throws]
fn get_value(req: &mut Request<State>) {
    let value = req.path_param("value")?;
    req.send_json(MyResp {
        name: req.state().name.clone(),
        value,
    })?;
}

#[throws]
fn main() {
    let mut routes = Routes::new();
    routes.add("GET /value/:value", &get_value)?;
    serve(
        "127.0.0.1:1234",
        routes,
        Arc::new(State {
            name: "hello-example".into(),
        }),
    )?;
}
