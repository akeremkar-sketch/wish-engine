//! Skeletal animation from glTF (design note M6 §6): skins, animation
//! clips, sampling a clip at a time into node placements, and skinning a
//! primitive's vertices on the CPU.
//!
//! Sampling is plain arithmetic on the file's keyframes (step, linear and
//! cubic spline interpolation; rotations by normalised linear blend of
//! quaternions on the shorter arc, as glTF allows), so the same model and
//! time always give the same pose.

use wish_core::math::{Mat4, Quat, Vec3, Vec4};
use wish_core::{Diagnostic, Result};

use crate::{Model, Placed, Primitive};

/// The most keyframes (all channels together) a model may have.
pub const MAX_KEYFRAMES: usize = 1 << 21;
/// The most joints one skin may have.
pub const MAX_JOINTS: usize = 1024;

/// A skin: the joints (nodes) that bend a mesh, and how each sat when the
/// mesh was bound to it.
#[derive(Clone, Debug, PartialEq)]
pub struct Skin {
    /// Joint nodes, in the order vertex joint indices refer to.
    pub joints: Vec<usize>,
    /// For each joint, the matrix that takes the mesh's space to the
    /// joint's space at bind time.
    pub inverse_bind: Vec<Mat4>,
}

/// What a channel animates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Property {
    /// The node's translation.
    Translation,
    /// The node's rotation.
    Rotation,
    /// The node's scale.
    Scale,
}

/// How values between keyframes are found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interpolation {
    /// The previous keyframe's value until the next.
    Step,
    /// A straight line (rotations: the shorter arc).
    Linear,
    /// A cubic Hermite spline with the file's tangents.
    CubicSpline,
}

/// One animated property of one node.
#[derive(Clone, Debug, PartialEq)]
pub struct Channel {
    /// The node.
    pub node: usize,
    /// Translation, rotation or scale.
    pub property: Property,
    /// How to fill in between keyframes.
    pub interpolation: Interpolation,
    /// Keyframe times in seconds, rising.
    pub times: Vec<f32>,
    /// Values: one per keyframe (three per keyframe for cubic splines:
    /// in-tangent, value, out-tangent). Vectors use xyz; rotations xyzw.
    pub values: Vec<Vec4>,
}

/// An animation clip.
#[derive(Clone, Debug, PartialEq)]
pub struct Animation {
    /// Its name in the file (clips are chosen by name).
    pub name: Option<String>,
    /// Its length in seconds: the last keyframe of any channel.
    pub duration: f32,
    /// Its channels.
    pub channels: Vec<Channel>,
}

fn bad(file: &str, what: &str) -> Diagnostic {
    Diagnostic::error(
        "gltf_invalid",
        format!("The model's animation data is not usable: {what}."),
        "Re-export the model's animations (baked keyframes) from the modelling tool, or check it in a glTF validator.",
    )
    .at_file(file)
}

