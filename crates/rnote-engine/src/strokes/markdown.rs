// Imports
use super::textstroke::{FontStyle, RangedTextAttribute, TextAttribute, TextStroke, TextStyle};
use rnote_compose::Color;
use std::ops::Range;

/// Detects whether plain text copied from an LLM or external editor contains Markdown syntax.
pub fn looks_like_markdown(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }

    let lines: Vec<&str> = trimmed.lines().collect();
    let is_multiline = lines.len() > 1;
    let mut list_lines = 0usize;
    let mut has_table_row = false;

    for line in &lines {
        let lt = line.trim_start();
        if let Some(after_hashes) = strip_heading_prefix(lt) {
            if !after_hashes.trim().is_empty() {
                return true;
            }
        }
        if lt.starts_with("```") {
            return true;
        }
        if lt.starts_with("> ") && lt[2..].trim().len() > 0 {
            return true;
        }
        if is_multiline && is_horizontal_rule(lt) {
            return true;
        }
        if parse_bullet_line(line).is_some()
            || parse_task_line(line).is_some()
            || parse_numbered_line(line).is_some()
        {
            list_lines += 1;
            if list_lines >= 2 {
                return true;
            }
        }
        let line_trim = line.trim();
        if has_table_row && is_table_separator(line_trim) {
            return true;
        }
        has_table_row = line_trim.starts_with('|') && line_trim.matches('|').count() >= 2;
    }

    if contains_paired_delimiter(trimmed, "**")
        || contains_paired_delimiter(trimmed, "__")
        || contains_paired_delimiter(trimmed, "~~")
        || contains_inline_code(trimmed)
        || contains_markdown_link(trimmed)
    {
        return true;
    }

    false
}

