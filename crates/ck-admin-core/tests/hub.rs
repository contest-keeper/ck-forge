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

mod signed_in {
    use super::{fake_hub, registry_file, reply};
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::time::Duration;

    use ck_admin_core::{
        Error, new_state, push_registry_signed_in, sign_in_url, sign_in_with_code, wait_for_code,
    };
    use tempfile::TempDir;

    fn browser(port: u16, target: &str) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
        write!(stream, "GET {target} HTTP/1.1\r\nhost: 127.0.0.1\r\n\r\n").expect("send");
        let mut answer = String::new();
        stream.read_to_string(&mut answer).expect("answer");
        answer
    }

    #[test]
    fn the_browser_page_carries_the_port_and_a_state_nobody_can_guess() {
        let url = sign_in_url("https://hub.example.org/", 4000, "abc").expect("url");
        assert_eq!(
            url,
            "https://hub.example.org/forge/connect?port=4000&state=abc"
        );
        assert_ne!(new_state(), new_state());
        assert_eq!(new_state().len(), 64);
        assert!(sign_in_url("http://hub.example.org", 4000, "abc").is_err());
    }

    #[test]
    fn the_code_is_taken_only_from_a_request_with_the_right_state() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("address").port();
        let waiting = std::thread::spawn(move || {
            wait_for_code(&listener, "right-state", Duration::from_secs(10))
        });
        assert!(browser(port, "/callback?code=evil&state=wrong").contains("400"));
        assert!(browser(port, "/favicon.ico").contains("400"));
        assert!(browser(port, "/callback?code=a%20b&state=right-state").contains("400"));
        assert!(browser(port, "/callback?code=good_code-1&state=right-state").contains("200"));
        assert_eq!(
            waiting.join().expect("thread").expect("code"),
            "good_code-1"
        );
    }

    #[test]
    fn it_gives_up_when_the_browser_never_comes_back() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let error = wait_for_code(&listener, "s", Duration::from_millis(300)).unwrap_err();
        assert!(error.to_string().contains("not finished in time"));
    }

    #[test]
    fn the_code_gives_a_session_that_sends_the_registry_as_a_bearer() {
        let dir = TempDir::new().expect("dir");
        let (hub, handle) = fake_hub(vec![
            reply(
                "200 OK",
                "",
                r#"{"token":"tok123","email":"ana@example.org","expires_in":43200}"#,
            ),
            reply(
                "200 OK",
                "",
                r#"{"added":1,"already_known":0,"conflicts":[]}"#,
            ),
        ]);
        let session = sign_in_with_code(&hub, "the-code").expect("session");
        assert_eq!(session.email, "ana@example.org");
        assert!(!format!("{session:?}").contains("tok123"));
        let pushed = push_registry_signed_in(&hub, &session, &registry_file(&dir)).expect("push");
        assert_eq!(pushed.added, 1);

        let seen = handle.join().expect("hub");
        assert!(seen[0].starts_with("POST /api/v1/forge/token"));
        assert!(seen[0].contains(r#""code": "the-code""#), "{}", seen[0]);
        assert!(seen[1].starts_with("POST /api/v1/sticks"));
        assert!(
            seen[1]
                .to_lowercase()
                .contains("authorization: bearer tok123")
        );
    }

    #[test]
    fn an_old_code_or_an_ended_session_says_to_sign_in_again() {
        let (hub, _handle) = fake_hub(vec![reply("400 Bad Request", "", "{}")]);
        let error = sign_in_with_code(&hub, "old").unwrap_err();
        assert!(error.to_string().contains("sign in again"));

        let dir = TempDir::new().expect("dir");
        let (hub, _handle) = fake_hub(vec![
            reply("200 OK", "", r#"{"token":"t","email":"a@b.c"}"#),
            reply("401 Unauthorized", "", "{}"),
        ]);
        let session = sign_in_with_code(&hub, "c").expect("session");
        let error = push_registry_signed_in(&hub, &session, &registry_file(&dir)).unwrap_err();
        assert!(matches!(error, Error::Hub(text) if text.contains("sign in again")));
    }
}

mod profiles {
    use ck_admin_core::{Profile, fetch_profile, list_profiles, sign_in_with_code, upload_profile};

    use super::{fake_hub, reply};

    fn signed_in_replies() -> Vec<String> {
        vec![reply("200 OK", "", r#"{"token":"tok123","email":"a@b.c"}"#)]
    }

    #[test]
    fn lists_loads_and_saves_profiles_as_the_signed_in_admin() {
        let settings = serde_json::to_string(&Profile::template().settings).expect("json");
        let mut replies = signed_in_replies();
        replies.push(reply(
            "200 OK",
            "",
            r#"[{"id":"p1","name":"Round","updated_at":"now"}]"#,
        ));
        replies.push(reply(
            "200 OK",
            "",
            &format!(r#"{{"id":"p1","name":"Round","settings":{settings},"wallpaper":{{"name":"w.png"}}}}"#),
        ));
        replies.push(reply("200 OK", "", "PNGBYTES"));
        replies.push(reply("204 No Content", "", ""));
        let (hub, handle) = fake_hub(replies);

        let session = sign_in_with_code(&hub, "c").expect("session");
        let list = list_profiles(&hub, &session).expect("list");
        assert_eq!(
            (list[0].id.as_str(), list[0].name.as_str()),
            ("p1", "Round")
        );

        let mut loaded = fetch_profile(&hub, &session, "p1").expect("fetch");
        assert_eq!(loaded.name, "Round");
        let wallpaper = loaded.wallpaper.as_ref().expect("wallpaper");
        assert_eq!(
            (wallpaper.name.as_str(), wallpaper.bytes.as_slice()),
            ("w.png", &b"PNGBYTES"[..])
        );

        loaded.settings.wifi = Some(ck_protocol::Wifi {
            ssid: "venue".to_owned(),
            password: Some(ck_protocol::WifiPassword::new("hunter2-hunter2")),
        });
        upload_profile(&hub, &session, "p1", &loaded).expect("upload");
        let seen = handle.join().expect("hub");
        assert!(seen[1].starts_with("GET /api/v1/profiles "));
        assert!(seen[4].starts_with("PUT /api/v1/profiles/p1"));
        assert!(
            seen[4].contains("UE5HQllURVM="),
            "wallpaper goes as base64: {}",
            seen[4]
        );
        assert!(seen[4].contains("venue"), "the network name is shared");
        assert!(
            !seen.iter().any(|r| r.contains("hunter2")),
            "the Wi-Fi password never goes to the Hub"
        );
        assert!(
            seen.iter()
                .skip(1)
                .all(|r| r.to_lowercase().contains("authorization: bearer tok123"))
        );
    }

    #[test]
    fn a_viewer_or_an_ended_session_gets_a_plain_answer() {
        let mut replies = signed_in_replies();
        replies.push(reply("403 Forbidden", "", "{}"));
        let (hub, _handle) = fake_hub(replies);
        let session = sign_in_with_code(&hub, "c").expect("session");
        let error = list_profiles(&hub, &session).unwrap_err();
        assert!(error.to_string().contains("not an admin"), "{error}");
    }
}
