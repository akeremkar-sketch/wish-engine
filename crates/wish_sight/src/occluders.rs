//! The walls that block sight, and the questions asked of them (design note
//! §3 and §4).

use std::collections::BTreeMap;
use std::sync::Arc;

use wish_core::Entity;

use crate::geometry::{CIRCLE, P, dot, nudge, pseudo_angle, ray_meets, segments_meet, sub, unit};

/// The most wall segments a world's occluders hold; beyond them the rest is
/// left out (and said so: `sight_occluders_cut_short`).
pub const MAX_SEGMENTS: usize = 65_536;
/// The most wall segments one visible area takes into account (the first by
/// their order); with them a polygon has at most 8 128 points.
pub const MAX_AREA_SEGMENTS: usize = 1000;
/// The longest a visible area reaches, in meters.
pub const MAX_RADIUS: f32 = 1000.0;
/// The side of the lookup grid's cells, in meters.
const CELL: f64 = 4.0;
/// A segment over more grid cells than this is kept on a short list that
/// every question checks.
const MAX_CELLS_PER_SEGMENT: i64 = 4096;
/// A question over more grid cells than this checks every segment.
const MAX_CELLS_PER_QUESTION: i64 = 16_384;

/// One straight piece of wall in the world's X–Y plane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Segment {
    /// One end.
    pub a: [f64; 2],
    /// The other end.
    pub b: [f64; 2],
    /// The entity whose wall it is (a tilemap or an occluder).
    pub owner: Entity,
}

#[derive(Debug, Default)]
struct Data {
    segments: Vec<Segment>,
    grid: BTreeMap<(i32, i32), Vec<u32>>,
    long: Vec<u32>,
    left_out: usize,
}

/// The walls of a world, ready for questions: a world extension (ADR-026),
/// rebuilt every tick by the `sight` system. Cheap to copy.
#[derive(Clone, Debug, Default)]
pub struct Occluders {
    data: Arc<Data>,
}

/// The answer to "can A see B".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sight {
    /// Whether nothing blocks the line.
    pub visible: bool,
    /// When something does: the owner of the wall nearest to A on the line
    /// (ties: the first segment).
    pub blocked_by: Option<Entity>,
    /// How many segments were tested (for fuel).
    pub tested: u32,
}

/// What can be seen from a point within a radius.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VisiblePolygon {
    /// The outline, in order counter-clockwise around the point (world X
    /// and Y).
    pub points: Vec<[f32; 2]>,
    /// How many ray and segment tests it took (for fuel).
    pub tested: u64,
}

fn cell_of(x: f64) -> i32 {
    // Saturating: a wall a billion meters away is in the last cell.
    (x / CELL).floor() as i32
}

/// A range of grid cells: the lowest, the highest, and how many.
type CellRange = ((i32, i32), (i32, i32), i64);

/// The grid cells a box covers, and how many.
fn cells(low: P, high: P) -> Option<CellRange> {
    if !(low[0].is_finite() && low[1].is_finite() && high[0].is_finite() && high[1].is_finite()) {
        return None;
    }
    let (x0, y0, x1, y1) = (
        cell_of(low[0]),
        cell_of(low[1]),
        cell_of(high[0]),
        cell_of(high[1]),
    );
    let count = (i64::from(x1) - i64::from(x0) + 1) * (i64::from(y1) - i64::from(y0) + 1);
    Some(((x0, y0), (x1, y1), count))
}

fn to_f64(point: [f32; 2]) -> P {
    [f64::from(point[0]), f64::from(point[1])]
}