/// Reads the skins and animations of a document into `model`.
pub(crate) fn read(
    model: &mut Model,
    file: &str,
    document: &gltf::Document,
    buffers: &[gltf::buffer::Data],
) -> Result<()> {
    let data = |buffer: gltf::Buffer<'_>| buffers.get(buffer.index()).map(|d| &d.0[..]);
    for skin in document.skins() {
        let joints: Vec<usize> = skin.joints().map(|node| node.index()).collect();
        if joints.len() > MAX_JOINTS {
            return Err(bad(file, "a skin has more than 1024 joints"));
        }
        let inverse_bind: Vec<Mat4> = match skin.reader(data).read_inverse_bind_matrices() {
            Some(matrices) => matrices.map(|m| Mat4::from_cols_array_2d(&m)).collect(),
            None => vec![Mat4::IDENTITY; joints.len()],
        };
        if inverse_bind.len() != joints.len()
            || inverse_bind.iter().any(|m| !m.is_finite())
            || joints.iter().any(|j| *j >= model.nodes.len())
        {
            return Err(bad(file, "a skin's joints and bind matrices do not match"));
        }
        model.skins.push(Skin {
            joints,
            inverse_bind,
        });
    }
    let mut keyframes = 0usize;
    for animation in document.animations() {
        let mut channels = Vec::new();
        let mut duration = 0.0f32;
        for channel in animation.channels() {
            let target = channel.target();
            let reader = channel.reader(data);
            let Some(times) = reader.read_inputs() else {
                continue;
            };
            let times: Vec<f32> = times.collect();
            let (property, values): (Property, Vec<Vec4>) = match reader.read_outputs() {
                Some(gltf::animation::util::ReadOutputs::Translations(v)) => (
                    Property::Translation,
                    v.map(|t| Vec3::from(t).extend(0.0)).collect(),
                ),
                Some(gltf::animation::util::ReadOutputs::Scales(v)) => (
                    Property::Scale,
                    v.map(|s| Vec3::from(s).extend(0.0)).collect(),
                ),
                Some(gltf::animation::util::ReadOutputs::Rotations(v)) => {
                    (Property::Rotation, v.into_f32().map(Vec4::from).collect())
                }
                Some(gltf::animation::util::ReadOutputs::MorphTargetWeights(_)) => {
                    model.warnings.push(
                        Diagnostic::warning(
                            "unsupported_animation",
                            "The model animates morph target weights, which are not supported; that part does not move.",
                            "Bake the shape change into a skeletal animation, or re-export without morph targets.",
                        )
                        .at_file(file),
                    );
                    continue;
                }
                None => continue,
            };
            let interpolation = match channel.sampler().interpolation() {
                gltf::animation::Interpolation::Step => Interpolation::Step,
                gltf::animation::Interpolation::Linear => Interpolation::Linear,
                gltf::animation::Interpolation::CubicSpline => Interpolation::CubicSpline,
            };
            let per_key = if interpolation == Interpolation::CubicSpline {
                3
            } else {
                1
            };
            let node = target.node().index();
            keyframes = keyframes.saturating_add(times.len());
            if keyframes > MAX_KEYFRAMES {
                return Err(bad(file, "it has more than two million keyframes"));
            }
            if times.is_empty()
                || values.len() != times.len() * per_key
                || node >= model.nodes.len()
                || times.iter().any(|t| !t.is_finite())
                || values.iter().any(|v| !v.is_finite())
                || times.windows(2).any(|w| w[1] < w[0])
            {
                return Err(bad(file, "a channel's times and values do not match"));
            }
            duration = duration.max(times.last().copied().unwrap_or(0.0));
            channels.push(Channel {
                node,
                property,
                interpolation,
                times,
                values,
            });
        }
        model.animations.push(Animation {
            name: animation.name().map(str::to_owned),
            duration,
            channels,
        });
    }
    Ok(())
}

impl Channel {
    /// The channel's value at `time` (clamped to its keyframes).
    fn sample(&self, time: f32) -> Vec4 {
        let last = self.times.len() - 1;
        let value = |key: usize| match self.interpolation {
            Interpolation::CubicSpline => self.values[key * 3 + 1],
            _ => self.values[key],
        };
        if time <= self.times[0] {
            return value(0);
        }
        if time >= self.times[last] {
            return value(last);
        }
        // The keyframe at or before `time`.
        let next = self.times.partition_point(|t| *t <= time).min(last);
        let key = next - 1;
        let (t0, t1) = (self.times[key], self.times[next]);
        let span = (t1 - t0).max(1e-9);
        let s = ((time - t0) / span).clamp(0.0, 1.0);
        match self.interpolation {
            Interpolation::Step => value(key),
            Interpolation::Linear => {
                let (a, b) = (value(key), value(next));
                if self.property == Property::Rotation {
                    // The shorter arc, then renormalised.
                    let b = if a.dot(b) < 0.0 { -b } else { b };
                    a.lerp(b, s).normalize_or_zero()
                } else {
                    a.lerp(b, s)
                }
            }
            Interpolation::CubicSpline => {
                let p0 = self.values[key * 3 + 1];
                let m0 = self.values[key * 3 + 2] * span;
                let m1 = self.values[next * 3] * span;
                let p1 = self.values[next * 3 + 1];
                let (s2, s3) = (s * s, s * s * s);
                let v = p0 * (2.0 * s3 - 3.0 * s2 + 1.0)
                    + m0 * (s3 - 2.0 * s2 + s)
                    + p1 * (-2.0 * s3 + 3.0 * s2)
                    + m1 * (s3 - s2);
                if self.property == Property::Rotation {
                    v.normalize_or_zero()
                } else {
                    v
                }
            }
        }
    }
}

impl Model {
    /// The index of the clip called `name`; the first clip for an empty name.
    pub fn animation_index(&self, name: &str) -> Option<usize> {
        if name.is_empty() {
            return (!self.animations.is_empty()).then_some(0);
        }
        self.animations
            .iter()
            .position(|a| a.name.as_deref() == Some(name))
    }

