//! The few pieces of Python's `str`, truthiness and `json` that BMAD's scripts
//! lean on, so each port keeps their exact behaviour instead of Rust's near
//! equivalents (`str.strip()` strips more than `str::trim`, `splitlines()`
//! splits on more than `lines()`).

use toml::Value;

/// `str.isspace()` for one character: Rust's `White_Space` plus the four
/// information separators Python also counts.
pub(crate) fn is_space(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

/// `str.strip()` with no argument.
pub(crate) fn strip(text: &str) -> &str {
    text.trim_matches(is_space)
}

/// `str.split()` with no argument: runs of whitespace, empty pieces dropped.
pub(crate) fn split(text: &str) -> impl Iterator<Item = &str> {
    text.split(is_space).filter(|piece| !piece.is_empty())
}

/// `str.splitlines()`: every line boundary Python knows, `\r\n` as one, and
/// no empty last line for a trailing boundary.
pub(crate) fn splitlines(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        let boundary = matches!(
            c,
            '\n' | '\r'
                | '\u{0b}'
                | '\u{0c}'
                | '\u{1c}'
                | '\u{1d}'
                | '\u{1e}'
                | '\u{85}'
                | '\u{2028}'
                | '\u{2029}'
        );
        if !boundary {
            continue;
        }
        lines.push(&text[start..at]);
        start = at + c.len_utf8();
        if c == '\r' {
            if let Some(&(next, '\n')) = chars.peek() {
                chars.next();
                start = next + 1;
            }
        }
    }
    if start < text.len() {
        lines.push(&text[start..]);
    }
    lines
}

/// Python's `bool(value)` for a value `tomllib` would have built.
pub(crate) fn truthy(value: &Value) -> bool {
    match value {
        Value::String(text) => !text.is_empty(),
        Value::Integer(number) => *number != 0,
        Value::Float(number) => *number != 0.0,
        Value::Boolean(on) => *on,
        Value::Datetime(_) => true,
        Value::Array(items) => !items.is_empty(),
        Value::Table(table) => !table.is_empty(),
    }
}

/// Python's `==` between two values `tomllib` would have built: numbers
/// compare by exact value across `bool`, `int` and `float` (`1 == 1.0 ==
/// True`, but `2**53 + 1 != float(2**53)`), and containers item by item.
pub(crate) fn equal(a: &Value, b: &Value) -> bool {
    fn integer(value: &Value) -> Option<i64> {
        match value {
            Value::Integer(number) => Some(*number),
            Value::Boolean(on) => Some(i64::from(*on)),
            _ => None,
        }
    }
    /// An integer and a float are equal only when the float is that exact
    /// integer; casting the integer to a float would round it.
    fn integer_is_float(integer: i64, float: f64) -> bool {
        const TWO_63: f64 = 9_223_372_036_854_775_808.0;
        float.fract() == 0.0 && (-TWO_63..TWO_63).contains(&float) && float as i64 == integer
    }
    match (a, b) {
        (Value::String(x), Value::String(y)) => x == y,
        (Value::Datetime(x), Value::Datetime(y)) => x == y,
        (Value::Float(x), Value::Float(y)) => x == y,
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(x, y)| equal(x, y))
        }
        (Value::Table(x), Value::Table(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(key, x)| y.get(key).is_some_and(|y| equal(x, y)))
        }
        (Value::Float(float), other) | (other, Value::Float(float)) => {
            integer(other).is_some_and(|integer| integer_is_float(integer, *float))
        }
        _ => matches!((integer(a), integer(b)), (Some(x), Some(y)) if x == y),
    }
}

/// Whether Python's `str.isprintable()` accepts `c`.
fn is_printable(c: char) -> bool {
    // The table is sorted, so a character is inside a range when it is one of
    // the bounds or falls between a start and its end (an odd insertion point).
    match super::py_printable::NOT_PRINTABLE.binary_search(&u32::from(c)) {
        Ok(_) => false,
        Err(at) => at % 2 == 0,
    }
}

/// `repr()` of a `str`: single quotes unless the text holds one and no double
/// quote; `\\`, `\t`, `\n`, `\r` and the quote escaped, and every character
/// `str.isprintable()` refuses as `\xhh`, `\uhhhh` or `\Uhhhhhhhh`.
pub(crate) fn repr(text: &str) -> String {
    let quote = if text.contains('\'') && !text.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::with_capacity(text.len() + 2);
    out.push(quote);
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if is_printable(c) => out.push(c),
            c => {
                let code = u32::from(c);
                let escaped = if code < 0x100 {
                    format!("\\x{code:02x}")
                } else if code < 0x1_0000 {
                    format!("\\u{code:04x}")
                } else {
                    format!("\\U{code:08x}")
                };
                out.push_str(&escaped);
            }
        }
    }
    out.push(quote);
    out
}

/// A JSON string as `json.dumps` writes it. With `ensure_ascii` (its default)
/// everything outside printable ASCII, DEL included, is `\uXXXX`, astral
/// characters as a surrogate pair; without it only `"`, `\` and the C0
/// controls are escaped.
pub(crate) fn json_string(text: &str, ensure_ascii: bool) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            ' '..='~' => out.push(c),
            c if !ensure_ascii && u32::from(c) >= 0x20 => out.push(c),
            c => {
                let mut units = [0u16; 2];
                for unit in c.encode_utf16(&mut units) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
        }
    }
    out.push('"');
    out
}

