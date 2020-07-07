use anyhow::Error;
use fehler::throws;
use serde::{Deserialize, Serialize};
use shs::{Request, Server};
use std::collections::HashMap;

#[derive(Default)]
struct State {
    dict: HashMap<String, String>,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
struct DictItem {
    key: String,
    value: String,
}

impl DictItem {
    fn new(key: &str, value: &str) -> DictItem {
        DictItem {
            key: key.into(),
            value: value.into(),
        }
    }
}

#[throws]
fn get_dict(req: &mut Request<State>) {
    let key: String = req.path_param("key")?;
    if let Some(value) = req.with_state(|s| s.dict.get(&key).cloned())? {
        req.write_json(&DictItem::new(&key, &value))?;
    } else {
        req.set_not_found();
    }
}

#[throws]
fn post_dict(req: &mut Request<State>) {
    let body: DictItem = req.read_json()?;
    req.with_state_mut(|s| {
        s.dict.insert(body.key.clone(), body.value.clone())
    })?;
}

#[throws]
fn create_server() -> Server<State> {
    let mut server = Server::new("127.0.0.1:1234", State::default())?;
    server.route("GET /dict/:key", &get_dict)?;
    server.route("POST /dict", &post_dict)?;
    server
}

#[throws]
fn main() {
    simple_logging::log_to_stderr(log::LevelFilter::Info);

    let server = create_server()?;
    server.launch()?;
}

#[cfg(test)]
mod tests {
    use super::*;
    use shs::{StatusCode, TestRequest};

    #[throws]
    #[test]
    fn test_server() {
        let server = create_server()?;

        // Not found
        let resp = server.test_request(&TestRequest::new("GET /dict/a")?)?;
        assert_eq!(resp.status, StatusCode::NotFound);

        // Add an item
        let resp = server.test_request(&TestRequest::new_with_json(
            "POST /dict",
            &DictItem::new("a", "b"),
        )?)?;
        assert_eq!(resp.status, StatusCode::Ok);

        // Found
        let resp = server.test_request(&TestRequest::new("GET /dict/a")?)?;
        assert_eq!(resp.status, StatusCode::Ok);
        assert_eq!(resp.json::<DictItem>()?, DictItem::new("a", "b"));
    }
}
