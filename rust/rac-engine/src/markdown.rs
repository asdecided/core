//! Markdown -> Product extraction (PORT-CONTRACT.d/03, parity landmine #2).
//!
//! Bespoke CommonMark *block-boundary* tokenizer reproducing markdown-it-py
//! 4.2.0's `"commonmark"` preset for the consumed surface only: `heading_open`
//! (tag + 0-based start line) and `inline` (raw content + start line), plus
//! the oracle's section-slicing walk on top (`src/rac/core/markdown.py`).
//!
//! The bake-off of PORT-CONTRACT decision 4 resolves to *bespoke*: external
//! markdown crates are not in the dependency allowlist and the consumed
//! surface is block boundaries only, so the block rules of markdown-it-py
//! (`code fence blockquote hr list reference html_block heading lheading
//! paragraph`) are ported line-by-line from the oracle venv's source.
//!
//! Python-semantics helpers (`py_strip`, `py_casefold`, `py_repr_str`,
//! Unicode `\d`) come from `crate::pycompat` per the layer-1 contract.

use std::collections::{BTreeSet, HashMap};
use std::io::Read;
use std::sync::OnceLock;

use crate::pycompat::{is_re_digit, py_casefold, py_is_space, py_repr_str, py_strip};

// ---------------------------------------------------------------------------
// Limits (src/rac/core/limits.py)
// ---------------------------------------------------------------------------

pub const DEFAULT_MAX_FILE_BYTES: u128 = 1 << 20; // 1 MiB
pub const MAX_FIELD_CHARS: usize = 256 << 10; // 262144 code points per section
pub const MAX_CAPTURED_LINES: usize = 50_000; // non-blank captured lines

/// Unicode Nd run starts (every decimal-digit run begins at digit value 0);
/// digit value = (cp - run_start) % 10. Generated from unicodedata (CPython
/// 3.11). Membership itself is checked with `pycompat::is_re_digit`.
const ND_RUN_STARTS: &[u32] = &[
    48, 1632, 1776, 1984, 2406, 2534, 2662, 2790, 2918, 3046, 3174, 3302, 3430, 3558, 3664, 3792,
    3872, 4160, 4240, 6112, 6160, 6470, 6608, 6784, 6800, 6992, 7088, 7232, 7248, 42528, 43216,
    43264, 43472, 43504, 43600, 44016, 65296, 66720, 68912, 69734, 69872, 69942, 70096, 70384,
    70736, 70864, 71248, 71360, 71472, 71904, 72016, 72784, 73040, 73120, 92768, 92864, 93008,
    120782, 123200, 123632, 125264, 130032,
];

fn nd_digit_value(c: char) -> Option<u32> {
    if !is_re_digit(c) {
        return None;
    }
    let cp = c as u32;
    let idx = match ND_RUN_STARTS.binary_search(&cp) {
        Ok(i) => i,
        Err(0) => return None,
        Err(i) => i - 1,
    };
    Some((cp - ND_RUN_STARTS[idx]) % 10)
}

/// Python `int(str)` (decimal): strips the Unicode whitespace set (which,
/// unlike `str.strip()`, excludes U+001C-U+001F — verified empirically, and
/// it matches Rust `str::trim`), accepts an optional sign, Unicode
/// Nd digits, and single underscores strictly between digits.
///
/// Python ints are unbounded; magnitudes beyond i128 SATURATE to i128::MAX
/// (sign applied afterwards). Every consumer only cares about "positive and
/// at least N" thresholds far below the saturation point, so the clamp is
/// unobservable.
pub fn py_parse_int(raw: &str) -> Option<i128> {
    let s = raw.trim();
    let mut chars = s.chars().peekable();
    let mut neg = false;
    match chars.peek() {
        Some('+') => {
            chars.next();
        }
        Some('-') => {
            neg = true;
            chars.next();
        }
        _ => {}
    }
    let mut value: i128 = 0;
    let mut last_was_digit = false;
    let mut any_digit = false;
    for c in chars {
        if c == '_' {
            if !last_was_digit {
                return None;
            }
            last_was_digit = false;
            continue;
        }
        let d = nd_digit_value(c)?;
        value = value
            .saturating_mul(10)
            .saturating_add(d as i128);
        last_was_digit = true;
        any_digit = true;
    }
    if !any_digit || !last_was_digit {
        return None;
    }
    Some(if neg { -value } else { value })
}

/// `max_file_bytes()` computed from a raw env value (None = unset).
pub fn max_file_bytes_from(raw: Option<&str>) -> u128 {
    if let Some(raw) = raw {
        if let Some(v) = py_parse_int(raw) {
            if v > 0 {
                return v as u128;
            }
        }
    }
    DEFAULT_MAX_FILE_BYTES
}

/// The per-file byte cap, honoring `DECIDED_MAX_FILE_BYTES` (REQ-001).
pub fn max_file_bytes() -> u128 {
    match std::env::var("DECIDED_MAX_FILE_BYTES") {
        Ok(v) => max_file_bytes_from(Some(&v)),
        Err(_) => DEFAULT_MAX_FILE_BYTES,
    }
}

/// True when `text` exceeds `cap` UTF-8 bytes. (Equivalent to the oracle's
/// chars-fast-path formulation: both reduce to `utf8_len > cap`.)
fn exceeds_byte_cap(text: &str, cap: u128) -> bool {
    text.len() as u128 > cap
}