impl Occluders {
    /// Walls from `segments`, in their order (beyond [`MAX_SEGMENTS`] the
    /// rest is left out; a segment with an end that is not a finite number
    /// is left out silently).
    pub fn new(segments: impl IntoIterator<Item = Segment>) -> Self {
        let mut data = Data::default();
        for segment in segments {
            if !(segment.a.iter().chain(&segment.b).all(|v| v.is_finite())) {
                continue;
            }
            if data.segments.len() >= MAX_SEGMENTS {
                data.left_out += 1;
                continue;
            }
            let index = data.segments.len() as u32;
            let low = [
                segment.a[0].min(segment.b[0]),
                segment.a[1].min(segment.b[1]),
            ];
            let high = [
                segment.a[0].max(segment.b[0]),
                segment.a[1].max(segment.b[1]),
            ];
            match cells(low, high) {
                Some(((x0, y0), (x1, y1), count)) if count <= MAX_CELLS_PER_SEGMENT => {
                    for y in y0..=y1 {
                        for x in x0..=x1 {
                            data.grid.entry((x, y)).or_default().push(index);
                        }
                    }
                }
                _ => data.long.push(index),
            }
            data.segments.push(segment);
        }
        Self {
            data: Arc::new(data),
        }
    }

    /// The wall segments, in order.
    pub fn segments(&self) -> &[Segment] {
        &self.data.segments
    }

    /// Whether there is no wall.
    pub fn is_empty(&self) -> bool {
        self.data.segments.is_empty()
    }

    /// How many segments were left out over [`MAX_SEGMENTS`].
    pub fn left_out(&self) -> usize {
        self.data.left_out
    }

    /// The segments whose grid cells meet the box `low`..`high`, by index,
    /// without repeats.
    fn near(&self, low: P, high: P) -> Vec<u32> {
        let data = &self.data;
        let mut found: Vec<u32> = match cells(low, high) {
            Some(((x0, y0), (x1, y1), count)) if count <= MAX_CELLS_PER_QUESTION => {
                let mut found = data.long.clone();
                // The grid holds only cells with walls: walk whichever is
                // smaller, the box's cells or the grid's.
                if count as usize <= data.grid.len() {
                    for y in y0..=y1 {
                        for x in x0..=x1 {
                            if let Some(list) = data.grid.get(&(x, y)) {
                                found.extend_from_slice(list);
                            }
                        }
                    }
                } else {
                    for ((x, y), list) in &data.grid {
                        if (x0..=x1).contains(x) && (y0..=y1).contains(y) {
                            found.extend_from_slice(list);
                        }
                    }
                }
                found
            }
            _ => (0..data.segments.len() as u32).collect(),
        };
        found.sort_unstable();
        found.dedup();
        found
    }

    /// Whether the straight line from `from` to `to` (world X and Y) crosses
    /// no wall. Touching a wall (an end on it, a corner, along an edge)
    /// counts as crossing. The walls of `ignore` do not block.
    pub fn can_see(&self, from: [f32; 2], to: [f32; 2], ignore: [Option<Entity>; 2]) -> Sight {
        let (a, b) = (to_f64(from), to_f64(to));
        if !(a.iter().chain(&b).all(|v| v.is_finite())) {
            return Sight {
                visible: false,
                blocked_by: None,
                tested: 0,
            };
        }
        let low = [a[0].min(b[0]), a[1].min(b[1])];
        let high = [a[0].max(b[0]), a[1].max(b[1])];
        let mut nearest: Option<(f64, Entity)> = None;
        let mut tested = 0u32;
        for index in self.near(low, high) {
            let segment = &self.data.segments[index as usize];
            if ignore.contains(&Some(segment.owner)) {
                continue;
            }
            tested = tested.saturating_add(1);
            if let Some(t) = segments_meet(a, b, segment.a, segment.b)
                && nearest.is_none_or(|(best, _)| t < best)
            {
                nearest = Some((t, segment.owner));
            }
        }
        Sight {
            visible: nearest.is_none(),
            blocked_by: nearest.map(|(_, owner)| owner),
            tested,
        }
    }

