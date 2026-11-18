//! The cloud, answered by a script instead of by Bambu.
//!
//! Nothing here touches the network: [`Script`] is a [`Transport`] that hands
//! back prepared answers and keeps the requests it was given, so the tests can
//! assert both what came out and what went in. The last test in the file is the
//! exception and the only one that opens a socket — a loopback server, to prove
//! the real client speaks HTTP.

use std::sync::{Arc, Mutex};

use shale_host::cloud::http::Response;
use shale_host::cloud::{
    api, devices, Cloud, CloudError, HttpRequest, HttpTransport, MemoryStore, SecretStore, SignIn,
    Transport,
};

/// Answers in order, and remembers what it was asked.
#[derive(Default)]
struct Script {
    answers: Mutex<Vec<Response>>,
    seen: Mutex<Vec<HttpRequest>>,
}

impl Script {
    fn new(answers: Vec<Response>) -> Arc<Self> {
        Arc::new(Self {
            answers: Mutex::new(answers),
            seen: Mutex::new(Vec::new()),
        })
    }

    fn seen(&self) -> Vec<HttpRequest> {
        self.seen.lock().unwrap().clone()
    }
}

impl Transport for Script {
    fn send(&self, request: HttpRequest) -> Result<Response, String> {
        self.seen.lock().unwrap().push(request);
        let mut answers = self.answers.lock().unwrap();
        if answers.is_empty() {
            return Err("the script ran out of answers".into());
        }
        Ok(answers.remove(0))
    }
}

fn answer(status: u16, json: &str) -> Response {
    Response {
        status,
        body: json.as_bytes().to_vec(),
    }
}

fn cloud(script: Arc<Script>, store: Arc<MemoryStore>) -> Cloud<Script> {
    let mut cloud = Cloud::new(api::GLOBAL_HOST, script, store);
    cloud.load();
    cloud
}

#[test]
fn a_password_sign_in_keeps_the_tokens_and_who_they_belong_to() {
    let script = Script::new(vec![answer(
        200,
        r#"{"accessToken":"a1","refreshToken":"r1","expiresIn":86400}"#,
    )]);
    let store = Arc::new(MemoryStore::default());
    let mut cloud = cloud(script.clone(), store.clone());

    assert_eq!(cloud.sign_in("a@b.c", "pw").unwrap(), SignIn::SignedIn);
    assert!(cloud.signed_in());
    assert_eq!(cloud.account(), Some("a@b.c"));

    // What went out: one POST, the password in the body, nothing in the URL.
    let seen = script.seen();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].method, "POST");
    assert_eq!(
        seen[0].url,
        "https://api.bambulab.com/v1/user-service/user/login"
    );
    let sent = String::from_utf8(seen[0].body.clone().unwrap()).unwrap();
    assert!(sent.contains("\"account\":\"a@b.c\""));
    assert!(sent.contains("\"password\":\"pw\""));

    // What stayed behind: the tokens, in the store, not in a log line.
    assert_eq!(store.get("cloud.access").unwrap().as_deref(), Some("a1"));
    assert_eq!(store.get("cloud.refresh").unwrap().as_deref(), Some("r1"));
    assert_eq!(
        store.get("cloud.account").unwrap().as_deref(),
        Some("a@b.c")
    );
}

#[test]
fn a_second_run_finds_the_account_without_asking_again() {
    let script = Script::new(vec![]);
    let store = Arc::new(MemoryStore::default());
    store.set("cloud.access", "a1").unwrap();
    store.set("cloud.refresh", "r1").unwrap();
    store
        .set(
            "cloud.expires",
            &(shale_host::cloud::now_unix() + 3600).to_string(),
        )
        .unwrap();
    store.set("cloud.account", "a@b.c").unwrap();

    let mut cloud = cloud(script.clone(), store);
    cloud.load();
    assert!(cloud.signed_in());
    assert_eq!(cloud.account(), Some("a@b.c"));
    assert!(script.seen().is_empty());
}

#[test]
fn a_code_request_is_an_answer_and_not_a_failure() {
    let script = Script::new(vec![answer(
        200,
        r#"{"loginType":"verifyCode","tfaKey":"key-1","accessToken":"","refreshToken":""}"#,
    )]);
    let store = Arc::new(MemoryStore::default());
    let mut cloud = cloud(script, store);

    let outcome = cloud.sign_in("a@b.c", "pw").unwrap();
    assert_eq!(
        outcome,
        SignIn::NeedsCode {
            tfa_key: "key-1".into()
        }
    );
    assert!(!cloud.signed_in());
}

