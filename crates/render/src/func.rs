//! PDF function objects (§7.10): sampled (type 0), exponential (type 2),
//! stitching (type 3) and PostScript calculator (type 4). Used by Separation /
//! DeviceN tint transforms and by shadings.

use cos::{Dict, Object};
use parser::PdfReader;

#[derive(Clone)]
pub enum Function {
    Identity,
    Sampled {
        domain: Vec<(f32, f32)>,
        range: Vec<(f32, f32)>,
        size: Vec<usize>,
        bps: u32,
        encode: Vec<(f32, f32)>,
        decode: Vec<(f32, f32)>,
        samples: Vec<u8>,
        n_out: usize,
    },
    Exponential {
        domain: (f32, f32),
        c0: Vec<f32>,
        c1: Vec<f32>,
        n: f32,
    },
    Stitching {
        domain: (f32, f32),
        funcs: Vec<Function>,
        bounds: Vec<f32>,
        encode: Vec<(f32, f32)>,
    },
    PostScript {
        domain: Vec<(f32, f32)>,
        range: Vec<(f32, f32)>,
        prog: Vec<PsTok>,
    },
}

impl Function {
    /// Parse a function (or an array of functions wrapped as a DeviceN-style
    /// parallel function) from an object.
    pub fn parse(reader: &PdfReader, obj: &Object) -> Function {
        let obj = reader.resolve(obj);
        // An array of functions: treat as n parallel 1-output functions.
        if let Object::Array(arr) = obj {
            let funcs: Vec<Function> = arr.iter().map(|o| Function::parse(reader, o)).collect();
            return Function::Stitching {
                domain: (0.0, 1.0),
                funcs,
                bounds: Vec::new(),
                encode: Vec::new(),
            }
            .into_parallel();
        }
        let dict = match obj {
            Object::Dict(d) => d.clone(),
            Object::Stream(s) => s.dict.clone(),
            _ => return Function::Identity,
        };
        let ftype = dict.get("FunctionType").and_then(int).unwrap_or(-1);
        match ftype {
            0 => parse_sampled(reader, &dict, obj),
            2 => parse_exponential(&dict),
            3 => parse_stitching(reader, &dict),
            4 => parse_postscript(reader, obj, &dict),
            _ => Function::Identity,
        }
    }

    /// Wrap a set of single-output functions so `eval` concatenates outputs.
    fn into_parallel(self) -> Function {
        // Encoded as a Stitching with empty bounds + a sentinel: reuse the
        // PostScript-less path by marking via empty `encode` and `bounds`.
        self
    }

    /// Evaluate the function, clamping inputs to the domain.
    pub fn eval(&self, inputs: &[f32]) -> Vec<f32> {
        match self {
            Function::Identity => inputs.to_vec(),
            Function::Exponential { domain, c0, c1, n } => {
                let x = clamp(inputs.first().copied().unwrap_or(0.0), domain.0, domain.1);
                let xn = x.powf(*n);
                let len = c0.len().max(c1.len());
                (0..len)
                    .map(|i| {
                        let a = c0.get(i).copied().unwrap_or(0.0);
                        let b = c1.get(i).copied().unwrap_or(0.0);
                        a + xn * (b - a)
                    })
                    .collect()
            }
            Function::Stitching {
                domain,
                funcs,
                bounds,
                encode,
            } => {
                // Empty bounds with multiple funcs ⇒ "parallel" array: run each
                // on the same input and concatenate one output apiece.
                if bounds.is_empty() && encode.is_empty() && funcs.len() != 1 {
                    let mut out = Vec::new();
                    for f in funcs {
                        out.extend(f.eval(inputs));
                    }
                    return out;
                }
                let x = clamp(inputs.first().copied().unwrap_or(0.0), domain.0, domain.1);
                let mut k = 0;
                while k < bounds.len() && x >= bounds[k] {
                    k += 1;
                }
                let lo = if k == 0 { domain.0 } else { bounds[k - 1] };
                let hi = if k < bounds.len() {
                    bounds[k]
                } else {
                    domain.1
                };
                let (e0, e1) = encode.get(k).copied().unwrap_or((0.0, 1.0));
                let xe = interp(x, lo, hi, e0, e1);
                funcs
                    .get(k)
                    .map(|f| f.eval(&[xe]))
                    .unwrap_or_else(|| vec![xe])
            }
            Function::Sampled {
                domain,
                range,
                size,
                bps,
                encode,
                decode,
                samples,
                n_out,
            } => eval_sampled(
                inputs, domain, range, size, *bps, encode, decode, samples, *n_out,
            ),
            Function::PostScript {
                domain,
                range,
                prog,
            } => {
                let mut stack: Vec<f32> = inputs
                    .iter()
                    .zip(domain.iter())
                    .map(|(&v, &(lo, hi))| clamp(v, lo, hi))
                    .collect();
                run_ps(prog, &mut stack);
                // Keep the last range.len() values, clamped to range.
                let n = range.len();
                let start = stack.len().saturating_sub(n);
                let mut out: Vec<f32> = stack[start..].to_vec();
                for (v, &(lo, hi)) in out.iter_mut().zip(range.iter()) {
                    *v = clamp(*v, lo, hi);
                }
                out
            }
        }
    }
}

