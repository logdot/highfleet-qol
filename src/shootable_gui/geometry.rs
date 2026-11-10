#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[repr(C)]
pub(super) struct Point {
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Projection {
    pub centre: Point,
    pub camera: Point,
    pub shake: Point,
    pub zoom: f32,
}

impl Projection {
    pub fn unproject(self, screen: Point) -> Option<Point> {
        if !self.zoom.is_finite() || self.zoom <= 0.0 {
            return None;
        }
        let point = Point {
            x: (screen.x - self.centre.x) / self.zoom + self.camera.x + self.shake.x,
            y: (screen.y - self.centre.y) / self.zoom + self.camera.y + self.shake.y,
        };
        (point.x.is_finite() && point.y.is_finite()).then_some(point)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Collider {
    pub origin: Point,
    pub vertices: Vec<Point>,
}

impl Collider {
    pub fn from_screen(projection: Projection, origin: Point, vertices: &[Point]) -> Option<Self> {
        if !(3..=8).contains(&vertices.len()) {
            return None;
        }
        let world_origin = projection.unproject(origin)?;
        let vertices: Vec<Point> = vertices
            .iter()
            .map(|point| Point {
                // Box2D uses metres; game world coordinates use 20 units per metre.
                x: (point.x - origin.x) / projection.zoom * 0.05,
                y: (point.y - origin.y) / projection.zoom * 0.05,
            })
            .collect();
        if vertices
            .iter()
            .any(|point| !point.x.is_finite() || !point.y.is_finite())
        {
            return None;
        }
        let mut distinct: Vec<Point> = Vec::new();
        for &point in &vertices {
            if distinct.iter().all(|other| {
                (point.x - other.x).powi(2) + (point.y - other.y).powi(2) >= 0.0025_f32.powi(2)
            }) {
                distinct.push(point);
            }
        }
        if distinct.len() < 3 {
            return None;
        }
        let a = distinct[0];
        let b = distinct[1];
        if !distinct[2..]
            .iter()
            .any(|c| ((b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)).abs() > 0.000001)
        {
            return None;
        }
        Some(Self {
            origin: world_origin,
            vertices,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(actual: Point, expected: Point) {
        let tolerance = 0.005;
        assert!(
            (actual.x - expected.x).abs() < tolerance && (actual.y - expected.y).abs() < tolerance,
            "{actual:?} != {expected:?}"
        );
    }

    #[test]
    fn unprojection_reverses_the_battle_camera_transform() {
        for zoom in [0.3, 0.7, 1.0, 7.0, 14.0] {
            for offset in [-1000.0, 0.0, 1234.0] {
                let projection = Projection {
                    centre: Point { x: 960.0, y: 540.0 },
                    camera: Point {
                        x: offset,
                        y: -offset,
                    },
                    shake: Point { x: 5.0, y: -3.0 },
                    zoom,
                };
                for world in [
                    Point::default(),
                    Point {
                        x: -200.0,
                        y: 123.0,
                    },
                    Point {
                        x: 9000.0,
                        y: -4000.0,
                    },
                ] {
                    let screen = Point {
                        x: (world.x - projection.camera.x - projection.shake.x) * zoom
                            + projection.centre.x,
                        y: (world.y - projection.camera.y - projection.shake.y) * zoom
                            + projection.centre.y,
                    };
                    close(projection.unproject(screen).unwrap(), world);
                }
            }
        }
    }

    #[test]
    fn invalid_camera_states_do_not_produce_physics_coordinates() {
        let valid = Projection {
            centre: Point { x: 960.0, y: 540.0 },
            camera: Point::default(),
            shake: Point::default(),
            zoom: 1.0,
        };
        for zoom in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert!(Projection { zoom, ..valid }
                .unproject(Point::default())
                .is_none());
        }
        for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert!(valid.unproject(Point { x: invalid, y: 0.0 }).is_none());
            assert!(Projection {
                camera: Point { x: 0.0, y: invalid },
                ..valid
            }
            .unproject(Point::default())
            .is_none());
            assert!(Projection {
                shake: Point { x: invalid, y: 0.0 },
                ..valid
            }
            .unproject(Point::default())
            .is_none());
            assert!(Projection {
                centre: Point { x: 0.0, y: invalid },
                ..valid
            }
            .unproject(Point::default())
            .is_none());
        }
    }

    #[test]
    fn collider_geometry_keeps_its_pixel_footprint_at_each_zoom() {
        let origin = Point { x: 800.0, y: 450.0 };
        let pixels = [
            Point { x: 780.0, y: 440.0 },
            Point { x: 820.0, y: 440.0 },
            Point { x: 820.0, y: 460.0 },
            Point { x: 780.0, y: 460.0 },
        ];
        for zoom in [0.3, 0.7, 1.0, 7.0, 14.0] {
            let projection = Projection {
                centre: Point { x: 960.0, y: 540.0 },
                camera: Point {
                    x: 20000.0,
                    y: -50000.0,
                },
                shake: Point { x: 2.0, y: 7.0 },
                zoom,
            };
            let collider = Collider::from_screen(projection, origin, &pixels).unwrap();
            close(collider.origin, projection.unproject(origin).unwrap());
            for (local, pixel) in collider.vertices.iter().zip(pixels) {
                close(
                    Point {
                        x: local.x * zoom / 0.05 + origin.x,
                        y: local.y * zoom / 0.05 + origin.y,
                    },
                    pixel,
                );
            }
        }
    }

    #[test]
    fn unusable_meshes_are_rejected_before_calling_box2d() {
        let projection = Projection {
            centre: Point::default(),
            camera: Point::default(),
            shake: Point::default(),
            zoom: 1.0,
        };
        for count in [0, 1, 2, 3, 8, 9] {
            let line = vec![Point::default(); count];
            assert!(Collider::from_screen(projection, Point::default(), &line).is_none());
        }
        let triangle = [
            Point { x: 0.0, y: 0.0 },
            Point { x: 20.0, y: 0.0 },
            Point { x: 0.0, y: 20.0 },
        ];
        for invalid in [f32::NAN, f32::INFINITY] {
            let mut vertices = triangle;
            vertices[1].x = invalid;
            assert!(Collider::from_screen(projection, Point::default(), &vertices).is_none());
        }
        // Polygon::Set welds nearby vertices with half of Box2D's 0.005m linear slop.
        let tiny = triangle.map(|point| Point {
            x: point.x * 0.0001,
            y: point.y * 0.0001,
        });
        assert!(Collider::from_screen(projection, Point::default(), &tiny).is_none());
    }
}
