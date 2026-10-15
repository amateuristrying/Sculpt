use super::*;
use std::io::Read;

fn entries(bytes: &[u8]) -> std::collections::BTreeMap<String, Vec<u8>> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    (0..zip.len())
        .map(|i| {
            let mut file = zip.by_index(i).unwrap();
            let name = file.name().to_owned();
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes).unwrap();
            (name, bytes)
        })
        .collect()
}

fn textured() -> Vec<u8> {
    let glb = assets::test_triangle_glb();
    let (mut scene, binary) = unpack(&glb).unwrap();
    let mut binary = binary.to_vec();
    while binary.len() % 4 != 0 {
        binary.push(0);
    }
    let normal_start = binary.len();
    for value in [0f32, 0., 1.].repeat(3) {
        binary.extend(value.to_le_bytes());
    }
    let uv_start = binary.len();
    for value in [0f32, 1., 1., 1., 0., 0.] {
        binary.extend(value.to_le_bytes());
    }
    let image_start = binary.len();
    let image = image::RgbaImage::from_fn(8, 8, |x, y| {
        image::Rgba([x as u8 * 30, y as u8 * 30, 64, 255])
    });
    let mut png = Cursor::new(Vec::new());
    image.write_to(&mut png, image::ImageFormat::Png).unwrap();
    binary.extend(png.get_ref());
    let views = scene["bufferViews"].as_array_mut().unwrap();
    let view_id = views.len();
    views.extend([
        serde_json::json!({"buffer":0,"byteOffset":normal_start,"byteLength":36}),
        serde_json::json!({"buffer":0,"byteOffset":uv_start,"byteLength":24}),
        serde_json::json!({"buffer":0,"byteOffset":image_start,"byteLength":png.get_ref().len()}),
    ]);
    let accessors = scene["accessors"].as_array_mut().unwrap();
    let id = accessors.len();
    accessors.extend([
        serde_json::json!({"bufferView":view_id,"componentType":5126,"type":"VEC3","count":3}),
        serde_json::json!({"bufferView":view_id+1,"componentType":5126,"type":"VEC2","count":3}),
    ]);
    scene["meshes"][0]["primitives"][0]["attributes"]["NORMAL"] = id.into();
    scene["meshes"][0]["primitives"][0]["attributes"]["TEXCOORD_0"] = (id + 1).into();
    scene["meshes"][0]["primitives"][0]["material"] = 0.into();
    scene["materials"] =
        serde_json::json!([{"pbrMetallicRoughness":{"baseColorTexture":{"index":0}}}]);
    scene["textures"] = serde_json::json!([{"source":0}]);
    scene["images"] = serde_json::json!([{"bufferView":view_id+2,"mimeType":"image/png"}]);
    scene["buffers"][0]["byteLength"] = binary.len().into();
    assets::test_encode_glb(&scene, &binary)
}

#[test]
fn gltf_keeps_binary_and_scene_exactly_including_texture() {
    for glb in [assets::test_triangle_glb(), textured()] {
        let (scene, binary) = unpack(&glb).unwrap();
        let bytes = embedded_gltf(&glb).unwrap();
        let mut converted: Value = serde_json::from_slice(&bytes).unwrap();
        let uri = converted["buffers"][0]
            .as_object_mut()
            .unwrap()
            .remove("uri")
            .unwrap();
        let base64 = uri
            .as_str()
            .unwrap()
            .strip_prefix("data:application/octet-stream;base64,")
            .unwrap();
        assert_eq!(STANDARD.decode(base64).unwrap(), binary);
        assert_eq!(converted, scene);
    }
}

#[test]
fn obj_includes_mtl_original_png_normals_and_flipped_uvs() {
    let glb = textured();
    let files = entries(&obj_archive(&glb).unwrap());
    assert_eq!(
        files.keys().map(String::as_str).collect::<Vec<_>>(),
        ["model.mtl", "model.obj", "texture_0.png"]
    );
    let obj = std::str::from_utf8(&files["model.obj"]).unwrap();
    assert!(obj.contains("vt 0 0\n") && obj.contains("vt 0 1\n"));
    assert_eq!(obj.matches("vn 0 0 1\n").count(), 3);
    assert!(obj.contains("f 1/1/1 2/2/2 3/3/3\n"));
    assert!(std::str::from_utf8(&files["model.mtl"])
        .unwrap()
        .contains("map_Kd texture_0.png\n"));
    let decoded = image::load_from_memory(&files["texture_0.png"])
        .unwrap()
        .to_rgba8();
    assert_eq!(decoded.get_pixel(7, 0).0, [210, 0, 64, 255]);
    assert_eq!(decoded.get_pixel(0, 7).0, [0, 210, 64, 255]);
}

#[test]
fn obj_preserves_instance_offsets_transforms_and_reflected_winding() {
    let glb = textured();
    let (mut scene, binary) = unpack(&glb).unwrap();
    scene["nodes"] = serde_json::json!([
        {"children":[1,2],"translation":[10,20,30]}, {"mesh":0,"scale":[2,3,4]}, {"mesh":0,"scale":[-2,3,4]}
    ]);
    let converted = assets::test_encode_glb(&scene, binary);
    let files = entries(&obj_archive(&converted).unwrap());
    let obj = std::str::from_utf8(&files["model.obj"]).unwrap();
    assert!(
        obj.contains("v 10 20 30\n") && obj.contains("v 12 20 30\n") && obj.contains("v 8 20 30\n")
    );
    assert!(obj.contains("f 1/1/1 2/2/2 3/3/3\n"));
    assert!(obj.contains("f 4/4/4 6/6/6 5/5/5\n"));
    let normals: Vec<Vec<f64>> = obj
        .lines()
        .filter(|s| s.starts_with("vn "))
        .map(|s| {
            s.split_whitespace()
                .skip(1)
                .map(|v| v.parse().unwrap())
                .collect()
        })
        .collect();
    assert_eq!(normals, vec![vec![0., 0., 1.]; 6]);
    assert_eq!(files.len(), 3, "instanced material should reuse one PNG");
}