// ---------------------------------------------------------------------------
// Data model (mirrors decided.core.models for the markdown-owned fields)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub severity: &'static str,
    pub code: &'static str,
    pub message: String,
    pub line: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Requirement {
    pub id: String,
    pub text: String,
    pub line: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MalformedRequirement {
    pub raw: String,
    pub line: i64,
    pub bad_id: Option<String>,
    pub empty_text: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SearchSection {
    pub heading: String,
    pub lines: Vec<String>,
}

/// `split_frontmatter` result (decided.core.frontmatter.FrontmatterSplit).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrontmatterSplit {
    pub raw: Option<String>,
    pub body: String,
    pub line_offset: usize,
    pub unterminated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Product {
    pub title: Option<String>,
    pub extra_title_lines: Vec<i64>,
    pub problem: Option<String>,
    pub requirements: Vec<Requirement>,
    pub malformed_requirements: Vec<MalformedRequirement>,
    pub success_metrics: Vec<String>,
    pub risks: Vec<String>,
    /// Insertion-ordered `{normalized heading -> joined body}` map.
    pub sections: Vec<(String, String)>,
    pub search_sections: Vec<SearchSection>,
    pub has_problem_section: bool,
    pub has_requirements_section: bool,
    pub has_metrics_section: bool,
    pub has_risks_section: bool,
    pub source_path: String,
    /// Raw frontmatter text for the frontmatter module (WS2) to parse into
    /// metadata; None when the document has no (terminated) frontmatter block.
    pub frontmatter_raw: Option<String>,
    /// Only the `malformed-frontmatter` unterminated-opener issue lives here;
    /// `parse_frontmatter` issues are the frontmatter module's concern.
    pub metadata_issues: Vec<Issue>,
    pub parse_issues: Vec<Issue>,
}

/// Split a leading `---` frontmatter block (decided.core.frontmatter).
pub fn split_frontmatter(text: &str) -> FrontmatterSplit {
    let lines: Vec<&str> = text.split('\n').collect();
    if lines.is_empty() || py_strip(lines[0]) != "---" {
        return FrontmatterSplit {
            raw: None,
            body: text.to_string(),
            line_offset: 0,
            unterminated: false,
        };
    }
    for i in 1..lines.len() {
        let s = py_strip(lines[i]);
        if s == "---" || s == "..." {
            return FrontmatterSplit {
                raw: Some(lines[1..i].join("\n")),
                body: lines[i + 1..].join("\n"),
                line_offset: i + 1,
                unterminated: false,
            };
        }
    }
    FrontmatterSplit {
        raw: None,
        body: text.to_string(),
        line_offset: 0,
        unterminated: true,
    }
}

// ---------------------------------------------------------------------------
// Block tokenizer: state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Token {
    pub typ: &'static str,
    pub tag: &'static str,
    pub map: Option<(usize, usize)>,
    pub content: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Parent {
    Root,
    Paragraph,
    Blockquote,
    List,
    Reference,
}

const MAX_NESTING: i32 = 20; // commonmark preset options.maxNesting

#[inline]
fn is_str_space(c: char) -> bool {
    c == ' ' || c == '\t'
}

struct State<'s> {
    src: &'s [char],
    b_marks: Vec<usize>,
    e_marks: Vec<usize>,
    t_shift: Vec<usize>,
    s_count: Vec<i32>,
    bs_count: Vec<i32>,
    blk_indent: i32,
    line: usize,
    line_max: usize,
    tight: bool,
    list_indent: i32,
    parent_type: Parent,
    level: i32,
    tokens: Vec<Token>,
}

impl<'s> State<'s> {
    fn new(src: &'s [char]) -> Self {
        let mut st = State {
            src,
            b_marks: Vec::new(),
            e_marks: Vec::new(),
            t_shift: Vec::new(),
            s_count: Vec::new(),
            bs_count: Vec::new(),
            blk_indent: 0,
            line: 0,
            line_max: 0,
            tight: false,
            list_indent: -1,
            parent_type: Parent::Root,
            level: 0,
            tokens: Vec::new(),
        };
        let length = src.len();
        let mut indent_found = false;
        let mut start = 0usize;
        let mut indent = 0usize;
        let mut offset = 0i32;
        for (pos, &ch) in src.iter().enumerate() {
            if !indent_found {
                if is_str_space(ch) {
                    indent += 1;
                    if ch == '\t' {
                        offset += 4 - offset % 4;
                    } else {
                        offset += 1;
                    }
                    continue;
                } else {
                    indent_found = true;
                }
            }
            if ch == '\n' || pos == length - 1 {
                let p = if ch != '\n' { pos + 1 } else { pos };
                st.b_marks.push(start);
                st.e_marks.push(p);
                st.t_shift.push(indent);
                st.s_count.push(offset);
                st.bs_count.push(0);
                indent_found = false;
                indent = 0;
                offset = 0;
                start = p + 1;
            }
        }
        // fake entry to simplify bounds checks
        st.b_marks.push(length);
        st.e_marks.push(length);
        st.t_shift.push(0);
        st.s_count.push(0);
        st.bs_count.push(0);
        st.line_max = st.b_marks.len() - 1;
        st
    }

    fn push(&mut self, typ: &'static str, tag: &'static str, nesting: i32) -> usize {
        if nesting < 0 {
            self.level -= 1;
        }
        if nesting > 0 {
            self.level += 1;
        }
        self.tokens.push(Token {
            typ,
            tag,
            map: None,
            content: String::new(),
        });
        self.tokens.len() - 1
    }

    fn is_empty(&self, line: usize) -> bool {
        self.b_marks[line] + self.t_shift[line] >= self.e_marks[line]
    }

    fn skip_empty_lines(&self, mut from: usize) -> usize {
        while from < self.line_max {
            if self.b_marks[from] + self.t_shift[from] < self.e_marks[from] {
                break;
            }
            from += 1;
        }
        from
    }

    fn skip_spaces(&self, mut pos: usize) -> usize {
        while let Some(&c) = self.src.get(pos) {
            if !is_str_space(c) {
                break;
            }
            pos += 1;
        }
        pos
    }

    fn skip_spaces_back(&self, mut pos: usize, minimum: usize) -> usize {
        if pos <= minimum {
            return pos;
        }
        while pos > minimum {
            pos -= 1;
            if !is_str_space(self.src[pos]) {
                return pos + 1;
            }
        }
        pos
    }

    fn skip_chars_str(&self, mut pos: usize, ch: char) -> usize {
        while let Some(&c) = self.src.get(pos) {
            if c != ch {
                break;
            }
            pos += 1;
        }
        pos
    }

    fn skip_chars_str_back(&self, mut pos: usize, ch: char, minimum: usize) -> usize {
        if pos <= minimum {
            return pos;
        }
        while pos > minimum {
            pos -= 1;
            if ch != self.src[pos] {
                return pos + 1;
            }
        }
        pos
    }

    fn get_lines(&self, begin: usize, end: usize, indent: i32, keep_last_lf: bool) -> String {
        if begin >= end {
            return String::new();
        }
        let mut out = String::new();
        for line in begin..end {
            let mut line_indent: i32 = 0;
            let line_start = self.b_marks[line];
            let mut first = line_start;
            let last = if line + 1 < end || keep_last_lf {
                self.e_marks[line] + 1
            } else {
                self.e_marks[line]
            };
            while first < last && line_indent < indent {
                match self.src.get(first) {
                    None => break, // Python would IndexError here; defensively stop
                    Some(&ch) => {
                        if is_str_space(ch) {
                            if ch == '\t' {
                                line_indent += 4 - (line_indent + self.bs_count[line]) % 4;
                            } else {
                                line_indent += 1;
                            }
                        } else if first - line_start < self.t_shift[line] {
                            line_indent += 1;
                        } else {
                            break;
                        }
                    }
                }
                first += 1;
            }
            if line_indent > indent {
                for _ in 0..(line_indent - indent) {
                    out.push(' ');
                }
            }
            let last_c = last.min(self.src.len());
            if first < last_c {
                out.extend(self.src[first..last_c].iter());
            }
        }
        out
    }

    fn is_code_block(&self, line: usize) -> bool {
        self.s_count[line] - self.blk_indent >= 4
    }
}

type Rule = fn(&mut State, usize, usize, bool) -> bool;

// Terminator chains for the commonmark preset:
// getRules("paragraph"|"reference"|"blockquote") = fence, blockquote, hr,
// list, html_block, heading; getRules("list") = fence, blockquote, hr.
const TERM_PARAGRAPH: &[Rule] = &[
    rule_fence,
    rule_blockquote,
    rule_hr,
    rule_list,
    rule_html_block,
    rule_heading,
];
const TERM_LIST: &[Rule] = &[rule_fence, rule_blockquote, rule_hr];
const RULES_ROOT: &[Rule] = &[
    rule_code,
    rule_fence,
    rule_blockquote,
    rule_hr,
    rule_list,
    rule_reference,
    rule_html_block,
    rule_heading,
    rule_lheading,
    rule_paragraph,
];

fn tokenize(state: &mut State, start_line: usize, end_line: usize) {
    let mut has_empty_lines = false;
    let mut line = start_line;
    while line < end_line {
        line = state.skip_empty_lines(line);
        state.line = line;
        if line >= end_line {
            break;
        }
        if state.s_count[line] < state.blk_indent {
            break;
        }
        if state.level >= MAX_NESTING {
            state.line = end_line;
            break;
        }
        for rule in RULES_ROOT {
            if rule(state, line, end_line, false) {
                break;
            }
        }
        state.tight = !has_empty_lines;
        line = state.line;
        if line >= 1 && (line - 1) < end_line && state.is_empty(line - 1) {
            has_empty_lines = true;
        }
        if line < end_line && state.is_empty(line) {
            has_empty_lines = true;
            line += 1;
            state.line = line;
        }
    }
}

// ---------------------------------------------------------------------------
// Block rules
// ---------------------------------------------------------------------------

fn rule_code(state: &mut State, start_line: usize, end_line: usize, _silent: bool) -> bool {
    if !state.is_code_block(start_line) {
        return false;
    }
    let mut last = start_line + 1;
    let mut next_line = start_line + 1;
    while next_line < end_line {
        if state.is_empty(next_line) {
            next_line += 1;
            continue;
        }
        if state.is_code_block(next_line) {
            next_line += 1;
            last = next_line;
            continue;
        }
        break;
    }
    state.line = last;
    let mut content = state.get_lines(start_line, last, 4 + state.blk_indent, false);
    content.push('\n');
    let i = state.push("code_block", "code", 0);
    state.tokens[i].content = content;
    state.tokens[i].map = Some((start_line, state.line));
    true
}

fn rule_fence(state: &mut State, start_line: usize, end_line: usize, silent: bool) -> bool {
    let mut have_end_marker = false;
    let mut pos = state.b_marks[start_line] + state.t_shift[start_line];
    let mut maximum = state.e_marks[start_line];
    if state.is_code_block(start_line) {
        return false;
    }
    if pos + 3 > maximum {
        return false;
    }
    let marker = state.src[pos];
    if marker != '~' && marker != '`' {
        return false;
    }
    let mem = pos;
    pos = state.skip_chars_str(pos, marker);
    let length = pos - mem;
    if length < 3 {
        return false;
    }
    let params: String = state.src[pos..maximum].iter().collect();
    if marker == '`' && params.contains('`') {
        return false;
    }
    if silent {
        return true;
    }
    let mut next_line = start_line;
    loop {
        next_line += 1;
        if next_line >= end_line {
            break;
        }
        pos = state.b_marks[next_line] + state.t_shift[next_line];
        let mem2 = pos;
        maximum = state.e_marks[next_line];
        if pos < maximum && state.s_count[next_line] < state.blk_indent {
            break;
        }
        match state.src.get(pos) {
            Some(&c) if c == marker => {}
            Some(_) => continue,
            None => break,
        }
        if state.is_code_block(next_line) {
            continue;
        }
        pos = state.skip_chars_str(pos, marker);
        if pos - mem2 < length {
            continue;
        }
        pos = state.skip_spaces(pos);
        if pos < maximum {
            continue;
        }
        have_end_marker = true;
        break;
    }
    let indent = state.s_count[start_line];
    state.line = next_line + if have_end_marker { 1 } else { 0 };
    let i = state.push("fence", "code", 0);
    state.tokens[i].content = state.get_lines(start_line + 1, next_line, indent, true);
    state.tokens[i].map = Some((start_line, state.line));
    true
}

fn rule_hr(state: &mut State, start_line: usize, _end_line: usize, silent: bool) -> bool {
    let mut pos = state.b_marks[start_line] + state.t_shift[start_line];
    let maximum = state.e_marks[start_line];
    if state.is_code_block(start_line) {
        return false;
    }
    let marker = match state.src.get(pos) {
        Some(&c) => c,
        None => return false,
    };
    pos += 1;
    if marker != '*' && marker != '-' && marker != '_' {
        return false;
    }
    let mut cnt = 1;
    while pos < maximum {
        let ch = state.src[pos];
        pos += 1;
        if ch != marker && !is_str_space(ch) {
            return false;
        }
        if ch == marker {
            cnt += 1;
        }
    }
    if cnt < 3 {
        return false;
    }
    if silent {
        return true;
    }
    state.line = start_line + 1;
    let i = state.push("hr", "hr", 0);
    state.tokens[i].map = Some((start_line, state.line));
    true
}

const H_TAGS: [&str; 6] = ["h1", "h2", "h3", "h4", "h5", "h6"];

fn rule_heading(state: &mut State, start_line: usize, _end_line: usize, silent: bool) -> bool {
    let mut pos = state.b_marks[start_line] + state.t_shift[start_line];
    let mut maximum = state.e_marks[start_line];
    if state.is_code_block(start_line) {
        return false;
    }
    let mut ch = state.src.get(pos).copied();
    if ch != Some('#') || pos >= maximum {
        return false;
    }
    let mut level = 1usize;
    pos += 1;
    ch = state.src.get(pos).copied();
    while ch == Some('#') && pos < maximum && level <= 6 {
        level += 1;
        pos += 1;
        ch = state.src.get(pos).copied();
    }
    if level > 6 || (pos < maximum && !ch.is_some_and(is_str_space)) {
        return false;
    }
    if silent {
        return true;
    }
    maximum = state.skip_spaces_back(maximum, pos);
    let tmp = state.skip_chars_str_back(maximum, '#', pos);
    if tmp > pos && is_str_space(state.src[tmp - 1]) {
        maximum = tmp;
    }
    state.line = start_line + 1;
    let tag = H_TAGS[level - 1];
    let i = state.push("heading_open", tag, 1);
    state.tokens[i].map = Some((start_line, state.line));
    let content: String = if maximum > pos {
        state.src[pos..maximum].iter().collect()
    } else {
        String::new()
    };
    let i2 = state.push("inline", "", 0);
    state.tokens[i2].content = py_strip(&content).to_string();
    state.tokens[i2].map = Some((start_line, state.line));
    state.push("heading_close", tag, -1);
    true
}

fn rule_lheading(state: &mut State, start_line: usize, end_line: usize, _silent: bool) -> bool {
    let mut level: Option<usize> = None;
    let mut next_line = start_line + 1;
    if state.is_code_block(start_line) {
        return false;
    }
    let old_parent = state.parent_type;
    state.parent_type = Parent::Paragraph;
    while next_line < end_line && !state.is_empty(next_line) {
        if state.s_count[next_line] - state.blk_indent > 3 {
            next_line += 1;
            continue;
        }
        if state.s_count[next_line] >= state.blk_indent {
            let mut pos = state.b_marks[next_line] + state.t_shift[next_line];
            let maximum = state.e_marks[next_line];
            if pos < maximum {
                let m = state.src[pos];
                if m == '-' || m == '=' {
                    pos = state.skip_chars_str(pos, m);
                    pos = state.skip_spaces(pos);
                    if pos >= maximum {
                        level = Some(if m == '=' { 1 } else { 2 });
                        break;
                    }
                }
            }
        }
        if state.s_count[next_line] < 0 {
            next_line += 1;
            continue;
        }
        let mut terminate = false;
        for rule in TERM_PARAGRAPH {
            if rule(state, next_line, end_line, true) {
                terminate = true;
                break;
            }
        }
        if terminate {
            break;
        }
        next_line += 1;
    }
    let lv = match level {
        Some(lv) => lv,
        // NOTE: the oracle does NOT restore parentType on this failure path
        // (unobservable: every terminator loop re-sets it first).
        None => return false,
    };
    let content =
        py_strip(&state.get_lines(start_line, next_line, state.blk_indent, false)).to_string();
    state.line = next_line + 1;
    let tag = H_TAGS[lv - 1];
    let i = state.push("heading_open", tag, 1);
    state.tokens[i].map = Some((start_line, state.line));
    let i2 = state.push("inline", "", 0);
    state.tokens[i2].content = content;
    state.tokens[i2].map = Some((start_line, state.line - 1));
    state.push("heading_close", tag, -1);
    state.parent_type = old_parent;
    true
}

fn rule_paragraph(state: &mut State, start_line: usize, _end_line: usize, _silent: bool) -> bool {
    let mut next_line = start_line + 1;
    let end_line = state.line_max; // paragraph ignores the passed endLine
    let old_parent = state.parent_type;
    state.parent_type = Parent::Paragraph;
    while next_line < end_line {
        if state.is_empty(next_line) {
            break;
        }
        if state.s_count[next_line] - state.blk_indent > 3 {
            next_line += 1;
            continue;
        }
        if state.s_count[next_line] < 0 {
            next_line += 1;
            continue;
        }
        let mut terminate = false;
        for rule in TERM_PARAGRAPH {
            if rule(state, next_line, end_line, true) {
                terminate = true;
                break;
            }
        }
        if terminate {
            break;
        }
        next_line += 1;
    }
    let content =
        py_strip(&state.get_lines(start_line, next_line, state.blk_indent, false)).to_string();
    state.line = next_line;
    let i = state.push("paragraph_open", "p", 1);
    state.tokens[i].map = Some((start_line, state.line));
    let i2 = state.push("inline", "", 0);
    state.tokens[i2].content = content;
    state.tokens[i2].map = Some((start_line, state.line));
    state.push("paragraph_close", "p", -1);
    state.parent_type = old_parent;
    true
}

fn rule_blockquote(state: &mut State, start_line: usize, end_line: usize, silent: bool) -> bool {
    let old_line_max = state.line_max;
    let mut pos = state.b_marks[start_line] + state.t_shift[start_line];
    let mut max = state.e_marks[start_line];
    if state.is_code_block(start_line) {
        return false;
    }
    if state.src.get(pos) != Some(&'>') {
        return false;
    }
    pos += 1;
    if silent {
        return true;
    }
    let mut initial: i32 = state.s_count[start_line] + 1;
    let mut offset: i32 = initial;
    let second = state.src.get(pos).copied();
    let mut adjust_tab = false;
    let space_after_marker;
    if second == Some(' ') {
        pos += 1;
        initial += 1;
        offset += 1;
        adjust_tab = false;
        space_after_marker = true;
    } else if second == Some('\t') {
        space_after_marker = true;
        if (state.bs_count[start_line] + offset) % 4 == 3 {
            pos += 1;
            initial += 1;
            offset += 1;
            adjust_tab = false;
        } else {
            adjust_tab = true;
        }
    } else {
        space_after_marker = false;
    }
    let mut old_b_marks = vec![state.b_marks[start_line]];
    state.b_marks[start_line] = pos;
    while pos < max {
        let ch = state.src[pos];
        if is_str_space(ch) {
            if ch == '\t' {
                offset +=
                    4 - (offset + state.bs_count[start_line] + if adjust_tab { 1 } else { 0 }) % 4;
            } else {
                offset += 1;
            }
        } else {
            break;
        }
        pos += 1;
    }
    let mut old_bs_count = vec![state.bs_count[start_line]];
    state.bs_count[start_line] =
        state.s_count[start_line] + 1 + if space_after_marker { 1 } else { 0 };
    let mut last_line_empty = pos >= max;
    let mut old_s_count = vec![state.s_count[start_line]];
    state.s_count[start_line] = offset - initial;
    let mut old_t_shift = vec![state.t_shift[start_line]];
    state.t_shift[start_line] = pos - state.b_marks[start_line];
    let old_parent = state.parent_type;
    state.parent_type = Parent::Blockquote;

    let mut next_line = start_line + 1;
    while next_line < end_line {
        let is_outdented = state.s_count[next_line] < state.blk_indent;
        pos = state.b_marks[next_line] + state.t_shift[next_line];
        max = state.e_marks[next_line];
        if pos >= max {
            break;
        }
        let evaluates_true = state.src[pos] == '>' && !is_outdented;
        pos += 1;
        if evaluates_true {
            let mut initial2: i32 = state.s_count[next_line] + 1;
            let mut offset2: i32 = initial2;
            let next_char = state.src.get(pos).copied();
            let mut adjust_tab2 = false;
            let space_after_marker2;
            if next_char == Some(' ') {
                pos += 1;
                initial2 += 1;
                offset2 += 1;
                adjust_tab2 = false;
                space_after_marker2 = true;
            } else if next_char == Some('\t') {
                space_after_marker2 = true;
                if (state.bs_count[next_line] + offset2) % 4 == 3 {
                    pos += 1;
                    initial2 += 1;
                    offset2 += 1;
                    adjust_tab2 = false;
                } else {
                    adjust_tab2 = true;
                }
            } else {
                space_after_marker2 = false;
            }
            old_b_marks.push(state.b_marks[next_line]);
            state.b_marks[next_line] = pos;
            while pos < max {
                let ch = state.src[pos];
                if is_str_space(ch) {
                    if ch == '\t' {
                        offset2 += 4
                            - (offset2
                                + state.bs_count[next_line]
                                + if adjust_tab2 { 1 } else { 0 })
                                % 4;
                    } else {
                        offset2 += 1;
                    }
                } else {
                    break;
                }
                pos += 1;
            }
            last_line_empty = pos >= max;
            old_bs_count.push(state.bs_count[next_line]);
            state.bs_count[next_line] =
                state.s_count[next_line] + 1 + if space_after_marker2 { 1 } else { 0 };
            old_s_count.push(state.s_count[next_line]);
            state.s_count[next_line] = offset2 - initial2;
            old_t_shift.push(state.t_shift[next_line]);
            state.t_shift[next_line] = pos - state.b_marks[next_line];
            next_line += 1;
            continue;
        }
        if last_line_empty {
            break;
        }
        let mut terminate = false;
        for rule in TERM_PARAGRAPH {
            if rule(state, next_line, end_line, true) {
                terminate = true;
                break;
            }
        }
        if terminate {
            // hard termination mode for paragraphs
            state.line_max = next_line;
            if state.blk_indent != 0 {
                old_b_marks.push(state.b_marks[next_line]);
                old_bs_count.push(state.bs_count[next_line]);
                old_t_shift.push(state.t_shift[next_line]);
                old_s_count.push(state.s_count[next_line]);
                state.s_count[next_line] -= state.blk_indent;
            }
            break;
        }
        old_b_marks.push(state.b_marks[next_line]);
        old_bs_count.push(state.bs_count[next_line]);
        old_t_shift.push(state.t_shift[next_line]);
        old_s_count.push(state.s_count[next_line]);
        // negative indentation: paragraph continuation
        state.s_count[next_line] = -1;
        next_line += 1;
    }
    let old_indent = state.blk_indent;
    state.blk_indent = 0;

    let open_idx = state.push("blockquote_open", "blockquote", 1);
    state.tokens[open_idx].map = Some((start_line, 0));
    tokenize(state, start_line, next_line);
    state.push("blockquote_close", "blockquote", -1);

    state.line_max = old_line_max;
    state.parent_type = old_parent;
    let end = state.line;
    state.tokens[open_idx].map = Some((start_line, end));

    for (i, &ts) in old_t_shift.iter().enumerate() {
        state.b_marks[i + start_line] = old_b_marks[i];
        state.t_shift[i + start_line] = ts;
        state.s_count[i + start_line] = old_s_count[i];
        state.bs_count[i + start_line] = old_bs_count[i];
    }
    state.blk_indent = old_indent;
    true
}

fn skip_bullet_list_marker(state: &State, start_line: usize) -> Option<usize> {
    let mut pos = state.b_marks[start_line] + state.t_shift[start_line];
    let maximum = state.e_marks[start_line];
    let marker = *state.src.get(pos)?;
    pos += 1;
    if marker != '*' && marker != '-' && marker != '+' {
        return None;
    }
    if pos < maximum {
        let ch = state.src[pos];
        if !is_str_space(ch) {
            return None;
        }
    }
    Some(pos)
}

fn skip_ordered_list_marker(state: &State, start_line: usize) -> Option<usize> {
    let start = state.b_marks[start_line] + state.t_shift[start_line];
    let mut pos = start;
    let maximum = state.e_marks[start_line];
    if pos + 1 >= maximum {
        return None;
    }
    let ch = state.src[pos];
    pos += 1;
    if !ch.is_ascii_digit() {
        return None;
    }
    loop {
        if pos >= maximum {
            return None;
        }
        let ch = state.src[pos];
        pos += 1;
        if ch.is_ascii_digit() {
            if pos - start >= 10 {
                return None;
            }
            continue;
        }
        if ch == ')' || ch == '.' {
            break;
        }
        return None;
    }
    if pos < maximum {
        let ch = state.src[pos];
        if !is_str_space(ch) {
            return None;
        }
    }
    Some(pos)
}

fn rule_list(state: &mut State, start_line: usize, end_line: usize, silent: bool) -> bool {
    let mut is_terminating_paragraph = false;
    if state.is_code_block(start_line) {
        return false;
    }
    if state.list_indent >= 0
        && state.s_count[start_line] - state.list_indent >= 4
        && state.s_count[start_line] < state.blk_indent
    {
        return false;
    }
    if silent
        && state.parent_type == Parent::Paragraph
        && state.s_count[start_line] >= state.blk_indent
    {
        is_terminating_paragraph = true;
    }
    let is_ordered;
    let mut pos_after_marker;
    if let Some(p) = skip_ordered_list_marker(state, start_line) {
        is_ordered = true;
        pos_after_marker = p;
        let start = state.b_marks[start_line] + state.t_shift[start_line];
        let digits: String = state.src[start..pos_after_marker - 1].iter().collect();
        let marker_value: i64 = digits.parse().unwrap_or(0);
        if is_terminating_paragraph && marker_value != 1 {
            return false;
        }
    } else if let Some(p) = skip_bullet_list_marker(state, start_line) {
        is_ordered = false;
        pos_after_marker = p;
    } else {
        return false;
    }
    if is_terminating_paragraph && state.skip_spaces(pos_after_marker) >= state.e_marks[start_line]
    {
        return false;
    }
    let marker_char = state.src[pos_after_marker - 1];
    if silent {
        return true;
    }

    let open_idx = state.push(
        if is_ordered {
            "ordered_list_open"
        } else {
            "bullet_list_open"
        },
        if is_ordered { "ol" } else { "ul" },
        1,
    );
    let list_start_line = start_line;
    state.tokens[open_idx].map = Some((list_start_line, 0));

    let mut start_line = start_line;
    let mut next_line = start_line;
    let old_parent = state.parent_type;
    state.parent_type = Parent::List;

    while next_line < end_line {
        let mut pos = pos_after_marker;
        let maximum = state.e_marks[next_line];
        let initial: i32 = state.s_count[next_line] + pos_after_marker as i32
            - (state.b_marks[start_line] as i32 + state.t_shift[start_line] as i32);
        let mut offset = initial;
        while pos < maximum {
            let ch = state.src[pos];
            if ch == '\t' {
                offset += 4 - (offset + state.bs_count[next_line]) % 4;
            } else if ch == ' ' {
                offset += 1;
            } else {
                break;
            }
            pos += 1;
        }
        let content_start = pos;
        let mut indent_after_marker: i32 = if content_start >= maximum {
            1
        } else {
            offset - initial
        };
        if indent_after_marker > 4 {
            indent_after_marker = 1;
        }
        let indent = initial + indent_after_marker;

        let item_idx = state.push("list_item_open", "li", 1);
        state.tokens[item_idx].map = Some((start_line, 0));

        let old_tight = state.tight;
        let old_t_shift = state.t_shift[start_line];
        let old_s_count = state.s_count[start_line];
        let old_list_indent = state.list_indent;
        state.list_indent = state.blk_indent;
        state.blk_indent = indent;
        state.tight = true;
        state.t_shift[start_line] = content_start - state.b_marks[start_line];
        state.s_count[start_line] = offset;

        if content_start >= maximum && state.is_empty(start_line + 1) {
            // empty list item workaround
            state.line = (state.line + 2).min(end_line);
        } else {
            tokenize(state, start_line, end_line);
        }

        // (the oracle tracks tight/prevEmptyEnd here; only used for the
        // hidden-paragraph marking, which the consumed surface never sees)

        state.blk_indent = state.list_indent;
        state.list_indent = old_list_indent;
        state.t_shift[start_line] = old_t_shift;
        state.s_count[start_line] = old_s_count;
        state.tight = old_tight;

        state.push("list_item_close", "li", -1);

        next_line = state.line;
        start_line = state.line;
        if let Some(m) = state.tokens[item_idx].map {
            state.tokens[item_idx].map = Some((m.0, next_line));
        }
        if next_line >= end_line {
            break;
        }
        if state.s_count[next_line] < state.blk_indent {
            break;
        }
        if state.is_code_block(start_line) {
            break;
        }
        let mut terminate = false;
        for rule in TERM_LIST {
            if rule(state, next_line, end_line, true) {
                terminate = true;
                break;
            }
        }
        if terminate {
            break;
        }
        if is_ordered {
            match skip_ordered_list_marker(state, next_line) {
                Some(p) => pos_after_marker = p,
                None => break,
            }
        } else {
            match skip_bullet_list_marker(state, next_line) {
                Some(p) => pos_after_marker = p,
                None => break,
            }
        }
        if marker_char != state.src[pos_after_marker - 1] {
            break;
        }
    }

    state.push(
        if is_ordered {
            "ordered_list_close"
        } else {
            "bullet_list_close"
        },
        if is_ordered { "ol" } else { "ul" },
        -1,
    );
    state.tokens[open_idx].map = Some((list_start_line, next_line));
    state.line = next_line;
    state.parent_type = old_parent;
    true
}

// --- reference rule + link helpers -----------------------------------------

struct DestResult {
    ok: bool,
    pos: usize,
    str_: String,
}

fn parse_link_destination(s: &[char], mut pos: usize, maximum: usize) -> DestResult {
    let start = pos;
    let mut result = DestResult {
        ok: false,
        pos: 0,
        str_: String::new(),
    };
    if s.get(pos) == Some(&'<') {
        pos += 1;
        while pos < maximum {
            let code = s[pos];
            if code == '\n' || code == '<' {
                return result;
            }
            if code == '>' {
                result.pos = pos + 1;
                let inner: String = s[start + 1..pos].iter().collect();
                result.str_ = unescape_all(&inner);
                result.ok = true;
                return result;
            }
            if code == '\\' && pos + 1 < maximum {
                pos += 2;
                continue;
            }
            pos += 1;
        }
        return result;
    }
    let mut level: i32 = 0;
    while pos < maximum {
        let code = s[pos];
        if code == ' ' {
            break;
        }
        if (code as u32) < 0x20 || code == '\u{7f}' {
            break;
        }
        if code == '\\' && pos + 1 < maximum {
            if s[pos + 1] == ' ' {
                break;
            }
            pos += 2;
            continue;
        }
        if code == '(' {
            level += 1;
            if level > 32 {
                return result;
            }
        }
        if code == ')' {
            if level == 0 {
                break;
            }
            level -= 1;
        }
        pos += 1;
    }
    if start == pos {
        return result;
    }
    if level != 0 {
        return result;
    }
    let inner: String = s[start..pos].iter().collect();
    result.str_ = unescape_all(&inner);
    result.pos = pos;
    result.ok = true;
    result
}

struct TitleResult {
    ok: bool,
    can_continue: bool,
    pos: usize,
    str_: String,
    marker: char,
}

fn parse_link_title(
    s: &[char],
    start: usize,
    maximum: usize,
    prev_state: Option<TitleResult>,
) -> TitleResult {
    let mut pos = start;
    let mut start = start;
    let mut state = TitleResult {
        ok: false,
        can_continue: false,
        pos: 0,
        str_: String::new(),
        marker: '\0',
    };
    if let Some(prev) = prev_state {
        state.str_ = prev.str_;
        state.marker = prev.marker;
    } else {
        if pos >= maximum {
            return state;
        }
        let marker = s[pos];
        if marker != '"' && marker != '\'' && marker != '(' {
            return state;
        }
        start += 1;
        pos += 1;
        state.marker = if marker == '(' { ')' } else { marker };
    }
    while pos < maximum {
        let code = s[pos];
        if code == state.marker {
            state.pos = pos + 1;
            let inner: String = s[start..pos].iter().collect();
            state.str_.push_str(&unescape_all(&inner));
            state.ok = true;
            return state;
        } else if code == '(' && state.marker == ')' {
            return state;
        } else if code == '\\' && pos + 1 < maximum {
            pos += 1;
        }
        pos += 1;
    }
    state.can_continue = true;
    let inner: String = s[start..pos.min(s.len())].iter().collect();
    state.str_.push_str(&unescape_all(&inner));
    state
}

const ENTITIES_JSON: &str = include_str!("../assets/spec/markdown-entities.json");

fn entities() -> &'static HashMap<String, String> {
    static MAP: OnceLock<HashMap<String, String>> = OnceLock::new();
    MAP.get_or_init(|| serde_json::from_str(ENTITIES_JSON).expect("entities json parses"))
}

