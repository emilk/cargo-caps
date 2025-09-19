#![allow(unused_imports, unreachable_code, clippy::unwrap_used)]

use std::hint::black_box;

pub fn test_capabilities() {
    #[cfg(feature = "panic")]
    {
        panic!("Intentional");
    }

    #[cfg(feature = "alloc")]
    {
        black_box(vec![1, 2, 3, 4, 5]);
        black_box(Box::new(42));
    }

    #[cfg(feature = "time")]
    {
        black_box(std::time::SystemTime::now());
        black_box(std::time::Instant::now());
    }

    #[cfg(feature = "signal")]
    {
        // TODO
    }

    #[cfg(feature = "sysinfo")]
    {
        black_box(std::env::var("PATH").unwrap_or_default());
    }

    #[cfg(feature = "stdio")]
    {
        println!("This is stdout output");
        eprintln!("This is stderr output");
        use std::io::{self, Write};
        io::stdout().flush().unwrap();
    }

    #[cfg(feature = "thread")]
    {
        let handle = std::thread::spawn(|| 42);
        handle.join().unwrap();
    }

    #[cfg(feature = "net")]
    {
        use std::net::TcpListener;

        // Try to bind to a local port (this exercises the networking capability)
        if let Ok(listener) = TcpListener::bind("127.0.0.1:0")
            && let Ok(addr) = listener.local_addr()
        {
            black_box(addr);
        }
    }

    #[cfg(feature = "fread")]
    {
        use std::fs;
        use std::io::Read;

        // Try to read from an existing file (like /etc/hosts on Unix systems)
        let read_path = "/etc/hosts";
        if let Ok(mut file) = fs::File::open(read_path) {
            let mut contents = String::new();
            if file.read_to_string(&mut contents).is_ok() {
                black_box(contents);
            }
        }
    }

    #[cfg(feature = "fwrite")]
    {
        use std::fs;
        use std::io::Write;

        // Create and write to a temporary file
        let temp_path = "/tmp/cargo_caps_test_write_file";
        if let Ok(mut file) = fs::File::create(temp_path) {
            let _ = file.write_all(b"Hello, file system!");
            let _ = fs::remove_file(temp_path);
        }
    }

    #[cfg(feature = "two_rands")]
    {
        use rand_07::RngCore as _;
        use rand_08::RngCore as _;

        eprintln!("{}", rand_07::thread_rng().next_u32());
        eprintln!("{}", rand_08::thread_rng().next_u32());
    }
}