#[test]
fn obj_normals_use_inverse_transpose_under_shear() {
    let glb = textured();
    let (mut scene, binary) = unpack(&glb).unwrap();
    scene["nodes"][0]["matrix"] =
        serde_json::json!([1, 0, 1, 0, 0, 1, 0, 0, 0, 0, 2, 0, 0, 0, 0, 1]);
    let files = entries(&obj_archive(&assets::test_encode_glb(&scene, binary)).unwrap());
    let obj = std::str::from_utf8(&files["model.obj"]).unwrap();
    let normal: Vec<f64> = obj
        .lines()
        .find(|s| s.starts_with("vn "))
        .unwrap()
        .split_whitespace()
        .skip(1)
        .map(|v| v.parse().unwrap())
        .collect();
    let diagonal = 1. / 2f64.sqrt();
    for (actual, expected) in normal.iter().zip([-diagonal, 0., diagonal]) {
        assert!((actual - expected).abs() < 1e-12);
    }
}

#[test]
fn obj_converts_linear_vertex_colors_and_accepts_unindexed_geometry() {
    let glb = assets::test_triangle_glb();
    let (mut scene, binary) = unpack(&glb).unwrap();
    let mut binary = binary.to_vec();
    let offset = binary.len();
    binary.extend([55u8, 0, 255, 255].repeat(3));
    let view = scene["bufferViews"].as_array().unwrap().len();
    scene["bufferViews"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({"buffer":0,"byteOffset":offset,"byteLength":12}));
    let id = scene["accessors"].as_array().unwrap().len();
    scene["accessors"].as_array_mut().unwrap().push(serde_json::json!({"bufferView":view,"componentType":5121,"normalized":true,"count":3,"type":"VEC4"}));
    scene["meshes"][0]["primitives"][0]["attributes"]["COLOR_0"] = id.into();
    scene["meshes"][0]["primitives"][0]
        .as_object_mut()
        .unwrap()
        .remove("indices");
    scene["buffers"][0]["byteLength"] = binary.len().into();
    let files = entries(&obj_archive(&assets::test_encode_glb(&scene, &binary)).unwrap());
    let obj = std::str::from_utf8(&files["model.obj"]).unwrap();
    let first: Vec<f64> = obj
        .lines()
        .find(|s| s.starts_with("v "))
        .unwrap()
        .split_whitespace()
        .skip(1)
        .map(|v| v.parse().unwrap())
        .collect();
    assert!((first[3] - 128. / 255.).abs() < 0.002);
    assert_eq!(first[4], 0.);
    assert!((first[5] - 1.).abs() < 1e-12);
    assert!(obj.contains("f 1 2 3\n"));
    assert!(!obj.contains("vt ") && !obj.contains("vn "));
}

#[test]
fn unsupported_or_malformed_attributes_fail_cleanly() {
    assert!(embedded_gltf(b"bad").is_err());
    assert!(obj_archive(b"bad").is_err());
    let glb = textured();
    for change in [
        "collapsed",
        "short-normal",
        "required-extension",
        "texture-transform",
    ] {
        let (mut scene, binary) = unpack(&glb).unwrap();
        match change {
            "collapsed" => scene["nodes"][0]["scale"] = serde_json::json!([0, 1, 1]),
            "short-normal" => {
                let id =
                    assets::integer(&scene["meshes"][0]["primitives"][0]["attributes"]["NORMAL"])
                        .unwrap();
                scene["accessors"][id]["count"] = 1.into();
            }
            "required-extension" => {
                scene["extensionsRequired"] = serde_json::json!(["KHR_materials_unlit"])
            }
            _ => {
                scene["materials"][0]["pbrMetallicRoughness"]["baseColorTexture"]["extensions"] =
                    serde_json::json!({"KHR_texture_transform":{"offset":[0.5,0.5]}})
            }
        }
        assert!(
            obj_archive(&assets::test_encode_glb(&scene, binary)).is_err(),
            "{change}"
        );
    }
}

#[test]
#[ignore = "requires a saved GLB and output directory for independent importer checks"]
fn export_saved_exchange_assets() {
    let input = std::env::var("SCULPT_EXCHANGE_INPUT").unwrap();
    let output = std::env::var("SCULPT_EXCHANGE_OUTPUT").unwrap();
    for (name, glb) in [
        (
            "reconstruction",
            assets::read_valid_glb(Path::new(&input)).unwrap(),
        ),
        ("uv-orientation", textured()),
    ] {
        let directory = Path::new(&output).join(name);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("source.glb"), &glb).unwrap();
        std::fs::write(directory.join("model.gltf"), embedded_gltf(&glb).unwrap()).unwrap();
        std::fs::write(directory.join("model.zip"), obj_archive(&glb).unwrap()).unwrap();
    }
}
