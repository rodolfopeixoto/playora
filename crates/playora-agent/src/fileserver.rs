//! Local HTTP file browser served by the agent on :7878.
//!
//! Endpoints:
//!   GET  /                     → browsable HTML index
//!   GET  /api/list?path=...    → JSON directory listing
//!   GET  /api/download?path=...→ stream file (supports Range for resumable downloads)
//!   POST /api/upload?path=...  → multipart file upload; if filename ends in .zip
//!                                we auto-extract on receipt and delete the zip
//!   POST /api/mkdir?path=...   → create directory
//!   POST /api/delete?path=...  → delete file/directory
//!
//! All paths are restricted to ALLOWED_ROOTS for safety.

use anyhow::Result;
use axum::body::Body;
use axum::extract::{Multipart, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, Request, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const ALLOWED_ROOTS: &[&str] = &[
    "/roms",
    "/boot",
    "/userdata",
    "/home",
    "/tmp",
    "/opt",
    "/etc",
    "/var",
    "/run",
];

#[derive(Clone)]
struct AppState {
    pin: String,
}

pub fn cmd_serve(bind: &str) -> Result<()> {
    use rand::Rng as _;
    let pin: String = {
        let mut rng = rand::thread_rng();
        (0..6).map(|_| rng.gen_range(0..10).to_string()).collect()
    };
    println!();
    println!("\x1b[1;35m╔══════════════════════════════════════════════════════╗\x1b[0m");
    println!("\x1b[1;35m║\x1b[0m  \x1b[1;37mFile Browser PIN: \x1b[1;33m{pin}\x1b[1;37m                              \x1b[1;35m║\x1b[0m");
    println!("\x1b[1;35m║\x1b[0m  \x1b[2mEnter this on http://<device-ip>:7878 to unlock\x1b[0m  \x1b[1;35m║\x1b[0m");
    println!("\x1b[1;35m╚══════════════════════════════════════════════════════╝\x1b[0m");
    println!();

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async move {
        let state = Arc::new(AppState { pin });
        let cors = tower_http::cors::CorsLayer::permissive();
        let app = Router::new()
            .route("/", get(index_page))
            .route("/login", get(login_page).post(login_submit))
            .route("/api/list", get(list_dir))
            .route("/api/download", get(download_file))
            .route("/api/upload", post(upload_file))
            .route("/api/mkdir", post(mkdir))
            .route("/api/delete", post(delete_path))
            .route("/api/move", post(move_path))
            .route("/api/restart-es", post(restart_es))
            .layer(axum::extract::DefaultBodyLimit::max(
                50 * 1024 * 1024 * 1024,
            ))
            .layer(cors)
            .with_state(state);
        let listener = tokio::net::TcpListener::bind(bind).await?;
        println!("Playora file server listening on http://{bind}/");
        println!("Browse from the dashboard's Device page → File Browser link.");
        axum::serve(listener, app).await?;
        Ok::<_, anyhow::Error>(())
    })
}

fn check_pin(headers: &HeaderMap, state: &AppState) -> Result<(), StatusCode> {
    let cookie = headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    for kv in cookie.split(';') {
        let kv = kv.trim();
        if let Some(v) = kv.strip_prefix("playora_pin=") {
            if v == state.pin {
                return Ok(());
            }
        }
    }
    Err(StatusCode::UNAUTHORIZED)
}

async fn login_page() -> Html<&'static str> {
    Html(LOGIN_HTML)
}

#[derive(Deserialize)]
struct LoginForm {
    pin: String,
}

async fn login_submit(
    State(state): State<Arc<AppState>>,
    axum::Form(form): axum::Form<LoginForm>,
) -> Response {
    if form.pin.trim() == state.pin {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::SET_COOKIE,
            HeaderValue::from_str(&format!(
                "playora_pin={}; Path=/; Max-Age=86400; SameSite=Lax",
                state.pin
            ))
            .unwrap(),
        );
        headers.insert(header::LOCATION, HeaderValue::from_static("/"));
        (StatusCode::SEE_OTHER, headers, "").into_response()
    } else {
        (
            StatusCode::UNAUTHORIZED,
            Html("<p>Wrong PIN — <a href=\"/login\">try again</a>.</p>"),
        )
            .into_response()
    }
}