fn parse_exponential(dict: &Dict) -> Function {
    let domain = pair(dict.get("Domain")).unwrap_or((0.0, 1.0));
    let c0 = floats(dict.get("C0")).unwrap_or_else(|| vec![0.0]);
    let c1 = floats(dict.get("C1")).unwrap_or_else(|| vec![1.0]);
    let n = dict.get("N").and_then(num).unwrap_or(1.0);
    Function::Exponential { domain, c0, c1, n }
}

fn parse_stitching(reader: &PdfReader, dict: &Dict) -> Function {
    let domain = pair(dict.get("Domain")).unwrap_or((0.0, 1.0));
    let funcs = match dict.get("Functions").map(|o| reader.resolve(o)) {
        Some(Object::Array(a)) => a.iter().map(|o| Function::parse(reader, o)).collect(),
        _ => Vec::new(),
    };
    let bounds = floats(dict.get("Bounds")).unwrap_or_default();
    let encode = pairs(dict.get("Encode"));
    Function::Stitching {
        domain,
        funcs,
        bounds,
        encode,
    }
}

fn parse_sampled(reader: &PdfReader, dict: &Dict, obj: &Object) -> Function {
    let domain = pairs(dict.get("Domain"));
    let range = pairs(dict.get("Range"));
    let size: Vec<usize> = floats(dict.get("Size"))
        .unwrap_or_default()
        .into_iter()
        .map(|f| f.max(1.0) as usize)
        .collect();
    let bps = dict.get("BitsPerSample").and_then(int).unwrap_or(8) as u32;
    let n_out = range.len();
    let encode = {
        let e = pairs(dict.get("Encode"));
        if e.len() == size.len() {
            e
        } else {
            size.iter()
                .map(|&s| (0.0, (s as f32 - 1.0).max(0.0)))
                .collect()
        }
    };
    let decode = {
        let d = pairs(dict.get("Decode"));
        if d.len() == range.len() {
            d
        } else {
            range.clone()
        }
    };
    let samples = match obj {
        Object::Stream(s) => reader.stream_data(s).unwrap_or_default(),
        _ => Vec::new(),
    };
    Function::Sampled {
        domain,
        range,
        size,
        bps,
        encode,
        decode,
        samples,
        n_out,
    }
}

