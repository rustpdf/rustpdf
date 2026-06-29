//! The content-stream interpreter: tokenizes operators and drives tiny-skia.

use crate::color::ColorSpace;
use crate::font::LoadedFont;
use crate::gstate::{GState, TextRender};
use crate::matrix::{device_transform, mean_scale, pdf_matrix};
use crate::{shading, xobject, PageBox, RenderOptions};
use cos::{Dict, Object};
use parser::{Lexer, PdfReader, Token};
use std::collections::HashMap;
use std::rc::Rc;
use tiny_skia::{
    BlendMode, Color, FillRule, LineCap, LineJoin, Mask, Paint, PathBuilder, Pixmap, Shader,
    Stroke, StrokeDash, Transform,
};

/// Entry point: render `page`'s content into `pixmap`.
pub fn render(
    reader: &PdfReader,
    page: &Dict,
    pbox: PageBox,
    opts: &RenderOptions,
    pixmap: &mut Pixmap,
) {
    let base = device_transform(&pbox, opts.scale);
    let resources = resolve_resources(reader, page);
    let content = page_content(reader, page);
    let mut r = Renderer {
        reader,
        pixmap,
        base,
        font_cache: HashMap::new(),
    };
    let gs = GState::default();
    r.run(&content, &resources, gs, 0);
}

struct Renderer<'a> {
    reader: &'a PdfReader,
    pixmap: &'a mut Pixmap,
    base: Transform,
    /// Cache loaded fonts by the font dict's identity (object bytes hash).
    font_cache: HashMap<usize, Option<Rc<LoadedFont>>>,
}

/// Per-stream text positioning matrices.
#[derive(Clone, Copy)]
struct TextState {
    tm: Transform,
    tlm: Transform,
}

impl<'a> Renderer<'a> {
    fn page_w(&self) -> u32 {
        self.pixmap.width()
    }
    fn page_h(&self) -> u32 {
        self.pixmap.height()
    }

