//! Color spaces (§8.6) and conversion of color components to RGB.

use crate::func::Function;
use cos::Object;
use parser::PdfReader;
use std::rc::Rc;

#[derive(Clone)]
pub enum ColorSpace {
    DeviceGray,
    DeviceRGB,
    DeviceCMYK,
    /// Indexed palette into a base space.
    Indexed {
        base: Box<ColorSpace>,
        hival: usize,
        lookup: Vec<u8>,
    },
    /// Separation or DeviceN: `n` tint components mapped through `func` into
    /// the alternate space.
    Separation {
        alt: Box<ColorSpace>,
        func: Rc<Function>,
        n: usize,
    },
    /// Pattern color space (handled specially by `scn`/`SCN`).
    Pattern,
}

impl ColorSpace {
    /// Number of color components an `sc`/`scn` for this space expects.
    pub fn components(&self) -> usize {
        match self {
            ColorSpace::DeviceGray => 1,
            ColorSpace::DeviceRGB => 3,
            ColorSpace::DeviceCMYK => 4,
            ColorSpace::Indexed { .. } => 1,
            ColorSpace::Separation { n, .. } => *n,
            ColorSpace::Pattern => 1,
        }
    }

    /// The opaque default color for this space (black-ish), used when an
    /// operator switches space without setting a color.
    pub fn default_rgb(&self) -> [f32; 3] {
        match self {
            ColorSpace::DeviceCMYK => self.to_rgb(&[0.0, 0.0, 0.0, 1.0]),
            ColorSpace::Indexed { .. } => self.to_rgb(&[0.0]),
            ColorSpace::Separation { n, .. } => self.to_rgb(&vec![1.0; *n]),
            _ => [0.0, 0.0, 0.0],
        }
    }

    /// Convert color components to straight RGB in `0..=1`.
    pub fn to_rgb(&self, comps: &[f32]) -> [f32; 3] {
        match self {
            ColorSpace::DeviceGray => {
                let g = comps.first().copied().unwrap_or(0.0);
                [g, g, g]
            }
            ColorSpace::DeviceRGB => [
                comps.first().copied().unwrap_or(0.0),
                comps.get(1).copied().unwrap_or(0.0),
                comps.get(2).copied().unwrap_or(0.0),
            ],
            ColorSpace::DeviceCMYK => {
                let c = comps.first().copied().unwrap_or(0.0);
                let m = comps.get(1).copied().unwrap_or(0.0);
                let y = comps.get(2).copied().unwrap_or(0.0);
                let k = comps.get(3).copied().unwrap_or(0.0);
                [
                    (1.0 - c) * (1.0 - k),
                    (1.0 - m) * (1.0 - k),
                    (1.0 - y) * (1.0 - k),
                ]
            }
            ColorSpace::Indexed {
                base,
                hival,
                lookup,
            } => {
                let n = base.components();
                let idx = (comps.first().copied().unwrap_or(0.0).round() as usize).min(*hival);
                let off = idx * n;
                let slice: Vec<f32> = (0..n)
                    .map(|i| lookup.get(off + i).copied().unwrap_or(0) as f32 / 255.0)
                    .collect();
                base.to_rgb(&slice)
            }
            ColorSpace::Separation { alt, func, .. } => {
                let out = func.eval(comps);
                alt.to_rgb(&out)
            }
            ColorSpace::Pattern => [0.0, 0.0, 0.0],
        }
    }

    /// Resolve a color space from a name or array form.
    pub fn parse(reader: &PdfReader, obj: &Object, resources: &cos::Dict) -> ColorSpace {
        let obj = reader.resolve(obj);
        match obj {
            Object::Name(n) => ColorSpace::from_name(reader, n.as_str(), resources),
            Object::Array(arr) if !arr.is_empty() => ColorSpace::from_array(reader, arr, resources),
            _ => ColorSpace::DeviceGray,
        }
    }

