#![deny(missing_docs)]

//! Easy-to-use non-async HTTP 1.1 server.

mod status_code;

use anyhow::{anyhow, Context, Error};
use bufstream::BufStream;
use fehler::{throw, throws};
use log::error;
use serde::{Deserialize, Serialize};
pub use status_code::StatusCode;
use std::collections::HashMap;
use std::convert::Infallible;
use std::io::{BufRead, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::str::FromStr;
use std::sync::{Arc, RwLock};
use std::thread;
use url::Url;

type HeaderName = unicase::UniCase<String>;

/// Request type passed to handlers. It provides both the input
/// request and the output response, as well as access to state shared
/// across requests.
pub struct Request {
    method: String,
    path_params: HashMap<String, String>,
    req_headers: HashMap<HeaderName, String>,
    req_body: Vec<u8>,
    url: Url,

    status: StatusCode,
    resp_body: Vec<u8>,
    resp_headers: HashMap<String, String>,
}

impl Request {
    /// Get the request URL.
    pub fn url(&self) -> &Url {
        &self.url
    }

    /// Get the request headers.
    pub fn headers(&self) -> &HashMap<HeaderName, String> {
        &self.req_headers
    }

    /// Deserialize the body as JSON.
    #[throws]
    pub fn read_json<'a, D: Deserialize<'a>>(&'a self) -> D {
        serde_json::from_slice(&self.req_body)?
    }

    /// Write the input as the response body. This also sets the
    /// `Content-Type` to `application/octet-stream`.
    pub fn write_bytes(&mut self, body: &[u8]) {
        self.resp_body = body.to_vec();
        self.set_content_type("application/octet-stream");
    }

    /// Serialize the input as the response body. This also sets the
    /// `Content-Type` to `application/json`.
    #[throws]
    pub fn write_json<S: Serialize>(&mut self, body: &S) {
        self.resp_body = serde_json::to_vec(body)?;
        self.set_content_type("application/json");
    }

    /// Write the input as the response body with utf-8 encoding. This
    /// also sets the `Content-Type` to `text/plain; charset=UTF-8`.
    pub fn write_text(&mut self, body: &str) {
        self.resp_body = body.as_bytes().to_vec();
        self.set_content_type("text/plain; charset=UTF-8");
    }

    /// Set the response status code.
    pub fn set_status(&mut self, status: StatusCode) {
        self.status = status;
    }

    /// Set the response status code to 404 (not found).
    pub fn set_not_found(&mut self) {
        self.set_status(StatusCode::NotFound);
    }

    /// Set a response header.
    pub fn set_header(&mut self, name: &str, value: &str) {
        self.resp_headers.insert(name.into(), value.into());
    }

    /// Set the `Content-Type` response header.
    pub fn set_content_type(&mut self, value: &str) {
        self.set_header("Content-Type", value);
    }

    /// Get a path parameter. For example, if an input route
    /// "/resource/:key" is defined, the handler can get the ":key"
    /// portion by calling `path_param("key")`. The returned type can
    /// be anything that implements `FromStr`.
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
}

/// Handler function for a route.
pub type Handler = dyn Fn(&mut Request) -> Result<(), Error> + Send + Sync;

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

struct Route {
    method: String,
    path: Path,
    handler: Box<Handler>,
}

#[throws]
fn dispatch_request(
    routes: Arc<RwLock<Vec<Route>>>,
    path: &Path,
    req: &mut Request,
) -> bool {
    for route in &*routes.read().unwrap() {
        if req.method != route.method {
            continue;
        }

        if let Some(path_params) = match_path(path, &route.path) {
            req.path_params = path_params;
            (route.handler)(req)?;
            return true;
        }
    }
    req.status = StatusCode::NotFound;
    false
}

#[throws]
fn handle_connection(stream: TcpStream, routes: Arc<RwLock<Vec<Route>>>) {
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
    let raw_path = parts[1];
    let path = raw_path.parse::<Path>()?;

    // Parse headers
    // TODO: do duplicate headers accumulate? should be Vec value if so
    let mut headers: HashMap<HeaderName, String> = HashMap::new();
    loop {
        let mut line = String::new();
        stream.read_line(&mut line).context("failed to read line")?;

        let mut parts = line.splitn(2, ':');
        if let Some(name) = parts.next() {
            let value = parts.next().unwrap_or("");
            headers.insert(name.into(), value.trim().to_string());
        }

        if line.trim().is_empty() {
            break;
        }
    }

    let mut req_body = Vec::new();
    if let Some(len) = headers.get(&HeaderName::new("Content-Length".into())) {
        if let Ok(len) = len.parse::<usize>() {
            req_body.resize(len, 0);
            stream.read_exact(&mut req_body)?;
        }
    }

    let host = headers
        .get(&HeaderName::new("host".into()))
        .ok_or_else(|| anyhow!("missing host header"))?;
    let mut url = Url::parse(&format!("http://{}", host))
        .with_context(|| format!("failed to parse host {}", host))?;
    url.set_path(raw_path);

    let mut req = Request {
        method: method.into(),
        path_params: HashMap::new(),
        req_headers: headers,
        req_body,
        url,

        resp_body: Vec::new(),
        status: StatusCode::Ok,
        resp_headers: HashMap::new(),
    };

    match dispatch_request(routes, &path, &mut req) {
        Err(err) => {
            error!("{}", err);
            req.resp_body = "internal server error".into();
            req.status = StatusCode::InternalServerError;
        }
        Ok(false) => {
            error!("not found: {}", raw_path);
        }
        Ok(true) => {}
    }

    stream.write_all(
        format!(
            "HTTP/1.1 {} {}\n",
            req.status,
            req.status.canonical_reason(),
        )
        .as_bytes(),
    )?;
    for (name, value) in req.resp_headers {
        stream.write_all(format!("{}: {}\n", name, value).as_bytes())?;
    }
    stream.write_all(
        format!("Content-Length: {}\n", req.resp_body.len()).as_bytes(),
    )?;
    stream.write_all(b"\n")?;
    stream.write_all(&req.resp_body)?;
}

/// Test request for calling Server::test_request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TestRequest {
    body: Vec<u8>,
    method: String,
    url: Url,
    headers: HashMap<String, String>,
}