fn is_valid_entity_code(c: u32) -> bool {
    if (0xD800..=0xDFFF).contains(&c) {
        return false;
    }
    if (0xFDD0..=0xFDEF).contains(&c) {
        return false;
    }
    if (c & 0xFFFF) == 0xFFFF || (c & 0xFFFF) == 0xFFFE {
        return false;
    }
    if c <= 0x08 {
        return false;
    }
    if c == 0x0B {
        return false;
    }
    if (0x0E..=0x1F).contains(&c) {
        return false;
    }
    if (0x7F..=0x9F).contains(&c) {
        return false;
    }
    c <= 0x10FFFF
}

const MD_ESCAPABLE: &str = "!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~";

/// markdown-it `unescapeAll`: backslash escapes plus HTML entity references.
/// Entity names that do not resolve are left verbatim (the oracle's
/// `replaceEntityPattern` returns the original match), so only resolvable
/// (ASCII) names need handling.
fn unescape_all(s: &str) -> String {
    if !s.contains('\\') && !s.contains('&') {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && i + 1 < chars.len() {
            let n = chars[i + 1];
            if n.is_ascii() && MD_ESCAPABLE.contains(n) {
                out.push(n);
                i += 2;
                continue;
            }
            out.push(c);
            i += 1;
            continue;
        }
        if c == '&' {
            // &([a-z#][a-z0-9]{1,31}); (IGNORECASE): head, maximal alnum run
            // (1..=31), then ';'.
            if let Some(&head) = chars.get(i + 1) {
                if head.is_ascii_alphabetic() || head == '#' {
                    let mut j = i + 2;
                    while j < chars.len() && chars[j].is_ascii_alphanumeric() {
                        j += 1;
                    }
                    let run = j - (i + 2);
                    if (1..=31).contains(&run) && chars.get(j) == Some(&';') {
                        let name: String = chars[i + 1..j].iter().collect();
                        if let Some(rep) = resolve_entity(&name) {
                            out.push_str(&rep);
                            i = j + 1;
                            continue;
                        }
                        // unresolved: keep the original text verbatim
                        out.extend(chars[i..=j].iter());
                        i = j + 1;
                        continue;
                    }
                }
            }
            out.push(c);
            i += 1;
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

fn resolve_entity(name: &str) -> Option<String> {
    if let Some(v) = entities().get(name) {
        return Some(v.clone());
    }
    let rest = name.strip_prefix('#')?;
    let code: Option<u32> = if let Some(hex) = rest.strip_prefix(['x', 'X']) {
        if (1..=8).contains(&hex.len()) && hex.chars().all(|c| c.is_ascii_hexdigit()) {
            u32::from_str_radix(hex, 16).ok()
        } else {
            None
        }
    } else if (1..=8).contains(&rest.len()) && rest.chars().all(|c| c.is_ascii_digit()) {
        rest.parse::<u32>().ok()
    } else {
        None
    };
    let code = code?;
    if is_valid_entity_code(code) {
        char::from_u32(code).map(|c| c.to_string())
    } else {
        None
    }
}

/// markdown-it `validateLink` over the (identity-)normalized destination.
fn validate_link(url: &str) -> bool {
    let stripped = py_strip(url);
    let lower: String = stripped.chars().flat_map(|c| c.to_lowercase()).collect();
    if lower.starts_with("javascript:")
        || lower.starts_with("vbscript:")
        || lower.starts_with("file:")
    {
        return false;
    }
    if lower.starts_with("data:") {
        return [
            "data:image/gif;",
            "data:image/png;",
            "data:image/jpeg;",
            "data:image/webp;",
        ]
        .iter()
        .any(|p| lower.starts_with(p));
    }
    true
}

fn get_next_line(state: &mut State, next_line: usize) -> Option<Vec<char>> {
    let end_line = state.line_max;
    if next_line >= end_line || state.is_empty(next_line) {
        return None;
    }
    let mut is_continuation = false;
    if state.is_code_block(next_line) {
        is_continuation = true;
    }
    if state.s_count[next_line] < 0 {
        is_continuation = true;
    }
    if !is_continuation {
        let old_parent = state.parent_type;
        state.parent_type = Parent::Reference;
        let mut terminate = false;
        for rule in TERM_PARAGRAPH {
            if rule(state, next_line, end_line, true) {
                terminate = true;
                break;
            }
        }
        state.parent_type = old_parent;
        if terminate {
            return None;
        }
    }
    let pos = state.b_marks[next_line] + state.t_shift[next_line];
    let maximum = state.e_marks[next_line];
    Some(state.src[pos..(maximum + 1).min(state.src.len())].to_vec())
}

fn rule_reference(state: &mut State, start_line: usize, _end_line: usize, silent: bool) -> bool {
    let pos0 = state.b_marks[start_line] + state.t_shift[start_line];
    let maximum0 = state.e_marks[start_line];
    let mut next_line = start_line + 1;
    if state.is_code_block(start_line) {
        return false;
    }
    if state.src.get(pos0) != Some(&'[') {
        return false;
    }
    let mut string: Vec<char> = state.src[pos0..(maximum0 + 1).min(state.src.len())].to_vec();
    let mut maximum = string.len();

    let mut label_end: Option<usize> = None;
    let mut pos = 1usize;
    while pos < maximum {
        let ch = string[pos];
        if ch == '[' {
            return false;
        } else if ch == ']' {
            label_end = Some(pos);
            break;
        } else if ch == '\n' {
            if let Some(cont) = get_next_line(state, next_line) {
                string.extend(cont);
                maximum = string.len();
                next_line += 1;
            }
        } else if ch == '\\' {
            pos += 1;
            if pos < maximum && string[pos] == '\n' {
                if let Some(cont) = get_next_line(state, next_line) {
                    string.extend(cont);
                    maximum = string.len();
                    next_line += 1;
                }
            }
        }
        pos += 1;
    }
    let label_end = match label_end {
        Some(le) => le,
        None => return false,
    };
    if string.get(label_end + 1) != Some(&':') {
        return false;
    }

    // skip optional whitespace after the colon
    pos = label_end + 2;
    while pos < maximum {
        let ch = string[pos];
        if ch == '\n' {
            if let Some(cont) = get_next_line(state, next_line) {
                string.extend(cont);
                maximum = string.len();
                next_line += 1;
            }
        } else if is_str_space(ch) {
        } else {
            break;
        }
        pos += 1;
    }

    let dest_res = parse_link_destination(&string, pos, maximum);
    if !dest_res.ok {
        return false;
    }
    // normalizeLink is approximated as identity for the validate step (its
    // percent-encoding cannot change the BAD_PROTO prefix decision).
    if !validate_link(&dest_res.str_) {
        return false;
    }
    pos = dest_res.pos;

    let dest_end_pos = pos;
    let dest_end_line_no = next_line;

    let start_pos = pos;
    while pos < maximum {
        let ch = string[pos];
        if ch == '\n' {
            if let Some(cont) = get_next_line(state, next_line) {
                string.extend(cont);
                maximum = string.len();
                next_line += 1;
            }
        } else if is_str_space(ch) {
        } else {
            break;
        }
        pos += 1;
    }

    let mut title_res = parse_link_title(&string, pos, maximum, None);
    while title_res.can_continue {
        match get_next_line(state, next_line) {
            None => break,
            Some(cont) => {
                string.extend(cont);
                pos = maximum;
                maximum = string.len();
                next_line += 1;
                title_res = parse_link_title(&string, pos, maximum, Some(title_res));
            }
        }
    }

    let mut title;
    if pos < maximum && start_pos != pos && title_res.ok {
        title = title_res.str_;
        pos = title_res.pos;
    } else {
        title = String::new();
        pos = dest_end_pos;
        next_line = dest_end_line_no;
    }

    while pos < maximum {
        if !is_str_space(string[pos]) {
            break;
        }
        pos += 1;
    }

    if pos < maximum && string[pos] != '\n' && !title.is_empty() {
        title = String::new();
        pos = dest_end_pos;
        next_line = dest_end_line_no;
        while pos < maximum {
            if !is_str_space(string[pos]) {
                break;
            }
            pos += 1;
        }
    }
    let _ = title;

    if pos < maximum && string[pos] != '\n' {
        return false;
    }

    let label: String = string[1..label_end].iter().collect();
    // normalizeReference(label) is empty iff the stripped label is empty
    if py_strip(&label).is_empty() {
        return false;
    }

    if silent {
        return true;
    }

    state.line = next_line;
    true
}

// --- html_block rule --------------------------------------------------------

const HTML_BLOCK_NAMES: &[&str] = &[
    "address", "article", "aside", "base", "basefont", "blockquote", "body", "caption", "center",
    "col", "colgroup", "dd", "details", "dialog", "dir", "div", "dl", "dt", "fieldset",
    "figcaption", "figure", "footer", "form", "frame", "frameset", "h1", "h2", "h3", "h4", "h5",
    "h6", "head", "header", "hr", "html", "iframe", "legend", "li", "link", "main", "menu",
    "menuitem", "nav", "noframes", "ol", "optgroup", "option", "p", "param", "search", "section",
    "summary", "table", "tbody", "td", "tfoot", "th", "thead", "title", "tr", "track", "ul",
];

/// Case-insensitive char match against an ASCII-lowercase pattern char,
/// covering Python re.IGNORECASE's simple case folding (incl. U+017F and
/// U+212A folding to ASCII).
#[inline]
fn ci_eq(c: char, lower: char) -> bool {
    c.eq_ignore_ascii_case(&lower)
        || (lower == 's' && c == '\u{17f}')
        || (lower == 'k' && c == '\u{212a}')
}

fn starts_with_ci(hay: &[char], pos: usize, needle: &str) -> bool {
    needle
        .chars()
        .enumerate()
        .all(|(i, l)| hay.get(pos + i).is_some_and(|&c| ci_eq(c, l)))
}

fn contains_ci(hay: &[char], needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    (0..hay.len()).any(|i| starts_with_ci(hay, i, needle))
}

fn contains_exact(hay: &[char], needle: &str) -> bool {
    let n: Vec<char> = needle.chars().collect();
    if n.is_empty() {
        return true;
    }
    if hay.len() < n.len() {
        return false;
    }
    (0..=hay.len() - n.len()).any(|i| hay[i..i + n.len()] == n[..])
}

#[inline]
fn unquoted_value_char(c: char) -> bool {
    (c as u32) > 0x20 && !matches!(c, '"' | '\'' | '=' | '<' | '>' | '`')
}

/// `^(open_tag|close_tag)\s*$` (HTML_OPEN_CLOSE_TAG_STR, no IGNORECASE).
fn match_open_close_tag_line(line: &[char]) -> bool {
    let n = line.len();
    if line.first() != Some(&'<') {
        return false;
    }
    let mut p = 1usize;
    let closing = line.get(1) == Some(&'/');
    if closing {
        p = 2;
    }
    if !line.get(p).is_some_and(|c| c.is_ascii_alphabetic()) {
        return false;
    }
    p += 1;
    while line
        .get(p)
        .is_some_and(|c| c.is_ascii_alphanumeric() || *c == '-')
    {
        p += 1;
    }
    if closing {
        while line.get(p).is_some_and(|&c| py_is_space(c)) {
            p += 1;
        }
        if line.get(p) != Some(&'>') {
            return false;
        }
        p += 1;
    } else {
        // (attribute)*
        loop {
            let save = p;
            let mut q = p;
            while line.get(q).is_some_and(|&c| py_is_space(c)) {
                q += 1;
            }
            if q == p {
                break;
            }
            if !line
                .get(q)
                .is_some_and(|c| c.is_ascii_alphabetic() || *c == '_' || *c == ':')
            {
                p = save;
                break;
            }
            q += 1;
            while line
                .get(q)
                .is_some_and(|c| c.is_ascii_alphanumeric() || matches!(*c, ':' | '.' | '_' | '-'))
            {
                q += 1;
            }
            // optional \s*=\s*attr_value
            let save2 = q;
            let mut r = q;
            while line.get(r).is_some_and(|&c| py_is_space(c)) {
                r += 1;
            }
            if line.get(r) == Some(&'=') {
                r += 1;
                while line.get(r).is_some_and(|&c| py_is_space(c)) {
                    r += 1;
                }
                match line.get(r) {
                    Some(&'\'') => {
                        r += 1;
                        while line.get(r).is_some_and(|&c| c != '\'') {
                            r += 1;
                        }
                        if line.get(r) == Some(&'\'') {
                            q = r + 1;
                        } else {
                            q = save2;
                        }
                    }
                    Some(&'"') => {
                        r += 1;
                        while line.get(r).is_some_and(|&c| c != '"') {
                            r += 1;
                        }
                        if line.get(r) == Some(&'"') {
                            q = r + 1;
                        } else {
                            q = save2;
                        }
                    }
                    Some(&c) if unquoted_value_char(c) => {
                        r += 1;
                        while line.get(r).is_some_and(|&cc| unquoted_value_char(cc)) {
                            r += 1;
                        }
                        q = r;
                    }
                    _ => {
                        q = save2;
                    }
                }
            } else {
                q = save2;
            }
            p = q;
        }
        while line.get(p).is_some_and(|&c| py_is_space(c)) {
            p += 1;
        }
        if line.get(p) == Some(&'/') {
            p += 1;
        }
        if line.get(p) != Some(&'>') {
            return false;
        }
        p += 1;
    }
    while p < n {
        if !py_is_space(line[p]) {
            return false;
        }
        p += 1;
    }
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HtmlSeq {
    Raw,     // script|pre|style|textarea ... matching close tag
    Comment, // <!-- ... -->
    Pi,      // <? ... ?>
    Decl,    // <![A-Z] ... >
    Cdata,   // <![CDATA[ ... ]]>
    Block,   // </?blockname(\s|/?>|$) ... blank line
    AnyTag,  // full open/close tag alone on the line ... blank line (no terminate)
}

fn starts_with_exact(hay: &[char], pos: usize, needle: &str) -> bool {
    needle
        .chars()
        .enumerate()
        .all(|(i, l)| hay.get(pos + i) == Some(&l))
}

fn html_seq_open(line: &[char]) -> Option<(HtmlSeq, bool)> {
    if line.first() != Some(&'<') {
        return None;
    }
    // 1. <(script|pre|style|textarea)(?=(\s|>|$)) IGNORECASE
    for name in ["script", "pre", "style", "textarea"] {
        if starts_with_ci(line, 1, name) {
            let after = 1 + name.len();
            match line.get(after) {
                None => return Some((HtmlSeq::Raw, true)),
                Some(&c) if py_is_space(c) || c == '>' => return Some((HtmlSeq::Raw, true)),
                _ => {}
            }
        }
    }
    // 2. <!--
    if line.len() >= 4 && line[1] == '!' && line[2] == '-' && line[3] == '-' {
        return Some((HtmlSeq::Comment, true));
    }
    // 3. <?
    if line.get(1) == Some(&'?') {
        return Some((HtmlSeq::Pi, true));
    }
    // 4. <![A-Z] — note <![CDATA[ also matches this earlier pattern, so the
    // decl close condition (`>`) wins for CDATA too, exactly like the oracle
    // (Python checks the HTML_SEQUENCES list in order and takes the first).
    if line.get(1) == Some(&'!') && line.get(2).is_some_and(|c| c.is_ascii_uppercase()) {
        return Some((HtmlSeq::Decl, true));
    }
    // 5. <![CDATA[ (exact case; unreachable after 4, kept for fidelity)
    if starts_with_exact(line, 0, "<![CDATA[") {
        return Some((HtmlSeq::Cdata, true));
    }
    // 6. </?(block names)(?=(\s|/?>|$)) IGNORECASE
    {
        let mut p = 1usize;
        if line.get(1) == Some(&'/') {
            p = 2;
        }
        for name in HTML_BLOCK_NAMES {
            if starts_with_ci(line, p, name) {
                let after = p + name.len();
                let ok = match line.get(after) {
                    None => true,
                    Some(&c) if py_is_space(c) || c == '>' => true,
                    Some(&'/') => line.get(after + 1) == Some(&'>'),
                    _ => false,
                };
                if ok {
                    return Some((HtmlSeq::Block, true));
                }
            }
        }
    }
    // 7. full open/close tag to end of line (cannot terminate a paragraph)
    if match_open_close_tag_line(line) {
        return Some((HtmlSeq::AnyTag, false));
    }
    None
}

fn html_seq_close(seq: HtmlSeq, line: &[char]) -> bool {
    match seq {
        HtmlSeq::Raw => ["</script>", "</pre>", "</style>", "</textarea>"]
            .iter()
            .any(|c| contains_ci(line, c)),
        HtmlSeq::Comment => contains_exact(line, "-->"),
        HtmlSeq::Pi => contains_exact(line, "?>"),
        HtmlSeq::Decl => contains_exact(line, ">"),
        HtmlSeq::Cdata => contains_exact(line, "]]>"),
        HtmlSeq::Block | HtmlSeq::AnyTag => line.is_empty(), // ^$
    }
}

fn rule_html_block(state: &mut State, start_line: usize, end_line: usize, silent: bool) -> bool {
    let mut pos = state.b_marks[start_line] + state.t_shift[start_line];
    let mut maximum = state.e_marks[start_line];
    if state.is_code_block(start_line) {
        return false;
    }
    if state.src.get(pos) != Some(&'<') {
        return false;
    }
    let mut line_text: Vec<char> = state.src[pos..maximum].to_vec();
    let (seq, terminator) = match html_seq_open(&line_text) {
        Some(x) => x,
        None => return false,
    };
    if silent {
        return terminator;
    }
    let mut next_line = start_line + 1;
    if !html_seq_close(seq, &line_text) {
        while next_line < end_line {
            if state.s_count[next_line] < state.blk_indent {
                break;
            }
            pos = state.b_marks[next_line] + state.t_shift[next_line];
            maximum = state.e_marks[next_line];
            line_text = state.src[pos.min(maximum)..maximum].to_vec();
            if html_seq_close(seq, &line_text) {
                if !line_text.is_empty() {
                    next_line += 1;
                }
                break;
            }
            next_line += 1;
        }
    }
    state.line = next_line;
    let i = state.push("html_block", "", 0);
    state.tokens[i].map = Some((start_line, next_line));
    state.tokens[i].content = state.get_lines(start_line, next_line, state.blk_indent, true);
    true
}

// ---------------------------------------------------------------------------
// Tokenizer driver
// ---------------------------------------------------------------------------

/// rules_core normalize: `\r\n?|\n` -> `\n`, then NUL -> U+FFFD.
fn normalize_src(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '\r' => {
                if it.peek() == Some(&'\n') {
                    it.next();
                }
                out.push('\n');
            }
            '\0' => out.push('\u{fffd}'),
            c => out.push(c),
        }
    }
    out
}

/// Tokenize a (frontmatter-stripped) Markdown body into the flat block-level
/// token stream, exactly as markdown-it-py 4.2.0 "commonmark" emits it.
pub fn tokenize_blocks(body: &str) -> Vec<Token> {
    let normalized = normalize_src(body);
    if normalized.is_empty() {
        return Vec::new();
    }
    let src: Vec<char> = normalized.chars().collect();
    let mut state = State::new(&src);
    let line_max = state.line_max;
    tokenize(&mut state, 0, line_max);
    state.tokens
}

/// One consumed-surface event: a heading (tag + raw inline text) or a body
/// inline (raw content), each with the block's 0-based start line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockEvent {
    pub heading: bool,
    pub tag: &'static str,
    pub line: i64, // map[0]; -1 when the token had no map (never in practice)
    pub content: String,
}

/// The exact token surface the oracle walk consumes (contract section 1).
pub fn consumed_events(body: &str) -> Vec<BlockEvent> {
    let tokens = tokenize_blocks(body);
    let mut events = Vec::new();
    for i in 0..tokens.len() {
        let t = &tokens[i];
        if t.typ == "heading_open" {
            let content = if i + 1 < tokens.len() {
                tokens[i + 1].content.clone()
            } else {
                String::new()
            };
            events.push(BlockEvent {
                heading: true,
                tag: t.tag,
                line: t.map.map_or(-1, |m| m.0 as i64),
                content,
            });
        } else if t.typ == "inline" && !(i > 0 && tokens[i - 1].typ == "heading_open") {
            events.push(BlockEvent {
                heading: false,
                tag: "",
                line: t.map.map_or(-1, |m| m.0 as i64),
                content: t.content.clone(),
            });
        }
    }
    events
}

// ---------------------------------------------------------------------------
// The walk (decided.core.markdown._WalkState)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sect {
    None,
    Other,
    Problem,
    Requirements,
    SuccessMetrics,
    Risks,
}