async fn restart_es(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<StatusCode, StatusCode> {
    check_pin(&headers, &state)?;
    let _ = std::process::Command::new("sudo")
        .args(["systemctl", "restart", "emulationstation"])
        .status();
    Ok(StatusCode::OK)
}

#[derive(Deserialize)]
struct MoveForm {
    from: String,
    to: String,
}

async fn move_path(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(form): Json<MoveForm>,
) -> Result<StatusCode, StatusCode> {
    check_pin(&headers, &state)?;
    let from = safe(&form.from)?;
    let to = safe(&form.to)?;
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::fs::rename(&from, &to).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::OK)
}

#[derive(Deserialize)]
struct PathQuery {
    path: String,
}

fn safe(path: &str) -> Result<PathBuf, StatusCode> {
    let p = PathBuf::from(path);
    // Resolve relative parents; canonicalize would fail on not-yet-created dirs.
    let resolved: PathBuf = p
        .components()
        .filter(|c| !matches!(c, std::path::Component::ParentDir))
        .collect();
    let s = resolved.to_string_lossy().to_string();
    if !ALLOWED_ROOTS.iter().any(|r| s.starts_with(r)) {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(resolved)
}

async fn list_dir(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<PathQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    check_pin(&headers, &state)?;
    let p = safe(&q.path)?;
    if !p.is_dir() {
        return Err(StatusCode::NOT_FOUND);
    }
    let mut entries: Vec<serde_json::Value> = Vec::new();
    let rd = std::fs::read_dir(&p).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    for e in rd.flatten() {
        let path = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        let md = match e.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        entries.push(json!({
            "name": name,
            "path": path.display().to_string(),
            "is_dir": md.is_dir(),
            "size": md.len(),
            "modified": md.modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
        }));
    }
    entries.sort_by(|a, b| {
        let ad = a["is_dir"].as_bool().unwrap_or(false);
        let bd = b["is_dir"].as_bool().unwrap_or(false);
        bd.cmp(&ad)
            .then_with(|| a["name"].as_str().cmp(&b["name"].as_str()))
    });
    Ok(Json(json!({
        "path": p.display().to_string(),
        "entries": entries,
        "allowed_roots": ALLOWED_ROOTS,
    })))
}

async fn download_file(
    State(state): State<Arc<AppState>>,
    Query(q): Query<PathQuery>,
    req: Request<Body>,
) -> Result<Response, StatusCode> {
    check_pin(req.headers(), &state)?;
    let p = safe(&q.path)?;
    if !p.is_file() {
        return Err(StatusCode::NOT_FOUND);
    }
    let md = std::fs::metadata(&p).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let total = md.len();
    let filename = p
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("file")
        .to_string();

    // Range support for resumable downloads.
    let range = req
        .headers()
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok());
    let (start, end) = if let Some(r) = range {
        parse_range(r, total).unwrap_or((0, total - 1))
    } else {
        (0, total - 1)
    };

    use tokio::io::AsyncSeekExt;
    let mut f = tokio::fs::File::open(&p)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    f.seek(std::io::SeekFrom::Start(start))
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let take = f.take(end - start + 1);
    let stream = tokio_util::io::ReaderStream::new(take);
    let body = Body::from_stream(stream);

    // Inline-display images so the dashboard / browser shows thumbnails.
    let lower = filename.to_lowercase();
    let (disp, ctype) = if lower.ends_with(".png") {
        ("inline", "image/png")
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        ("inline", "image/jpeg")
    } else if lower.ends_with(".webp") {
        ("inline", "image/webp")
    } else if lower.ends_with(".gif") {
        ("inline", "image/gif")
    } else if lower.ends_with(".svg") {
        ("inline", "image/svg+xml")
    } else if lower.ends_with(".txt") || lower.ends_with(".log") || lower.ends_with(".md") {
        ("inline", "text/plain; charset=utf-8")
    } else {
        ("attachment", "application/octet-stream")
    };

    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("{disp}; filename=\"{filename}\""))
            .unwrap_or(HeaderValue::from_static("attachment")),
    );
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(ctype));
    headers.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    headers.insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&format!("{}", end - start + 1)).unwrap(),
    );
    if range.is_some() {
        headers.insert(
            header::CONTENT_RANGE,
            HeaderValue::from_str(&format!("bytes {start}-{end}/{total}")).unwrap(),
        );
    }
    let status = if range.is_some() {
        StatusCode::PARTIAL_CONTENT
    } else {
        StatusCode::OK
    };
    Ok((status, headers, body).into_response())
}

