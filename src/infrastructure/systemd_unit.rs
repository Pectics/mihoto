pub(crate) fn escape_exec_arg(value: &str) -> String {
    if value
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || "/._:-".contains(character))
    {
        return value.to_string();
    }
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "$$")
        .replace('%', "%%");
    format!("\"{escaped}\"")
}

#[cfg(test)]
mod tests {
    use super::escape_exec_arg;

    #[test]
    fn escapes_systemd_exec_arguments() {
        assert_eq!(
            escape_exec_arg("/usr/local/bin/mihomo"),
            "/usr/local/bin/mihomo"
        );
        assert_eq!(escape_exec_arg("/opt/mihomo core"), "\"/opt/mihomo core\"");
        assert_eq!(
            escape_exec_arg("/opt/$mihomo%core"),
            "\"/opt/$$mihomo%%core\""
        );
    }
}