struct Walk {
    offset: i64,
    title: Option<String>,
    extra_title_lines: Vec<i64>,
    section: Sect,
    current_h2: Option<String>,
    search_sections: Vec<SearchSection>,
    current_search: Option<usize>,
    problem_lines: Vec<String>,
    requirement_lines: Vec<(String, i64)>,
    metric_lines: Vec<String>,
    risk_lines: Vec<String>,
    section_order: Vec<String>,
    section_bodies: HashMap<String, Vec<String>>,
    section_chars: HashMap<String, usize>,
    captured_lines: usize,
    truncated_fields: BTreeSet<String>,
    body_truncated: bool,
    has_problem: bool,
    has_requirements: bool,
    has_metrics: bool,
    has_risks: bool,
}

impl Walk {
    fn new(offset: i64) -> Self {
        Walk {
            offset,
            title: None,
            extra_title_lines: Vec::new(),
            section: Sect::None,
            current_h2: None,
            search_sections: Vec::new(),
            current_search: None,
            problem_lines: Vec::new(),
            requirement_lines: Vec::new(),
            metric_lines: Vec::new(),
            risk_lines: Vec::new(),
            section_order: Vec::new(),
            section_bodies: HashMap::new(),
            section_chars: HashMap::new(),
            captured_lines: 0,
            truncated_fields: BTreeSet::new(),
            body_truncated: false,
            has_problem: false,
            has_requirements: false,
            has_metrics: false,
            has_risks: false,
        }
    }

