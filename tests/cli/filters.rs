const FILTERS: &[(&str, &str)] = &[
    (r#"/(?:private/)?tmp/[^\s"]+"#, "[TEMP_PATH]"),
    (r#"/(?:private/)?var/folders/[^\s"]+/T/[^\s"]+"#, "[TEMP_PATH]"),
    (r#"[A-Za-z]:\\[^\r\n]*\\Temp\\[^\s"]+"#, "[TEMP_PATH]"),
    // clap prints argv[0], which has the .exe suffix on Windows.
    (r"\.exe\b", ""),
    (r"\b\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?\b", "[VERSION]"),
    (
        r"\b\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})\b",
        "[TIMESTAMP]",
    ),
];

pub fn with_snapshot_filters(assertion: impl FnOnce()) {
    let mut settings = insta::Settings::clone_current();
    for &(pattern, replacement) in FILTERS {
        settings.add_filter(pattern, replacement);
    }
    settings.bind(assertion);
}

#[cfg(test)]
mod tests {
    use super::with_snapshot_filters;

    #[test]
    fn normalizes_unstable_values() {
        with_snapshot_filters(|| {
            insta::assert_snapshot!(
                "version 1.2.3\ncreated 2026-07-27T14:30:45Z\nunix /tmp/dataseek/run.log\nmacos /var/folders/ab/cdef/T/dataseek/run.log\nwindows C:\\Users\\dev\\AppData\\Local\\Temp\\dataseek\\run.log\nusage dataseek.exe [OPTIONS]",
                @r"
            version [VERSION]
            created [TIMESTAMP]
            unix [TEMP_PATH]
            macos [TEMP_PATH]
            windows [TEMP_PATH]
            usage dataseek [OPTIONS]"
            );
        });
    }
}
