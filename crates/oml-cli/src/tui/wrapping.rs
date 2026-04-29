use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

#[derive(Clone, Debug)]
pub(super) struct WrapOptions<'a> {
    width: usize,
    initial_indent: Line<'a>,
    subsequent_indent: Line<'a>,
    break_words: bool,
}

impl<'a> WrapOptions<'a> {
    pub(super) fn new(width: usize) -> Self {
        Self {
            width,
            initial_indent: Line::default(),
            subsequent_indent: Line::default(),
            break_words: true,
        }
    }

    pub(super) fn initial_indent(mut self, indent: impl Into<Line<'a>>) -> Self {
        self.initial_indent = indent.into();
        self
    }

    pub(super) fn subsequent_indent(mut self, indent: impl Into<Line<'a>>) -> Self {
        self.subsequent_indent = indent.into();
        self
    }

    pub(super) fn break_words(mut self, break_words: bool) -> Self {
        self.break_words = break_words;
        self
    }
}

impl From<usize> for WrapOptions<'_> {
    fn from(width: usize) -> Self {
        Self::new(width)
    }
}

pub(super) fn adaptive_wrap_line<'a>(
    line: &'a Line<'a>,
    options: WrapOptions<'a>,
) -> Vec<Line<'static>> {
    let options = if line_contains_url_like(line) {
        options.break_words(false)
    } else {
        options
    };
    word_wrap_line(line, options)
}

pub(super) fn adaptive_wrap_lines<'a, I>(lines: I, options: WrapOptions<'a>) -> Vec<Line<'static>>
where
    I: IntoIterator<Item = Line<'a>>,
{
    let mut out = Vec::new();
    for (index, line) in lines.into_iter().enumerate() {
        let line_options = if index == 0 {
            options.clone()
        } else {
            options
                .clone()
                .initial_indent(options.subsequent_indent.clone())
        };
        out.extend(adaptive_wrap_line(&line, line_options));
    }
    out
}

pub(super) fn word_wrap_line<'a>(
    line: &'a Line<'a>,
    options: WrapOptions<'a>,
) -> Vec<Line<'static>> {
    let flat = line
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();
    let text_options = textwrap::Options::new(options.width)
        .break_words(options.break_words)
        .word_separator(textwrap::WordSeparator::new())
        .word_splitter(if options.break_words {
            textwrap::WordSplitter::HyphenSplitter
        } else {
            textwrap::WordSplitter::NoHyphenation
        })
        .wrap_algorithm(textwrap::WrapAlgorithm::FirstFit);
    let wrapped = textwrap::wrap(&flat, text_options);
    if wrapped.is_empty() {
        return vec![owned_line_with_indent(&options.initial_indent, "")];
    }

    wrapped
        .iter()
        .enumerate()
        .map(|(index, text)| {
            let indent = if index == 0 {
                &options.initial_indent
            } else {
                &options.subsequent_indent
            };
            owned_line_with_indent(indent, text)
        })
        .collect()
}

fn owned_line_with_indent(indent: &Line<'_>, text: &str) -> Line<'static> {
    let mut spans = indent
        .spans
        .iter()
        .map(|span| Span::styled(span.content.to_string(), span.style))
        .collect::<Vec<_>>();
    spans.push(Span::raw(text.to_owned()));
    Line::from(spans)
}

fn line_contains_url_like(line: &Line<'_>) -> bool {
    line.spans
        .iter()
        .any(|span| text_contains_url_like(span.content.as_ref()))
}

fn text_contains_url_like(text: &str) -> bool {
    text.split_whitespace().any(is_url_like_token)
}

fn is_url_like_token(token: &str) -> bool {
    token.starts_with("http://")
        || token.starts_with("https://")
        || token.starts_with("file://")
        || (token.contains('.') && token.contains('/') && token.width() > 12)
}
