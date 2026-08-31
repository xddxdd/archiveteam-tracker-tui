pub fn human_bytes(b: u64) -> String {
    const UNITS: [&str; 7] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB"];
    let mut v = b as f64;
    let mut i = 0usize;
    while v >= 1024.0 && i < 6 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{b} B")
    } else {
        format!("{v:.1} {}", UNITS[i])
    }
}

pub fn compact(n: u64) -> String {
    if n < 1_000 {
        n.to_string()
    } else if n < 1_000_000 {
        format!("{:.1}K", n as f64 / 1e3)
    } else if n < 1_000_000_000 {
        format!("{:.1}M", n as f64 / 1e6)
    } else {
        format!("{:.1}B", n as f64 / 1e9)
    }
}

pub fn speed(bytes_per_sec: f64) -> String {
    if bytes_per_sec < 1.0 {
        format!("{bytes_per_sec:.1} B/s")
    } else {
        format!("{}/s", human_bytes(bytes_per_sec as u64))
    }
}

pub fn duration(d: std::time::Duration) -> String {
    let s = d.as_secs();
    let h = s / 3600;
    let m = (s % 3600) / 60;
    let sec = s % 60;
    if h > 0 {
        format!("{h}h{m:02}m")
    } else {
        format!("{m}m{sec:02}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes() {
        assert_eq!(human_bytes(0), "0 B");
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(1024), "1.0 KiB");
        assert_eq!(human_bytes(3 * 1024 * 1024 * 1024), "3.0 GiB");
    }

    #[test]
    fn counts() {
        assert_eq!(compact(999), "999");
        assert_eq!(compact(75_647), "75.6K");
        assert_eq!(compact(64_231_156), "64.2M");
        assert_eq!(compact(1_100_000_000), "1.1B");
    }
}
