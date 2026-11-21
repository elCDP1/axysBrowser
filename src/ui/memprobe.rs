//! Memory accounting for this process and its renderer descendants.
//!
//! Used by the `AXYS_MEM_SMOKE` self-test (see `ui::memsmoke`), the resources
//! page and CI. Each platform answers the same questions with its own
//! facility, and the process tree is assembled the same way on both: Linux
//! walks `/proc` (PSS from `smaps_rollup` is preferred because WebKit's
//! processes share a lot of memory and RSS would count it once per process;
//! when `smaps_rollup` is unreadable the resident size from `status` stands
//! in), Windows asks the kernel for each process's working set. Both sum the
//! main process plus every descendant, sandbox helpers included.

use std::collections::HashMap;

/// One row of the process table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Proc {
    pub pid: u32,
    pub ppid: u32,
    pub comm: String,
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

/// Memory of the whole browser (main process included), in KiB.
pub fn tree_memory_kb() -> u64 {
    current_tree()
        .iter()
        .map(|process| process_memory_kb(process.pid))
        .sum()
}

/// Memory in KiB and the PIDs of the running web (renderer) processes,
/// both from a single process-table scan. The resources page and the memory
/// self-test need the two together; calling [`tree_memory_kb`] and
/// [`web_process_pids`] back to back walked the whole table twice.
pub fn tree_snapshot() -> (u64, Vec<u32>) {
    let tree = current_tree();

    let memory_kb = tree
        .iter()
        .map(|process| process_memory_kb(process.pid))
        .sum();

    let web_pids = tree
        .iter()
        .filter(|process| is_web_process(&process.comm))
        .map(|process| process.pid)
        .collect();

    (memory_kb, web_pids)
}

/// PIDs of the web processes currently running under this browser.
pub fn web_process_pids() -> Vec<u32> {
    current_tree()
        .into_iter()
        .filter(|process| is_web_process(&process.comm))
        .map(|process| process.pid)
        .collect()
}

// Linux: `/proc` is the process table and `smaps_rollup`/`status` the sizes.

/// Parses `/proc/<pid>/stat` into (comm, ppid). The command name may
/// contain spaces and parentheses, so fields start after the LAST ')'.
#[cfg(target_os = "linux")]
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
#[cfg(target_os = "linux")]
fn kb_field(text: &str, key: &str) -> Option<u64> {
    text.lines()
        .find_map(|line| line.strip_prefix(key)?.strip_prefix(':'))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|value| value.parse().ok())
}

/// PSS in KiB from `smaps_rollup` text.
#[cfg(target_os = "linux")]
pub fn parse_pss_kb(smaps_rollup: &str) -> Option<u64> {
    kb_field(smaps_rollup, "Pss")
}

/// Resident size in KiB from `status` text.
#[cfg(target_os = "linux")]
pub fn parse_rss_kb(status: &str) -> Option<u64> {
    kb_field(status, "VmRSS")
}

#[cfg(target_os = "linux")]
fn process_table() -> Vec<Proc> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };

    entries
        .flatten()
        .filter_map(|entry| {
            let pid = entry.file_name().to_str()?.parse::<u32>().ok()?;
            let stat = std::fs::read_to_string(entry.path().join("stat")).ok()?;
            let (comm, ppid) = parse_stat(&stat)?;

            Some(Proc { pid, ppid, comm })
        })
        .collect()
}

/// Memory of one process in KiB: PSS when available, else RSS, else 0.
#[cfg(target_os = "linux")]
pub fn process_memory_kb(pid: u32) -> u64 {
    std::fs::read_to_string(format!("/proc/{pid}/smaps_rollup"))
        .ok()
        .and_then(|text| parse_pss_kb(&text))
        .or_else(|| {
            std::fs::read_to_string(format!("/proc/{pid}/status"))
                .ok()
                .and_then(|text| parse_rss_kb(&text))
        })
        .unwrap_or(0)
}

/// Whether a process is a WebKit web (renderer) process. `comm` is
/// truncated to 15 characters by the kernel, hence the prefix match.
#[cfg(target_os = "linux")]
pub fn is_web_process(comm: &str) -> bool {
    comm.starts_with("WebKitWebProc")
}

