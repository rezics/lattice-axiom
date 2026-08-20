pub(crate) fn is_canonical_identifier_segment(value: &str) -> bool {
    let mut characters = value.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        return false;
    }

    let mut last = first;
    for character in characters {
        if !character.is_ascii_lowercase()
            && !character.is_ascii_digit()
            && !matches!(character, '-' | '_' | '.')
        {
            return false;
        }
        last = character;
    }
    last.is_ascii_lowercase() || last.is_ascii_digit()
}