impl TestRequest {
    /// Create a new test request with the method, URL, and body set.
    ///
    /// The input string should be in the format "METHOD /path". The
    /// path will automatically be expanded to a full URL:
    /// "http://example.com/path".
    #[throws]
    pub fn new_with_body(s: &str, body: &[u8]) -> TestRequest {
        let parts = s.split_whitespace().collect::<Vec<_>>();
        TestRequest {
            body: body.into(),
            method: parts[0].into(),
            url: Url::parse(&format!("http://example.com{}", parts[1]))?,
            headers: HashMap::new(),
        }
    }

    /// Create a new test request with the method, URL, and body set.
    ///
    /// The input string should be in the format "METHOD /path". The
    /// path will automatically be expanded to a full URL:
    /// "http://example.com/path".
    #[throws]
    pub fn new_with_json<S: Serialize>(s: &str, body: &S) -> TestRequest {
        let parts = s.split_whitespace().collect::<Vec<_>>();
        TestRequest {
            body: serde_json::to_vec(body)?,
            method: parts[0].into(),
            url: Url::parse(&format!("http://example.com{}", parts[1]))?,
            headers: HashMap::new(),
        }
    }

    /// Create a new test request with the method and URL set.
    ///
    /// The input string should be in the format "METHOD /path". The
    /// path will automatically be expanded to a full URL:
    /// "http://example.com/path".
    #[throws]
    pub fn new(s: &str) -> TestRequest {
        Self::new_with_body(s, &Vec::new())?
    }

    #[throws]
    fn path(&self) -> Path {
        self.url.path().parse()?
    }
}

/// Response from calling Server::test_request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TestResponse {
    /// Response code.
    pub status: StatusCode,

    /// Response body.
    pub body: Vec<u8>,

    /// Response headers.
    pub headers: HashMap<HeaderName, String>,
}

impl TestResponse {
    /// Parse the test response body as JSON.
    #[throws]
    pub fn json<'a, D: Deserialize<'a>>(&'a self) -> D {
        serde_json::from_slice(&self.body)?
    }
}

fn convert_header_map_to_unicase(
    map: &HashMap<String, String>,
) -> HashMap<HeaderName, String> {
    map.iter()
        .map(|(key, val)| (HeaderName::new(key.clone()), val.clone()))
        .collect()
}

/// HTTP 1.1 server.
///
/// Example usage:
/// ```no_run
/// use anyhow::Error;
/// use fehler::throws;
/// use shs::{Request, Server};
///
/// #[throws]
/// fn handler(req: &mut Request) {
///     todo!();
/// }
///
/// let mut server = Server::new("127.0.0.1:1234")?;
/// server.route("GET /hello", &handler)?;
/// server.launch()?;
/// # Ok::<(), Error>(())
/// ```
pub struct Server {
    address: SocketAddr,
    routes: Arc<RwLock<Vec<Route>>>,
}

impl Server {
    /// Create a new Server.
    #[throws]
    pub fn new(address: &str) -> Server {
        Server {
            address: address.parse::<SocketAddr>()?,
            routes: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Add a new route. The basic format is `"METHOD /path"`. The
    /// path can contain parameters that start with a colon, for
    /// example `"/resource/:key"`; these parameters act as wild cards
    /// that can match any single path segment.
    #[throws]
    pub fn route(&mut self, route: &str, handler: &'static Handler) {
        let mut iter = route.split_whitespace();
        let method = iter.next().ok_or_else(|| anyhow!("missing method"))?;
        let path = iter.next().ok_or_else(|| anyhow!("missing path"))?;
        let mut routes = self.routes.write().unwrap();
        routes.push(Route {
            method: method.into(),
            path: path.parse()?,
            handler: Box::new(handler),
        });
    }

    /// Start the server.
    pub fn launch(self) -> Result<(), Error> {
        let listener = TcpListener::bind(self.address)?;
        loop {
            let (tcp_stream, _addr) = listener.accept()?;
            let routes = self.routes.clone();

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

    /// Send a fake request for testing.
    #[throws]
    pub fn test_request(&self, input: &TestRequest) -> TestResponse {
        let mut req = Request {
            method: input.method.clone(),
            path_params: HashMap::new(),
            req_headers: convert_header_map_to_unicase(&input.headers),
            req_body: input.body.clone(),
            url: input.url.clone(),

            resp_body: Vec::new(),
            status: StatusCode::Ok,
            resp_headers: HashMap::new(),
        };
        let path = input.path()?;
        dispatch_request(self.routes.clone(), &path, &mut req)?;

        TestResponse {
            status: req.status,
            body: req.resp_body,
            headers: convert_header_map_to_unicase(&req.resp_headers),
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
