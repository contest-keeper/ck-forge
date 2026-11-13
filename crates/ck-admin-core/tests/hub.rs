use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

use ck_admin_core::{Error, push_registry};
use tempfile::TempDir;

/// Answers one request per canned reply, and hands back what it was sent.
fn fake_hub(replies: Vec<String>) -> (String, thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let address = format!("http://{}", listener.local_addr().expect("address"));
    let handle = thread::spawn(move || {
        let mut seen = Vec::new();
        for reply in replies {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut buffer = vec![0_u8; 65536];
            let mut got = 0;
            // Read until the whole request (headers and body) has arrived.
            loop {
                got += stream.read(&mut buffer[got..]).expect("read");
                let text = String::from_utf8_lossy(&buffer[..got]).into_owned();
                if let Some((head, body)) = text.split_once("\r\n\r\n") {
                    let length = head
                        .lines()
                        .find_map(|l| {
                            l.to_lowercase()
                                .strip_prefix("content-length: ")
                                .map(str::to_owned)
                        })
                        .and_then(|n| n.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    if body.len() >= length {
                        seen.push(text);
                        break;
                    }
                }
            }
            stream.write_all(reply.as_bytes()).expect("reply");
        }
        seen
    });
    (address, handle)
}

fn reply(status: &str, headers: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\n{headers}content-length: {}\r\ncontent-type: application/json\r\nconnection: close\r\n\r\n{body}",
        body.len()
    )
}

fn registry_file(dir: &TempDir) -> std::path::PathBuf {
    let path = dir.path().join("ck-registry.json");
    std::fs::write(
        &path,
        r#"{"sticks":[{"stick_id":"s1","token":"0123456789abcdef"}]}"#,
    )
    .expect("write");
    path
}

#[test]
fn logs_in_then_sends_the_registry_with_the_session() {
    let dir = TempDir::new().expect("dir");
    let (hub, handle) = fake_hub(vec![
        reply(
            "200 OK",
            "set-cookie: ck_session=abc123; HttpOnly; Secure\r\n",
            "{}",
        ),
        reply(
            "200 OK",
            "",
            r#"{"added":1,"already_known":0,"conflicts":[]}"#,
        ),
    ]);
    let pushed = push_registry(&hub, "ana", "a-long-password", &registry_file(&dir)).expect("push");
    assert_eq!((pushed.added, pushed.already_known), (1, 0));

    let seen = handle.join().expect("hub");
    assert!(seen[0].starts_with("POST /api/v1/login"));
    assert!(seen[0].contains(r#""username": "ana""#), "{}", seen[0]);
    assert!(seen[1].starts_with("POST /api/v1/sticks"));
    assert!(seen[1].to_lowercase().contains("cookie: ck_session=abc123"));
    assert!(seen[1].contains(r#""stick_id":"s1""#));
}

#[test]
fn a_wrong_password_is_reported_and_the_registry_is_not_sent() {
    let dir = TempDir::new().expect("dir");
    let (hub, handle) = fake_hub(vec![reply("401 Unauthorized", "", "{}")]);
    let error = push_registry(&hub, "ana", "nope", &registry_file(&dir)).unwrap_err();
    assert!(error.to_string().contains("refused that name or password"));
    assert_eq!(handle.join().expect("hub").len(), 1);
}

#[test]
fn a_conflict_still_reports_what_was_added() {
    let dir = TempDir::new().expect("dir");
    let (hub, _handle) = fake_hub(vec![
        reply(
            "200 OK",
            "set-cookie: ck_session=abc123; HttpOnly\r\n",
            "{}",
        ),
        reply(
            "409 Conflict",
            "",
            r#"{"added":1,"already_known":0,"conflicts":["s2"]}"#,
        ),
    ]);
    let pushed = push_registry(&hub, "ana", "a-long-password", &registry_file(&dir)).expect("push");
    assert_eq!(pushed.conflicts, ["s2"]);
}

#[test]
fn a_password_is_never_sent_over_plain_http_to_another_machine() {
    let dir = TempDir::new().expect("dir");
    let error =
        push_registry("http://hub.example.org", "ana", "pw", &registry_file(&dir)).unwrap_err();
    assert!(matches!(error, Error::Hub(_)));
}
