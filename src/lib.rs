use anyhow::{anyhow, Context, Error};
use bufstream::BufStream;
use fehler::{throw, throws};
pub use http::StatusCode;
use log::error;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::convert::Infallible;
use std::io::{BufRead, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::str::FromStr;
use std::sync::{Arc, RwLock};
use std::thread;

pub struct Request<T> {
    req_body: Vec<u8>,

    status: StatusCode,
    // TODO make this a Vec<u8>
    resp_body: String,
    resp_headers: HashMap<String, String>,

    path_params: HashMap<String, String>,
    state: Arc<RwLock<T>>,
}

impl<T: Send + Sync> Request<T> {
    #[throws]
    pub fn read_json<'a, D: Deserialize<'a>>(&'a self) -> D {
        serde_json::from_slice(&self.req_body)?
    }

    #[throws]
    pub fn write_json<S: Serialize>(&mut self, body: &S) {
        let json = serde_json::to_string(body)?;
        self.resp_body.push_str(&json);
        self.set_content_type("application/json");
    }

    pub fn set_status(&mut self, status: StatusCode) {
        self.status = status;
    }

    pub fn set_not_found(&mut self) {
        self.set_status(StatusCode::NOT_FOUND);
    }

    pub fn set_header(&mut self, name: &str, value: &str) {
        self.resp_headers.insert(name.into(), value.into());
    }

    pub fn set_content_type(&mut self, value: &str) {
        self.set_header("Content-Type", value);
    }

    #[throws]
    pub fn path_param<F>(&self, name: &str) -> F
    where
        F::Err: std::error::Error + Send + Sync + 'static,
        F: FromStr,
    {
        let value = self
            .path_params
            .get(name)
            .ok_or_else(|| anyhow!("path param {} not found", name))?;
        value
            .parse()
            .with_context(|| format!("failed to parse path param {}", name))?
    }

    #[throws]
    pub fn with_state<R, F>(&self, f: F) -> R
    where
        F: Fn(&T) -> R,
    {
        match self.state.read() {
            Ok(guard) => f(&guard),
            // Can't propagate with `?` here because RwLockReadGuard
            // cannot be sent between threads
            Err(err) => throw!(anyhow!("failed to lock state guard: {}", err)),
        }
    }

    #[throws]
    pub fn with_state_mut<R, F>(&self, f: F) -> R
    where
        F: Fn(&mut T) -> R,
    {
        match self.state.write() {
            Ok(mut guard) => f(&mut guard),
            // Can't propagate with `?` here because RwLockWriteGuard
            // cannot be sent between threads
            Err(err) => throw!(anyhow!("failed to lock state guard: {}", err)),
        }
    }
}

pub type Handler<T> =
    dyn Fn(&mut Request<T>) -> Result<(), Error> + Send + Sync;

#[derive(Clone)]
struct Path {
    parts: Vec<String>,
}

fn match_path(
    path: &Path,
    route_path: &Path,
) -> Option<HashMap<String, String>> {
    let mut map = HashMap::new();
    for (left, right) in path.parts.iter().zip(route_path.parts.iter()) {
        let is_placeholder = right.starts_with(':');
        if !is_placeholder && left != right {
            return None;
        }
        if is_placeholder {
            map.insert(right[1..].to_string(), left.to_string());
        }
    }
    Some(map)
}

impl FromStr for Path {
    type Err = Infallible;

    #[throws(Self::Err)]
    fn from_str(s: &str) -> Path {
        Path {
            parts: s.split('/').map(|p| p.to_string()).collect(),
        }
    }
}

struct Route<T> {
    method: String,
    path: Path,
    handler: Box<Handler<T>>,
}

pub struct Routes<T> {
    routes: Vec<Route<T>>,
}

impl<T> Routes<T> {
    pub fn new() -> Routes<T> {
        Routes { routes: Vec::new() }
    }

    #[throws]
    pub fn add(&mut self, route: &str, handler: &'static Handler<T>) {
        let mut iter = route.split_whitespace();
        let method = iter.next().ok_or_else(|| anyhow!("missing method"))?;
        let path = iter.next().ok_or_else(|| anyhow!("missing path"))?;
        self.routes.push(Route {
            method: method.into(),
            path: path.parse()?,
            handler: Box::new(handler),
        });
    }
}

impl<T> Default for Routes<T> {
    fn default() -> Routes<T> {
        Routes { routes: Vec::new() }
    }
}

#[throws]
fn handle_connection<T>(
    stream: TcpStream,
    routes: Arc<Routes<T>>,
    state: Arc<RwLock<T>>,
) {
    let mut stream = BufStream::new(stream);
    let mut line = String::new();
    stream
        .read_line(&mut line)
        .context("missing request header")?;
    let parts = line.split_whitespace().take(3).collect::<Vec<_>>();
    if parts.len() != 3 {
        throw!(anyhow!("invalid request: {}", line));
    }
    let method = parts[0];
    let path = parts[1].parse::<Path>()?;

    // Parse headers
    // TODO: do duplicate headers accumulate? should be Vec value if so
    let mut headers: HashMap<String, String> = HashMap::new();
    loop {
        let mut line = String::new();
        stream.read_line(&mut line).context("failed to read line")?;

        let mut parts = line.split(':');
        if let Some(name) = parts.next() {
            let value = parts.next().unwrap_or("");
            headers.insert(name.to_string(), value.trim().to_string());
        }

        if line.trim().is_empty() {
            break;
        }
    }

    // TODO: handle case
    let mut req_body = Vec::new();
    if let Some(len) = headers.get("Content-Length") {
        if let Ok(len) = len.parse::<usize>() {
            req_body.resize(len, 0);
            stream.read_exact(&mut req_body)?;
        }
    }

    for route in &routes.routes {
        if method != route.method {
            continue;
        }

        if let Some(path_params) = match_path(&path, &route.path) {
            let mut req = Request {
                // TODO
                req_body,

                resp_body: String::new(),
                status: StatusCode::OK,
                resp_headers: HashMap::new(),
                path_params,
                state,
            };
            if let Err(err) = (route.handler)(&mut req) {
                error!("{}", err);
                req.resp_body = "internal server error".into();
                req.status = StatusCode::INTERNAL_SERVER_ERROR;
            }
            stream.write(
                format!(
                    "HTTP/1.1 {} {}\n",
                    req.status.as_u16(),
                    req.status.canonical_reason().unwrap_or("")
                )
                .as_bytes(),
            )?;
            for (name, value) in req.resp_headers {
                stream.write(format!("{}: {}\n", name, value).as_bytes())?;
            }
            stream.write(
                format!("Content-Length: {}\n", req.resp_body.len()).as_bytes(),
            )?;
            stream.write(b"\n")?;
            stream.write(req.resp_body.as_bytes())?;
            return;
        }
    }

    // No matching route found
    let status = StatusCode::NOT_FOUND;
    stream.write(
        format!(
            "HTTP/1.1 {} {}\n\n",
            status.as_u16(),
            status.canonical_reason().unwrap_or("")
        )
        .as_bytes(),
    )?;
}

pub fn serve<T: Send + Sync + 'static>(
    address: &str,
    routes: Routes<T>,
    state: Arc<RwLock<T>>,
) -> Result<(), Error> {
    let socket = address.parse::<SocketAddr>()?;
    let listener = TcpListener::bind(socket)?;
    let routes = Arc::new(routes);
    loop {
        let (tcp_stream, _addr) = listener.accept()?;
        let routes = routes.clone();
        let state = state.clone();

        // Handle the request in a new thread
        if let Err(err) = thread::Builder::new()
            .name("shs-handler".into())
            .spawn(move || {
                if let Err(err) = handle_connection(tcp_stream, routes, state) {
                    error!("{}", err);
                }
            })
        {
            error!("failed to spawn thread: {}", err);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