/// Parses a Markdown document (typically copied from an LLM summary) into plain rendered text
/// and sorted, UTF-8-safe `RangedTextAttribute` spans suitable for `TextStroke`.
pub fn parse_markdown_to_text_parts(
    md: &str,
    base_style: &TextStyle,
) -> (String, Vec<RangedTextAttribute>) {
    let normalized = md.replace("\r\n", "\n");
    let lines: Vec<&str> = normalized.trim_end_matches('\n').split('\n').collect();

    let mut out = String::with_capacity(normalized.len());
    let mut attrs: Vec<RangedTextAttribute> = Vec::new();
    let palette = MarkdownPalette::from_base(base_style);

    let mut i = 0usize;
    while i < lines.len() {
        let line = lines[i];
        let trimmed_start = line.trim_start();

        // 1. Fenced code block: ```lang ... ```
        if let Some(rest) = trimmed_start.strip_prefix("```") {
            if !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
            let lang = rest.trim();
            if !lang.is_empty() {
                let tag_start = out.len();
                out.push_str(&lang.to_ascii_uppercase());
                let tag_end = out.len();
                attrs.push(RangedTextAttribute {
                    range: tag_start..tag_end,
                    attribute: TextAttribute::FontSize((palette.base_size * 0.68).max(10.0)),
                });
                attrs.push(RangedTextAttribute {
                    range: tag_start..tag_end,
                    attribute: TextAttribute::FontWeight(700),
                });
                attrs.push(RangedTextAttribute {
                    range: tag_start..tag_end,
                    attribute: TextAttribute::TextColor(palette.muted),
                });
                out.push('\n');
            }

            i += 1;
            let code_start = out.len();
            while i < lines.len() {
                if lines[i].trim_start().starts_with("```") {
                    i += 1;
                    break;
                }
                if out.len() > code_start {
                    out.push('\n');
                }
                out.push_str(lines[i]);
                i += 1;
            }
            let code_end = out.len();
            if code_end > code_start {
                attrs.push(RangedTextAttribute {
                    range: code_start..code_end,
                    attribute: TextAttribute::FontFamily(String::from("monospace")),
                });
                attrs.push(RangedTextAttribute {
                    range: code_start..code_end,
                    attribute: TextAttribute::FontSize((palette.base_size * 0.85).max(12.0)),
                });
                attrs.push(RangedTextAttribute {
                    range: code_start..code_end,
                    attribute: TextAttribute::TextColor(palette.code),
                });
            }
            if i < lines.len() {
                out.push('\n');
            }
            continue;
        }

        // 2. Markdown table block: | ... | followed by | --- | separator
        if line.trim().starts_with('|')
            && line.trim().matches('|').count() >= 2
            && i + 1 < lines.len()
            && is_table_separator(lines[i + 1].trim())
        {
            if !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
            let mut raw_rows: Vec<Vec<String>> = Vec::new();
            raw_rows.push(split_table_row(line));
            i += 2; // skip header + separator
            while i < lines.len() {
                let row_trim = lines[i].trim();
                if row_trim.starts_with('|') && row_trim.matches('|').count() >= 2 {
                    if !is_table_separator(row_trim) {
                        raw_rows.push(split_table_row(lines[i]));
                    }
                    i += 1;
                } else {
                    break;
                }
            }

            let n_cols = raw_rows.iter().map(|r| r.len()).max().unwrap_or(0);
            if n_cols > 0 {
                let mut col_widths = vec![0usize; n_cols];
                for row in &raw_rows {
                    for (c, cell) in row.iter().enumerate() {
                        let clean = strip_inline_markers(cell);
                        col_widths[c] = col_widths[c].max(clean.chars().count());
                    }
                }

                let table_start = out.len();
                for (r_idx, row) in raw_rows.iter().enumerate() {
                    if r_idx > 0 {
                        out.push('\n');
                    }
                    let row_start = out.len();
                    for c in 0..n_cols {
                        if c > 0 {
                            out.push_str(" | ");
                        }
                        let cell_text = row.get(c).map(|s| strip_inline_markers(s)).unwrap_or_default();
                        let char_len = cell_text.chars().count();
                        out.push_str(&cell_text);
                        for _ in char_len..col_widths[c] {
                            out.push(' ');
                        }
                    }
                    let row_end = out.len();
                    if r_idx == 0 && row_end > row_start {
                        attrs.push(RangedTextAttribute {
                            range: row_start..row_end,
                            attribute: TextAttribute::FontWeight(700),
                        });
                    }
                }
                let table_end = out.len();
                if table_end > table_start {
                    attrs.push(RangedTextAttribute {
                        range: table_start..table_end,
                        attribute: TextAttribute::FontFamily(String::from("monospace")),
                    });
                    attrs.push(RangedTextAttribute {
                        range: table_start..table_end,
                        attribute: TextAttribute::FontSize((palette.base_size * 0.88).max(12.0)),
                    });
                }
            }
            if i < lines.len() {
                out.push('\n');
            }
            continue;
        }

        if i > 0 && !out.ends_with('\n') {
            out.push('\n');
        }

        // 3. Horizontal rule: --- / *** / ___
        if is_horizontal_rule(trimmed_start) {
            let hr_start = out.len();
            out.push_str("────────────────────────────────");
            let hr_end = out.len();
            attrs.push(RangedTextAttribute {
                range: hr_start..hr_end,
                attribute: TextAttribute::TextColor(palette.muted),
            });
            i += 1;
            continue;
        }

        // 4. Headings: # .. ######
        if let Some((level, content)) = parse_heading_line(trimmed_start) {
            let scale = match level {
                1 => 1.55,
                2 => 1.35,
                3 => 1.18,
                _ => 1.08,
            };
            let h_start = out.len();
            parse_inline(content.trim(), &mut out, &mut attrs, &palette);
            let h_end = out.len();
            if h_end > h_start {
                attrs.push(RangedTextAttribute {
                    range: h_start..h_end,
                    attribute: TextAttribute::FontWeight(700),
                });
                attrs.push(RangedTextAttribute {
                    range: h_start..h_end,
                    attribute: TextAttribute::FontSize(palette.base_size * scale),
                });
            }
            i += 1;
            continue;
        }

        // 5. Blockquote: > ...
        if let Some(quote_content) = trimmed_start.strip_prefix("> ") {
            let q_start = out.len();
            out.push_str("▎ ");
            let prefix_end = out.len();
            attrs.push(RangedTextAttribute {
                range: q_start..prefix_end,
                attribute: TextAttribute::TextColor(palette.link),
            });
            attrs.push(RangedTextAttribute {
                range: q_start..prefix_end,
                attribute: TextAttribute::FontWeight(700),
            });
            let body_start = out.len();
            parse_inline(quote_content, &mut out, &mut attrs, &palette);
            let body_end = out.len();
            if body_end > body_start {
                attrs.push(RangedTextAttribute {
                    range: body_start..body_end,
                    attribute: TextAttribute::Style(FontStyle::Italic),
                });
                attrs.push(RangedTextAttribute {
                    range: body_start..body_end,
                    attribute: TextAttribute::TextColor(palette.muted),
                });
            }
            i += 1;
            continue;
        }

        // 6. Task list: - [ ] / - [x]
        if let Some((indent_depth, checked, content)) = parse_task_line(line) {
            for _ in 0..indent_depth {
                out.push_str("  ");
            }
            let mark_start = out.len();
            out.push_str(if checked { "  ☑ " } else { "  ☐ " });
            let mark_end = out.len();
            attrs.push(RangedTextAttribute {
                range: mark_start..mark_end,
                attribute: TextAttribute::TextColor(if checked {
                    palette.link
                } else {
                    palette.muted
                }),
            });
            parse_inline(content, &mut out, &mut attrs, &palette);
            i += 1;
            continue;
        }

        // 7. Bullet list: - / * / +
        if let Some((indent_depth, content)) = parse_bullet_line(line) {
            match indent_depth {
                0 => out.push_str("  • "),
                1 => out.push_str("    ◦ "),
                _ => out.push_str("      ▪ "),
            }
            parse_inline(content, &mut out, &mut attrs, &palette);
            i += 1;
            continue;
        }

        // 8. Numbered list: 1. / 2)
        if let Some((indent_depth, num_prefix, content)) = parse_numbered_line(line) {
            for _ in 0..indent_depth {
                out.push_str("  ");
            }
            out.push_str("  ");
            let num_start = out.len();
            out.push_str(num_prefix);
            let num_end = out.len();
            out.push(' ');
            attrs.push(RangedTextAttribute {
                range: num_start..num_end,
                attribute: TextAttribute::FontWeight(700),
            });
            parse_inline(content, &mut out, &mut attrs, &palette);
            i += 1;
            continue;
        }

        // 9. Ordinary paragraph line
        parse_inline(line, &mut out, &mut attrs, &palette);
        i += 1;
    }

    // Critical piet-cairo invariant: valid non-empty UTF-8 char-boundary ranges, sorted by start.
    let len = out.len();
    attrs.retain(|a| {
        a.range.start < a.range.end
            && a.range.end <= len
            && out.is_char_boundary(a.range.start)
            && out.is_char_boundary(a.range.end)
    });
    attrs.sort_by_key(|a| (a.range.start, a.range.end));

    (out, attrs)
}

