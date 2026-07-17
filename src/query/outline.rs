use super::{
    NoteOutlinePagination, NoteOutlineResult, OutlineHeading, VaultQueries, public::PageSlice,
};

const NOTE_OUTLINE_PAGE_SIZE: usize = 100;

impl VaultQueries {
    #[tracing::instrument(
        name = "vault.query.get_note_outline",
        fields(operation.kind = "query", operation.name = "get_note_outline"),
        err
    )]
    pub fn get_note_outline(&self, note: &str, page: usize) -> anyhow::Result<NoteOutlineResult> {
        let (parsed, _) = self.parse_note(note)?;
        let headings = parsed
            .headings
            .iter()
            .filter(|heading| heading.level != 1)
            .map(|heading| OutlineHeading {
                heading: heading.path.join("/"),
                line: heading.source.line_start,
            })
            .collect::<Vec<_>>();
        let total_headings = headings.len();
        let slice = PageSlice::new(headings, page, NOTE_OUTLINE_PAGE_SIZE)?;
        let pagination = slice.pagination();
        Ok(NoteOutlineResult {
            note: parsed.path.to_owned(),
            headings: slice.into_items(),
            pagination: NoteOutlinePagination {
                page: pagination.page,
                total_pages: pagination.total_pages,
                total_headings,
            },
        })
    }
}
