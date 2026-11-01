use r6k_paths::{candidate_dirs, find_replay_dir, inspect};
use std::path::Path;

fn main() {
    for d in candidate_dirs() {
        println!("{:?}  {}", inspect(&d), d.display());
    }
    println!("found: {:?}", find_replay_dir());
    if let Some(p) = std::env::args().nth(1) {
        println!("{:?}  {}", inspect(Path::new(&p)), p);
    }
}
