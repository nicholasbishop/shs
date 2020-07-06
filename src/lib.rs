use anyhow::{anyhow, Context, Error};
use bufstream::BufStream;
use fehler::{throw, throws};
use log::error;
use serde::Serialize;
use std::collections::HashMap;
use std::convert::Infallible;
use std::io::BufRead;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::str::FromStr;
use std::sync::Arc;
use std::thread;

pub struct Request<T> {
    //writer: &'a BufStream,
    path_params: HashMap<String, String>,
    state: Arc<T>,
}

impl<T> Request<T> {
    #[throws]
    pub fn send_json<S: Serialize>(&mut self, _t: S) {
        // TODO: write body
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

    pub fn state(&self) -> &T {
        &self.state
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
    state: Arc<T>,
) {
    let stream = BufStream::new(stream);
    let mut lines = stream.lines();
    let line = lines
        .next()
        .ok_or_else(|| anyhow!("missing request header"))??;
    let parts = line.split_whitespace().take(3).collect::<Vec<_>>();
    if parts.len() != 3 {
        throw!(anyhow!("invalid request: {}", line));
    }
    let method = parts[0];
    let path = parts[1].parse::<Path>()?;

    // Parse headers
    // TODO: do duplicate headers accumulate? should be Vec value if so
    let mut headers: HashMap<String, String> = HashMap::new();
    for line in lines {
        let line = match line {
            Ok(line) => line,
            Err(err) => {
                error!("failed to read header: {}", err);
                continue;
            }
        };
        let mut parts = line.split(':');
        if let Some(name) = parts.next() {
            let value = parts.next().unwrap_or("");
            headers.insert(name.to_string(), value.to_string());
        }

        if line.trim().is_empty() {
            break;
        }
    }

    for route in &routes.routes {
        if method != route.method {
            continue;
        }

        if let Some(path_params) = match_path(&path, &route.path) {
            let mut req = Request { path_params, state };
            if let Err(err) = (route.handler)(&mut req) {
                error!("{}", err);
            // TODO: handle error
            } else {
                // TODO: handle success
            }
            break;
        }
    }
}

pub fn serve<T: Send + Sync + 'static>(
    address: &str,
    routes: Routes<T>,
    state: Arc<T>,
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
