use anyhow::{anyhow, Context, Error};
use fehler::{throw, throws};
use log::error;
use serde::Serialize;
use std::any::Any;
use std::collections::HashMap;
use std::convert::Infallible;
use std::io::{self, BufRead};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::str::FromStr;
use std::sync::Arc;
use std::thread;

pub struct Request<'a> {
    state: &'a dyn Any,
    path_params: HashMap<String, String>,
}

impl<'a> Request<'a> {
    #[throws]
    pub fn send_json<T: Serialize>(&mut self, _t: T) {
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

    #[throws]
    pub fn state<T: Any>(&self) -> &'a T {
        self.state
            .downcast_ref()
            .ok_or_else(|| anyhow!("failed to cast state"))?
    }
}

pub type Handler = dyn Fn(&mut Request) -> Result<(), Error> + Send + Sync;

#[derive(Clone)]
struct Path {
    parts: Vec<String>,
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

struct Route {
    method: String,
    path: Path,
    handler: Box<Handler>,
}

pub struct Routes {
    routes: Vec<Route>,
}

impl Routes {
    pub fn new() -> Routes {
        Routes { routes: Vec::new() }
    }

    #[throws]
    pub fn add(&mut self, route: &str, handler: &'static Handler) {
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

impl Default for Routes {
    fn default() -> Routes {
        Routes { routes: Vec::new() }
    }
}

#[throws]
fn handle_connection(stream: TcpStream, routes: Arc<Routes>) {
    let reader = io::BufReader::new(stream);
    let mut lines = reader.lines();
    let line = lines
        .next()
        .ok_or_else(|| anyhow!("missing request header"))??;
    let parts = line.split_whitespace().take(3).collect::<Vec<_>>();
    if parts.len() != 3 {
        throw!(anyhow!("invalid request: {}", line));
    }
    let method = parts[0];
    let path = parts[1];
    let protocol = parts[2];

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
    }
}

pub fn serve<State>(
    address: &str,
    routes: Routes,
    _state: State,
) -> Result<(), Error> {
    let socket = address.parse::<SocketAddr>()?;
    let listener = TcpListener::bind(socket)?;
    let routes = Arc::new(routes);
    loop {
        let (tcp_stream, _addr) = listener.accept()?;
        let routes = routes.clone();

        // Handle the request in a new thread
        if let Err(err) = thread::Builder::new()
            .name("shs-handler".into())
            .spawn(move || {
                if let Err(err) = handle_connection(tcp_stream, routes) {
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
