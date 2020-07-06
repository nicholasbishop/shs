use anyhow::Error;
use fehler::throws;
use serde::{Deserialize, Serialize};
use shs::{serve, Request, Routes};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

#[derive(Clone)]
struct State {
    dict: HashMap<String, String>,
}

#[derive(Serialize)]
struct GetDictResp {
    key: String,
    value: String,
}

#[throws]
fn get_dict(req: &mut Request<State>) {
    let key: String = req.path_param("key")?;
    if let Some(value) = req.with_state(|s| s.dict.get(&key).cloned())? {
        req.write_json(&GetDictResp {
            key: key.clone(),
            value: value.to_string(),
        })?;
    } else {
        req.set_not_found();
    }
}

#[derive(Deserialize)]
struct PostDictReq {
    key: String,
    value: String,
}

#[throws]
fn post_dict(req: &mut Request<State>) {
    let body: PostDictReq = req.read_json()?;
    req.with_state_mut(|s| {
        s.dict.insert(body.key.clone(), body.value.clone())
    })?;
}

#[throws]
fn main() {
    simple_logging::log_to_stderr(log::LevelFilter::Info);

    let mut routes = Routes::new();
    routes.add("GET /dict/:key", &get_dict)?;
    routes.add("POST /dict", &post_dict)?;
    serve(
        "127.0.0.1:1234",
        routes,
        Arc::new(RwLock::new(State {
            dict: HashMap::new(),
        })),
    )?;
}
