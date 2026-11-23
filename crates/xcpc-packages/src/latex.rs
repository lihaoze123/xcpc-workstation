//! Thin Polygon LaTeX → XCPC Markdown converter.
//!
//! Preserves `$LaTeX$` / `$$LaTeX$$` math directly (rendered by `xcpc-convert` via MiTeX),
//! translates Polygon's standard text macros & lists into clean Markdown, extracts
//! `\includegraphics` asset references, and delegates complex LaTeX environments
//! (`tabular`, `tikzpicture`, `figure`, etc.) to `tylax` inside ```` ```typst ```` raw blocks.

use xcpc_domain::Language;

use crate::types::PackageWarning;

/// Extracted raw LaTeX sections of a Polygon problem statement.
#[derive(Debug, Clone, Default)]
pub struct PolygonTexSections {
    pub title: Option<String>,
    pub legend: String,
    pub input: String,
    pub output: String,
    pub interaction: String,
    pub notes: String,
    pub scoring: String,
    pub tutorial: Option<String>,
}

/// Result of converting a LaTeX fragment to Markdown.
#[derive(Debug, Clone, Default)]
pub struct ConvertedLatex {
    pub markdown: String,
    /// Raw filenames / paths referenced by `\includegraphics`.
    pub images: Vec<String>,
}

/// Parses a combined Polygon `statements/<lang>/problem.tex` file into sections.
pub fn parse_polygon_problem_tex(tex: &str) -> PolygonTexSections {
    let mut sections = PolygonTexSections::default();

    // Extract title from `\begin{problem}{Title}{...}{...}{...}{...}` if present.
    let body = if let Some(idx) = tex.find(r"\begin{problem}") {
        let after = &tex[idx + r"\begin{problem}".len()..];
        let (title, rest) = parse_braced_arg(after.trim_start())
            .map(|(t, r)| (Some(t), r))
            .unwrap_or((None, after));
        sections.title = title.map(|t| clean_tex_title(&t));

        // Skip up to 4 additional `{...}` header arguments (input, output, time, memory).
        let mut cursor = rest;
        for _ in 0..4 {
            let trimmed = cursor.trim_start();
            if let Some((_, next)) = parse_braced_arg(trimmed) {
                cursor = next;
            } else {
                break;
            }
        }
        if let Some(end_idx) = cursor.rfind(r"\end{problem}") {
            &cursor[..end_idx]
        } else {
            cursor
        }
    } else {
        tex
    };

    #[derive(Clone, Copy)]
    enum Sec {
        Legend,
        Input,
        Output,
        Interaction,
        Examples,
        Notes,
        Scoring,
    }

    let markers: &[(&str, Sec)] = &[
        (r"\InputFile", Sec::Input),
        (r"\OutputFile", Sec::Output),
        (r"\Interaction", Sec::Interaction),
        (r"\Examples", Sec::Examples),
        (r"\Example", Sec::Examples),
        (r"\Notes", Sec::Notes),
        (r"\Note", Sec::Notes),
        (r"\Scoring", Sec::Scoring),
    ];

    let mut current = Sec::Legend;
    let mut buf = String::new();
    let flush = |sec: Sec, text: &mut String, out: &mut PolygonTexSections| {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            match sec {
                Sec::Legend => append_section(&mut out.legend, trimmed),
                Sec::Input => append_section(&mut out.input, trimmed),
                Sec::Output => append_section(&mut out.output, trimmed),
                Sec::Interaction => append_section(&mut out.interaction, trimmed),
                Sec::Examples => {} // Samples are imported from `samples/`
                Sec::Notes => append_section(&mut out.notes, trimmed),
                Sec::Scoring => append_section(&mut out.scoring, trimmed),
            }
        }
        text.clear();
    };

    for line in body.lines() {
        let trimmed = line.trim();
        let mut matched_marker = None;
        for &(m, sec) in markers {
            if let Some(rest) = trimmed.strip_prefix(m)
                && (rest.is_empty()
                    || !rest.chars().next().is_some_and(|c| c.is_ascii_alphabetic()))
            {
                matched_marker = Some((sec, rest.trim()));
                break;
            }
        }
        if let Some((next_sec, rest)) = matched_marker {
            flush(current, &mut buf, &mut sections);
            current = next_sec;
            if !rest.is_empty() {
                buf.push_str(rest);
                buf.push('\n');
            }
        } else {
            buf.push_str(line);
            buf.push('\n');
        }
    }
    flush(current, &mut buf, &mut sections);

    sections
}

