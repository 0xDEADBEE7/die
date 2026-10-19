//! Minimal JSON output for check results (no serde in the spike).

pub trait J {
    fn j(&self) -> String;
}

pub struct Raw(pub String);

impl J for Raw {
    fn j(&self) -> String {
        if self.0.is_empty() { "null".into() } else { self.0.clone() }
    }
}

macro_rules! num {
    ($($t:ty),*) => { $(impl J for $t { fn j(&self) -> String { self.to_string() } })* };
}
num!(i32, i64, u16, u32, u64, usize, u128, bool);

impl J for f64 {
    fn j(&self) -> String {
        format!("{:.2}", self)
    }
}

impl J for str {
    fn j(&self) -> String {
        let mut o = String::from("\"");
        for c in self.chars() {
            match c {
                '"' => o.push_str("\\\""),
                '\\' => o.push_str("\\\\"),
                c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
                c => o.push(c),
            }
        }
        o + "\""
    }
}

impl J for String {
    fn j(&self) -> String {
        self.as_str().j()
    }
}

impl J for &str {
    fn j(&self) -> String {
        (*self).j()
    }
}

impl<T: J> J for Option<T> {
    fn j(&self) -> String {
        self.as_ref().map(|v| v.j()).unwrap_or_else(|| "null".into())
    }
}

impl<T: J> J for Vec<T> {
    fn j(&self) -> String {
        format!("[{}]", self.iter().map(|v| v.j()).collect::<Vec<_>>().join(","))
    }
}

pub fn obj(fields: &[(&str, String)]) -> String {
    format!("{{{}}}", fields.iter().map(|(k, v)| format!("{}:{}", k.j(), v)).collect::<Vec<_>>().join(","))
}

#[macro_export]
macro_rules! jo {
    ($($k:literal : $v:expr),* $(,)?) => { $crate::json::obj(&[$(($k, $crate::json::J::j(&$v))),*]) };
}
