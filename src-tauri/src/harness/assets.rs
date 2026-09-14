use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{io::Cursor, path::{Path, PathBuf}};
use tauri::{Manager, State};

use super::SculptInferenceHarness;

pub const MAX_IMAGE_BYTES: usize = 30 * 1024 * 1024;
pub const MAX_GLB_BYTES: usize = 128 * 1024 * 1024;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceAsset {
    pub id: String,
    pub name: String,
    pub mime_type: String,
    pub width: u32,
    pub height: u32,
    pub sha256: String,
}

#[derive(Clone)]
pub struct SourceRecord {
    pub asset: SourceAsset,
    pub path: PathBuf,
}

pub fn decode_source(name: String, data_url: &str, directory: &Path) -> Result<SourceRecord, String> {
    if data_url.len() > MAX_IMAGE_BYTES * 4 / 3 + 128 {
        return Err("Source images must be smaller than 30 MB".into());
    }
    let (header, encoded) = data_url.split_once(',').ok_or("Invalid image data")?;
    let (mime, extension, expected) = match header {
        "data:image/png;base64" => ("image/png", "png", image::ImageFormat::Png),
        "data:image/jpeg;base64" => ("image/jpeg", "jpg", image::ImageFormat::Jpeg),
        "data:image/webp;base64" => ("image/webp", "webp", image::ImageFormat::WebP),
        _ => return Err("Choose a PNG, JPG, or WEBP image".into()),
    };
    let bytes = STANDARD.decode(encoded).map_err(|_| "Invalid image encoding")?;
    if bytes.is_empty() || bytes.len() > MAX_IMAGE_BYTES { return Err("Invalid image size".into()); }
    if image::guess_format(&bytes).map_err(|_| "Unrecognized image contents")? != expected {
        return Err("Image contents do not match its file type".into());
    }
    let reader = image::ImageReader::with_format(Cursor::new(&bytes), expected);
    let (width, height) = reader.into_dimensions().map_err(|_| "Could not read image dimensions")?;
    if width < 16 || height < 16 || u64::from(width) * u64::from(height) > 40_000_000 {
        return Err("Use an image between 16 × 16 and 40 megapixels".into());
    }
    // Decode with bounded allocations to reject corrupt files before admitting a job.
    let mut reader = image::ImageReader::with_format(Cursor::new(&bytes), expected);
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(192 * 1024 * 1024);
    reader.limits(limits);
    reader.decode().map_err(|_| "This image is corrupt or exceeds the decode limit")?;
    let id = uuid::Uuid::new_v4().to_string();
    std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    let path = directory.join(format!("{id}.{extension}"));
    std::fs::write(&path, &bytes).map_err(|e| format!("Could not store source image: {e}"))?;
    let safe_name = Path::new(&name).file_name().and_then(|name| name.to_str()).unwrap_or("image");
    let asset = SourceAsset { id, name: safe_name.chars().take(255).collect(), mime_type: mime.into(), width, height, sha256: format!("{:x}", Sha256::digest(&bytes)) };
    Ok(SourceRecord { asset, path })
}

#[tauri::command]
pub async fn import_source(app: tauri::AppHandle, harness: State<'_, SculptInferenceHarness>, name: String, data_url: String) -> Result<SourceAsset, String> {
    let directory = app.path().app_local_data_dir().map_err(|e| e.to_string())?.join("sources");
    let record = tauri::async_runtime::spawn_blocking(move || decode_source(name, &data_url, &directory)).await.map_err(|e| e.to_string())??;
    let asset = record.asset.clone();
    harness.sources.lock().map_err(|_| "Source registry unavailable")?.insert(asset.id.clone(), record);
    Ok(asset)
}

pub fn read_valid_glb(path: &Path) -> Result<Vec<u8>, String> {
    let length = std::fs::metadata(path).map_err(|_| "The generated asset is missing")?.len();
    if !(20..=MAX_GLB_BYTES as u64).contains(&length) { return Err("Invalid generated GLB size".into()); }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    validate_glb(&bytes)?;
    Ok(bytes)
}

pub fn validate_glb(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() < 20 || &bytes[0..4] != b"glTF" || u32::from_le_bytes(bytes[4..8].try_into().unwrap()) != 2 || u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize != bytes.len() { return Err("Invalid GLB header".into()); }
    let json_length = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    if json_length == 0 || json_length % 4 != 0 || &bytes[16..20] != b"JSON" || json_length > bytes.len() - 20 { return Err("Invalid GLB scene chunk".into()); }
    let scene: serde_json::Value = serde_json::from_slice(&bytes[20..20 + json_length]).map_err(|_| "Invalid GLB scene JSON")?;
    if !scene.get("meshes").and_then(|v| v.as_array()).is_some_and(|v| !v.is_empty()) { return Err("Generated GLB has no meshes".into()); }
    let mut offset = 20 + json_length;
    let mut has_binary = false;
    while offset < bytes.len() {
        if bytes.len() - offset < 8 { return Err("Truncated GLB buffer chunk".into()); }
        let chunk_length = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        if chunk_length % 4 != 0 || chunk_length > bytes.len() - offset - 8 { return Err("Invalid GLB buffer chunk".into()); }
        if &bytes[offset + 4..offset + 8] == b"BIN\0" { has_binary = true; }
        offset += 8 + chunk_length;
    }
    if !has_binary { return Err("Generated GLB has no binary buffer".into()); }
    // A generated asset must be self-contained; loading it must never fetch an
    // arbitrary external texture/buffer URL supplied by a worker.
    for category in ["buffers", "images"] {
        if let Some(entries) = scene.get(category).and_then(|v| v.as_array()) {
            if entries.iter().any(|v| v.get("uri").is_some()) { return Err("External asset references are not allowed in generated GLB files".into()); }
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn read_generated_asset(harness: State<'_, SculptInferenceHarness>, asset_id: String) -> Result<String, String> {
    let path = harness.outputs.lock().map_err(|_| "Asset registry unavailable")?.get(&asset_id).cloned().ok_or("Unknown generated asset")?;
    tauri::async_runtime::spawn_blocking(move || read_valid_glb(&path).map(|bytes| STANDARD.encode(bytes))).await.map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_sources_are_rejected_before_writing() {
        let dir = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        for data in ["data:text/plain;base64,YWJj", "data:image/png;base64,!!!!!", "data:image/jpeg;base64,aGVsbG8="] {
            assert!(decode_source("../../bad".into(), data, &dir).is_err());
        }
        assert!(!dir.exists());
    }

    #[test]
    fn rejects_empty_or_external_mesh_results() {
        assert!(validate_glb(&[]).is_err());
        let mut json = serde_json::to_vec(&serde_json::json!({"meshes":[{}], "buffers":[{"uri":"https://example.org/private"}]})).unwrap();
        while json.len() % 4 != 0 { json.push(b' '); }
        let mut bytes = b"glTF".to_vec();
        bytes.extend(2u32.to_le_bytes()); bytes.extend(((20 + json.len()) as u32).to_le_bytes());
        bytes.extend((json.len() as u32).to_le_bytes()); bytes.extend(b"JSON"); bytes.extend(json);
        bytes.extend(4u32.to_le_bytes()); bytes.extend(b"BIN\0"); bytes.extend([0u8; 4]);
        let total_length = bytes.len() as u32;
        bytes[8..12].copy_from_slice(&total_length.to_le_bytes());
        assert!(validate_glb(&bytes).unwrap_err().contains("External"));
    }
}