#[allow(clippy::too_many_arguments)]
fn eval_sampled(
    inputs: &[f32],
    domain: &[(f32, f32)],
    _range: &[(f32, f32)],
    size: &[usize],
    bps: u32,
    encode: &[(f32, f32)],
    decode: &[(f32, f32)],
    samples: &[u8],
    n_out: usize,
) -> Vec<f32> {
    let m = size.len();
    if m == 0 || n_out == 0 {
        return vec![0.0; n_out];
    }
    // Encode inputs into sample-grid coordinates.
    let mut e = vec![0.0f32; m];
    for i in 0..m {
        let (d0, d1) = domain.get(i).copied().unwrap_or((0.0, 1.0));
        let (en0, en1) = encode
            .get(i)
            .copied()
            .unwrap_or((0.0, (size[i] as f32) - 1.0));
        let x = clamp(inputs.get(i).copied().unwrap_or(0.0), d0, d1);
        e[i] = clamp(
            interp(x, d0, d1, en0, en1),
            0.0,
            (size[i] as f32 - 1.0).max(0.0),
        );
    }
    let max_val = ((1u64 << bps) - 1) as f32;

    // Multilinear interpolation over the 2^m surrounding grid corners.
    let mut out = vec![0.0f32; n_out];
    let lo: Vec<usize> = e.iter().map(|&v| v.floor() as usize).collect();
    let frac: Vec<f32> = e.iter().map(|&v| v - v.floor()).collect();
    let corners = 1usize << m.min(8); // cap dimensionality defensively
    for corner in 0..corners {
        let mut weight = 1.0f32;
        let mut idx = vec![0usize; m];
        for (i, item) in idx.iter_mut().enumerate().take(m) {
            let bit = (corner >> i) & 1;
            let c = (lo[i] + bit).min(size[i] - 1);
            *item = c;
            weight *= if bit == 1 { frac[i] } else { 1.0 - frac[i] };
        }
        if weight == 0.0 {
            continue;
        }
        // Flatten grid index (first dimension varies fastest, per spec).
        let mut flat = 0usize;
        let mut stride = 1usize;
        for i in 0..m {
            flat += idx[i] * stride;
            stride *= size[i];
        }
        for (j, slot) in out.iter_mut().enumerate().take(n_out) {
            let raw = read_sample(samples, (flat * n_out + j) as u64, bps);
            *slot += weight * (raw as f32 / max_val);
        }
    }
    // Decode normalized [0,1] outputs into the Decode range.
    for (j, v) in out.iter_mut().enumerate().take(n_out) {
        let (de0, de1) = decode.get(j).copied().unwrap_or((0.0, 1.0));
        *v = de0 + *v * (de1 - de0);
    }
    out
}

/// Read the `idx`-th `bps`-bit big-endian sample from a packed buffer.
fn read_sample(data: &[u8], idx: u64, bps: u32) -> u64 {
    let bit_off = idx * bps as u64;
    let mut val = 0u64;
    for i in 0..bps as u64 {
        let b = bit_off + i;
        let byte = (b / 8) as usize;
        let bit = 7 - (b % 8) as u32;
        let set = data.get(byte).map(|&v| (v >> bit) & 1).unwrap_or(0);
        val = (val << 1) | set as u64;
    }
    val
}

// ---- Type 4 PostScript calculator -----------------------------------------

#[derive(Clone, Debug)]
pub enum PsTok {
    Num(f32),
    Op(PsOp),
    Proc(Vec<PsTok>),
}

#[derive(Clone, Copy, Debug)]
pub enum PsOp {
    Add,
    Sub,
    Mul,
    Div,
    Idiv,
    Mod,
    Neg,
    Abs,
    Sqrt,
    Sin,
    Cos,
    Atan,
    Exp,
    Ln,
    Log,
    Cvi,
    Cvr,
    Floor,
    Ceiling,
    Round,
    Truncate,
    Dup,
    Pop,
    Exch,
    Copy,
    Index,
    Roll,
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
    And,
    Or,
    Not,
    Xor,
    Bitshift,
    True,
    False,
    If,
    Ifelse,
}

fn parse_postscript(reader: &PdfReader, obj: &Object, dict: &Dict) -> Function {
    let domain = pairs(dict.get("Domain"));
    let range = pairs(dict.get("Range"));
    let body = match obj {
        Object::Stream(s) => reader.stream_data(s).unwrap_or_default(),
        _ => Vec::new(),
    };
    let toks = lex_ps(&body);
    // The whole program is wrapped in a single outer { } block.
    let prog = match toks.into_iter().next() {
        Some(PsTok::Proc(p)) => p,
        other => other.into_iter().collect(),
    };
    Function::PostScript {
        domain,
        range,
        prog,
    }
}

