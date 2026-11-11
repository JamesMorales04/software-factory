pub fn render(value: &str) -> String {
    serde_json::json!({ "ok": true, "value": value }).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_expected_json() {
        let rendered: serde_json::Value = serde_json::from_str(&render("factory")).unwrap();
        assert_eq!(rendered["ok"], true);
        assert_eq!(rendered["value"], "factory");
    }
}
