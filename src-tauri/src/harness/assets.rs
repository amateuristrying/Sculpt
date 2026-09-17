use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::{Cursor, Read},
    path::{Path, PathBuf},
};
use tauri::State;

use super::SculptInferenceHarness;

pub const MAX_IMAGE_BYTES: usize = 30 * 1024 * 1024;
pub const MAX_GLB_BYTES: usize = 128 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
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

pub fn decode_source(
    name: String,
    data_url: &str,
    directory: &Path,
) -> Result<SourceRecord, String> {
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
    let bytes = STANDARD
        .decode(encoded)
        .map_err(|_| "Invalid image encoding")?;
    if bytes.is_empty() || bytes.len() > MAX_IMAGE_BYTES {
        return Err("Invalid image size".into());
    }
    if image::guess_format(&bytes).map_err(|_| "Unrecognized image contents")? != expected {
        return Err("Image contents do not match its file type".into());
    }
    let reader = image::ImageReader::with_format(Cursor::new(&bytes), expected);
    let (width, height) = reader
        .into_dimensions()
        .map_err(|_| "Could not read image dimensions")?;
    if width < 16 || height < 16 || u64::from(width) * u64::from(height) > 40_000_000 {
        return Err("Use an image between 16 × 16 and 40 megapixels".into());
    }
    // Decode with bounded allocations to reject corrupt files before admitting a job.
    let mut reader = image::ImageReader::with_format(Cursor::new(&bytes), expected);
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(192 * 1024 * 1024);
    reader.limits(limits);
    reader
        .decode()
        .map_err(|_| "This image is corrupt or exceeds the decode limit")?;
    let id = uuid::Uuid::new_v4().to_string();
    std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    let path = directory.join(format!("{id}.{extension}"));
    std::fs::write(&path, &bytes).map_err(|e| format!("Could not store source image: {e}"))?;
    let safe_name = Path::new(&name)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("image");
    let asset = SourceAsset {
        id,
        name: safe_name.chars().take(255).collect(),
        mime_type: mime.into(),
        width,
        height,
        sha256: format!("{:x}", Sha256::digest(&bytes)),
    };
    Ok(SourceRecord { asset, path })
}