    fn from_name(reader: &PdfReader, name: &str, resources: &cos::Dict) -> ColorSpace {
        match name {
            "DeviceGray" | "G" | "CalGray" => ColorSpace::DeviceGray,
            "DeviceRGB" | "RGB" | "CalRGB" | "Lab" => ColorSpace::DeviceRGB,
            "DeviceCMYK" | "CMYK" => ColorSpace::DeviceCMYK,
            "Pattern" => ColorSpace::Pattern,
            other => {
                // Named space in the resource dictionary's /ColorSpace.
                if let Some(cs) = lookup_resource_cs(reader, resources, other) {
                    return ColorSpace::parse(reader, &cs, resources);
                }
                ColorSpace::DeviceGray
            }
        }
    }

    fn from_array(reader: &PdfReader, arr: &[Object], resources: &cos::Dict) -> ColorSpace {
        let head = match reader.resolve(&arr[0]) {
            Object::Name(n) => n.as_str().to_string(),
            _ => return ColorSpace::DeviceGray,
        };
        match head.as_str() {
            "ICCBased" => {
                // Use /N (or fall back to /Alternate) to pick a device space.
                if let Some(stream) = arr.get(1) {
                    if let Object::Stream(s) = reader.resolve(stream) {
                        let n = s.dict.get("N").and_then(int).unwrap_or(3);
                        if let Some(alt) = s.dict.get("Alternate") {
                            return ColorSpace::parse(reader, alt, resources);
                        }
                        return match n {
                            1 => ColorSpace::DeviceGray,
                            4 => ColorSpace::DeviceCMYK,
                            _ => ColorSpace::DeviceRGB,
                        };
                    }
                }
                ColorSpace::DeviceRGB
            }
            "CalGray" => ColorSpace::DeviceGray,
            "CalRGB" | "Lab" => ColorSpace::DeviceRGB,
            "Indexed" | "I" => {
                let base = arr
                    .get(1)
                    .map(|b| ColorSpace::parse(reader, b, resources))
                    .unwrap_or(ColorSpace::DeviceRGB);
                let hival = arr
                    .get(2)
                    .and_then(|o| int(reader.resolve(o)))
                    .unwrap_or(0)
                    .max(0) as usize;
                let lookup = match arr.get(3).map(|o| reader.resolve(o)) {
                    Some(Object::String(s)) => s.as_bytes().to_vec(),
                    Some(Object::Stream(s)) => reader.stream_data(s).unwrap_or_default(),
                    _ => Vec::new(),
                };
                ColorSpace::Indexed {
                    base: Box::new(base),
                    hival,
                    lookup,
                }
            }
            "Separation" => {
                let alt = arr
                    .get(2)
                    .map(|a| ColorSpace::parse(reader, a, resources))
                    .unwrap_or(ColorSpace::DeviceGray);
                let func = arr
                    .get(3)
                    .map(|f| Function::parse(reader, f))
                    .unwrap_or(Function::Identity);
                ColorSpace::Separation {
                    alt: Box::new(alt),
                    func: Rc::new(func),
                    n: 1,
                }
            }
            "DeviceN" => {
                let names = match arr.get(1).map(|o| reader.resolve(o)) {
                    Some(Object::Array(a)) => a.len(),
                    _ => 1,
                };
                let alt = arr
                    .get(2)
                    .map(|a| ColorSpace::parse(reader, a, resources))
                    .unwrap_or(ColorSpace::DeviceGray);
                let func = arr
                    .get(3)
                    .map(|f| Function::parse(reader, f))
                    .unwrap_or(Function::Identity);
                ColorSpace::Separation {
                    alt: Box::new(alt),
                    func: Rc::new(func),
                    n: names.max(1),
                }
            }
            "Pattern" => ColorSpace::Pattern,
            _ => ColorSpace::DeviceGray,
        }
    }
}

fn lookup_resource_cs(reader: &PdfReader, resources: &cos::Dict, name: &str) -> Option<Object> {
    let cs_dict = reader.resolve_dict(resources.get("ColorSpace")?)?;
    cs_dict.get(name).cloned()
}

fn int(o: &Object) -> Option<i64> {
    match o {
        Object::Integer(n) => Some(*n),
        Object::Real(r) => Some(*r as i64),
        _ => None,
    }
}