fn append_section(dst: &mut String, piece: &str) {
    if !dst.is_empty() {
        dst.push_str("\n\n");
    }
    dst.push_str(piece);
}

/// Cleans a Polygon LaTeX problem title into plain text.
pub fn clean_tex_title(raw: &str) -> String {
    let stripped = strip_tex_comments(raw);
    let mut warnings = Vec::new();
    let conv = convert_latex_fragment(&stripped, &mut warnings, "name.tex");
    conv.markdown
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Assembles `PolygonTexSections` into a single XCPC `statement(.zh/.en).md` file,
/// returning `(markdown, referenced_image_names)`.
pub fn assemble_statement_markdown(
    sections: &PolygonTexSections,
    lang: Language,
    warnings: &mut Vec<PackageWarning>,
    source_prefix: &str,
) -> ConvertedLatex {
    let mut out = String::new();
    let mut images = Vec::new();

    let (h_input, h_output, h_interaction, h_scoring, h_note) = match lang {
        Language::Chinese => ("输入格式", "输出格式", "交互格式", "评分方式", "提示"),
        Language::English => ("Input", "Output", "Interaction", "Scoring", "Note"),
    };

    let mut push_part = |heading: Option<&str>, raw: &str, name: &str| {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return;
        }
        let path = format!("{source_prefix}/{name}");
        let conv = convert_latex_fragment(trimmed, warnings, &path);
        let body = conv.markdown.trim();
        if body.is_empty() {
            return;
        }
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        if let Some(h) = heading {
            out.push_str("## ");
            out.push_str(h);
            out.push_str("\n\n");
        }
        out.push_str(body);
        images.extend(conv.images);
    };

    push_part(None, &sections.legend, "legend.tex");
    push_part(Some(h_input), &sections.input, "input.tex");
    push_part(Some(h_output), &sections.output, "output.tex");
    push_part(
        Some(h_interaction),
        &sections.interaction,
        "interaction.tex",
    );
    push_part(Some(h_scoring), &sections.scoring, "scoring.tex");
    push_part(Some(h_note), &sections.notes, "notes.tex");

    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }

    ConvertedLatex {
        markdown: out,
        images,
    }
}

/// Converts a single LaTeX body fragment into XCPC Markdown + raw `$LaTeX$` math,
/// falling back to `tylax` inside ```` ```typst ```` blocks for complex environments.
pub fn convert_latex_fragment(
    raw: &str,
    warnings: &mut Vec<PackageWarning>,
    source_path: &str,
) -> ConvertedLatex {
    let text = strip_tex_comments(&raw.replace("\r\n", "\n"));
    let mut images = Vec::new();
    let out = convert_inner(&text, &mut images, warnings, source_path);
    let cleaned = collapse_blank_lines(&out);
    ConvertedLatex {
        markdown: cleaned,
        images,
    }
}

fn strip_tex_comments(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for line in input.lines() {
        let mut cut = line.len();
        let mut escaped = false;
        for (i, ch) in line.char_indices() {
            if escaped {
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
            } else if ch == '%' {
                cut = i;
                break;
            }
        }
        out.push_str(&line[..cut]);
        out.push('\n');
    }
    if !input.ends_with('\n') && out.ends_with('\n') {
        out.pop();
    }
    out
}

