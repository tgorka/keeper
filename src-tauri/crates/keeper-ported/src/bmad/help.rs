// Reads BMAD-METHOD's `_bmad/_config/bmad-help.csv`, the catalogue the
// installer writes and the `bmad-help` skill reads, at tag v6.12.0
// (05bfbd46d00766ec88eb9b42e76be2c575d64d7b). MIT, Copyright (c) 2025 BMad
// Code, LLC; see `UPSTREAM.md`. No script reads it upstream: the skill's model
// does, so this is the file's grammar, not a port of code.

//! The catalogue of a BMAD install: one row per skill action, each with its
//! menu code, its phase and the actions it comes after and before.
//!
//! Nothing in the file is a key. A skill has a row per action, and one with no
//! action may repeat (`bmad-brainstorming`); a menu code names one row per
//! module that uses it (`CE` is `bmad-create-epics-and-stories` and
//! `gds-create-epics-and-stories`). So rows stay in file order, each is
//! addressed by its line, and every lookup answers with every row it matches.
//! A `_meta` row describes a module, not a skill, and is kept apart.

use std::fmt;

/// The header the catalogue starts with, column for column.
pub const COLUMNS: [&str; 13] = [
    "module",
    "skill",
    "display-name",
    "menu-code",
    "description",
    "action",
    "args",
    "phase",
    "preceded-by",
    "followed-by",
    "required",
    "output-location",
    "outputs",
];

/// The skill name a module's own row carries.
pub const META: &str = "_meta";

/// Why the catalogue does not read. `Display` is the sentence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelpError {
    message: String,
}

impl HelpError {
    fn at(line: usize, why: impl fmt::Display) -> Self {
        Self {
            message: format!("bmad-help.csv line {line}: {why}"),
        }
    }
}

impl fmt::Display for HelpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for HelpError {}

/// One row: a skill's action, or a module's `_meta` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelpRow {
    /// The file line the row starts on, 1-based (the header is line 1).
    pub line: usize,
    pub module: String,
    pub skill: String,
    pub display_name: String,
    pub menu_code: String,
    pub description: String,
    /// Empty for a skill's only action.
    pub action: String,
    pub args: String,
    pub phase: String,
    /// A `skill` or `skill:action` this one comes after; empty for none.
    pub preceded_by: String,
    /// A `skill` or `skill:action` this one comes before; empty for none.
    pub followed_by: String,
    pub required: bool,
    pub output_location: String,
    pub outputs: String,
}

impl HelpRow {
    /// `skill:action`, or the bare skill for a row with no action.
    pub fn token(&self) -> String {
        if self.action.is_empty() {
            self.skill.clone()
        } else {
            format!("{}:{}", self.skill, self.action)
        }
    }
}

/// A read catalogue.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Help {
    /// Every skill's row, in file order.
    pub rows: Vec<HelpRow>,
    /// Every `_meta` row, in file order.
    pub meta: Vec<HelpRow>,
}

/// The file's RFC 4180 records, each with the line it starts on.
fn records(text: &str) -> Result<Vec<(usize, Vec<String>)>, HelpError> {
    let mut out = Vec::new();
    let mut fields = Vec::new();
    let mut field = String::new();
    let (mut line, mut start) = (1, 1);
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    let mut any = false;
    while let Some(c) = chars.next() {
        any = true;
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    field.push('"');
                }
                '"' => quoted = false,
                '\n' => {
                    line += 1;
                    field.push(c);
                }
                _ => field.push(c),
            }
            continue;
        }
        match c {
            '"' if field.is_empty() => quoted = true,
            ',' => fields.push(std::mem::take(&mut field)),
            '\r' if chars.peek() == Some(&'\n') => {}
            '\n' => {
                fields.push(std::mem::take(&mut field));
                out.push((start, std::mem::take(&mut fields)));
                line += 1;
                start = line;
                any = false;
            }
            _ => field.push(c),
        }
    }
    if quoted {
        return Err(HelpError::at(start, "a quoted field is never closed"));
    }
    if any {
        fields.push(field);
        out.push((start, fields));
    }
    Ok(out)
}

/// Read the catalogue's text.
pub fn parse(text: &str) -> Result<Help, HelpError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut records = records(text)?.into_iter();
    let Some((_, header)) = records.next() else {
        return Err(HelpError::at(1, "the file is empty"));
    };
    if header != COLUMNS {
        return Err(HelpError::at(
            1,
            format!("the header is not {}", COLUMNS.join(",")),
        ));
    }
    let mut help = Help::default();
    for (line, fields) in records {
        if fields.len() == 1 && fields[0].is_empty() {
            continue;
        }
        let Ok(fields) = <[String; 13]>::try_from(fields) else {
            return Err(HelpError::at(line, "a row has 13 fields"));
        };
        let [module, skill, display_name, menu_code, description, action, args, phase, preceded_by, followed_by, required, output_location, outputs] =
            fields;
        let required = match required.as_str() {
            "true" => true,
            "false" | "" => false,
            other => {
                return Err(HelpError::at(
                    line,
                    format!("required is true or false, not {other}"),
                ))
            }
        };
        let row = HelpRow {
            line,
            module,
            skill,
            display_name,
            menu_code,
            description,
            action,
            args,
            phase,
            preceded_by,
            followed_by,
            required,
            output_location,
            outputs,
        };
        if row.skill == META {
            help.meta.push(row);
        } else {
            help.rows.push(row);
        }
    }
    Ok(help)
}

