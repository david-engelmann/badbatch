//! Python text behaviour the crate's documents depend on.
//!
//! Badbatch's formats are defined in terms of Python string
//! operations, so the crate carries a string type that holds a
//! Python string exactly, as the sequence of its code points,
//! lone surrogates included. The fixture reader can produce a
//! lone surrogate from an escape, the command line can produce
//! one from an argument byte that cannot stand in UTF-8, and
//! both are observable in the refusal lines the tool prints, so
//! neither may be collapsed to a replacement character. Ordering
//! of the type is code point order, which is Python's string
//! order.

use std::cmp::Ordering;
use std::fmt;

use crate::printable_table::{DECIMAL_RANGES, NON_PRINTABLE_RANGES};

/// A Python string, held as the sequence of its code points.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PyStr(pub Vec<u32>);

impl PyStr {
    /// The string a Rust string holds, code point for code point.
    /// The name mirrors the oracle's constructor on purpose. It
    /// is an infallible code point copy, not a parse, so the
    /// fallible FromStr trait is not the shape it should carry.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(text: &str) -> PyStr {
        PyStr(text.chars().map(|ch| ch as u32).collect())
    }

    /// The string Python's argument decoding produces from raw
    /// bytes, where a byte that cannot stand in UTF-8 becomes the
    /// surrogate for that byte alone and decoding resumes at the
    /// next byte.
    pub fn from_surrogateescape(bytes: &[u8]) -> PyStr {
        let mut points = Vec::new();
        let mut index = 0;
        while index < bytes.len() {
            let lead = bytes[index];
            if lead < 0x80 {
                points.push(lead as u32);
                index += 1;
                continue;
            }
            let width = match lead {
                0xC2..=0xDF => 2,
                0xE0..=0xEF => 3,
                0xF0..=0xF4 => 4,
                _ => 0,
            };
            let mut decoded = None;
            if width > 0 && index + width <= bytes.len() {
                let rest = &bytes[index + 1..index + width];
                if rest.iter().all(|byte| (0x80..=0xBF).contains(byte)) {
                    let second = rest[0];
                    let in_range = match lead {
                        0xE0 => (0xA0..=0xBF).contains(&second),
                        0xED => (0x80..=0x9F).contains(&second),
                        0xF0 => (0x90..=0xBF).contains(&second),
                        0xF4 => (0x80..=0x8F).contains(&second),
                        _ => true,
                    };
                    if in_range {
                        let mut point = (lead as u32) & (0x7F >> width);
                        for byte in rest {
                            point = (point << 6) | (*byte as u32 & 0x3F);
                        }
                        decoded = Some(point);
                    }
                }
            }
            match decoded {
                Some(point) => {
                    points.push(point);
                    index += width;
                }
                None => {
                    points.push(0xDC00 + lead as u32);
                    index += 1;
                }
            }
        }
        PyStr(points)
    }

    /// The number of code points in the string.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the string is empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Whether the string opens with the given code point.
    pub fn starts_with_point(&self, point: u32) -> bool {
        self.0.first() == Some(&point)
    }

    /// The UTF-8 bytes of the string. Content the tool hashes is
    /// read from strictly decoded text and so never holds a lone
    /// surrogate, which Python's encoder would refuse outright.
    pub fn to_utf8_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for &point in &self.0 {
            match char::from_u32(point) {
                Some(ch) => {
                    let mut buffer = [0u8; 4];
                    out.extend_from_slice(ch.encode_utf8(&mut buffer).as_bytes());
                }
                None => {
                    // A lone surrogate, written in its three byte
                    // form. Unreachable for decoded content.
                    out.push(0xE0 | (point >> 12) as u8);
                    out.push(0x80 | ((point >> 6) & 0x3F) as u8);
                    out.push(0x80 | (point & 0x3F) as u8);
                }
            }
        }
        out
    }

    /// The string as it appears when Python writes it to a
    /// stream whose error handler replaces surrogates with their
    /// escaped form. Every refusal line the tool prints is such a
    /// write, so a lone surrogate stands as a backslash, the
    /// letter u, and four lowercase hex digits.
    pub fn to_display(&self) -> String {
        let mut out = String::new();
        for &point in &self.0 {
            if (0xD800..=0xDFFF).contains(&point) {
                out.push_str(&format!("\\u{point:04x}"));
            } else if let Some(ch) = char::from_u32(point) {
                out.push(ch);
            }
        }
        out
    }

    /// The string with Python whitespace removed from both ends,
    /// as str.strip reads it.
    pub fn strip(&self) -> PyStr {
        let mut start = 0;
        let mut end = self.0.len();
        while start < end && is_whitespace(self.0[start]) {
            start += 1;
        }
        while end > start && is_whitespace(self.0[end - 1]) {
            end -= 1;
        }
        PyStr(self.0[start..end].to_vec())
    }

    /// The fields of the string under Python's argument free
    /// split, which splits on runs of whitespace and yields no
    /// empty fields, as str.split reads it with no argument.
    pub fn split_whitespace(&self) -> Vec<PyStr> {
        let mut fields = Vec::new();
        let mut current: Vec<u32> = Vec::new();
        for &point in &self.0 {
            if is_whitespace(point) {
                if !current.is_empty() {
                    fields.push(PyStr(std::mem::take(&mut current)));
                }
            } else {
                current.push(point);
            }
        }
        if !current.is_empty() {
            fields.push(PyStr(current));
        }
        fields
    }

    /// The parts of the string cut at every occurrence of one
    /// code point, empty parts kept, as str.split reads it with
    /// that separator.
    pub fn split_point(&self, separator: u32) -> Vec<PyStr> {
        let mut parts = Vec::new();
        let mut current: Vec<u32> = Vec::new();
        for &point in &self.0 {
            if point == separator {
                parts.push(PyStr(std::mem::take(&mut current)));
            } else {
                current.push(point);
            }
        }
        parts.push(PyStr(current));
        parts
    }

    /// The strings joined by the given separator, as str.join.
    pub fn join(parts: &[PyStr], separator: &PyStr) -> PyStr {
        let mut out: Vec<u32> = Vec::new();
        for (index, part) in parts.iter().enumerate() {
            if index > 0 {
                out.extend_from_slice(&separator.0);
            }
            out.extend_from_slice(&part.0);
        }
        PyStr(out)
    }
}