fn convert_inner(
    input: &str,
    images: &mut Vec<String>,
    warnings: &mut Vec<PackageWarning>,
    source_path: &str,
) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        let rest = &input[i..];

        // 1. Display math `$$...$$`
        if let Some(after_dd) = rest.strip_prefix("$$")
            && let Some(end) = find_unescaped(after_dd, "$$")
        {
            let inner = &after_dd[..end];
            out.push_str("$$");
            out.push_str(inner);
            out.push_str("$$");
            i += 2 + end + 2;
            continue;
        }

        // 2. Display math `\[...\]`
        if let Some(after_lb) = rest.strip_prefix(r"\[")
            && let Some(end) = after_lb.find(r"\]")
        {
            let inner = &after_lb[..end];
            out.push_str("$$\n");
            out.push_str(inner.trim());
            out.push_str("\n$$");
            i += 2 + end + 2;
            continue;
        }

        // 3. Inline math `\(...\)`
        if let Some(after_lp) = rest.strip_prefix(r"\(")
            && let Some(end) = after_lp.find(r"\)")
        {
            let inner = &after_lp[..end];
            out.push('$');
            out.push_str(inner);
            out.push('$');
            i += 2 + end + 2;
            continue;
        }

        // 4. Inline math `$...$`
        if let Some(after_d) = rest.strip_prefix('$')
            && let Some(end) = find_single_dollar_end(after_d)
        {
            let inner = &after_d[..end];
            out.push('$');
            out.push_str(inner);
            out.push('$');
            i += 1 + end + 1;
            continue;
        }

        // 5. LaTeX quotes `` and ''
        if rest.starts_with("``") || rest.starts_with("''") {
            out.push('"');
            i += 2;
            continue;
        }

        // 6. Non-breaking space `~`
        if rest.starts_with('~') {
            out.push(' ');
            i += 1;
            continue;
        }

        // 7. Backslash commands and environments
        if let Some(after_bs) = rest.strip_prefix('\\') {
            if let Some(after_begin) = rest.strip_prefix(r"\begin")
                && let Some((env_name, after_name)) = parse_braced_arg(after_begin.trim_start())
            {
                let end_tag = format!(r"\end{{{env_name}}}");
                if let Some(end_pos) = find_matching_env_end(after_name, &env_name) {
                    let env_body = &after_name[..end_pos];
                    let full_end_offset = (after_name.as_ptr() as usize - rest.as_ptr() as usize)
                        + end_pos
                        + end_tag.len();
                    let full_block = &rest[..full_end_offset];
                    convert_environment(
                        &env_name,
                        env_body,
                        full_block,
                        &mut out,
                        images,
                        warnings,
                        source_path,
                    );
                    i += full_end_offset;
                    continue;
                }
            }

            // Explicit line break `\\`
            if rest.starts_with(r"\\") {
                out.push_str("  \n");
                i += 2;
                // Skip optional `[height]` after `\\`
                if let Some(after_opt) = skip_bracket_arg(&input[i..]) {
                    i = input.len() - after_opt.len();
                }
                continue;
            }

            // Escaped punctuation: \%, \_, \&, \#, \$, \{, \}
            if let Some(ch) = after_bs.chars().next()
                && matches!(ch, '%' | '_' | '&' | '#' | '$' | '{' | '}')
            {
                if ch == '$' {
                    out.push_str(r"\$");
                } else {
                    out.push(ch);
                }
                i += 1 + ch.len_utf8();
                continue;
            }

            // `\ ` (explicit space)
            if rest.starts_with(r"\ ") {
                out.push(' ');
                i += 2;
                continue;
            }

            // Named command `\foo...`
            let cmd_len = after_bs
                .bytes()
                .take_while(|b| b.is_ascii_alphabetic() || *b == b'*')
                .count();
            if cmd_len > 0 {
                let cmd = &after_bs[..cmd_len];
                let after_cmd = &after_bs[cmd_len..];
                if let Some(consumed) =
                    handle_command(cmd, after_cmd, &mut out, images, warnings, source_path)
                {
                    i += 1 + cmd_len + consumed;
                    continue;
                }
            }
        }

        // Default: copy one UTF-8 char.
        let ch = rest.chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }

    out
}

