// Args:
// dir: .

use std::fs;
use std::io::Read;

fn main() {
    let mut this_file = fs::File::open("wasitests/quine.rs").expect("could not find src file");
    let md = this_file.metadata().unwrap();
    let mut in_str = String::new();
    this_file.read_to_string(&mut in_str).unwrap();
    println!("{}", in_str);
}
