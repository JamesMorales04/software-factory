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

#[cfg(test)]
mod tests {
    use super::one_line;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn output_never_contains_a_line_break(value in any::<String>(), max_chars in 0usize..256) {
            let rendered = one_line(&value, max_chars);
            prop_assert!(!rendered.contains('\n'));
            prop_assert!(!rendered.contains('\r'));
        }

        #[test]
        fn output_is_bounded(value in any::<String>(), max_chars in 0usize..256) {
            let rendered = one_line(&value, max_chars);
            prop_assert!(rendered.chars().count() <= max_chars.saturating_add(3));
        }

        #[test]
        fn plain_short_input_is_preserved(value in "[^\r\n]{0,64}") {
            let rendered = one_line(&value, 128);
            prop_assert_eq!(rendered, value);
        }
    }

    #[test]
    fn truncation_is_visible() {
        insta::assert_snapshot!(one_line("abcdef", 3), @"abc...");
    }
}
