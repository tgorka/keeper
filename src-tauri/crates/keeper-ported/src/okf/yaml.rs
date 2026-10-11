//! The YAML subset an OKF drive's configuration and frontmatter are written
//! in: block mappings and sequences, plain, single- and double-quoted
//! scalars, `|` and `>` block scalars with their chomping indicators, flow
//! sequences and mappings of scalars, comments.
//!
//! Written from the YAML 1.1 rules PyYAML follows and proved against the
//! drive's tools (`tests/fixtures/okf/`): with PyYAML installed they parse
//! through it, so its answer is the one the fixture records. Anchors,
//! aliases, tags and reserved indicators are outside the subset and are an
//! error; a timestamp stays a string.

/// A parsed node.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    List(Vec<Value>),
    /// In document order; a repeated key keeps its first place and its last
    /// value, as Python's `dict` does.
    Map(Vec<(String, Value)>),
}

impl Value {
    /// The value under `key`, when this is a mapping holding it.
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Map(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// The text of a string.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(text) => Some(text),
            _ => None,
        }
    }

    /// The items of a sequence.
    pub fn as_list(&self) -> Option<&[Value]> {
        match self {
            Value::List(items) => Some(items),
            _ => None,
        }
    }

    /// Python's truthiness, which the drive's tools test flags with.
    pub fn truthy(&self) -> bool {
        match self {
            Value::Null => false,
            Value::Bool(b) => *b,
            Value::Int(n) => *n != 0,
            Value::Float(f) => *f != 0.0,
            Value::Str(s) => !s.is_empty(),
            Value::List(items) => !items.is_empty(),
            Value::Map(entries) => !entries.is_empty(),
        }
    }

    /// A scalar as Python's `str()` writes it; `None` for a collection.
    pub fn scalar_text(&self) -> Option<String> {
        match self {
            Value::Null => Some("None".to_owned()),
            Value::Bool(true) => Some("True".to_owned()),
            Value::Bool(false) => Some("False".to_owned()),
            Value::Int(n) => Some(n.to_string()),
            Value::Float(f) => Some(format!("{f:?}")),
            Value::Str(s) => Some(s.clone()),
            Value::List(_) | Value::Map(_) => None,
        }
    }
}

/// Why a text is not in the subset, at a 1-based line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YamlError {
    pub line: usize,
    pub message: String,
}

impl std::fmt::Display for YamlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for YamlError {}

/// One source line: its text without the line break, and whether one
/// followed it.
#[derive(Debug, Clone)]
struct Line {
    text: String,
    broken: bool,
}

/// Parse `src`; an empty or comment-only text is `Null`.
pub fn parse(src: &str) -> Result<Value, YamlError> {
    let src = src.strip_prefix('\u{feff}').unwrap_or(src);
    let mut lines: Vec<Line> = Vec::new();
    let mut rest = src;
    while !rest.is_empty() {
        let (text, broken, next) = match rest.find('\n') {
            Some(at) => (&rest[..at], true, &rest[at + 1..]),
            None => (rest, false, ""),
        };
        lines.push(Line {
            text: text.strip_suffix('\r').unwrap_or(text).to_owned(),
            broken,
        });
        rest = next;
    }
    let mut parser = Parser { lines };
    let Some(first) = parser.significant(0) else {
        return Ok(Value::Null);
    };
    let indent = parser.indent(first)?;
    let (value, next) = parser.node(first, indent)?;
    if let Some(extra) = parser.significant(next) {
        return Err(parser.error(extra, "content after the document's root node"));
    }
    Ok(value)
}

struct Parser {
    lines: Vec<Line>,
}

/// Whether `text` is a comment or nothing.
fn blank(text: &str) -> bool {
    let trimmed = text.trim_start_matches([' ', '\t']);
    trimmed.is_empty() || trimmed.starts_with('#')
}

fn is_item(content: &str) -> bool {
    content == "-" || content.starts_with("- ")
}

impl Parser {
    fn error(&self, at: usize, message: &str) -> YamlError {
        YamlError {
            line: at + 1,
            message: message.to_owned(),
        }
    }

    /// The first line at or after `from` that holds content.
    fn significant(&self, from: usize) -> Option<usize> {
        (from..self.lines.len()).find(|&i| !blank(&self.lines[i].text))
    }

