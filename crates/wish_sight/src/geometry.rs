//! Plane geometry that gives the same bits on every machine (design note
//! §4): `f64` with `+`, `-`, `*`, `/` and `sqrt` only (IEEE 754 defines each
//! to the bit, and Rust never fuses them), no trigonometry at run time.

/// How many fixed directions go around a circle.
pub const CIRCLE_STEPS: usize = 128;

/// The sine and cosine of `x` (radians, within ±π) by a Taylor series of
/// plain arithmetic, for tables the compiler evaluates.
const fn sin_cos(x: f64) -> (f64, f64) {
    let mut sin = 0.0;
    let mut cos = 0.0;
    // term = x^n / n!
    let mut term = 1.0;
    let mut n = 0;
    while n < 40 {
        match n % 4 {
            0 => cos += term,
            1 => sin += term,
            2 => cos -= term,
            _ => sin -= term,
        }
        n += 1;
        term = term * x / n as f64;
    }
    (sin, cos)
}

/// `CIRCLE_STEPS` unit directions, counter-clockwise from +X.
pub const CIRCLE: [[f64; 2]; CIRCLE_STEPS] = {
    let mut table = [[0.0; 2]; CIRCLE_STEPS];
    let mut i = 0;
    while i < CIRCLE_STEPS {
        let mut angle = i as f64 * (2.0 * std::f64::consts::PI / CIRCLE_STEPS as f64);
        if angle > std::f64::consts::PI {
            angle -= 2.0 * std::f64::consts::PI;
        }
        let (sin, cos) = sin_cos(angle);
        table[i] = [cos, sin];
        i += 1;
    }
    table
};

/// cos and sin of the small turn that passes a ray just beside a corner
/// (0.0001 rad).
const NUDGE: (f64, f64) = sin_cos(0.0001);

/// A 2D point or vector.
pub type P = [f64; 2];

/// `a − b`.
pub fn sub(a: P, b: P) -> P {
    [a[0] - b[0], a[1] - b[1]]
}

/// The z of the cross product of `a` and `b`.
pub fn cross(a: P, b: P) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

/// The dot product.
pub fn dot(a: P, b: P) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}

/// Which side of the line `p → q` the point `r` is on: positive to the
/// left, negative to the right, zero on it.
pub fn orient(p: P, q: P, r: P) -> f64 {
    cross(sub(q, p), sub(r, p))
}

/// `v` scaled to length 1; `None` for a zero (or not finite) vector.
pub fn unit(v: P) -> Option<P> {
    let length = dot(v, v).sqrt();
    (length > 0.0 && length.is_finite()).then(|| [v[0] / length, v[1] / length])
}

/// `v` turned by the nudge, counter-clockwise (`sign` 1) or clockwise (-1).
pub fn nudge(v: P, sign: f64) -> P {
    let (sin, cos) = NUDGE;
    let s = sin * sign;
    [cos * v[0] - s * v[1], s * v[0] + cos * v[1]]
}

/// A number that grows with the angle of `v` counter-clockwise from +X, from
/// 0 to 4 (the "diamond angle": division only, no trigonometry). `v` must
/// not be zero.
pub fn pseudo_angle(v: P) -> f64 {
    let [x, y] = v;
    if y >= 0.0 {
        if x >= 0.0 {
            y / (x + y)
        } else {
            1.0 - x / (-x + y)
        }
    } else if x < 0.0 {
        2.0 - y / (-x - y)
    } else {
        3.0 + x / (x - y)
    }
}

/// Whether `r`, known to be on the line through `p` and `q`, lies within
/// the segment's box.
fn within(p: P, q: P, r: P) -> bool {
    r[0] >= p[0].min(q[0])
        && r[0] <= p[0].max(q[0])
        && r[1] >= p[1].min(q[1])
        && r[1] <= p[1].max(q[1])
}