    fn open_heading(&mut self, tag: &str, map: Option<(usize, usize)>, heading_text: &str) {
        if tag == "h1" {
            if self.title.is_none() {
                self.title = Some(py_strip(heading_text).to_string());
            } else {
                self.extra_title_lines
                    .push(map.map_or(0, |m| m.0 as i64 + 1 + self.offset));
            }
            self.section = Sect::None;
            self.current_h2 = None;
            self.current_search = None;
        } else if tag == "h2" {
            let normalized = py_casefold(py_strip(heading_text));
            self.current_h2 = Some(normalized.clone());
            if !self.section_bodies.contains_key(&normalized) {
                self.section_bodies.insert(normalized.clone(), Vec::new());
                self.section_order.push(normalized.clone());
            }
            self.search_sections.push(SearchSection {
                heading: py_strip(heading_text).to_string(),
                lines: Vec::new(),
            });
            self.current_search = Some(self.search_sections.len() - 1);
            self.section = match normalized.as_str() {
                "problem" => {
                    self.has_problem = true;
                    Sect::Problem
                }
                "requirements" => {
                    self.has_requirements = true;
                    Sect::Requirements
                }
                "success metrics" => {
                    self.has_metrics = true;
                    Sect::SuccessMetrics
                }
                "risks" => {
                    self.has_risks = true;
                    Sect::Risks
                }
                _ => Sect::None,
            };
        } else {
            self.section = Sect::Other;
        }
    }