    /// What can be seen from `origin` (world X and Y) within `radius`
    /// meters: a polygon around it, cut by walls (design note §4). Empty
    /// for a radius of 0 or a point that is not a finite number; the radius
    /// is at most [`MAX_RADIUS`].
    pub fn visible_polygon(&self, origin: [f32; 2], radius: f32) -> VisiblePolygon {
        let radius = if radius.is_finite() {
            radius.clamp(0.0, MAX_RADIUS)
        } else {
            0.0
        };
        let o = to_f64(origin);
        if radius <= 0.0 || !(o[0].is_finite() && o[1].is_finite()) {
            return VisiblePolygon::default();
        }
        let r = f64::from(radius);
        let mut near = self.near([o[0] - r, o[1] - r], [o[0] + r, o[1] + r]);
        near.truncate(MAX_AREA_SEGMENTS);
        let walls: Vec<(P, P)> = near
            .iter()
            .map(|&i| {
                let segment = &self.data.segments[i as usize];
                (segment.a, segment.b)
            })
            .collect();
        // The directions to look along: the fixed circle, then past every
        // wall end in reach (and just beside it), then where walls cross
        // the circle.
        let mut directions: Vec<P> = CIRCLE.to_vec();
        let r2 = r * r;
        for &(a, b) in &walls {
            for end in [a, b] {
                let v = sub(end, o);
                let d2 = dot(v, v);
                if d2 > 0.0
                    && d2 <= r2
                    && let Some(v) = unit(v)
                {
                    directions.push(v);
                    directions.push(nudge(v, 1.0));
                    directions.push(nudge(v, -1.0));
                }
            }
            // |a + s (b − a) − o|² = r²
            let e = sub(b, a);
            let w = sub(a, o);
            let qa = dot(e, e);
            let qb = 2.0 * dot(e, w);
            let qc = dot(w, w) - r2;
            let disc = qb * qb - 4.0 * qa * qc;
            if qa > 0.0 && disc >= 0.0 {
                let root = disc.sqrt();
                for s in [(-qb - root) / (2.0 * qa), (-qb + root) / (2.0 * qa)] {
                    if (0.0..=1.0).contains(&s)
                        && let Some(v) = unit([w[0] + s * e[0], w[1] + s * e[1]])
                    {
                        directions.push(v);
                    }
                }
            }
        }
        let mut tested = 0u64;
        let local = Local::new(o, r, &walls);
        let mut stamps = vec![0u32; walls.len()];
        let mut rays: Vec<(f64, usize, [f32; 2])> = directions
            .iter()
            .enumerate()
            .map(|(order, &dir)| {
                let stamp = u32::try_from(order + 1).unwrap_or(u32::MAX);
                let (reach, count) = local.reach(o, dir, r, &walls, (&mut stamps, stamp));
                tested += count;
                let point = [
                    (o[0] + dir[0] * reach) as f32,
                    (o[1] + dir[1] * reach) as f32,
                ];
                (pseudo_angle(dir), order, point)
            })
            .collect();
        rays.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));
        let mut points: Vec<[f32; 2]> = Vec::with_capacity(rays.len());
        for (_, _, point) in rays {
            if points.last() != Some(&point) {
                points.push(point);
            }
        }
        while points.len() > 1 && points.first() == points.last() {
            points.pop();
        }
        VisiblePolygon { points, tested }
    }
}

/// How many cells a side of a visible area's own grid has.
const LOCAL_SIDE: usize = 32;

/// The walls of one visible area in a small grid of their own (the circle's
/// box cut into `LOCAL_SIDE` × `LOCAL_SIDE` cells), so that a ray tests only
/// the walls of the cells it passes, nearest cell first, and stops at the
/// first wall. A wall is in every cell its box (widened by a millionth of a
/// cell) meets, so a hit on a cell's edge is found from either side: the
/// answer is the nearest hit of all the walls, as testing every wall
/// would give, at a fraction of the tests.
struct Local {
    low: P,
    cell: f64,
    cells: Vec<Vec<u32>>,
}