/// Converts a styled `TextStroke` back into clean Markdown text for copying to external editors / LLMs.
pub fn textstroke_to_markdown(stroke: &TextStroke) -> String {
    let text = &stroke.text;
    if text.is_empty() {
        return String::new();
    }

    let base_size = stroke.text_style.font_size;
    let attrs = &stroke.text_style.ranged_text_attributes;
    let mut out = String::with_capacity(text.len() + 32);

    let mut lines_with_offsets: Vec<(usize, &str)> = Vec::new();
    let mut offset = 0usize;
    for raw_line in text.split('\n') {
        lines_with_offsets.push((offset, raw_line));
        offset += raw_line.len() + 1;
    }

    let mut i = 0usize;
    while i < lines_with_offsets.len() {
        let (line_start, raw_line) = lines_with_offsets[i];
        let line_end = line_start + raw_line.len();
        if i > 0 {
            out.push('\n');
        }

        // Check if this whole line (and possibly subsequent lines) is a multi-line monospace code block
        if !raw_line.is_empty()
            && line_has_full_monospace(attrs, line_start..line_end)
            && i + 1 < lines_with_offsets.len()
            && lines_with_offsets[i + 1].1.len() > 0
            && line_has_full_monospace(
                attrs,
                lines_with_offsets[i + 1].0
                    ..(lines_with_offsets[i + 1].0 + lines_with_offsets[i + 1].1.len()),
            )
        {
            out.push_str("```\n");
            while i < lines_with_offsets.len() {
                let (ls, rl) = lines_with_offsets[i];
                let le = ls + rl.len();
                if rl.is_empty() || !line_has_full_monospace(attrs, ls..le) {
                    if rl.is_empty()
                        && i + 1 < lines_with_offsets.len()
                        && !lines_with_offsets[i + 1].1.is_empty()
                        && line_has_full_monospace(
                            attrs,
                            lines_with_offsets[i + 1].0
                                ..(lines_with_offsets[i + 1].0 + lines_with_offsets[i + 1].1.len()),
                        )
                    {
                        out.push('\n');
                        i += 1;
                        continue;
                    }
                    break;
                }
                out.push_str(rl);
                out.push('\n');
                i += 1;
            }
            out.push_str("```");
            continue;
        }

        // Horizontal rule
        if raw_line.trim() == "────────────────────────────────" {
            out.push_str("---");
            i += 1;
            continue;
        }

        // Heading detection via dominant font size on line
        if let Some(max_size) = max_font_size_in_range(attrs, line_start..line_end) {
            if max_size >= base_size * 1.45 {
                out.push_str("# ");
                render_inline_markdown(raw_line, line_start, attrs, &mut out, true);
                i += 1;
                continue;
            } else if max_size >= base_size * 1.25 {
                out.push_str("## ");
                render_inline_markdown(raw_line, line_start, attrs, &mut out, true);
                i += 1;
                continue;
            } else if max_size >= base_size * 1.12 {
                out.push_str("### ");
                render_inline_markdown(raw_line, line_start, attrs, &mut out, true);
                i += 1;
                continue;
            }
        }

        // List / task / blockquote prefix normalization
        if let Some(rest) = raw_line.strip_prefix("▎ ") {
            out.push_str("> ");
            let shift = raw_line.len() - rest.len();
            render_inline_markdown(rest, line_start + shift, attrs, &mut out, true);
        } else if let Some(rest) = raw_line.strip_prefix("      ▪ ") {
            out.push_str("    - ");
            let shift = raw_line.len() - rest.len();
            render_inline_markdown(rest, line_start + shift, attrs, &mut out, false);
        } else if let Some(rest) = raw_line.strip_prefix("    ◦ ") {
            out.push_str("  - ");
            let shift = raw_line.len() - rest.len();
            render_inline_markdown(rest, line_start + shift, attrs, &mut out, false);
        } else if let Some(rest) = raw_line.strip_prefix("  • ") {
            out.push_str("- ");
            let shift = raw_line.len() - rest.len();
            render_inline_markdown(rest, line_start + shift, attrs, &mut out, false);
        } else if let Some(rest) = raw_line.strip_prefix("  ☑ ") {
            out.push_str("- [x] ");
            let shift = raw_line.len() - rest.len();
            render_inline_markdown(rest, line_start + shift, attrs, &mut out, false);
        } else if let Some(rest) = raw_line.strip_prefix("  ☐ ") {
            out.push_str("- [ ] ");
            let shift = raw_line.len() - rest.len();
            render_inline_markdown(rest, line_start + shift, attrs, &mut out, false);
        } else {
            render_inline_markdown(raw_line, line_start, attrs, &mut out, false);
        }

        i += 1;
    }

    out
}