impl fmt::Display for PyStr {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.to_display())
    }
}

impl From<&str> for PyStr {
    fn from(text: &str) -> PyStr {
        PyStr::from_str(text)
    }
}

impl PartialEq<&str> for PyStr {
    fn eq(&self, other: &&str) -> bool {
        self.0.iter().copied().eq(other.chars().map(|ch| ch as u32))
    }
}

/// Whether a code point is Python whitespace, the set both
/// str.strip and str.split read. The ranges are the ones the
/// oracle runtime reports for its Unicode version.
pub fn is_whitespace(point: u32) -> bool {
    matches!(
        point,
        0x09..=0x0D
            | 0x1C..=0x20
            | 0x85
            | 0xA0
            | 0x1680
            | 0x2000..=0x200A
            | 0x2028..=0x2029
            | 0x202F
            | 0x205F
            | 0x3000
    )
}

/// Whether a code point prints as itself in a Python repr, the
/// ASCII space excepted as the repr rule excepts it.
pub fn is_printable(point: u32) -> bool {
    if point == 0x20 {
        return true;
    }
    NON_PRINTABLE_RANGES
        .binary_search_by(|&(lo, hi)| {
            if point < lo {
                Ordering::Greater
            } else if point > hi {
                Ordering::Less
            } else {
                Ordering::Equal
            }
        })
        .is_err()
}

/// Whether a code point is a Unicode decimal digit, the digit
/// class of the oracle's text patterns.
pub fn is_decimal(point: u32) -> bool {
    decimal_range(point).is_some()
}

