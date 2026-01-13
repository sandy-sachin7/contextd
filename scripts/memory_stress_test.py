#!/usr/bin/env python3
"""
Memory Stress Test for contextd

Generates a large test dataset (10K files) and monitors memory usage
during indexing and repeated queries.

Requirements:
- psutil: pip install psutil
"""

import os
import subprocess
import sys
import time
import tempfile
import shutil
import signal
from pathlib import Path

try:
    import psutil
except ImportError:
    print("ERROR: psutil required. Install with: pip install psutil")
    sys.exit(1)

# Configuration
NUM_FILES = 10000
QUERIES_TO_RUN = 1000
MEMORY_LIMIT_MB = 500  # Expected max memory usage

def generate_test_data(base_dir: Path, num_files: int):
    """Generate test files with realistic code content"""
    print(f"Generating {num_files} test files in {base_dir}...")

    templates = [
        # Rust template
        """
use std::collections::HashMap;

pub struct TestStruct_{} {{
    data: HashMap<String, i32>,
    count: usize,
}}

impl TestStruct_{} {{
    pub fn new() -> Self {{
        Self {{
            data: HashMap::new(),
            count: 0,
        }}
    }}

    pub fn process(&mut self, key: String, value: i32) {{
        self.data.insert(key, value);
        self.count += 1;
    }}
}}

#[cfg(test)]
mod tests {{
    use super::*;

    #[test]
    fn test_creation() {{
        let ts = TestStruct_{}::new();
        assert_eq!(ts.count, 0);
    }}
}}
""",
        # Python template
        """
class TestClass_{}:
    def __init__(self):
        self.data = {{}}
        self.count = 0

    def process(self, key, value):
        self.data[key] = value
        self.count += 1

    def get_stats(self):
        return {{
            'count': self.count,
            'keys': len(self.data)
        }}

def main():
    obj = TestClass_{}()
    for i in range(100):
        obj.process(f'key_{{i}}', i * 2)
    print(f'Processed {{obj.count}} items')

if __name__ == '__main__':
    main()
""",
        # Markdown template
        """
# Test Document {}

## Overview

This is test document number {}. It contains sample content for testing
the indexing and search capabilities of contextd.

## Features

- Feature A: Lorem ipsum dolor sit amet
- Feature B: Consectetur adipiscing elit
- Feature C: Sed do eiusmod tempor incididunt

## Implementation

The implementation follows best practices:

1. First step involves initialization
2. Second step processes the data
3. Third step validates results

## Code Example

```rust
fn example_{}() {{
    let data = vec![1, 2, 3, 4, 5];
    let sum: i32 = data.iter().sum();
    println!("Sum: {{}}", sum);
}}
```

## Conclusion

Document {} demonstrates the capabilities of the system.
"""
    ]

    extensions = ['.rs', '.py', '.md']

    for i in range(num_files):
        template_idx = i % len(templates)
        ext = extensions[template_idx]

        # Create subdirectories for organization
        subdir = base_dir / f"category_{i // 100}"
        subdir.mkdir(exist_ok=True)

        file_path = subdir / f"test_{i}{ext}"
        content = templates[template_idx].format(i, i, i, i, i)

        file_path.write_text(content)

        if (i + 1) % 1000 == 0:
            print(f"  Generated {i + 1}/{num_files} files...")

    print(f"✓ Generated {num_files} files")

def get_memory_usage(pid: int) -> float:
    """Get RSS memory usage in MB"""
    try:
        process = psutil.Process(pid)
        return process.memory_info().rss / (1024 * 1024)  # Convert to MB
    except (psutil.NoSuchProcess, psutil.AccessDenied):
        return 0.0

def monitor_daemon(pid: int, duration_seconds: int, label: str):
    """Monitor memory usage of a process"""
    print(f"\\nMonitoring {label} for {duration_seconds}s...")

    start_time = time.time()
    peak_memory = 0
    samples = []

    while time.time() - start_time < duration_seconds:
        mem_mb = get_memory_usage(pid)
        if mem_mb > 0:
            peak_memory = max(peak_memory, mem_mb)
            samples.append(mem_mb)
        time.sleep(0.5)

    if samples:
        avg_memory = sum(samples) / len(samples)
        print(f"  Average memory: {avg_memory:.2f} MB")
        print(f"  Peak memory: {peak_memory:.2f} MB")
        print(f"  Samples: {len(samples)}")
        return peak_memory
    else:
        print("  WARNING: Could not read memory")
        return 0

