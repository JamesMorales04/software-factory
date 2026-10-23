use std::env;

pub(crate) const DEFAULT_CORE_URL: &str = "http://127.0.0.1:8765";
pub(crate) const DEFAULT_IMAGE: &str = "localhost/software-factory-core:dev";

pub(crate) fn core_url() -> Result<String, String> {
    read("FACTORY_CORE_URL", DEFAULT_CORE_URL)
}

pub(crate) fn core_url_is_explicit() -> bool {
    env::var_os("FACTORY_CORE_URL").is_some()
}

pub(crate) fn image() -> Result<String, String> {
    read("FACTORY_IMAGE", DEFAULT_IMAGE)
}

fn read(name: &str, default: &str) -> Result<String, String> {
    match env::var(name) {
        Ok(value) => Ok(value),
        Err(env::VarError::NotPresent) => Ok(default.to_owned()),
        Err(env::VarError::NotUnicode(_)) => Err(format!("{name} is not valid UTF-8")),
    }
}
