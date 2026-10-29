use super::{CODE_REJECTION_LIMIT, code_from_entropy, verification_code};

#[test]
fn rejection_sampling_discards_the_biased_tail() {
    assert_eq!(CODE_REJECTION_LIMIT, 4_294_000_000);
    let mut values = [CODE_REJECTION_LIMIT, u32::MAX, 42].into_iter();
    assert_eq!(
        code_from_entropy(|| values.next()).as_deref(),
        Some("000042")
    );

    let mut values = [CODE_REJECTION_LIMIT - 1].into_iter();
    assert_eq!(
        code_from_entropy(|| values.next()).as_deref(),
        Some("999999")
    );

    assert!(code_from_entropy(|| None).is_none());
}

#[test]
fn system_random_codes_are_uniform_six_digit_strings() {
    let mut seen_high = false;
    let mut seen_low = false;
    for _ in 0..512 {
        let code = verification_code().expect("system randomness is available");
        assert_eq!(code.len(), 6);
        assert!(code.bytes().all(|byte| byte.is_ascii_digit()));
        let value: u32 = code.parse().expect("numeric code");
        assert!(value < 1_000_000);
        seen_high |= value >= 500_000;
        seen_low |= value < 500_000;
    }
    assert!(seen_high && seen_low, "generator must span the code range");
}
