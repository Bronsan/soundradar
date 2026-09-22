//! Library management HTTP UI + JSON API (127.0.0.1 only).

use crate::{config as cfgmod, dsp, index as idxmod, library as libmod, wav};
use anyhow::{Context, Result};
use std::io::Read;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

struct AppState {
    lib_path: PathBuf,
    idx_path: PathBuf,
    cfg_path: PathBuf,
    store: libmod::Store,
    idx: idxmod::Index,
    cfg: cfgmod::Config,
}

const INDEX_HTML: &str = include_str!("static/index.html");
const STYLE_CSS: &str = include_str!("static/style.css");
const APP_JS: &str = include_str!("static/app.js");

pub fn run(
    port: u16,
    open: bool,
    library: Option<PathBuf>,
    overlay: bool,
    config: Option<PathBuf>,
) -> Result<()> {
    let lib_path = library.unwrap_or_else(libmod::default_path);
    let idx_path = idxmod::default_path_for(&lib_path);
    let cfg_path = config.unwrap_or_else(cfgmod::default_path);
    let store = if lib_path.exists() {
        libmod::Store::open(&lib_path)?
    } else {
        libmod::create_empty(&lib_path)?
    };
    let idx = if idx_path.exists() {
        idxmod::Index::load(&idx_path).unwrap_or_else(|_| {
            // placeholder empty
            idxmod::Index {
                params: idxmod::Params::default(),
                params_fingerprint: idxmod::Params::default().fingerprint(),
                items: vec![],
                samples: vec![],
                patches: vec![],
                anchors: vec![],
            }
        })
    } else {
        idxmod::Index {
            params: idxmod::Params::default(),
            params_fingerprint: idxmod::Params::default().fingerprint(),
            items: vec![],
            samples: vec![],
            patches: vec![],
            anchors: vec![],
        }
    };
    let cfg = cfgmod::Config::load(&cfg_path).unwrap_or_default();

    let state = Arc::new(Mutex::new(AppState {
        lib_path,
        idx_path,
        cfg_path,
        store,
        idx,
        cfg,
    }));

    let mut bound = None;
    for delta in 0..20u16 {
        let p = port + delta;
        match tiny_http::Server::http(("127.0.0.1", p)) {
            Ok(s) => {
                println!("[serve] soundradar P1 音效库管理端");
                println!("[serve] 地址         : http://127.0.0.1:{}/", p);
                if overlay {
                    println!("[serve] 覆盖层       : 已启用");
                }
                bound = Some((s, p));
                break;
            }
            Err(_) => continue,
        }
    }
    let (server, port) = bound.ok_or_else(|| anyhow::anyhow!("[serve] 无法绑定端口"))?;
    if open {
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", "", &format!("http://127.0.0.1:{}/", port)])
            .spawn();
    }

    for mut req in server.incoming_requests() {
        let url = req.url().to_string();
        let method = req.method().clone();
        let mut body = Vec::new();
        let _ = req.as_reader().read_to_end(&mut body);
        let st = state.clone();
        let resp = handle(st, method, &url, &body);
        let _ = req.respond(resp);
    }
    Ok(())
}

fn json_resp(code: u16, s: String) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
    let mut r = tiny_http::Response::from_data(s.into_bytes()).with_status_code(code);
    r.add_header(
        tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json; charset=utf-8"[..])
            .unwrap(),
    );
    r
}

fn text_resp(code: u16, body: &'static str, ctype: &'static str) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
    let mut r = tiny_http::Response::from_data(body.as_bytes().to_vec()).with_status_code(code);
    r.add_header(tiny_http::Header::from_bytes(&b"Content-Type"[..], ctype.as_bytes()).unwrap());
    r
}

fn bytes_resp(code: u16, data: Vec<u8>, ctype: &str) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
    let mut r = tiny_http::Response::from_data(data).with_status_code(code);
    if let Ok(h) = tiny_http::Header::from_bytes(&b"Content-Type"[..], ctype.as_bytes()) {
        r.add_header(h);
    }
    r
}

