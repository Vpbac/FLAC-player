//! HTTP Streaming Server for mobile devices (iPhone / iPad / Android)
//!
//! Exposes:
//! - GET /api/status -> Server status & version
//! - GET /api/tracks -> JSON list of tracks currently in playlist
//! - GET /api/stream/{index} -> Streams audio file with HTTP Range support (enables seeking on iOS)
//! - GET /api/cover/{index} -> Album cover art image

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::net::UdpSocket;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use tiny_http::{Header, Response, Server, StatusCode};

use crate::library::{self, TrackInfo};

const INDEX_HTML: &str = include_str!("web/index.html");

#[allow(dead_code)]
pub struct ServerHandle {
    pub running: Arc<AtomicBool>,
    pub port: u16,
    pub lan_ips: Vec<String>,
}

pub fn get_local_ips() -> Vec<String> {
    let mut ips = Vec::new();
    // Trick to get default outbound LAN IP without sending packets
    if let Ok(socket) = UdpSocket::bind("0.0.0.0:0") {
        if socket.connect("8.8.8.8:80").is_ok() {
            if let Ok(addr) = socket.local_addr() {
                let ip = addr.ip().to_string();
                if ip != "0.0.0.0" && !ip.starts_with("127.") {
                    ips.push(ip);
                }
            }
        }
    }
    if ips.is_empty() {
        ips.push("192.168.1.10".into());
    }
    ips
}