/// Whether `pid` still exists and is not a zombie.
#[cfg(target_os = "linux")]
pub fn is_alive(pid: u32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
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

// Windows: Toolhelp for the process table, the kernel for each working set.

#[cfg(windows)]
mod win {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ,
    };

    /// An open process handle, closed on drop.
    pub struct Process(HANDLE);

    impl Process {
        /// Opens `pid` for a query. Reading the working set needs `VM_READ`,
        /// which a process we only parent may refuse, so that bit is optional.
        pub fn open(pid: u32) -> Option<Self> {
            // SAFETY: no pointers, and the handle is either valid or zero.
            let wide =
                unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, 0, pid) };

            if wide != 0 {
                return Some(Self(wide));
            }

            // SAFETY: as above.
            let narrow = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };

            (narrow != 0).then_some(Self(narrow))
        }

        /// Working set in KiB, or 0 when the query is refused.
        pub fn working_set_kb(&self) -> u64 {
            // SAFETY: the counters struct is plain data, its `cb` is set to
            // its own size as the API requires, and the handle is live.
            let mut counters: PROCESS_MEMORY_COUNTERS = unsafe { std::mem::zeroed() };

            counters.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;

            // SAFETY: the handle is live and `counters` outlives the call.
            let ok = unsafe { GetProcessMemoryInfo(self.0, &mut counters, counters.cb) } != 0;

            if ok {
                (counters.WorkingSetSize / 1024) as u64
            } else {
                0
            }
        }
    }

    impl Drop for Process {
        fn drop(&mut self) {
            // SAFETY: this struct owns the handle and drops once.
            unsafe { CloseHandle(self.0) };
        }
    }

    /// Every process on the machine, as `(pid, parent pid, image name)`.
    pub fn table() -> Vec<(u32, u32, String)> {
        let mut rows = Vec::new();

        // SAFETY: the snapshot handle is checked and closed on every path;
        // `entry` is a plain struct the API fills in, sized beforehand.
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);

            if snapshot == 0 || snapshot == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
                return rows;
            }

            let mut entry: PROCESSENTRY32W = std::mem::zeroed();

            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

            let mut more = Process32FirstW(snapshot, &mut entry) != 0;

            while more {
                let name = &entry.szExeFile;
                let len = name
                    .iter()
                    .position(|unit| *unit == 0)
                    .unwrap_or(name.len());

                rows.push((
                    entry.th32ProcessID,
                    entry.th32ParentProcessID,
                    String::from_utf16_lossy(&name[..len]),
                ));

                more = Process32NextW(snapshot, &mut entry) != 0;
            }

            CloseHandle(snapshot);
        }

        rows
    }
}

#[cfg(windows)]
fn process_table() -> Vec<Proc> {
    win::table()
        .into_iter()
        .map(|(pid, ppid, comm)| Proc { pid, ppid, comm })
        .collect()
}

#[cfg(windows)]
pub fn process_memory_kb(pid: u32) -> u64 {
    win::Process::open(pid).map_or(0, |process| process.working_set_kb())
}

#[cfg(windows)]
pub fn is_alive(pid: u32) -> bool {
    // Windows has no zombies: a process that can be opened still exists.
    win::Process::open(pid).is_some()
}

/// Whether a process is a WebKit web (renderer) process. Servo renders in
/// this process, so on Windows nothing in the tree is one.
#[cfg(windows)]
pub fn is_web_process(_comm: &str) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn stat_parsing_survives_odd_command_names() {
        let line = "42 (Web (Content) x) S 7 42 42 0 -1 4194560 100 0 0 0";

        assert_eq!(parse_stat(line), Some(("Web (Content) x".to_string(), 7)));
        assert_eq!(parse_stat("garbage"), None);
    }

    #[cfg(target_os = "linux")]
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

    #[cfg(target_os = "linux")]
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

    #[test]
    fn snapshot_combines_memory_and_web_pids() {
        let (memory_kb, web_pids) = tree_snapshot();

        assert!(memory_kb > 0, "the browser itself must be counted");
        assert!(
            web_pids.iter().all(|pid| is_alive(*pid)),
            "reported web processes must be live at snapshot time"
        );
    }
}
