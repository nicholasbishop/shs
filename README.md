# shs (simple http server)

[![crates.io](https://img.shields.io/crates/v/shs.svg)](https://crates.io/crates/shs)
[![Documentation](https://docs.rs/shs/badge.svg)](https://docs.rs/shs)

The shs crate provides an easy-to-use non-async HTTP server.

Example:

```rust
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
```

## Design goals

The Rust ecosystem already has great HTTP server libraries that
attempt to be as fast as possible, e.g. [actix-web](https://actix.rs)
and [warp](https://github.com/seanmonstar/warp). But this speed
sometimes comes at the expense of ease-of-use, and for some projects
it makes sense to trade off some performance. For example, you might
know that the server will only be used in an internal network with a
limited number of clients connected to it.

Perhaps the main way this library differs from faster server libraries
is that it does not use async. Instead, a new thread is spawned for
each connection. This helps with ease-of-use in a few ways. First, you
don't have to worry about accidentally blocking the async runtime. You
can block a thread for as long as you like and it won't interfere with
other threads unless there's a locking bug. (It's easier to search for
locks than to search for something blocking an async function.)
Second, async code "infects" everything; every place you were using
`std::fs` needs to switch to using `tokio::fs`, a great many functions
will need to have `async` and `await` added, and so on. Third, the
async ecosystem in stable rust is still pretty new. Right now there
are tough problems like the tokio/async-std split, lack of tooling to
find accidental async-blocking code, and occasional crazy compilation
errors. I fully expect the async ecosystem to improve a lot over the
next few years, and this is not at all a complaint against the way
Rust has implemented async! It's a great technical achievement, it
just has tradeoffs like anything else.

Another difference from other Rust HTTP servers is that it is more
"stringly" typed. For example, routes are defined with strings like
`"GET /path/:param"` instead of something like
`router.get(Path::new("path").param("param"))`. It's less efficient
and some errors that could be caught at compile time will be caught at
runtime instead, but it's quicker to write and, more importantly,
easier to read.

## Safety

This crate does not directly use any `unsafe` code, although the
libraries it depends on might.