fn parse_range(h: &str, total: u64) -> Option<(u64, u64)> {
    let s = h.strip_prefix("bytes=")?;
    let mut parts = s.split('-');
    let start: u64 = parts.next()?.parse().ok()?;
    let end_str = parts.next()?;
    let end: u64 = if end_str.is_empty() {
        total - 1
    } else {
        end_str.parse().ok()?
    };
    Some((start, end))
}

async fn upload_file(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<PathQuery>,
    mut multi: Multipart,
) -> Result<Json<serde_json::Value>, StatusCode> {
    check_pin(&headers, &state)?;
    let dir = safe(&q.path)?;
    std::fs::create_dir_all(&dir).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut written = Vec::new();
    while let Some(field) = multi
        .next_field()
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?
    {
        let name = field
            .file_name()
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("upload_{}", chrono::Utc::now().timestamp()));
        let target = dir.join(&name);
        let bytes = field
            .bytes()
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let total = bytes.len();
        std::fs::write(&target, &bytes).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let mut entry = json!({
            "name": name,
            "path": target.display().to_string(),
            "size": total,
            "extracted": false,
        });

        // Auto-extract .zip uploads + delete the source.
        let lname = name.to_lowercase();
        if lname.ends_with(".zip") {
            let st = std::process::Command::new("unzip")
                .arg("-o")
                .arg(&target)
                .arg("-d")
                .arg(&dir)
                .status();
            if matches!(st, Ok(s) if s.success()) {
                std::fs::remove_file(&target).ok();
                entry["extracted"] = json!(true);
            }
        }
        written.push(entry);
    }
    Ok(Json(json!({"uploaded": written})))
}

async fn mkdir(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<PathQuery>,
) -> Result<StatusCode, StatusCode> {
    check_pin(&headers, &state)?;
    let p = safe(&q.path)?;
    std::fs::create_dir_all(&p).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::CREATED)
}