fn convert_environment(
    env: &str,
    body: &str,
    full_block: &str,
    out: &mut String,
    images: &mut Vec<String>,
    warnings: &mut Vec<PackageWarning>,
    source_path: &str,
) {
    match env {
        "itemize" => {
            ensure_blank_line(out);
            for item in split_latex_items(body) {
                let converted = convert_inner(&item, images, warnings, source_path);
                let mut lines = converted.trim().lines();
                if let Some(first) = lines.next() {
                    out.push_str("- ");
                    out.push_str(first.trim());
                    out.push('\n');
                    for cont in lines {
                        out.push_str("  ");
                        out.push_str(cont);
                        out.push('\n');
                    }
                }
            }
            out.push('\n');
        }
        "enumerate" => {
            ensure_blank_line(out);
            for (idx, item) in split_latex_items(body).into_iter().enumerate() {
                let converted = convert_inner(&item, images, warnings, source_path);
                let mut lines = converted.trim().lines();
                if let Some(first) = lines.next() {
                    out.push_str(&format!("{}. ", idx + 1));
                    out.push_str(first.trim());
                    out.push('\n');
                    for cont in lines {
                        out.push_str("   ");
                        out.push_str(cont);
                        out.push('\n');
                    }
                }
            }
            out.push('\n');
        }
        "verbatim" | "lstlisting" => {
            ensure_blank_line(out);
            out.push_str("```\n");
            out.push_str(body.trim_matches('\n'));
            out.push_str("\n```\n\n");
        }
        "center" => {
            // If `\begin{center}` contains complex environments like `tabular`/`tikzpicture`,
            // convert the inner body normally (which will delegate those to `tylax`).
            ensure_blank_line(out);
            let converted = convert_inner(body.trim(), images, warnings, source_path);
            out.push_str(converted.trim());
            out.push_str("\n\n");
        }
        "quote" | "quotation" => {
            ensure_blank_line(out);
            let converted = convert_inner(body.trim(), images, warnings, source_path);
            for line in converted.trim().lines() {
                out.push_str("> ");
                out.push_str(line);
                out.push('\n');
            }
            out.push('\n');
        }
        "equation" | "equation*" | "align" | "align*" | "gather" | "gather*" | "multline"
        | "multline*" | "flalign" | "flalign*" => {
            ensure_blank_line(out);
            out.push_str("$$\n");
            out.push_str(full_block.trim());
            out.push_str("\n$$\n\n");
        }
        _ => {
            // Complex LaTeX environment (e.g. `tabular`, `tikzpicture`, `figure`, `algorithm`):
            // Delegate to `tylax` and emit a ```` ```typst ```` raw block.
            ensure_blank_line(out);
            let typst_code = tylax::convert_document_with_ast(full_block);
            let trimmed_typst = typst_code.trim();
            out.push_str("```typst\n");
            out.push_str(trimmed_typst);
            out.push_str("\n```\n\n");
            warnings.push(PackageWarning::with_path(
                "latex_fallback_typst",
                format!(
                    "complex LaTeX environment `\\begin{{{env}}}` was converted to a Typst raw block via tylax"
                ),
                source_path,
            ));
        }
    }
}