struct MarkdownPalette {
    base_size: f64,
    code: Color,
    muted: Color,
    link: Color,
}

impl MarkdownPalette {
    fn from_base(base_style: &TextStyle) -> Self {
        let c = base_style.color;
        let luma = 0.299 * c.r + 0.587 * c.g + 0.114 * c.b;
        let is_dark_ink = luma <= 0.55;
        Self {
            base_size: base_style.font_size,
            code: if is_dark_ink {
                Color::new(0.14, 0.28, 0.46, 1.0)
            } else {
                Color::new(0.55, 0.82, 0.98, 1.0)
            },
            muted: if is_dark_ink {
                Color::new(0.38, 0.40, 0.44, 1.0)
            } else {
                Color::new(0.72, 0.74, 0.78, 1.0)
            },
            link: if is_dark_ink {
                Color::new(0.11, 0.44, 0.85, 1.0)
            } else {
                Color::new(0.47, 0.76, 1.0, 1.0)
            },
        }
    }
}

fn parse_heading_line(trimmed_start: &str) -> Option<(usize, &str)> {
    let content = strip_heading_prefix(trimmed_start)?;
    let hashes = trimmed_start.len() - content.len() - 1;
    Some((hashes, content))
}

fn strip_heading_prefix(trimmed_start: &str) -> Option<&str> {
    let bytes = trimmed_start.as_bytes();
    let mut count = 0usize;
    while count < bytes.len() && count < 6 && bytes[count] == b'#' {
        count += 1;
    }
    if count > 0 && bytes.get(count) == Some(&b' ') {
        Some(&trimmed_start[count + 1..])
    } else {
        None
    }
}

fn is_horizontal_rule(trimmed: &str) -> bool {
    let s = trimmed.trim();
    if s.len() < 3 {
        return false;
    }
    let no_spaces: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    if no_spaces.len() < 3 {
        return false;
    }
    let first = no_spaces.chars().next().unwrap();
    matches!(first, '-' | '*' | '_') && no_spaces.chars().all(|c| c == first)
}

fn parse_task_line(line: &str) -> Option<(usize, bool, &str)> {
    let leading_spaces = line.bytes().take_while(|&b| b == b' ').count();
    let indent_depth = leading_spaces / 2;
    let rest = &line[leading_spaces..];
    for prefix in ["- [ ] ", "* [ ] ", "+ [ ] "] {
        if let Some(content) = rest.strip_prefix(prefix) {
            return Some((indent_depth, false, content));
        }
    }
    for prefix in ["- [x] ", "- [X] ", "* [x] ", "* [X] ", "+ [x] ", "+ [X] "] {
        if let Some(content) = rest.strip_prefix(prefix) {
            return Some((indent_depth, true, content));
        }
    }
    None
}

fn parse_bullet_line(line: &str) -> Option<(usize, &str)> {
    let leading_spaces = line.bytes().take_while(|&b| b == b' ').count();
    let indent_depth = leading_spaces / 2;
    let rest = &line[leading_spaces..];
    for prefix in ["- ", "* ", "+ "] {
        if let Some(content) = rest.strip_prefix(prefix) {
            return Some((indent_depth, content));
        }
    }
    None
}

