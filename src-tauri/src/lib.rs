mod harness;

use tauri_plugin_dialog::DialogExt;

#[tauri::command]
async fn save_glb(
    app: tauri::AppHandle,
    bytes: Vec<u8>,
    default_name: String,
) -> Result<Option<String>, String> {
    if bytes.len() < 12 || &bytes[..4] != b"glTF" {
        return Err("Export is not a valid GLB file".into());
    }
    if bytes.len() > 128 * 1024 * 1024 {
        return Err("Prototype exports are limited to 128 MB".into());
    }
    let version = u32::from_le_bytes(bytes[4..8].try_into().map_err(|_| "Invalid GLB version")?);
    let length =
        u32::from_le_bytes(bytes[8..12].try_into().map_err(|_| "Invalid GLB length")?) as usize;
    if version != 2 || length != bytes.len() {
        return Err("GLB header is invalid".into());
    }
    let file_name = std::path::Path::new(&default_name)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("Sculpt-asset.glb")
        .to_owned();
    tauri::async_runtime::spawn_blocking(move || {
        let selected = app
            .dialog()
            .file()
            .set_title("Export 3D asset")
            .set_file_name(file_name)
            .add_filter("Binary glTF", &["glb"])
            .blocking_save_file();
        let Some(selected) = selected else {
            return Ok(None);
        };
        let mut path = selected.into_path().map_err(|error| error.to_string())?;
        if path.extension().is_none() {
            path.set_extension("glb");
        }
        std::fs::write(&path, bytes).map_err(|error| format!("Could not save asset: {error}"))?;
        Ok(Some(path.to_string_lossy().to_string()))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(harness::SculptInferenceHarness::default())
        .invoke_handler(tauri::generate_handler![
            harness::detect_hardware,
            harness::get_engines,
            harness::generate_asset,
            harness::cancel_generation,
            harness::assets::import_source,
            harness::assets::read_generated_asset,
            harness::python::backend_status,
            save_glb,
        ])
        .run(tauri::generate_context!())
        .expect("Sculpt could not start");
}
