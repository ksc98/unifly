//! Human-readable byte and duration formatting helpers.

/// Format bytes into a compact human-readable string (e.g., "245M", "1.2G").
pub fn fmt_bytes_short(bytes: u64) -> String {
    if bytes >= 1_000_000_000 {
        format!("{:.1}G", bytes as f64 / 1_000_000_000.0)
    } else if bytes >= 1_000_000 {
        format!("{}M", bytes / 1_000_000)
    } else if bytes >= 1_000 {
        format!("{}K", bytes / 1_000)
    } else {
        format!("{bytes}B")
    }
}

/// Format a TX/RX byte pair as "245M/52M".
pub fn fmt_tx_rx(tx: u64, rx: u64) -> String {
    format!("{}/{}", fmt_bytes_short(tx), fmt_bytes_short(rx))
}

/// Format seconds into a compact human duration (e.g., "47d", "4h 23m", "12m").
pub fn fmt_uptime(secs: u64) -> String {
    let days = secs / 86400;
    let hours = (secs % 86400) / 3600;
    let minutes = (secs % 3600) / 60;

    if days > 0 {
        format!("{days}d")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{minutes}m")
    }
}

/// Format a rate in bytes/sec as "245 Mbps".
pub fn fmt_rate(bytes_per_sec: u64) -> String {
    let bits = bytes_per_sec * 8;
    if bits >= 1_000_000_000 {
        format!("{:.1} Gbps", bits as f64 / 1_000_000_000.0)
    } else if bits >= 1_000_000 {
        format!("{:.1} Mbps", bits as f64 / 1_000_000.0)
    } else if bits >= 1_000 {
        format!("{:.1} Kbps", bits as f64 / 1_000.0)
    } else {
        format!("{bits} bps")
    }
}