    /// Every node's matrix in the model's space with clip `animation` at
    /// `time` seconds (`None`: the file's rest pose). Parents before
    /// children; a node outside the scene keeps its local matrix.
    pub fn pose(&self, animation: Option<usize>, time: f32) -> Vec<Mat4> {
        let mut local: Vec<(Vec3, Quat, Vec3)> = self
            .nodes
            .iter()
            .map(|node| (node.translation, node.rotation, node.scale))
            .collect();
        if let Some(clip) = animation.and_then(|index| self.animations.get(index)) {
            for channel in &clip.channels {
                let Some(slot) = local.get_mut(channel.node) else {
                    continue;
                };
                let value = channel.sample(time);
                match channel.property {
                    Property::Translation => slot.0 = value.truncate(),
                    Property::Scale => slot.2 = value.truncate(),
                    Property::Rotation => {
                        let q = Quat::from_vec4(value);
                        slot.1 = if q.length_squared() > 1e-12 {
                            q.normalize()
                        } else {
                            Quat::IDENTITY
                        };
                    }
                }
            }
        }
        let mut global = vec![Mat4::IDENTITY; self.nodes.len()];
        let mut done = vec![false; self.nodes.len()];
        let mut stack: Vec<(usize, Mat4, usize)> = self
            .roots
            .iter()
            .rev()
            .map(|n| (*n, Mat4::IDENTITY, 0))
            .collect();
        while let Some((index, parent, depth)) = stack.pop() {
            if depth > crate::MAX_NODE_DEPTH || index >= self.nodes.len() || done[index] {
                continue;
            }
            let (t, r, s) = local[index];
            let matrix = parent * Mat4::from_scale_rotation_translation(s, r, t);
            global[index] = matrix;
            done[index] = true;
            for child in self.nodes[index].children.iter().rev() {
                stack.push((*child, matrix, depth + 1));
            }
        }
        global
    }

    /// `placed` again, with node matrices from `pose` (see [`Model::pose`]).
    /// A skinned primitive is placed with the identity: its vertices come
    /// out of [`Model::skin`] already in the model's space (glTF ignores the
    /// skinned mesh's own node placement).
    pub fn placed_posed(&self, pose: &[Mat4]) -> Vec<Placed> {
        self.placed()
            .into_iter()
            .map(|placed| {
                let skinned = self.nodes.get(placed.node).and_then(|n| n.skin).is_some();
                Placed {
                    matrix: if skinned {
                        Mat4::IDENTITY
                    } else {
                        pose.get(placed.node).copied().unwrap_or(placed.matrix)
                    },
                    ..placed
                }
            })
            .collect()
    }

    /// The vertices of a skinned primitive bent by `pose`: positions and
    /// normals in the model's space. `None` when the node has no skin or
    /// the primitive has no joint data.
    pub fn skin(&self, placed: &Placed, pose: &[Mat4]) -> Option<(Vec<Vec3>, Vec<Vec3>)> {
        let skin = self.skins.get(self.nodes.get(placed.node)?.skin?)?;
        let primitive: &Primitive = self
            .meshes
            .get(placed.mesh)?
            .primitives
            .get(placed.primitive)?;
        if primitive.joints.len() != primitive.positions.len() {
            return None;
        }
        let palette: Vec<Mat4> = skin
            .joints
            .iter()
            .zip(&skin.inverse_bind)
            .map(|(joint, inverse)| pose.get(*joint).copied().unwrap_or(Mat4::IDENTITY) * *inverse)
            .collect();
        let mut positions = Vec::with_capacity(primitive.positions.len());
        let mut normals = Vec::with_capacity(primitive.positions.len());
        for (index, position) in primitive.positions.iter().enumerate() {
            let joints = primitive.joints[index];
            let weights = primitive
                .weights
                .get(index)
                .copied()
                .unwrap_or([1.0, 0.0, 0.0, 0.0]);
            let total: f32 = weights.iter().sum();
            let mut matrix = Mat4::ZERO;
            for (joint, weight) in joints.iter().zip(weights) {
                if weight > 0.0
                    && let Some(m) = palette.get(usize::from(*joint))
                {
                    matrix += *m * (weight / total.max(1e-6));
                }
            }
            if total <= 1e-6 {
                matrix = Mat4::IDENTITY;
            }
            positions.push(matrix.transform_point3(*position));
            let normal = primitive.normals.get(index).copied().unwrap_or(Vec3::Y);
            normals.push(
                matrix
                    .transform_vector3(normal)
                    .try_normalize()
                    .unwrap_or(Vec3::Y),
            );
        }
        Some((positions, normals))
    }
}
