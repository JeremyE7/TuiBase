use std::sync::LazyLock;

use ratatui::{
    style::{Color, Style},
    text::{Line, Span, Text},
};
use syntect::{
    easy::HighlightLines, highlighting::ThemeSet, parsing::SyntaxSet, util::LinesWithEndings,
};

static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);
static THEME_SET: LazyLock<ThemeSet> = LazyLock::new(ThemeSet::load_defaults);

pub fn highlight_sql(source: &str) -> Text<'static> {
    // 3. Buscar la sintaxis SQL.
    let syntax = SYNTAX_SET.find_syntax_by_extension("sql").unwrap();

    // 4. Crear el resaltador.
    let mut highlighter = HighlightLines::new(syntax, &THEME_SET.themes["base16-ocean.dark"]);

    let mut rendered_lines: Vec<Line<'static>> = Vec::new();
    for line in LinesWithEndings::from(source) {
        let highlighted = highlighter.highlight_line(line, &SYNTAX_SET);

        match highlighted {
            Ok(segments) => {
                let mut line_sintaxed: Vec<Span<'static>> = Vec::new();
                for (style, fragment) in segments {
                    let color =
                        Color::Rgb(style.foreground.r, style.foreground.g, style.foreground.b);
                    let ratatui_style = Style::default().fg(color);
                    let fragment = fragment
                        .trim_end_matches(|character| character == '\r' || character == '\n')
                        .to_owned();
                    let span = Span::styled(fragment, ratatui_style);
                    line_sintaxed.push(span);
                }
                rendered_lines.push(Line::from(line_sintaxed));
            }
            Err(_) => {
                let plain_line = line
                    .trim_end_matches(|character| character == '\r' || character == '\n')
                    .to_owned();

                rendered_lines.push(Line::from(plain_line));
            }
        }
    }
    return Text::from(rendered_lines);
}
