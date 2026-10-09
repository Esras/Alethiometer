use super::*;

#[test]
fn gist_skips_frontmatter_and_markup() {
    let g = note_gist("---\ntags: [x]\n---\n\n## Where we are\n- Offer: 12% off\n- [ ] True-down\n");
    assert_eq!(g, "Where we are / Offer: 12% off / True-down");
}