async fn delete_path(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<PathQuery>,
) -> Result<StatusCode, StatusCode> {
    check_pin(&headers, &state)?;
    let p = safe(&q.path)?;
    if !p.exists() {
        return Ok(StatusCode::NO_CONTENT);
    }
    if p.is_dir() {
        std::fs::remove_dir_all(&p).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    } else {
        std::fs::remove_file(&p).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    Ok(StatusCode::OK)
}

async fn index_page(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if check_pin(&headers, &state).is_err() {
        let mut h = HeaderMap::new();
        h.insert(header::LOCATION, HeaderValue::from_static("/login"));
        return (StatusCode::SEE_OTHER, h, "").into_response();
    }
    Html(INDEX_HTML).into_response()
}

const LOGIN_HTML: &str = r##"<!doctype html>
<html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Playora · Unlock</title>
<style>
body{font-family:-apple-system,BlinkMacSystemFont,'Inter','Segoe UI',sans-serif;background:#0a0a0d;color:#e6e6ea;margin:0;height:100vh;display:flex;align-items:center;justify-content:center}
form{background:#101015;border:1px solid #1f1f26;border-radius:10px;padding:32px;text-align:center;min-width:300px}
h1{font-size:18px;margin:0 0 6px 0}
p{color:#777;font-size:13px;margin:0 0 18px 0}
input{width:100%;background:#0a0a0a;color:#fff;border:1px solid #2a2a35;border-radius:8px;padding:14px;font-size:20px;text-align:center;letter-spacing:6px;font-family:'JetBrains Mono',monospace}
button{margin-top:14px;width:100%;background:#1a3d5c;color:#7c9eff;border:1px solid #2a5078;border-radius:8px;padding:12px;font-size:14px;cursor:pointer}
button:hover{background:#234e75}
.hint{color:#444;font-size:11px;margin-top:14px}
</style></head>
<body>
<form method="post" action="/login">
  <h1>Playora File Browser</h1>
  <p>Enter the 6-digit PIN shown on the R36S screen.</p>
  <input name="pin" autofocus inputmode="numeric" pattern="[0-9]*" maxlength="6" placeholder="123456">
  <button type="submit">Unlock</button>
  <div class="hint">PIN refreshes every time you re-run Playora File Browser.</div>
</form>
</body></html>
"##;

const INDEX_HTML: &str = r##"<!doctype html>
<html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Playora · File Browser</title>
<style>
*{box-sizing:border-box}
body{font-family:-apple-system,BlinkMacSystemFont,'Inter','Segoe UI',Roboto,sans-serif;background:#0a0a0d;color:#e6e6ea;margin:0;padding:24px}
h1{font-size:20px;margin:0 0 4px 0}
.sub{color:#777;font-size:12px;margin-bottom:18px}
.bar{display:flex;gap:8px;margin-bottom:14px;flex-wrap:wrap}
.bar button,.bar a{background:#1a1a22;color:#9ad;border:1px solid #2a2a35;border-radius:6px;padding:6px 12px;cursor:pointer;font-size:12px;text-decoration:none}
.bar button:hover{background:#23232f}
.bar input{background:#0a0a0a;color:#cfcfcf;border:1px solid #2a2a35;border-radius:6px;padding:6px 10px;font-family:monospace;font-size:12px;flex:1;min-width:260px}
table{width:100%;border-collapse:separate;border-spacing:0;background:#101015;border:1px solid #1f1f26;border-radius:8px;overflow:hidden;font-size:13px}
th,td{padding:8px 12px;text-align:left;border-bottom:1px solid #1a1a1f}
tr:last-child td{border-bottom:none}
th{color:#777;font-weight:500;font-size:11px;text-transform:uppercase;background:#0d0d12}
tbody tr:hover td{background:#13131a}
.dir{color:#7c9eff}
.file{color:#cfcfcf}
.size,.mtime{color:#777;font-size:11px}
.act button{background:transparent;color:#9aa;border:1px solid #2a2a35;border-radius:4px;padding:3px 8px;cursor:pointer;font-size:11px;margin-left:4px}
.act button:hover{background:#1a1a22;color:#fff}
.act button.del{color:#d65656;border-color:#4a1414}
.drop{margin-top:14px;border:2px dashed #2a2a35;border-radius:10px;padding:30px;text-align:center;color:#666;font-size:13px;transition:border-color .15s}
.drop.over{border-color:#7c9eff;color:#fff}
.progress{margin-top:10px;font-size:11px;color:#9ad}
.crumbs{font-family:monospace;font-size:12px;color:#7c9eff;margin-bottom:10px}
.crumbs a{color:#7c9eff;text-decoration:none}
.crumbs a:hover{text-decoration:underline}
</style></head>
<body>
<h1>Playora · File Browser</h1>
<p class="sub">Browse, upload, download anywhere under /roms, /boot, /userdata, /home, /tmp. ZIP uploads auto-extract.</p>
<div class="bar">
  <input id="path" value="/roms" />
  <button onclick="load(document.getElementById('path').value)">Go</button>
  <button onclick="load('/roms')">/roms</button>
  <button onclick="load('/boot')">/boot</button>
  <button onclick="load('/userdata')">/userdata</button>
  <button onclick="up()">Up</button>
  <button onclick="newdir()">+ Folder</button>
  <button onclick="restartES()" style="background:#3a0a0a;color:#ff7676;border-color:#4a1414">Restart EmulationStation</button>
</div>
<div class="crumbs" id="crumbs"></div>
<table>
<thead><tr><th>Name</th><th>Size</th><th>Modified</th><th></th></tr></thead>
<tbody id="rows"></tbody>
</table>
<div class="drop" id="drop">Drop files here to upload — ZIPs auto-extract.</div>
<div class="progress" id="prog"></div>

<script>
function fmtSize(b){if(!b)return '';if(b<1024)return b+' B';if(b<1048576)return(b/1024).toFixed(1)+' KB';if(b<1073741824)return(b/1048576).toFixed(1)+' MB';return(b/1073741824).toFixed(2)+' GB';}
function fmtTime(t){if(!t)return '';return new Date(t*1000).toLocaleString();}
function up(){const p=document.getElementById('path').value;const parts=p.split('/').filter(Boolean);if(parts.length<=1){load('/');return;}parts.pop();load('/'+parts.join('/'));}
function newdir(){const cur=document.getElementById('path').value;const n=prompt('Folder name?');if(!n)return;fetch('/api/mkdir?path='+encodeURIComponent(cur+'/'+n),{method:'POST'}).then(()=>load(cur));}
function delPath(p){if(!confirm('Delete '+p+'?'))return;fetch('/api/delete?path='+encodeURIComponent(p),{method:'POST'}).then(()=>load(document.getElementById('path').value));}
function renamePath(p){const n=prompt('Rename to (full path)?',p);if(!n||n===p)return;fetch('/api/move',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({from:p,to:n})}).then(()=>load(document.getElementById('path').value));}
function restartES(){if(!confirm('Restart EmulationStation?'))return;fetch('/api/restart-es',{method:'POST'}).then(r=>{if(r.ok)alert('ES restart requested');else alert('failed — must run sudo from device')});}
function load(p){
  document.getElementById('path').value=p;
  document.getElementById('crumbs').innerHTML=p.split('/').filter(Boolean).reduce((acc,part,i,a)=>{const sub='/'+a.slice(0,i+1).join('/');return acc+' / <a href="#" onclick="load(\''+sub+'\');return false">'+part+'</a>';},'/');
  fetch('/api/list?path='+encodeURIComponent(p)).then(r=>r.json()).then(d=>{
    const tb=document.getElementById('rows');tb.innerHTML='';
    (d.entries||[]).forEach(e=>{
      const tr=document.createElement('tr');
      const name=e.is_dir?'<a class="dir" href="#" onclick="load(\''+e.path+'\');return false">📁 '+e.name+'</a>':'<span class="file">📄 '+e.name+'</span>';
      const rn='<button onclick="renamePath(\''+e.path+'\')">Rename</button>';
      const act=e.is_dir?rn+' <button class="del" onclick="delPath(\''+e.path+'\')">Delete</button>':'<a href="/api/download?path='+encodeURIComponent(e.path)+'"><button>Download</button></a> '+rn+' <button class="del" onclick="delPath(\''+e.path+'\')">Delete</button>';
      tr.innerHTML='<td>'+name+'</td><td class="size">'+fmtSize(e.size)+'</td><td class="mtime">'+fmtTime(e.modified)+'</td><td class="act">'+act+'</td>';
      tb.appendChild(tr);
    });
  });
}
const drop=document.getElementById('drop');
drop.addEventListener('dragover',e=>{e.preventDefault();drop.classList.add('over')});
drop.addEventListener('dragleave',()=>drop.classList.remove('over'));
drop.addEventListener('drop',e=>{
  e.preventDefault();drop.classList.remove('over');
  const cur=document.getElementById('path').value;
  for(const f of e.dataTransfer.files){
    const fd=new FormData();fd.append('file',f);
    document.getElementById('prog').innerText='uploading '+f.name+' ('+fmtSize(f.size)+')...';
    const xhr=new XMLHttpRequest();
    xhr.open('POST','/api/upload?path='+encodeURIComponent(cur));
    xhr.upload.onprogress=evt=>{if(evt.lengthComputable){const pct=Math.round(evt.loaded*100/evt.total);document.getElementById('prog').innerText=f.name+': '+pct+'% ('+fmtSize(evt.loaded)+'/'+fmtSize(evt.total)+')';}};
    xhr.onload=()=>{document.getElementById('prog').innerText=f.name+': done';load(cur);};
    xhr.send(fd);
  }
});
load('/roms');
</script>
</body></html>
"##;

use tokio::io::AsyncReadExt as _;
