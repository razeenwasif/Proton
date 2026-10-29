//! A folder of Digital `.dig` files, every test case in each run.
//! Ignored by default: it needs the folder, named by
//! `PROTON_CIRCUIT_DIG_DIR`, e.g. a course's CPU.
//!
//! ```text
//! PROTON_CIRCUIT_DIG_DIR=~/Courses/…/src cargo test -p proton-circuit --test digital_files -- --ignored --nocapture
//! ```

#[test]
#[ignore = "needs PROTON_CIRCUIT_DIG_DIR, a folder of .dig files"]
fn every_test_case_in_a_folder_of_dig_files_passes() {
    let Ok(dir) = std::env::var("PROTON_CIRCUIT_DIG_DIR") else { return };
    let dir = std::path::PathBuf::from(dir);
    let mut files: Vec<String> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.ends_with(".dig")).collect();
    files.sort();
    let (mut passed, mut failed) = (0, 0);
    for f in files {
        let started = std::time::Instant::now();
        let built = match proton_circuit::dig::build(&dir, &f) {
            Ok(b) => b,
            Err(e) => {
                println!("{f}: not built: {e}");
                failed += 1;
                continue;
            }
        };
        if built.tests.is_empty() {
            continue;
        }
        for o in proton_circuit::testlang::run_all(&built) {
            let label = if o.label.is_empty() { "(unnamed)".to_string() } else { o.label.clone() };
            if o.passed() {
                passed += 1;
                println!("PASS {f} {label}: {} rows", o.rows);
            } else {
                failed += 1;
                println!("FAIL {f} {label}: {} rows; {}", o.rows, o.error.clone().unwrap_or_default());
                for line in o.failures.iter().take(3) {
                    println!("     {line}");
                }
            }
        }
        println!("     ({f}: {} parts, {} nets, {:?})", built.netlist.parts(), built.netlist.nets(), started.elapsed());
    }
    println!("{passed} passed, {failed} failed");
    assert_eq!(failed, 0);
}

/// How fast a CPU's clock goes in the simulator: the basic and extended
/// CPUs of the folder, each clocked for two seconds with its first test
/// case's program loaded.
#[test]
#[ignore = "needs PROTON_CIRCUIT_DIG_DIR, a folder with basic-cpu.dig"]
fn a_cpu_clocked_as_fast_as_it_goes() {
    let Ok(dir) = std::env::var("PROTON_CIRCUIT_DIG_DIR") else { return };
    let dir = std::path::PathBuf::from(dir);
    for file in ["basic-cpu.dig", "extended-cpu.dig"] {
        let Ok(built) = proton_circuit::dig::build(&dir, file) else { continue };
        let Some(&(clock, _)) = built.inputs.get("CLK") else { continue };
        let mut sim = proton_circuit::Sim::new(built.netlist.clone(), 0x5eed).unwrap();
        // FIB's program, if the file has it: a loop to keep the CPU busy.
        if let (Some(ram), Some((_, text))) = (built.program, built.tests.iter().find(|(l, _)| l == "FIB")) {
            if let Some(start) = text.find("program(") {
                let words: Vec<u64> = text[start + 8..].split(')').next().unwrap_or("").split(',').filter_map(|w| u64::from_str_radix(w.trim().trim_start_matches("0x"), 16).ok()).collect();
                sim.load(ram, &words).unwrap();
            }
        }
        let started = std::time::Instant::now();
        let mut cycles = 0u64;
        while started.elapsed() < std::time::Duration::from_secs(2) {
            for _ in 0..100 {
                sim.tick(clock).unwrap();
            }
            cycles += 100;
        }
        let hz = cycles as f64 / started.elapsed().as_secs_f64();
        println!("{file}: {} parts, {cycles} cycles, {:.0} Hz", built.netlist.parts(), hz);
    }
}
