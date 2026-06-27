//! Throughput / stress benchmark — generates marketing numbers.
//!
//! Builds a representative one-page report (header band + ~35 lines of shaped,
//! subsetted Unicode text — the expensive part of real PDF generation) over and
//! over, measuring single-thread throughput and how it scales across threads
//! (the core is `Send`, so independent documents run truly in parallel).
//!
//! Run: `cargo run --release -p pdf --example stress`
//!   optional arg: total document count (otherwise auto-calibrated to ~1.5s).

use std::thread;
use std::time::{Duration, Instant};

use pdf::Document;

/// Font embedded so the benchmark needs no I/O per document.
const FONT: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/fonts/Roboto-Regular.ttf"
));

/// A representative page: a colored header band, a title and ~35 body lines
/// (accents + kerning pairs so shaping/subsetting do real work).
fn build_doc() -> Vec<u8> {
    let mut doc = Document::new();
    let font = doc.add_font(FONT.to_vec()).expect("font");

    let page = doc.add_page();
    // Header band.
    page.content()
        .save_state()
        .set_fill_rgb(0.10, 0.12, 0.18)
        .rect(0.0, 800.0, 595.0, 42.0)
        .fill()
        .restore_state();

    // Title.
    page.text(font, 22.0)
        .fill(1.0, 1.0, 1.0)
        .at(40.0, 812.0)
        .show("Quarterly Report — Açúcar & Café S.A.");

    // Body lines.
    let t = page.text(font, 11.0);
    t.fill(0.12, 0.12, 0.14);
    let body = [
        "Receita líquida cresceu 18% no trimestre, impulsionada pela",
        "expansão internacional e pela linha premium de cafés especiais.",
        "AVATAR To Wave — kerning pairs exercise GPOS shaping at scale.",
        "Margem operacional avançou para 24,6%, ante 21,1% no ano anterior.",
        "Fluxo de caixa livre somou R$ 412 milhões, alta de 27% a/a.",
    ];
    for i in 0..35 {
        let line = body[i % body.len()];
        t.at(40.0, 770.0 - i as f64 * 17.0).show(line);
    }

    doc.to_bytes().expect("build")
}

/// Build `n` documents on the current thread; return total bytes produced.
fn build_n(n: usize) -> usize {
    (0..n).map(|_| build_doc().len()).sum()
}

fn fmt_rate(docs: usize, dur: Duration) -> f64 {
    docs as f64 / dur.as_secs_f64()
}

/// Peak resident set size of this process, in bytes (Unix `getrusage`).
/// `ru_maxrss` is bytes on macOS and kilobytes on Linux.
#[cfg(unix)]
fn peak_rss_bytes() -> u64 {
    // SAFETY: zeroed `rusage` is a valid value; the call only reads/writes it.
    unsafe {
        let mut ru: libc::rusage = std::mem::zeroed();
        if libc::getrusage(libc::RUSAGE_SELF, &mut ru) != 0 {
            return 0;
        }
        let maxrss = ru.ru_maxrss as u64;
        if cfg!(target_os = "macos") {
            maxrss
        } else {
            maxrss * 1024
        }
    }
}
#[cfg(not(unix))]
fn peak_rss_bytes() -> u64 {
    0
}

fn main() {
    let cores = thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);

    // --- Warm up (page caches, allocator) ---
    let sample = build_doc();
    let doc_kb = sample.len() as f64 / 1024.0;
    for _ in 0..20 {
        std::hint::black_box(build_doc());
    }

    // --- Calibrate total work so the single-thread run takes ~1.5s ---
    let docs: usize = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or_else(|| {
            let t0 = Instant::now();
            let mut built = 0usize;
            while t0.elapsed() < Duration::from_millis(400) {
                std::hint::black_box(build_doc());
                built += 1;
            }
            let rate = fmt_rate(built, t0.elapsed());
            (rate * 1.5).max(200.0) as usize
        });

    println!();
    println!("rust-pdf throughput benchmark");
    println!("  machine cores      : {cores}");
    println!("  page               : 1 page, header + title + 35 shaped text lines");
    println!("  output PDF size    : {doc_kb:.1} KB/doc");
    println!("  documents per run  : {docs}");
    println!();

    // --- Single-thread baseline ---
    let t0 = Instant::now();
    let bytes = build_n(docs);
    let single = t0.elapsed();
    let single_rate = fmt_rate(docs, single);
    let mb_s = (bytes as f64 / 1_048_576.0) / single.as_secs_f64();
    println!("single-thread:");
    println!(
        "  {single_rate:>9.0} docs/sec   |  {:.2} ms/doc  |  {mb_s:.1} MB/sec",
        1000.0 / single_rate
    );
    println!();

    // --- Multi-thread scaling ---
    let mut counts: Vec<usize> = Vec::new();
    let mut c = 1;
    while c < cores {
        counts.push(c);
        c *= 2;
    }
    counts.push(cores);
    counts.dedup();

    println!("multi-thread scaling (same total work, split across threads):");
    println!("  threads      docs/sec     speedup     efficiency");
    for &nt in &counts {
        let per = docs / nt;
        let total = per * nt;
        let t0 = Instant::now();
        let handles: Vec<_> = (0..nt)
            .map(|_| thread::spawn(move || build_n(per)))
            .collect();
        for h in handles {
            let _ = h.join().expect("thread");
        }
        let dur = t0.elapsed();
        let rate = fmt_rate(total, dur);
        let speedup = rate / single_rate;
        let eff = speedup / nt as f64 * 100.0;
        println!("  {nt:>5}    {rate:>10.0}      {speedup:>5.2}x       {eff:>5.0}%");
    }
    println!();

    // --- Memory: peak RSS after sustaining full load on every core ---
    let peak = peak_rss_bytes();
    if peak > 0 {
        let peak_mb = peak as f64 / 1_048_576.0;
        let per_thread_kb = peak as f64 / cores as f64 / 1024.0;
        println!("memory:");
        println!("  {peak_mb:>9.1} MB peak RSS  |  ~{per_thread_kb:.0} KB per concurrent document (worst case)");
        println!("  no garbage collector, no GC pauses — memory is freed deterministically");
        println!();
    }
}
