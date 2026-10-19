//! Test-only memory accounting over this process and its WebKit
//! descendants.
//!
//! Used by the `AXYS_MEM_SMOKE` self-test (see `ui::memsmoke`) and CI. It
//! walks `/proc`, collects the main process plus every descendant (sandbox
//! helpers included) and sums their memory. PSS (proportional set size,
//! from `smaps_rollup`) is preferred because WebKit's processes share a lot
//! of memory and RSS would count it once per process; when `smaps_rollup`
//! is unreadable the resident size from `status` is used instead.

use std::collections::HashMap;
use std::fs;

/// One row of the process table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Proc {
    pub pid: u32,
    pub ppid: u32,
    pub comm: String,
}

/// Parses `/proc/<pid>/stat` into (comm, ppid). The command name may
/// contain spaces and parentheses, so fields start after the LAST ')'.
pub fn parse_stat(stat: &str) -> Option<(String, u32)> {
    let open = stat.find('(')?;
    let close = stat.rfind(')')?;

    if close < open {
        return None;
    }

    let comm = stat[open + 1..close].to_string();
    let ppid = stat[close + 1..].split_whitespace().nth(1)?.parse().ok()?;

    Some((comm, ppid))
}

/// Reads a `Key:   1234 kB` line out of `/proc` style text.
fn kb_field(text: &str, key: &str) -> Option<u64> {
    text.lines()
        .find_map(|line| line.strip_prefix(key)?.strip_prefix(':'))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|value| value.parse().ok())
}

/// PSS in KiB from `smaps_rollup` text.
pub fn parse_pss_kb(smaps_rollup: &str) -> Option<u64> {
    kb_field(smaps_rollup, "Pss")
}

/// Resident size in KiB from `status` text.
pub fn parse_rss_kb(status: &str) -> Option<u64> {
    kb_field(status, "VmRSS")
}

fn process_table() -> Vec<Proc> {
    let Ok(entries) = fs::read_dir("/proc") else {
        return Vec::new();
    };

    entries
        .flatten()
        .filter_map(|entry| {
            let pid = entry.file_name().to_str()?.parse::<u32>().ok()?;
            let stat = fs::read_to_string(entry.path().join("stat")).ok()?;
            let (comm, ppid) = parse_stat(&stat)?;

            Some(Proc { pid, ppid, comm })
        })
        .collect()
}

/// `root` plus every descendant of it found in `table`.
pub fn tree_of(table: &[Proc], root: u32) -> Vec<Proc> {
    let mut by_parent: HashMap<u32, Vec<&Proc>> = HashMap::new();

    for process in table {
        by_parent.entry(process.ppid).or_default().push(process);
    }

    let mut result: Vec<Proc> = table
        .iter()
        .filter(|process| process.pid == root)
        .cloned()
        .collect();

    let mut index = 0;

    while index < result.len() {
        let pid = result[index].pid;

        if let Some(children) = by_parent.get(&pid) {
            for child in children {
                if !result.iter().any(|seen| seen.pid == child.pid) {
                    result.push((*child).clone());
                }
            }
        }

        index += 1;
    }

    result
}

/// This process and all its descendants, right now.
pub fn current_tree() -> Vec<Proc> {
    tree_of(&process_table(), std::process::id())
}

/// Memory of one process in KiB: PSS when available, else RSS, else 0.
pub fn process_memory_kb(pid: u32) -> u64 {
    fs::read_to_string(format!("/proc/{pid}/smaps_rollup"))
        .ok()
        .and_then(|text| parse_pss_kb(&text))
        .or_else(|| {
            fs::read_to_string(format!("/proc/{pid}/status"))
                .ok()
                .and_then(|text| parse_rss_kb(&text))
        })
        .unwrap_or(0)
}

/// Memory of the whole browser (main process included), in KiB.
pub fn tree_memory_kb() -> u64 {
    current_tree()
        .iter()
        .map(|process| process_memory_kb(process.pid))
        .sum()
}

/// Whether a process is a WebKit web (renderer) process. `comm` is
/// truncated to 15 characters by the kernel, hence the prefix match.
pub fn is_web_process(comm: &str) -> bool {
    comm.starts_with("WebKitWebProc")
}

/// PIDs of the web processes currently running under this browser.
pub fn web_process_pids() -> Vec<u32> {
    current_tree()
        .into_iter()
        .filter(|process| is_web_process(&process.comm))
        .map(|process| process.pid)
        .collect()
}

/// Whether `pid` still exists and is not a zombie.
pub fn is_alive(pid: u32) -> bool {
    fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()
        .and_then(|stat| {
            let close = stat.rfind(')')?;

            stat[close + 1..]
                .split_whitespace()
                .next()
                .map(|state| state != "Z")
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stat_parsing_survives_odd_command_names() {
        let line = "42 (Web (Content) x) S 7 42 42 0 -1 4194560 100 0 0 0";

        assert_eq!(parse_stat(line), Some(("Web (Content) x".to_string(), 7)));
        assert_eq!(parse_stat("garbage"), None);
    }

    #[test]
    fn pss_and_rss_are_read_in_kib() {
        let smaps = "Rss:  900 kB\nPss:                1234 kB\nPss_Anon: 5 kB\n";
        let status = "Name:\tx\nVmRSS:\t   777 kB\n";

        assert_eq!(parse_pss_kb(smaps), Some(1234));
        assert_eq!(parse_rss_kb(status), Some(777));
        assert_eq!(parse_pss_kb("nothing"), None);
    }

    #[test]
    fn tree_includes_root_and_all_descendants_only() {
        let table = vec![
            Proc {
                pid: 1,
                ppid: 0,
                comm: "init".into(),
            },
            Proc {
                pid: 10,
                ppid: 1,
                comm: "axysbrowser".into(),
            },
            Proc {
                pid: 11,
                ppid: 10,
                comm: "bwrap".into(),
            },
            Proc {
                pid: 12,
                ppid: 11,
                comm: "WebKitWebProces".into(),
            },
            Proc {
                pid: 20,
                ppid: 1,
                comm: "other".into(),
            },
        ];

        let pids: Vec<u32> = tree_of(&table, 10).iter().map(|p| p.pid).collect();

        assert_eq!(pids, vec![10, 11, 12], "main process must be included");
    }

    #[test]
    fn web_process_detection_handles_truncated_comm() {
        assert!(is_web_process("WebKitWebProces"));
        assert!(!is_web_process("WebKitNetworkPr"));
        assert!(!is_web_process("axysbrowser"));
    }

    #[test]
    fn own_process_is_measurable_and_alive() {
        let me = std::process::id();

        assert!(process_memory_kb(me) > 0);
        assert!(is_alive(me));
        assert!(current_tree().iter().any(|process| process.pid == me));
    }
}
