//! Media domain (AGENT 2 OWNS THIS FILE): inline images, uploads, downloads.

use super::{Event, Session};
use std::path::PathBuf;
use std::sync::mpsc::Sender;

/// `Command::FetchImage` — fetch + decode one image for inline display.
///
/// Core API:
/// `ost::api::fetch_media_data(&client, url) -> Result<MediaBytes>`
/// `MediaBytes { data: Vec<u8>, content_type: Option<String> }`
/// (handles skypetoken auth for Microsoft hosts itself; 15 MB cap).
///
/// Decode with the `image` crate (jpeg/png/webp/gif already enabled) to
/// RGBA8. Downscale huge images in the worker (cap ~1600 px on the long
/// edge, simple sampling is fine) to keep textures light. Then emit
/// `Event::ImageReady { url, rgba, size: [w, h] }`, or
/// `Event::ImageFailed(url)` on any failure (never panic, never block the
/// runtime: decode on `tokio::task::spawn_blocking` if needed).
pub async fn fetch_image(ses: &Session, tx: &Sender<Event>, url: &str) {
    let _ = (ses, tx, url);
}

/// `Command::UploadFile` — send a local file into a chat.
///
/// Core API:
/// `ost::api::upload_file_data(&client, chat_id, local_path) -> Result<SharedFile>`
/// (or `upload_file_data_with_progress` with a progress callback for
/// `Event::UploadProgress` updates). After success emit `Event::UploadDone`
/// AND trigger a history refetch like `conv::send` does (read_messages_page
/// newest page, emit `Event::Messages` with empty members/resolved_name).
pub async fn upload(ses: &Session, tx: &Sender<Event>, chat_id: &str, path: &PathBuf) {
    let _ = (ses, tx, chat_id, path);
}

/// `Command::DownloadFile` — fetch a shared file to disk and open it.
///
/// Core API:
/// `ost::api::fetch_media_data(&client, url) -> Result<MediaBytes>` works for
/// file objects too (auth per host is handled). Write to
/// `~/Downloads/<name>` (create a numbered `name (1).ext` variant if taken),
/// emit `Event::DownloadDone { name, path }`, then open with
/// `open::that(path)` (the `open` crate is already a dependency).
pub async fn download(ses: &Session, tx: &Sender<Event>, url: &str, name: &str) {
    let _ = (ses, tx, url, name);
}