/// The range of the decimal table holding one code point.
fn decimal_range(point: u32) -> Option<(u32, u32)> {
    DECIMAL_RANGES
        .binary_search_by(|&(lo, hi)| {
            if point < lo {
                Ordering::Greater
            } else if point > hi {
                Ordering::Less
            } else {
                Ordering::Equal
            }
        })
        .ok()
        .map(|index| DECIMAL_RANGES[index])
}

/// The value of a Unicode decimal digit, or None where the
/// code point is not one. Every block in the decimal table
/// holds its ten digits in order from its zero, and the one
/// merged range in the table holds five such blocks in a row,
/// so the value is the offset within the block of ten.
pub fn decimal_value(point: u32) -> Option<u32> {
    decimal_range(point).map(|(lo, _)| (point - lo) % 10)
}

/// The repr of one code point inside a quoted Python string,
/// without the quotes.
fn repr_body(point: u32, quote: u32, out: &mut String) {
    if point == quote || point == 0x5C {
        out.push('\\');
        out.push(char::from_u32(point).unwrap_or('\u{FFFD}'));
    } else if point == 0x0A {
        out.push_str("\\n");
    } else if point == 0x0D {
        out.push_str("\\r");
    } else if point == 0x09 {
        out.push_str("\\t");
    } else if is_printable(point) {
        if let Some(ch) = char::from_u32(point) {
            out.push(ch);
        }
    } else if point <= 0xFF {
        out.push_str(&format!("\\x{point:02x}"));
    } else if point <= 0xFFFF {
        out.push_str(&format!("\\u{point:04x}"));
    } else {
        out.push_str(&format!("\\U{point:08x}"));
    }
}

/// The repr of a Python string, as repr reads it, quote choice
/// and escapes and all.
pub fn py_repr(text: &PyStr) -> String {
    let has_single = text.0.contains(&0x27);
    let has_double = text.0.contains(&0x22);
    let quote: u32 = if has_single && !has_double {
        0x22
    } else {
        0x27
    };
    let mut out = String::new();
    out.push(char::from_u32(quote).unwrap_or('\''));
    for &point in &text.0 {
        repr_body(point, quote, &mut out);
    }
    out.push(char::from_u32(quote).unwrap_or('\''));
    out
}

/// The repr of a Python float, the shortest decimal form that
/// round trips, as repr reads it. Whole values keep one place
/// after the point, and the exponent form serves values whose
/// decimal exponent is smaller than minus 4 or at least 16,
/// with at least two exponent digits.
pub fn py_float_repr(value: f64) -> String {
    if value == 0.0 {
        return if value.is_sign_negative() {
            "-0.0".to_string()
        } else {
            "0.0".to_string()
        };
    }
    if !value.is_finite() {
        // Unreachable for the values the plan prints, which the
        // unit interval check has already bounded.
        return format!("{value}");
    }
    let negative = value < 0.0;
    let magnitude = if negative { -value } else { value };
    // Rust's exponent form is the shortest digit string that
    // round trips, in the shape d.ddde<exp>.
    let scientific = format!("{magnitude:e}");
    let (mantissa, exponent_text) = scientific
        .split_once('e')
        .expect("exponent form always carries an exponent");
    let exponent: i32 = exponent_text.parse().expect("exponent is a number");
    let digits: String = mantissa.chars().filter(|ch| *ch != '.').collect();
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    if !(-4..16).contains(&exponent) {
        out.push_str(&digits[..1]);
        if digits.len() > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        out.push_str(&format!("e{exponent:+03}"));
        return out;
    }
    if exponent >= 0 {
        let point_at = (exponent + 1) as usize;
        if digits.len() > point_at {
            out.push_str(&digits[..point_at]);
            out.push('.');
            out.push_str(&digits[point_at..]);
        } else {
            out.push_str(&digits);
            for _ in digits.len()..point_at {
                out.push('0');
            }
            out.push_str(".0");
        }
    } else {
        out.push_str("0.");
        for _ in 0..(-exponent - 1) {
            out.push('0');
        }
        out.push_str(&digits);
    }
    out
}
