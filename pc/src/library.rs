//! Music library: scanning folders, reading tags and cover art.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crossbeam_channel::Sender;
use lofty::prelude::*;
use serde::{Deserialize, Serialize};

pub const AUDIO_EXTS: &[&str] = &["flac", "wav", "mp3", "m4a", "alac", "ogg", "oga", "aac", "mp4"];

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrackInfo {
    pub path: PathBuf,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub track_no: u32,
    pub duration: f64,
    pub sample_rate: u32,
    pub bits: u8,
    pub channels: u8,
    pub bitrate: u32,
}

impl TrackInfo {
    pub fn format_label(&self) -> String {
        let ext = self
            .path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_uppercase();
        let rate = if self.sample_rate % 1000 == 0 {
            format!("{}", self.sample_rate / 1000)
        } else {
            format!("{:.1}", self.sample_rate as f64 / 1000.0)
        };
        if self.bits > 0 {
            format!("{ext} {}/{rate}", self.bits)
        } else {
            format!("{ext} {rate}k {}kbps", self.bitrate)
        }
    }

    pub fn is_hires(&self) -> bool {
        self.bits >= 24 || self.sample_rate > 48_000
    }

    pub fn matches(&self, q: &str) -> bool {
        if q.is_empty() {
            return true;
        }
        let q = q.to_lowercase();
        self.title.to_lowercase().contains(&q)
            || self.artist.to_lowercase().contains(&q)
            || self.album.to_lowercase().contains(&q)
    }
}

