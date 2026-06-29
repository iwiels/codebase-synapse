pub fn estimate_tokens(text: &str) -> usize {
    (text.len() as f64 * 0.25) as usize
}

pub fn truncate_to_tokens(text: &str, max_tokens: usize) -> &str {
    let max_bytes = (max_tokens as f64 * 4.0) as usize;
    if text.len() <= max_bytes {
        return text;
    }
    let mut end = max_bytes;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}
