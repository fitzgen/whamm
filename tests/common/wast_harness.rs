use crate::common::{run_whamm, setup_logger, try_path};
use log::{debug, error};
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use wabt::wat2wasm;
const OUTPUT_WHAMMED_WAST: &str = "output/tests/wast_suite/should_pass";
const OUTPUT_UNINSTR_WAST: &str = "output/tests/wast_suite/should_fail";

pub fn main() -> Result<(), std::io::Error> {
    setup_logger();
    // Find all the wast files to run as tests
    let wast_tests = find_wast_tests();

    let mut all_wast_should_pass = vec![];
    let mut all_wast_should_fail = vec![];
    for test in wast_tests {
        let f = File::open(test.clone())?;
        let mut reader = BufReader::new(f);

        // Convention: Only one module per wast!
        let module_wat = get_wasm_module(&mut reader)?;
        if module_wat.is_empty() {
            panic!(
                "Could not find the Wasm module in the wast file: {:?}",
                test.clone()
            );
        }
        let module_wasm = match wat2wasm(module_wat.as_bytes()) {
            Err(e) => {
                panic!(
                    "Unable to convert wat to wasm for module: {}\nDue to error: {:?}",
                    module_wat, e
                );
            }
            Ok(res) => res,
        };

        // Get the `whamm!` scripts and corresponding test cases for this module
        let test_cases = get_test_cases(reader);

        debug!("{module_wat}\n");

        for test_case in test_cases.iter() {
            test_case.print();
        }

        match generate_should_fail_bin_wast(&module_wasm, &test_cases, &test) {
            Err(e) => {
                panic!(
                    "Unable to write UN-instrumented wast file due to error: {:?}",
                    e
                );
            }
            Ok(mut files) => {
                all_wast_should_fail.append(&mut files);
            }
        };

        match generate_instrumented_bin_wast(&module_wasm, &test_cases, &test) {
            Err(e) => {
                panic!(
                    "Unable to write instrumented wast file due to error: {:?}",
                    e
                );
            }
            Ok(mut files) => all_wast_should_pass.append(&mut files),
        };
    }

    // Now that we've generated the wast files, let's run them on the configured interpreters!
    run_wast_tests(all_wast_should_fail, all_wast_should_pass);
    Ok(())
}

fn run_wast_tests(wast_should_fail: Vec<String>, wast_should_pass: Vec<String>) {
    let inters = get_available_interpreters();
    assert!(!inters.is_empty(), "No supported interpreters are configured, fail!\n\
        To fix, add an executable binary under {INT_PATH} for one of the following interpreter options:\n\
        1. the wizeng interpreter, named '{WIZENG_SPEC_INT}'. https://github.com/titzer/wizard-engine/tree/master\n\
        2. the Wasm reference interpreter, named '{WASM_REF_INT}'. https://github.com/WebAssembly/spec/tree/main/interpreter\n");

    println!("\n>>> available interpreters:");
    for (i, inter) in inters.iter().enumerate() {
        println!("{i}. {inter}");
    }
    println!();

    run_wast_tests_that_should_fail(&inters, wast_should_fail);
    run_wast_tests_that_should_pass(&inters, wast_should_pass);
}

/// Run all the wast files that should FAIL on each of the configured interpreters
fn run_wast_tests_that_should_fail(inters: &[String], wast_files: Vec<String>) {
    for inter in inters.iter() {
        for wast in wast_files.iter() {
            let res = run_wast_test(inter, wast);
            if res.status.success() {
                error!("The following command should have FAILED (ran un-instrumented): '{inter} {wast}'");
            }
            assert!(!res.status.success());
        }
    }
}

/// Run all the wast files that should PASS on each of the configured interpreters
fn run_wast_tests_that_should_pass(inters: &[String], wast_files: Vec<String>) {
    for inter in inters.iter() {
        for wast in wast_files.iter() {
            let res = run_wast_test(inter, wast);
            if !res.status.success() {
                error!(
                    "The following command should have PASSED: '{inter} {wast}'\n{}\n{}",
                    String::from_utf8(res.stdout).unwrap(),
                    String::from_utf8(res.stderr).unwrap()
                );
            }
            assert!(res.status.success());
        }
    }
}

fn run_wast_test(inter: &String, wast_file_name: &String) -> Output {
    Command::new(inter)
        .arg(wast_file_name)
        .output()
        .expect("failed to execute process")
}

