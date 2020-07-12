use fehler::throws;
use serde::Serialize;
use shs::{Request, RequestError, Server, StatusCode};

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("empty message")]
    EmptyMessage,
    #[error("message too long: {0}")]
    MessageTooLong(usize),
}

#[derive(Serialize)]
struct Resp {
    name: String,
}

fn msg_handler(req: &mut Request) -> Result<(), Error> {
    let message: String = req.path_param("message").unwrap();
    if message.is_empty() {
        Err(Error::EmptyMessage)
    } else if message.len() > 42 {
        Err(Error::MessageTooLong(message.len()))
    } else {
        req.write_text("thanks for the nice message");
        Ok(())
    }
}

fn error_handler(req: &mut Request, err: &RequestError<Error>) {
    match err {
        RequestError::NotFound => {
            req.set_status(StatusCode::NotFound);
        }
        RequestError::Custom(Error::EmptyMessage) => {
            req.write_text("empty message");
            req.set_status(StatusCode::BadRequest);
        }
        RequestError::Custom(Error::MessageTooLong(len)) => {
            req.write_text(&format!("message too long: {}", len));
            req.set_status(StatusCode::BadRequest);
        }
    }
}

#[throws(anyhow::Error)]
fn main() {
    simple_logging::log_to_stderr(log::LevelFilter::Info);

    let mut server = Server::new("127.0.0.1:1234")?;
    server.route("POST /:message", &msg_handler)?;
    server.set_error_handler(&error_handler);
    server.launch()?;
}