    fn indent(&self, at: usize) -> Result<usize, YamlError> {
        let text = &self.lines[at].text;
        let spaces = text.len() - text.trim_start_matches(' ').len();
        if text[spaces..].starts_with('\t') {
            return Err(self.error(at, "a tab in indentation"));
        }
        Ok(spaces)
    }

    fn content(&self, at: usize) -> &str {
        self.lines[at].text.trim_start_matches(' ')
    }

    /// The node starting at line `at`, indented `indent`: a sequence, a
    /// mapping or a scalar. Returns it and the line after it.
    fn node(&mut self, at: usize, indent: usize) -> Result<(Value, usize), YamlError> {
        let content = self.content(at);
        if is_item(content) {
            return self.sequence(at, indent);
        }
        if split_key(content).is_some() {
            return self.mapping(at, indent);
        }
        let text = content.to_owned();
        self.inline_value(at, &text, indent.saturating_sub(1))
    }

    fn sequence(&mut self, mut at: usize, indent: usize) -> Result<(Value, usize), YamlError> {
        let mut items = Vec::new();
        loop {
            let content = self.content(at).to_owned();
            let after = content[1..].trim_start_matches(' ');
            let next = if after.is_empty() || after.starts_with('#') {
                match self.significant(at + 1) {
                    Some(child) if self.indent(child)? > indent => {
                        let child_indent = self.indent(child)?;
                        let (value, next) = self.node(child, child_indent)?;
                        items.push(value);
                        next
                    }
                    _ => {
                        items.push(Value::Null);
                        at + 1
                    }
                }
            } else {
                // The item's content stands where the dash was: the line is
                // rewritten so the dash is indentation, and parsed as a node.
                let column = indent + (content.len() - after.len());
                self.lines[at].text = format!("{}{after}", " ".repeat(column));
                let (value, next) = self.node(at, column)?;
                items.push(value);
                next
            };
            match self.significant(next) {
                Some(line) if self.indent(line)? == indent && is_item(self.content(line)) => {
                    at = line;
                }
                Some(line) if self.indent(line)? > indent => {
                    return Err(self.error(line, "a line indented deeper than its sequence"));
                }
                _ => return Ok((Value::List(items), next)),
            }
        }
    }

    fn mapping(&mut self, mut at: usize, indent: usize) -> Result<(Value, usize), YamlError> {
        let mut entries: Vec<(String, Value)> = Vec::new();
        loop {
            let content = self.content(at).to_owned();
            let Some((raw_key, rest)) = split_key(&content) else {
                return Err(self.error(at, "a line in a mapping that is not `key: value`"));
            };
            let key = unquote_key(raw_key).map_err(|message| self.error(at, &message))?;
            let rest = rest.trim_start_matches(' ');
            let (value, next) = if rest.is_empty() || rest.starts_with('#') {
                match self.significant(at + 1) {
                    Some(child) if self.indent(child)? > indent => {
                        let child_indent = self.indent(child)?;
                        self.node(child, child_indent)?
                    }
                    // A sequence may sit at its key's own indentation.
                    Some(child)
                        if self.indent(child)? == indent && is_item(self.content(child)) =>
                    {
                        self.sequence(child, indent)?
                    }
                    _ => (Value::Null, at + 1),
                }
            } else {
                let rest = rest.to_owned();
                self.inline_value(at, &rest, indent)?
            };
            match entries.iter_mut().find(|(k, _)| *k == key) {
                Some(entry) => entry.1 = value,
                None => entries.push((key, value)),
            }
            match self.significant(next) {
                Some(line) if self.indent(line)? == indent => {
                    if is_item(self.content(line)) {
                        return Ok((Value::Map(entries), next));
                    }
                    at = line;
                }
                Some(line) if self.indent(line)? > indent => {
                    return Err(self.error(line, "a line indented deeper than its mapping"));
                }
                _ => return Ok((Value::Map(entries), next)),
            }
        }
    }