/// Nesting/recursion cap for Type 4 (PostScript calculator) functions. These
/// have no loops, so legitimate `{…}` nesting is shallow; the bound stops a
/// hostile `{{{{…` from overflowing the native stack during parse or eval.
const MAX_PS_DEPTH: u32 = 100;

fn lex_ps(data: &[u8]) -> Vec<PsTok> {
    let mut pos = 0;
    parse_ps_block(data, &mut pos, false, 0)
}

fn parse_ps_block(data: &[u8], pos: &mut usize, nested: bool, depth: u32) -> Vec<PsTok> {
    let mut out = Vec::new();
    while *pos < data.len() {
        let c = data[*pos];
        if c.is_ascii_whitespace() {
            *pos += 1;
            continue;
        }
        if c == b'{' {
            *pos += 1;
            let inner = if depth >= MAX_PS_DEPTH {
                // Too deep: consume the braces but drop the contents so we
                // neither recurse further nor mis-balance the outer scan.
                skip_ps_block(data, pos);
                Vec::new()
            } else {
                parse_ps_block(data, pos, true, depth + 1)
            };
            out.push(PsTok::Proc(inner));
            continue;
        }
        if c == b'}' {
            *pos += 1;
            if nested {
                return out;
            }
            continue;
        }
        if c == b'%' {
            while *pos < data.len() && data[*pos] != b'\n' {
                *pos += 1;
            }
            continue;
        }
        // token
        let start = *pos;
        while *pos < data.len() {
            let d = data[*pos];
            if d.is_ascii_whitespace() || d == b'{' || d == b'}' || d == b'%' {
                break;
            }
            *pos += 1;
        }
        let word = &data[start..*pos];
        if let Ok(s) = std::str::from_utf8(word) {
            if let Ok(n) = s.parse::<f32>() {
                out.push(PsTok::Num(n));
                continue;
            }
            if let Some(op) = ps_op(s) {
                out.push(PsTok::Op(op));
                continue;
            }
        }
        // unknown token: ignore
    }
    out
}

/// Consume a `{…}` block's bytes (already past the opening brace) without
/// building any tokens, tracking brace balance iteratively. Used when the
/// nesting cap is hit so the outer parser stays in sync.
fn skip_ps_block(data: &[u8], pos: &mut usize) {
    let mut balance = 1u32;
    while *pos < data.len() && balance > 0 {
        match data[*pos] {
            b'{' => balance += 1,
            b'}' => balance -= 1,
            b'%' => {
                while *pos < data.len() && data[*pos] != b'\n' {
                    *pos += 1;
                }
                continue;
            }
            _ => {}
        }
        *pos += 1;
    }
}

fn ps_op(s: &str) -> Option<PsOp> {
    use PsOp::*;
    Some(match s {
        "add" => Add,
        "sub" => Sub,
        "mul" => Mul,
        "div" => Div,
        "idiv" => Idiv,
        "mod" => Mod,
        "neg" => Neg,
        "abs" => Abs,
        "sqrt" => Sqrt,
        "sin" => Sin,
        "cos" => Cos,
        "atan" => Atan,
        "exp" => Exp,
        "ln" => Ln,
        "log" => Log,
        "cvi" => Cvi,
        "cvr" => Cvr,
        "floor" => Floor,
        "ceiling" => Ceiling,
        "round" => Round,
        "truncate" => Truncate,
        "dup" => Dup,
        "pop" => Pop,
        "exch" => Exch,
        "copy" => Copy,
        "index" => Index,
        "roll" => Roll,
        "eq" => Eq,
        "ne" => Ne,
        "gt" => Gt,
        "ge" => Ge,
        "lt" => Lt,
        "le" => Le,
        "and" => And,
        "or" => Or,
        "not" => Not,
        "xor" => Xor,
        "bitshift" => Bitshift,
        "true" => True,
        "false" => False,
        "if" => If,
        "ifelse" => Ifelse,
        _ => return None,
    })
}

