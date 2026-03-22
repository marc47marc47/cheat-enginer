use super::value_type::{ScanType, ScanValue};

pub fn compare(current: &ScanValue, previous: &ScanValue, scan_type: ScanType, target: Option<&ScanValue>) -> bool {
    match scan_type {
        ScanType::ExactValue => {
            if let Some(t) = target {
                current.matches(t)
            } else {
                false
            }
        }
        ScanType::UnknownInitial => true,
        ScanType::Increased => current.greater_than(previous),
        ScanType::Decreased => current.less_than(previous),
        ScanType::Changed => current != previous,
        ScanType::Unchanged => current == previous,
        ScanType::GreaterThan => {
            if let Some(t) = target {
                current.greater_than(t)
            } else {
                false
            }
        }
        ScanType::LessThan => {
            if let Some(t) = target {
                current.less_than(t)
            } else {
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_value() {
        let current = ScanValue::U32(42);
        let prev = ScanValue::U32(10);
        let target = ScanValue::U32(42);
        assert!(compare(&current, &prev, ScanType::ExactValue, Some(&target)));
        assert!(!compare(&prev, &current, ScanType::ExactValue, Some(&target)));
    }

    #[test]
    fn test_increased() {
        let prev = ScanValue::U32(10);
        let current = ScanValue::U32(20);
        assert!(compare(&current, &prev, ScanType::Increased, None));
        assert!(!compare(&prev, &current, ScanType::Increased, None));
    }

    #[test]
    fn test_decreased() {
        let prev = ScanValue::U32(20);
        let current = ScanValue::U32(10);
        assert!(compare(&current, &prev, ScanType::Decreased, None));
    }

    #[test]
    fn test_changed_unchanged() {
        let a = ScanValue::U32(10);
        let b = ScanValue::U32(20);
        assert!(compare(&b, &a, ScanType::Changed, None));
        assert!(!compare(&a, &a, ScanType::Changed, None));
        assert!(compare(&a, &a, ScanType::Unchanged, None));
        assert!(!compare(&b, &a, ScanType::Unchanged, None));
    }
}