    /// A value written on line `at` after its key or dash: a block scalar,
    /// a quoted or flow value, or a plain scalar that may go on over the
    /// lines indented deeper than `indent`.
    fn inline_value(
        &mut self,
        at: usize,
        text: &str,
        indent: usize,
    ) -> Result<(Value, usize), YamlError> {
        if let Some(header) = block_header(text) {
            return self.block_scalar(at, indent, header);
        }
        let first = text.chars().next().unwrap_or(' ');
        if matches!(first, '&' | '*' | '!' | '%' | '@' | '`') {
            return Err(self.error(
                at,
                "anchors, aliases, tags and reserved indicators are outside this subset",
            ));
        }
        if matches!(first, '"' | '\'' | '[' | '{') {
            let (value, used) = flow_value(text).map_err(|message| self.error(at, &message))?;
            let rest = text[used..].trim_start_matches(' ');
            if !(rest.is_empty() || rest.starts_with('#')) {
                return Err(self.error(at, "text after a quoted or flow value"));
            }
            return Ok((value, at + 1));
        }
        let mut words = vec![strip_comment(text).to_owned()];
        let mut next = at + 1;
        while let Some(line) = self.significant(next) {
            if self.indent(line)? <= indent {
                break;
            }
            let content = self.content(line);
            if split_key(content).is_some() || is_item(content) {
                return Err(self.error(line, "a collection under a plain scalar"));
            }
            words.push(strip_comment(content).to_owned());
            next = line + 1;
        }
        Ok((plain_scalar(&words.join(" ")), next))
    }