const INT_PATH: &str = "./output/tests/interpreters";
const WIZENG_SPEC_INT: &str = "spectest.x86-linux";
const WASM_REF_INT: &str = "wasm";
fn get_available_interpreters() -> Vec<String> {
    let supported_interpreters = [WASM_REF_INT, WIZENG_SPEC_INT];
    let mut available_interpreters = Vec::new();

    for interpreter in supported_interpreters.iter() {
        let int_path = format!("{INT_PATH}/{interpreter}");
        match Command::new(&int_path).arg("--help").output() {
            Err(..) => {
                // do nothing
            }
            Ok(res) => {
                if res.status.success() {
                    available_interpreters.push(int_path);
                }
            }
        }
    }

    available_interpreters
}

// ==============================
// ---- WAST FILE GENERATION ----
// ==============================

fn generate_should_fail_bin_wast(
    module_wasm: &Vec<u8>,
    test_cases: &[WastTestCase],
    wast_path: &Path,
) -> Result<Vec<String>, std::io::Error> {
    let mut created_wast_files = vec![];
    for (test_idx, test_case) in test_cases.iter().enumerate() {
        for (assertion_idx, assertion) in test_case.assertions.iter().enumerate() {
            // create the wast
            // call.wast -> call.idx.bin.wast
            let file_name = new_wast_name(wast_path, test_idx, Some(assertion_idx));
            let new_file_path = format!("{OUTPUT_UNINSTR_WAST}/{file_name}");
            try_path(&new_file_path);

            // Write new wast files, one assertion at a time
            write_bin_wast_file(&new_file_path, module_wasm, &[assertion.clone()])?;
            created_wast_files.push(new_file_path);
        }
    }
    Ok(created_wast_files)
}

fn generate_instrumented_bin_wast(
    module_wasm: &[u8],
    test_cases: &[WastTestCase],
    wast_path: &Path,
) -> Result<Vec<String>, std::io::Error> {
    let mut created_wast_files = vec![];
    for (idx, test_case) in test_cases.iter().enumerate() {
        // instrument A COPY OF the module with the whamm script
        // copy, so you don't accidentally manipulate the core module
        // (which is then instrumented in subsequent tests)
        let cloned_module = module_wasm.to_vec();
        let module_to_instrument = cloned_module.as_slice();
        let (instrumented_module_wasm, instrumented_module_wat) = run_whamm(
            module_to_instrument,
            &test_case.whamm_script,
            &format!("{:?}", wast_path),
        );

        debug!("AFTER INSTRUMENTATION");
        debug!("{instrumented_module_wat}");

        // create the wast
        // call.wast -> call.idx.bin.wast
        let file_name = new_wast_name(wast_path, idx, None);
        let new_file_path = format!("{OUTPUT_WHAMMED_WAST}/{file_name}");
        try_path(&new_file_path);

        write_bin_wast_file(
            &new_file_path,
            &instrumented_module_wasm,
            &test_case.assertions,
        )?;
        created_wast_files.push(new_file_path);
    }
    Ok(created_wast_files)
}

fn write_bin_wast_file(
    file_path: &String,
    module_wasm: &Vec<u8>,
    assertions: &[String],
) -> Result<(), std::io::Error> {
    let mut wast_file = File::create(file_path)?;

    // output the module binary with format: (module binary "<binary>")
    wast_file.write_all("(module binary ".as_bytes())?;
    wast_file.write_all(vec_as_hex(module_wasm.as_slice()).as_bytes())?;
    wast_file.write_all(")\n\n".as_bytes())?;

    // output the associated assertions (line by line)
    for assert in assertions.iter() {
        wast_file.write_all(assert.as_bytes())?;
        wast_file.write_all(&[b'\n'])?;
    }
    wast_file.write_all(&[b'\n'])?;
    wast_file
        .flush()
        .expect("Failed to flush out the wast file");

    Ok(())
}

// ==============================
// ---- TEST CASE COLLECTION ----
// ==============================

const WAST_SUITE_DIR: &str = "tests/wast_suite";
const MODULE_PREFIX_PATTERN: &str = "(module";
const ASSERT_PREFIX_PATTERN: &str = "(assert";
const WHAMM_PREFIX_PATTERN: &str = ";; WHAMM --> ";

