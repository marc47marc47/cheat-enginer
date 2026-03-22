pub use crate::platform::{ProcessHandle, ProcessInfo};

/// Filter processes: name or window title must contain ALL query characters (any order, any position, case-insensitive)
pub fn filter_processes(processes: &[ProcessInfo], query: &str) -> Vec<ProcessInfo> {
    if query.is_empty() {
        return processes.to_vec();
    }
    let query_chars: Vec<char> = query.to_lowercase().chars().collect();
    processes
        .iter()
        .filter(|p| {
            let name_lower = p.name.to_lowercase();
            let title_lower = p.window_title.as_deref().unwrap_or("").to_lowercase();
            let combined = format!("{name_lower} {title_lower}");
            query_chars.iter().all(|c| combined.contains(*c))
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fuzzy_filter() {
        let procs = vec![
            ProcessInfo { pid: 1, name: "chrome.exe".into(), window_title: Some("Google".into()) },
            ProcessInfo { pid: 2, name: "notepad.exe".into(), window_title: Some("Untitled".into()) },
            ProcessInfo { pid: 3, name: "explorer.exe".into(), window_title: None },
        ];

        // "ce" matches "chrome.exe" (has c and e) and "notepad.exe" (has e but no c... wait)
        let r = filter_processes(&procs, "cr");
        assert!(r.iter().any(|p| p.name == "chrome.exe"));
        assert!(!r.iter().any(|p| p.name == "notepad.exe"));

        // "xp" matches "explorer.exe"
        let r = filter_processes(&procs, "xp");
        assert!(r.iter().any(|p| p.name == "explorer.exe"));

        // empty query returns all
        let r = filter_processes(&procs, "");
        assert_eq!(r.len(), 3);
    }
}