/// `repr()` of a `float`, which `json.dumps` writes for a finite one: the
/// shortest digits that read back, fixed-point from 1e-4 up to 1e16 (with
/// `.0` when integral), scientific with a signed two-digit-or-more exponent
/// otherwise.
pub(crate) fn float_repr(number: f64) -> String {
    let scientific = format!("{number:e}");
    let (mantissa, exponent) = scientific.split_once('e').unwrap_or((&scientific, "0"));
    let exponent: i32 = exponent.parse().unwrap_or(0);
    let (sign, mantissa) = match mantissa.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", mantissa),
    };
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    if (-4..16).contains(&exponent) {
        let point = exponent + 1;
        if point <= 0 {
            let zeros = "0".repeat(point.unsigned_abs() as usize);
            return format!("{sign}0.{zeros}{digits}");
        }
        let point = point as usize;
        let whole: String = if digits.len() > point {
            digits[..point].to_owned()
        } else {
            format!("{digits}{}", "0".repeat(point - digits.len()))
        };
        let fraction = digits.get(point..).filter(|f| !f.is_empty()).unwrap_or("0");
        return format!("{sign}{whole}.{fraction}");
    }
    let (first, rest) = digits.split_at(1);
    let rest = if rest.is_empty() {
        String::new()
    } else {
        format!(".{rest}")
    };
    let exponent_sign = if exponent < 0 { '-' } else { '+' };
    format!(
        "{sign}{first}{rest}e{exponent_sign}{:02}",
        exponent.unsigned_abs()
    )
}

/// A float as `json.dumps` writes it with its default `allow_nan`.
pub(crate) fn json_float(number: f64) -> String {
    if number.is_nan() {
        "NaN".to_owned()
    } else if number == f64::INFINITY {
        "Infinity".to_owned()
    } else if number == f64::NEG_INFINITY {
        "-Infinity".to_owned()
    } else {
        float_repr(number)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splitlines_and_strip_follow_python() {
        assert_eq!(splitlines("a\r\nb\rc\n"), ["a", "b", "c"]);
        assert_eq!(
            splitlines("a\u{2028}b\u{1c}c\n\nd"),
            ["a", "b", "c", "", "d"]
        );
        assert!(splitlines("").is_empty());
        assert_eq!(strip("\u{1f} x \u{a0}"), "x");
        assert_eq!(
            split(" a \n b\u{1e}c ").collect::<Vec<_>>(),
            ["a", "b", "c"]
        );
    }

    /// Every expected string below is what Python 3.14 printed for the same
    /// input (`repr(s)`, `json.dumps(s)`, `json.dumps(s, ensure_ascii=False)`,
    /// `repr(f)`).
    #[test]
    fn repr_and_json_follow_python() {
        assert_eq!(repr("noequals"), "'noequals'");
        assert_eq!(repr("it's"), "\"it's\"");
        assert_eq!(repr("a\nb\u{1}"), "'a\\nb\\x01'");
        assert_eq!(repr("bad\u{200b}field"), "'bad\\u200bfield'");
        assert_eq!(repr("a\u{2028}b"), "'a\\u2028b'");
        assert_eq!(repr("x\u{e0001}y"), "'x\\U000e0001y'");
        assert_eq!(repr("\u{7f}\u{a0}é🧭"), "'\\x7f\\xa0é🧭'");
        assert_eq!(repr("pua\u{e000}"), "'pua\\ue000'");

        assert_eq!(
            json_string("a/ż🧭\"\n", true),
            "\"a/\\u017c\\ud83e\\udded\\\"\\n\""
        );
        assert_eq!(
            json_string("a\u{7f}b/.memlog.md", true),
            "\"a\\u007fb/.memlog.md\""
        );
        assert_eq!(
            json_string("a\u{7f}b\u{1f}é\u{2028}", false),
            "\"a\u{7f}b\\u001fé\u{2028}\""
        );
    }

    #[test]
    fn floats_print_as_python_prints_them() {
        for (number, python) in [
            (1e100, "1e+100"),
            (1e-7, "1e-07"),
            (1e16, "1e+16"),
            (1e15, "1000000000000000.0"),
            (123_456_789_012_345.6, "123456789012345.6"),
            (0.0001, "0.0001"),
            (0.00001, "1e-05"),
            (-0.0, "-0.0"),
            (3.0, "3.0"),
            (0.1, "0.1"),
            (1.5e300, "1.5e+300"),
            (2.5e-300, "2.5e-300"),
            (5e-324, "5e-324"),
            (-1234.5678, "-1234.5678"),
            (1e22, "1e+22"),
        ] {
            assert_eq!(float_repr(number), python, "{number:e}");
        }
        assert_eq!(json_float(f64::NAN), "NaN");
        assert_eq!(json_float(f64::INFINITY), "Infinity");
        assert_eq!(json_float(f64::NEG_INFINITY), "-Infinity");
    }

    /// `9007199254740993 == 9007199254740992.0` is false in Python (the float
    /// is 2**53 exactly); `9007199254740992 == 9007199254740992.0` and
    /// `True == 1.0` are true.
    #[test]
    fn numbers_compare_by_exact_value() {
        let int = Value::Integer;
        let float = Value::Float;
        assert!(!equal(
            &int(9_007_199_254_740_993),
            &float(9_007_199_254_740_992.0)
        ));
        assert!(equal(
            &int(9_007_199_254_740_992),
            &float(9_007_199_254_740_992.0)
        ));
        assert!(equal(
            &float(9_007_199_254_740_992.0),
            &int(9_007_199_254_740_992)
        ));
        assert!(equal(&Value::Boolean(true), &float(1.0)));
        assert!(equal(&Value::Boolean(false), &int(0)));
        assert!(!equal(&int(i64::MAX), &float(9_223_372_036_854_775_808.0)));
        assert!(!equal(&int(1), &float(1.5)));
        assert!(!equal(&float(f64::NAN), &float(f64::NAN)));
    }
}