    fn capture_inline(&mut self, content: &str, map: Option<(usize, usize)>) {
        if self.body_truncated {
            return;
        }
        if let Some(h2) = self.current_h2.clone() {
            self.capture_generic_body(content, &h2);
        }
        if self.body_truncated || matches!(self.section, Sect::None | Sect::Other) {
            return;
        }
        self.capture_field(content, map);
    }

    fn capture_generic_body(&mut self, content: &str, heading: &str) {
        for raw in content.split('\n') {
            let stripped = py_strip(raw);
            if stripped.is_empty() {
                continue;
            }
            if self.captured_lines >= MAX_CAPTURED_LINES {
                self.body_truncated = true;
                break;
            }
            let n_chars = stripped.chars().count();
            let used = self.section_chars.get(heading).copied().unwrap_or(0);
            if used + n_chars > MAX_FIELD_CHARS {
                self.truncated_fields.insert(heading.to_string());
                continue;
            }
            if !self.section_bodies.contains_key(heading) {
                self.section_bodies.insert(heading.to_string(), Vec::new());
                self.section_order.push(heading.to_string());
            }
            self.section_bodies
                .get_mut(heading)
                .expect("just ensured")
                .push(stripped.to_string());
            self.section_chars
                .insert(heading.to_string(), used + n_chars + 1);
            self.captured_lines += 1;
            if let Some(cs) = self.current_search {
                self.search_sections[cs].lines.push(stripped.to_string());
            }
        }
    }

