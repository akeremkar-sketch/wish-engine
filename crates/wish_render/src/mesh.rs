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

/// The distinct edges of a triangle mesh as pairs of vertex indices, in a
/// fixed order (sorted), for drawing wireframes. Two triangles that share an
/// edge give it once.
pub(crate) fn edge_indices(indices: &[u32]) -> Vec<u32> {
    let mut edges = std::collections::BTreeSet::new();
    for triangle in indices.as_chunks::<3>().0 {
        for (a, b) in [(0, 1), (1, 2), (2, 0)] {
            let (low, high) = (triangle[a].min(triangle[b]), triangle[a].max(triangle[b]));
            edges.insert((low, high));
        }
    }
    edges.into_iter().flat_map(|(a, b)| [a, b]).collect()
}

/// The mesh of a built-in shape (1 m across, centred on the origin).
pub fn primitive(shape: Shape) -> MeshData {
    match shape {
        Shape::Cube => cube(),
        Shape::Sphere => sphere(32, 16),
        Shape::Plane => plane(),
    }
}

/// The mesh of a built-in shape for a material with textures: the triangles
/// of [`primitive`], with texture coordinates that show a picture the right
/// way round and tangents for normal maps.
///
/// Texture coordinates follow glTF: [0, 0] is the picture's top left corner.
/// - **Cube:** every face shows the whole picture, upright on the four
///   sides seen from outside; on the top its upper edge is at -Z, on the
///   bottom at +Z.
/// - **Plane:** the picture's left edge is at -X and its upper edge at -Z:
///   seen from above by a camera that looks along -Z, it reads as it does
///   in an image viewer.
/// - **Sphere:** the picture wraps once around (its seam at +X, its middle
///   at -X, running to the right seen from outside), its upper edge at the
///   north pole.
///
/// Tangents point along the picture's width, with a bitangent sign of +1:
/// `cross(normal, tangent)` points to the picture's upper edge, which is
/// "up" in a glTF normal map.
///
/// [`primitive`] itself is left as it always was (other texture coordinates,
/// no tangents): surfaces without textures are drawn from it, so they are
/// drawn exactly as before there were textures on shapes.
pub fn primitive_textured(shape: Shape) -> MeshData {
    let mut mesh = primitive(shape);
    match shape {
        Shape::Cube => {
            // Each face is four vertices: along u, then along v. Flipping v
            // puts the picture's upper edge where the face's `v` axis ends.
            for uv in &mut mesh.uvs {
                uv.y = 1.0 - uv.y;
            }
            for (face, tangents) in mesh.tangents.chunks_mut(4).enumerate() {
                let along = match (
                    mesh.positions.get(face * 4),
                    mesh.positions.get(face * 4 + 1),
                ) {
                    (Some(first), Some(second)) => (*second - *first).normalize_or_zero(),
                    _ => Vec3::X,
                };
                tangents.fill(along.extend(1.0));
            }
        }
        Shape::Plane => {
            for (uv, position) in mesh.uvs.iter_mut().zip(&mesh.positions) {
                *uv = Vec2::new(position.x + 0.5, position.z + 0.5);
            }
            mesh.tangents.fill(Vec4::new(1.0, 0.0, 0.0, 1.0));
        }
        Shape::Sphere => {
            for (tangent, uv) in mesh.tangents.iter_mut().zip(&mesh.uvs) {
                let phi = core::f32::consts::TAU * uv.x;
                *tangent = Vec4::new(-phi.sin(), 0.0, -phi.cos(), 1.0);
            }
        }
    }
    mesh
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
    fn textured_shapes_keep_the_triangles_and_carry_a_right_handed_picture_frame() {
        for shape in [Shape::Cube, Shape::Sphere, Shape::Plane] {
            let plain = primitive(shape);
            let mesh = primitive_textured(shape);
            assert_eq!(mesh.positions, plain.positions, "{shape:?}");
            assert_eq!(mesh.normals, plain.normals, "{shape:?}");
            assert_eq!(mesh.indices, plain.indices, "{shape:?}");
            // The untextured mesh is the one it always was.
            assert!(plain.tangents.iter().all(|t| *t == Vec4::ZERO));
            for (index, tangent) in mesh.tangents.iter().enumerate() {
                let along = tangent.truncate();
                assert!((along.length() - 1.0).abs() < 1e-5, "{shape:?} {index}");
                assert!(along.dot(mesh.normals[index]).abs() < 1e-5);
                assert_eq!(tangent.w, 1.0);
            }
            // Across every triangle the tangent is where u grows, and
            // cross(normal, tangent) is where v shrinks (the picture's top).
            for triangle in mesh.indices.chunks(3) {
                let [a, b, c] = [triangle[0], triangle[1], triangle[2]].map(|i| i as usize);
                let (e1, e2) = (
                    mesh.positions[b] - mesh.positions[a],
                    mesh.positions[c] - mesh.positions[a],
                );
                let (d1, d2) = (mesh.uvs[b] - mesh.uvs[a], mesh.uvs[c] - mesh.uvs[a]);
                let det = d1.x * d2.y - d2.x * d1.y;
                if det.abs() < 1e-6 {
                    continue;
                }
                let along_u = (e1 * d2.y - e2 * d1.y) / det;
                let along_v = (e2 * d1.x - e1 * d2.x) / det;
                let tangent = mesh.tangents[a].truncate();
                let up = mesh.normals[a].cross(tangent);
                assert!(along_u.normalize().dot(tangent) > 0.9, "{shape:?} u");
                assert!(along_v.normalize().dot(up) < -0.9, "{shape:?} v");
            }
        }
        // A cube's side shows the picture upright: v is 0 at the top.
        let cube = primitive_textured(Shape::Cube);
        for ((position, normal), uv) in cube.positions.iter().zip(&cube.normals).zip(&cube.uvs) {
            if normal.y == 0.0 {
                assert_eq!(uv.y, if position.y > 0.0 { 0.0 } else { 1.0 });
            }
        }
        // The plane reads like a picture seen from above: left at -X, top
        // at -Z.
        let plane = primitive_textured(Shape::Plane);
        for (position, uv) in plane.positions.iter().zip(&plane.uvs) {
            assert_eq!((uv.x, uv.y), (position.x + 0.5, position.z + 0.5));
        }
    }

    #[test]
    fn shared_edges_are_listed_once_in_a_fixed_order() {
        // Two triangles sharing the edge 0-2 of a quad: 5 distinct edges.
        let edges = edge_indices(&[0, 1, 2, 0, 2, 3]);
        assert_eq!(edges, [0, 1, 0, 2, 0, 3, 1, 2, 2, 3]);
        assert_eq!(edge_indices(&[]), Vec::<u32>::new());
        // The cube's faces do not share vertices (each has its own normals),
        // so each face lists its own 4 sides and its diagonal: 6 * 5.
        let cube = primitive(Shape::Cube);
        assert_eq!(edge_indices(&cube.indices).len() / 2, 30);
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