def main():
    print("=" * 60)
    print("Contextd Memory Stress Test")
    print("=" * 60)

    # Create temporary directory
    test_dir = Path(tempfile.mkdtemp(prefix="contextd_stress_"))
    print(f"\\nTest directory: {test_dir}")

    try:
        # Generate test data
        data_dir = test_dir / "data"
        data_dir.mkdir()
        generate_test_data(data_dir, NUM_FILES)

        # Create config
        config_content = f"""
[server]
host = "127.0.0.1"
port = 15030

[storage]
db_path = "{test_dir / 'test.db'}"
model_path = "models"

[watch]
paths = ["{data_dir}"]
debounce_ms = 200

[search]
enable_cache = true

[chunking]
max_chunk_size = 512
"""
        config_path = test_dir / "config.toml"
        config_path.write_text(config_content)

        # Start daemon
        print(f"\nStarting daemon...")
        daemon = subprocess.Popen(
            ["./target/release/contextd", "--config", str(config_path), "daemon"],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE
        )

        pid = daemon.pid
        print(f"  Daemon PID: {pid}")

        # Wait for startup
        time.sleep(3)

        # Check if daemon is still running
        if daemon.poll() is not None:
            print("ERROR: Daemon failed to start")
            stdout, stderr = daemon.communicate()
            print(f"STDOUT: {stdout.decode()}")
            print(f"STDERR: {stderr.decode()}")
            return 1

        # Monitor during indexing (first 30 seconds)
        indexing_peak = monitor_daemon(pid, 30, "Initial Indexing")

        # Wait for indexing to complete
        print("\\nWaiting for indexing to settle...")
        time.sleep(10)

        # Run repeated queries and monitor memory
        print(f"\\nRunning {QUERIES_TO_RUN} queries...")
        query_start_mem = get_memory_usage(pid)

        for i in range(QUERIES_TO_RUN):
            subprocess.run(
                ["./target/release/contextd", "query", "test function class"],
                capture_output=True,
                text=True,
                timeout=5
            )

            if (i + 1) % 100 == 0:
                current_mem = get_memory_usage(pid)
                print(f"  Queries: {i + 1}/{QUERIES_TO_RUN}, Memory: {current_mem:.2f} MB")

        query_end_mem = get_memory_usage(pid)
        memory_growth = query_end_mem - query_start_mem

        # Results
        print("\\n" + "=" * 60)
        print("RESULTS")
        print("=" * 60)
        print(f"Files indexed: {NUM_FILES}")
        print(f"Queries executed: {QUERIES_TO_RUN}")
        print(f"\\nMemory Usage:")
        print(f"  Peak during indexing: {indexing_peak:.2f} MB")
        print(f"  Before queries: {query_start_mem:.2f} MB")
        print(f"  After queries: {query_end_mem:.2f} MB")
        print(f"  Memory growth: {memory_growth:+.2f} MB")

        # Assess results
        print(f"\\nAssessment:")
        if indexing_peak > MEMORY_LIMIT_MB:
            print(f"  ⚠  Peak memory ({indexing_peak:.2f} MB) exceeds limit ({MEMORY_LIMIT_MB} MB)")
        else:
            print(f"  ✓ Peak memory ({indexing_peak:.2f} MB) within limit ({MEMORY_LIMIT_MB} MB)")

        if memory_growth > 50:
            print(f"  ⚠  Significant memory growth during queries: {memory_growth:.2f} MB")
            print(f"     Possible memory leak!")
        elif memory_growth > 10:
            print(f"  � Memory growth during queries: {memory_growth:.2f} MB (moderate)")
        else:
            print(f"  ✓ Minimal memory growth during queries: {memory_growth:.2f} MB")

        # Cleanup
        daemon.send_signal(signal.SIGTERM)
        daemon.wait(timeout=5)

        print("\\n" + "=" * 60)
        print("Test Complete")
        print("=" * 60)

        return 0

    except Exception as e:
        print(f"\\nERROR: {e}")
        import traceback
        traceback.print_exc()
        return 1

    finally:
        # Clean up temp directory
        try:
            if daemon.poll() is None:
                daemon.kill()
                daemon.wait()
        except:
            pass

        print(f"\\nCleaning up {test_dir}...")
        shutil.rmtree(test_dir, ignore_errors=True)

if __name__ == "__main__":
    sys.exit(main())
