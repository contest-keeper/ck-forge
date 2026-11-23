use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

use ck_admin_core::{Error, push_registry};
use tempfile::TempDir;

/// Answers one request per canned reply, and hands back what it was sent.
fn fake_hub<T: AsRef<[u8]> + Send + 'static>(
    replies: Vec<T>,
) -> (String, thread::JoinHandle<Vec<String>>) {
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
            stream.write_all(reply.as_ref()).expect("reply");
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

/// The Hub's answer about an account that belongs to one organization, `acm`.
fn me() -> String {
    reply("200 OK", "", r#"{"organizations":[{"slug":"acm"}]}"#)
}

mod hub_images {
    use super::{fake_hub, reply};
    use ck_admin_core::{
        HubImageStep, HubSession, download_hub_image, list_hub_images, prepare_hub_image_download,
    };
    use serde_json::json;
    use tempfile::TempDir;

    const PATH: &str = "/download/image/keeper-os-fixture";
    const ARCHIVE: &[u8] = b"\x28\xb5\x2f\xfd\x20\x03\x19\x00\x00abc";
    const BLOB_PATH: &str = "/fixture.img.zst?sig=fixture-signed-value";

    fn session() -> HubSession {
        HubSession::restore("fixture-bearer".to_owned(), "admin@example.test".to_owned())
    }

    fn catalogue(path: &str, size: usize, digest: &str) -> String {
        reply(
            "200 OK",
            "",
            &json!({"images": [{
                "id": "keeper-os-fixture", "name": "Keeper OS", "version": "1.0",
                "url": path, "size": size, "sha256": digest, "family": "ubuntu",
                "profile": "generic", "source_offer_url": null
            }]})
            .to_string(),
        )
    }

    fn archive_reply(status: &str, headers: &str) -> Vec<u8> {
        let mut bytes = format!(
            "HTTP/1.1 {status}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n",
            ARCHIVE.len(),
        )
        .into_bytes();
        bytes.extend_from_slice(ARCHIVE);
        bytes
    }

    #[test]
    fn pulls_a_verified_image_without_forwarding_the_hub_session() {
        let (blob, blob_requests) = fake_hub(vec![archive_reply("200 OK", "")]);
        let location = format!("Location: {blob}{BLOB_PATH}\r\n");
        let list = catalogue(PATH, ARCHIVE.len(), &ck_crypto::sha256_hex(ARCHIVE));
        let (hub, hub_requests) =
            fake_hub(vec![list.clone(), list, reply("302 Found", &location, "")]);
        let items = list_hub_images(&hub, &session()).expect("catalogue");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].name, "Keeper OS");
        assert_eq!(items[0].filename, None, "older catalogue without filename");

        let download =
            prepare_hub_image_download(&hub, &session(), &items[0].id).expect("location");
        let dir = TempDir::new().expect("dir");
        let out = dir.path().join("base.img");
        let mut stages = Vec::new();
        download_hub_image(download, &out, |step, done, total| {
            stages.push((step, done, total));
        })
        .expect("verified image");
        assert_eq!(std::fs::read(&out).expect("raw image"), b"abc");
        assert_eq!(dir.path().read_dir().expect("files").count(), 1);
        assert!(stages.iter().any(|(step, done, total)| {
            *step == HubImageStep::Download && *done == *total && *total > 0
        }));
        assert!(stages.iter().any(|(step, done, total)| {
            *step == HubImageStep::Import && *done == *total && *total > 0
        }));
        let target = dir.path().join("written.img");
        let checked = ck_writer::write_image(&out, &target, |_, _, _| {}).expect("write test file");
        assert_eq!(checked, ck_crypto::sha256_hex(b"abc"));
        assert!(
            std::fs::read(target)
                .expect("written image")
                .starts_with(b"abc")
        );

        let seen = hub_requests.join().expect("hub requests");
        assert!(seen[2].starts_with(&format!("GET {PATH} HTTP/1.1")));
        for request in seen {
            assert!(
                request
                    .to_ascii_lowercase()
                    .contains("authorization: bearer fixture-bearer")
            );
        }
        let seen = blob_requests.join().expect("blob requests");
        assert!(seen[0].starts_with(&format!("GET {BLOB_PATH} HTTP/1.1")));
        assert!(!seen[0].to_ascii_lowercase().contains("authorization:"));
        assert!(!seen[0].to_ascii_lowercase().contains("cookie:"));
    }

    #[test]
    fn rejects_unsafe_catalogue_paths_and_locations_without_disclosing_them() {
        let digest = ck_crypto::sha256_hex(ARCHIVE);
        let unsafe_path = "https://other.example.test/?sig=fixture-signed-value";
        let (hub, _) = fake_hub(vec![catalogue(unsafe_path, ARCHIVE.len(), &digest)]);
        let error = list_hub_images(&hub, &session()).unwrap_err();
        assert!(!error.to_string().contains("fixture-signed-value"));
        for location in [
            "http://outside.example.test/image?sig=fixture-signed-value",
            "https://name:password@outside.example.test/image?sig=fixture-signed-value",
            "/relative?sig=fixture-signed-value",
        ] {
            let (hub, _) = fake_hub(vec![
                catalogue(PATH, ARCHIVE.len(), &digest),
                reply("302 Found", &format!("Location: {location}\r\n"), ""),
            ]);
            let Err(error) = prepare_hub_image_download(&hub, &session(), "keeper-os-fixture")
            else {
                panic!("unsafe location accepted");
            };
            assert!(!error.to_string().contains("fixture-signed-value"));
        }
    }

    #[test]
    fn refuses_wrong_sizes_corrupt_archives_and_storage_redirects() {
        let digest = ck_crypto::sha256_hex(ARCHIVE);
        let corrupt_digest = "0".repeat(64);
        for (size, expected, status, headers) in [
            (ARCHIVE.len() - 1, digest.as_str(), "200 OK", ""),
            (ARCHIVE.len() + 1, digest.as_str(), "200 OK", ""),
            (ARCHIVE.len(), corrupt_digest.as_str(), "200 OK", ""),
            (
                ARCHIVE.len(),
                digest.as_str(),
                "302 Found",
                "Location: /elsewhere?sig=fixture-signed-value\r\n",
            ),
        ] {
            let (blob, _) = fake_hub(vec![archive_reply(status, headers)]);
            let (hub, _) = fake_hub(vec![
                catalogue(PATH, size, expected),
                reply("302 Found", &format!("Location: {blob}{BLOB_PATH}\r\n"), ""),
            ]);
            let download = prepare_hub_image_download(&hub, &session(), "keeper-os-fixture")
                .expect("location");
            let dir = TempDir::new().expect("dir");
            let out = dir.path().join("base.img");
            let error = download_hub_image(download, &out, |_, _, _| {}).unwrap_err();
            assert!(!error.to_string().contains("fixture-signed-value"));
            if status == "302 Found" {
                assert!(error.to_string().contains("storage returned HTTP 302"));
            }
            assert_eq!(dir.path().read_dir().expect("files").count(), 0);
        }
    }
}

