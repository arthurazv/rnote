// Shape recognizer for "Hold to Shape"
use p2d::bounding_volume::Aabb;
use p2d::glamx::DAffine2;
use p2d::math::Vector2;
use rnote_compose::shapes::{Ellipse, Line, Polygon, Rectangle, Shape};

/// Attempts to recognize a geometric shape from a sequence of raw drawn points.
pub fn recognize_shape(points: &[Vector2]) -> Option<Shape> {
    if points.len() < 6 {
        return None;
    }

    let aabb = Aabb::from_points(points.iter().copied());
    let width = aabb.extents().x;
    let height = aabb.extents().y;
    let diag = (width * width + height * height).sqrt();

    // Ignore tiny strokes
    if diag < 20.0 {
        return None;
    }

    let mut path_len = 0.0;
    for w in points.windows(2) {
        path_len += (w[1] - w[0]).length();
    }
    if path_len < 1e-4 {
        return None;
    }

    let start = points[0];
    let end = *points.last().unwrap();
    let d_close = (end - start).length();

    let is_closed = d_close <= 0.35 * diag || d_close <= 0.28 * path_len;

    if is_closed {
        // Run Ramer-Douglas-Peucker simplification to detect polygons (triangles, rectangles)
        let simplified = ramer_douglas_peucker(points, 0.065 * diag);

        // 1. Triangle: simplifies to 3 vertices (4 points including start/end closure)
        if simplified.len() == 4 || (simplified.len() == 3 && d_close <= 0.15 * diag) {
            let v0 = simplified[0];
            let v1 = simplified[1];
            let v2 = simplified[2];
            return Some(Shape::Polygon(Polygon {
                start: v0,
                path: vec![v1, v2],
            }));
        }

        // 2. Rectangle / Quadrilateral: simplifies to 4 vertices (5 points including closure)
        if simplified.len() == 5 {
            // Check if angles are approximately 90 degrees
            let v0 = simplified[0];
            let v1 = simplified[1];
            let v2 = simplified[2];
            let v3 = simplified[3];

            let e0 = (v1 - v0).normalize();
            let e1 = (v2 - v1).normalize();
            let e2 = (v3 - v2).normalize();
            let e3 = (v0 - v3).normalize();

            // Dot products of adjacent edges should be close to 0 for ~90 degrees
            let dot01 = e0.dot(e1).abs();
            let dot12 = e1.dot(e2).abs();
            let dot23 = e2.dot(e3).abs();
            let dot30 = e3.dot(e0).abs();

            if dot01 <= 0.35 && dot12 <= 0.35 && dot23 <= 0.35 && dot30 <= 0.35 {
                return Some(Shape::Rectangle(Rectangle::from_corners(
                    aabb.mins, aabb.maxs,
                )));
            }
        }

        // 3. Circle / Ellipse
        let center = (aabb.mins + aabb.maxs) * 0.5;
        let mut sum_dist = 0.0;
        for &p in points {
            sum_dist += (p - center).length();
        }
        let mean_radius = sum_dist / points.len() as f64;

        let mut sum_var = 0.0;
        for &p in points {
            let diff = (p - center).length() - mean_radius;
            sum_var += diff * diff;
        }
        let std_dev = (sum_var / points.len() as f64).sqrt();
        let cv = std_dev / mean_radius.max(1e-4);

        if cv <= 0.14 {
            let aspect_diff = (width - height).abs() / width.max(height).max(1e-4);
            if aspect_diff <= 0.25 {
                // Perfect Circle
                let r = (width + height) * 0.25;
                return Some(Shape::Ellipse(Ellipse {
                    radii: Vector2::splat(r),
                    affine: DAffine2::from_translation(center),
                }));
            } else {
                // Ellipse
                return Some(Shape::Ellipse(Ellipse {
                    radii: Vector2::new(width * 0.5, height * 0.5),
                    affine: DAffine2::from_translation(center),
                }));
            }
        }
    } else {
        // Open Path -> Try Straight Line
        let d_direct = (end - start).length();
        if d_direct >= 25.0 && d_direct / path_len >= 0.88 {
            let mut max_dev = 0.0;
            for &p in points {
                let dev = point_to_segment_dist(p, start, end);
                if dev > max_dev {
                    max_dev = dev;
                }
            }

            if max_dev <= 0.12 * d_direct {
                let mut snapped_end = end;
                let delta = end - start;

                // Snap to horizontal (within ~6 degrees)
                if (delta.y / d_direct).abs() <= 0.10 {
                    snapped_end.y = start.y;
                }
                // Snap to vertical (within ~6 degrees)
                else if (delta.x / d_direct).abs() <= 0.10 {
                    snapped_end.x = start.x;
                }
                // Snap to 45 degrees
                else if ((delta.x.abs() - delta.y.abs()) / d_direct).abs() <= 0.10 {
                    let sign_x = if delta.x >= 0.0 { 1.0 } else { -1.0 };
                    let sign_y = if delta.y >= 0.0 { 1.0 } else { -1.0 };
                    let diag_len = (delta.x.abs() + delta.y.abs()) * 0.5;
                    snapped_end = start + Vector2::new(sign_x * diag_len, sign_y * diag_len);
                }

                return Some(Shape::Line(Line {
                    start,
                    end: snapped_end,
                }));
            }
        }
    }

    None
}

