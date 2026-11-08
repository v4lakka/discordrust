use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, Query, State,
    },
    response::{Html, Response},
    routing::{get, post},
    Json, Router,
};
use futures_util::{SinkExt, StreamExt};
use rusqlite::{params, Connection};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::broadcast;

struct App {
    db: Mutex<Connection>,
    // (id de canal o "*" para todos, json ya serializado)
    tx: broadcast::Sender<Arc<(String, String)>>,
    online: Mutex<BTreeMap<String, usize>>,
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn code() -> String {
    let n = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("{:016x}", n)[8..].to_string()
}

fn clean(s: &str, n: usize) -> String {
    s.trim().chars().filter(|c| !c.is_control()).take(n).collect()
}

fn broadcast_all(a: &App, v: Value) {
    let _ = a.tx.send(Arc::new(("*".to_string(), v.to_string())));
}

#[tokio::main]
async fn main() {
    let db = Connection::open("data.db").expect("no pude abrir la base");
    db.execute_batch(
        "PRAGMA journal_mode=WAL;
         CREATE TABLE IF NOT EXISTS servers(id INTEGER PRIMARY KEY, name TEXT NOT NULL, code TEXT UNIQUE NOT NULL);
         CREATE TABLE IF NOT EXISTS channels(id INTEGER PRIMARY KEY, server_id INTEGER NOT NULL, name TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS msgs(id INTEGER PRIMARY KEY, ch INTEGER NOT NULL, user TEXT NOT NULL, text TEXT NOT NULL, ts INTEGER NOT NULL);
         CREATE INDEX IF NOT EXISTS i_ch ON msgs(ch, id);",
    )
    .expect("no pude crear las tablas");

    let vacio = db.query_row("SELECT COUNT(*) FROM servers", [], |r| r.get::<_, i64>(0)).unwrap_or(0) == 0;
    if vacio {
        db.execute("INSERT INTO servers(name, code) VALUES('Inicio', 'inicio')", []).ok();
        db.execute("INSERT INTO channels(server_id, name) VALUES(1, 'general'), (1, 'random')", []).ok();
    }

    let (tx, _) = broadcast::channel(256);
    let app = Arc::new(App { db: Mutex::new(db), tx, online: Mutex::new(BTreeMap::new()) });

    let router = Router::new()
        .route("/", get(|| async { Html(include_str!("index.html")) }))
        .route("/s", post(new_srv))
        .route("/s/:code", get(get_srv))
        .route("/s/:code/c", post(new_chan))
        .route("/h/:ch", get(history))
        .route("/ws", get(ws))
        .with_state(app);

    let addr = std::env::var("ADDR").unwrap_or_else(|_| "0.0.0.0:3000".into());
    let listener = tokio::net::TcpListener::bind(&addr).await.expect("no pude abrir el puerto");
    println!("minicord escuchando en http://{addr}");
    open_browser(&addr);
    axum::serve(listener, router).await.unwrap();
}

#[cfg(windows)]
fn open_browser(addr: &str) {
    if addr.ends_with(":3000") {
        let _ = std::process::Command::new("cmd").args(["/C", "start", "http://localhost:3000"]).spawn();
    }
}
#[cfg(not(windows))]
fn open_browser(_addr: &str) {}

fn srv(db: &Connection, code: &str) -> Option<Value> {
    let (id, name): (i64, String) = db
        .query_row("SELECT id, name FROM servers WHERE code = ?1", params![code], |r| Ok((r.get(0)?, r.get(1)?)))
        .ok()?;
    let mut st = db.prepare("SELECT id, name FROM channels WHERE server_id = ?1 ORDER BY id").ok()?;
    let chs: Vec<Value> = st
        .query_map(params![id], |r| Ok(json!({ "id": r.get::<_, i64>(0)?, "name": r.get::<_, String>(1)? })))
        .ok()?
        .filter_map(Result::ok)
        .collect();
    Some(json!({ "id": id, "name": name, "code": code, "channels": chs }))
}

#[derive(Deserialize)]
struct Nm {
    name: String,
}

async fn get_srv(State(a): State<Arc<App>>, Path(code): Path<String>) -> Json<Value> {
    let db = a.db.lock().unwrap();
    let v = srv(&db, &code).unwrap_or(Value::Null);
    Json(v)
}

async fn new_srv(State(a): State<Arc<App>>, Json(b): Json<Nm>) -> Json<Value> {
    let name = clean(&b.name, 32);
    if name.is_empty() {
        return Json(Value::Null);
    }
    let c = code();
    let db = a.db.lock().unwrap();
    db.execute("INSERT INTO servers(name, code) VALUES(?1, ?2)", params![name, c]).ok();
    let sid = db.last_insert_rowid();
    db.execute("INSERT INTO channels(server_id, name) VALUES(?1, 'general')", params![sid]).ok();
    let v = srv(&db, &c).unwrap_or(Value::Null);
    Json(v)
}

async fn new_chan(State(a): State<Arc<App>>, Path(sid): Path<i64>, Json(b): Json<Nm>) -> Json<Value> {
    let name = clean(&b.name, 32).to_lowercase().replace(' ', "-");
    if name.is_empty() {
        return Json(Value::Null);
    }
    a.db.lock().unwrap().execute("INSERT INTO channels(server_id, name) VALUES(?1, ?2)", params![sid, name]).ok();
    broadcast_all(&a, json!({ "t": "chan", "sid": sid }));
    Json(json!(true))
}

fn chan_ok(a: &App, id: i64) -> bool {
    a.db.lock().unwrap().query_row("SELECT 1 FROM channels WHERE id = ?1", params![id], |_| Ok(())).is_ok()
}

async fn history(State(a): State<Arc<App>>, Path(ch): Path<i64>) -> Json<Vec<Value>> {
    let db = a.db.lock().unwrap();
    let mut st = db
        .prepare("SELECT user, text, ts FROM (SELECT * FROM msgs WHERE ch = ?1 ORDER BY id DESC LIMIT 50) ORDER BY id")
        .unwrap();
    let rows: Vec<Value> = st
        .query_map(params![ch], |r| {
            Ok(json!({ "t": "msg", "user": r.get::<_, String>(0)?, "text": r.get::<_, String>(1)?, "ts": r.get::<_, i64>(2)? }))
        })
        .unwrap()
        .filter_map(Result::ok)
        .collect();
    Json(rows)
}

#[derive(Deserialize)]
struct Q {
    name: String,
}

async fn ws(u: WebSocketUpgrade, Query(q): Query<Q>, State(a): State<Arc<App>>) -> Response {
    let name = clean(&q.name, 24);
    let name = if name.is_empty() { "anon".to_string() } else { name };
    u.max_message_size(8192).on_upgrade(move |s| conn(s, a, name))
}

fn presence(a: &App) {
    let users: Vec<String> = a.online.lock().unwrap().keys().cloned().collect();
    broadcast_all(a, json!({ "t": "users", "users": users }));
}

async fn conn(sock: WebSocket, a: Arc<App>, name: String) {
    let (mut out, mut inp) = sock.split();
    let mut rx = a.tx.subscribe();
    *a.online.lock().unwrap().entry(name.clone()).or_insert(0) += 1;
    presence(&a);

    let (mut ch, mut chs) = (0i64, String::new());

    loop {
        tokio::select! {
            m = inp.next() => match m {
                Some(Ok(Message::Text(t))) => {
                    let Ok(v) = serde_json::from_str::<Value>(&t) else { continue };
                    match v["t"].as_str() {
                        Some("join") => {
                            if let Some(c) = v["ch"].as_i64() {
                                if chan_ok(&a, c) {
                                    ch = c;
                                    chs = c.to_string();
                                }
                            }
                        }
                        Some("msg") => {
                            let text = clean(v["text"].as_str().unwrap_or(""), 2000);
                            if text.is_empty() || ch == 0 {
                                continue;
                            }
                            let ts = now();
                            a.db.lock().unwrap()
                                .execute("INSERT INTO msgs(ch, user, text, ts) VALUES (?1, ?2, ?3, ?4)", params![ch, name, text, ts])
                                .ok();
                            let j = json!({ "t": "msg", "user": name, "text": text, "ts": ts }).to_string();
                            let _ = a.tx.send(Arc::new((chs.clone(), j)));
                        }
                        _ => {}
                    }
                }
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                _ => {}
            },
            b = rx.recv() => match b {
                Ok(m) => {
                    if (m.0 == "*" || m.0 == chs) && out.send(Message::Text(m.1.clone())).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(_) => break,
            }
        }
    }

    {
        let mut o = a.online.lock().unwrap();
        if let Some(n) = o.get_mut(&name) {
            *n -= 1;
            if *n == 0 {
                o.remove(&name);
            }
        }
    }
    presence(&a);
}