#[tauri::command]
pub async fn import_source(
    harness: State<'_, SculptInferenceHarness>,
    name: String,
    data_url: String,
) -> Result<SourceAsset, String> {
    harness
        .library_work(move |library| {
            let directory = library.source_directory()?;
            let record = decode_source(name, &data_url, &directory)?;
            let asset = record.asset.clone();
            let path = record.path.clone();
            if let Err(error) = library.register_source(record) {
                let _ = std::fs::remove_file(path);
                return Err(error);
            }
            Ok(asset)
        })
        .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredSource {
    asset: SourceAsset,
    data_url: String,
    size: u64,
}

#[tauri::command]
pub async fn read_source(
    harness: State<'_, SculptInferenceHarness>,
    source_id: String,
) -> Result<StoredSource, String> {
    harness
        .library_work(move |library| {
            let (asset, bytes) = library.read_source(&source_id)?;
            Ok(StoredSource {
                data_url: format!(
                    "data:{};base64,{}",
                    asset.mime_type,
                    STANDARD.encode(&bytes)
                ),
                size: bytes.len() as u64,
                asset,
            })
        })
        .await
}

pub fn read_valid_glb(path: &Path) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(path).map_err(|_| "The generated asset is missing")?;
    let length = file.metadata().map_err(|e| e.to_string())?.len();
    if !(20..=MAX_GLB_BYTES as u64).contains(&length) {
        return Err("Invalid generated GLB size".into());
    }
    // Bound the read itself: the file can grow after its metadata was inspected.
    let mut bytes = Vec::with_capacity(length as usize);
    file.take(MAX_GLB_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    validate_glb(&bytes)?;
    Ok(bytes)
}

pub fn validate_glb(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() < 20
        || bytes.len() > MAX_GLB_BYTES
        || &bytes[0..4] != b"glTF"
        || u32::from_le_bytes(bytes[4..8].try_into().unwrap()) != 2
        || u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize != bytes.len()
    {
        return Err("Invalid GLB header".into());
    }
    let json_length = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    if json_length == 0
        || json_length % 4 != 0
        || &bytes[16..20] != b"JSON"
        || json_length > bytes.len() - 20
    {
        return Err("Invalid GLB scene chunk".into());
    }
    let scene: serde_json::Value = serde_json::from_slice(&bytes[20..20 + json_length])
        .map_err(|_| "Invalid GLB scene JSON")?;
    if scene["asset"]["version"] != "2.0" {
        return Err("Generated GLB must use glTF 2.0".into());
    }
    let mut offset = 20 + json_length;
    let mut binary = None;
    while offset < bytes.len() {
        if bytes.len() - offset < 8 {
            return Err("Truncated GLB buffer chunk".into());
        }
        let chunk_length =
            u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        if chunk_length % 4 != 0 || chunk_length > bytes.len() - offset - 8 {
            return Err("Invalid GLB buffer chunk".into());
        }
        if &bytes[offset + 4..offset + 8] == b"BIN\0" {
            if binary.is_some() {
                return Err("Generated GLB has multiple binary buffers".into());
            }
            binary = Some(&bytes[offset + 8..offset + 8 + chunk_length]);
        }
        offset += 8 + chunk_length;
    }
    let binary = binary.ok_or("Generated GLB has no binary buffer")?;
    // A generated asset must be self-contained; loading it must never fetch an
    // arbitrary external texture/buffer URL supplied by a worker.
    for category in ["buffers", "images"] {
        if let Some(entries) = scene.get(category).and_then(|v| v.as_array()) {
            if entries.iter().any(|v| v.get("uri").is_some()) {
                return Err(
                    "External asset references are not allowed in generated GLB files".into(),
                );
            }
        }
    }
    let buffers = scene["buffers"]
        .as_array()
        .ok_or("Generated GLB has no embedded buffer")?;
    if buffers.len() != 1 {
        return Err("Generated GLB must have one embedded buffer".into());
    }
    let declared_length = integer(&buffers[0]["byteLength"])?;
    if declared_length == 0 || declared_length > binary.len() || binary.len() - declared_length > 3
    {
        return Err("Generated GLB buffer length does not match its data".into());
    }
    let binary = &binary[..declared_length];
    let meshes = scene["meshes"]
        .as_array()
        .filter(|meshes| !meshes.is_empty())
        .ok_or("Generated GLB has no meshes")?;
    for mesh in meshes {
        let primitives = mesh["primitives"]
            .as_array()
            .filter(|items| !items.is_empty())
            .ok_or("Generated GLB mesh has no primitives")?;
        for primitive in primitives {
            if primitive.get("mode").is_some_and(|mode| mode != 4) {
                return Err("Generated GLB must contain triangle geometry".into());
            }
            let positions = accessor_data(
                &scene,
                &primitive["attributes"]["POSITION"],
                binary,
                "VEC3",
                3,
                &[5126],
            )?;
            if positions.count < 3 {
                return Err("Generated GLB has too few vertices".into());
            }
            for vertex in 0..positions.count {
                if !positions
                    .position(vertex)
                    .iter()
                    .all(|value| value.is_finite())
                {
                    return Err("Generated GLB has non-finite vertex positions".into());
                }
            }
            let indices = primitive
                .get("indices")
                .map(|index| accessor_data(&scene, index, binary, "SCALAR", 1, &[5121, 5123, 5125]))
                .transpose()?;
            let count = indices.as_ref().map_or(positions.count, |data| data.count);
            if count < 3 || count % 3 != 0 {
                return Err("Generated GLB has incomplete triangles".into());
            }
            let mut visible_triangle = false;
            for start in (0..count).step_by(3) {
                let mut triangle = [[0.0; 3]; 3];
                for (corner, point) in triangle.iter_mut().enumerate() {
                    let vertex = indices
                        .as_ref()
                        .map_or(start + corner, |data| data.index(start + corner));
                    if vertex >= positions.count {
                        return Err("Generated GLB triangle index is outside its vertices".into());
                    }
                    *point = positions.position(vertex);
                }
                let a: [f64; 3] = std::array::from_fn(|axis| triangle[1][axis] - triangle[0][axis]);
                let b: [f64; 3] = std::array::from_fn(|axis| triangle[2][axis] - triangle[0][axis]);
                visible_triangle |= (0..3).any(|axis| {
                    a[(axis + 1) % 3] * b[(axis + 2) % 3] - a[(axis + 2) % 3] * b[(axis + 1) % 3]
                        != 0.0
                });
            }
            if !visible_triangle {
                return Err("Generated GLB contains only degenerate triangles".into());
            }
        }
    }
    validate_scene_graph(&scene, meshes.len())?;
    Ok(())
}

fn integer(value: &serde_json::Value) -> Result<usize, String> {
    value
        .as_u64()
        .and_then(|number| usize::try_from(number).ok())
        .ok_or_else(|| "Generated GLB has an invalid buffer or accessor value".into())
}

struct AccessorData<'a> {
    bytes: &'a [u8],
    count: usize,
    stride: usize,
    component_bytes: usize,
}