pub fn start_server(playlist: Arc<Mutex<Vec<TrackInfo>>>, port: u16) -> Result<ServerHandle, String> {
    let server = Server::http(format!("0.0.0.0:{port}")).map_err(|e| e.to_string())?;
    let running = Arc::new(AtomicBool::new(true));
    let is_running = running.clone();
    let lan_ips = get_local_ips();

    thread::Builder::new()
        .name("http-streamer".into())
        .spawn(move || {
            while is_running.load(Ordering::Relaxed) {
                let request = match server.recv() {
                    Ok(rq) => rq,
                    Err(_) => break,
                };

                let url = request.url().to_string();
                let method = request.method().as_str().to_uppercase();

                // CORS headers
                let cors = Header::from_bytes(&b"Access-Control-Allow-Origin"[..], &b"*"[..]).unwrap();
                let cors_methods = Header::from_bytes(&b"Access-Control-Allow-Methods"[..], &b"GET, OPTIONS"[..]).unwrap();
                let cors_headers = Header::from_bytes(&b"Access-Control-Allow-Headers"[..], &b"Range, Content-Type"[..]).unwrap();

                if method == "OPTIONS" {
                    let mut resp = Response::empty(StatusCode(200));
                    resp.add_header(cors);
                    resp.add_header(cors_methods);
                    resp.add_header(cors_headers);
                    let _ = request.respond(resp);
                    continue;
                }

                if url == "/" || url == "/index.html" || url.starts_with("/?") {
                    let mut resp = Response::from_string(INDEX_HTML);
                    resp.add_header(Header::from_bytes(&b"Content-Type"[..], &b"text/html; charset=utf-8"[..]).unwrap());
                    resp.add_header(Header::from_bytes(&b"Cache-Control"[..], &b"public, max-age=604800"[..]).unwrap());
                    resp.add_header(cors);
                    let _ = request.respond(resp);
                    continue;
                }

                if url == "/sw.js" {
                    let sw_code = r#"
const CACHE_NAME = 'flac-shell-v1';
self.addEventListener('install', e => {
  e.waitUntil(caches.open(CACHE_NAME).then(c => c.addAll(['/'])).then(() => self.skipWaiting()));
});
self.addEventListener('activate', e => {
  e.waitUntil(self.clients.claim());
});
self.addEventListener('fetch', e => {
  const url = new URL(e.request.url);
  if (url.pathname === '/' || url.pathname === '/index.html') {
    e.respondWith(fetch(e.request).then(r => {
      const clone = r.clone();
      caches.open(CACHE_NAME).then(c => c.put('/', clone));
      return r;
    }).catch(() => caches.match('/')));
  }
});
"#;
                    let mut resp = Response::from_string(sw_code);
                    resp.add_header(Header::from_bytes(&b"Content-Type"[..], &b"application/javascript; charset=utf-8"[..]).unwrap());
                    resp.add_header(cors);
                    let _ = request.respond(resp);
                    continue;
                }

                if url == "/api/status" || url.starts_with("/api/status?") {
                    let mut resp = Response::from_string("{\"status\":\"ok\",\"service\":\"FLAC Hi-Res Streamer\"}");
                    resp.add_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
                    resp.add_header(Header::from_bytes(&b"Cache-Control"[..], &b"no-store, no-cache, must-revalidate"[..]).unwrap());
                    resp.add_header(cors);
                    let _ = request.respond(resp);
                    continue;
                }

                if url == "/api/tracks" || url.starts_with("/api/tracks?") {
                    let tracks = playlist.lock().unwrap().clone();
                    let json = serde_json::to_string(&tracks).unwrap_or_else(|_| "[]".into());
                    let mut resp = Response::from_string(json);
                    resp.add_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
                    resp.add_header(Header::from_bytes(&b"Cache-Control"[..], &b"no-store, no-cache, must-revalidate"[..]).unwrap());
                    resp.add_header(cors);
                    let _ = request.respond(resp);
                    continue;
                }

                if url.starts_with("/api/cover/") {
                    let idx_str = url.trim_start_matches("/api/cover/");
                    if let Ok(idx) = idx_str.parse::<usize>() {
                        let path: Option<PathBuf> = playlist.lock().unwrap().get(idx).map(|t| t.path.clone());
                        if let Some(p) = path {
                            if let Some(bytes) = library::read_cover(&p) {
                                let mut resp = Response::from_data(bytes);
                                resp.add_header(Header::from_bytes(&b"Content-Type"[..], &b"image/jpeg"[..]).unwrap());
                                resp.add_header(cors);
                                let _ = request.respond(resp);
                                continue;
                            }
                        }
                    }
                    let mut resp = Response::empty(StatusCode(404));
                    resp.add_header(cors);
                    let _ = request.respond(resp);
                    continue;
                }

                if url.starts_with("/api/stream/") {
                    let idx_str = url.trim_start_matches("/api/stream/");
                    if let Ok(idx) = idx_str.parse::<usize>() {
                        let path: Option<PathBuf> = playlist.lock().unwrap().get(idx).map(|t| t.path.clone());
                        if let Some(p) = path {
                            if let Ok(mut file) = File::open(&p) {
                                if let Ok(metadata) = file.metadata() {
                                    let total_len = metadata.len();
                                    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("flac").to_lowercase();
                                    let mime = match ext.as_str() {
                                        "flac" => "audio/flac",
                                        "wav" => "audio/wav",
                                        "mp3" => "audio/mpeg",
                                        "m4a" | "alac" | "aac" => "audio/mp4",
                                        "ogg" | "oga" => "audio/ogg",
                                        _ => "application/octet-stream",
                                    };

                                    // Parse Range Header if present
                                    let range_header = request.headers().iter().find(|h| h.field.equiv("Range")).map(|h| h.value.as_str());

                                    let mut start = 0u64;
                                    let mut end = total_len - 1;
                                    let mut is_partial = false;

                                    if let Some(range_val) = range_header {
                                        if let Some(range_spec) = range_val.strip_prefix("bytes=") {
                                            let parts: Vec<&str> = range_spec.split('-').collect();
                                            if !parts.is_empty() {
                                                if let Ok(s) = parts[0].parse::<u64>() {
                                                    start = s;
                                                    is_partial = true;
                                                }
                                                if parts.len() > 1 && !parts[1].is_empty() {
                                                    if let Ok(e) = parts[1].parse::<u64>() {
                                                        end = e.min(total_len - 1);
                                                    }
                                                }
                                            }
                                        }
                                    }

                                    if start <= end && start < total_len {
                                        let content_len = end - start + 1;
                                        let _ = file.seek(SeekFrom::Start(start));
                                        let mut buffer = vec![0u8; content_len as usize];
                                        let _ = file.read_exact(&mut buffer);

                                        let status_code = if is_partial { StatusCode(206) } else { StatusCode(200) };
                                        let mut resp = Response::from_data(buffer).with_status_code(status_code);

                                        resp.add_header(Header::from_bytes(&b"Content-Type"[..], mime.as_bytes()).unwrap());
                                        resp.add_header(Header::from_bytes(&b"Accept-Ranges"[..], &b"bytes"[..]).unwrap());
                                        resp.add_header(cors);

                                        if is_partial {
                                            let range_header_val = format!("bytes {start}-{end}/{total_len}");
                                            resp.add_header(Header::from_bytes(&b"Content-Range"[..], range_header_val.as_bytes()).unwrap());
                                        }

                                        let _ = request.respond(resp);
                                        continue;
                                    }
                                }
                            }
                        }
                    }
                    let mut resp = Response::empty(StatusCode(404));
                    resp.add_header(cors);
                    let _ = request.respond(resp);
                    continue;
                }

                // Default 404
                let mut resp = Response::empty(StatusCode(404));
                resp.add_header(cors);
                let _ = request.respond(resp);
            }
        })
        .map_err(|e| e.to_string())?;

    Ok(ServerHandle { running, port, lan_ips })
}
