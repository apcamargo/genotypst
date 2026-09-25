use serde::Serialize;
use std::f64::consts::{PI, TAU};
use std::ops::{Add, AddAssign, Div, Mul, Sub, SubAssign};

pub(crate) const EPSILON_3: f64 = 1e-3;
pub(crate) const EPSILON_7: f64 = 1e-7;
pub(crate) const GEOM_EPS: f64 = 1e-9;

/// Compares two floats using exact equality, an absolute tolerance, or a ULP
/// tolerance for equally signed IEEE-754 values.
#[must_use]
pub(crate) fn approx_eq(left: f64, right: f64, epsilon: f64, ulps: u64) -> bool {
    left == right
        || (left - right).abs() <= epsilon
        || (left.is_sign_positive() == right.is_sign_positive()
            && left.to_bits().abs_diff(right.to_bits()) <= ulps)
}

#[must_use]
pub(crate) fn normalize_angle(angle: f64) -> f64 {
    angle.rem_euclid(TAU)
}

#[must_use]
pub(crate) fn normalize_angle_diff(diff: f64) -> f64 {
    (diff + PI).rem_euclid(TAU) - PI
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub(crate) struct Vec2 {
    pub(crate) x: f64,
    pub(crate) y: f64,
}

impl Vec2 {
    #[must_use]
    pub(crate) const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    #[must_use]
    pub(crate) const fn zero() -> Self {
        Self { x: 0.0, y: 0.0 }
    }

    #[must_use]
    pub(crate) fn length(self) -> f64 {
        self.length_squared().sqrt()
    }

    #[must_use]
    pub(crate) fn length_squared(self) -> f64 {
        self.x * self.x + self.y * self.y
    }

    #[must_use]
    pub(crate) fn distance(self, other: Self) -> f64 {
        (other - self).length()
    }

    #[must_use]
    pub(crate) fn dot(self, other: Self) -> f64 {
        self.x * other.x + self.y * other.y
    }

    #[must_use]
    pub(crate) fn cross(self, other: Self) -> f64 {
        self.x * other.y - self.y * other.x
    }

    #[must_use]
    pub(crate) fn normalized(self) -> Option<Self> {
        let len = self.length();
        if len < GEOM_EPS {
            None
        } else {
            Some(self / len)
        }
    }

    #[must_use]
    pub(crate) fn perp_left(self) -> Self {
        Self::new(-self.y, self.x)
    }

    #[must_use]
    pub(crate) fn perp_right(self) -> Self {
        Self::new(self.y, -self.x)
    }

    #[must_use]
    pub(crate) fn rotate(self, angle_rad: f64) -> Self {
        let (sin, cos) = angle_rad.sin_cos();
        Self::new(self.x * cos - self.y * sin, self.x * sin + self.y * cos)
    }

    #[must_use]
    pub(crate) fn angle(self) -> f64 {
        self.y.atan2(self.x)
    }

    #[must_use]
    pub(crate) fn angle_between(self, other: Self) -> f64 {
        self.signed_angle_between(other).abs()
    }

    #[must_use]
    pub(crate) fn signed_angle_between(self, other: Self) -> f64 {
        normalize_angle_diff(other.angle() - self.angle())
    }
}

impl Add for Vec2 {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self::new(self.x + other.x, self.y + other.y)
    }
}

impl Sub for Vec2 {
    type Output = Self;
    fn sub(self, other: Self) -> Self {
        Self::new(self.x - other.x, self.y - other.y)
    }
}

impl Mul<f64> for Vec2 {
    type Output = Self;
    fn mul(self, scalar: f64) -> Self {
        Self::new(self.x * scalar, self.y * scalar)
    }
}

impl Div<f64> for Vec2 {
    type Output = Self;
    fn div(self, scalar: f64) -> Self {
        Self::new(self.x / scalar, self.y / scalar)
    }
}

impl AddAssign for Vec2 {
    fn add_assign(&mut self, other: Self) {
        self.x += other.x;
        self.y += other.y;
    }
}

