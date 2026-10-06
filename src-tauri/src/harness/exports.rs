//! Native exports operate on verified library assets, independently of inference.
use super::{assets, SculptInferenceHarness};
use serde_json::Value;
use std::path::Path;
use tauri::State;

type Matrix = [[f64; 4]; 4]; // row-major internally; glTF serializes column-major
type Triangle = [[f64; 3]; 3];
const IDENTITY: Matrix = [
    [1., 0., 0., 0.],
    [0., 1., 0., 0.],
    [0., 0., 1., 0.],
    [0., 0., 0., 1.],
];
const MAX_FACES: usize = (assets::MAX_GLB_BYTES - 84) / 50;

fn multiply(a: Matrix, b: Matrix) -> Matrix {
    std::array::from_fn(|r| std::array::from_fn(|c| (0..4).map(|k| a[r][k] * b[k][c]).sum()))
}

fn numbers<const N: usize>(value: Option<&Value>, fallback: [f64; N]) -> Result<[f64; N], String> {
    let Some(value) = value else {
        return Ok(fallback);
    };
    let values = value
        .as_array()
        .filter(|v| v.len() == N)
        .ok_or("Invalid node transform")?;
    let mut result = fallback;
    for (output, value) in result.iter_mut().zip(values) {
        *output = value
            .as_f64()
            .filter(|v| v.is_finite())
            .ok_or("Invalid node transform")?;
    }
    Ok(result)
}

fn transform(node: &Value) -> Result<Matrix, String> {
    if let Some(value) = node.get("matrix") {
        if ["translation", "rotation", "scale"]
            .iter()
            .any(|key| node.get(key).is_some())
        {
            return Err("Node mixes matrix and TRS transforms".into());
        }
        let flat = numbers(Some(value), [0.; 16])?;
        let matrix: Matrix = std::array::from_fn(|r| std::array::from_fn(|c| flat[c * 4 + r]));
        if matrix[3] != [0., 0., 0., 1.] {
            return Err("Non-affine node transform".into());
        }
        return Ok(matrix);
    }
    let t = numbers(node.get("translation"), [0.; 3])?;
    let s = numbers(node.get("scale"), [1.; 3])?;
    let q = numbers(node.get("rotation"), [0., 0., 0., 1.])?;
    let length = q.iter().map(|v| v * v).sum::<f64>().sqrt();
    if (length - 1.).abs() > 1e-4 {
        return Err("Invalid node rotation quaternion".into());
    }
    let [x, y, z, w] = q.map(|v| v / length);
    let mut matrix = [
        [
            1. - 2. * (y * y + z * z),
            2. * (x * y - z * w),
            2. * (x * z + y * w),
            t[0],
        ],
        [
            2. * (x * y + z * w),
            1. - 2. * (x * x + z * z),
            2. * (y * z - x * w),
            t[1],
        ],
        [
            2. * (x * z - y * w),
            2. * (y * z + x * w),
            1. - 2. * (x * x + y * y),
            t[2],
        ],
        [0., 0., 0., 1.],
    ];
    for row in matrix.iter_mut().take(3) {
        for c in 0..3 {
            row[c] *= s[c];
        }
    }
    Ok(matrix)
}

fn determinant(m: Matrix) -> f64 {
    m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
}

