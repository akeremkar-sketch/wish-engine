//! Small models made in code, for tests and examples: no download, and the
//! expected results can be worked out by hand.

/// A `.gltf` (JSON with its data inline) of a bar 0.2 m wide and 2 m tall
/// standing on the origin, bent by two joints: `root` at the origin and
/// `elbow` at y = 1. The bar's lower vertices follow `root`, the upper ones
/// `elbow`. The clip `bend` turns `elbow` 90° about +Z over one second
/// (linear); the clip `still` holds the rest pose for half a second.
pub fn bending_bar() -> String {
    let mut bin: Vec<u8> = Vec::new();
    let mut views: Vec<(usize, usize)> = Vec::new();
    let mut push = |bytes: Vec<u8>, bin: &mut Vec<u8>| {
        while !bin.len().is_multiple_of(4) {
            bin.push(0);
        }
        let offset = bin.len();
        bin.extend_from_slice(&bytes);
        views.push((offset, bytes.len()));
        views.len() - 1
    };
    let floats = |values: &[f32]| {
        values
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<u8>>()
    };
    // Six vertices: three rows (y = 0, 1, 2) of two.
    let positions = floats(&[
        -0.1, 0.0, 0.0, 0.1, 0.0, 0.0, -0.1, 1.0, 0.0, 0.1, 1.0, 0.0, -0.1, 2.0, 0.0, 0.1, 2.0, 0.0,
    ]);
    let normals = floats(&[0.0, 0.0, 1.0].repeat(6));
    let joints: Vec<u8> = [
        [0u16, 0, 0, 0],
        [0, 0, 0, 0],
        [0, 1, 0, 0],
        [0, 1, 0, 0],
        [1, 0, 0, 0],
        [1, 0, 0, 0],
    ]
    .iter()
    .flatten()
    .flat_map(|j| j.to_le_bytes())
    .collect();
    let weights = floats(&[
        1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.5, 0.5, 0.0, 0.0, 0.5, 0.5, 0.0, 0.0, 1.0, 0.0,
        0.0, 0.0, 1.0, 0.0, 0.0, 0.0,
    ]);
    let indices: Vec<u8> = [0u16, 1, 3, 0, 3, 2, 2, 3, 5, 2, 5, 4]
        .iter()
        .flat_map(|i| i.to_le_bytes())
        .collect();
    // Inverse bind: root at the origin (identity), elbow at y = 1.
    let inverse_bind = floats(&[
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, //
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, -1.0, 0.0, 1.0,
    ]);
    let times = floats(&[0.0, 1.0]);
    let half = std::f32::consts::FRAC_1_SQRT_2;
    let rotations = floats(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, half, half]);
    let still_times = floats(&[0.0, 0.5]);
    let still_rotations = floats(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]);
    let v = [
        push(positions, &mut bin),
        push(normals, &mut bin),
        push(joints, &mut bin),
        push(weights, &mut bin),
        push(indices, &mut bin),
        push(inverse_bind, &mut bin),
        push(times, &mut bin),
        push(rotations, &mut bin),
        push(still_times, &mut bin),
        push(still_rotations, &mut bin),
    ];
    let views_json: Vec<String> = views
        .iter()
        .map(|(offset, length)| {
            format!(r#"{{"buffer":0,"byteOffset":{offset},"byteLength":{length}}}"#)
        })
        .collect();
    let data = base64(&bin);
    format!(
        r#"{{
  "asset": {{ "version": "2.0" }},
  "scene": 0,
  "scenes": [{{ "nodes": [0, 2] }}],
  "nodes": [
    {{ "name": "root", "children": [1] }},
    {{ "name": "elbow", "translation": [0, 1, 0] }},
    {{ "name": "bar", "mesh": 0, "skin": 0 }}
  ],
  "skins": [{{ "joints": [0, 1], "inverseBindMatrices": 5 }}],
  "meshes": [{{ "name": "bar", "primitives": [{{
    "attributes": {{ "POSITION": 0, "NORMAL": 1, "JOINTS_0": 2, "WEIGHTS_0": 3 }},
    "indices": 4 }}] }}],
  "animations": [
    {{ "name": "bend",
      "samplers": [{{ "input": 6, "output": 7, "interpolation": "LINEAR" }}],
      "channels": [{{ "sampler": 0, "target": {{ "node": 1, "path": "rotation" }} }}] }},
    {{ "name": "still",
      "samplers": [{{ "input": 8, "output": 9, "interpolation": "LINEAR" }}],
      "channels": [{{ "sampler": 0, "target": {{ "node": 1, "path": "rotation" }} }}] }}
  ],
  "accessors": [
    {{ "bufferView": {p}, "componentType": 5126, "count": 6, "type": "VEC3", "min": [-0.1, 0, 0], "max": [0.1, 2, 0] }},
    {{ "bufferView": {n}, "componentType": 5126, "count": 6, "type": "VEC3" }},
    {{ "bufferView": {j}, "componentType": 5123, "count": 6, "type": "VEC4" }},
    {{ "bufferView": {w}, "componentType": 5126, "count": 6, "type": "VEC4" }},
    {{ "bufferView": {i}, "componentType": 5123, "count": 12, "type": "SCALAR" }},
    {{ "bufferView": {ib}, "componentType": 5126, "count": 2, "type": "MAT4" }},
    {{ "bufferView": {t}, "componentType": 5126, "count": 2, "type": "SCALAR", "min": [0], "max": [1] }},
    {{ "bufferView": {r}, "componentType": 5126, "count": 2, "type": "VEC4" }},
    {{ "bufferView": {st}, "componentType": 5126, "count": 2, "type": "SCALAR", "min": [0], "max": [0.5] }},
    {{ "bufferView": {sr}, "componentType": 5126, "count": 2, "type": "VEC4" }}
  ],
  "bufferViews": [{views}],
  "buffers": [{{ "byteLength": {len}, "uri": "data:application/octet-stream;base64,{data}" }}]
}}"#,
        p = v[0],
        n = v[1],
        j = v[2],
        w = v[3],
        i = v[4],
        ib = v[5],
        t = v[6],
        r = v[7],
        st = v[8],
        sr = v[9],
        views = views_json.join(","),
        len = bin.len(),
    )
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for (index, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            if index <= chunk.len() {
                out.push(char::from(TABLE[((n >> shift) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}
