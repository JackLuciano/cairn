const SERVICE: &str = "dev.cairn.app";

fn entry(name: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, name).map_err(|e| e.to_string())
}

pub(crate) fn get(name: &str) -> Option<String> {
    entry(name).ok()?.get_password().ok().filter(|s| !s.is_empty())
}

pub(crate) fn set(name: &str, value: &str) -> Result<(), String> {
    entry(name)?.set_password(value).map_err(|e| e.to_string())
}

pub(crate) fn delete(name: &str) -> Result<(), String> {
    entry(name)?.delete_credential().map_err(|e| e.to_string())
}
