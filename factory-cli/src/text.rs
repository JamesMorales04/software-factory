pub(crate) fn one_line(value: &str, max_chars: usize) -> String {
    let flattened = value.replace(['\r', '\n'], " ");
    let mut chars = flattened.chars();
    let kept: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{kept}...")
    } else {
        kept
    }
}