fn handle(
    st: Arc<Mutex<AppState>>,
    method: tiny_http::Method,
    url: &str,
    body: &[u8],
) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
    let path = url.split('?').next().unwrap_or("/");
    match (method, path) {
        (tiny_http::Method::Get, "/") | (tiny_http::Method::Get, "/index.html") => {
            text_resp(200, INDEX_HTML, "text/html; charset=utf-8")
        }
        (tiny_http::Method::Get, "/style.css") => text_resp(200, STYLE_CSS, "text/css; charset=utf-8"),
        (tiny_http::Method::Get, "/app.js") => {
            text_resp(200, APP_JS, "application/javascript; charset=utf-8")
        }
        (tiny_http::Method::Get, "/api/library") => {
            let g = st.lock().unwrap();
            let (n_items, n_samples) = g.store.stats();
            let v = serde_json::json!({
                "path": g.lib_path.display().to_string(),
                "name": g.store.manifest.name,
                "items": g.store.order.iter().filter_map(|id| {
                    g.store.items.get(id).map(|it| serde_json::json!({
                        "id": it.id,
                        "name": it.name,
                        "icon": it.icon,
                        "threshold": it.threshold,
                        "cooldownMs": it.cooldown_ms,
                        "tags": it.tags,
                        "note": it.note,
                        "createdAt": it.created_at,
                        "updatedAt": it.updated_at,
                        "samples": it.samples.len(),
                    }))
                }).collect::<Vec<_>>(),
                "count": n_items,
                "sampleCount": n_samples,
                "feature": g.store.manifest.feature,
            });
            json_resp(200, v.to_string())
        }
        (tiny_http::Method::Get, "/api/config") => {
            let g = st.lock().unwrap();
            json_resp(200, serde_json::to_string_pretty(&g.cfg).unwrap_or_default())
        }
        (tiny_http::Method::Patch, "/api/config") | (tiny_http::Method::Post, "/api/config") => {
            let mut g = st.lock().unwrap();
            if let Ok(v) = serde_json::from_slice::<serde_json::Value>(body) {
                if let Some(c) = serde_json::from_value::<cfgmod::Config>(v.clone()).ok() {
                    g.cfg = c;
                    let _ = g.cfg.save(&g.cfg_path.clone());
                }
            }
            json_resp(200, serde_json::to_string_pretty(&g.cfg).unwrap_or_default())
        }
        (tiny_http::Method::Get, p) if p.starts_with("/api/items/") && p.ends_with("/icon.png") => {
            let id = p
                .trim_start_matches("/api/items/")
                .trim_end_matches("/icon.png");
            let id = id.split('/').next().unwrap_or("");
            let g = st.lock().unwrap();
            if let Some(icon) = g.store.icon_png(id) {
                bytes_resp(200, icon.to_vec(), "image/png")
            } else {
                bytes_resp(200, libmod::placeholder_icon(), "image/png")
            }
        }
        (tiny_http::Method::Get, p) if p.starts_with("/api/items/") => {
            let id = p.trim_start_matches("/api/items/").trim_end_matches('/');
            let g = st.lock().unwrap();
            match g.store.items.get(id) {
                Some(it) => json_resp(200, serde_json::to_string_pretty(it).unwrap_or_default()),
                None => json_resp(404, r#"{"error":"not found"}"#.into()),
            }
        }
        (tiny_http::Method::Get, "/api/overlay") => {
            json_resp(200, r#"{"available":true,"visible":true}"#.into())
        }
        (tiny_http::Method::Get, "/api/live") | (tiny_http::Method::Get, "/api/live/devices") => {
            json_resp(200, r#"{"devices":[{"id":"default","name":"默认扬声器"}],"source":"file"}"#.into())
        }
        (tiny_http::Method::Get, "/api/candidates") => {
            let g = st.lock().unwrap();
            let dir = std::path::Path::new(&g.cfg.recall.dir);
            let dir = if dir.is_absolute() {
                dir.to_path_buf()
            } else {
                g.lib_path
                    .parent()
                    .map(|p| p.join(&g.cfg.recall.dir))
                    .unwrap_or_else(|| dir.to_path_buf())
            };
            let mut items = vec![];
            if let Ok(rd) = std::fs::read_dir(&dir) {
                for e in rd.flatten() {
                    if e.path().extension().and_then(|s| s.to_str()) == Some("wav") {
                        items.push(serde_json::json!({
                            "id": e.file_name().to_string_lossy(),
                            "path": e.path().display().to_string(),
                        }));
                    }
                }
            }
            let v = serde_json::json!({"count": items.len(), "items": items, "ringSeconds": g.cfg.recall.seconds, "coveredSeconds": g.cfg.recall.seconds});
            json_resp(200, v.to_string())
        }
        _ => json_resp(404, r#"{"error":"not found"}"#.into()),
    }
}

#[allow(dead_code)]
fn rebuild_from_store(st: &mut AppState) -> Result<()> {
    let params = idxmod::Params::default();
    let mut items = Vec::new();
    let mut samples = Vec::new();
    let mut patches = Vec::new();
    let mut anchors = Vec::new();
    let mut cursor = 0u32;
    for id in &st.store.order.clone() {
        let it = st.store.items.get(id).unwrap();
        let start = cursor;
        let mut n_ok = 0u32;
        for s in &it.samples {
            if let Some(raw) = st.store.sample_wav(&it.id, &s.file) {
                let audio = wav::read_wav_bytes(raw)?;
                let mono = audio.canonical_mono_48k();
                if let Some((q, a)) = dsp::fingerprint_best_patch(&mono) {
                    patches.push(q);
                    anchors.push(a);
                    samples.push(idxmod::SampleEntry {
                        path: s.file.clone(),
                        item_index: items.len() as u32,
                        sample_index: n_ok as u16,
                        energy: 0.0,
                    });
                    n_ok += 1;
                    cursor += 1;
                }
            }
        }
        items.push(idxmod::ItemEntry {
            id: it.id.clone(),
            name: it.name.clone(),
            sample_count: n_ok,
            sample_start: start,
            threshold: it.threshold,
            cooldown_ms: it.cooldown_ms,
        });
    }
    st.idx = idxmod::Index {
        params_fingerprint: params.fingerprint(),
        params,
        items,
        samples,
        patches,
        anchors,
    };
    let path = st.idx_path.clone();
    st.idx.save(&path)?;
    Ok(())
}
