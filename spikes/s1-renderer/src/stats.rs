//! Clocks, percentiles and a tiny JSON writer. Measurement plumbing only.

use std::path::PathBuf;

/// Host time in seconds: the clock CACurrentMediaTime, NSEvent timestamps
/// (system uptime) and MTLDrawable.presentedTime share (mach_absolute_time).
pub fn now_s() -> f64 {
    objc2_quartz_core::CACurrentMediaTime()
}

/// CPU time consumed by the calling thread, in seconds.
pub fn thread_cpu_s() -> f64 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut ts) };
    ts.tv_sec as f64 + ts.tv_nsec as f64 * 1e-9
}

/// User + system CPU time of the whole process, in seconds.
pub fn proc_cpu_s() -> f64 {
    let mut ru: libc::rusage = unsafe { std::mem::zeroed() };
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut ru) };
    let tv = |t: libc::timeval| t.tv_sec as f64 + t.tv_usec as f64 * 1e-6;
    tv(ru.ru_utime) + tv(ru.ru_stime)
}

pub fn is_main_thread() -> bool {
    unsafe { libc::pthread_main_np() == 1 }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Summary { pub n: usize, pub p50: f64, pub p90: f64, pub p99: f64, pub max: f64, pub mean: f64 }

pub fn summarize(v: &[f64]) -> Summary {
    if v.is_empty() { return Summary::default(); }
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let q = |p: f64| s[((s.len() - 1) as f64 * p).round() as usize];
    Summary { n: s.len(), p50: q(0.5), p90: q(0.9), p99: q(0.99), max: s[s.len() - 1], mean: s.iter().sum::<f64>() / s.len() as f64 }
}

impl Summary {
    pub fn json(&self) -> J {
        J::obj(vec![("n", J::N(self.n as f64)), ("p50", J::N(self.p50)), ("p90", J::N(self.p90)),
                    ("p99", J::N(self.p99)), ("max", J::N(self.max)), ("mean", J::N(self.mean))])
    }
    pub fn line(&self, unit: &str) -> String {
        format!("n={} p50={:.3}{u} p90={:.3}{u} p99={:.3}{u} max={:.3}{u}", self.n, self.p50, self.p90, self.p99, self.max, u = unit)
    }
}

/// Minimal JSON value, enough for result files.
pub enum J { N(f64), S(String), B(bool), A(Vec<J>), O(Vec<(String, J)>), Null }

impl J {
    pub fn obj(v: Vec<(&str, J)>) -> J { J::O(v.into_iter().map(|(k, v)| (k.to_string(), v)).collect()) }
    pub fn s(x: impl Into<String>) -> J { J::S(x.into()) }
    pub fn nums(v: &[f64]) -> J { J::A(v.iter().map(|x| J::N(*x)).collect()) }
    pub fn write(&self, out: &mut String) {
        match self {
            J::N(x) if x.is_finite() => { if x.fract() == 0.0 && x.abs() < 1e15 { out.push_str(&format!("{}", *x as i64)) } else { out.push_str(&format!("{:.4}", x)) } }
            J::N(_) | J::Null => out.push_str("null"),
            J::B(b) => out.push_str(if *b { "true" } else { "false" }),
            J::S(s) => { out.push('"'); for c in s.chars() { match c { '"' => out.push_str("\\\""), '\\' => out.push_str("\\\\"), c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)), c => out.push(c) } } out.push('"'); }
            J::A(v) => { out.push('['); for (i, x) in v.iter().enumerate() { if i > 0 { out.push(','); } x.write(out); } out.push(']'); }
            J::O(v) => { out.push_str("{\n"); for (i, (k, x)) in v.iter().enumerate() { if i > 0 { out.push_str(",\n"); } J::S(k.clone()).write(out); out.push_str(": "); x.write(out); } out.push_str("\n}"); }
        }
    }
    pub fn to_string(&self) -> String { let mut s = String::new(); self.write(&mut s); s }
}

pub fn results_dir() -> PathBuf {
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("results");
    std::fs::create_dir_all(&d).ok();
    d
}

pub fn write_result(name: &str, j: &J) -> PathBuf {
    let p = results_dir().join(format!("{name}.json"));
    std::fs::write(&p, j.to_string()).expect("write result");
    eprintln!("[s1] wrote {}", p.display());
    p
}

/// Small deterministic PRNG so runs are repeatable without a rand crate.
pub struct Lcg(pub u64);
impl Lcg {
    pub fn next(&mut self) -> u64 { self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407); self.0 >> 33 }
    pub fn range(&mut self, lo: f64, hi: f64) -> f64 { lo + (self.next() % 10_000) as f64 / 10_000.0 * (hi - lo) }
}
