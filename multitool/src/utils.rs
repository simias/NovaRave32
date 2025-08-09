pub fn format_size(sz: u64) -> String {
    format!("{}B ({})", sz, humansize::format_size(sz, humansize::BINARY))
}