    fn capture_field(&mut self, content: &str, map: Option<(usize, usize)>) {
        let start_line = map.map_or(0, |m| m.0 as i64 + self.offset);
        let mut lines: Vec<(String, i64)> = Vec::new();
        for (offset, raw) in content.split('\n').enumerate() {
            let stripped = py_strip(raw);
            if !stripped.is_empty() {
                lines.push((stripped.to_string(), start_line + offset as i64 + 1));
            }
        }
        match self.section {
            Sect::Problem => self
                .problem_lines
                .extend(lines.into_iter().map(|(t, _)| t)),
            Sect::Requirements => self.requirement_lines.extend(lines),
            Sect::SuccessMetrics => self.metric_lines.extend(lines.into_iter().map(|(t, _)| t)),
            Sect::Risks => self.risk_lines.extend(lines.into_iter().map(|(t, _)| t)),
            _ => {}
        }
    }
}

fn classify_requirement_line(text: &str, line: i64) -> Result<Requirement, MalformedRequirement> {
    // _BRACKET_RE = ^\[(?P<id>[^\]]*)\]\s*(?P<text>.*)$
    let malformed_no_id = || MalformedRequirement {
        raw: text.to_string(),
        line,
        bad_id: None,
        empty_text: false,
    };
    if !text.starts_with('[') {
        return Err(malformed_no_id());
    }
    let close = match text.find(']') {
        Some(i) => i,
        None => return Err(malformed_no_id()),
    };
    let id_group = &text[1..close];
    let mut rest = &text[close + 1..];
    // \s* — Python re \s == str.isspace set (verified empirically)
    rest = rest.trim_start_matches(py_is_space);
    let req_id = py_strip(id_group);
    let desc = py_strip(rest);
    let canonical = req_id
        .strip_prefix("REQ-")
        .is_some_and(|d| !d.is_empty() && d.chars().all(is_re_digit));
    if !canonical {
        return Err(MalformedRequirement {
            raw: text.to_string(),
            line,
            bad_id: Some(req_id.to_string()),
            empty_text: false,
        });
    }
    if desc.is_empty() {
        return Err(MalformedRequirement {
            raw: text.to_string(),
            line,
            bad_id: Some(req_id.to_string()),
            empty_text: true,
        });
    }
    Ok(Requirement {
        id: req_id.to_string(),
        text: desc.to_string(),
        line,
    })
}