impl AccessorData<'_> {
    fn position(&self, index: usize) -> [f64; 3] {
        std::array::from_fn(|axis| {
            let start = index * self.stride + axis * 4;
            f32::from_le_bytes(self.bytes[start..start + 4].try_into().unwrap()) as f64
        })
    }

    fn index(&self, index: usize) -> usize {
        let start = index * self.stride;
        match self.component_bytes {
            1 => self.bytes[start] as usize,
            2 => u16::from_le_bytes(self.bytes[start..start + 2].try_into().unwrap()) as usize,
            _ => u32::from_le_bytes(self.bytes[start..start + 4].try_into().unwrap()) as usize,
        }
    }
}

fn accessor_data<'a>(
    scene: &serde_json::Value,
    index: &serde_json::Value,
    binary: &'a [u8],
    kind: &str,
    components: usize,
    allowed_components: &[u64],
) -> Result<AccessorData<'a>, String> {
    let invalid = "Generated GLB has invalid or unsupported geometry accessors";
    let accessor = scene["accessors"]
        .as_array()
        .and_then(|items| items.get(integer(index).ok()?))
        .ok_or(invalid)?;
    let component_type = accessor["componentType"].as_u64().ok_or(invalid)?;
    if accessor["type"] != kind
        || !allowed_components.contains(&component_type)
        || accessor.get("sparse").is_some()
        || accessor.get("normalized").is_some_and(|v| v != false)
    {
        return Err(invalid.into());
    }
    let component_bytes = match component_type {
        5121 => 1,
        5123 => 2,
        _ => 4,
    };
    let element_bytes = component_bytes * components;
    let count = integer(&accessor["count"])?;
    let view = scene["bufferViews"]
        .as_array()
        .and_then(|items| items.get(integer(&accessor["bufferView"]).ok()?))
        .ok_or(invalid)?;
    if view["buffer"] != 0 || count == 0 {
        return Err(invalid.into());
    }
    let view_start = view
        .get("byteOffset")
        .map(integer)
        .transpose()?
        .unwrap_or(0);
    let view_length = integer(&view["byteLength"])?;
    let view_end = view_start
        .checked_add(view_length)
        .filter(|end| *end <= binary.len())
        .ok_or(invalid)?;
    let offset = accessor
        .get("byteOffset")
        .map(integer)
        .transpose()?
        .unwrap_or(0);
    let stride = view
        .get("byteStride")
        .map(integer)
        .transpose()?
        .unwrap_or(element_bytes);
    if stride < element_bytes
        || stride > 252
        || stride % component_bytes != 0
        || view_start % component_bytes != 0
        || offset % component_bytes != 0
    {
        return Err(invalid.into());
    }
    let size = (count - 1)
        .checked_mul(stride)
        .and_then(|v| v.checked_add(element_bytes))
        .ok_or(invalid)?;
    let end = offset
        .checked_add(size)
        .filter(|end| *end <= view_length)
        .ok_or(invalid)?;
    Ok(AccessorData {
        bytes: &binary[view_start..view_end][offset..end],
        count,
        stride,
        component_bytes,
    })
}

