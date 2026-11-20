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

/// Returns the editable control vertices (in document coordinates) for any `Shape`.
pub fn shape_control_vertices(shape: &Shape) -> Vec<Vector2> {
    match shape {
        Shape::Line(line) => vec![line.start, line.end],
        Shape::Arrow(arrow) => vec![arrow.start, arrow.tip],
        Shape::Rectangle(rect) => {
            let he = rect.cuboid.half_extents;
            vec![
                rect.affine.transform_point2(Vector2::new(-he.x, -he.y)),
                rect.affine.transform_point2(Vector2::new(he.x, -he.y)),
                rect.affine.transform_point2(Vector2::new(he.x, he.y)),
                rect.affine.transform_point2(Vector2::new(-he.x, he.y)),
            ]
        }
        Shape::Ellipse(ellipse) => {
            let rx = ellipse.radii.x;
            let ry = ellipse.radii.y;
            vec![
                ellipse.affine.translation,
                ellipse.affine.transform_point2(Vector2::new(rx, 0.0)),
                ellipse.affine.transform_point2(Vector2::new(-rx, 0.0)),
                ellipse.affine.transform_point2(Vector2::new(0.0, ry)),
                ellipse.affine.transform_point2(Vector2::new(0.0, -ry)),
            ]
        }
        Shape::QuadraticBezier(qb) => vec![qb.start, qb.cp, qb.end],
        Shape::CubicBezier(cb) => vec![cb.start, cb.cp1, cb.cp2, cb.end],
        Shape::Polyline(pl) => {
            let mut verts = Vec::with_capacity(1 + pl.path.len());
            verts.push(pl.start);
            verts.extend_from_slice(&pl.path);
            verts
        }
        Shape::Polygon(pg) => {
            let mut verts = Vec::with_capacity(1 + pg.path.len());
            verts.push(pg.start);
            verts.extend_from_slice(&pg.path);
            verts
        }
    }
}