impl Help {
    /// Every row of `skill`.
    pub fn by_skill(&self, skill: &str) -> Vec<&HelpRow> {
        self.rows.iter().filter(|row| row.skill == skill).collect()
    }

    /// Every row a `skill:action` token names.
    pub fn by_action(&self, skill: &str, action: &str) -> Vec<&HelpRow> {
        self.rows
            .iter()
            .filter(|row| row.skill == skill && row.action == action)
            .collect()
    }

    /// Every row whose menu code is `code`, cased as the file has it.
    pub fn by_code(&self, code: &str) -> Vec<&HelpRow> {
        self.rows
            .iter()
            .filter(|row| !row.menu_code.is_empty() && row.menu_code == code)
            .collect()
    }

    /// Every row `name` names: a `skill:action` token, else a menu code,
    /// else a skill.
    pub fn named(&self, name: &str) -> Vec<&HelpRow> {
        if let Some((skill, action)) = name.split_once(':') {
            return self.by_action(skill, action);
        }
        let coded = self.by_code(name);
        if coded.is_empty() {
            self.by_skill(name)
        } else {
            coded
        }
    }

    /// The `preceded-by` and `followed-by` tokens no row is, with the line
    /// that names each.
    pub fn dangling(&self) -> Vec<(usize, &str)> {
        let names = |token: &str| match token.split_once(':') {
            Some((skill, action)) => !self.by_action(skill, action).is_empty(),
            None => !self.by_skill(token).is_empty(),
        };
        self.rows
            .iter()
            .flat_map(|row| {
                [row.preceded_by.as_str(), row.followed_by.as_str()]
                    .into_iter()
                    .filter(|token| !token.is_empty() && !names(token))
                    .map(move |token| (row.line, token))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// This repository's own install, as the installer wrote it.
    fn installed() -> Help {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../_bmad/_config/bmad-help.csv"
        );
        parse(&std::fs::read_to_string(path).expect("bmad-help.csv")).expect("it reads")
    }

    /// The phase graph over the real catalogue: rows by line, every match
    /// of a lookup, `_meta` apart, and the two tokens no row is.
    #[test]
    fn the_catalogue_reads_as_its_rows() {
        let help = installed();
        let lines = |rows: Vec<&HelpRow>| rows.iter().map(|row| row.line).collect::<Vec<_>>();

        let build = help.by_skill("bmad-build");
        assert_eq!(lines(build.clone()), [27]);
        assert_eq!(
            (build[0].preceded_by.as_str(), build[0].followed_by.as_str()),
            ("bmad-sprint-planning", "bmad-code-review")
        );
        assert!(build[0].required);
        assert_eq!(lines(help.by_skill("bmad-sprint-planning")), [20, 26]);
        let analysis = help.by_action("bmad-agent-builder", "quality-analysis");
        assert_eq!(lines(analysis.clone()), [5]);
        assert_eq!(analysis[0].token(), "bmad-agent-builder:quality-analysis");
        // A quoted field holding a comma reads whole.
        assert!(analysis[0].description.contains("structure, cohesion"));

        assert_eq!(lines(help.named("CA")), [24]);
        assert_eq!(help.named("CA")[0].skill, "bmad-architecture");
        let ce = help.named("CE");
        assert_eq!(
            ce.iter().map(|row| row.skill.as_str()).collect::<Vec<_>>(),
            [
                "bmad-create-epics-and-stories",
                "gds-create-epics-and-stories"
            ]
        );
        assert_eq!(lines(help.named("bmad-build")), [27]);
        assert_eq!(lines(help.named("bmad-agent-builder:build-process")), [4]);
        assert!(help.named("bmad-no-such-skill").is_empty());
        assert_eq!(lines(help.by_skill("bmad-brainstorming")), [21, 33, 45]);

        assert!(help.rows.iter().all(|row| row.skill != META));
        assert_eq!(help.meta.len(), 6);
        assert_eq!(help.meta[0].line, 2);

        let dangling: Vec<&str> = help.dangling().into_iter().map(|(_, t)| t).collect();
        assert_eq!(dangling, ["bmad-create-story:create", "bmad-dev-story"]);
    }

    /// RFC 4180: doubled quotes, a line break inside quotes, CRLF, and a
    /// row of the wrong width or an unclosed quote refused by line.
    #[test]
    fn the_file_is_read_as_rfc_4180() {
        let header = COLUMNS.join(",");
        let text = format!(
            "{header}\r\nM,s,S,X,\"say \"\"hi\"\",\nthen go\",a,,anytime,,,true,,\r\nM,t,T,Y,d,,,anytime,s:a,,false,,\n"
        );
        let help = parse(&text).expect("reads");
        assert_eq!(help.rows[0].description, "say \"hi\",\nthen go");
        assert_eq!(help.rows[1].line, 4);
        assert!(help.dangling().is_empty());
        assert_eq!(
            parse(&format!("{header}\nM,s\n")).map_err(|e| e.to_string()),
            Err("bmad-help.csv line 2: a row has 13 fields".to_owned())
        );
        assert_eq!(
            parse(&format!("{header}\nM,\"s\n")).map_err(|e| e.to_string()),
            Err("bmad-help.csv line 2: a quoted field is never closed".to_owned())
        );
        assert!(parse("module,skill\n").is_err());
    }
}