pub fn is_audio(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| AUDIO_EXTS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

pub fn read_track(path: &Path) -> TrackInfo {
    let fallback_title = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let mut t = TrackInfo {
        path: path.to_path_buf(),
        title: fallback_title,
        artist: String::new(),
        album: String::new(),
        track_no: 0,
        duration: 0.0,
        sample_rate: 0,
        bits: 0,
        channels: 0,
        bitrate: 0,
    };
    if let Ok(file) = lofty::read_from_path(path) {
        let p = file.properties();
        t.duration = p.duration().as_secs_f64();
        t.sample_rate = p.sample_rate().unwrap_or(0);
        t.bits = p.bit_depth().unwrap_or(0);
        t.channels = p.channels().unwrap_or(0);
        t.bitrate = p.audio_bitrate().unwrap_or(0);
        if let Some(tag) = file.primary_tag().or_else(|| file.first_tag()) {
            if let Some(v) = tag.title() {
                if !v.trim().is_empty() {
                    t.title = v.to_string();
                }
            }
            if let Some(v) = tag.artist() {
                t.artist = v.to_string();
            }
            if let Some(v) = tag.album() {
                t.album = v.to_string();
            }
            t.track_no = tag.track().unwrap_or(0);
        }
    }
    t
}

/// Embedded front cover, or cover.jpg / folder.jpg next to the file.
pub fn read_cover(path: &Path) -> Option<Vec<u8>> {
    if let Ok(file) = lofty::read_from_path(path) {
        for tag in file.tags() {
            let pics = tag.pictures();
            let front = pics
                .iter()
                .find(|p| p.pic_type() == lofty::picture::PictureType::CoverFront)
                .or_else(|| pics.first());
            if let Some(p) = front {
                return Some(p.data().to_vec());
            }
        }
    }
    let dir = path.parent()?;
    for name in ["cover.jpg", "cover.png", "folder.jpg", "folder.png", "front.jpg", "Cover.jpg", "Folder.jpg"] {
        let p = dir.join(name);
        if p.exists() {
            if let Ok(b) = std::fs::read(&p) {
                return Some(b);
            }
        }
    }
    None
}

pub enum ScanMsg {
    Tracks(Vec<TrackInfo>),
    Removed(Vec<PathBuf>),
    Done,
}

#[cfg(windows)]
fn is_file_ready(path: &Path) -> bool {
    use std::os::windows::fs::OpenOptionsExt;
    // FILE_SHARE_READ (1) without FILE_SHARE_WRITE ensures no other process is still copying/writing the file
    std::fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(path)
        .and_then(|f| f.metadata())
        .map(|m| m.len() > 0)
        .unwrap_or(false)
}

#[cfg(not(windows))]
fn is_file_ready(path: &Path) -> bool {
    std::fs::metadata(path).map(|m| m.len() > 0).unwrap_or(false)
}

/// Scan files/folders in a background thread, sending tracks in batches.
pub fn scan(paths: Vec<PathBuf>, tx: Sender<ScanMsg>, repaint: impl Fn() + Send + 'static) {
    std::thread::spawn(move || {
        let mut files: Vec<PathBuf> = Vec::new();
        for p in paths {
            if p.is_dir() {
                for e in walkdir::WalkDir::new(&p).follow_links(true).into_iter().flatten() {
                    if e.file_type().is_file() && is_audio(e.path()) {
                        files.push(e.into_path());
                    }
                }
            } else if is_audio(&p) {
                files.push(p);
            }
        }
        // Folder order, then natural file order.
        files.sort();
        let mut batch = Vec::new();
        let mut dir: Option<PathBuf> = None;
        let flush = |batch: &mut Vec<TrackInfo>| {
            if batch.is_empty() {
                return;
            }
            // Within an album folder, sort by track number when available.
            batch.sort_by(|a: &TrackInfo, b: &TrackInfo| {
                (a.track_no == 0, a.track_no, &a.path).cmp(&(b.track_no == 0, b.track_no, &b.path))
            });
            let _ = tx.send(ScanMsg::Tracks(std::mem::take(batch)));
            repaint();
        };
        for f in files {
            let parent = f.parent().map(|p| p.to_path_buf());
            if parent != dir {
                flush(&mut batch);
                dir = parent;
            }
            batch.push(read_track(&f));
        }
        flush(&mut batch);
        let _ = tx.send(ScanMsg::Done);
    });
}

/// Background folder watcher that automatically detects newly added or deleted audio files
/// inside watched folders without requiring manual user action.
pub fn spawn_watcher(
    watched_folders: Arc<Mutex<Vec<PathBuf>>>,
    known_paths: Arc<Mutex<HashSet<PathBuf>>>,
    server_playlist: Arc<Mutex<Vec<TrackInfo>>>,
    tx: Sender<ScanMsg>,
    repaint: impl Fn() + Send + 'static,
) {
    std::thread::Builder::new()
        .name("folder-watcher".into())
        .spawn(move || loop {
            std::thread::sleep(Duration::from_secs(2));

            let folders: Vec<PathBuf> = match watched_folders.lock() {
                Ok(g) => g.clone(),
                Err(_) => continue,
            };
            if folders.is_empty() {
                continue;
            }

            // 1. Check for new audio files in watched folders
            let mut candidates: Vec<PathBuf> = Vec::new();
            for dir in &folders {
                if !dir.is_dir() {
                    continue;
                }
                for e in walkdir::WalkDir::new(dir).follow_links(true).into_iter().flatten() {
                    if e.file_type().is_file() && is_audio(e.path()) {
                        candidates.push(e.into_path());
                    }
                }
            }

            let new_files: Vec<PathBuf> = {
                let known = match known_paths.lock() {
                    Ok(g) => g,
                    Err(_) => continue,
                };
                candidates
                    .into_iter()
                    .filter(|p| !known.contains(p) && is_file_ready(p))
                    .collect()
            };

            if !new_files.is_empty() {
                let mut sorted_files = new_files;
                sorted_files.sort();
                let mut new_tracks: Vec<TrackInfo> = sorted_files.iter().map(|f| read_track(f)).collect();
                new_tracks.sort_by(|a, b| {
                    (a.track_no == 0, a.track_no, &a.path).cmp(&(b.track_no == 0, b.track_no, &b.path))
                });

                if let Ok(mut known) = known_paths.lock() {
                    for f in &sorted_files {
                        known.insert(f.clone());
                    }
                }

                // Update HTTP streaming server playlist immediately (works even when PC UI is minimized)
                if let Ok(mut srv) = server_playlist.lock() {
                    for t in &new_tracks {
                        if !srv.iter().any(|existing| existing.path == t.path) {
                            srv.push(t.clone());
                        }
                    }
                }

                let _ = tx.send(ScanMsg::Tracks(new_tracks));
                repaint();
            }

            // 2. Check for files that were deleted/renamed on disk (when parent directory still exists)
            let removed_files: Vec<PathBuf> = {
                let srv = match server_playlist.lock() {
                    Ok(g) => g,
                    Err(_) => continue,
                };
                srv.iter()
                    .filter(|t| {
                        let parent_exists = t.path.parent().map(|p| p.is_dir()).unwrap_or(false);
                        parent_exists && !t.path.exists()
                    })
                    .map(|t| t.path.clone())
                    .collect()
            };

            if !removed_files.is_empty() {
                if let Ok(mut known) = known_paths.lock() {
                    for p in &removed_files {
                        known.remove(p);
                    }
                }
                if let Ok(mut srv) = server_playlist.lock() {
                    srv.retain(|t| !removed_files.contains(&t.path));
                }
                let _ = tx.send(ScanMsg::Removed(removed_files));
                repaint();
            }
        })
        .ok();
}