    fn block_scalar(
        &mut self,
        at: usize,
        indent: usize,
        (folded, chomp): (bool, Chomp),
    ) -> Result<(Value, usize), YamlError> {
        let mut body: Vec<(String, bool)> = Vec::new();
        let mut content_indent = None;
        let mut next = at + 1;
        while next < self.lines.len() {
            let line = &self.lines[next];
            let spaces = line.text.len() - line.text.trim_start_matches(' ').len();
            if line.text.trim().is_empty() {
                body.push((String::new(), line.broken));
                next += 1;
                continue;
            }
            let column = *content_indent.get_or_insert(spaces);
            if spaces < column || spaces <= indent {
                break;
            }
            body.push((line.text[column..].to_owned(), line.broken));
            next += 1;
        }
        // Blank lines after the last content line belong to the chomping,
        // and those after the block to whatever follows it.
        let content_end = body.iter().rposition(|(text, _)| !text.is_empty());
        let (lines, trailing) = match content_end {
            Some(end) => body.split_at(end + 1),
            None => body.split_at(0),
        };
        let mut text = String::new();
        if folded {
            let mut previous_blank = true;
            for (line, _) in lines {
                if line.is_empty() {
                    text.push('\n');
                    previous_blank = true;
                    continue;
                }
                if !previous_blank {
                    text.push(' ');
                }
                text.push_str(line);
                previous_blank = false;
            }
        } else {
            let joined: Vec<&str> = lines.iter().map(|(line, _)| line.as_str()).collect();
            text = joined.join("\n");
        }
        let last_broken = lines.last().is_some_and(|(_, broken)| *broken);
        match chomp {
            Chomp::Strip => {}
            Chomp::Clip => {
                if last_broken {
                    text.push('\n');
                }
            }
            Chomp::Keep => {
                if last_broken {
                    text.push('\n');
                }
                text.extend(trailing.iter().filter(|(_, broken)| *broken).map(|_| '\n'));
            }
        }
        Ok((Value::Str(text), next))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Chomp {
    Strip,
    Clip,
    Keep,
}

/// `|` or `>`, with an optional `-` or `+`, and nothing but a comment after.
fn block_header(text: &str) -> Option<(bool, Chomp)> {
    let text = strip_comment(text);
    let mut chars = text.chars();
    let folded = match chars.next()? {
        '|' => false,
        '>' => true,
        _ => return None,
    };
    let chomp = match chars.as_str() {
        "" => Chomp::Clip,
        "-" => Chomp::Strip,
        "+" => Chomp::Keep,
        _ => return None,
    };
    Some((folded, chomp))
}

/// A plain scalar's text without a trailing ` #` comment.
fn strip_comment(text: &str) -> &str {
    let mut previous = ' ';
    for (at, c) in text.char_indices() {
        if c == '#' && (previous == ' ' || previous == '\t') {
            return text[..at].trim_end();
        }
        previous = c;
    }
    text.trim_end()
}

/// `key: rest` split at the key's colon: a colon followed by a space or the
/// line's end, outside quotes.
fn split_key(content: &str) -> Option<(&str, &str)> {
    if content.starts_with('#') || is_item(content) {
        return None;
    }
    let bytes = content.as_bytes();
    let key_end = match bytes.first()? {
        quote @ (b'"' | b'\'') => {
            let close = content[1..].find(*quote as char)? + 1;
            let after = content[close + 1..].trim_start_matches(' ');
            if !after.starts_with(':') {
                return None;
            }
            content.len() - after.len()
        }
        b'[' | b'{' => return None,
        _ => {
            let mut found = None;
            for (at, c) in content.char_indices() {
                if c == ':' && matches!(bytes.get(at + 1), None | Some(b' ') | Some(b'\t')) {
                    found = Some(at);
                    break;
                }
                if c == '#' && at > 0 && bytes[at - 1] == b' ' {
                    return None;
                }
            }
            found?
        }
    };
    Some((&content[..key_end], &content[key_end + 1..]))
}

fn unquote_key(raw: &str) -> Result<String, String> {
    let raw = raw.trim_end();
    match raw.chars().next() {
        Some('"' | '\'') => {
            let (value, _) = flow_value(raw)?;
            Ok(value.scalar_text().unwrap_or_default())
        }
        _ => Ok(raw.to_owned()),
    }
}

/// A quoted scalar or a flow collection at the start of `text`, and how
/// many bytes it used.
fn flow_value(text: &str) -> Result<(Value, usize), String> {
    let mut chars = text.char_indices().peekable();
    let (_, open) = chars.next().ok_or("an empty value")?;
    match open {
        '"' => {
            let mut out = String::new();
            while let Some((at, c)) = chars.next() {
                match c {
                    '"' => return Ok((Value::Str(out), at + 1)),
                    '\\' => {
                        let (_, escaped) = chars.next().ok_or("an unfinished escape")?;
                        match escaped {
                            'n' => out.push('\n'),
                            't' => out.push('\t'),
                            'r' => out.push('\r'),
                            '0' => out.push('\0'),
                            '\\' => out.push('\\'),
                            '"' => out.push('"'),
                            '/' => out.push('/'),
                            ' ' => out.push(' '),
                            'x' | 'u' | 'U' => {
                                let digits = match escaped {
                                    'x' => 2,
                                    'u' => 4,
                                    _ => 8,
                                };
                                let mut code = String::new();
                                for _ in 0..digits {
                                    code.push(chars.next().ok_or("an unfinished escape")?.1);
                                }
                                let point =
                                    u32::from_str_radix(&code, 16).map_err(|_| "a bad escape")?;
                                out.push(char::from_u32(point).ok_or("a bad escape")?);
                            }
                            _ => return Err("an escape outside this subset".to_owned()),
                        }
                    }
                    _ => out.push(c),
                }
            }
            Err("an unclosed double quote".to_owned())
        }
        '\'' => {
            let mut out = String::new();
            while let Some((at, c)) = chars.next() {
                if c == '\'' {
                    if chars.peek().map(|(_, n)| *n) == Some('\'') {
                        chars.next();
                        out.push('\'');
                        continue;
                    }
                    return Ok((Value::Str(out), at + 1));
                }
                out.push(c);
            }
            Err("an unclosed single quote".to_owned())
        }
        '[' | '{' => {
            let close = if open == '[' { ']' } else { '}' };
            let mut depth = 0usize;
            let mut quote: Option<char> = None;
            let mut end = None;
            for (at, c) in text.char_indices() {
                match quote {
                    Some(q) if c == q => quote = None,
                    Some(_) => {}
                    None => match c {
                        '"' | '\'' => quote = Some(c),
                        '[' | '{' => depth += 1,
                        ']' | '}' => {
                            depth -= 1;
                            if depth == 0 {
                                if c != close {
                                    return Err("mismatched brackets".to_owned());
                                }
                                end = Some(at);
                                break;
                            }
                        }
                        _ => {}
                    },
                }
            }
            let end = end.ok_or("an unclosed flow collection")?;
            let items = split_flow(&text[1..end]);
            let value = if open == '[' {
                Value::List(items.into_iter().map(flow_item).collect::<Result<_, _>>()?)
            } else {
                let mut entries = Vec::new();
                for item in items {
                    let (key, value) = match split_key(item) {
                        Some((key, value)) => (unquote_key(key)?, flow_item(value.trim())?),
                        None => (unquote_key(item)?, Value::Null),
                    };
                    entries.push((key, value));
                }
                Value::Map(entries)
            };
            Ok((value, end + 1))
        }
        _ => Err("not a quoted or flow value".to_owned()),
    }
}

/// The comma-separated items of a flow collection's inside, nested
/// collections and quotes kept whole; an empty trailing item dropped.
fn split_flow(inner: &str) -> Vec<&str> {
    let mut items = Vec::new();
    let mut depth = 0usize;
    let mut quote: Option<char> = None;
    let mut start = 0;
    for (at, c) in inner.char_indices() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None => match c {
                '"' | '\'' => quote = Some(c),
                '[' | '{' => depth += 1,
                ']' | '}' => depth = depth.saturating_sub(1),
                ',' if depth == 0 => {
                    items.push(inner[start..at].trim());
                    start = at + 1;
                }
                _ => {}
            },
        }
    }
    let last = inner[start..].trim();
    if !last.is_empty() {
        items.push(last);
    }
    items
}