fn point_to_segment_dist(p: Vector2, a: Vector2, b: Vector2) -> f64 {
    let ab = b - a;
    let len_sq = ab.length_squared();
    if len_sq < 1e-6 {
        return (p - a).length();
    }
    let t = ((p - a).dot(ab) / len_sq).clamp(0.0, 1.0);
    let proj = a + ab * t;
    (p - proj).length()
}

fn ramer_douglas_peucker(points: &[Vector2], epsilon: f64) -> Vec<Vector2> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let first = points[0];
    let last = points[points.len() - 1];

    let mut max_dist = 0.0;
    let mut index = 0;

    for (i, &p) in points.iter().enumerate().skip(1).take(points.len() - 2) {
        let dist = point_to_segment_dist(p, first, last);
        if dist > max_dist {
            max_dist = dist;
            index = i;
        }
    }

    if max_dist > epsilon {
        let mut left = ramer_douglas_peucker(&points[..=index], epsilon);
        let right = ramer_douglas_peucker(&points[index..], epsilon);
        left.pop();
        left.extend(right);
        left
    } else {
        vec![first, last]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_recognize_line() {
        let mut points = Vec::new();
        for i in 0..20 {
            // Horizontal line with slight noise
            points.push(Vector2::new(i as f64 * 10.0, 100.0 + ((i % 2) as f64 * 0.5)));
        }
        let recognized = recognize_shape(&points);
        assert!(matches!(recognized, Some(Shape::Line(_))));
    }

    #[test]
    fn test_recognize_circle() {
        let mut points = Vec::new();
        let center = Vector2::new(100.0, 100.0);
        let r = 50.0;
        for i in 0..30 {
            let angle = (i as f64 / 30.0) * std::f64::consts::TAU;
            points.push(center + Vector2::new(r * angle.cos(), r * angle.sin()));
        }
        let recognized = recognize_shape(&points);
        assert!(matches!(recognized, Some(Shape::Ellipse(_))));
    }

    #[test]
    fn test_recognize_rectangle() {
        let mut points = Vec::new();
        // Top edge
        for x in (0..=100).step_by(10) {
            points.push(Vector2::new(x as f64, 0.0));
        }
        // Right edge
        for y in (0..=100).step_by(10) {
            points.push(Vector2::new(100.0, y as f64));
        }
        // Bottom edge
        for x in (0..=100).rev().step_by(10) {
            points.push(Vector2::new(x as f64, 100.0));
        }
        // Left edge
        for y in (0..=100).rev().step_by(10) {
            points.push(Vector2::new(0.0, y as f64));
        }
        let recognized = recognize_shape(&points);
        assert!(matches!(recognized, Some(Shape::Rectangle(_))));
    }

    #[test]
    fn test_recognize_triangle() {
        let mut points = Vec::new();
        // Side 1: (0, 100) -> (50, 0)
        for i in 0..10 {
            let t = i as f64 / 10.0;
            points.push(Vector2::new(50.0 * t, 100.0 * (1.0 - t)));
        }
        // Side 2: (50, 0) -> (100, 100)
        for i in 0..10 {
            let t = i as f64 / 10.0;
            points.push(Vector2::new(50.0 + 50.0 * t, 100.0 * t));
        }
        // Side 3: (100, 100) -> (0, 100)
        for i in 0..10 {
            let t = i as f64 / 10.0;
            points.push(Vector2::new(100.0 * (1.0 - t), 100.0));
        }
        let recognized = recognize_shape(&points);
        assert!(matches!(recognized, Some(Shape::Polygon(_))));
    }
}
