//! The pins of a `.dig` file's parts that touch nothing: where this
//! reader's idea of a part's shape and Digital's differ.
//!
//! `cargo run -p proton-circuit --example loose_pins -- <dir> <file.dig>`

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (dir, file) = (std::path::Path::new(&args[1]), &args[2]);
    match proton_circuit::dig::loose_pins(dir, file) {
        Ok(list) => {
            for l in &list {
                println!("{l}");
            }
            println!("{} loose", list.len());
        }
        Err(e) => eprintln!("{e}"),
    }
}
