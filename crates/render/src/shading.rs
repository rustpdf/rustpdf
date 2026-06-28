//! Axial (type 2) and radial (type 3) shadings → tiny-skia gradient shaders.
//!
//! Other shading types (function-based type 1, mesh types 4–7) are not yet
//! supported and return `None` (the area is left unpainted) — see PENDING.md.

use crate::color::ColorSpace;
use crate::func::Function;
use cos::{Dict, Object};
use parser::PdfReader;
use tiny_skia::{
    Color, GradientStop, LinearGradient, Point, RadialGradient, Shader, SpreadMode, Transform,
};

const STOPS: usize = 32;

/// Build a gradient shader for a shading object, transformed into device space.
pub fn shader_for(
    reader: &PdfReader,
    obj: &Object,
    resources: &Dict,
    total: Transform,
) -> Option<Shader<'static>> {
    let dict = match reader.resolve(obj) {
        Object::Dict(d) => d.clone(),
        Object::Stream(s) => s.dict.clone(),
        _ => return None,
    };
    let stype = int_of(&dict, "ShadingType")?;
    let cs = match dict.get("ColorSpace") {
        Some(o) => ColorSpace::parse(reader, o, resources),
        None => ColorSpace::DeviceRGB,
    };
    let func = dict.get("Function").map(|f| Function::parse(reader, f));
    let domain = floats(&dict, "Domain").unwrap_or_else(|| vec![0.0, 1.0]);
    let (t0, t1) = (
        domain.first().copied().unwrap_or(0.0),
        domain.get(1).copied().unwrap_or(1.0),
    );
    let _extend = bools(&dict, "Extend");
    // PDF's Extend pads beyond the axis; tiny-skia's Pad is the closest match
    // in both the extended and non-extended cases.
    let mode = SpreadMode::Pad;

    let stops = build_stops(&cs, func.as_ref(), t0, t1);
    let coords = floats(&dict, "Coords")?;

    match stype {
        2 => {
            // Axial: [x0 y0 x1 y1]
            let p0 = Point::from_xy(*coords.first()?, *coords.get(1)?);
            let p1 = Point::from_xy(*coords.get(2)?, *coords.get(3)?);
            LinearGradient::new(p0, p1, stops, mode, total)
        }
        3 => {
            // Radial: [x0 y0 r0 x1 y1 r1]. tiny-skia models a focal gradient;
            // use the end circle as the main circle and the start as focal.
            let start = Point::from_xy(*coords.first()?, *coords.get(1)?);
            let end = Point::from_xy(*coords.get(3)?, *coords.get(4)?);
            let r1 = *coords.get(5)?;
            RadialGradient::new(start, end, r1.max(0.001), stops, mode, total)
        }
        _ => None,
    }
}

fn build_stops(cs: &ColorSpace, func: Option<&Function>, t0: f32, t1: f32) -> Vec<GradientStop> {
    let mut stops = Vec::with_capacity(STOPS);
    for i in 0..STOPS {
        let frac = i as f32 / (STOPS as f32 - 1.0);
        let t = t0 + frac * (t1 - t0);
        let comps = match func {
            Some(f) => f.eval(&[t]),
            None => vec![t],
        };
        let rgb = cs.to_rgb(&comps);
        let color = Color::from_rgba(
            rgb[0].clamp(0.0, 1.0),
            rgb[1].clamp(0.0, 1.0),
            rgb[2].clamp(0.0, 1.0),
            1.0,
        )
        .unwrap_or(Color::BLACK);
        stops.push(GradientStop::new(frac, color));
    }
    stops
}

fn int_of(d: &Dict, key: &str) -> Option<i64> {
    match d.get(key) {
        Some(Object::Integer(n)) => Some(*n),
        Some(Object::Real(r)) => Some(*r as i64),
        _ => None,
    }
}

fn floats(d: &Dict, key: &str) -> Option<Vec<f32>> {
    match d.get(key) {
        Some(Object::Array(a)) => Some(
            a.iter()
                .filter_map(|o| match o {
                    Object::Integer(n) => Some(*n as f32),
                    Object::Real(r) => Some(*r as f32),
                    _ => None,
                })
                .collect(),
        ),
        _ => None,
    }
}

fn bools(d: &Dict, key: &str) -> [bool; 2] {
    match d.get(key) {
        Some(Object::Array(a)) => {
            let g = |i: usize| matches!(a.get(i), Some(Object::Bool(true)));
            [g(0), g(1)]
        }
        _ => [false, false],
    }
}