fn validate_scene_graph(scene: &serde_json::Value, mesh_count: usize) -> Result<(), String> {
    let invalid = "Generated GLB has no valid renderable scene";
    let nodes = scene["nodes"].as_array().ok_or(invalid)?;
    let scene_index = scene.get("scene").map(integer).transpose()?.unwrap_or(0);
    let selected = scene["scenes"]
        .as_array()
        .and_then(|items| items.get(scene_index))
        .ok_or(invalid)?;
    let roots = selected["nodes"].as_array().ok_or(invalid)?;
    let mut stack = Vec::new();
    for root in roots {
        stack.push((integer(root)?, false));
    }
    let mut state = vec![0; nodes.len()];
    let mut has_mesh = false;
    while let Some((index, exiting)) = stack.pop() {
        let node = nodes.get(index).ok_or(invalid)?;
        if exiting {
            state[index] = 2;
            continue;
        }
        if state[index] == 1 {
            return Err("Generated GLB scene graph contains a cycle".into());
        }
        if state[index] == 2 {
            continue;
        }
        state[index] = 1;
        stack.push((index, true));
        if let Some(mesh) = node.get("mesh") {
            if integer(mesh)? >= mesh_count {
                return Err(invalid.into());
            }
            has_mesh = true;
        }
        if let Some(children) = node.get("children") {
            for child in children.as_array().ok_or(invalid)? {
                stack.push((integer(child)?, false));
            }
        }
    }
    if !has_mesh {
        return Err(invalid.into());
    }
    Ok(())
}

#[tauri::command]
pub async fn read_generated_asset(
    harness: State<'_, SculptInferenceHarness>,
    asset_id: String,
) -> Result<tauri::ipc::Response, String> {
    harness
        .library_work(move |library| {
            let path = library.asset_path(&asset_id)?;
            read_valid_glb(&path).map(tauri::ipc::Response::new)
        })
        .await
}

