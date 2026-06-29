use xxhash_rust::xxh3::Xxh3;

pub fn content_hash(content: &[u8]) -> u64 {
    let mut hasher = Xxh3::new();
    hasher.update(content);
    hasher.digest()
}

pub fn hash_to_string(hash: u64) -> String {
    format!("{:016x}", hash)
}