impl SubAssign for Vec2 {
    fn sub_assign(&mut self, other: Self) {
        self.x -= other.x;
        self.y -= other.y;
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Circle {
    pub(crate) center: Vec2,
    pub(crate) radius: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Aabb {
    pub(crate) min: Vec2,
    pub(crate) max: Vec2,
}

impl Aabb {
    #[must_use]
    pub(crate) const fn empty() -> Self {
        Self {
            min: Vec2::new(f64::INFINITY, f64::INFINITY),
            max: Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY),
        }
    }

    pub(crate) fn include(&mut self, point: Vec2) {
        self.min.x = self.min.x.min(point.x);
        self.min.y = self.min.y.min(point.y);
        self.max.x = self.max.x.max(point.x);
        self.max.y = self.max.y.max(point.y);
    }

    #[must_use]
    pub(crate) fn overlaps_with_gap(self, other: Self, gap: f64) -> bool {
        self.max.x >= other.min.x - gap
            && other.max.x >= self.min.x - gap
            && self.max.y >= other.min.y - gap
            && other.max.y >= self.min.y - gap
    }
}

#[must_use]
pub(crate) fn circle_from_three_points(a: Vec2, b: Vec2, c: Vec2) -> Option<Circle> {
    let d1 = b - a;
    let d2 = c - b;
    let cross = d1.cross(d2);
    if cross.abs() < GEOM_EPS {
        return None;
    }
    let m1 = (a + b) * 0.5;
    let m2 = (b + c) * 0.5;
    let p2 = d2.perp_left();
    let t = (m2 - m1).cross(p2) / cross;
    let center = m1 + d1.perp_left() * t;
    let radius = center.distance(a);
    Some(Circle { center, radius })
}

#[must_use]
pub(crate) fn circle_circle_intersections(c1: Circle, c2: Circle) -> Option<(Vec2, Option<Vec2>)> {
    let d = c1.center.distance(c2.center);
    if d > c1.radius + c2.radius || d < (c1.radius - c2.radius).abs() || d < GEOM_EPS {
        return None;
    }
    let a = (c1.radius * c1.radius - c2.radius * c2.radius + d * d) / (2.0 * d);
    let h_sq = c1.radius * c1.radius - a * a;
    let h = if h_sq < 0.0 { 0.0 } else { h_sq.sqrt() };

    let p_mid = c1.center + (c2.center - c1.center) * (a / d);
    if h.abs() < GEOM_EPS {
        Some((p_mid, None))
    } else {
        let perp = (c2.center - c1.center).perp_left() * (h / d);
        Some((p_mid + perp, Some(p_mid - perp)))
    }
}

#[must_use]
pub(crate) fn circle_line_intersections(
    circle: Circle,
    line_start: Vec2,
    line_end: Vec2,
) -> Vec<Vec2> {
    let line_delta = line_end - line_start;
    let center_delta = line_start - circle.center;
    let quadratic_a = line_delta.dot(line_delta);
    let quadratic_b = 2.0 * center_delta.dot(line_delta);
    let quadratic_c = center_delta.dot(center_delta) - circle.radius * circle.radius;

    if quadratic_a.abs() < GEOM_EPS {
        if quadratic_b.abs() < GEOM_EPS {
            Vec::new()
        } else {
            vec![line_start + line_delta * (-quadratic_c / quadratic_b)]
        }
    } else {
        let discriminant = quadratic_b * quadratic_b - 4.0 * quadratic_a * quadratic_c;
        if discriminant < -GEOM_EPS {
            Vec::new()
        } else if discriminant.abs() < GEOM_EPS {
            vec![line_start + line_delta * (-quadratic_b / (2.0 * quadratic_a))]
        } else {
            let sqrt_discriminant = discriminant.sqrt();
            let denominator = 2.0 * quadratic_a;
            vec![
                line_start + line_delta * ((-quadratic_b - sqrt_discriminant) / denominator),
                line_start + line_delta * ((-quadratic_b + sqrt_discriminant) / denominator),
            ]
        }
    }
}

#[must_use]
pub(crate) fn is_point_on_segment(
    point: Vec2,
    segment_start: Vec2,
    segment_end: Vec2,
    epsilon: f64,
) -> bool {
    let segment_delta = segment_end - segment_start;
    let length_squared = segment_delta.length_squared();
    if length_squared < GEOM_EPS {
        return point.distance(segment_start) < epsilon;
    }
    let interpolation = (point - segment_start).dot(segment_delta) / length_squared;
    if interpolation < -epsilon || interpolation > 1.0 + epsilon {
        return false;
    }
    let projection = segment_start + segment_delta * interpolation.clamp(0.0, 1.0);
    point.distance(projection) < epsilon
}

#[must_use]
pub(crate) fn segments_intersect(a0: Vec2, a1: Vec2, b0: Vec2, b1: Vec2) -> bool {
    // Broad-phase AABB check.
    let min_a = Vec2::new(a0.x.min(a1.x), a0.y.min(a1.y));
    let max_a = Vec2::new(a0.x.max(a1.x), a0.y.max(a1.y));
    let min_b = Vec2::new(b0.x.min(b1.x), b0.y.min(b1.y));
    let max_b = Vec2::new(b0.x.max(b1.x), b0.y.max(b1.y));

    if min_a.x - EPSILON_7 > max_b.x
        || max_a.x + EPSILON_7 < min_b.x
        || min_a.y - EPSILON_7 > max_b.y
        || max_a.y + EPSILON_7 < min_b.y
    {
        return false;
    }

    let d1 = a1 - a0;
    let d2 = b1 - b0;
    let denom = d1.cross(d2);

    if denom.abs() < EPSILON_7 {
        // Parallel or collinear.
        if (b0 - a0).cross(d1).abs() >= EPSILON_7 {
            return false; // parallel but not collinear
        }
        // Collinear. Check for overlap.
        let len_sq1 = d1.length_squared();
        if len_sq1 < GEOM_EPS {
            return is_point_on_segment(a0, b0, b1, EPSILON_7);
        }
        let t0 = (b0 - a0).dot(d1) / len_sq1;
        let t1 = (b1 - a0).dot(d1) / len_sq1;
        let min_t = t0.min(t1);
        let max_t = t0.max(t1);
        max_t >= -EPSILON_7 && min_t <= 1.0 + EPSILON_7
    } else {
        let t = (b0 - a0).cross(d2) / denom;
        let u = (b0 - a0).cross(d1) / denom;
        (-EPSILON_7..=1.0 + EPSILON_7).contains(&t) && (-EPSILON_7..=1.0 + EPSILON_7).contains(&u)
    }
}

#[must_use]
pub(crate) fn point_is_right_of_line(a: Vec2, b: Vec2, p: Vec2) -> bool {
    (b - a).cross(p - a) < 0.0
}

#[cfg(test)]
mod tests {
    use super::{
        Aabb, Circle, EPSILON_7, GEOM_EPS, Vec2, approx_eq, circle_circle_intersections,
        circle_from_three_points, circle_line_intersections, is_point_on_segment, normalize_angle,
        normalize_angle_diff, point_is_right_of_line, segments_intersect,
    };
    use std::f64::consts::{FRAC_PI_2, PI, TAU};

    const fn aabb(min: Vec2, max: Vec2) -> Aabb {
        Aabb { min, max }
    }

    fn assert_points_match(actual: &[Vec2], expected: &[Vec2]) {
        assert_eq!(actual.len(), expected.len());
        let mut unmatched = expected.to_vec();
        for &point in actual {
            let Some(index) = unmatched
                .iter()
                .position(|&candidate| point.distance(candidate) < 1e-9)
            else {
                panic!("unexpected point {point:?}; expected {expected:?}");
            };
            unmatched.swap_remove(index);
        }
    }

    #[test]
    fn aabb_accumulates_points() {
        let mut bounds = Aabb::empty();

        bounds.include(Vec2::new(3.0, -2.0));
        bounds.include(Vec2::new(-1.0, 4.0));

        assert_eq!(bounds, aabb(Vec2::new(-1.0, -2.0), Vec2::new(3.0, 4.0)));
    }

    #[test]
    fn aabb_overlap_respects_total_inclusive_gap() {
        let origin = aabb(Vec2::zero(), Vec2::new(1.0, 1.0));
        let overlapping = aabb(Vec2::new(0.5, 0.5), Vec2::new(1.5, 1.5));
        let horizontally_separated = aabb(Vec2::new(3.0, 0.0), Vec2::new(4.0, 1.0));
        let vertically_separated = aabb(Vec2::new(0.0, -2.0), Vec2::new(1.0, -1.0));
        let beyond_gap = aabb(Vec2::new(3.000_001, 0.0), Vec2::new(4.0, 1.0));

        assert!(origin.overlaps_with_gap(overlapping, 0.0));
        assert!(origin.overlaps_with_gap(horizontally_separated, 2.0));
        assert!(horizontally_separated.overlaps_with_gap(origin, 2.0));
        assert!(origin.overlaps_with_gap(vertically_separated, 1.0));
        assert!(!origin.overlaps_with_gap(beyond_gap, 2.0));
        assert!(!origin.overlaps_with_gap(horizontally_separated, 0.0));
    }

    #[test]
    fn approximate_equality_respects_absolute_and_ulp_margins() {
        assert!(approx_eq(0.0, -0.0, 0.0, 0));
        assert!(approx_eq(0.0, 1e-10, 1e-9, 0));
        assert!(!approx_eq(0.0, 1e-8, 1e-9, 0));

        let value = 1.0_f64;
        let two_ulps_away = f64::from_bits(value.to_bits() + 2);
        let three_ulps_away = f64::from_bits(value.to_bits() + 3);
        assert!(approx_eq(value, two_ulps_away, 0.0, 2));
        assert!(!approx_eq(value, three_ulps_away, 0.0, 2));
        assert!(!approx_eq(-1e-300, 1e-300, 0.0, u64::MAX));
    }

    #[test]
    fn circle_circle_intersections_cover_all_root_counts() {
        let unit = Circle {
            center: Vec2::zero(),
            radius: 1.0,
        };
        assert!(
            circle_circle_intersections(
                unit,
                Circle {
                    center: Vec2::new(3.0, 0.0),
                    radius: 1.0,
                }
            )
            .is_none()
        );

        let tangent = circle_circle_intersections(
            unit,
            Circle {
                center: Vec2::new(2.0, 0.0),
                radius: 1.0,
            },
        )
        .unwrap();
        assert_eq!(tangent, (Vec2::new(1.0, 0.0), None));

        let two = circle_circle_intersections(
            unit,
            Circle {
                center: Vec2::new(1.0, 0.0),
                radius: 1.0,
            },
        )
        .unwrap();
        let second = two.1.expect("intersecting circles must have two roots");
        let height = 3.0_f64.sqrt() / 2.0;
        assert_points_match(
            &[two.0, second],
            &[Vec2::new(0.5, height), Vec2::new(0.5, -height)],
        );
    }

    #[test]
    fn perpendiculars_have_opposite_handedness() {
        // `arcs.rs` picks a winding with these, so swapping them silently
        // reverses every fitted arc.
        let east = Vec2::new(1.0, 0.0);
        assert_eq!(east.perp_left(), Vec2::new(0.0, 1.0));
        assert_eq!(east.perp_right(), Vec2::new(0.0, -1.0));
        assert!(east.cross(east.perp_left()) > 0.0);
        assert!(east.cross(east.perp_right()) < 0.0);
    }

    #[test]
    fn rotation_composes_additively_and_preserves_length() {
        let start = Vec2::new(2.0, -1.0);
        let once = start.rotate(0.4).rotate(0.9);
        let twice = start.rotate(1.3);
        assert!(once.distance(twice) < 1e-12);
        assert!((start.length() - once.length()).abs() < 1e-12);
        // A quarter turn is the left perpendicular, which pins the direction.
        assert!(start.rotate(FRAC_PI_2).distance(start.perp_left()) < 1e-12);
    }

    #[test]
    fn angles_normalize_into_their_documented_ranges() {
        for angle in [-3.0 * TAU, -0.5, 0.0, 0.5, TAU, 3.0 * TAU + 1.0] {
            let normalized = normalize_angle(angle);
            assert!(
                (0.0..TAU).contains(&normalized),
                "{angle} normalized to {normalized}"
            );
            assert!(
                (normalized - angle)
                    .rem_euclid(TAU)
                    .min((angle - normalized).rem_euclid(TAU))
                    < 1e-9
            );
        }

        for diff in [-3.0 * PI, -PI, -0.25, 0.0, 0.25, PI, 3.0 * PI] {
            let normalized = normalize_angle_diff(diff);
            assert!(
                (-PI..=PI).contains(&normalized),
                "{diff} normalized to {normalized}"
            );
        }
        // Sign convention: a small negative difference stays negative.
        assert!(normalize_angle_diff(-0.25) < 0.0);
        assert!(normalize_angle_diff(TAU - 0.25) < 0.0);
    }

    #[test]
    fn signed_angle_between_carries_a_sign_and_wraps_the_short_way() {
        let east = Vec2::new(1.0, 0.0);
        let north = Vec2::new(0.0, 1.0);
        assert!((east.signed_angle_between(north) - FRAC_PI_2).abs() < 1e-12);
        assert!((north.signed_angle_between(east) + FRAC_PI_2).abs() < 1e-12);
        assert!((east.angle_between(north) - FRAC_PI_2).abs() < 1e-12);
        assert!((north.angle_between(east) - FRAC_PI_2).abs() < 1e-12);

        // Nearly opposite vectors must take the short way round, not 2pi minus it.
        let almost_west = Vec2::new(-1.0, 0.05);
        assert!(east.signed_angle_between(almost_west).abs() < PI);
    }

    #[test]
    fn normalization_rejects_vectors_below_the_geometric_epsilon() {
        assert!(Vec2::zero().normalized().is_none());
        assert!(Vec2::new(GEOM_EPS / 2.0, 0.0).normalized().is_none());
        let unit = Vec2::new(3.0, 4.0)
            .normalized()
            .expect("a length-5 vector must normalize");
        assert!((unit.length() - 1.0).abs() < 1e-12);
        assert!(unit.distance(Vec2::new(0.6, 0.8)) < 1e-12);
    }

    #[test]
    fn three_points_recover_their_circle_and_reject_collinear_input() {
        let circle = circle_from_three_points(
            Vec2::new(1.0, 0.0),
            Vec2::new(0.0, 1.0),
            Vec2::new(-1.0, 0.0),
        )
        .expect("three points on the unit circle must fit");
        assert!(circle.center.distance(Vec2::zero()) < 1e-12);
        assert!((circle.radius - 1.0).abs() < 1e-12);

        // An offset circle, so the test cannot pass by returning the origin.
        let offset = circle_from_three_points(
            Vec2::new(12.0, 5.0),
            Vec2::new(10.0, 7.0),
            Vec2::new(8.0, 5.0),
        )
        .expect("an offset circle must fit");
        assert!(offset.center.distance(Vec2::new(10.0, 5.0)) < 1e-9);
        assert!((offset.radius - 2.0).abs() < 1e-9);

        assert!(
            circle_from_three_points(
                Vec2::new(0.0, 0.0),
                Vec2::new(1.0, 1.0),
                Vec2::new(2.0, 2.0)
            )
            .is_none()
        );
    }

    #[test]
    fn segment_intersection_covers_crossing_touching_and_collinear_cases() {
        let cases = [
            // Proper crossing.
            ((0.0, 0.0), (2.0, 2.0), (0.0, 2.0), (2.0, 0.0), true),
            // Shared endpoint.
            ((0.0, 0.0), (1.0, 0.0), (1.0, 0.0), (1.0, 1.0), true),
            // T-touch: an endpoint landing in the other segment's interior.
            ((0.0, 0.0), (2.0, 0.0), (1.0, 0.0), (1.0, 1.0), true),
            // Collinear and overlapping.
            ((0.0, 0.0), (2.0, 0.0), (1.0, 0.0), (3.0, 0.0), true),
            // Collinear but disjoint.
            ((0.0, 0.0), (1.0, 0.0), (2.0, 0.0), (3.0, 0.0), false),
            // Parallel, never collinear.
            ((0.0, 0.0), (2.0, 0.0), (0.0, 1.0), (2.0, 1.0), false),
            // Non-parallel but the crossing lies beyond both segments.
            ((0.0, 0.0), (1.0, 0.0), (3.0, 1.0), (3.0, 5.0), false),
            // Degenerate zero-length segment on and off the other segment.
            ((1.0, 0.0), (1.0, 0.0), (0.0, 0.0), (2.0, 0.0), true),
            ((1.0, 5.0), (1.0, 5.0), (0.0, 0.0), (2.0, 0.0), false),
        ];

        for (a0, a1, b0, b1, expected) in cases {
            let a0 = Vec2::new(a0.0, a0.1);
            let a1 = Vec2::new(a1.0, a1.1);
            let b0 = Vec2::new(b0.0, b0.1);
            let b1 = Vec2::new(b1.0, b1.1);
            assert_eq!(
                segments_intersect(a0, a1, b0, b1),
                expected,
                "{a0:?}-{a1:?} vs {b0:?}-{b1:?}"
            );
            // The predicate is used on unordered pairs, so it must be symmetric.
            assert_eq!(
                segments_intersect(b0, b1, a0, a1),
                expected,
                "asymmetric verdict for {a0:?}-{a1:?} vs {b0:?}-{b1:?}"
            );
        }
    }

    #[test]
    fn point_on_segment_respects_its_epsilon_at_both_ends() {
        let start = Vec2::new(0.0, 0.0);
        let end = Vec2::new(4.0, 0.0);
        let epsilon = 1e-6;

        assert!(is_point_on_segment(
            Vec2::new(2.0, 0.0),
            start,
            end,
            epsilon
        ));
        assert!(is_point_on_segment(start, start, end, epsilon));
        assert!(is_point_on_segment(end, start, end, epsilon));
        // Perpendicular offset just inside and just outside the tolerance.
        assert!(is_point_on_segment(
            Vec2::new(2.0, epsilon / 2.0),
            start,
            end,
            epsilon
        ));
        assert!(!is_point_on_segment(
            Vec2::new(2.0, epsilon * 10.0),
            start,
            end,
            epsilon
        ));
        // Beyond either end along the line.
        assert!(!is_point_on_segment(
            Vec2::new(-1.0, 0.0),
            start,
            end,
            epsilon
        ));
        assert!(!is_point_on_segment(
            Vec2::new(5.0, 0.0),
            start,
            end,
            epsilon
        ));

        // A degenerate segment collapses to a proximity test on its point.
        assert!(is_point_on_segment(start, start, start, epsilon));
        assert!(!is_point_on_segment(
            Vec2::new(1.0, 0.0),
            start,
            start,
            epsilon
        ));
    }

    #[test]
    fn right_of_line_distinguishes_both_sides_and_collinear_points() {
        let a = Vec2::zero();
        let b = Vec2::new(1.0, 0.0);
        // Travelling east, a point to the south is on the right.
        assert!(point_is_right_of_line(a, b, Vec2::new(0.5, -1.0)));
        assert!(!point_is_right_of_line(a, b, Vec2::new(0.5, 1.0)));
        // Collinear is not "right of", which keeps the arc winding stable.
        assert!(!point_is_right_of_line(a, b, Vec2::new(0.5, 0.0)));
        // Reversing the line reverses the verdict.
        assert!(point_is_right_of_line(b, a, Vec2::new(0.5, 1.0)));
    }

    #[test]
    fn segment_intersection_tolerance_matches_its_epsilon() {
        // The broad phase rejects beyond EPSILON_7, so a gap either side of it
        // must flip the verdict. This pins the constant to observable behavior.
        let a0 = Vec2::zero();
        let a1 = Vec2::new(1.0, 0.0);
        let near = Vec2::new(1.0 + EPSILON_7 / 10.0, 0.0);
        let far = Vec2::new(1.0 + EPSILON_7 * 100.0, 0.0);
        assert!(segments_intersect(a0, a1, near, Vec2::new(2.0, 1.0)));
        assert!(!segments_intersect(a0, a1, far, Vec2::new(2.0, 1.0)));
    }

    #[test]
    fn circle_line_intersections_cover_all_root_counts() {
        let unit = Circle {
            center: Vec2::zero(),
            radius: 1.0,
        };

        assert!(
            circle_line_intersections(unit, Vec2::new(-2.0, 2.0), Vec2::new(2.0, 2.0)).is_empty()
        );
        assert_points_match(
            &circle_line_intersections(unit, Vec2::new(-2.0, 1.0), Vec2::new(2.0, 1.0)),
            &[Vec2::new(0.0, 1.0)],
        );
        assert_points_match(
            &circle_line_intersections(unit, Vec2::new(-2.0, 0.0), Vec2::new(2.0, 0.0)),
            &[Vec2::new(-1.0, 0.0), Vec2::new(1.0, 0.0)],
        );
    }
}
