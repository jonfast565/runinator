#[allow(unused_imports)]
use super::*;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct StyledLine {
    pub(crate) plain: String,
    pub(super) spans: Vec<StyledSpan>,
}

impl StyledLine {
    pub(crate) fn to_ratatui_line(&self) -> Line<'static> {
        Line::from(
            self.spans
                .iter()
                .map(|span| Span::styled(span.text.clone(), span.style))
                .collect::<Vec<_>>(),
        )
    }
}