fn run_ps(prog: &[PsTok], stack: &mut Vec<f32>) {
    run_ps_depth(prog, stack, 0);
}

fn run_ps_depth(prog: &[PsTok], stack: &mut Vec<f32>, depth: u32) {
    if depth >= MAX_PS_DEPTH {
        return;
    }
    // Pending procedure blocks for if/ifelse are tracked as index ranges.
    let mut procs: Vec<&[PsTok]> = Vec::new();
    for tok in prog {
        match tok {
            PsTok::Num(n) => stack.push(*n),
            PsTok::Proc(p) => procs.push(p),
            PsTok::Op(op) => apply_ps(*op, stack, &mut procs, depth),
        }
    }
}

fn pop(s: &mut Vec<f32>) -> f32 {
    s.pop().unwrap_or(0.0)
}
fn bool_f(b: bool) -> f32 {
    if b {
        1.0
    } else {
        0.0
    }
}

fn apply_ps(op: PsOp, s: &mut Vec<f32>, procs: &mut Vec<&[PsTok]>, depth: u32) {
    use PsOp::*;
    match op {
        Add => {
            let (b, a) = (pop(s), pop(s));
            s.push(a + b);
        }
        Sub => {
            let (b, a) = (pop(s), pop(s));
            s.push(a - b);
        }
        Mul => {
            let (b, a) = (pop(s), pop(s));
            s.push(a * b);
        }
        Div => {
            let (b, a) = (pop(s), pop(s));
            s.push(if b != 0.0 { a / b } else { 0.0 });
        }
        Idiv => {
            let (b, a) = (pop(s), pop(s));
            // `checked_div` covers both b==0 and the i64::MIN / -1 overflow.
            let r = (a as i64).checked_div(b as i64).unwrap_or(0);
            s.push(r as f32);
        }
        Mod => {
            let (b, a) = (pop(s), pop(s));
            let r = (a as i64).checked_rem(b as i64).unwrap_or(0);
            s.push(r as f32);
        }
        Neg => {
            let a = pop(s);
            s.push(-a);
        }
        Abs => {
            let a = pop(s);
            s.push(a.abs());
        }
        Sqrt => {
            let a = pop(s);
            s.push(a.max(0.0).sqrt());
        }
        Sin => {
            let a = pop(s);
            s.push(a.to_radians().sin());
        }
        Cos => {
            let a = pop(s);
            s.push(a.to_radians().cos());
        }
        Atan => {
            let (den, num) = (pop(s), pop(s));
            let mut deg = num.atan2(den).to_degrees();
            if deg < 0.0 {
                deg += 360.0;
            }
            s.push(deg);
        }
        Exp => {
            let (e, base) = (pop(s), pop(s));
            s.push(base.powf(e));
        }
        Ln => {
            let a = pop(s);
            s.push(a.max(1e-9).ln());
        }
        Log => {
            let a = pop(s);
            s.push(a.max(1e-9).log10());
        }
        Cvi | Truncate => {
            let a = pop(s);
            s.push(a.trunc());
        }
        Cvr => {}
        Floor => {
            let a = pop(s);
            s.push(a.floor());
        }
        Ceiling => {
            let a = pop(s);
            s.push(a.ceil());
        }
        Round => {
            let a = pop(s);
            s.push(a.round());
        }
        Dup => {
            let a = *s.last().unwrap_or(&0.0);
            s.push(a);
        }
        Pop => {
            pop(s);
        }
        Exch => {
            let (b, a) = (pop(s), pop(s));
            s.push(b);
            s.push(a);
        }
        Copy => {
            let n = pop(s) as i64;
            if n > 0 {
                let len = s.len();
                let n = (n as usize).min(len);
                for i in 0..n {
                    s.push(s[len - n + i]);
                }
            }
        }
        Index => {
            let n = pop(s) as i64;
            if n >= 0 && (n as usize) < s.len() {
                let v = s[s.len() - 1 - n as usize];
                s.push(v);
            } else {
                s.push(0.0);
            }
        }
        Roll => {
            let j = pop(s) as i64;
            let n = pop(s) as i64;
            if n > 0 && (n as usize) <= s.len() {
                let n = n as usize;
                let len = s.len();
                let slice = &mut s[len - n..];
                let j = ((j % n as i64) + n as i64) as usize % n;
                slice.rotate_right(j);
            }
        }
        Eq => {
            let (b, a) = (pop(s), pop(s));
            s.push(bool_f(a == b));
        }
        Ne => {
            let (b, a) = (pop(s), pop(s));
            s.push(bool_f(a != b));
        }
        Gt => {
            let (b, a) = (pop(s), pop(s));
            s.push(bool_f(a > b));
        }
        Ge => {
            let (b, a) = (pop(s), pop(s));
            s.push(bool_f(a >= b));
        }
        Lt => {
            let (b, a) = (pop(s), pop(s));
            s.push(bool_f(a < b));
        }
        Le => {
            let (b, a) = (pop(s), pop(s));
            s.push(bool_f(a <= b));
        }
        And => {
            let (b, a) = (pop(s), pop(s));
            s.push(((a as i64) & (b as i64)) as f32);
        }
        Or => {
            let (b, a) = (pop(s), pop(s));
            s.push(((a as i64) | (b as i64)) as f32);
        }
        Xor => {
            let (b, a) = (pop(s), pop(s));
            s.push(((a as i64) ^ (b as i64)) as f32);
        }
        Not => {
            let a = pop(s);
            s.push(bool_f(a == 0.0));
        }
        Bitshift => {
            let (sh, a) = (pop(s) as i64, pop(s) as i64);
            // Shift counts are attacker-controlled: a shift of >= 64 (or the
            // negation of a large-negative count) would panic in debug and is
            // UB-adjacent in release. Clamp to a well-defined 0 past 63 bits.
            let v = if sh >= 0 {
                if sh >= 64 {
                    0
                } else {
                    a << sh
                }
            } else {
                let r = sh.unsigned_abs();
                if r >= 64 {
                    if a < 0 {
                        -1
                    } else {
                        0
                    }
                } else {
                    a >> r
                }
            };
            s.push(v as f32);
        }
        True => s.push(1.0),
        False => s.push(0.0),
        If => {
            let proc = procs.pop();
            let cond = pop(s);
            if cond != 0.0 {
                if let Some(p) = proc {
                    run_ps_depth(p, s, depth + 1);
                }
            }
        }
        Ifelse => {
            let p2 = procs.pop();
            let p1 = procs.pop();
            let cond = pop(s);
            let chosen = if cond != 0.0 { p1 } else { p2 };
            if let Some(p) = chosen {
                run_ps_depth(p, s, depth + 1);
            }
        }
    }
}

