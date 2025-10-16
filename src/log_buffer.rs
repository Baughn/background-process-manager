use regex::Regex;
use std::collections::VecDeque;

const MAX_LOG_INSTANCES: usize = 10;
const MAX_LINES_PER_INSTANCE: usize = 10000;

#[derive(Debug, Clone)]
pub struct LogInstance {
    pub lines: VecDeque<String>,
}

impl LogInstance {
    pub fn new() -> Self {
        Self {
            lines: VecDeque::with_capacity(MAX_LINES_PER_INSTANCE),
        }
    }

    pub fn append(&mut self, line: String) {
        if self.lines.len() >= MAX_LINES_PER_INSTANCE {
            self.lines.pop_front();
        }
        self.lines.push_back(line);
    }

    pub fn search(
        &self,
        pattern: Option<&str>,
        context_lines: Option<usize>,
        head: Option<usize>,
        tail: Option<usize>,
    ) -> Vec<String> {
        let mut result = Vec::new();
        let context = context_lines.unwrap_or(0);

        if let Some(pattern) = pattern {
            // Regex search with context
            let re = match Regex::new(pattern) {
                Ok(re) => re,
                Err(_) => return vec![format!("Invalid regex pattern: {}", pattern)],
            };

            let lines: Vec<_> = self.lines.iter().collect();
            let mut matched_indices = Vec::new();

            // Find all matching lines
            for (i, line) in lines.iter().enumerate() {
                if re.is_match(line) {
                    matched_indices.push(i);
                }
            }

            if matched_indices.is_empty() {
                return vec!["No matches found".to_string()];
            }

            // Expand to include context
            let mut included = vec![false; lines.len()];
            for &idx in &matched_indices {
                let start = idx.saturating_sub(context);
                let end = (idx + context + 1).min(lines.len());
                for item in included.iter_mut().take(end).skip(start) {
                    *item = true;
                }
            }

            // Collect lines with context
            for (i, line) in lines.iter().enumerate() {
                if included[i] {
                    let marker = if matched_indices.contains(&i) { " * " } else { "   " };
                    result.push(format!("{}{}", marker, line));
                }
            }
        } else {
            // No pattern, just return all lines
            for line in &self.lines {
                result.push(line.clone());
            }
        }

        // Apply head/tail limiting
        if let Some(n) = head {
            result.truncate(n);
        } else if let Some(n) = tail {
            let start = result.len().saturating_sub(n);
            result = result[start..].to_vec();
        }

        if result.is_empty() {
            result.push("(empty)".to_string());
        }

        result
    }
}

#[derive(Debug)]
pub struct LogBuffer {
    instances: VecDeque<LogInstance>,
}

impl LogBuffer {
    pub fn new() -> Self {
        Self {
            instances: VecDeque::with_capacity(MAX_LOG_INSTANCES),
        }
    }

    pub fn new_instance(&mut self) {
        if self.instances.len() >= MAX_LOG_INSTANCES {
            self.instances.pop_front();
        }
        self.instances.push_back(LogInstance::new());
    }

    pub fn append(&mut self, line: String) {
        if self.instances.is_empty() {
            self.new_instance();
        }
        if let Some(current) = self.instances.back_mut() {
            current.append(line);
        }
    }

    pub fn get_instance(&self, index: Option<i32>) -> Option<&LogInstance> {
        let idx = index.unwrap_or(-1);
        if self.instances.is_empty() {
            return None;
        }

        if idx < 0 {
            // Python-style negative indexing: -1 = last, -2 = second-to-last, etc.
            let pos = idx.unsigned_abs() as usize - 1;
            if pos < self.instances.len() {
                Some(&self.instances[self.instances.len() - 1 - pos])
            } else {
                None
            }
        } else {
            // Positive indexing: 0 = first, 1 = second, etc.
            self.instances.get(idx as usize)
        }
    }

    pub fn search(
        &self,
        index: Option<i32>,
        pattern: Option<&str>,
        context_lines: Option<usize>,
        head: Option<usize>,
        tail: Option<usize>,
    ) -> Vec<String> {
        match self.get_instance(index) {
            Some(instance) => instance.search(pattern, context_lines, head, tail),
            None => vec![format!(
                "Log instance {} not found (have {} instances)",
                index.unwrap_or(-1),
                self.instances.len()
            )],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_buffer_indexing() {
        let mut buffer = LogBuffer::new();

        // Create first instance
        buffer.new_instance();
        buffer.append("first-1".to_string());

        // Create second instance
        buffer.new_instance();
        buffer.append("second-1".to_string());

        // Create third instance
        buffer.new_instance();
        buffer.append("third-1".to_string());

        // Test negative indexing (Python-style)
        assert_eq!(buffer.get_instance(Some(-1)).unwrap().lines[0], "third-1");
        assert_eq!(buffer.get_instance(Some(-2)).unwrap().lines[0], "second-1");
        assert_eq!(buffer.get_instance(Some(-3)).unwrap().lines[0], "first-1");

        // Test positive indexing
        assert_eq!(buffer.get_instance(Some(0)).unwrap().lines[0], "first-1");
        assert_eq!(buffer.get_instance(Some(1)).unwrap().lines[0], "second-1");
        assert_eq!(buffer.get_instance(Some(2)).unwrap().lines[0], "third-1");

        // Test default (should be -1, most recent)
        assert_eq!(buffer.get_instance(None).unwrap().lines[0], "third-1");
    }
}