fn parse_numbered_line(line: &str) -> Option<(usize, &str, &str)> {
    let leading_spaces = line.bytes().take_while(|&b| b == b' ').count();
    let indent_depth = leading_spaces / 2;
    let rest = &line[leading_spaces..];
    let digits = rest.bytes().take_while(|b| b.is_ascii_digit()).count();
    if digits == 0 {
        return None;
    }
    let after_digits = &rest[digits..];
    if after_digits.starts_with(". ") || after_digits.starts_with(") ") {
        let prefix = &rest[..digits + 1];
        let content = &rest[digits + 2..];
        return Some((indent_depth, prefix, content));
    }
    None
}

fn is_table_separator(trimmed: &str) -> bool {
    if !trimmed.contains('|') || !trimmed.contains('-') {
        return false;
    }
    trimmed
        .chars()
        .all(|c| matches!(c, '|' | '-' | ':' | ' ' | '\t'))
}

fn split_table_row(line: &str) -> Vec<String> {
    let t = line.trim().trim_start_matches('|').trim_end_matches('|');
    t.split('|').map(|c| c.trim().to_string()).collect()
}

fn strip_inline_markers(input: &str) -> String {
    let mut dummy_out = String::new();
    let mut dummy_attrs = Vec::new();
    let dummy_palette = MarkdownPalette {
        base_size: 24.0,
        code: Color::BLACK,
        muted: Color::BLACK,
        link: Color::BLACK,
    };
    parse_inline(input, &mut dummy_out, &mut dummy_attrs, &dummy_palette);
    dummy_out
}

fn contains_paired_delimiter(text: &str, delim: &str) -> bool {
    let mut search_from = 0usize;
    while let Some(rel_start) = text[search_from..].find(delim) {
        let content_start = search_from + rel_start + delim.len();
        if let Some(rel_end) = text[content_start..].find(delim) {
            if rel_end > 0 {
                let inner = &text[content_start..content_start + rel_end];
                if !inner.starts_with(' ') && !inner.ends_with(' ') {
                    return true;
                }
            }
            search_from = content_start + rel_end + delim.len();
        } else {
            break;
        }
    }
    false
}

fn contains_inline_code(text: &str) -> bool {
    let mut search_from = 0usize;
    while let Some(rel_start) = text[search_from..].find('`') {
        let start = search_from + rel_start;
        // Skip fenced ```
        if text[start..].starts_with("```") {
            search_from = start + 3;
            continue;
        }
        let content_start = start + 1;
        if let Some(rel_end) = text[content_start..].find('`') {
            if rel_end > 0 && !text[content_start..content_start + rel_end].contains('\n') {
                return true;
            }
            search_from = content_start + rel_end + 1;
        } else {
            break;
        }
    }
    false
}

fn contains_markdown_link(text: &str) -> bool {
    let mut search_from = 0usize;
    while let Some(rel_open) = text[search_from..].find('[') {
        let open = search_from + rel_open;
        if let Some(rel_mid) = text[open + 1..].find("](") {
            let url_start = open + 1 + rel_mid + 2;
            if let Some(rel_close) = text[url_start..].find(')') {
                let url = &text[url_start..url_start + rel_close];
                if url.starts_with("http://") || url.starts_with("https://") {
                    return true;
                }
            }
        }
        search_from = open + 1;
    }
    false
}

