use crate::query::public::{Locator, PageSlice, ResolvedReference};

#[test]
fn page_slice_uses_one_based_pages_and_preserves_real_totals() {
    let empty = PageSlice::<usize>::new(Vec::new(), 1, 100).expect("empty first page");
    assert_eq!(empty.total_items(), 0);
    assert_eq!(empty.pagination().page, 1);
    assert_eq!(empty.pagination().total_pages, 0);
    assert!(empty.items().is_empty());

    let out_of_range = PageSlice::new(vec![1, 2, 3], 3, 2).expect("out of range page");
    assert_eq!(out_of_range.total_items(), 3);
    assert_eq!(out_of_range.pagination().page, 3);
    assert_eq!(out_of_range.pagination().total_pages, 2);
    assert!(out_of_range.items().is_empty());
    assert!(PageSlice::<usize>::new(Vec::new(), 0, 100).is_err());
}

#[test]
fn locators_and_references_preserve_obsidian_navigation_semantics() {
    assert_eq!(Locator::lines("人物/林动.md", 12, 12), "人物/林动.md#L12");
    assert_eq!(
        Locator::lines("人物/林动.md", 12, 38),
        "人物/林动.md#L12-L38"
    );

    let parent = ResolvedReference::heading("人物/林动.md", vec!["身体".into()]);
    let child = ResolvedReference::heading("人物/林动.md", vec!["身体".into(), "伤势".into()]);
    let block = ResolvedReference::block("人物/林动.md", "profile");

    assert_eq!(child.format(), "人物/林动.md#身体#伤势");
    assert!(parent.contains(&child));
    assert!(!parent.contains(&block));
    assert!(block.contains(&block));
}