/// Recursively finds all tests in a specified directory
fn find_wast_tests() -> Vec<PathBuf> {
    let mut wast_tests = Vec::new();
    let suite_path = Path::new(WAST_SUITE_DIR);

    find_tests(suite_path, &mut wast_tests);
    fn find_tests(path: &Path, tests: &mut Vec<PathBuf>) {
        for f in path.read_dir().unwrap() {
            let f = f.unwrap();
            if f.file_type().unwrap().is_dir() {
                find_tests(&f.path(), tests);
                continue;
            }

            match f.path().extension().and_then(|s| s.to_str()) {
                Some("wast") => {} // found a test!
                Some("wasm") => panic!(
                    "use `*.wat` or `*.wast` instead of binaries: {:?}",
                    f.path()
                ),
                _ => continue,
            }
            tests.push(f.path());
        }
    }

    wast_tests
}

/// Parses the wasm module from the wast file passed as a buffer.
fn get_wasm_module(reader: &mut BufReader<File>) -> Result<String, std::io::Error> {
    let mut module = "".to_string();
    let mut num_left_parens = 0;
    let mut num_right_parens = 0;
    let mut is_module = false;

    let mut line = String::new();
    while reader.read_line(&mut line)? > 0 {
        if line.starts_with(MODULE_PREFIX_PATTERN) {
            // this is the beginning of the module
            is_module = true;
        }

        if is_module {
            // Add the line to the module string
            module += &line;

            // count the number of left/right parens (to know when finished parsing module)
            num_left_parens += count_matched_chars(&line, &'(');
            num_right_parens += count_matched_chars(&line, &')');

            if num_left_parens == num_right_parens {
                // we're done parsing the module!
                break;
            }
            fn count_matched_chars(s: &str, c: &char) -> usize {
                s.chars().filter(|ch| *ch == *c).count()
            }
        }
        line.clear();
    }

    Ok(module)
}

/// Holds a single test case encoded in the wast.
#[derive(Default)]
struct WastTestCase {
    whamm_script: String,
    assertions: Vec<String>,
}
impl WastTestCase {
    fn print(&self) {
        debug!(">>> TEST CASE <<<");
        debug!("{}", self.whamm_script);

        for assertion in &self.assertions {
            debug!("{assertion}");
        }
    }
}

/// Creates a vector of test cases from the passed buffer.
/// Convention: `whamm!` scripts are in comments beginning with "WHAMM --> "
/// Convention: All test cases under a `whamm!` script should be run on the same instrumented module.
fn get_test_cases(reader: BufReader<File>) -> Vec<WastTestCase> {
    let mut test_cases = Vec::new();

    let mut first = true;
    let mut matched = false;
    let mut curr_test = WastTestCase::default();
    for line in reader.lines().map_while(Result::ok) {
        if let Some(whamm) = line.strip_prefix(WHAMM_PREFIX_PATTERN) {
            if !first {
                test_cases.push(curr_test);
                // this is the start of a new test case
                curr_test = WastTestCase::default();
            }
            first = false;
            matched = true;
            curr_test.whamm_script = whamm.to_string();
        } else if line.starts_with(MODULE_PREFIX_PATTERN) {
            panic!("Only one module per wast file!!")
        } else if line.starts_with(ASSERT_PREFIX_PATTERN) {
            // this is an assertion within the current test case
            curr_test.assertions.push(line);
        }
    }
    if matched {
        // Make sure all tests are added!
        test_cases.push(curr_test);
    }

    test_cases
}

// ===================
// ---- UTILITIES ----
// ===================

fn new_wast_name(wast_path: &Path, idx: usize, idx2: Option<usize>) -> String {
    let file_name = wast_path.file_name().unwrap().to_str().unwrap().to_string();
    let file_ext = wast_path.extension().unwrap().to_str().unwrap();
    let file_name_stripped = file_name.strip_suffix(file_ext).unwrap();
    if let Some(idx2) = idx2 {
        format!("{file_name_stripped}whamm{idx}.assertion{idx2}.bin.wast")
    } else {
        format!("{file_name_stripped}whamm{idx}.bin.wast")
    }
}

/// Creates a String representing the &[u8] in hex format.
fn vec_as_hex(vec: &[u8]) -> String {
    // opening quote
    let mut res = "\"".to_string();

    // Iterate through each byte in the vector
    for &byte in vec {
        // Add each byte as a two-digit hexadecimal number with leading '\'
        res += format!("\\{:02x}", byte).as_str();
    }

    // closing quote
    res += "\"";
    res
}