/// Updates a single control vertex of `shape` to `new_pos` (in document coordinates).
/// When `constrain_aspect` is true (e.g. Shift held), snaps line angles to 45° or keeps uniform aspect ratios.
pub fn set_shape_control_vertex(
    shape: &mut Shape,
    index: usize,
    new_pos: Vector2,
    constrain_aspect: bool,
) {
    let snap_line_endpoint = |anchor: Vector2, target: Vector2| -> Vector2 {
        if !constrain_aspect {
            return target;
        }
        let delta = target - anchor;
        let len = delta.length();
        if len > 1e-4 {
            let angle = delta.y.atan2(delta.x);
            let step = std::f64::consts::FRAC_PI_4;
            let snapped = (angle / step).round() * step;
            anchor + Vector2::new(snapped.cos(), snapped.sin()) * len
        } else {
            target
        }
    };

    match shape {
        Shape::Line(line) => match index {
            0 => line.start = snap_line_endpoint(line.end, new_pos),
            1 => line.end = snap_line_endpoint(line.start, new_pos),
            _ => {}
        },
        Shape::Arrow(arrow) => match index {
            0 => arrow.start = snap_line_endpoint(arrow.tip, new_pos),
            1 => arrow.tip = snap_line_endpoint(arrow.start, new_pos),
            _ => {}
        },
        Shape::Rectangle(rect) => {
            if index >= 4 {
                return;
            }
            let he = rect.cuboid.half_extents;
            let local_corners = [
                Vector2::new(-he.x, -he.y),
                Vector2::new(he.x, -he.y),
                Vector2::new(he.x, he.y),
                Vector2::new(-he.x, he.y),
            ];
            let opp_local = local_corners[(index + 2) % 4];
            let mut local_new = rect.affine.inverse().transform_point2(new_pos);

            if constrain_aspect {
                let delta = local_new - opp_local;
                let side = delta.x.abs().max(delta.y.abs()).max(4.0);
                local_new = opp_local
                    + Vector2::new(
                        if delta.x >= 0.0 { side } else { -side },
                        if delta.y >= 0.0 { side } else { -side },
                    );
            }

            let local_center = (opp_local + local_new) * 0.5;
            let new_he = Vector2::new(
                ((local_new.x - opp_local.x).abs() * 0.5).max(2.0),
                ((local_new.y - opp_local.y).abs() * 0.5).max(2.0),
            );
            rect.affine.translation = rect.affine.transform_point2(local_center);
            rect.cuboid.half_extents = new_he;
        }
        Shape::Ellipse(ellipse) => match index {
            0 => {
                ellipse.affine.translation = new_pos;
            }
            1 | 2 => {
                let local = ellipse.affine.inverse().transform_point2(new_pos);
                let rx = local.x.abs().max(2.0);
                ellipse.radii.x = rx;
                if constrain_aspect {
                    ellipse.radii.y = rx;
                }
            }
            3 | 4 => {
                let local = ellipse.affine.inverse().transform_point2(new_pos);
                let ry = local.y.abs().max(2.0);
                ellipse.radii.y = ry;
                if constrain_aspect {
                    ellipse.radii.x = ry;
                }
            }
            _ => {}
        },
        Shape::QuadraticBezier(qb) => match index {
            0 => qb.start = new_pos,
            1 => qb.cp = new_pos,
            2 => qb.end = new_pos,
            _ => {}
        },
        Shape::CubicBezier(cb) => match index {
            0 => cb.start = new_pos,
            1 => cb.cp1 = new_pos,
            2 => cb.cp2 = new_pos,
            3 => cb.end = new_pos,
            _ => {}
        },
        Shape::Polyline(pl) => {
            if index == 0 {
                pl.start = new_pos;
            } else if let Some(p) = pl.path.get_mut(index - 1) {
                *p = new_pos;
            }
        }
        Shape::Polygon(pg) => {
            if index == 0 {
                pg.start = new_pos;
            } else if let Some(p) = pg.path.get_mut(index - 1) {
                *p = new_pos;
            }
        }
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

    #[test]
    fn test_shape_control_vertices_and_editing() {
        // 1. Line vertex editing
        let mut line_shape = Shape::Line(Line {
            start: Vector2::new(0.0, 0.0),
            end: Vector2::new(100.0, 0.0),
        });
        assert_eq!(shape_control_vertices(&line_shape).len(), 2);
        set_shape_control_vertex(&mut line_shape, 1, Vector2::new(100.0, 50.0), false);
        if let Shape::Line(l) = &line_shape {
            assert!((l.end.x - 100.0).abs() < 1e-6);
            assert!((l.end.y - 50.0).abs() < 1e-6);
        } else {
            panic!("Expected Line");
        }

        // 2. Rectangle corner editing
        let mut rect_shape = Shape::Rectangle(Rectangle::from_corners(
            Vector2::new(0.0, 0.0),
            Vector2::new(100.0, 100.0),
        ));
        assert_eq!(shape_control_vertices(&rect_shape).len(), 4);
        // Drag bottom-right corner (index 2) to (200, 150) while top-left (0, 0) stays anchored
        set_shape_control_vertex(&mut rect_shape, 2, Vector2::new(200.0, 150.0), false);
        let verts = shape_control_vertices(&rect_shape);
        assert!((verts[0].x - 0.0).abs() < 1e-6 && (verts[0].y - 0.0).abs() < 1e-6);
        assert!((verts[2].x - 200.0).abs() < 1e-6 && (verts[2].y - 150.0).abs() < 1e-6);

        // 3. Ellipse center & radius editing
        let mut ellipse_shape = Shape::Ellipse(Ellipse {
            radii: Vector2::new(40.0, 40.0),
            affine: DAffine2::from_translation(Vector2::new(50.0, 50.0)),
        });
        assert_eq!(shape_control_vertices(&ellipse_shape).len(), 5);
        set_shape_control_vertex(&mut ellipse_shape, 1, Vector2::new(120.0, 50.0), true);
        if let Shape::Ellipse(e) = &ellipse_shape {
            assert!((e.radii.x - 70.0).abs() < 1e-6);
            assert!((e.radii.y - 70.0).abs() < 1e-6);
        } else {
            panic!("Expected Ellipse");
        }

        // 4. Polygon (Triangle) vertex editing
        let mut poly_shape = Shape::Polygon(Polygon {
            start: Vector2::new(0.0, 0.0),
            path: vec![Vector2::new(50.0, 100.0), Vector2::new(100.0, 0.0)],
        });
        assert_eq!(shape_control_vertices(&poly_shape).len(), 3);
        set_shape_control_vertex(&mut poly_shape, 1, Vector2::new(60.0, 140.0), false);
        let pverts = shape_control_vertices(&poly_shape);
        assert!((pverts[1].x - 60.0).abs() < 1e-6 && (pverts[1].y - 140.0).abs() < 1e-6);
    }

    #[test]
    fn test_brush_hold_to_shape_continuous_pull() {
        use crate::Engine;
        use crate::strokes::Stroke;
        use rnote_compose::penevent::PenEvent;
        use rnote_compose::penpath::Element;
        use std::collections::HashSet;
        use std::time::{Duration, Instant};

        let mut engine = Engine::default();
        let t0 = Instant::now();

        // Draw a straight-ish line across 8 points
        for i in 0..8 {
            let pos = Vector2::new(10.0 + i as f64 * 20.0, 50.0);
            let _ = engine.handle_pen_event(
                PenEvent::Down {
                    element: Element::new(pos, 0.5),
                    modifier_keys: HashSet::new(),
                },
                None,
                t0 + Duration::from_millis(i * 15),
            );
        }

        // Hold still for >= 420ms to trigger hold-to-shape snap
        let _ = engine.handle_pen_event(
            PenEvent::Down {
                element: Element::new(Vector2::new(150.0, 50.0), 0.5),
                modifier_keys: HashSet::new(),
            },
            None,
            t0 + Duration::from_millis(600),
        );

        // Verify it snapped to a ShapeStroke::Line
        let key = engine.store.stroke_keys_as_rendered()[0];
        assert!(matches!(
            engine.store.get_stroke_ref(key),
            Some(Stroke::ShapeStroke(_))
        ));

        // Now keep pen down and pull the endpoint to (250.0, 120.0)
        let _ = engine.handle_pen_event(
            PenEvent::Down {
                element: Element::new(Vector2::new(250.0, 120.0), 0.5),
                modifier_keys: HashSet::new(),
            },
            None,
            t0 + Duration::from_millis(650),
        );

        if let Some(Stroke::ShapeStroke(ss)) = engine.store.get_stroke_ref(key) {
            if let Shape::Line(line) = &ss.shape {
                assert!((line.end.x - 250.0).abs() < 1e-4);
                assert!((line.end.y - 120.0).abs() < 1e-4);
            } else {
                panic!("Expected Shape::Line");
            }
        } else {
            panic!("Expected ShapeStroke");
        }

        // Lift pen
        let _ = engine.handle_pen_event(
            PenEvent::Up {
                element: Element::new(Vector2::new(250.0, 120.0), 0.5),
                modifier_keys: HashSet::new(),
            },
            None,
            t0 + Duration::from_millis(670),
        );
    }
}