mod enrolment {
    use ck_admin_core::{
        CredentialScope, HardwareId, HubSession, mint_enrolment_code, push_registry_signed_in,
        record_config_hash, sign_settings,
    };
    use ck_protocol::Settings;
    use serde_json::{Value, json};
    use time::{OffsetDateTime, format_description::well_known::Rfc3339};

    use super::{fake_hub, me, reply};

    fn session() -> HubSession {
        HubSession::restore(
            "fixture-admin-session".to_owned(),
            "admin@example.test".to_owned(),
        )
    }

    fn utc(text: &str) -> OffsetDateTime {
        OffsetDateTime::parse(text, &Rfc3339).expect("UTC time")
    }

    #[test]
    fn asks_the_hub_to_sign_the_exact_bytes_of_the_settings() {
        let signature = "ck-settings-signature-v1 0123456789abcdef c2ln\n";
        let (hub, requests) = fake_hub(vec![
            me(),
            reply(
                "200 OK",
                "",
                &json!({"signature": signature, "key_id": "0123456789abcdef"}).to_string(),
            ),
        ]);
        let settings = b"{\"schema\": 3,\n  \"stick_id\": \"03c9f556\"}";
        let answer = sign_settings(&hub, &session(), settings).expect("signed");
        assert_eq!(answer, signature);
        let requests = requests.join().expect("Hub requests");
        assert!(requests[0].starts_with("GET /api/v1/me HTTP/1.1\r\n"));
        let (headers, body) = requests[1].split_once("\r\n\r\n").expect("HTTP request");
        assert!(headers.starts_with("POST /api/v1/orgs/acm/config-signatures HTTP/1.1\r\n"));
        assert!(
            headers
                .to_ascii_lowercase()
                .contains("authorization: bearer fixture-admin-session")
        );
        let sent: Value = serde_json::from_str(body).expect("request");
        let bytes = base64::Engine::decode(
            &base64::engine::general_purpose::STANDARD,
            sent["config"].as_str().expect("base64"),
        )
        .expect("base64");
        assert_eq!(
            bytes, settings,
            "the Hub gets exactly the bytes that will be written"
        );
    }

