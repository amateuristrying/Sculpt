"""Self-contained glTF assets with explicit material semantics."""
def linear_vertex_colors(colors):
    import numpy as np
    result = colors.copy()
    rgb = result[:, :3].astype(np.float64) / 255
    linear = np.where(rgb <= 0.04045, rgb / 12.92, ((rgb + 0.055) / 1.055) ** 2.4)
    result[:, :3] = np.rint(linear * 255).astype(np.uint8)
    return result


def export_glb(mesh, output):
    from trimesh.visual.material import PBRMaterial
    from trimesh.visual.texture import TextureVisuals
    from .files import atomic_write_bytes

    # glTF's implicit material is metallic. Most source objects are not metal;
    # make a neutral, rough surface explicit, retaining every reconstructed color.
    # The model predicts image-space RGB. glTF COLOR_0 is a linear multiplier;
    # leaving those samples in sRGB makes exported objects pale and washed out.
    colors = linear_vertex_colors(mesh.visual.vertex_colors)
    visual = TextureVisuals(material=PBRMaterial(
        name='Reconstructed color', baseColorFactor=[255, 255, 255, 255],
        metallicFactor=0.0, roughnessFactor=0.8))
    visual.vertex_attributes['color'] = colors
    mesh.visual = visual
    atomic_write_bytes(output, mesh.export(file_type='glb', include_normals=True))