fn handle_command(
    cmd: &str,
    after_cmd: &str,
    out: &mut String,
    images: &mut Vec<String>,
    warnings: &mut Vec<PackageWarning>,
    source_path: &str,
) -> Option<usize> {
    let base_ptr = after_cmd.as_ptr() as usize;
    let consumed_of = |rem: &str| rem.as_ptr() as usize - base_ptr;

    match cmd {
        "textbf" | "bf" | "bfseries" => {
            if let Some((arg, rem)) = parse_braced_arg(after_cmd.trim_start()) {
                let inner = convert_inner(&arg, images, warnings, source_path);
                out.push_str("**");
                out.push_str(inner.trim());
                out.push_str("**");
                return Some(consumed_of(rem));
            }
            None
        }
        "textit" | "emph" | "it" | "itshape" => {
            if let Some((arg, rem)) = parse_braced_arg(after_cmd.trim_start()) {
                let inner = convert_inner(&arg, images, warnings, source_path);
                out.push('*');
                out.push_str(inner.trim());
                out.push('*');
                return Some(consumed_of(rem));
            }
            None
        }
        "texttt" | "t" | "tt" => {
            if let Some((arg, rem)) = parse_braced_arg(after_cmd.trim_start()) {
                out.push('`');
                out.push_str(&arg);
                out.push('`');
                return Some(consumed_of(rem));
            }
            None
        }
        "sout" => {
            if let Some((arg, rem)) = parse_braced_arg(after_cmd.trim_start()) {
                let inner = convert_inner(&arg, images, warnings, source_path);
                out.push_str("~~");
                out.push_str(inner.trim());
                out.push_str("~~");
                return Some(consumed_of(rem));
            }
            None
        }
        "underline" | "textsc" | "text" | "textrm" | "textsf" | "mbox" => {
            if let Some((arg, rem)) = parse_braced_arg(after_cmd.trim_start()) {
                let inner = convert_inner(&arg, images, warnings, source_path);
                out.push_str(&inner);
                return Some(consumed_of(rem));
            }
            None
        }
        "url" => {
            if let Some((arg, rem)) = parse_braced_arg(after_cmd.trim_start()) {
                out.push('<');
                out.push_str(arg.trim());
                out.push('>');
                return Some(consumed_of(rem));
            }
            None
        }
        "href" => {
            if let Some((url, rem1)) = parse_braced_arg(after_cmd.trim_start())
                && let Some((label, rem2)) = parse_braced_arg(rem1.trim_start())
            {
                let inner = convert_inner(&label, images, warnings, source_path);
                out.push('[');
                out.push_str(inner.trim());
                out.push_str("](");
                out.push_str(url.trim());
                out.push(')');
                return Some(consumed_of(rem2));
            }
            None
        }
        "includegraphics" => {
            let mut cur = after_cmd.trim_start();
            if let Some(after_opt) = skip_bracket_arg(cur) {
                cur = after_opt.trim_start();
            }
            if let Some((raw_img, rem)) = parse_braced_arg(cur) {
                let raw_img = raw_img.trim().to_owned();
                let filename = std::path::Path::new(&raw_img)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(&raw_img)
                    .to_owned();
                images.push(raw_img);
                out.push_str(&format!("![](assets/{filename})"));
                return Some(consumed_of(rem));
            }
            None
        }
        "verb" => {
            let mut chars = after_cmd.chars();
            if let Some(delim) = chars.next() {
                let body = &after_cmd[delim.len_utf8()..];
                if let Some(end) = body.find(delim) {
                    out.push('`');
                    out.push_str(&body[..end]);
                    out.push('`');
                    return Some(delim.len_utf8() + end + delim.len_utf8());
                }
            }
            None
        }
        "par" => {
            out.push_str("\n\n");
            Some(0)
        }
        "newline" => {
            out.push_str("  \n");
            Some(0)
        }
        "ldots" | "dots" => {
            out.push_str("...");
            Some(0)
        }
        "epigraph" => {
            if let Some((quote, rem1)) = parse_braced_arg(after_cmd.trim_start())
                && let Some((author, rem2)) = parse_braced_arg(rem1.trim_start())
            {
                ensure_blank_line(out);
                let q = convert_inner(&quote, images, warnings, source_path);
                let a = convert_inner(&author, images, warnings, source_path);
                for line in q.trim().lines() {
                    out.push_str("> ");
                    out.push_str(line);
                    out.push('\n');
                }
                if !a.trim().is_empty() {
                    out.push_str("> — ");
                    out.push_str(a.trim());
                    out.push('\n');
                }
                out.push('\n');
                return Some(consumed_of(rem2));
            }
            None
        }
        // Ignore pure LaTeX layout commands that carry no Markdown content.
        "noindent" | "centering" | "smallskip" | "medskip" | "bigskip" | "clearpage"
        | "newpage" => Some(0),
        "vspace" | "hspace" => {
            let cur = after_cmd.trim_start();
            if let Some((_, rem)) = parse_braced_arg(cur) {
                Some(consumed_of(rem))
            } else {
                Some(0)
            }
        }
        _ => None,
    }
}

