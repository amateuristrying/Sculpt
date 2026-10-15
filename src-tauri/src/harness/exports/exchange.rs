//! Lossless container conversion and Wavefront export of saved static assets.
use super::{assets, determinant, multiply, numbers, transform, SculptInferenceHarness, IDENTITY};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Deserialize;
use serde_json::Value;
use std::{
    fmt::Write as _,
    io::{Cursor, Write as _},
    path::Path,
};
use tauri::State;

const MAX_EXPORT_BYTES: usize = 256 * 1024 * 1024;

fn unpack(glb: &[u8]) -> Result<(Value, &[u8]), String> {
    assets::validate_glb(glb)?;
    let length = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    let scene: Value = serde_json::from_slice(&glb[20..20 + length]).map_err(|e| e.to_string())?;
    let mut offset = 20 + length;
    while offset < glb.len() {
        let size = u32::from_le_bytes(glb[offset..offset + 4].try_into().unwrap()) as usize;
        if &glb[offset + 4..offset + 8] == b"BIN\0" {
            let declared = assets::integer(&scene["buffers"][0]["byteLength"])?;
            return Ok((scene, &glb[offset + 8..offset + 8 + declared]));
        }
        offset += size + 8;
    }
    Err("No GLB binary buffer".into())
}

pub(super) fn embedded_gltf(glb: &[u8]) -> Result<Vec<u8>, String> {
    let (mut scene, binary) = unpack(glb)?;
    scene["buffers"][0]["uri"] = format!(
        "data:application/octet-stream;base64,{}",
        STANDARD.encode(binary)
    )
    .into();
    let bytes = serde_json::to_vec(&scene).map_err(|e| e.to_string())?;
    if bytes.len() > MAX_EXPORT_BYTES {
        return Err("glTF exceeds the 256 MB export limit".into());
    }
    Ok(bytes)
}

fn srgb(value: f64) -> f64 {
    let value = value.clamp(0., 1.);
    if value <= 0.0031308 {
        12.92 * value
    } else {
        1.055 * value.powf(1. / 2.4) - 0.055
    }
}

fn finite<const N: usize>(values: [f64; N]) -> Result<[f64; N], String> {
    if values.iter().all(|v| v.is_finite()) {
        Ok(values)
    } else {
        Err("Non-finite OBJ attribute".into())
    }
}

