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

fn has_end_of_headers(bytes: &[u8]) -> bool {
    bytes.windows(4).any(|window| window == b"\r\n\r\n")
}

fn answer(mut stream: TcpStream, user_data: &str) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    // A request can arrive in several pieces. Answering and closing before the rest is read resets the
    // connection, so read up to the end of the headers.
    let mut request = [0u8; 2048];
    let mut count = 0;
    while count < request.len() && !has_end_of_headers(&request[..count]) {
        match stream.read(&mut request[count..]) {
            Ok(0) | Err(_) => break,
            Ok(read) => count += read,
        }
    }
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

    #[test]
    fn waits_for_the_rest_of_a_request_that_arrives_in_pieces() {
        let seed = Seed::start("#!/bin/bash\necho hi\n".to_owned()).expect("start");
        let mut stream = TcpStream::connect(("127.0.0.1", seed.port)).expect("connect");
        stream.write_all(b"GET ").expect("send");
        stream.flush().expect("flush");
        thread::sleep(Duration::from_millis(200));
        stream
            .write_all(b"/user-data HTTP/1.1\r\nHost: x\r\n\r\n")
            .expect("send");
        let mut text = String::new();
        stream.read_to_string(&mut text).expect("read");
        assert!(text.ends_with("echo hi\n"));
    }
}
