//! Test-only RSS accounting over our WebKit descendant processes.
//!
//! Used by the `AXYS_MEM_SMOKE` self-test (see `ui::window`) and CI: it
//! walks `/proc` collecting every descendant of this process (sandbox
//! helpers included) and sums resident pages. Page size is assumed to be
//! 4096 bytes, true on every mainstream Linux target.

use std::collections::{HashMap, VecDeque};
use std::fs;

/// Resident memory of all current descendant processes, in KiB.
pub fn descendants_rss_kb() -> u64 {
    const PAGE_KB: u64 = 4;

    // pid -> (ppid, rss_pages)
    let mut table: HashMap<u32, (u32, u64)> = HashMap::new();

    let Ok(entries) = fs::read_dir("/proc") else {
        return 0;
    };

    for entry in entries.flatten() {
        let name = entry.file_name();

        let Some(pid) = name.to_str().and_then(|n| n.parse::<u32>().ok()) else {
            continue;
        };

        let Ok(stat) = fs::read_to_string(entry.path().join("stat")) else {
            continue;
        };

        // comm may contain spaces/parens: fields start after the last ')'.
        let Some(close) = stat.rfind(')') else {
            continue;
        };

        let fields: Vec<&str> = stat[close + 1..].split_whitespace().collect();

        if fields.len() < 22 {
            continue;
        }

        let ppid = fields[1].parse::<u32>().unwrap_or(0);
        let rss_pages = fields[21].parse::<u64>().unwrap_or(0);

        table.insert(pid, (ppid, rss_pages));
    }

    let me = std::process::id();
    let mut total_pages = 0u64;
    let mut queue = VecDeque::from([me]);

    while let Some(pid) = queue.pop_front() {
        for (child, (ppid, rss)) in table.iter() {
            if *ppid == pid {
                total_pages += rss;
                queue.push_back(*child);
            }
        }
    }

    total_pages * PAGE_KB
}
