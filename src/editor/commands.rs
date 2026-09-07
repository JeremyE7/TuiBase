use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisualMode {
    Character,
    Line,
    Block,
}

impl fmt::Display for VisualMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            Self::Character => "VISUAL",
            Self::Line => "V-LINE",
            Self::Block => "V-BLOCK",
        };
        f.write_str(label)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operator {
    Delete,
    Change,
    Yank,
}