/// Export the native, verified artifact directly. The webview only sends an ID.
#[tauri::command]
pub async fn save_generated_glb(
    app: tauri::AppHandle,
    harness: State<'_, SculptInferenceHarness>,
    asset_id: String,
    default_name: String,
) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let bytes = harness
        .library_work(move |library| read_valid_glb(&library.asset_path(&asset_id)?))
        .await?;
    let name = Path::new(&default_name)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("Sculpt.glb")
        .to_owned();
    tauri::async_runtime::spawn_blocking(move || {
        let Some(selected) = app
            .dialog()
            .file()
            .set_title("Export 3D asset")
            .set_file_name(name)
            .add_filter("Binary glTF", &["glb"])
            .blocking_save_file()
        else {
            return Ok(None);
        };
        let mut path = selected.into_path().map_err(|error| error.to_string())?;
        if path.extension().is_none() {
            path.set_extension("glb");
        }
        std::fs::write(&path, bytes).map_err(|error| format!("Could not save asset: {error}"))?;
        Ok(Some(path.to_string_lossy().into_owned()))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[cfg(test)]
pub(crate) fn test_triangle_glb() -> Vec<u8> {
    let document = serde_json::json!({
        "asset": {"version": "2.0"}, "scene": 0,
        "scenes": [{"nodes": [0]}], "nodes": [{"mesh": 0}],
        "meshes": [{"primitives": [{"attributes": {"POSITION": 0}, "indices": 1}]}],
        "buffers": [{"byteLength": 40}],
        "bufferViews": [{"buffer": 0, "byteLength": 36}, {"buffer": 0, "byteOffset": 36, "byteLength": 3}],
        "accessors": [
            {"bufferView": 0, "componentType": 5126, "type": "VEC3", "count": 3},
            {"bufferView": 1, "componentType": 5121, "type": "SCALAR", "count": 3}
        ]
    });
    let mut binary = Vec::new();
    for coordinate in [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
        binary.extend(coordinate.to_le_bytes());
    }
    binary.extend([0, 1, 2, 0]);
    test_encode_glb(&document, &binary)
}

#[cfg(test)]
fn test_encode_glb(document: &serde_json::Value, binary: &[u8]) -> Vec<u8> {
    let mut json = serde_json::to_vec(document).unwrap();
    while json.len() % 4 != 0 {
        json.push(b' ');
    }
    let mut bytes = b"glTF".to_vec();
    bytes.extend(2u32.to_le_bytes());
    bytes.extend(((28 + json.len() + binary.len()) as u32).to_le_bytes());
    bytes.extend((json.len() as u32).to_le_bytes());
    bytes.extend(b"JSON");
    bytes.extend(json);
    bytes.extend((binary.len() as u32).to_le_bytes());
    bytes.extend(b"BIN\0");
    bytes.extend(binary);
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_sources_are_rejected_before_writing() {
        let dir = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        for data in [
            "data:text/plain;base64,YWJj",
            "data:image/png;base64,!!!!!",
            "data:image/jpeg;base64,aGVsbG8=",
        ] {
            assert!(decode_source("../../bad".into(), data, &dir).is_err());
        }
        assert!(!dir.exists());
    }

    #[test]
    fn rejects_empty_or_external_mesh_results() {
        assert!(validate_glb(&[]).is_err());
        let (mut document, binary) = triangle_parts();
        document["buffers"][0]["uri"] = "https://example.org/private".into();
        let bytes = test_encode_glb(&document, &binary);
        assert!(validate_glb(&bytes).unwrap_err().contains("External"));
    }

    fn triangle_parts() -> (serde_json::Value, Vec<u8>) {
        let bytes = test_triangle_glb();
        let json_length = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        (
            serde_json::from_slice(&bytes[20..20 + json_length]).unwrap(),
            bytes[28 + json_length..].to_vec(),
        )
    }

    #[test]
    fn accepts_renderable_indexed_and_nonindexed_geometry() {
        assert!(validate_glb(&test_triangle_glb()).is_ok());
        let (mut document, binary) = triangle_parts();
        document["meshes"][0]["primitives"][0]
            .as_object_mut()
            .unwrap()
            .remove("indices");
        assert!(validate_glb(&test_encode_glb(&document, &binary)).is_ok());
    }

    #[test]
    fn rejects_mesh_shells_and_unrenderable_scenes() {
        let (document, binary) = triangle_parts();
        for category in ["primitives", "nodes", "cycle", "mode"] {
            let mut changed = document.clone();
            match category {
                "primitives" => changed["meshes"][0]["primitives"] = serde_json::json!([]),
                "nodes" => changed["scenes"][0]["nodes"] = serde_json::json!([]),
                "cycle" => changed["nodes"][0]["children"] = serde_json::json!([0]),
                _ => changed["meshes"][0]["primitives"][0]["mode"] = 0.into(),
            }
            assert!(
                validate_glb(&test_encode_glb(&changed, &binary)).is_err(),
                "{category}"
            );
        }
    }

    #[test]
    fn rejects_accessor_overflows_and_invalid_triangle_indices() {
        let (document, binary) = triangle_parts();
        for field in ["byteOffset", "count", "bufferView"] {
            let mut changed = document.clone();
            changed["accessors"][0][field] = u64::MAX.into();
            assert!(
                validate_glb(&test_encode_glb(&changed, &binary)).is_err(),
                "{field}"
            );
        }
        let mut invalid_indices = binary;
        invalid_indices[38] = 3;
        assert!(validate_glb(&test_encode_glb(&document, &invalid_indices))
            .unwrap_err()
            .contains("index"));
    }

    #[test]
    fn rejects_nonfinite_positions_and_all_degenerate_triangles() {
        let (document, mut binary) = triangle_parts();
        binary[..4].copy_from_slice(&f32::NAN.to_le_bytes());
        assert!(validate_glb(&test_encode_glb(&document, &binary))
            .unwrap_err()
            .contains("non-finite"));
        binary[..36].fill(0);
        assert!(validate_glb(&test_encode_glb(&document, &binary))
            .unwrap_err()
            .contains("degenerate"));
    }
}
