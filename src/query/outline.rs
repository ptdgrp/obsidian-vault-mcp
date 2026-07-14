use super::{
    NoteOutlinePagination, NoteOutlineResult, OutlineHeading, VaultQueries, public::PageSlice,
};

const NOTE_OUTLINE_PAGE_SIZE: usize = 100;

impl VaultQueries {
    pub fn get_note_outline(&self, note: &str, page: usize) -> anyhow::Result<NoteOutlineResult> {
        let parsed = self.parse_note(note)?;
        let headings = parsed
            .headings
            .iter()
            .filter(|heading| heading.level != 1)
            .map(|heading| OutlineHeading {
                heading: heading.path.join("/"),
                level: heading.level,
                line: heading.source.line_start,
            })
            .collect::<Vec<_>>();
        let total_headings = headings.len();
        let total_pages = total_headings.div_ceil(NOTE_OUTLINE_PAGE_SIZE);
        if page > total_pages.max(1) {
            anyhow::bail!("page {page} out of range; total pages: {total_pages}");
        }
        let slice = PageSlice::new(headings, page, NOTE_OUTLINE_PAGE_SIZE)?;
        let pagination = slice.pagination();
        Ok(NoteOutlineResult {
            note: parsed.path,
            headings: slice.into_items(),
            pagination: NoteOutlinePagination {
                page: pagination.page,
                total_pages: pagination.total_pages,
                total_headings,
            },
        })
    }
}