/// Fixed archive entry names keep paths portable and avoid name/path injection.
/// Coordinates stay in the saved asset's Y-up space; STL is the printing export.
pub(super) fn obj_archive(glb: &[u8]) -> Result<Vec<u8>, String> {
    let (scene, binary) = unpack(glb)?;
    if scene["extensionsRequired"]
        .as_array()
        .is_some_and(|v| !v.is_empty())
    {
        return Err("OBJ export cannot preserve required glTF extensions; use GLB".into());
    }
    let mut obj = String::from("# Sculpt | Y up | original asset coordinates\nmtllib model.mtl\n");
    let mut mtl = String::from("# Sculpt Wavefront materials; use GLB for full PBR semantics\n");
    let mut images: Vec<(String, &[u8])> = Vec::new();
    let nodes = scene["nodes"].as_array().unwrap();
    let selected = scene
        .get("scene")
        .map(assets::integer)
        .transpose()?
        .unwrap_or(0);
    let mut stack = scene["scenes"][selected]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .map(|id| Ok((assets::integer(id)?, IDENTITY)))
        .collect::<Result<Vec<_>, String>>()?;
    let mut visited = vec![false; nodes.len()];
    let (mut vertex_offset, mut uv_offset, mut normal_offset, mut part) = (1, 1, 1, 0);
    while let Some((index, parent)) = stack.pop() {
        if visited[index] {
            return Err("OBJ requires a scene tree without shared nodes".into());
        }
        visited[index] = true;
        let node = &nodes[index];
        if node.get("skin").is_some() {
            return Err("Skinned meshes cannot be exported as static OBJ".into());
        }
        let world = multiply(parent, transform(node)?);
        let det = determinant(world);
        if !det.is_finite() || det == 0. {
            return Err("Invalid or collapsed node transform".into());
        }
        if let Some(children) = node["children"].as_array() {
            for child in children.iter().rev() {
                stack.push((assets::integer(child)?, world));
            }
        }
        let Some(mesh_id) = node.get("mesh") else {
            continue;
        };
        for primitive in scene["meshes"][assets::integer(mesh_id)?]["primitives"]
            .as_array()
            .unwrap()
        {
            if primitive.get("targets").is_some() {
                return Err("Morph targets are unsupported for OBJ export".into());
            }
            let attributes = &primitive["attributes"];
            let positions =
                assets::accessor_data(&scene, &attributes["POSITION"], binary, "VEC3", 3, &[5126])?;
            let normals = attributes
                .get("NORMAL")
                .map(|id| assets::accessor_data(&scene, id, binary, "VEC3", 3, &[5126]))
                .transpose()?;
            let uvs = attributes
                .get("TEXCOORD_0")
                .map(|id| assets::accessor_data(&scene, id, binary, "VEC2", 2, &[5126]))
                .transpose()?;
            let colors = attributes
                .get("COLOR_0")
                .map(|id| assets::color_data(&scene, id, binary))
                .transpose()?;
            for data in [&normals, &uvs, &colors].into_iter().flatten() {
                if data.count != positions.count {
                    return Err("OBJ attributes do not match vertex count".into());
                }
            }
            let material = primitive
                .get("material")
                .map(|id| assets::integer(id).map(|i| &scene["materials"][i]))
                .transpose()?;
            let pbr = material.map(|m| &m["pbrMetallicRoughness"]);
            let factor = numbers(pbr.and_then(|p| p.get("baseColorFactor")), [1.; 4])?;
            // Vertex colors in OBJ are a widely used extension (not core Wavefront).
            // Bake the base factor into them; keep Kd white to avoid applying it twice.
            let diffuse = if colors.is_some() {
                [1.; 3]
            } else {
                [factor[0], factor[1], factor[2]]
            };
            writeln!(
                mtl,
                "newmtl material_{part}\nKd {} {} {}\nd {}\nKs 0 0 0\nillum 2",
                diffuse[0], diffuse[1], diffuse[2], factor[3]
            )
            .unwrap();
            if let Some(texture) = pbr.and_then(|p| p.get("baseColorTexture")) {
                if texture.get("extensions").is_some() {
                    return Err("OBJ texture transforms are unsupported; use GLB".into());
                }
                let source = assets::integer(
                    &scene["textures"][assets::integer(&texture["index"])?]["source"],
                )?;
                let filename = format!("texture_{source}.png");
                if !images.iter().any(|(name, _)| name == &filename) {
                    let view = &scene["bufferViews"]
                        [assets::integer(&scene["images"][source]["bufferView"])?];
                    let offset = view
                        .get("byteOffset")
                        .map(assets::integer)
                        .transpose()?
                        .unwrap_or(0);
                    let length = assets::integer(&view["byteLength"])?;
                    images.push((filename.clone(), &binary[offset..offset + length]));
                }
                writeln!(mtl, "map_Kd {filename}").unwrap();
            }
            writeln!(obj, "o part_{part}\nusemtl material_{part}\ns 1").unwrap();
            for i in 0..positions.count {
                let local = positions.position(i);
                let p: [f64; 3] = finite(std::array::from_fn(|a| {
                    world[a][3] + (0..3).map(|k| world[a][k] * local[k]).sum::<f64>()
                }))?;
                write!(obj, "v {} {} {}", p[0], p[1], p[2]).unwrap();
                if let Some(colors) = &colors {
                    let c: [f64; 3] = finite(colors.values(i))?;
                    writeln!(
                        obj,
                        " {} {} {}",
                        srgb(c[0] * factor[0]),
                        srgb(c[1] * factor[1]),
                        srgb(c[2] * factor[2])
                    )
                    .unwrap();
                } else {
                    obj.push('\n');
                }
                if let Some(uvs) = &uvs {
                    let uv: [f64; 2] = finite(uvs.values(i))?;
                    // glTF images use a top-left origin; Wavefront uses bottom-left.
                    writeln!(obj, "vt {} {}", uv[0], 1. - uv[1]).unwrap();
                }
                if let Some(normals) = &normals {
                    let n = finite(normals.position(i))?;
                    // Inverse transpose of the world linear matrix (cofactors / det).
                    let n: [f64; 3] = finite(std::array::from_fn(|r| {
                        (0..3)
                            .map(|c| {
                                let cofactor = world[(r + 1) % 3][(c + 1) % 3]
                                    * world[(r + 2) % 3][(c + 2) % 3]
                                    - world[(r + 1) % 3][(c + 2) % 3]
                                        * world[(r + 2) % 3][(c + 1) % 3];
                                cofactor * n[c] / det
                            })
                            .sum()
                    }))?;
                    let length = n.iter().map(|v| v * v).sum::<f64>().sqrt();
                    if !length.is_finite() || length == 0. {
                        return Err("Invalid OBJ normal".into());
                    }
                    writeln!(
                        obj,
                        "vn {} {} {}",
                        n[0] / length,
                        n[1] / length,
                        n[2] / length
                    )
                    .unwrap();
                }
                if obj.len() > MAX_EXPORT_BYTES {
                    return Err("OBJ exceeds the 256 MB export limit".into());
                }
            }
            let indices = primitive
                .get("indices")
                .map(|id| {
                    assets::accessor_data(&scene, id, binary, "SCALAR", 1, &[5121, 5123, 5125])
                })
                .transpose()?;
            let count = indices.as_ref().map_or(positions.count, |v| v.count);
            for face in (0..count).step_by(3) {
                let mut corners = [face, face + 1, face + 2];
                if det < 0. {
                    corners.swap(1, 2);
                }
                obj.push('f');
                for corner in corners {
                    let v = indices.as_ref().map_or(corner, |data| data.index(corner));
                    write!(obj, " {}", vertex_offset + v).unwrap();
                    if uvs.is_some() || normals.is_some() {
                        obj.push('/');
                        if uvs.is_some() {
                            write!(obj, "{}", uv_offset + v).unwrap();
                        }
                        if normals.is_some() {
                            write!(obj, "/{}", normal_offset + v).unwrap();
                        }
                    }
                }
                obj.push('\n');
                if obj.len() > MAX_EXPORT_BYTES {
                    return Err("OBJ exceeds the 256 MB export limit".into());
                }
            }
            vertex_offset += positions.count;
            if uvs.is_some() {
                uv_offset += positions.count;
            }
            if normals.is_some() {
                normal_offset += positions.count;
            }
            part += 1;
        }
    }
    if obj.len() + mtl.len() + images.iter().map(|(_, b)| b.len()).sum::<usize>() > MAX_EXPORT_BYTES
    {
        return Err("OBJ package exceeds the 256 MB export limit".into());
    }
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored)
        .unix_permissions(0o644);
    for (name, data) in [("model.obj", obj.as_bytes()), ("model.mtl", mtl.as_bytes())]
        .into_iter()
        .chain(images.iter().map(|(name, bytes)| (name.as_str(), *bytes)))
    {
        zip.start_file(name, options).map_err(|e| e.to_string())?;
        zip.write_all(data).map_err(|e| e.to_string())?;
    }
    let bytes = zip.finish().map_err(|e| e.to_string())?.into_inner();
    if bytes.len() > MAX_EXPORT_BYTES {
        return Err("OBJ package exceeds the 256 MB export limit".into());
    }
    Ok(bytes)
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExchangeFormat {
    Obj,
    Gltf,
}