/// Where the closed segment `a → b` meets the closed segment `c → d`, as the
/// share of the way from `a` to `b` (0 to 1); `None` when they do not meet.
/// Touching counts: an end on the other segment, or overlapping along one
/// line.
pub fn segments_meet(a: P, b: P, c: P, d: P) -> Option<f64> {
    let d1 = orient(c, d, a);
    let d2 = orient(c, d, b);
    let d3 = orient(a, b, c);
    let d4 = orient(a, b, d);
    let crosses = |u: f64, v: f64| (u > 0.0 && v < 0.0) || (u < 0.0 && v > 0.0);
    if crosses(d1, d2) && crosses(d3, d4) {
        return Some(d1 / (d1 - d2));
    }
    let along = |r: P| {
        let ab = sub(b, a);
        let length = dot(ab, ab);
        if length > 0.0 {
            (dot(sub(r, a), ab) / length).clamp(0.0, 1.0)
        } else {
            0.0
        }
    };
    let mut nearest: Option<f64> = None;
    let mut keep = |t: f64| {
        nearest = Some(nearest.map_or(t, |n| n.min(t)));
    };
    if d1 == 0.0 && within(c, d, a) {
        keep(0.0);
    }
    if d2 == 0.0 && within(c, d, b) {
        keep(1.0);
    }
    if d3 == 0.0 && within(a, b, c) {
        keep(along(c));
    }
    if d4 == 0.0 && within(a, b, d) {
        keep(along(d));
    }
    nearest
}

/// How far along the ray from `origin` in the unit direction `dir` it meets
/// the segment `c → d` (meters, at least 0); `None` when it does not.
/// A segment along the ray is not met (its neighbours are).
pub fn ray_meets(origin: P, dir: P, c: P, d: P) -> Option<f64> {
    let e = sub(d, c);
    let denom = cross(dir, e);
    if denom == 0.0 {
        return None;
    }
    let w = sub(c, origin);
    let t = cross(w, e) / denom;
    let s = cross(w, dir) / denom;
    (t >= 0.0 && (0.0..=1.0).contains(&s)).then_some(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_circle_table_is_unit_and_ordered() {
        let mut last = -1.0;
        for [x, y] in CIRCLE {
            assert!((x * x + y * y - 1.0).abs() < 1e-12);
            let angle = pseudo_angle([x, y]);
            assert!(angle > last, "counter-clockwise");
            last = angle;
        }
        assert_eq!(CIRCLE[0], [1.0, 0.0]);
        assert!((CIRCLE[32][0]).abs() < 1e-15 && (CIRCLE[32][1] - 1.0).abs() < 1e-15);
    }

    #[test]
    fn segments_meet_when_they_cross_or_touch() {
        let t = segments_meet([0.0, 0.0], [2.0, 0.0], [1.0, -1.0], [1.0, 1.0]);
        assert_eq!(t, Some(0.5));
        // Touching at an end.
        assert_eq!(
            segments_meet([0.0, 0.0], [1.0, 0.0], [1.0, 0.0], [1.0, 5.0]),
            Some(1.0)
        );
        // Along the same line, overlapping.
        assert_eq!(
            segments_meet([0.0, 0.0], [4.0, 0.0], [1.0, 0.0], [2.0, 0.0]),
            Some(0.25)
        );
        // Apart.
        assert_eq!(
            segments_meet([0.0, 0.0], [1.0, 0.0], [2.0, -1.0], [2.0, 1.0]),
            None
        );
        assert_eq!(
            segments_meet([0.0, 0.0], [4.0, 0.0], [5.0, 0.0], [6.0, 0.0]),
            None
        );
    }

    #[test]
    fn a_ray_meets_what_is_in_front_of_it() {
        assert_eq!(
            ray_meets([0.0, 0.0], [1.0, 0.0], [3.0, -1.0], [3.0, 1.0]),
            Some(3.0)
        );
        assert_eq!(
            ray_meets([0.0, 0.0], [-1.0, 0.0], [3.0, -1.0], [3.0, 1.0]),
            None
        );
        assert_eq!(
            ray_meets([0.0, 0.0], [1.0, 0.0], [1.0, 0.0], [2.0, 0.0]),
            None
        );
    }
}
