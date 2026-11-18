//! A tiny web server on this computer that gives cloud-init in the VM its instructions.
//!
//! cloud-init asks for `user-data` and `meta-data` at the address it is given. Serving them over
//! the VM's own network avoids building a configuration disk, which needs tools Windows lacks.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::Error;

pub struct Seed {
    pub port: u16,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Seed {
    pub fn start(user_data: String) -> Result<Self, Error> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .map_err(|e| Error::io("starting the seed server", e))?;
        listener
            .set_nonblocking(true)
            .map_err(|e| Error::io("starting the seed server", e))?;
        let port = listener
            .local_addr()
            .map_err(|e| Error::io("reading the seed port", e))?
            .port();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let thread = thread::spawn(move || {
            while !flag.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => answer(stream, &user_data),
                    Err(_) => thread::sleep(Duration::from_millis(50)),
                }
            }
        });
        Ok(Self {
            port,
            stop,
            thread: Some(thread),
        })
    }
}

impl Drop for Seed {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn answer(mut stream: TcpStream, user_data: &str) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let mut request = [0u8; 2048];
    let Ok(count) = stream.read(&mut request) else {
        return;
    };
    let request = String::from_utf8_lossy(&request[..count]);
    let path = request.split_whitespace().nth(1).unwrap_or("");
    let (status, body) = match path {
        "/user-data" => ("200 OK", user_data),
        "/meta-data" => (
            "200 OK",
            "instance-id: ck-base-image\nlocal-hostname: ck-base\n",
        ),
        "/vendor-data" => ("200 OK", ""),
        _ => ("404 Not Found", ""),
    };
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get(port: u16, path: &str) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
        write!(stream, "GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").expect("send");
        let mut text = String::new();
        stream.read_to_string(&mut text).expect("read");
        text
    }

    #[test]
    fn serves_user_data_and_refuses_the_rest() {
        let seed = Seed::start("#!/bin/bash\necho hi\n".to_owned()).expect("start");
        assert!(get(seed.port, "/user-data").ends_with("echo hi\n"));
        assert!(get(seed.port, "/meta-data").contains("instance-id"));
        assert!(get(seed.port, "/etc/passwd").starts_with("HTTP/1.1 404"));
    }
}