impl Local {
    fn new(o: P, r: f64, walls: &[(P, P)]) -> Self {
        let side = LOCAL_SIDE as i64;
        let cell = 2.0 * r / LOCAL_SIDE as f64;
        let low = [o[0] - r, o[1] - r];
        let slack = cell * 1e-6;
        let index = |v: f64, l: f64| (((v - l) / cell).floor() as i64).clamp(0, side - 1);
        let mut cells = vec![Vec::new(); LOCAL_SIDE * LOCAL_SIDE];
        for (i, &(a, b)) in walls.iter().enumerate() {
            let (x0, x1) = (
                index(a[0].min(b[0]) - slack, low[0]),
                index(a[0].max(b[0]) + slack, low[0]),
            );
            let (y0, y1) = (
                index(a[1].min(b[1]) - slack, low[1]),
                index(a[1].max(b[1]) + slack, low[1]),
            );
            for y in y0..=y1 {
                for x in x0..=x1 {
                    cells[(y * side + x) as usize].push(i as u32);
                }
            }
        }
        Self { low, cell, cells }
    }

    /// How far the ray from `o` along the unit `dir` reaches before a wall
    /// (at most `r`), and how many walls it tested. `stamps` marks the walls
    /// this ray (`stamp`) already tested.
    fn reach(
        &self,
        o: P,
        dir: P,
        r: f64,
        walls: &[(P, P)],
        (stamps, stamp): (&mut [u32], u32),
    ) -> (f64, u64) {
        let side = LOCAL_SIDE as i64;
        let start = |axis: usize| {
            (((o[axis] - self.low[axis]) / self.cell).floor() as i64).clamp(0, side - 1)
        };
        let (mut cx, mut cy) = (start(0), start(1));
        // Per axis: the step, the distance to the next cell edge, and the
        // distance between edges.
        let axis = |axis: usize, at: i64| -> (i64, f64, f64) {
            let d = dir[axis];
            if d > 0.0 {
                let edge = self.low[axis] + (at + 1) as f64 * self.cell;
                (1, (edge - o[axis]) / d, self.cell / d)
            } else if d < 0.0 {
                let edge = self.low[axis] + at as f64 * self.cell;
                (-1, (edge - o[axis]) / d, -self.cell / d)
            } else {
                (0, f64::INFINITY, f64::INFINITY)
            }
        };
        let (step_x, mut next_x, delta_x) = axis(0, cx);
        let (step_y, mut next_y, delta_y) = axis(1, cy);
        let mut best = r;
        let mut tested = 0u64;
        loop {
            for &i in &self.cells[(cy * side + cx) as usize] {
                let i = i as usize;
                if stamps[i] == stamp {
                    continue;
                }
                stamps[i] = stamp;
                tested += 1;
                let (a, b) = walls[i];
                if let Some(t) = ray_meets(o, dir, a, b)
                    && t < best
                {
                    best = t;
                }
            }
            let exit = next_x.min(next_y);
            if best < exit || exit >= r {
                break;
            }
            if next_x < next_y {
                cx += step_x;
                next_x += delta_x;
            } else {
                cy += step_y;
                next_y += delta_y;
            }
            if !(0..side).contains(&cx) || !(0..side).contains(&cy) {
                break;
            }
        }
        (best, tested)
    }
}

/// The walls of an outline: each point to the next, and the last back to
/// the first (nothing for fewer than two points).
pub fn outline_segments(points: &[[f64; 2]], owner: Entity) -> Vec<Segment> {
    if points.len() < 2 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(points.len());
    for (i, &a) in points.iter().enumerate() {
        let b = points[(i + 1) % points.len()];
        if a != b {
            out.push(Segment { a, b, owner });
        }
    }
    out
}

/// Whether `point` (world X and Y) is inside the polygon `outline` (even–odd
/// rule), for tests and tools.
pub fn inside(outline: &[[f32; 2]], point: [f32; 2]) -> bool {
    let p = to_f64(point);
    let mut inside = false;
    for (i, a) in outline.iter().enumerate() {
        let (a, b) = (to_f64(*a), to_f64(outline[(i + 1) % outline.len()]));
        if (a[1] > p[1]) != (b[1] > p[1]) {
            let x = a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]);
            if p[0] < x {
                inside = !inside;
            }
        }
    }
    inside
}