fn split_latex_items(body: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut current = String::new();
    let mut started = false;
    let mut depth = 0usize;
    let bytes = body.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        let rest = &body[i..];
        if rest.starts_with(r"\begin{") {
            depth += 1;
            current.push_str(r"\begin{");
            i += 7;
            continue;
        }
        if rest.starts_with(r"\end{") {
            depth = depth.saturating_sub(1);
            current.push_str(r"\end{");
            i += 5;
            continue;
        }
        if depth == 0
            && let Some(after) = rest.strip_prefix(r"\item")
        {
            let next_is_alpha = after
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic());
            if !next_is_alpha {
                if started {
                    items.push(current.trim().to_owned());
                    current.clear();
                }
                started = true;
                i += 5;
                // Skip optional `\item[label]`
                if let Some(after_opt) = skip_bracket_arg(body[i..].trim_start()) {
                    i = body.len() - after_opt.len();
                }
                continue;
            }
        }
        let ch = rest.chars().next().unwrap();
        if started {
            current.push(ch);
        }
        i += ch.len_utf8();
    }
    if started && !current.trim().is_empty() {
        items.push(current.trim().to_owned());
    }
    items
}

fn parse_braced_arg(s: &str) -> Option<(String, &str)> {
    if !s.starts_with('{') {
        return None;
    }
    let mut depth = 0usize;
    let mut escaped = false;
    for (i, ch) in s.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
        } else if ch == '{' {
            depth += 1;
        } else if ch == '}' {
            depth -= 1;
            if depth == 0 {
                return Some((s[1..i].to_owned(), &s[i + 1..]));
            }
        }
    }
    None
}

fn skip_bracket_arg(s: &str) -> Option<&str> {
    if !s.starts_with('[') {
        return None;
    }
    let mut depth = 0usize;
    let mut escaped = false;
    for (i, ch) in s.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
        } else if ch == '[' {
            depth += 1;
        } else if ch == ']' {
            depth -= 1;
            if depth == 0 {
                return Some(&s[i + 1..]);
            }
        }
    }
    None
}

fn find_matching_env_end(body: &str, env: &str) -> Option<usize> {
    let begin_tag = format!(r"\begin{{{env}}}");
    let end_tag = format!(r"\end{{{env}}}");
    let mut depth = 1usize;
    let mut i = 0;
    while i < body.len() {
        let rest = &body[i..];
        if rest.starts_with(&begin_tag) {
            depth += 1;
            i += begin_tag.len();
        } else if rest.starts_with(&end_tag) {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
            i += end_tag.len();
        } else {
            let ch = rest.chars().next().unwrap();
            i += ch.len_utf8();
        }
    }
    None
}

fn find_unescaped(s: &str, needle: &str) -> Option<usize> {
    let mut i = 0;
    let mut escaped = false;
    while i < s.len() {
        let rest = &s[i..];
        if !escaped && rest.starts_with(needle) {
            return Some(i);
        }
        let ch = rest.chars().next().unwrap();
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        }
        i += ch.len_utf8();
    }
    None
}

fn find_single_dollar_end(s: &str) -> Option<usize> {
    let mut i = 0;
    let mut escaped = false;
    while i < s.len() {
        let rest = &s[i..];
        let ch = rest.chars().next().unwrap();
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '$' {
            return Some(i);
        }
        i += ch.len_utf8();
    }
    None
}

fn ensure_blank_line(out: &mut String) {
    if out.is_empty() {
        return;
    }
    if !out.ends_with('\n') {
        out.push('\n');
    }
    if !out.ends_with("\n\n") {
        out.push('\n');
    }
}

fn collapse_blank_lines(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut blank_run = 0usize;
    for line in input.lines() {
        if line.trim().is_empty() {
            blank_run += 1;
            if blank_run <= 1 && !out.is_empty() {
                out.push('\n');
            }
        } else {
            blank_run = 0;
            out.push_str(line.trim_end());
            out.push('\n');
        }
    }
    out.trim().to_owned()
}