// ---- small helpers ---------------------------------------------------------

fn clamp(v: f32, lo: f32, hi: f32) -> f32 {
    let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
    v.max(lo).min(hi)
}

fn interp(x: f32, x0: f32, x1: f32, y0: f32, y1: f32) -> f32 {
    if (x1 - x0).abs() < 1e-9 {
        y0
    } else {
        y0 + (x - x0) * (y1 - y0) / (x1 - x0)
    }
}

fn int(o: &Object) -> Option<i64> {
    match o {
        Object::Integer(n) => Some(*n),
        Object::Real(r) => Some(*r as i64),
        _ => None,
    }
}

fn num(o: &Object) -> Option<f32> {
    match o {
        Object::Integer(n) => Some(*n as f32),
        Object::Real(r) => Some(*r as f32),
        _ => None,
    }
}

fn floats(o: Option<&Object>) -> Option<Vec<f32>> {
    match o {
        Some(Object::Array(a)) => Some(a.iter().filter_map(num).collect()),
        _ => None,
    }
}

fn pair(o: Option<&Object>) -> Option<(f32, f32)> {
    match o {
        Some(Object::Array(a)) if a.len() >= 2 => Some((num(&a[0])?, num(&a[1])?)),
        _ => None,
    }
}

fn pairs(o: Option<&Object>) -> Vec<(f32, f32)> {
    match o {
        Some(Object::Array(a)) => a
            .chunks(2)
            .filter_map(|c| Some((num(c.first()?)?, num(c.get(1)?)?)))
            .collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exponential_interpolates_linearly() {
        let f = Function::Exponential {
            domain: (0.0, 1.0),
            c0: vec![0.0, 0.0, 0.0],
            c1: vec![1.0, 0.5, 0.0],
            n: 1.0,
        };
        let mid = f.eval(&[0.5]);
        assert!((mid[0] - 0.5).abs() < 1e-4);
        assert!((mid[1] - 0.25).abs() < 1e-4);
        assert!((mid[2] - 0.0).abs() < 1e-4);
    }

    #[test]
    fn postscript_arithmetic_and_control() {
        // { 2 mul 1 add } ⇒ y = 2x + 1
        let prog = lex_ps(b"{ 2 mul 1 add }");
        let prog = match prog.into_iter().next() {
            Some(PsTok::Proc(p)) => p,
            _ => unreachable!(),
        };
        let f = Function::PostScript {
            domain: vec![(0.0, 10.0)],
            range: vec![(0.0, 100.0)],
            prog,
        };
        assert!((f.eval(&[3.0])[0] - 7.0).abs() < 1e-4);
    }

    #[test]
    fn postscript_ifelse() {
        // { dup 5 gt { 100 } { 0 } ifelse } ⇒ 100 if x>5 else 0
        let prog = match lex_ps(b"{ dup 5 gt { 100 } { 0 } ifelse }")
            .into_iter()
            .next()
        {
            Some(PsTok::Proc(p)) => p,
            _ => unreachable!(),
        };
        let f = Function::PostScript {
            domain: vec![(0.0, 10.0)],
            range: vec![(0.0, 100.0)],
            prog,
        };
        assert_eq!(f.eval(&[8.0])[0], 100.0);
        assert_eq!(f.eval(&[2.0])[0], 0.0);
    }

    #[test]
    fn postscript_deeply_nested_braces_do_not_overflow_stack() {
        // 200k nested `{` used to recurse once per brace at parse time,
        // overflowing the native stack. It must now parse without crashing.
        let mut src = vec![b'{'; 200_001];
        src.extend(std::iter::repeat_n(b'}', 200_001));
        let _ = lex_ps(&src); // must return, not abort
    }

    #[test]
    fn postscript_extreme_operands_do_not_panic() {
        // bitshift >= 64, idiv/mod by zero and i64::MIN / -1 must all be
        // well-defined rather than panicking.
        for prog_src in [
            b"{ 1 1000 bitshift }".as_slice(),
            b"{ 1 -1000 bitshift }".as_slice(),
            b"{ 5 0 idiv }".as_slice(),
            b"{ 5 0 mod }".as_slice(),
        ] {
            let prog = match lex_ps(prog_src).into_iter().next() {
                Some(PsTok::Proc(p)) => p,
                _ => unreachable!(),
            };
            let f = Function::PostScript {
                domain: vec![(-1e30, 1e30)],
                range: vec![(-1e30, 1e30)],
                prog,
            };
            let _ = f.eval(&[1.0]); // must not panic
        }
    }

    #[test]
    fn stitching_picks_subfunction() {
        let left = Function::Exponential {
            domain: (0.0, 1.0),
            c0: vec![0.0],
            c1: vec![10.0],
            n: 1.0,
        };
        let right = Function::Exponential {
            domain: (0.0, 1.0),
            c0: vec![100.0],
            c1: vec![200.0],
            n: 1.0,
        };
        let f = Function::Stitching {
            domain: (0.0, 1.0),
            funcs: vec![left, right],
            bounds: vec![0.5],
            encode: vec![(0.0, 1.0), (0.0, 1.0)],
        };
        // x=0.25 is in [0,0.5] → encoded to 0.5 of left → 5.0
        assert!((f.eval(&[0.25])[0] - 5.0).abs() < 1e-3);
        // x=0.75 is in [0.5,1] → encoded to 0.5 of right → 150.0
        assert!((f.eval(&[0.75])[0] - 150.0).abs() < 1e-3);
    }
}