#[tauri::command]
pub async fn save_generated_exchange(
    app: tauri::AppHandle,
    harness: State<'_, SculptInferenceHarness>,
    asset_id: String,
    default_name: String,
    format: ExchangeFormat,
) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let glb = harness
        .library_work(move |library| assets::read_valid_glb(&library.asset_path(&asset_id)?))
        .await?;
    tauri::async_runtime::spawn_blocking(move || {
        let (bytes, extension, title) = match format {
            ExchangeFormat::Obj => (obj_archive(&glb)?, "zip", "Export OBJ + materials"),
            ExchangeFormat::Gltf => (embedded_gltf(&glb)?, "gltf", "Export embedded glTF"),
        };
        let name = Path::new(&default_name)
            .file_name()
            .and_then(|v| v.to_str())
            .unwrap_or("Sculpt");
        let Some(selected) = app
            .dialog()
            .file()
            .set_title(title)
            .set_file_name(name)
            .add_filter(title, &[extension])
            .blocking_save_file()
        else {
            return Ok(None);
        };
        let mut path = selected.into_path().map_err(|e| e.to_string())?;
        if path.extension().is_none() {
            path.set_extension(extension);
        }
        std::fs::write(&path, bytes).map_err(|e| format!("Could not save asset: {e}"))?;
        Ok(Some(path.to_string_lossy().into_owned()))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests;