fn flow_item(item: &str) -> Result<Value, String> {
    match item.chars().next() {
        Some('"' | '\'' | '[' | '{') => flow_value(item).map(|(value, _)| value),
        _ => Ok(plain_scalar(item)),
    }
}

/// A plain scalar resolved as YAML 1.1 resolves it: null, a boolean, an
/// integer, a float, else a string.
fn plain_scalar(text: &str) -> Value {
    let text = text.trim();
    match text {
        "" | "~" | "null" | "Null" | "NULL" => return Value::Null,
        "true" | "True" | "TRUE" | "yes" | "Yes" | "YES" | "on" | "On" | "ON" => {
            return Value::Bool(true)
        }
        "false" | "False" | "FALSE" | "no" | "No" | "NO" | "off" | "Off" | "OFF" => {
            return Value::Bool(false)
        }
        _ => {}
    }
    let digits = text.strip_prefix(['-', '+']).unwrap_or(text);
    if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
        if let Ok(n) = text.parse::<i64>() {
            return Value::Int(n);
        }
    }
    let float_shape = digits.contains('.')
        && digits
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_digit() || b == b'.')
        && digits
            .bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'e' | b'E' | b'-' | b'+'));
    if float_shape {
        if let Ok(f) = text.parse::<f64>() {
            return Value::Float(f);
        }
    }
    Value::Str(text.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(text: &str) -> Value {
        Value::Str(text.to_owned())
    }

    #[test]
    fn block_scalars_fold_and_chomp_as_yaml_does() {
        let parsed = parse("a: >-\n  one\n  two\n\n  three\nb: |\n  x\n  y\n\nc: |+\n  z\n\n")
            .expect("parses");
        assert_eq!(parsed.get("a"), Some(&s("one two\nthree")));
        assert_eq!(parsed.get("b"), Some(&s("x\ny\n")));
        assert_eq!(parsed.get("c"), Some(&s("z\n\n")));
    }

    #[test]
    fn scalars_resolve_as_yaml_one_one_does() {
        let parsed =
            parse("a: no\nb: 12\nc: ~\nd: 1.5\ne: 'it''s'\nf: \"q\\\"\" # c\ng: x # y\nh: a#b\n")
                .expect("parses");
        assert_eq!(parsed.get("a"), Some(&Value::Bool(false)));
        assert_eq!(parsed.get("b"), Some(&Value::Int(12)));
        assert_eq!(parsed.get("c"), Some(&Value::Null));
        assert_eq!(parsed.get("d"), Some(&Value::Float(1.5)));
        assert_eq!(parsed.get("e"), Some(&s("it's")));
        assert_eq!(parsed.get("f"), Some(&s("q\"")));
        assert_eq!(parsed.get("g"), Some(&s("x")));
        assert_eq!(parsed.get("h"), Some(&s("a#b")));
    }

    #[test]
    fn what_the_subset_leaves_out_is_an_error() {
        for text in [
            "a: &x 1\n",
            "a: *x\n",
            "a: !tag x\n",
            "a: [1, 2\n",
            "a: 'open\n",
            "a:\n\t- x\n",
        ] {
            assert!(parse(text).is_err(), "{text:?}");
        }
    }
}