    #[test]
    fn says_why_the_hub_would_not_sign_without_echoing_what_it_answered() {
        for (status, words) in [
            ("503 Service Unavailable", "no settings signing key"),
            ("409 Conflict", "another organization"),
            ("429 Too Many Requests", "too many"),
            ("403 Forbidden", "cannot get settings signed"),
        ] {
            let (hub, _requests) = fake_hub(vec![me(), reply(status, "", "fixture-secret-body")]);
            let error = sign_settings(&hub, &session(), b"{}")
                .unwrap_err()
                .to_string();
            assert!(error.contains(words), "{status}: {error}");
            assert!(!error.contains("fixture-secret-body"));
        }
    }

    #[test]
    fn tells_the_hub_the_hash_of_the_settings_it_wrote() {
        let (hub, requests) = fake_hub(vec![me(), reply("204 No Content", "", "")]);
        let stick = ck_protocol::StickId::parse("03c9f556").expect("stick id");
        record_config_hash(&hub, &session(), &stick, &"ab".repeat(32)).expect("recorded");
        let requests = requests.join().expect("Hub requests");
        let (headers, body) = requests[1].split_once("\r\n\r\n").expect("HTTP request");
        assert!(headers.starts_with("POST /api/v1/orgs/acm/config-hashes HTTP/1.1\r\n"));
        assert!(
            headers
                .to_ascii_lowercase()
                .contains("authorization: bearer fixture-admin-session")
        );
        assert_eq!(
            serde_json::from_str::<Value>(body).expect("request"),
            json!({"stick_id": "03c9f556", "sha256": "ab".repeat(32)}),
        );
    }

    #[test]
    fn mints_one_code_with_the_allocated_machine_and_contest_scope() {
        let fixture_code = "ABCD-EFGH-JKLM-NPQR-STUV";
        let (hub, requests) = fake_hub(vec![
            me(),
            reply(
                "201 Created",
                "Cache-Control: no-store\r\n",
                &json!({"code": fixture_code, "expires_at": "2026-11-19T23:00:00Z"}).to_string(),
            ),
        ]);
        let settings = Settings {
            team: "Team A".to_owned(),
            site: "Lab B".to_owned(),
            contest: ck_protocol::Contest {
                name: "Regional Finals".to_owned(),
                ..ck_protocol::Contest::default()
            },
            ..Settings::default()
        };
        let scope = CredentialScope {
            contest_id: "regional-2026".to_owned(),
            valid_from: utc("2026-11-19T22:00:00Z"),
            valid_until: utc("2026-11-20T04:00:00Z"),
        };

        let disk = HardwareId {
            fingerprint: "ab".repeat(32),
            serial_hint: "1234".to_owned(),
        };
        let minted = mint_enrolment_code(&hub, &session(), &settings, 4, Some(&scope), Some(&disk))
            .expect("mint enrolment code");
        assert_eq!(minted.code.expose(), fixture_code);
        assert_eq!(minted.expires_at, utc("2026-11-19T23:00:00Z"));
        assert!(!format!("{minted:?}").contains(fixture_code));
        let requests = requests.join().expect("Hub requests");
        let (headers, body) = requests[1].split_once("\r\n\r\n").expect("HTTP request");
        assert!(headers.starts_with("POST /api/v1/orgs/acm/enrolment-codes HTTP/1.1\r\n"));
        assert!(
            headers
                .to_ascii_lowercase()
                .contains("authorization: bearer fixture-admin-session")
        );
        assert_eq!(
            serde_json::from_str::<Value>(body).expect("mint request"),
            json!({"team": "Team A", "site": "Lab B", "contest": "Regional Finals",
                "machine_number": 4, "credential_scope": scope, "lifetime_minutes": 1440,
                "hardware": {"fingerprint": "ab".repeat(32), "serial_hint": "1234"}}),
        );
    }

    #[test]
    fn rejects_a_redirect_and_an_invalid_code_without_exposing_the_hub_body() {
        let fixture = "fixture-secret-value";
        for response in [
            reply(
                "307 Temporary Redirect",
                &format!("Location: https://example.test/?secret={fixture}\r\n"),
                "",
            ),
            reply(
                "201 Created",
                "",
                &json!({"code": fixture, "expires_at": "2026-11-19T23:00:00Z"}).to_string(),
            ),
        ] {
            let (hub, _) = fake_hub(vec![response]);
            let error = mint_enrolment_code(&hub, &session(), &Settings::default(), 1, None, None)
                .unwrap_err();
            assert!(!error.to_string().contains(fixture));
        }
    }

