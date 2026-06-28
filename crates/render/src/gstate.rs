//! The PDF graphics state and its `q`/`Q` stack.

use crate::color::ColorSpace;
use crate::font::LoadedFont;
use std::rc::Rc;
use tiny_skia::{LineCap, LineJoin, Mask, Transform};

/// Text rendering mode (`Tr` operator), Table 106.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextRender {
    Fill,
    Stroke,
    FillStroke,
    Invisible,
    FillClip,
    StrokeClip,
    FillStrokeClip,
    Clip,
}

impl TextRender {
    pub fn from_i32(n: i32) -> TextRender {
        match n {
            1 => TextRender::Stroke,
            2 => TextRender::FillStroke,
            3 => TextRender::Invisible,
            4 => TextRender::FillClip,
            5 => TextRender::StrokeClip,
            6 => TextRender::FillStrokeClip,
            7 => TextRender::Clip,
            _ => TextRender::Fill,
        }
    }
    pub fn fills(self) -> bool {
        matches!(
            self,
            TextRender::Fill
                | TextRender::FillStroke
                | TextRender::FillClip
                | TextRender::FillStrokeClip
        )
    }
    pub fn strokes(self) -> bool {
        matches!(
            self,
            TextRender::Stroke
                | TextRender::FillStroke
                | TextRender::StrokeClip
                | TextRender::FillStrokeClip
        )
    }
    pub fn invisible(self) -> bool {
        self == TextRender::Invisible
    }
}

/// Everything tracked by the graphics state. Cloned on `q`, restored on `Q`.
#[derive(Clone)]
pub struct GState {
    /// Current transformation matrix, PDF user space → page space (points).
    pub ctm: Transform,

    // --- colors (straight, 0..=1 RGB) + their source color spaces ---
    pub fill_rgb: [f32; 3],
    pub stroke_rgb: [f32; 3],
    pub fill_cs: ColorSpace,
    pub stroke_cs: ColorSpace,

    // --- line style ---
    pub line_width: f32,
    pub line_cap: LineCap,
    pub line_join: LineJoin,
    pub miter_limit: f32,
    pub dash: Vec<f32>,
    pub dash_phase: f32,

    // --- transparency ---
    pub fill_alpha: f32,
    pub stroke_alpha: f32,
    pub blend: tiny_skia::BlendMode,

    // --- clipping (device space) ---
    pub clip: Option<Rc<Mask>>,

    // --- text state ---
    pub font: Option<Rc<LoadedFont>>,
    pub font_size: f32,
    pub char_spacing: f32,
    pub word_spacing: f32,
    pub h_scale: f32, // Tz / 100
    pub leading: f32,
    pub text_rise: f32,
    pub render_mode: TextRender,
}

impl Default for GState {
    fn default() -> Self {
        GState {
            ctm: Transform::identity(),
            fill_rgb: [0.0, 0.0, 0.0],
            stroke_rgb: [0.0, 0.0, 0.0],
            fill_cs: ColorSpace::DeviceGray,
            stroke_cs: ColorSpace::DeviceGray,
            line_width: 1.0,
            line_cap: LineCap::Butt,
            line_join: LineJoin::Miter,
            miter_limit: 10.0,
            dash: Vec::new(),
            dash_phase: 0.0,
            fill_alpha: 1.0,
            stroke_alpha: 1.0,
            blend: tiny_skia::BlendMode::SourceOver,
            clip: None,
            font: None,
            font_size: 0.0,
            char_spacing: 0.0,
            word_spacing: 0.0,
            h_scale: 1.0,
            leading: 0.0,
            text_rise: 0.0,
            render_mode: TextRender::Fill,
        }
    }
}
