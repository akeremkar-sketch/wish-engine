//! Triangle meshes on the CPU, and the built-in shapes.
//!
//! Triangles wind counter-clockwise seen from outside (their front face),
//! which is what the renderer draws; the tests check this for every shape.

use wish_core::math::{Vec2, Vec3, Vec4};

use crate::components::Shape;

/// A triangle mesh: positions, normals, texture coordinates and tangents per
/// vertex, three indices per triangle.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshData {
    /// Vertex positions, local space.
    pub positions: Vec<Vec3>,
    /// Unit normals, one per vertex.
    pub normals: Vec<Vec3>,
    /// Texture coordinates, one per vertex.
    pub uvs: Vec<Vec2>,
    /// Tangents (xyz direction, w the bitangent's sign), one per vertex;
    /// zero where there is no normal map.
    pub tangents: Vec<Vec4>,
    /// Three per triangle, counter-clockwise seen from the front.
    pub indices: Vec<u32>,
}

/// Bytes per vertex in the GPU vertex buffer: position (12), normal (12),
/// texture coordinates (8), tangent (16).
pub(crate) const VERTEX_STRIDE: u64 = 48;

impl MeshData {
    /// The vertex buffer: position and normal, little-endian `f32`s.
    pub(crate) fn vertex_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.positions.len() * VERTEX_STRIDE as usize);
        for (index, position) in self.positions.iter().enumerate() {
            let normal = self.normals.get(index).copied().unwrap_or(Vec3::Y);
            let uv = self.uvs.get(index).copied().unwrap_or(Vec2::ZERO);
            let tangent = self.tangents.get(index).copied().unwrap_or(Vec4::ZERO);
            let values = position
                .to_array()
                .into_iter()
                .chain(normal.to_array())
                .chain(uv.to_array())
                .chain(tangent.to_array());
            for value in values {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        bytes
    }

    /// The index buffer, little-endian `u32`s.
    pub(crate) fn index_bytes(&self) -> Vec<u8> {
        self.indices
            .iter()
            .flat_map(|index| index.to_le_bytes())
            .collect()
    }

    fn push_quad(&mut self, corners: [Vec3; 4], normal: Vec3) {
        let base = u32::try_from(self.positions.len()).unwrap_or(u32::MAX);
        self.positions.extend(corners);
        self.normals.extend([normal; 4]);
        self.uvs.extend([Vec2::ZERO, Vec2::X, Vec2::ONE, Vec2::Y]);
        self.tangents.extend([Vec4::ZERO; 4]);
        self.indices
            .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

impl From<&wish_import::Primitive> for MeshData {
    fn from(primitive: &wish_import::Primitive) -> Self {
        Self {
            positions: primitive.positions.clone(),
            normals: primitive.normals.clone(),
            uvs: primitive.uvs.clone(),
            tangents: primitive.tangents.clone(),
            indices: primitive.indices.clone(),
        }
    }
}

/// The mesh of a built-in shape (1 m across, centred on the origin).
pub fn primitive(shape: Shape) -> MeshData {
    match shape {
        Shape::Cube => cube(),
        Shape::Sphere => sphere(32, 16),
        Shape::Plane => plane(),
    }
}

fn plane() -> MeshData {
    let mut mesh = MeshData::default();
    let h = 0.5;
    // Counter-clockwise seen from +Y.
    mesh.push_quad(
        [
            Vec3::new(-h, 0.0, -h),
            Vec3::new(-h, 0.0, h),
            Vec3::new(h, 0.0, h),
            Vec3::new(h, 0.0, -h),
        ],
        Vec3::Y,
    );
    mesh
}

fn cube() -> MeshData {
    let mut mesh = MeshData::default();
    let h = 0.5;
    // For each face: its normal and two in-face axes `u`, `v` with
    // u x v = normal, so u, then u+v, then v runs counter-clockwise.
    let faces = [
        (Vec3::X, Vec3::NEG_Z, Vec3::Y),
        (Vec3::NEG_X, Vec3::Z, Vec3::Y),
        (Vec3::Y, Vec3::X, Vec3::NEG_Z),
        (Vec3::NEG_Y, Vec3::X, Vec3::Z),
        (Vec3::Z, Vec3::X, Vec3::Y),
        (Vec3::NEG_Z, Vec3::NEG_X, Vec3::Y),
    ];
    for (normal, u, v) in faces {
        let centre = normal * h;
        mesh.push_quad(
            [
                centre - u * h - v * h,
                centre + u * h - v * h,
                centre + u * h + v * h,
                centre - u * h + v * h,
            ],
            normal,
        );
    }
    mesh
}

/// A UV sphere of radius 0.5 with `segments` around and `rings` from pole to
/// pole.
fn sphere(segments: u32, rings: u32) -> MeshData {
    let mut mesh = MeshData::default();
    for ring in 0..=rings {
        let theta = core::f32::consts::PI * ring as f32 / rings as f32;
        for segment in 0..=segments {
            let phi = core::f32::consts::TAU * segment as f32 / segments as f32;
            let normal = Vec3::new(
                theta.sin() * phi.cos(),
                theta.cos(),
                -theta.sin() * phi.sin(),
            );
            mesh.positions.push(normal * 0.5);
            mesh.normals.push(normal);
            mesh.uvs.push(Vec2::new(
                segment as f32 / segments as f32,
                ring as f32 / rings as f32,
            ));
            mesh.tangents.push(Vec4::ZERO);
        }
    }
    let row = segments + 1;
    for ring in 0..rings {
        for segment in 0..segments {
            let a = ring * row + segment;
            let b = a + row;
            // Skip the triangles that collapse to a line at the poles.
            if ring != 0 {
                mesh.indices.extend([a, b, a + 1]);
            }
            if ring != rings - 1 {
                mesh.indices.extend([a + 1, b, b + 1]);
            }
        }
    }
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check_winding(mesh: &MeshData) {
        assert_eq!(mesh.indices.len() % 3, 0);
        for triangle in mesh.indices.chunks(3) {
            let [a, b, c] = [triangle[0], triangle[1], triangle[2]].map(|index| index as usize);
            let (pa, pb, pc) = (mesh.positions[a], mesh.positions[b], mesh.positions[c]);
            let face = (pb - pa).cross(pc - pa);
            assert!(face.length() > 1e-9, "degenerate triangle {triangle:?}");
            let normal = mesh.normals[a] + mesh.normals[b] + mesh.normals[c];
            assert!(
                face.dot(normal) > 0.0,
                "triangle {triangle:?} faces inwards"
            );
        }
    }

    #[test]
    fn every_shape_winds_outwards_and_has_unit_normals() {
        for shape in [Shape::Cube, Shape::Sphere, Shape::Plane] {
            let mesh = primitive(shape);
            assert_eq!(mesh.positions.len(), mesh.normals.len());
            assert!(mesh.normals.iter().all(|n| (n.length() - 1.0).abs() < 1e-5));
            check_winding(&mesh);
            let extent = mesh
                .positions
                .iter()
                .fold(0.0f32, |m, p| m.max(p.abs().max_element()));
            assert!((extent - 0.5).abs() < 1e-6, "{shape:?} is 1 m across");
        }
    }

    #[test]
    fn buffers_have_the_documented_layout() {
        let mesh = primitive(Shape::Plane);
        assert_eq!(mesh.vertex_bytes().len() as u64, 4 * VERTEX_STRIDE);
        assert_eq!(mesh.index_bytes().len(), 6 * 4);
        let first: [u8; 4] = mesh.vertex_bytes()[..4].try_into().unwrap();
        assert_eq!(f32::from_le_bytes(first), -0.5);
    }
}