    #[test]
    fn only_registered_legacy_sticks_are_pushed_to_hub() {
        let folder = tempfile::tempdir().expect("folder");
        let path = folder.path().join("registry.json");
        let legacy = json!({"sticks": [{"stick_id": "old", "token": "fixture-legacy-key"}]});
        let mut local = legacy.clone();
        local["enrolments"] =
            json!([{ "stick_id": "new", "contest": "Contest", "machine_number": 2 }]);
        std::fs::write(&path, local.to_string()).expect("write registry");
        let (hub, requests) = fake_hub(vec![
            me(),
            reply(
                "200 OK",
                "",
                r#"{"added":1,"already_known":0,"conflicts":[]}"#,
            ),
        ]);
        push_registry_signed_in(&hub, &session(), &path).expect("push legacy registry");
        let requests = requests.join().expect("hub requests");
        let (_, body) = requests[1].split_once("\r\n\r\n").expect("HTTP request");
        assert_eq!(
            serde_json::from_str::<Value>(body).expect("old wire"),
            legacy
        );
    }
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
        me(),
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
    assert!(seen[0].contains(r#""email": "ana""#), "{}", seen[0]);
    assert!(seen[1].starts_with("GET /api/v1/me"));
    assert!(seen[1].to_lowercase().contains("cookie: ck_session=abc123"));
    assert!(seen[2].starts_with("POST /api/v1/orgs/acm/sticks"));
    assert!(seen[2].to_lowercase().contains("cookie: ck_session=abc123"));
    assert!(seen[2].contains(r#""stick_id":"s1""#));
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
        me(),
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
    use super::{fake_hub, me, registry_file, reply};
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
            me(),
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
        assert!(seen[1].starts_with("GET /api/v1/me"));
        assert!(seen[2].starts_with("POST /api/v1/orgs/acm/sticks"));
        for request in &seen[1..] {
            assert!(
                request
                    .to_lowercase()
                    .contains("authorization: bearer tok123")
            );
        }
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
    use ck_admin_core::{
        Profile, fetch_profile, list_profiles, retire_stick_credential, sign_in_with_code,
        upload_profile,
    };

    use super::{fake_hub, me, reply};

    fn signed_in_replies() -> Vec<String> {
        vec![reply("200 OK", "", r#"{"token":"tok123","email":"a@b.c"}"#)]
    }

    #[test]
    fn lists_loads_and_saves_profiles_as_the_signed_in_admin() {
        let mut settings = serde_json::to_value(&Profile::template().settings).expect("json");
        settings["stick_key_window"] = serde_json::json!({
            "valid_from": "2026-11-19T08:00:00Z", "valid_until": "2026-11-19T13:00:00Z"
        });
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
        assert_eq!(
            loaded
                .stick_key_window
                .as_ref()
                .expect("key window")
                .valid_from,
            "2026-11-19T08:00:00Z"
        );
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
        assert!(seen[4].contains("2026-11-19T13:00:00Z"));
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
    fn retiring_a_key_uses_the_signed_in_admin_and_the_exact_stick_id() {
        let mut replies = signed_in_replies();
        replies.push(me());
        replies.push(reply("204 No Content", "", ""));
        let (hub, handle) = fake_hub(replies);
        let session = sign_in_with_code(&hub, "c").expect("session");
        assert!(retire_stick_credential(&hub, &session, "../another-stick").is_err());
        retire_stick_credential(&hub, &session, "old-stick").expect("retire");
        let seen = handle.join().expect("hub");
        assert!(seen[2].starts_with("DELETE /api/v1/orgs/acm/sticks/old-stick/credential "));
        assert!(
            seen[2]
                .to_lowercase()
                .contains("authorization: bearer tok123")
        );
    }

    #[test]
    fn an_account_with_no_or_several_organizations_is_refused() {
        for (answer, words) in [
            (r#"{"organizations":[]}"#, "no organization"),
            (
                r#"{"organizations":[{"slug":"a"},{"slug":"b"}]}"#,
                "several organizations",
            ),
            (r#"{"organizations":[{"slug":"../x"}]}"#, "cannot use"),
        ] {
            let mut replies = signed_in_replies();
            replies.push(reply("200 OK", "", answer));
            let (hub, _handle) = fake_hub(replies);
            let session = sign_in_with_code(&hub, "c").expect("session");
            let error = retire_stick_credential(&hub, &session, "old-stick").unwrap_err();
            assert!(error.to_string().contains(words), "{answer}: {error}");
        }
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
