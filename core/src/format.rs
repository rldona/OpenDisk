//! Small formatting helpers shared by the CLI and the app.

/// Human readable size in decimal units, like Finder ("1.5 GB").
pub fn bytes(n: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    if n < 1000 {
        return format!("{n} B");
    }
    let mut v = n as f64;
    let mut unit = 0;
    while v >= 1000.0 && unit < UNITS.len() - 1 {
        v /= 1000.0;
        unit += 1;
    }
    if v >= 100.0 {
        format!("{v:.0} {}", UNITS[unit])
    } else {
        format!("{v:.1} {}", UNITS[unit])
    }
}

/// Parses sizes such as `500`, `10MB`, `1.5 GB`, `2g` (decimal units).
pub fn parse_bytes(s: &str) -> Option<u64> {
    let s = s.trim().to_ascii_lowercase();
    let split = s.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(s.len());
    let (num, unit) = s.split_at(split);
    let num: f64 = num.parse().ok()?;
    let mult = match unit.trim().trim_end_matches('b') {
        "" => 1.0,
        "k" => 1e3,
        "m" => 1e6,
        "g" => 1e9,
        "t" => 1e12,
        _ => return None,
    };
    Some((num * mult) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_and_parses() {
        assert_eq!(bytes(512), "512 B");
        assert_eq!(bytes(1_500_000_000), "1.5 GB");
        assert_eq!(bytes(180_000_000_000), "180 GB");
        assert_eq!(parse_bytes("10MB"), Some(10_000_000));
        assert_eq!(parse_bytes("1.5 GB"), Some(1_500_000_000));
        assert_eq!(parse_bytes("2g"), Some(2_000_000_000));
        assert_eq!(parse_bytes("7"), Some(7));
        assert_eq!(parse_bytes("x"), None);
    }
}