fn parse_inline(
    input: &str,
    out: &mut String,
    attrs: &mut Vec<RangedTextAttribute>,
    palette: &MarkdownPalette,
) {
    let mut i = 0usize;
    let bytes = input.as_bytes();

    while i < bytes.len() {
        let rest = &input[i..];

        // Escape sequence \* \_ \` \~ \[
        if bytes[i] == b'\\' && i + 1 < bytes.len() {
            if let Some(ch) = input[i + 1..].chars().next() {
                if matches!(ch, '*' | '_' | '`' | '~' | '[' | ']' | '$') {
                    out.push(ch);
                    i += 1 + ch.len_utf8();
                    continue;
                }
            }
        }

        // Inline code: `...`
        if bytes[i] == b'`' && !rest.starts_with("``") {
            if let Some(end_rel) = rest[1..].find('`') {
                if end_rel > 0 {
                    let code_inner = &rest[1..1 + end_rel];
                    let start = out.len();
                    out.push_str(code_inner);
                    let end = out.len();
                    attrs.push(RangedTextAttribute {
                        range: start..end,
                        attribute: TextAttribute::FontFamily(String::from("monospace")),
                    });
                    attrs.push(RangedTextAttribute {
                        range: start..end,
                        attribute: TextAttribute::FontSize((palette.base_size * 0.90).max(12.0)),
                    });
                    attrs.push(RangedTextAttribute {
                        range: start..end,
                        attribute: TextAttribute::TextColor(palette.code),
                    });
                    i += 1 + end_rel + 1;
                    continue;
                }
            }
        }

        // Inline LaTeX math: $...$ or \(...\)
        if rest.starts_with("\\(") {
            if let Some(end_rel) = rest[2..].find("\\)") {
                let math_raw = &rest[2..2 + end_rel];
                let start = out.len();
                out.push_str(&render_latex_math(math_raw));
                let end = out.len();
                if end > start {
                    attrs.push(RangedTextAttribute {
                        range: start..end,
                        attribute: TextAttribute::Style(FontStyle::Italic),
                    });
                }
                i += 2 + end_rel + 2;
                continue;
            }
        }
        if bytes[i] == b'$' && !rest.starts_with("$$") {
            if let Some(end_rel) = rest[1..].find('$') {
                let math_raw = &rest[1..1 + end_rel];
                if end_rel > 0 && !math_raw.contains('\n') {
                    let start = out.len();
                    out.push_str(&render_latex_math(math_raw));
                    let end = out.len();
                    if end > start {
                        attrs.push(RangedTextAttribute {
                            range: start..end,
                            attribute: TextAttribute::Style(FontStyle::Italic),
                        });
                    }
                    i += 1 + end_rel + 1;
                    continue;
                }
            }
        }

        // Markdown link: [label](url)
        if bytes[i] == b'[' {
            if let Some(mid_rel) = rest[1..].find("](") {
                let label_raw = &rest[1..1 + mid_rel];
                let url_start_rel = 1 + mid_rel + 2;
                if let Some(close_rel) = rest[url_start_rel..].find(')') {
                    let url = &rest[url_start_rel..url_start_rel + close_rel];
                    if !url.contains(' ') && !label_raw.contains('\n') {
                        let start = out.len();
                        parse_inline(label_raw, out, attrs, palette);
                        let end = out.len();
                        if end > start {
                            attrs.push(RangedTextAttribute {
                                range: start..end,
                                attribute: TextAttribute::Underline(true),
                            });
                            attrs.push(RangedTextAttribute {
                                range: start..end,
                                attribute: TextAttribute::TextColor(palette.link),
                            });
                        }
                        i += url_start_rel + close_rel + 1;
                        continue;
                    }
                }
            }
        }

        // Bold + Italic: ***...*** or ___...___
        if rest.starts_with("***") {
            if let Some(end_rel) = find_closing_delim(&rest[3..], "***") {
                let inner = &rest[3..3 + end_rel];
                let start = out.len();
                parse_inline(inner, out, attrs, palette);
                let end = out.len();
                if end > start {
                    attrs.push(RangedTextAttribute {
                        range: start..end,
                        attribute: TextAttribute::FontWeight(700),
                    });
                    attrs.push(RangedTextAttribute {
                        range: start..end,
                        attribute: TextAttribute::Style(FontStyle::Italic),
                    });
                }
                i += 3 + end_rel + 3;
                continue;
            }
        }

        // Bold: **...** or __...__
        if rest.starts_with("**") || rest.starts_with("__") {
            let delim = &rest[..2];
            if let Some(end_rel) = find_closing_delim(&rest[2..], delim) {
                let inner = &rest[2..2 + end_rel];
                let start = out.len();
                parse_inline(inner, out, attrs, palette);
                let end = out.len();
                if end > start {
                    attrs.push(RangedTextAttribute {
                        range: start..end,
                        attribute: TextAttribute::FontWeight(700),
                    });
                }
                i += 2 + end_rel + 2;
                continue;
            }
        }

        // Strikethrough: ~~...~~
        if rest.starts_with("~~") {
            if let Some(end_rel) = find_closing_delim(&rest[2..], "~~") {
                let inner = &rest[2..2 + end_rel];
                let start = out.len();
                parse_inline(inner, out, attrs, palette);
                let end = out.len();
                if end > start {
                    attrs.push(RangedTextAttribute {
                        range: start..end,
                        attribute: TextAttribute::Strikethrough(true),
                    });
                }
                i += 2 + end_rel + 2;
                continue;
            }
        }

        // Italic: *...* or _..._ (avoid mid-word snake_case underscores)
        if bytes[i] == b'*' || (bytes[i] == b'_' && is_word_left_boundary(input, i)) {
            let ch_byte = bytes[i];
            let delim = &rest[..1];
            if let Some(end_rel) = find_closing_single(rest, ch_byte) {
                let inner = &rest[1..1 + end_rel];
                let close_idx = i + 1 + end_rel + 1;
                let right_ok = ch_byte == b'*' || is_word_right_boundary(input, close_idx);
                if right_ok && !inner.is_empty() && !inner.starts_with(' ') && !inner.ends_with(' ') {
                    let start = out.len();
                    parse_inline(inner, out, attrs, palette);
                    let end = out.len();
                    if end > start {
                        attrs.push(RangedTextAttribute {
                            range: start..end,
                            attribute: TextAttribute::Style(FontStyle::Italic),
                        });
                    }
                    let _ = delim;
                    i += 1 + end_rel + 1;
                    continue;
                }
            }
        }

        // Ordinary UTF-8 character
        let ch = rest.chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
}

fn find_closing_delim(haystack: &str, delim: &str) -> Option<usize> {
    let rel = haystack.find(delim)?;
    if rel == 0 || haystack[..rel].starts_with(' ') {
        return None;
    }
    Some(rel)
}

fn find_closing_single(rest: &str, ch: u8) -> Option<usize> {
    let bytes = rest.as_bytes();
    let mut k = 1usize;
    while k < bytes.len() {
        if bytes[k] == ch && (k + 1 >= bytes.len() || bytes[k + 1] != ch) {
            return Some(k - 1);
        }
        k += 1;
    }
    None
}

fn is_word_left_boundary(s: &str, idx: usize) -> bool {
    if idx == 0 {
        return true;
    }
    s[..idx]
        .chars()
        .next_back()
        .is_some_and(|c| !c.is_alphanumeric())
}

fn is_word_right_boundary(s: &str, idx_after: usize) -> bool {
    if idx_after >= s.len() {
        return true;
    }
    s[idx_after..]
        .chars()
        .next()
        .is_some_and(|c| !c.is_alphanumeric())
}

fn render_latex_math(raw: &str) -> String {
    let replacements: &[(&str, &str)] = &[
        ("\\rightarrow", "→"),
        ("\\Rightarrow", "⇒"),
        ("\\leftrightarrow", "↔"),
        ("\\approx", "≈"),
        ("\\infty", "∞"),
        ("\\alpha", "α"),
        ("\\beta", "β"),
        ("\\gamma", "γ"),
        ("\\delta", "δ"),
        ("\\Delta", "Δ"),
        ("\\epsilon", "ε"),
        ("\\theta", "θ"),
        ("\\lambda", "λ"),
        ("\\mu", "μ"),
        ("\\pi", "π"),
        ("\\sigma", "σ"),
        ("\\Sigma", "Σ"),
        ("\\omega", "ω"),
        ("\\Omega", "Ω"),
        ("\\forall", "∀"),
        ("\\exists", "∃"),
        ("\\subset", "⊂"),
        ("\\times", "×"),
        ("\\cdot", "·"),
        ("\\sqrt", "√"),
        ("\\sum", "∑"),
        ("\\int", "∫"),
        ("\\neq", "≠"),
        ("\\leq", "≤"),
        ("\\geq", "≥"),
        ("\\pm", "±"),
        ("\\to", "→"),
        ("\\in", "∈"),
        ("^2", "²"),
        ("^3", "³"),
        ("^n", "ⁿ"),
    ];
    let mut res = raw.trim().to_string();
    for (pat, rep) in replacements {
        res = res.replace(pat, rep);
    }
    res
}

fn line_has_full_monospace(attrs: &[RangedTextAttribute], range: Range<usize>) -> bool {
    if range.start >= range.end {
        return false;
    }
    attrs.iter().any(|a| {
        matches!(&a.attribute, TextAttribute::FontFamily(f) if f == "monospace")
            && a.range.start <= range.start
            && a.range.end >= range.end
    })
}

fn max_font_size_in_range(attrs: &[RangedTextAttribute], range: Range<usize>) -> Option<f64> {
    attrs
        .iter()
        .filter_map(|a| {
            if a.range.start < range.end && a.range.end > range.start {
                if let TextAttribute::FontSize(sz) = a.attribute {
                    return Some(sz);
                }
            }
            None
        })
        .reduce(f64::max)
}

fn render_inline_markdown(
    line_slice: &str,
    global_offset: usize,
    attrs: &[RangedTextAttribute],
    out: &mut String,
    ignore_bold_for_heading: bool,
) {
    let mut idx = 0usize;
    let mut active_bold = false;
    let mut active_italic = false;
    let mut active_strike = false;
    let mut active_code = false;

    for ch in line_slice.chars() {
        let gpos = global_offset + idx;
        let (mut bold, mut italic, mut strike, mut code) = (false, false, false, false);
        for a in attrs {
            if a.range.start <= gpos && gpos < a.range.end {
                match &a.attribute {
                    TextAttribute::FontWeight(w) if *w >= 700 && !ignore_bold_for_heading => {
                        bold = true;
                    }
                    TextAttribute::Style(FontStyle::Italic) if !ignore_bold_for_heading => {
                        italic = true;
                    }
                    TextAttribute::Strikethrough(true) => {
                        strike = true;
                    }
                    TextAttribute::FontFamily(f) if f == "monospace" => {
                        code = true;
                    }
                    _ => {}
                }
            }
        }

        // Close transitioned delimiters cleanly
        if active_code && !code {
            out.push('`');
            active_code = false;
        }
        if active_italic && !italic {
            out.push('*');
            active_italic = false;
        }
        if active_bold && !bold {
            out.push_str("**");
            active_bold = false;
        }
        if active_strike && !strike {
            out.push_str("~~");
            active_strike = false;
        }

        // Open new delimiters
        if !active_strike && strike {
            out.push_str("~~");
            active_strike = true;
        }
        if !active_bold && bold {
            out.push_str("**");
            active_bold = true;
        }
        if !active_italic && italic {
            out.push('*');
            active_italic = true;
        }
        if !active_code && code {
            out.push('`');
            active_code = true;
        }

        out.push(ch);
        idx += ch.len_utf8();
    }

    if active_code {
        out.push('`');
    }
    if active_italic {
        out.push('*');
    }
    if active_bold {
        out.push_str("**");
    }
    if active_strike {
        out.push_str("~~");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use p2d::math::Vector2;

    #[test]
    fn test_looks_like_markdown() {
        assert!(looks_like_markdown("# Resumo de Banco de Dados\nTexto simples."));
        assert!(looks_like_markdown("O modelo usa **OLTP** para transações."));
        assert!(looks_like_markdown("Use `SELECT * FROM users;` no terminal."));
        assert!(looks_like_markdown("- Item 1\n- Item 2"));
        assert!(looks_like_markdown("```sql\nSELECT 1;\n```"));
        assert!(looks_like_markdown("| Col A | Col B |\n|---|---|\n| 1 | 2 |"));

        assert!(!looks_like_markdown("Texto normal copiado de um artigo."));
        assert!(!looks_like_markdown("variavel_snake_case_com_sublinhado"));
    }

    #[test]
    fn test_parse_llm_markdown_summary_utf8() {
        let md = "\
# Resumo: OLTP vs OLAP

**OLTP** foca em *transações* rápidas (`INSERT`, `UPDATE`) e ~~locks longos~~ devem ser evitados.

> Observação crítica: índices reduzem latência quando $x^2 \\leq \\infty$.

- [x] Normalização até 3FN
- Sem bloqueios longos
  - Subtópico com acentuação: ç, ã, é

| Tipo | Uso | Latência |
|---|---|---|
| OLTP | Vendas | Baixa |
| OLAP | BI | Média |

```sql
SELECT id, nome FROM clientes;
```
";
        let base_style = TextStyle::default();
        let (rendered, attrs) = parse_markdown_to_text_parts(md, &base_style);

        assert!(rendered.contains("Resumo: OLTP vs OLAP"));
        assert!(rendered.contains("◦ Subtópico com acentuação: ç, ã, é"));
        assert!(rendered.contains("☑ Normalização até 3FN"));
        assert!(rendered.contains("x² ≤ ∞"));
        assert!(!rendered.contains("**OLTP**"));

        for attr in &attrs {
            assert!(attr.range.start < attr.range.end);
            assert!(attr.range.end <= rendered.len());
            assert!(rendered.is_char_boundary(attr.range.start));
            assert!(rendered.is_char_boundary(attr.range.end));
        }

        let mut styled = base_style.clone();
        styled.ranged_text_attributes = attrs;
        let layout = styled.build_text_layout(&mut piet_cairo::CairoText::new(), rendered);
        assert!(layout.is_ok(), "piet_cairo layout build must succeed without panicking");
    }

    #[test]
    fn test_markdown_roundtrip_export() {
        let md = "# Título Principal\n\nTexto com **negrito** e `código`.\n\n- Primeiro item\n- Segundo item";
        let base_style = TextStyle::default();
        let (rendered, attrs) = parse_markdown_to_text_parts(md, &base_style);
        let mut style = base_style;
        style.ranged_text_attributes = attrs;
        let stroke = TextStroke::new(rendered, Vector2::ZERO, style);

        let exported = textstroke_to_markdown(&stroke);
        assert!(exported.contains("# Título Principal"));
        assert!(exported.contains("**negrito**"));
        assert!(exported.contains("`código`"));
        assert!(exported.contains("- Primeiro item"));
    }
}