    fn run(&mut self, content: &[u8], resources: &Dict, init: GState, depth: u32) {
        if depth > 12 {
            return; // guard runaway form-xobject recursion
        }
        let mut gs = init;
        let mut stack: Vec<GState> = Vec::new();
        let mut ops: Vec<Object> = Vec::new();

        let mut pb = PathBuilder::new();
        let mut cur = (0.0f32, 0.0f32);
        let mut start = (0.0f32, 0.0f32);
        let mut pending_clip: Option<FillRule> = None;

        let mut ts = TextState {
            tm: Transform::identity(),
            tlm: Transform::identity(),
        };
        let mut text_clip = PathBuilder::new();
        let mut text_clip_active = false;

        let mut lex = Lexer::new(content);
        while let Some(tok) = lex.next_token() {
            match tok {
                Token::Integer(n) => ops.push(Object::Integer(n)),
                Token::Real(r) => ops.push(Object::Real(r)),
                Token::Str(s) => ops.push(Object::String(cos::PdfString::literal(s))),
                Token::Name(n) => ops.push(Object::Name(name_from(n))),
                Token::ArrayOpen => ops.push(read_array(&mut lex)),
                Token::DictOpen => ops.push(read_dict(&mut lex)),
                Token::ArrayClose | Token::DictClose => {}
                Token::Keyword(kw) => {
                    match kw.as_slice() {
                        b"true" => {
                            ops.push(Object::Bool(true));
                            continue;
                        }
                        b"false" => {
                            ops.push(Object::Bool(false));
                            continue;
                        }
                        b"null" => {
                            ops.push(Object::Null);
                            continue;
                        }
                        b"BI" => {
                            self.inline_image(&mut lex, resources, &gs);
                            ops.clear();
                            continue;
                        }
                        _ => {}
                    }
                    self.op(
                        &kw,
                        &ops,
                        resources,
                        &mut gs,
                        &mut stack,
                        &mut pb,
                        &mut cur,
                        &mut start,
                        &mut pending_clip,
                        &mut ts,
                        &mut text_clip,
                        &mut text_clip_active,
                        depth,
                    );
                    ops.clear();
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn op(
        &mut self,
        op: &[u8],
        ops: &[Object],
        resources: &Dict,
        gs: &mut GState,
        stack: &mut Vec<GState>,
        pb: &mut PathBuilder,
        cur: &mut (f32, f32),
        start: &mut (f32, f32),
        pending_clip: &mut Option<FillRule>,
        ts: &mut TextState,
        text_clip: &mut PathBuilder,
        text_clip_active: &mut bool,
        depth: u32,
    ) {
        let n = |i: usize| ops.get(i).and_then(num);
        match op {
            // ---- graphics state ----
            b"q" => stack.push(gs.clone()),
            b"Q" => {
                if let Some(s) = stack.pop() {
                    *gs = s;
                }
            }
            b"cm" => {
                if let (Some(a), Some(b), Some(c), Some(d), Some(e), Some(f)) =
                    (n(0), n(1), n(2), n(3), n(4), n(5))
                {
                    gs.ctm = gs.ctm.pre_concat(pdf_matrix(a, b, c, d, e, f));
                }
            }
            b"w" => {
                if let Some(w) = n(0) {
                    gs.line_width = w;
                }
            }
            b"J" => {
                if let Some(c) = n(0) {
                    gs.line_cap = match c as i32 {
                        1 => LineCap::Round,
                        2 => LineCap::Square,
                        _ => LineCap::Butt,
                    };
                }
            }
            b"j" => {
                if let Some(j) = n(0) {
                    gs.line_join = match j as i32 {
                        1 => LineJoin::Round,
                        2 => LineJoin::Bevel,
                        _ => LineJoin::Miter,
                    };
                }
            }
            b"M" => {
                if let Some(m) = n(0) {
                    gs.miter_limit = m.max(1.0);
                }
            }
            b"d" => {
                if let Some(Object::Array(arr)) = ops.first() {
                    gs.dash = arr.iter().filter_map(num).collect();
                    gs.dash_phase = n(1).unwrap_or(0.0);
                }
            }
            b"gs" => self.apply_ext_gstate(ops.first(), resources, gs),
            b"ri" | b"i" => {}

            // ---- path construction ----
            b"m" => {
                if let (Some(x), Some(y)) = (n(0), n(1)) {
                    pb.move_to(x, y);
                    *cur = (x, y);
                    *start = (x, y);
                }
            }
            b"l" => {
                if let (Some(x), Some(y)) = (n(0), n(1)) {
                    ensure_start(pb, cur);
                    pb.line_to(x, y);
                    *cur = (x, y);
                }
            }
            b"c" => {
                if let (Some(x1), Some(y1), Some(x2), Some(y2), Some(x3), Some(y3)) =
                    (n(0), n(1), n(2), n(3), n(4), n(5))
                {
                    ensure_start(pb, cur);
                    pb.cubic_to(x1, y1, x2, y2, x3, y3);
                    *cur = (x3, y3);
                }
            }
            b"v" => {
                if let (Some(x2), Some(y2), Some(x3), Some(y3)) = (n(0), n(1), n(2), n(3)) {
                    ensure_start(pb, cur);
                    pb.cubic_to(cur.0, cur.1, x2, y2, x3, y3);
                    *cur = (x3, y3);
                }
            }
            b"y" => {
                if let (Some(x1), Some(y1), Some(x3), Some(y3)) = (n(0), n(1), n(2), n(3)) {
                    ensure_start(pb, cur);
                    pb.cubic_to(x1, y1, x3, y3, x3, y3);
                    *cur = (x3, y3);
                }
            }
            b"re" => {
                if let (Some(x), Some(y), Some(w), Some(h)) = (n(0), n(1), n(2), n(3)) {
                    pb.move_to(x, y);
                    pb.line_to(x + w, y);
                    pb.line_to(x + w, y + h);
                    pb.line_to(x, y + h);
                    pb.close();
                    *cur = (x, y);
                    *start = (x, y);
                }
            }
            b"h" => {
                pb.close();
                *cur = *start;
            }

            // ---- path painting ----
            b"S" => self.finish_path(pb, gs, None, Some(()), pending_clip, cur),
            b"s" => {
                pb.close();
                self.finish_path(pb, gs, None, Some(()), pending_clip, cur);
            }
            b"f" | b"F" => {
                self.finish_path(pb, gs, Some(FillRule::Winding), None, pending_clip, cur)
            }
            b"f*" => self.finish_path(pb, gs, Some(FillRule::EvenOdd), None, pending_clip, cur),
            b"B" | b"B*" => {
                let rule = if op == b"B*" {
                    FillRule::EvenOdd
                } else {
                    FillRule::Winding
                };
                self.finish_path(pb, gs, Some(rule), Some(()), pending_clip, cur);
            }
            b"b" | b"b*" => {
                pb.close();
                let rule = if op == b"b*" {
                    FillRule::EvenOdd
                } else {
                    FillRule::Winding
                };
                self.finish_path(pb, gs, Some(rule), Some(()), pending_clip, cur);
            }
            b"n" => self.finish_path(pb, gs, None, None, pending_clip, cur),
            b"W" => *pending_clip = Some(FillRule::Winding),
            b"W*" => *pending_clip = Some(FillRule::EvenOdd),

            // ---- color ----
            b"g" => {
                gs.fill_cs = ColorSpace::DeviceGray;
                gs.fill_rgb = gray(n(0));
            }
            b"G" => {
                gs.stroke_cs = ColorSpace::DeviceGray;
                gs.stroke_rgb = gray(n(0));
            }
            b"rg" => {
                gs.fill_cs = ColorSpace::DeviceRGB;
                gs.fill_rgb = [
                    n(0).unwrap_or(0.0),
                    n(1).unwrap_or(0.0),
                    n(2).unwrap_or(0.0),
                ];
            }
            b"RG" => {
                gs.stroke_cs = ColorSpace::DeviceRGB;
                gs.stroke_rgb = [
                    n(0).unwrap_or(0.0),
                    n(1).unwrap_or(0.0),
                    n(2).unwrap_or(0.0),
                ];
            }
            b"k" => {
                gs.fill_cs = ColorSpace::DeviceCMYK;
                gs.fill_rgb = ColorSpace::DeviceCMYK.to_rgb(&collect_nums(ops));
            }
            b"K" => {
                gs.stroke_cs = ColorSpace::DeviceCMYK;
                gs.stroke_rgb = ColorSpace::DeviceCMYK.to_rgb(&collect_nums(ops));
            }
            b"cs" => {
                gs.fill_cs = self.resolve_cs(ops.first(), resources);
                gs.fill_rgb = gs.fill_cs.default_rgb();
            }
            b"CS" => {
                gs.stroke_cs = self.resolve_cs(ops.first(), resources);
                gs.stroke_rgb = gs.stroke_cs.default_rgb();
            }
            b"sc" | b"scn" => {
                let nums = collect_nums(ops);
                if !nums.is_empty() {
                    gs.fill_rgb = gs.fill_cs.to_rgb(&nums);
                }
            }
            b"SC" | b"SCN" => {
                let nums = collect_nums(ops);
                if !nums.is_empty() {
                    gs.stroke_rgb = gs.stroke_cs.to_rgb(&nums);
                }
            }

            // ---- text state ----
            b"BT" => {
                ts.tm = Transform::identity();
                ts.tlm = Transform::identity();
            }
            b"ET" => {
                if *text_clip_active {
                    let built = std::mem::replace(text_clip, PathBuilder::new());
                    if let Some(path) = built.finish() {
                        self.intersect_clip(gs, &path, FillRule::Winding, self.base);
                    }
                    *text_clip_active = false;
                }
            }
            b"Tc" => gs.char_spacing = n(0).unwrap_or(0.0),
            b"Tw" => gs.word_spacing = n(0).unwrap_or(0.0),
            b"Tz" => gs.h_scale = n(0).unwrap_or(100.0) / 100.0,
            b"TL" => gs.leading = n(0).unwrap_or(0.0),
            b"Ts" => gs.text_rise = n(0).unwrap_or(0.0),
            b"Tr" => gs.render_mode = TextRender::from_i32(n(0).unwrap_or(0.0) as i32),
            b"Tf" => {
                if let Some(Object::Name(fname)) = ops.first() {
                    gs.font = self.load_font(fname.as_str(), resources);
                    gs.font_size = n(1).unwrap_or(0.0);
                }
            }
            b"Td" => {
                let (tx, ty) = (n(0).unwrap_or(0.0), n(1).unwrap_or(0.0));
                ts.tlm = ts.tlm.pre_concat(Transform::from_translate(tx, ty));
                ts.tm = ts.tlm;
            }
            b"TD" => {
                let (tx, ty) = (n(0).unwrap_or(0.0), n(1).unwrap_or(0.0));
                gs.leading = -ty;
                ts.tlm = ts.tlm.pre_concat(Transform::from_translate(tx, ty));
                ts.tm = ts.tlm;
            }
            b"Tm" => {
                if let (Some(a), Some(b), Some(c), Some(d), Some(e), Some(f)) =
                    (n(0), n(1), n(2), n(3), n(4), n(5))
                {
                    ts.tlm = pdf_matrix(a, b, c, d, e, f);
                    ts.tm = ts.tlm;
                }
            }
            b"T*" => {
                ts.tlm = ts
                    .tlm
                    .pre_concat(Transform::from_translate(0.0, -gs.leading));
                ts.tm = ts.tlm;
            }
            b"Tj" => {
                if let Some(Object::String(s)) = ops.first() {
                    self.show_text(
                        s.as_bytes(),
                        gs,
                        ts,
                        text_clip,
                        text_clip_active,
                        resources,
                        depth,
                    );
                }
            }
            b"'" => {
                ts.tlm = ts
                    .tlm
                    .pre_concat(Transform::from_translate(0.0, -gs.leading));
                ts.tm = ts.tlm;
                if let Some(Object::String(s)) = ops.first() {
                    self.show_text(
                        s.as_bytes(),
                        gs,
                        ts,
                        text_clip,
                        text_clip_active,
                        resources,
                        depth,
                    );
                }
            }
            b"\"" => {
                gs.word_spacing = n(0).unwrap_or(0.0);
                gs.char_spacing = n(1).unwrap_or(0.0);
                ts.tlm = ts
                    .tlm
                    .pre_concat(Transform::from_translate(0.0, -gs.leading));
                ts.tm = ts.tlm;
                if let Some(Object::String(s)) = ops.get(2) {
                    self.show_text(
                        s.as_bytes(),
                        gs,
                        ts,
                        text_clip,
                        text_clip_active,
                        resources,
                        depth,
                    );
                }
            }
            b"TJ" => {
                if let Some(Object::Array(arr)) = ops.first() {
                    self.show_text_array(
                        arr,
                        gs,
                        ts,
                        text_clip,
                        text_clip_active,
                        resources,
                        depth,
                    );
                }
            }
            b"d0" | b"d1" => {}

            // ---- XObjects ----
            b"Do" => {
                if let Some(Object::Name(name)) = ops.first() {
                    self.do_xobject(name.as_str(), resources, gs, depth);
                }
            }

            // ---- shading ----
            b"sh" => {
                if let Some(Object::Name(name)) = ops.first() {
                    self.paint_shading(name.as_str(), resources, gs);
                }
            }

            // ---- marked content / ignored ----
            b"BDC" | b"BMC" | b"EMC" | b"DP" | b"MP" | b"BX" | b"EX" => {}
            _ => {}
        }
        let _ = (text_clip, text_clip_active);
    }

    // ---- path painting helpers ----

    fn finish_path(
        &mut self,
        pb: &mut PathBuilder,
        gs: &mut GState,
        fill: Option<FillRule>,
        stroke: Option<()>,
        pending_clip: &mut Option<FillRule>,
        cur: &mut (f32, f32),
    ) {
        let built = std::mem::replace(pb, PathBuilder::new());
        let path = built.finish();
        let total = self.base.pre_concat(gs.ctm);
        if let Some(path) = &path {
            if let Some(rule) = fill {
                let paint = solid_paint(gs.fill_rgb, gs.fill_alpha, gs.blend);
                self.pixmap.fill_path(
                    path,
                    &paint,
                    rule,
                    total,
                    gs.clip.as_ref().map(|m| m.as_ref()),
                );
            }
            if stroke.is_some() {
                self.stroke(path, gs, total);
            }
            if let Some(rule) = pending_clip.take() {
                self.intersect_clip(gs, path, rule, total);
            }
        } else {
            // `n`/`W n` with no geometry can still set an (empty) clip.
            pending_clip.take();
        }
        *cur = (0.0, 0.0);
    }

    fn stroke(&mut self, path: &tiny_skia::Path, gs: &GState, total: Transform) {
        let scale = mean_scale(&total);
        let mut width = gs.line_width;
        if width * scale < 1.0 {
            // zero/hairline width ⇒ ~1 device pixel.
            width = 1.0 / scale;
        }
        let dash = if gs.dash.iter().any(|&d| d > 0.0) {
            StrokeDash::new(gs.dash.clone(), gs.dash_phase)
        } else {
            None
        };
        let stroke = Stroke {
            width,
            miter_limit: gs.miter_limit,
            line_cap: gs.line_cap,
            line_join: gs.line_join,
            dash,
        };
        let paint = solid_paint(gs.stroke_rgb, gs.stroke_alpha, gs.blend);
        self.pixmap.stroke_path(
            path,
            &paint,
            &stroke,
            total,
            gs.clip.as_ref().map(|m| m.as_ref()),
        );
    }

    fn intersect_clip(
        &self,
        gs: &mut GState,
        path: &tiny_skia::Path,
        rule: FillRule,
        total: Transform,
    ) {
        let mut mask = match &gs.clip {
            Some(existing) => existing.as_ref().clone(),
            None => match Mask::new(self.page_w(), self.page_h()) {
                Some(m) => {
                    // Fresh mask: set to this path's coverage directly.
                    let mut m = m;
                    m.fill_path(path, rule, true, total);
                    gs.clip = Some(Rc::new(m));
                    return;
                }
                None => return,
            },
        };
        mask.intersect_path(path, rule, true, total);
        gs.clip = Some(Rc::new(mask));
    }

    // ---- text ----

    #[allow(clippy::too_many_arguments)]
    fn show_text_array(
        &mut self,
        arr: &[Object],
        gs: &mut GState,
        ts: &mut TextState,
        text_clip: &mut PathBuilder,
        text_clip_active: &mut bool,
        resources: &Dict,
        depth: u32,
    ) {
        for el in arr {
            match el {
                Object::String(s) => self.show_text(
                    s.as_bytes(),
                    gs,
                    ts,
                    text_clip,
                    text_clip_active,
                    resources,
                    depth,
                ),
                Object::Integer(_) | Object::Real(_) => {
                    let adj = num(el).unwrap_or(0.0);
                    let tx = -adj / 1000.0 * gs.font_size * gs.h_scale;
                    ts.tm = ts.tm.pre_concat(Transform::from_translate(tx, 0.0));
                }
                _ => {}
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn show_text(
        &mut self,
        bytes: &[u8],
        gs: &mut GState,
        ts: &mut TextState,
        text_clip: &mut PathBuilder,
        text_clip_active: &mut bool,
        resources: &Dict,
        depth: u32,
    ) {
        let Some(font) = gs.font.clone() else {
            return;
        };
        let fs = gs.font_size;
        let upem = font.units_per_em().max(1.0);
        let mode = gs.render_mode;
        let is_type3 = font.type3().is_some();
        let glyphs = font.decode(bytes);
        for g in glyphs {
            // Render matrix: base · ctm · Tm · [fs·Th 0 0 fs 0 rise] · (1/upem)
            let param = pdf_matrix(fs * gs.h_scale, 0.0, 0.0, fs, 0.0, gs.text_rise);
            let trm = self
                .base
                .pre_concat(gs.ctm)
                .pre_concat(ts.tm)
                .pre_concat(param);
            let glyph_to_device = trm.pre_concat(Transform::from_scale(1.0 / upem, 1.0 / upem));

            if is_type3 {
                if !mode.invisible() && fs != 0.0 {
                    self.draw_type3_glyph(&font, g.gid as u8, gs, ts, param, resources, depth);
                }
                // Advance, then skip the outline path below.
                let w0 = g.width;
                let mut tx = (w0 * fs + gs.char_spacing) * gs.h_scale;
                if g.is_space {
                    tx += gs.word_spacing * gs.h_scale;
                }
                ts.tm = ts.tm.pre_concat(Transform::from_translate(tx, 0.0));
                continue;
            }

            if !mode.invisible() && fs != 0.0 {
                if let Some(path) = glyph_path(&font, g.gid) {
                    if mode.fills() {
                        let paint = solid_paint(gs.fill_rgb, gs.fill_alpha, gs.blend);
                        self.pixmap.fill_path(
                            &path,
                            &paint,
                            FillRule::Winding,
                            glyph_to_device,
                            gs.clip.as_ref().map(|m| m.as_ref()),
                        );
                    }
                    if mode.strokes() {
                        self.stroke(&path, gs, glyph_to_device);
                    }
                    if matches!(
                        mode,
                        TextRender::FillClip
                            | TextRender::StrokeClip
                            | TextRender::FillStrokeClip
                            | TextRender::Clip
                    ) {
                        append_transformed(text_clip, &path, glyph_to_device);
                        *text_clip_active = true;
                    }
                }
            }

            // Advance.
            let w0 = g.width; // text-space units (already /1000)
            let mut tx = (w0 * fs + gs.char_spacing) * gs.h_scale;
            if g.is_space {
                tx += gs.word_spacing * gs.h_scale;
            }
            ts.tm = ts.tm.pre_concat(Transform::from_translate(tx, 0.0));
        }
    }

    /// Draw one Type 3 glyph by executing its CharProc content stream, mapped to
    /// device space by `base · ctm · Tm · param · FontMatrix`. Uncolored (`d1`)
    /// glyphs inherit the current fill color; colored (`d0`) ones set their own.
    #[allow(clippy::too_many_arguments)]
    fn draw_type3_glyph(
        &mut self,
        font: &LoadedFont,
        code: u8,
        gs: &GState,
        ts: &TextState,
        param: Transform,
        resources: &Dict,
        depth: u32,
    ) {
        let Some(t3) = font.type3() else {
            return;
        };
        let Some(proc) = t3.char_proc(code) else {
            return;
        };
        let fm = t3.font_matrix;
        let fmx = pdf_matrix(fm[0], fm[1], fm[2], fm[3], fm[4], fm[5]);
        let mut sub = gs.clone();
        // run() prefixes self.base, so set sub.ctm = ctm · Tm · param · FontMatrix.
        sub.ctm = gs.ctm.pre_concat(ts.tm).pre_concat(param).pre_concat(fmx);
        sub.font = None; // CharProcs don't show text; avoid accidental recursion.
        let res = t3.resources.clone().unwrap_or_else(|| resources.clone());
        let proc = proc.to_vec();
        self.run(&proc, &res, sub, depth + 1);
    }

    // ---- fonts ----

    fn load_font(&mut self, name: &str, resources: &Dict) -> Option<Rc<LoadedFont>> {
        let font_res = self.reader.resolve_dict(resources.get("Font")?)?;
        let font_obj = font_res.get(name)?;
        let key = obj_key(font_obj);
        if let Some(cached) = self.font_cache.get(&key) {
            return cached.clone();
        }
        let dict = self.reader.resolve_dict(font_obj)?.clone();
        let loaded = LoadedFont::load(self.reader, &dict).map(Rc::new);
        self.font_cache.insert(key, loaded.clone());
        loaded
    }

    // ---- color space ----

    fn resolve_cs(&self, obj: Option<&Object>, resources: &Dict) -> ColorSpace {
        match obj {
            Some(o) => ColorSpace::parse(self.reader, o, resources),
            None => ColorSpace::DeviceGray,
        }
    }

    // ---- ExtGState ----

    fn apply_ext_gstate(&self, name: Option<&Object>, resources: &Dict, gs: &mut GState) {
        let Some(Object::Name(name)) = name else {
            return;
        };
        let Some(egs_dict) = self
            .reader
            .resolve_dict(resources.get("ExtGState").unwrap_or(&Object::Null))
        else {
            return;
        };
        let Some(egs) = egs_dict
            .get(name.as_str())
            .and_then(|o| self.reader.resolve_dict(o))
        else {
            return;
        };
        if let Some(ca) = egs.get("ca").and_then(num) {
            gs.fill_alpha = ca.clamp(0.0, 1.0);
        }
        if let Some(ca) = egs.get("CA").and_then(num) {
            gs.stroke_alpha = ca.clamp(0.0, 1.0);
        }
        if let Some(lw) = egs.get("LW").and_then(num) {
            gs.line_width = lw;
        }
        if let Some(Object::Name(bm)) = egs.get("BM") {
            gs.blend = blend_mode(bm.as_str());
        }
        if let Some(Object::Array(a)) = egs.get("BM") {
            if let Some(Object::Name(bm)) = a.first() {
                gs.blend = blend_mode(bm.as_str());
            }
        }
        // Font [ref size]
        if let Some(Object::Array(fa)) = egs.get("Font") {
            if let (Some(fref), Some(sz)) = (fa.first(), fa.get(1).and_then(num)) {
                if let Some(fd) = self.reader.resolve_dict(fref) {
                    gs.font = LoadedFont::load(self.reader, fd).map(Rc::new);
                    gs.font_size = sz;
                }
            }
        }
    }

    // ---- XObjects ----

    fn do_xobject(&mut self, name: &str, resources: &Dict, gs: &GState, depth: u32) {
        let Some(xobjs) = self
            .reader
            .resolve_dict(resources.get("XObject").unwrap_or(&Object::Null))
        else {
            return;
        };
        let Some(obj) = xobjs.get(name).map(|o| self.reader.resolve(o).clone()) else {
            return;
        };
        let Object::Stream(stream) = obj else {
            return;
        };
        let subtype = match stream.dict.get("Subtype") {
            Some(Object::Name(n)) => n.as_str().to_string(),
            _ => String::new(),
        };
        match subtype.as_str() {
            "Image" => self.draw_image(&stream, resources, gs),
            "Form" => self.draw_form(&stream, resources, gs, depth),
            _ => {}
        }
    }

    fn draw_form(&mut self, stream: &cos::Stream, parent_res: &Dict, gs: &GState, depth: u32) {
        let mut sub = gs.clone();
        // Apply the form /Matrix.
        if let Some(Object::Array(m)) = stream.dict.get("Matrix") {
            let v: Vec<f32> = m.iter().filter_map(num).collect();
            if v.len() == 6 {
                sub.ctm = sub
                    .ctm
                    .pre_concat(pdf_matrix(v[0], v[1], v[2], v[3], v[4], v[5]));
            }
        }
        // Clip to /BBox.
        if let Some(Object::Array(b)) = stream.dict.get("BBox") {
            let v: Vec<f32> = b.iter().filter_map(num).collect();
            if v.len() == 4 {
                let (x0, y0, x1, y1) = (
                    v[0].min(v[2]),
                    v[1].min(v[3]),
                    v[0].max(v[2]),
                    v[1].max(v[3]),
                );
                let mut pbb = PathBuilder::new();
                pbb.move_to(x0, y0);
                pbb.line_to(x1, y0);
                pbb.line_to(x1, y1);
                pbb.line_to(x0, y1);
                pbb.close();
                if let Some(path) = pbb.finish() {
                    let total = self.base.pre_concat(sub.ctm);
                    self.intersect_clip(&mut sub, &path, FillRule::Winding, total);
                }
            }
        }
        let res = match stream
            .dict
            .get("Resources")
            .and_then(|o| self.reader.resolve_dict(o))
        {
            Some(d) => d.clone(),
            None => parent_res.clone(),
        };
        let data = self.reader.stream_data(stream).unwrap_or_default();
        self.run(&data, &res, sub, depth + 1);
    }

    fn draw_image(&mut self, stream: &cos::Stream, resources: &Dict, gs: &GState) {
        let Some(img) = xobject::decode_image(self.reader, stream, resources, gs.fill_rgb) else {
            return;
        };
        let (iw, ih) = (img.width() as f32, img.height() as f32);
        // Map image pixel space → user unit square → device.
        // sample (col,row) → (col/iw, 1 - row/ih) in the unit square.
        let m = pdf_matrix(1.0 / iw, 0.0, 0.0, -1.0 / ih, 0.0, 1.0);
        let total = self.base.pre_concat(gs.ctm).pre_concat(m);
        let paint = tiny_skia::PixmapPaint {
            opacity: gs.fill_alpha,
            blend_mode: gs.blend,
            quality: tiny_skia::FilterQuality::Bilinear,
        };
        self.pixmap.draw_pixmap(
            0,
            0,
            img.as_ref(),
            &paint,
            total,
            gs.clip.as_ref().map(|m| m.as_ref()),
        );
    }

    fn inline_image(&mut self, lex: &mut Lexer, resources: &Dict, gs: &GState) {
        let Some((dict, data)) = xobject::read_inline_image(lex) else {
            return;
        };
        if let Some(img) = xobject::decode_inline(self.reader, &dict, &data, resources, gs.fill_rgb)
        {
            let (iw, ih) = (img.width() as f32, img.height() as f32);
            let m = pdf_matrix(1.0 / iw, 0.0, 0.0, -1.0 / ih, 0.0, 1.0);
            let total = self.base.pre_concat(gs.ctm).pre_concat(m);
            let paint = tiny_skia::PixmapPaint {
                opacity: gs.fill_alpha,
                blend_mode: gs.blend,
                quality: tiny_skia::FilterQuality::Bilinear,
            };
            self.pixmap.draw_pixmap(
                0,
                0,
                img.as_ref(),
                &paint,
                total,
                gs.clip.as_ref().map(|m| m.as_ref()),
            );
        }
    }

    // ---- shading ----

    fn paint_shading(&mut self, name: &str, resources: &Dict, gs: &GState) {
        let Some(sh_dict) = self
            .reader
            .resolve_dict(resources.get("Shading").unwrap_or(&Object::Null))
        else {
            return;
        };
        let Some(obj) = sh_dict.get(name).cloned() else {
            return;
        };
        let total = self.base.pre_concat(gs.ctm);
        let Some(shader) = shading::shader_for(self.reader, &obj, resources, total) else {
            return;
        };
        // Fill the whole page rect (clipped to the current clip) with the shading.
        let mut pb = PathBuilder::new();
        pb.push_rect(
            tiny_skia::Rect::from_xywh(0.0, 0.0, self.page_w() as f32, self.page_h() as f32)
                .unwrap(),
        );
        if let Some(path) = pb.finish() {
            let paint = Paint {
                shader,
                blend_mode: gs.blend,
                anti_alias: true,
                ..Default::default()
            };
            self.pixmap.fill_path(
                &path,
                &paint,
                FillRule::Winding,
                Transform::identity(),
                gs.clip.as_ref().map(|m| m.as_ref()),
            );
        }
    }
}

// ---- free helpers ----

fn ensure_start(pb: &mut PathBuilder, cur: &(f32, f32)) {
    if pb.is_empty() {
        pb.move_to(cur.0, cur.1);
    }
}

fn solid_paint(rgb: [f32; 3], alpha: f32, blend: BlendMode) -> Paint<'static> {
    let c = Color::from_rgba(
        rgb[0].clamp(0.0, 1.0),
        rgb[1].clamp(0.0, 1.0),
        rgb[2].clamp(0.0, 1.0),
        alpha.clamp(0.0, 1.0),
    )
    .unwrap_or(Color::BLACK);
    Paint {
        shader: Shader::SolidColor(c),
        blend_mode: blend,
        anti_alias: true,
        ..Default::default()
    }
}

fn blend_mode(name: &str) -> BlendMode {
    match name {
        "Multiply" => BlendMode::Multiply,
        "Screen" => BlendMode::Screen,
        "Overlay" => BlendMode::Overlay,
        "Darken" => BlendMode::Darken,
        "Lighten" => BlendMode::Lighten,
        "ColorDodge" => BlendMode::ColorDodge,
        "ColorBurn" => BlendMode::ColorBurn,
        "HardLight" => BlendMode::HardLight,
        "SoftLight" => BlendMode::SoftLight,
        "Difference" => BlendMode::Difference,
        "Exclusion" => BlendMode::Exclusion,
        _ => BlendMode::SourceOver,
    }
}

fn gray(v: Option<f32>) -> [f32; 3] {
    let g = v.unwrap_or(0.0);
    [g, g, g]
}

fn collect_nums(ops: &[Object]) -> Vec<f32> {
    ops.iter().filter_map(num).collect()
}

/// Build a tiny-skia path from a glyph outline (font units).
fn glyph_path(font: &LoadedFont, gid: u16) -> Option<tiny_skia::Path> {
    let mut builder = GlyphOutline {
        pb: PathBuilder::new(),
    };
    let face = font.face();
    face.outline_glyph(ttf_parser::GlyphId(gid), &mut builder)?;
    builder.pb.finish()
}

struct GlyphOutline {
    pb: PathBuilder,
}

impl ttf_parser::OutlineBuilder for GlyphOutline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.pb.move_to(x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.pb.line_to(x, y);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.pb.quad_to(x1, y1, x, y);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.pb.cubic_to(x1, y1, x2, y2, x, y);
    }
    fn close(&mut self) {
        self.pb.close();
    }
}

/// Append `path` (in some local space) into `dst`, pre-transformed by `t`.
fn append_transformed(dst: &mut PathBuilder, path: &tiny_skia::Path, t: Transform) {
    if let Some(tp) = path.clone().transform(t) {
        dst.push_path(&tp);
    }
}

fn obj_key(o: &Object) -> usize {
    // Cheap structural-ish key; reference number when available.
    match o {
        Object::Reference(r) => (r.number as usize) << 16 | r.generation as usize,
        _ => o as *const _ as usize,
    }
}

fn num(o: &Object) -> Option<f32> {
    match o {
        Object::Integer(n) => Some(*n as f32),
        Object::Real(r) => Some(*r as f32),
        _ => None,
    }
}

/// Build a `cos::Name` from raw name bytes (names are ASCII in practice).
fn name_from(bytes: Vec<u8>) -> cos::Name {
    cos::Name::new(String::from_utf8_lossy(&bytes).into_owned())
}

// ---- operand structure readers ----

fn read_array(lex: &mut Lexer) -> Object {
    let mut items = Vec::new();
    while let Some(tok) = lex.next_token() {
        match tok {
            Token::ArrayClose => break,
            Token::Integer(n) => items.push(Object::Integer(n)),
            Token::Real(r) => items.push(Object::Real(r)),
            Token::Str(s) => items.push(Object::String(cos::PdfString::literal(s))),
            Token::Name(n) => items.push(Object::Name(name_from(n))),
            Token::ArrayOpen => items.push(read_array(lex)),
            Token::DictOpen => items.push(read_dict(lex)),
            Token::DictClose => {}
            Token::Keyword(k) => match k.as_slice() {
                b"true" => items.push(Object::Bool(true)),
                b"false" => items.push(Object::Bool(false)),
                b"null" => items.push(Object::Null),
                _ => {}
            },
        }
    }
    Object::Array(items)
}

fn read_dict(lex: &mut Lexer) -> Object {
    let mut dict = Dict::new();
    loop {
        let key = match lex.next_token() {
            Some(Token::Name(n)) => n,
            Some(Token::DictClose) | None => break,
            _ => continue,
        };
        let val = match lex.next_token() {
            Some(Token::Integer(n)) => Object::Integer(n),
            Some(Token::Real(r)) => Object::Real(r),
            Some(Token::Str(s)) => Object::String(cos::PdfString::literal(s)),
            Some(Token::Name(n)) => Object::Name(name_from(n)),
            Some(Token::ArrayOpen) => read_array(lex),
            Some(Token::DictOpen) => read_dict(lex),
            Some(Token::Keyword(k)) => match k.as_slice() {
                b"true" => Object::Bool(true),
                b"false" => Object::Bool(false),
                _ => Object::Null,
            },
            Some(Token::DictClose) | None => break,
            _ => Object::Null,
        };
        dict.set(name_from(key), val);
    }
    Object::Dict(dict)
}

// ---- page plumbing ----

fn resolve_resources(reader: &PdfReader, page: &Dict) -> Dict {
    // Resources are inheritable up the page tree.
    let mut cur = page.clone();
    for _ in 0..32 {
        if let Some(res) = cur.get("Resources").and_then(|o| reader.resolve_dict(o)) {
            return res.clone();
        }
        match cur.get("Parent").and_then(|p| reader.resolve_dict(p)) {
            Some(parent) => cur = parent.clone(),
            None => break,
        }
    }
    Dict::new()
}

fn page_content(reader: &PdfReader, page: &Dict) -> Vec<u8> {
    let mut out = Vec::new();
    match page.get("Contents").map(|o| reader.resolve(o)) {
        Some(Object::Stream(s)) => {
            if let Ok(data) = reader.stream_data(s) {
                out = data;
            }
        }
        Some(Object::Array(items)) => {
            for item in items {
                if let Object::Stream(s) = reader.resolve(item) {
                    if let Ok(data) = reader.stream_data(s) {
                        out.extend_from_slice(&data);
                        out.push(b'\n');
                    }
                }
            }
        }
        _ => {}
    }
    out
}