/// STL has no unit metadata. Coordinates are millimetres, Z-up, centered on XY
/// and resting on Z=0. Uniform scaling preserves proportions and connectivity.
pub fn binary_stl(glb: &[u8], height_mm: f64) -> Result<Vec<u8>, String> {
    if !height_mm.is_finite() || !(0.1..=10_000.).contains(&height_mm) {
        return Err("Object height must be between 0.1 and 10,000 mm".into());
    }
    assets::validate_glb(glb)?;
    // Chunk bounds and accessor data were validated above, including unknown chunks.
    let json_length = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    let scene: Value =
        serde_json::from_slice(&glb[20..20 + json_length]).map_err(|e| e.to_string())?;
    let mut offset = 20 + json_length;
    let binary = loop {
        let length = u32::from_le_bytes(glb[offset..offset + 4].try_into().unwrap()) as usize;
        if &glb[offset + 4..offset + 8] == b"BIN\0" {
            break &glb[offset + 8..offset + 8 + length];
        }
        offset += 8 + length;
    };
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
        .map(|id| Ok((assets::integer(id)?, IDENTITY)))
        .collect::<Result<Vec<_>, String>>()?;
    let mut visited = vec![false; nodes.len()];
    let mut triangles: Vec<Triangle> = Vec::new();
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    while let Some((index, parent)) = stack.pop() {
        if visited[index] {
            return Err("STL export requires a scene tree without shared nodes".into());
        }
        visited[index] = true;
        let node = &nodes[index];
        if node.get("skin").is_some() {
            return Err("Skinned meshes cannot be exported as static STL".into());
        }
        let world = multiply(parent, transform(node)?);
        let det = determinant(world);
        if !det.is_finite() || det == 0. {
            return Err("Invalid or collapsed node transform".into());
        }
        if let Some(children) = node["children"].as_array() {
            for child in children {
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
                return Err("Morph targets are unsupported for STL export".into());
            }
            let positions = assets::accessor_data(
                &scene,
                &primitive["attributes"]["POSITION"],
                binary,
                "VEC3",
                3,
                &[5126],
            )?;
            let indices = primitive
                .get("indices")
                .map(|id| {
                    assets::accessor_data(&scene, id, binary, "SCALAR", 1, &[5121, 5123, 5125])
                })
                .transpose()?;
            let count = indices.as_ref().map_or(positions.count, |data| data.count);
            if triangles.len() + count / 3 > MAX_FACES {
                return Err("STL exceeds the 128 MB export limit".into());
            }
            for face in (0..count).step_by(3) {
                let mut triangle = [[0.; 3]; 3];
                for (corner, point) in triangle.iter_mut().enumerate() {
                    let vertex = indices
                        .as_ref()
                        .map_or(face + corner, |data| data.index(face + corner));
                    let local = positions.position(vertex);
                    for axis in 0..3 {
                        point[axis] =
                            world[axis][3] + (0..3).map(|k| world[axis][k] * local[k]).sum::<f64>();
                        if !point[axis].is_finite() {
                            return Err("Non-finite transformed geometry".into());
                        }
                        min[axis] = min[axis].min(point[axis]);
                        max[axis] = max[axis].max(point[axis]);
                    }
                }
                if det < 0. {
                    triangle.swap(1, 2);
                }
                triangles.push(triangle);
            }
        }
    }
    let scale = height_mm / (max[1] - min[1]);
    if !scale.is_finite() || scale <= 0. {
        return Err("The mesh has no measurable height".into());
    }
    let mut bytes = vec![0; 80];
    let header = b"Sculpt binary STL | millimetres | Z up";
    bytes[..header.len()].copy_from_slice(header);
    bytes.extend((triangles.len() as u32).to_le_bytes());
    bytes.reserve(triangles.len() * 50);
    for triangle in triangles {
        let points = triangle.map(|p| {
            [
                ((p[0] - min[0]) - (max[0] - min[0]) / 2.) * scale,
                -((p[2] - min[2]) - (max[2] - min[2]) / 2.) * scale,
                (p[1] - min[1]) * scale,
            ]
            .map(|v| v as f32)
        });
        if points.iter().flatten().any(|v| !v.is_finite()) {
            return Err("Mesh dimensions exceed STL precision".into());
        }
        let a: [f64; 3] = std::array::from_fn(|k| points[1][k] as f64 - points[0][k] as f64);
        let b: [f64; 3] = std::array::from_fn(|k| points[2][k] as f64 - points[0][k] as f64);
        let normal = [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ];
        let length = normal.iter().map(|v| v * v).sum::<f64>().sqrt();
        for value in normal
            .map(|v| if length > 0. { (v / length) as f32 } else { 0. })
            .into_iter()
            .chain(points.into_iter().flatten())
        {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend(0u16.to_le_bytes());
    }
    Ok(bytes)
}

#[tauri::command]
pub async fn save_generated_stl(
    app: tauri::AppHandle,
    harness: State<'_, SculptInferenceHarness>,
    asset_id: String,
    default_name: String,
    height_mm: f64,
) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let glb = harness
        .library_work(move |library| assets::read_valid_glb(&library.asset_path(&asset_id)?))
        .await?;
    // Conversion and save dialog run off the UI thread and outside the library lock.
    tauri::async_runtime::spawn_blocking(move || {
        let bytes = binary_stl(&glb, height_mm)?;
        let name = Path::new(&default_name)
            .file_name()
            .and_then(|v| v.to_str())
            .unwrap_or("Sculpt.stl");
        let Some(selected) = app
            .dialog()
            .file()
            .set_title("Export STL in millimetres")
            .set_file_name(name)
            .add_filter("Binary STL", &["stl"])
            .blocking_save_file()
        else {
            return Ok(None);
        };
        let mut path = selected.into_path().map_err(|e| e.to_string())?;
        if path.extension().is_none() {
            path.set_extension("stl");
        }
        std::fs::write(&path, bytes).map_err(|e| format!("Could not save STL: {e}"))?;
        Ok(Some(path.to_string_lossy().into_owned()))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(edit: impl FnOnce(&mut Value)) -> Vec<u8> {
        let glb = assets::test_triangle_glb();
        let length = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
        let mut scene: Value = serde_json::from_slice(&glb[20..20 + length]).unwrap();
        edit(&mut scene);
        assets::test_encode_glb(&scene, &glb[28 + length..])
    }

    fn floats(bytes: &[u8]) -> Vec<f32> {
        bytes
            .chunks_exact(4)
            .map(|v| f32::from_le_bytes(v.try_into().unwrap()))
            .collect()
    }

    #[test]
    fn binary_records_have_mm_height_z_up_and_unit_normals() {
        let stl = binary_stl(&assets::test_triangle_glb(), 120.).unwrap();
        assert_eq!(stl.len(), 134);
        assert_eq!(&stl[80..84], &1u32.to_le_bytes());
        let record = floats(&stl[84..132]);
        assert_eq!(&record[..3], &[0., -1., 0.]);
        assert_eq!(&record[3..], &[-60., 0., 0., 60., 0., 0., -60., 0., 120.]);
        assert_eq!(&stl[132..], &[0, 0]);
    }

    #[test]
    fn nested_transforms_instances_and_reflected_winding_are_applied() {
        let glb = fixture(|scene| {
            scene["nodes"] = serde_json::json!([
                {"children":[1,2], "translation":[10,20,30], "scale":[2,3,1]},
                {"mesh":0}, {"mesh":0, "translation":[2,0,0], "scale":[-1,1,1]}
            ]);
        });
        let stl = binary_stl(&glb, 90.).unwrap();
        assert_eq!(stl.len(), 184);
        for offset in [84, 134] {
            assert_eq!(&floats(&stl[offset..offset + 12]), &[0., -1., 0.]);
        }
        let positions: Vec<_> = [84, 134]
            .into_iter()
            .flat_map(|o| floats(&stl[o + 12..o + 48]))
            .collect();
        let xs: Vec<_> = positions.chunks(3).map(|p| p[0]).collect();
        assert_eq!(xs.iter().copied().fold(f32::INFINITY, f32::min), -60.);
        assert_eq!(xs.iter().copied().fold(f32::NEG_INFINITY, f32::max), 60.);
    }

    #[test]
    fn column_major_matrix_matches_trs_and_unindexed_geometry() {
        let trs = fixture(|s| {
            s["nodes"][0]["scale"] = serde_json::json!([2, 3, 4]);
        });
        let matrix = fixture(|s| {
            s["nodes"][0]["matrix"] =
                serde_json::json!([2, 0, 0, 0, 0, 3, 0, 0, 0, 0, 4, 0, 10, 20, 30, 1]);
            s["meshes"][0]["primitives"][0]
                .as_object_mut()
                .unwrap()
                .remove("indices");
        });
        assert_eq!(
            binary_stl(&trs, 90.).unwrap(),
            binary_stl(&matrix, 90.).unwrap()
        );
        let q = fixture(|s| {
            s["nodes"][0]["rotation"] = serde_json::json!([0, 0, 1, 0]);
        });
        let rotated = binary_stl(&q, 100.).unwrap();
        assert_eq!(&floats(&rotated[84..96]), &[0., -1., 0.]);
    }

    #[test]
    fn invalid_sizes_and_unsupported_transforms_fail_without_panics() {
        for height in [f64::NAN, f64::INFINITY, 0., -1., 10001.] {
            assert!(binary_stl(&assets::test_triangle_glb(), height).is_err());
        }
        for node in [
            serde_json::json!({"mesh":0,"scale":[0,1,1]}),
            serde_json::json!({"mesh":0,"rotation":[0,0,0,0]}),
            serde_json::json!({"mesh":0,"matrix":[1,2]}),
            serde_json::json!({"mesh":0,"skin":0}),
            serde_json::json!({"mesh":0,"translation":["bad",0,0]}),
        ] {
            assert!(binary_stl(&fixture(|s| s["nodes"][0] = node), 100.).is_err());
        }
        assert!(binary_stl(b"invalid", 100.).is_err());
        assert!(binary_stl(
            &fixture(|s| s["scenes"][0]["nodes"] = serde_json::json!([0, 0])),
            100.
        )
        .is_err());
    }

    /// Export a complete saved evaluation run without downloading or re-running AI.
    #[test]
    #[ignore = "requires local evaluation GLBs and an output directory"]
    fn export_evaluation_run() {
        let input = std::env::var("SCULPT_STL_INPUT").expect("SCULPT_STL_INPUT");
        let output = std::env::var("SCULPT_STL_OUTPUT").expect("SCULPT_STL_OUTPUT");
        std::fs::create_dir_all(&output).unwrap();
        let mut count = 0;
        for entry in std::fs::read_dir(input).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path().join("mesh.glb");
            if !path.is_file() {
                continue;
            }
            let glb = assets::read_valid_glb(&path).unwrap();
            let stl = binary_stl(&glb, 100.).unwrap();
            let destination = Path::new(&output)
                .join(entry.file_name())
                .with_extension("stl");
            std::fs::write(destination, stl).unwrap();
            count += 1;
        }
        assert!(count > 0, "No evaluation meshes found");
        println!("Exported {count} meshes at 100 mm");
    }
}