#[test]
fn the_code_completes_the_same_sign_in() {
    let script = Script::new(vec![answer(
        200,
        r#"{"accessToken":"a2","refreshToken":"r2","expiresIn":3600}"#,
    )]);
    let store = Arc::new(MemoryStore::default());
    let mut cloud = cloud(script.clone(), store.clone());

    assert_eq!(
        cloud.submit_code("a@b.c", "key-1", "123456").unwrap(),
        SignIn::SignedIn
    );
    let sent = String::from_utf8(script.seen()[0].body.clone().unwrap()).unwrap();
    assert!(sent.contains("\"code\":\"123456\""));
    assert!(sent.contains("\"tfaKey\":\"key-1\""));
    assert_eq!(store.get("cloud.access").unwrap().as_deref(), Some("a2"));
}

#[test]
fn a_wrong_code_says_which_call_refused_it_and_what_bambu_said() {
    let script = Script::new(vec![answer(400, r#"{"error":"the code is wrong"}"#)]);
    let store = Arc::new(MemoryStore::default());
    let mut cloud = cloud(script, store);

    let err = cloud.submit_code("a@b.c", "key-1", "000000").unwrap_err();
    match &err {
        CloudError::Status {
            call,
            status,
            message,
        } => {
            assert_eq!(*status, 400);
            assert!(call.contains("/login"), "{call}");
            assert_eq!(message, "the code is wrong");
        }
        other => panic!("{other:?}"),
    }
    assert!(!cloud.signed_in());
}

#[test]
fn an_expired_token_is_refreshed_before_it_is_used() {
    let script = Script::new(vec![
        answer(
            200,
            r#"{"accessToken":"a3","refreshToken":"r3","expiresIn":7200}"#,
        ),
        answer(200, r#"{"devices":[]}"#),
    ]);
    let store = Arc::new(MemoryStore::default());
    store.set("cloud.access", "old").unwrap();
    store.set("cloud.refresh", "r1").unwrap();
    store.set("cloud.expires", "1").unwrap(); // 1970
    store.set("cloud.account", "a@b.c").unwrap();

    let mut cloud = cloud(script.clone(), store.clone());
    cloud.load();
    assert!(cloud.devices().unwrap().is_empty());

    let seen = script.seen();
    assert_eq!(
        seen[0].url,
        "https://api.bambulab.com/v1/user-service/user/refreshtoken"
    );
    assert_eq!(
        seen[1].url,
        "https://api.bambulab.com/v1/iot-service/api/user/device/list"
    );
    assert!(seen[1]
        .headers
        .iter()
        .any(|(name, value)| name == "Authorization" && value == "Bearer a3"));
    assert_eq!(store.get("cloud.access").unwrap().as_deref(), Some("a3"));
}

#[test]
fn a_device_list_arrives_sorted_with_the_machines_that_can_print_first() {
    let script = Script::new(vec![
        answer(
            200,
            r#"{"accessToken":"a","refreshToken":"r","expiresIn":3600}"#,
        ),
        answer(
            200,
            r#"{"devices":[
                {"dev_id":"01","name":"beta","online":true,"print_status":"RUNNING","mc_percent":12},
                {"dev_id":"02","name":"alpha","online":true,"print_status":"IDLE"},
                {"dev_id":"03","name":"gone","online":false}
            ]}"#,
        ),
    ]);
    let store = Arc::new(MemoryStore::default());
    let mut cloud = cloud(script, store);
    cloud.sign_in("a@b.c", "pw").unwrap();

    let list = cloud.devices().unwrap();
    let rows = devices::rows(&list);
    assert_eq!(rows[0].label, "alpha");
    assert!(rows[0].state.may_print());
    assert_eq!(rows[1].state.line(), "printing, 12%");
    assert_eq!(rows[2].state, devices::State::Offline);
}

#[test]
fn sending_a_print_needs_a_printer_and_a_file() {
    let script = Script::new(vec![]);
    let store = Arc::new(MemoryStore::default());
    let mut cloud = cloud(script, store);

    assert!(matches!(
        cloud.send_print("", "cube", b"G28\n").unwrap_err(),
        CloudError::Refused(_)
    ));
    assert!(matches!(
        cloud.send_print("01", "cube", b"").unwrap_err(),
        CloudError::Refused(_)
    ));
}

#[test]
fn a_sent_print_returns_the_task_the_cloud_made() {
    let script = Script::new(vec![
        answer(
            200,
            r#"{"accessToken":"a","refreshToken":"r","expiresIn":3600}"#,
        ),
        answer(200, r#"{"id":12345}"#),
    ]);
    let store = Arc::new(MemoryStore::default());
    let mut cloud = cloud(script.clone(), store);
    cloud.sign_in("a@b.c", "pw").unwrap();

    let id = cloud.send_print("01P00", "cube", b"G28\n").unwrap();
    assert_eq!(id, "12345");

    let seen = script.seen();
    let upload = &seen[1];
    assert_eq!(
        upload.url,
        "https://api.bambulab.com/v1/iot-service/api/user/task"
    );
    assert!(upload
        .headers
        .iter()
        .any(|(name, value)| name == "Content-Type" && value.starts_with("multipart/form-data")));
    let body = String::from_utf8(upload.body.clone().unwrap()).unwrap();
    assert!(body.contains("name=\"task\""));
    assert!(body.contains("\"dev_id\":\"01P00\""));
    assert!(body.contains("name=\"file\"; filename=\"plate.gcode\""));
}

#[test]
fn being_signed_out_is_said_plainly() {
    let script = Script::new(vec![]);
    let store = Arc::new(MemoryStore::default());
    let mut cloud = cloud(script, store);
    assert_eq!(cloud.devices().unwrap_err(), CloudError::SignedOut);
}

#[test]
fn no_error_carries_a_token() {
    let script = Script::new(vec![answer(500, r#"{"error":"internal"}"#)]);
    let store = Arc::new(MemoryStore::default());
    store.set("cloud.access", "SECRET-ACCESS").unwrap();
    store.set("cloud.refresh", "SECRET-REFRESH").unwrap();
    store
        .set(
            "cloud.expires",
            &(shale_host::cloud::now_unix() + 3600).to_string(),
        )
        .unwrap();
    let mut cloud = cloud(script, store);
    cloud.load();

    let text = cloud.devices().unwrap_err().to_string();
    assert!(!text.contains("SECRET-ACCESS"), "{text}");
    assert!(!text.contains("SECRET-REFRESH"), "{text}");
    assert!(text.contains("500"), "{text}");
}

/// The one test with a socket: a loopback server that answers HTTP/1.1 with
/// chunked encoding, to prove the client parses what a real one sends.
#[test]
fn the_real_client_speaks_http_to_a_loopback_server() {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buffer = [0u8; 2048];
            let read = stream.read(&mut buffer).unwrap_or(0);
            let request = String::from_utf8_lossy(&buffer[..read]).to_string();
            let body = if request.contains("Content-Length: 2") {
                "4\r\n{\"a\"\r\n3\r\n:1}\r\n0\r\n\r\n".to_string()
            } else {
                r#"{"devices":[]}"#.to_string()
            };
            let head = if body.starts_with("4\r\n") {
                "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n"
                    .to_string()
            } else {
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
            };
            let _ = stream.write_all(format!("{head}{body}").as_bytes());
        }
    });

    let transport = HttpTransport;
    let first = transport
        .send(HttpRequest {
            name: "GET /devices",
            method: "GET",
            url: format!("http://127.0.0.1:{port}/devices"),
            headers: vec![("Accept".into(), "application/json".into())],
            body: None,
        })
        .unwrap();
    assert_eq!(first.status, 200);
    assert_eq!(
        first.json().unwrap()["devices"].as_array().unwrap().len(),
        0
    );

    let second = transport
        .send(HttpRequest {
            name: "POST /echo",
            method: "POST",
            url: format!("http://127.0.0.1:{port}/echo"),
            headers: vec![("Content-Type".into(), "application/json".into())],
            body: Some(b"{}".to_vec()),
        })
        .unwrap();
    assert_eq!(second.json().unwrap()["a"], 1);
    server.join().unwrap();
}