fn budget_issues(walk: &Walk) -> Vec<Issue> {
    let mut issues = Vec::new();
    for heading in &walk.truncated_fields {
        issues.push(Issue {
            severity: "warning",
            code: "field-truncated",
            message: format!(
                "section {} exceeds the {}-char field cap and was truncated",
                py_repr_str(heading),
                MAX_FIELD_CHARS
            ),
            line: None,
        });
    }
    if walk.body_truncated {
        issues.push(Issue {
            severity: "warning",
            code: "body-truncated",
            message: format!(
                "document body exceeds the {}-line capture cap and was truncated",
                MAX_CAPTURED_LINES
            ),
            line: None,
        });
    }
    issues
}

fn degraded_product(source_path: &str, issues: Vec<Issue>) -> Product {
    Product {
        source_path: source_path.to_string(),
        parse_issues: issues,
        ..Default::default()
    }
}

fn oversize_issue(cap: u128, kind: &str) -> Issue {
    Issue {
        severity: "error",
        code: "artifact-oversize",
        message: format!(
            "artifact exceeds the {cap}-byte {kind} cap (set DECIDED_MAX_FILE_BYTES to raise it)"
        ),
        line: Some(1),
    }
}

// ---------------------------------------------------------------------------
// parse / parse_file envelopes
// ---------------------------------------------------------------------------

/// Parse Markdown `text` into a `Product` (decided.core.markdown.parse), with the
/// byte cap taken from the environment.
pub fn parse(text: &str, source_path: &str) -> Product {
    parse_with_cap(text, source_path, max_file_bytes())
}

/// `parse` with an explicit byte cap (testing seam; the oracle reads the env).
pub fn parse_with_cap(text: &str, source_path: &str, cap: u128) -> Product {
    if exceeds_byte_cap(text, cap) {
        return degraded_product(source_path, vec![oversize_issue(cap, "parse")]);
    }
    let split = split_frontmatter(text);
    let mut metadata_issues = Vec::new();
    if split.raw.is_none() && split.unterminated {
        metadata_issues.push(Issue {
            severity: "error",
            code: "malformed-frontmatter",
            message: "frontmatter block opened with --- on line 1 but never closed".to_string(),
            line: Some(1),
        });
    }

    let tokens = tokenize_blocks(&split.body);
    let mut walk = Walk::new(split.line_offset as i64);
    for i in 0..tokens.len() {
        let t = &tokens[i];
        if t.typ == "heading_open" {
            let heading_text = if i + 1 < tokens.len() {
                tokens[i + 1].content.clone()
            } else {
                String::new()
            };
            walk.open_heading(t.tag, t.map, &heading_text);
        } else if t.typ == "inline" && !(i > 0 && tokens[i - 1].typ == "heading_open") {
            walk.capture_inline(&t.content, t.map);
        }
    }

    let mut requirements = Vec::new();
    let mut malformed = Vec::new();
    for (line_text, line_no) in &walk.requirement_lines {
        match classify_requirement_line(line_text, *line_no) {
            Ok(r) => requirements.push(r),
            Err(m) => malformed.push(m),
        }
    }
    let problem = if walk.has_problem {
        Some(py_strip(&walk.problem_lines.join("\n")).to_string())
    } else {
        None
    };
    let sections: Vec<(String, String)> = walk
        .section_order
        .iter()
        .map(|h| (h.clone(), walk.section_bodies[h].join("\n")))
        .collect();
    let parse_issues = budget_issues(&walk);

    Product {
        title: walk.title,
        extra_title_lines: walk.extra_title_lines,
        problem,
        requirements,
        malformed_requirements: malformed,
        success_metrics: walk.metric_lines,
        risks: walk.risk_lines,
        sections,
        search_sections: walk.search_sections,
        has_problem_section: walk.has_problem,
        has_requirements_section: walk.has_requirements,
        has_metrics_section: walk.has_metrics,
        has_risks_section: walk.has_risks,
        source_path: source_path.to_string(),
        frontmatter_raw: split.raw,
        metadata_issues,
        parse_issues,
    }
}

/// Linux `strerror(errno)` text, recovered from std's io::Error display
/// (`"<strerror> (os error <n>)"`).
fn strerror(errno: i32) -> String {
    let s = std::io::Error::from_raw_os_error(errno).to_string();
    match s.rfind(" (os error ") {
        Some(i) => s[..i].to_string(),
        None => s,
    }
}

/// Python `str(OSError)` with a filename: `[Errno N] <strerror>: '<path>'`.
fn os_error_message(err: &std::io::Error, path: &str) -> String {
    match err.raw_os_error() {
        Some(n) => format!("[Errno {}] {}: {}", n, strerror(n), py_repr_str(path)),
        None => err.to_string(),
    }
}

fn unreadable_issue(err: &std::io::Error, path: &str) -> Issue {
    Issue {
        severity: "error",
        code: "unreadable-artifact",
        message: format!("cannot read artifact: {}", os_error_message(err, path)),
        line: Some(1),
    }
}

/// Read `path` and parse it (decided.core.markdown.parse_file).
pub fn parse_file(path: &str) -> Product {
    parse_file_with_cap(path, max_file_bytes())
}

/// `parse_file` with an explicit byte cap (testing seam).
pub fn parse_file_with_cap(path: &str, cap: u128) -> Product {
    let size = match std::fs::metadata(path) {
        Ok(m) => m.len(),
        Err(e) => return degraded_product(path, vec![unreadable_issue(&e, path)]),
    };
    if size as u128 > cap {
        return degraded_product(path, vec![oversize_issue(cap, "file")]);
    }
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) => return degraded_product(path, vec![unreadable_issue(&e, path)]),
    };
    let mut data = Vec::new();
    let take_n: u64 = cap.saturating_add(1).min(u64::MAX as u128) as u64;
    if let Err(e) = file.take(take_n).read_to_end(&mut data) {
        return degraded_product(path, vec![unreadable_issue(&e, path)]);
    }
    if data.len() as u128 > cap {
        return degraded_product(path, vec![oversize_issue(cap, "file")]);
    }
    match String::from_utf8(data) {
        Ok(text) => parse_with_cap(&text, path, cap),
        Err(e) => {
            let text = String::from_utf8_lossy(e.as_bytes()).into_owned();
            let mut product = parse_with_cap(&text, path, cap);
            product.parse_issues.push(Issue {
                severity: "warning",
                code: "non-utf8-content",
                message: "artifact is not valid UTF-8; decoded lossily".to_string(),
                line: Some(1),
            });
            product
        }
    }
}
