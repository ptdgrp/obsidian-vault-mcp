pub fn parse_byte_size(input: &str) -> Result<usize, String> {
    let input = input.trim();
    let invalid = || {
        format!(
            "invalid byte size '{input}'; expected bytes or b, k, m suffixes, such as 4096, 2k, 1m, or 12.4k"
        )
    };
    let suffix_start = input
        .find(|character: char| character.is_ascii_alphabetic())
        .unwrap_or(input.len());
    let (number, suffix) = input.split_at(suffix_start);
    let multiplier = match suffix.to_ascii_lowercase().as_str() {
        "" | "b" => 1_u128,
        "k" => 1024_u128,
        "m" => 1024_u128 * 1024_u128,
        _ => return Err(invalid()),
    };
    let (whole, fraction) = match number.split_once('.') {
        Some((whole, fraction)) if !fraction.is_empty() => (whole, fraction),
        Some(_) => return Err(invalid()),
        None => (number, ""),
    };
    if whole.is_empty()
        || !whole.chars().all(|character| character.is_ascii_digit())
        || !fraction.chars().all(|character| character.is_ascii_digit())
    {
        return Err(invalid());
    }

    let scale = 10_u128
        .checked_pow(u32::try_from(fraction.len()).map_err(|_| invalid())?)
        .ok_or_else(invalid)?;
    let whole = whole.parse::<u128>().map_err(|_| invalid())?;
    let fraction = if fraction.is_empty() {
        0
    } else {
        fraction.parse::<u128>().map_err(|_| invalid())?
    };
    let scaled = whole
        .checked_mul(scale)
        .and_then(|value| value.checked_add(fraction))
        .ok_or_else(invalid)?;
    let bytes = scaled.checked_mul(multiplier).ok_or_else(invalid)?;
    let rounded = bytes
        .checked_add(scale.saturating_sub(1))
        .ok_or_else(invalid)?
        / scale;
    usize::try_from(rounded).map_err(|_| invalid())
}

#[cfg(test)]
mod tests {
    use super::parse_byte_size;

    #[test]
    fn parses_byte_size_suffixes_and_decimals() {
        assert_eq!(parse_byte_size("1m"), Ok(1024 * 1024));
        assert_eq!(parse_byte_size("2k"), Ok(2 * 1024));
        assert_eq!(parse_byte_size("2b"), Ok(2));
        assert_eq!(parse_byte_size("12.4k"), Ok(12_698));
    }

    #[test]
    fn rejects_invalid_byte_size_literals() {
        assert!(parse_byte_size("1g").is_err());
        assert!(parse_byte_size(".4k").is_err());
        assert!(parse_byte_size("12.4.5k").is_err());
    }
}
